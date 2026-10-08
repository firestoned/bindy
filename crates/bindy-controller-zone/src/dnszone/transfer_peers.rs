// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Keeping a zone's transfer peers in step with its pods (ADR-0019).
//!
//! A primary's `allow-transfer` / `also-notify` and a secondary's `primaries`
//! name pods (and Services) by IP. Pods are replaced with new IPs; these
//! helpers compute the peers a zone should carry now, push them to existing
//! zones, and report what could not be pushed.

use crate::crd::{InstanceReference, ZoneTransferPeers};
use anyhow::Result;
use bindy_bind9::bind9::zone_ops::PeerUpdate;
use bindy_bind9::instances::InstanceResolver;
use bindy_bind9::peers::{desired_transfer_peers, service_cluster_ip};
use k8s_openapi::api::core::v1::Service;
use kube::{Api, Client};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// `Degraded` reason: a secondary is configured for the zone but holds no
/// data (its transfer is pending or denied).
pub const REASON_SECONDARY_NOT_LOADED: &str = "SecondaryNotLoaded";

/// `Degraded` reason: a primary's `allow-transfer` / `also-notify` could not
/// be rewritten to the zone's current secondaries.
pub const REASON_PEERS_NOT_UPDATED: &str = "TransferPeersNotUpdated";

/// `Degraded` reason: the zone has secondaries but no primary pod admitted by
/// the zones-loaded gate to transfer from.
pub const REASON_NO_TRANSFER_SOURCE: &str = "NoTransferSource";

/// The Service port name of the bindcar API.
const BINDCAR_PORT_NAME: &str = "http";

/// What one reconcile managed to push of the zone's peers.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PeerPushResult {
    /// Primary endpoints whose `allow-transfer` / `also-notify` could not be
    /// rewritten, one human-readable entry each.
    pub primary_failures: Vec<String>,
    /// Secondary endpoints that failed to take the zone (or its replacement).
    pub secondary_failures: usize,
    /// The secondaries were not configured at all for want of a transfer
    /// source.
    pub secondaries_skipped: bool,
}

impl PeerPushResult {
    /// Whether every server now names the desired peers, so they can be
    /// recorded in `status.transferPeers`.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.primary_failures.is_empty()
            && self.secondary_failures == 0
            && !self.secondaries_skipped
    }
}

/// Set `Degraded` (`SecondaryNotLoaded`) when a secondary is configured for
/// the zone but not loaded there (in-memory only).
///
/// # Arguments
/// * `status_updater` - The zone's status updater
/// * `zone_name` - The zone's DNS name
/// * `not_loaded` - One entry per secondary endpoint, naming the instance and
///   the endpoint; nothing is set when empty
pub fn mark_secondaries_not_loaded(
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    zone_name: &str,
    not_loaded: &[String],
) {
    if not_loaded.is_empty() {
        return;
    }
    status_updater.set_condition(
        "Degraded",
        "True",
        REASON_SECONDARY_NOT_LOADED,
        &format!(
            "Zone {zone_name} is configured but not loaded on secondary {}: its transfer from the primaries is pending or denied",
            not_loaded.join(", ")
        ),
    );
}

/// Set `Degraded` (`TransferPeersNotUpdated`) when a primary's transfer peers
/// could not be rewritten (in-memory only).
///
/// # Arguments
/// * `status_updater` - The zone's status updater
/// * `zone_name` - The zone's DNS name
/// * `failures` - One entry per primary endpoint; nothing is set when empty
pub fn mark_peer_refresh_failed(
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    zone_name: &str,
    failures: &[String],
) {
    if failures.is_empty() {
        return;
    }
    status_updater.set_condition(
        "Degraded",
        "True",
        REASON_PEERS_NOT_UPDATED,
        &format!(
            "Zone {zone_name}: allow-transfer/also-notify not updated on {}",
            failures.join("; ")
        ),
    );
}

/// Set `Degraded` (`NoTransferSource`) for a zone with secondaries and no
/// admitted primary pod to transfer from (in-memory only).
///
/// # Arguments
/// * `status_updater` - The zone's status updater
/// * `zone_name` - The zone's DNS name
pub fn mark_no_transfer_source(
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    zone_name: &str,
) {
    status_updater.set_condition(
        "Degraded",
        "True",
        REASON_NO_TRANSFER_SOURCE,
        &format!(
            "Zone {zone_name} has secondaries but no primary pod with its zones loaded to transfer from yet"
        ),
    );
}

