// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The zone-transfer peer sets of a zone (ADR-0019).
//!
//! A zone served by primaries and secondaries names pods by IP in two places:
//! a primary's `allow-transfer` / `also-notify`, and a secondary's
//! `primaries`. BIND9 pods keep their zones on an `emptyDir` and get a new IP
//! whenever they are replaced, so these lists must follow the pods. This
//! module computes the lists a zone should carry *now*, from the shared BIND9
//! Pod store (no API call), and decides which side of the zone must be
//! refreshed against the lists last pushed (`status.transferPeers`).
//!
//! Everything here is pure: the caller hands in the store contents.

use crate::constants::{CONDITION_STATUS_TRUE, ZONES_LOADED_CONDITION_TYPE};
use crate::crd::{InstanceReference, ZoneTransferPeers};
use crate::labels::K8S_INSTANCE;
use k8s_openapi::api::core::v1::{Pod, Service};
use std::sync::Arc;

/// The `status.phase` of a pod whose containers have started.
pub const POD_PHASE_RUNNING: &str = "Running";

/// The `spec.clusterIP` value of a headless Service.
const HEADLESS_CLUSTER_IP: &str = "None";

/// Whether the pod's readiness gate (if it has one) admitted it: its zones are
/// loaded (ADR-0017). A pod without the gate predates it and counts as
/// admitted.
#[must_use]
pub fn pod_zones_admitted(pod: &Pod) -> bool {
    let gated = pod
        .spec
        .as_ref()
        .and_then(|spec| spec.readiness_gates.as_ref())
        .is_some_and(|gates| {
            gates
                .iter()
                .any(|gate| gate.condition_type == ZONES_LOADED_CONDITION_TYPE)
        });
    if !gated {
        return true;
    }
    pod.status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions.iter().any(|condition| {
                condition.type_ == ZONES_LOADED_CONDITION_TYPE
                    && condition.status == CONDITION_STATUS_TRUE
            })
        })
}

/// Whether the pod is a live peer: it has an IP, is `Running` and is not
/// terminating. A terminating pod is leaving; naming it would leave a stale
/// entry behind the moment it is gone.
fn pod_is_live_peer(pod: &Pod) -> bool {
    if pod.metadata.deletion_timestamp.is_some() {
        return false;
    }
    let Some(status) = pod.status.as_ref() else {
        return false;
    };
    if status.pod_ip.as_deref().is_none_or(str::is_empty) {
        return false;
    }
    status.phase.as_deref() == Some(POD_PHASE_RUNNING)
}

/// The IPs of `instance`'s live pods, sorted and de-duplicated.
///
/// # Arguments
/// * `pods` - Every pod in the BIND9 Pod store
/// * `namespace` - The instance's namespace
/// * `instance_name` - The instance's name (the pods' `app.kubernetes.io/instance` label)
/// * `require_admitted` - Only pods the zones-loaded gate admitted (or that
///   carry no gate); used for transfer sources, which must hold every zone
///
/// # Returns
/// The pod IPs.
#[must_use]
pub fn peer_pod_ips(
    pods: &[Arc<Pod>],
    namespace: &str,
    instance_name: &str,
    require_admitted: bool,
) -> Vec<String> {
    let mut ips: Vec<String> = pods
        .iter()
        .filter(|pod| pod.metadata.namespace.as_deref() == Some(namespace))
        .filter(|pod| {
            pod.metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get(K8S_INSTANCE))
                .is_some_and(|name| name == instance_name)
        })
        .filter(|pod| pod_is_live_peer(pod))
        .filter(|pod| !require_admitted || pod_zones_admitted(pod))
        .filter_map(|pod| pod.status.as_ref()?.pod_ip.clone())
        .collect();
    ips.sort();
    ips.dedup();
    ips
}

/// The peer sets a zone should carry now (ADR-0019 decision 1).
///
/// # Arguments
/// * `pods` - Every pod in the BIND9 Pod store
/// * `primary_refs` - The zone's primary instances
/// * `secondary_refs` - The zone's secondary instances
/// * `notify` - The NOTIFY targets: the secondary instances' Service ClusterIPs
///
/// # Returns
/// `primaries`: admitted, live primary pod IPs (a secondary's transfer
/// sources); `secondaries`: live secondary pod IPs, gated or not (the
/// primaries' `allow-transfer`); `notify` as given. Every list sorted and
/// de-duplicated.
#[must_use]
pub fn desired_transfer_peers(
    pods: &[Arc<Pod>],
    primary_refs: &[InstanceReference],
    secondary_refs: &[InstanceReference],
    notify: Vec<String>,
) -> ZoneTransferPeers {
    let collect = |refs: &[InstanceReference], require_admitted: bool| {
        let mut ips: Vec<String> = refs
            .iter()
            .flat_map(|r| peer_pod_ips(pods, &r.namespace, &r.name, require_admitted))
            .collect();
        ips.sort();
        ips.dedup();
        ips
    };
    let mut notify = notify;
    notify.sort();
    notify.dedup();
    ZoneTransferPeers {
        primaries: collect(primary_refs, true),
        secondaries: collect(secondary_refs, false),
        notify,
    }
}

/// Which side of a zone must be refreshed (ADR-0019 decision 2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PeerChanges {
    /// The transfer sources moved: every secondary's `primaries` must be
    /// replaced.
    pub primaries_changed: bool,
    /// The secondaries or their NOTIFY targets moved: every primary's
    /// `allow-transfer` / `also-notify` must be rewritten.
    pub secondaries_changed: bool,
}

impl PeerChanges {
    /// Whether any side must be refreshed.
    #[must_use]
    pub fn any(&self) -> bool {
        self.primaries_changed || self.secondaries_changed
    }
}

/// Compare the desired peers with the ones last pushed.
///
/// # Arguments
/// * `recorded` - `status.transferPeers`, absent on a zone never reconciled by
///   an operator that records it
/// * `desired` - The peers the zone should carry now
///
/// # Returns
/// Both sides changed when nothing is recorded; otherwise each side changed
/// exactly when its lists differ.
#[must_use]
pub fn peer_changes(
    recorded: Option<&ZoneTransferPeers>,
    desired: &ZoneTransferPeers,
) -> PeerChanges {
    let Some(recorded) = recorded else {
        return PeerChanges {
            primaries_changed: true,
            secondaries_changed: true,
        };
    };
    PeerChanges {
        primaries_changed: recorded.primaries != desired.primaries,
        secondaries_changed: recorded.secondaries != desired.secondaries
            || recorded.notify != desired.notify,
    }
}

/// The address a primary sends NOTIFY to for the secondaries behind
/// `service`: its ClusterIP, or `None` for a headless Service or one whose
/// ClusterIP is not allocated yet.
#[must_use]
pub fn service_cluster_ip(service: &Service) -> Option<String> {
    service
        .spec
        .as_ref()?
        .cluster_ip
        .as_deref()
        .filter(|ip| !ip.is_empty() && *ip != HEADLESS_CLUSTER_IP)
        .map(str::to_string)
}

#[cfg(test)]
#[path = "peers_tests.rs"]
mod peers_tests;
