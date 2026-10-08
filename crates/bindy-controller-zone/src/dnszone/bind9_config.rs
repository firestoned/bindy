// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! BIND9 configuration orchestration for DNS zones.
//!
//! This module coordinates zone configuration on primary and secondary BIND9 instances,
//! managing status updates and error handling throughout the configuration process.

use anyhow::{anyhow, Result};
use bindy_bind9::peers::peer_changes;
use kube::ResourceExt;
use std::sync::Arc;
use tracing::{debug, info};

use super::transfer_peers::{
    mark_no_transfer_source, mark_peer_refresh_failed, mark_secondaries_not_loaded, PeerPushResult,
};
use crate::crd::{DNSZone, InstanceReference};

/// Configure zone on all BIND9 instances (primary and secondary).
///
/// This function orchestrates the complete BIND9 configuration workflow:
/// 1. Sets initial "Progressing" status
/// 2. Computes the zone's transfer peers from the stores and compares them
///    with the ones last pushed (`status.transferPeers`, ADR-0019)
/// 3. Configures zone on all primary instances (a zone created here gets the
///    current peers)
/// 4. When the secondaries or their NOTIFY targets moved, rewrites every
///    primary's `allow-transfer` / `also-notify`
/// 5. Configures zone on all secondary instances, replacing the zone where
///    the transfer sources moved, and reports secondaries not loaded
/// 6. Records the peers once every server took them
///
/// # Arguments
///
/// * `ctx` - Application context with Kubernetes client
/// * `dnszone` - The DNSZone resource being reconciled
/// * `status_updater` - Status updater for condition updates
/// * `instance_refs` - All instance references assigned to the zone
/// * `_unreconciled_instances` - Unused: every instance is configured on every
///   reconcile
///
/// # Returns
///
/// Tuple of `(primary_outcome, secondary_outcome)` - per-instance and per-endpoint
/// configuration counts for primary and secondary instances (see
/// [`super::types::ZoneConfigOutcome`])
///
/// # Errors
///
/// Returns an error if:
/// - No primary instance is assigned (cannot configure secondary zones)
/// - Primary configuration fails completely
/// - Kubernetes API operations fail
///
/// Note: Secondary configuration failure is non-fatal and reported through
/// `Degraded`. On every fatal error path the Ready condition is set to False
/// and the Progressing condition is resolved before the error is returned.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
pub async fn configure_zone_on_instances(
    ctx: Arc<crate::context::Context>,
    dnszone: &DNSZone,
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    instance_refs: &[InstanceReference],
    _unreconciled_instances: &[InstanceReference],
) -> Result<(
    super::types::ZoneConfigOutcome,
    super::types::ZoneConfigOutcome,
)> {
    let client = ctx.client.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let spec = &dnszone.spec;

    tracing::debug!("Ensuring BIND9 zone exists on all instances (declarative reconciliation)");

    // Set initial Progressing status (in-memory)
    status_updater.set_condition(
        "Progressing",
        "True",
        "PrimaryReconciling",
        "Configuring zone on primary servers",
    );

    // The peers the zone should carry now, from the Pod store (ADR-0019).
    let (primary_refs, secondary_refs, peers) =
        match super::transfer_peers::zone_transfer_peers(&ctx, instance_refs).await {
            Ok(found) => found,
            Err(e) => {
                set_failure_conditions(
                    status_updater,
                    "PrimaryFailed",
                    &format!("Failed to find the zone's instances: {e}"),
                );
                status_updater.apply(&client).await?;
                return Err(e);
            }
        };

    if primary_refs.is_empty() {
        let message = "No primary servers found - cannot configure secondary zones";
        set_failure_conditions(status_updater, "PrimaryFailed", message);
        status_updater.apply(&client).await?;
        return Err(anyhow!(
            "No primary servers found for zone {}/{} - cannot configure secondary zones",
            namespace,
            spec.zone_name
        ));
    }

    let recorded = dnszone
        .status
        .as_ref()
        .and_then(|status| status.transfer_peers.as_ref());
    let changes = peer_changes(recorded, &peers);
    if changes.any() {
        info!(
            "Transfer peers of zone {}/{} changed (primaries: {}, secondaries/notify: {}): recorded {:?}, now {:?}",
            namespace,
            spec.zone_name,
            changes.primaries_changed,
            changes.secondaries_changed,
            recorded,
            peers
        );
    } else {
        debug!(
            "Transfer peers of zone {}/{} unchanged: {:?}",
            namespace, spec.zone_name, peers
        );
    }

    // Add/update zone on all primary instances. A zone created here gets the
    // current peers. We pass ALL instances (not just unreconciled ones) so
    // zones are recreated after pod restarts; add is idempotent.
    let primary_outcome = match super::add_dnszone(
        ctx.clone(),
        dnszone.clone(),
        status_updater,
        instance_refs,
        &peers,
    )
    .await
    {
        Ok(outcome) => {
            status_updater.set_condition(
                "Progressing",
                "True",
                "PrimaryReconciled",
                &format!(
                    "Zone {} configured on {} primary instance(s) ({} endpoint(s))",
                    spec.zone_name, outcome.instances_configured, outcome.endpoints_configured
                ),
            );
            outcome
        }
        Err(e) => {
            set_failure_conditions(
                status_updater,
                "PrimaryFailed",
                &format!("Failed to configure zone on primary servers: {e}"),
            );
            status_updater.apply(&client).await?;
            return Err(e);
        }
    };

    let mut push = PeerPushResult::default();

    // The secondaries (or the Services NOTIFY reaches them through) moved:
    // rewrite every primary's allow-transfer / also-notify BEFORE the
    // secondaries are pointed at them, so a new secondary is allowed first.
    if changes.secondaries_changed {
        let resolver = bindy_bind9::instances::InstanceResolver::for_kube(&ctx.client, &ctx.stores);
        push.primary_failures = super::transfer_peers::refresh_primary_peers(
            &ctx,
            &spec.zone_name,
            &primary_refs,
            &peers,
            &resolver,
        )
        .await;
        mark_peer_refresh_failed(status_updater, &spec.zone_name, &push.primary_failures);
    }

    status_updater.set_condition(
        "Progressing",
        "True",
        "SecondaryReconciling",
        "Configuring zone on secondary servers",
    );

    let secondary_outcome = if secondary_refs.is_empty() {
        super::types::ZoneConfigOutcome::default()
    } else if peers.primaries.is_empty() {
        // Every primary pod is still loading its zones (or none runs): the
        // secondaries keep what they have until one is admitted, whose
        // Endpoints change wakes this zone.
        push.secondaries_skipped = true;
        mark_no_transfer_source(status_updater, &spec.zone_name);
        super::types::ZoneConfigOutcome::default()
    } else {
        match super::add_dnszone_to_secondaries(
            ctx.clone(),
            dnszone.clone(),
            &peers.primaries,
            status_updater,
            instance_refs,
            changes.primaries_changed,
        )
        .await
        {
            Ok(secondary) => {
                push.secondary_failures = secondary.failures;
                mark_secondaries_not_loaded(status_updater, &spec.zone_name, &secondary.not_loaded);
                if secondary.outcome.endpoints_configured > 0 {
                    status_updater.set_condition(
                        "Progressing",
                        "True",
                        "SecondaryReconciled",
                        &format!(
                            "Zone {} configured on {} secondary instance(s) ({} endpoint(s))",
                            spec.zone_name,
                            secondary.outcome.instances_configured,
                            secondary.outcome.endpoints_configured
                        ),
                    );
                }
                secondary.outcome
            }
            Err(e) => {
                // Secondary failure is non-fatal - primaries still work
                tracing::warn!(
                    "Failed to configure zone on secondary servers: {}. Primary servers are still operational.",
                    e
                );
                push.secondary_failures += 1;
                status_updater.set_condition(
                    "Degraded",
                    "True",
                    "SecondaryFailed",
                    &format!(
                        "Zone configured on {} primary instance(s) but secondary configuration failed: {e}",
                        primary_outcome.instances_configured
                    ),
                );
                super::types::ZoneConfigOutcome::default()
            }
        }
    };

    // Record the peers only once every server names them; otherwise the next
    // reconcile (a backoff retry, the zone is Degraded) pushes them again.
    if push.complete() {
        status_updater.set_transfer_peers(peers);
    } else {
        debug!(
            "Transfer peers of zone {}/{} not fully pushed ({:?}); not recording them",
            namespace, spec.zone_name, push
        );
    }

    Ok((primary_outcome, secondary_outcome))
}

/// Sets the condition triple for a fatal zone configuration failure (in-memory).
///
/// Ensures the conditions converge to a consistent failed state:
/// - `Degraded=True` with the failure reason and message
/// - `Ready=False` with the same reason and message (a previously set
///   `Ready=True` must never survive a failed reconciliation)
/// - `Progressing=False` (the reconciliation attempt has finished)
///
/// # Arguments
///
/// * `status_updater` - Status updater collecting in-memory condition changes
/// * `reason` - Programmatic failure reason in `CamelCase` (e.g. `PrimaryFailed`)
/// * `message` - Human-readable failure explanation
pub fn set_failure_conditions(
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    reason: &str,
    message: &str,
) {
    status_updater.set_condition("Degraded", "True", reason, message);
    status_updater.set_condition("Ready", "False", reason, message);
    status_updater.set_condition(
        "Progressing",
        "False",
        reason,
        "Reconciliation attempt finished with errors",
    );
}

#[cfg(test)]
#[path = "bind9_config_tests.rs"]
mod bind9_config_tests;