/// The NOTIFY targets of a zone's secondaries: each secondary instance's
/// Service ClusterIP (ADR-0019). A missing or headless Service is skipped
/// with a warning; its secondaries refresh on the SOA timer and on the zone
/// reconcile's retransfer.
///
/// # Arguments
/// * `client` - Kubernetes client (one GET per secondary instance)
/// * `secondary_refs` - The zone's secondary instances
pub async fn secondary_notify_targets(
    client: &Client,
    secondary_refs: &[InstanceReference],
) -> Vec<String> {
    let mut targets = Vec::with_capacity(secondary_refs.len());
    for instance in secondary_refs {
        let api: Api<Service> = Api::namespaced(client.clone(), &instance.namespace);
        match api.get_opt(&instance.name).await {
            Ok(Some(service)) => match service_cluster_ip(&service) {
                Some(ip) => targets.push(ip),
                None => warn!(
                    "Service {}/{} has no ClusterIP; its secondaries get no NOTIFY",
                    instance.namespace, instance.name
                ),
            },
            Ok(None) => debug!(
                "Service {}/{} does not exist (yet); no NOTIFY target for it",
                instance.namespace, instance.name
            ),
            Err(e) => warn!(
                "Cannot read Service {}/{} for its NOTIFY target: {e}",
                instance.namespace, instance.name
            ),
        }
    }
    targets
}

/// The zone's primary and secondary instances and the peers it should carry
/// now (ADR-0019 decision 1).
///
/// # Arguments
/// * `ctx` - Controller context (instance and Pod stores, client)
/// * `instance_refs` - Every instance the zone selects
///
/// # Returns
/// `(primary_refs, secondary_refs, peers)`.
///
/// # Errors
/// Returns an error if an instance's role cannot be read.
pub async fn zone_transfer_peers(
    ctx: &crate::context::Context,
    instance_refs: &[InstanceReference],
) -> Result<(
    Vec<InstanceReference>,
    Vec<InstanceReference>,
    ZoneTransferPeers,
)> {
    let primary_refs = bindy_bind9::primary::filter_primary_instances_cached(
        &ctx.client,
        &ctx.stores.bind9_instances,
        instance_refs,
    )
    .await?;
    let secondary_refs = super::secondary::filter_secondary_instances(
        &ctx.client,
        &ctx.stores.bind9_instances,
        instance_refs,
    )
    .await?;
    let notify = secondary_notify_targets(&ctx.client, &secondary_refs).await;
    let peers = desired_transfer_peers(
        &ctx.stores.bind9_pods.state(),
        &primary_refs,
        &secondary_refs,
        notify,
    );
    Ok((primary_refs, secondary_refs, peers))
}

/// Rewrite `allow-transfer` / `also-notify` on every primary endpoint that
/// has the zone (ADR-0019 decision 2).
///
/// # Arguments
/// * `ctx` - Controller context
/// * `zone_name` - The zone's DNS name
/// * `primary_refs` - The zone's primary instances
/// * `peers` - The peers to write
/// * `resolver` - Where the primaries' endpoints come from
///
/// # Returns
/// One entry per endpoint (or instance) that could not be rewritten.
pub async fn refresh_primary_peers(
    ctx: &Arc<crate::context::Context>,
    zone_name: &str,
    primary_refs: &[InstanceReference],
    peers: &ZoneTransferPeers,
    resolver: &InstanceResolver,
) -> Vec<String> {
    let mut failures = Vec::new();
    for instance in primary_refs {
        let endpoints = match resolver
            .endpoints(&instance.namespace, &instance.name, BINDCAR_PORT_NAME)
            .await
        {
            Ok(endpoints) => endpoints,
            Err(e) => {
                failures.push(format!(
                    "primary {}/{}: endpoints unknown: {e}",
                    instance.namespace, instance.name
                ));
                continue;
            }
        };
        let manager =
            crate::dnszone::zone_manager_for_instance(ctx, &instance.name, &instance.namespace);
        for endpoint in endpoints {
            let server = format!("{}:{}", endpoint.ip, endpoint.port);
            match manager
                .update_primary_transfer_peers(zone_name, &server, &peers.secondaries, &peers.notify)
                .await
            {
                Ok(PeerUpdate::Updated) => debug!(
                    "Zone {zone_name} on primary {server}: transfer peers rewritten"
                ),
                Ok(PeerUpdate::UpdatedWithoutNotify) => info!(
                    "Zone {zone_name} on primary {server}: allow-transfer rewritten; its pre-ADR-0019 also-notify stays until the pod is replaced"
                ),
                Ok(PeerUpdate::ZoneAbsent) => debug!(
                    "Zone {zone_name} is not on primary {server} yet; it gets the peers when it is created"
                ),
                Err(e) => failures.push(format!(
                    "primary {}/{} ({server}): {e:#}",
                    instance.namespace, instance.name
                )),
            }
        }
    }
    failures
}

#[cfg(test)]
#[path = "transfer_peers_tests.rs"]
mod transfer_peers_tests;
