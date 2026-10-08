// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for the zone-transfer peer sets (ADR-0019).

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::constants::{CONDITION_STATUS_TRUE, ZONES_LOADED_CONDITION_TYPE};
    use crate::crd::{InstanceReference, ZoneTransferPeers};
    use crate::labels::K8S_INSTANCE;
    use k8s_openapi::api::core::v1::{Pod, PodCondition, PodReadinessGate, PodSpec, PodStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    const NS: &str = "dns";

    /// A running pod of `instance` with `ip`, gated or not, admitted or not.
    fn pod(name: &str, instance: &str, ip: Option<&str>, gated: bool, admitted: bool) -> Arc<Pod> {
        let mut labels = BTreeMap::new();
        labels.insert(K8S_INSTANCE.to_string(), instance.to_string());
        let mut conditions = Vec::new();
        if admitted {
            conditions.push(PodCondition {
                type_: ZONES_LOADED_CONDITION_TYPE.to_string(),
                status: CONDITION_STATUS_TRUE.to_string(),
                ..Default::default()
            });
        }
        Arc::new(Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(NS.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(PodSpec {
                readiness_gates: gated.then(|| {
                    vec![PodReadinessGate {
                        condition_type: ZONES_LOADED_CONDITION_TYPE.to_string(),
                    }]
                }),
                ..Default::default()
            }),
            status: Some(PodStatus {
                pod_ip: ip.map(str::to_string),
                phase: Some(POD_PHASE_RUNNING.to_string()),
                conditions: Some(conditions),
                ..Default::default()
            }),
        })
    }

    fn terminating(pod: &Arc<Pod>) -> Arc<Pod> {
        let mut p = (**pod).clone();
        p.metadata.deletion_timestamp = Some(Time(k8s_openapi::jiff::Timestamp::now()));
        Arc::new(p)
    }

    fn with_phase(pod: &Arc<Pod>, phase: &str) -> Arc<Pod> {
        let mut p = (**pod).clone();
        if let Some(status) = p.status.as_mut() {
            status.phase = Some(phase.to_string());
        }
        Arc::new(p)
    }

    fn instance_ref(name: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: NS.to_string(),
            last_reconciled_at: None,
        }
    }

    // --- peer_pod_ips --------------------------------------------------------

    #[test]
    fn peer_pod_ips_returns_only_the_instance_pods_sorted() {
        let pods = vec![
            pod("p-b", "primary", Some("10.0.0.9"), false, false),
            pod("p-a", "primary", Some("10.0.0.2"), false, false),
            pod("other", "secondary", Some("10.0.0.5"), false, false),
        ];

        let ips = peer_pod_ips(&pods, NS, "primary", false);

        assert_eq!(ips, vec!["10.0.0.2".to_string(), "10.0.0.9".to_string()]);
    }

    #[test]
    fn peer_pod_ips_drops_terminating_pods() {
        // The rc.7 bug: a pod LIST filtered on phase == Running kept the old
        // pods of a rollout, so the secondary named primaries that died a
        // minute later.
        let old = pod("old", "primary", Some("10.0.0.1"), false, false);
        let pods = vec![
            terminating(&old),
            pod("new", "primary", Some("10.0.0.2"), false, false),
        ];

        let ips = peer_pod_ips(&pods, NS, "primary", false);

        assert_eq!(ips, vec!["10.0.0.2".to_string()]);
    }

    #[test]
    fn peer_pod_ips_drops_pods_without_ip_or_not_running() {
        let pending = pod("pending", "primary", None, false, false);
        let done = with_phase(
            &pod("done", "primary", Some("10.0.0.3"), false, false),
            "Succeeded",
        );
        let pods = vec![pending, done];

        assert!(peer_pod_ips(&pods, NS, "primary", false).is_empty());
    }

    #[test]
    fn peer_pod_ips_ignores_other_namespaces() {
        let mut other = (*pod("x", "primary", Some("10.0.0.4"), false, false)).clone();
        other.metadata.namespace = Some("elsewhere".to_string());
        let pods = vec![Arc::new(other)];

        assert!(peer_pod_ips(&pods, NS, "primary", false).is_empty());
    }

    #[test]
    fn peer_pod_ips_requiring_admission_skips_gated_pods() {
        // A primary still loading its zones must not be a transfer source: a
        // forced retransfer from it would copy a partial zone.
        let pods = vec![
            pod("loading", "primary", Some("10.0.0.1"), true, false),
            pod("admitted", "primary", Some("10.0.0.2"), true, true),
            pod("ungated", "primary", Some("10.0.0.3"), false, false),
        ];

        let ips = peer_pod_ips(&pods, NS, "primary", true);

        assert_eq!(ips, vec!["10.0.0.2".to_string(), "10.0.0.3".to_string()]);
    }

    #[test]
    fn peer_pod_ips_without_admission_keeps_gated_pods() {
        // A new secondary must be allowed to transfer before it can load.
        let pods = vec![pod("loading", "secondary", Some("10.0.0.7"), true, false)];

        assert_eq!(
            peer_pod_ips(&pods, NS, "secondary", false),
            vec!["10.0.0.7".to_string()]
        );
    }

    #[test]
    fn peer_pod_ips_deduplicates() {
        let pods = vec![
            pod("a", "primary", Some("10.0.0.1"), false, false),
            pod("b", "primary", Some("10.0.0.1"), false, false),
        ];

        assert_eq!(
            peer_pod_ips(&pods, NS, "primary", false),
            vec!["10.0.0.1".to_string()]
        );
    }

    // --- desired_transfer_peers ----------------------------------------------

    #[test]
    fn desired_transfer_peers_combines_both_roles() {
        let pods = vec![
            pod("p1", "primary-1", Some("10.0.1.1"), true, true),
            pod("p2", "primary-2", Some("10.0.2.1"), true, true),
            pod("p2-new", "primary-2", Some("10.0.2.2"), true, false),
            pod("s1", "secondary-1", Some("10.0.3.1"), true, false),
        ];

        let peers = desired_transfer_peers(
            &pods,
            &[instance_ref("primary-2"), instance_ref("primary-1")],
            &[instance_ref("secondary-1")],
            vec!["10.96.0.20".to_string(), "10.96.0.10".to_string()],
        );

        assert_eq!(
            peers,
            ZoneTransferPeers {
                primaries: vec!["10.0.1.1".to_string(), "10.0.2.1".to_string()],
                secondaries: vec!["10.0.3.1".to_string()],
                notify: vec!["10.96.0.10".to_string(), "10.96.0.20".to_string()],
            }
        );
    }

    // --- peer_changes --------------------------------------------------------

    fn peers(primaries: &[&str], secondaries: &[&str], notify: &[&str]) -> ZoneTransferPeers {
        let owned = |list: &[&str]| list.iter().map(|s| (*s).to_string()).collect();
        ZoneTransferPeers {
            primaries: owned(primaries),
            secondaries: owned(secondaries),
            notify: owned(notify),
        }
    }

    #[test]
    fn peer_changes_nothing_recorded_means_everything_changed() {
        // A zone never reconciled by an ADR-0019 operator (or upgraded from
        // rc.7) must have its peers pushed everywhere once.
        let desired = peers(&["10.0.0.1"], &["10.0.0.2"], &["10.96.0.1"]);

        let changes = peer_changes(None, &desired);

        assert!(changes.primaries_changed);
        assert!(changes.secondaries_changed);
    }

    #[test]
    fn peer_changes_identical_is_a_no_op() {
        let desired = peers(&["10.0.0.1"], &["10.0.0.2"], &["10.96.0.1"]);

        let changes = peer_changes(Some(&desired.clone()), &desired);

        assert!(!changes.primaries_changed);
        assert!(!changes.secondaries_changed);
        assert!(!changes.any());
    }

    #[test]
    fn peer_changes_moved_primary_touches_only_secondaries() {
        let recorded = peers(&["10.0.0.1"], &["10.0.0.2"], &["10.96.0.1"]);
        let desired = peers(&["10.0.0.9"], &["10.0.0.2"], &["10.96.0.1"]);

        let changes = peer_changes(Some(&recorded), &desired);

        assert!(changes.primaries_changed);
        assert!(!changes.secondaries_changed);
    }

    #[test]
    fn peer_changes_moved_secondary_touches_only_primaries() {
        let recorded = peers(&["10.0.0.1"], &["10.0.0.2"], &["10.96.0.1"]);
        let desired = peers(&["10.0.0.1"], &["10.0.0.8"], &["10.96.0.1"]);

        let changes = peer_changes(Some(&recorded), &desired);

        assert!(!changes.primaries_changed);
        assert!(changes.secondaries_changed);
    }

    #[test]
    fn peer_changes_new_notify_target_touches_primaries() {
        // A recreated secondary Service has a new ClusterIP.
        let recorded = peers(&["10.0.0.1"], &["10.0.0.2"], &["10.96.0.1"]);
        let desired = peers(&["10.0.0.1"], &["10.0.0.2"], &["10.96.0.7"]);

        let changes = peer_changes(Some(&recorded), &desired);

        assert!(changes.secondaries_changed);
        assert!(!changes.primaries_changed);
    }

    // --- service_cluster_ip ---------------------------------------------------

    fn service(cluster_ip: Option<&str>) -> k8s_openapi::api::core::v1::Service {
        k8s_openapi::api::core::v1::Service {
            spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                cluster_ip: cluster_ip.map(str::to_string),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn service_cluster_ip_returns_the_address() {
        assert_eq!(
            service_cluster_ip(&service(Some("10.96.0.10"))),
            Some("10.96.0.10".to_string())
        );
    }

    #[test]
    fn service_cluster_ip_none_for_headless_or_unset() {
        assert_eq!(service_cluster_ip(&service(Some("None"))), None);
        assert_eq!(service_cluster_ip(&service(Some(""))), None);
        assert_eq!(service_cluster_ip(&service(None)), None);
    }
}
