// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `zones_gate.rs`: the pure decisions of the zones-loaded
//! readiness gate (ADR-0017).

#[cfg(test)]
mod tests {
    use super::super::{
        condition_matches, gate_outcome, gate_patch, gate_step, gated_pods_for_zone, pod_gate_key,
        required_zones, sibling_addresses, transition_time, truncate_message, zone_gate_key,
        GateStep, ZoneLoad, MAX_GATE_MESSAGE_CHARS,
    };
    use crate::constants::{
        CONDITION_STATUS_FALSE, CONDITION_STATUS_TRUE, ZONES_LOADED_CONDITION_TYPE,
        ZONES_LOADED_REASON_FAILED, ZONES_LOADED_REASON_LOADED, ZONES_LOADED_REASON_LOADING,
        ZONES_LOADED_REASON_PARTIAL,
    };
    use crate::crd::{Bind9Instance, DNSZone};
    use bindy_bind9::instances::EndpointAddress;
    use k8s_openapi::api::core::v1::Pod;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
    use kube::runtime::reflector::ObjectRef;
    use serde_json::{json, Value};
    use std::sync::Arc;

    const NS: &str = "dns";
    const INSTANCE: &str = "primary-0";
    const POD_IP: &str = "10.1.0.2";

    // ------------------------------------------------------------------
    // Fixtures
    // ------------------------------------------------------------------

    /// A BIND9 pod of `INSTANCE`, with the gate in its spec, the given
    /// `ContainersReady` status and, optionally, a gate condition.
    fn pod(name: &str, containers_ready: &str, gate: Option<&str>) -> Pod {
        let mut conditions = vec![json!({"type": "ContainersReady", "status": containers_ready})];
        if let Some(status) = gate {
            conditions.push(json!({
                "type": ZONES_LOADED_CONDITION_TYPE,
                "status": status,
                "reason": ZONES_LOADED_REASON_LOADING,
                "message": "loading",
                "lastTransitionTime": "2026-10-06T10:00:00Z",
            }));
        }
        serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": name,
                "namespace": NS,
                "labels": {"app.kubernetes.io/instance": INSTANCE},
            },
            "spec": {
                "containers": [{"name": "bind9"}],
                "readinessGates": [{"conditionType": ZONES_LOADED_CONDITION_TYPE}],
            },
            "status": {"podIP": POD_IP, "conditions": conditions},
        }))
        .expect("valid Pod")
    }

    fn instance(namespace: &str, name: &str, annotations: Value) -> Arc<Bind9Instance> {
        Arc::new(
            serde_json::from_value(json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "Bind9Instance",
                "metadata": {
                    "name": name,
                    "namespace": namespace,
                    "labels": {"tier": "edge"},
                    "annotations": annotations,
                },
                "spec": {"clusterRef": "prod", "role": "primary"}
            }))
            .expect("valid Bind9Instance"),
        )
    }

    /// A zone in `namespace` selecting `tier=<tier>` instances, configured
    /// (`Configured`) on the instances listed in `configured_on`.
    fn zone(namespace: &str, name: &str, tier: &str, configured_on: &[&str]) -> DNSZone {
        let instances: Vec<Value> = configured_on
            .iter()
            .map(|inst| {
                json!({
                    "apiVersion": "bindy.firestoned.io/v1beta1",
                    "kind": "Bind9Instance",
                    "name": inst,
                    "namespace": NS,
                    "status": "Configured",
                })
            })
            .collect();
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {"name": name, "namespace": namespace},
            "spec": {
                "zoneName": format!("{name}.example"),
                "soaRecord": {
                    "primaryNs": "ns1.example.com.",
                    "adminEmail": "admin.example.com.",
                    "serial": 1,
                    "refresh": 3600,
                    "retry": 600,
                    "expire": 604_800,
                    "negativeTtl": 86400
                },
                "bind9InstancesFrom": [{"selector": {"matchLabels": {"tier": tier}}}],
            },
            "status": {"conditions": [], "bind9Instances": instances},
        }))
        .expect("valid DNSZone")
    }

    fn names(zones: &[Arc<DNSZone>]) -> Vec<String> {
        let mut names: Vec<String> = zones
            .iter()
            .map(|z| z.metadata.name.clone().unwrap_or_default())
            .collect();
        names.sort();
        names
    }

    // ------------------------------------------------------------------
    // gate_step: which pods the gate controller acts on
    // ------------------------------------------------------------------

    #[test]
    fn a_container_ready_gated_pod_is_evaluated() {
        let step = gate_step(&pod("p", "True", None));
        assert_eq!(
            step,
            GateStep::Evaluate {
                instance_namespace: NS.to_string(),
                instance_name: INSTANCE.to_string(),
                pod_ip: POD_IP.to_string(),
            }
        );
    }

    #[test]
    fn a_pod_whose_gate_is_false_is_evaluated_again() {
        assert!(matches!(
            gate_step(&pod("p", "True", Some(CONDITION_STATUS_FALSE))),
            GateStep::Evaluate { .. }
        ));
    }

    #[test]
    fn a_pod_whose_gate_is_true_is_left_alone() {
        assert!(matches!(
            gate_step(&pod("p", "True", Some(CONDITION_STATUS_TRUE))),
            GateStep::Done(_)
        ));
    }

    /// The gate is a one-way latch: a zone that starts selecting the instance
    /// after the pod was admitted must not pull a serving pod back out of its
    /// Service. Neither the pod's own reconcile nor the zone mapper touches it.
    #[test]
    fn an_admitted_pod_stays_admitted_when_a_new_zone_selects_its_instance() {
        let admitted = pod("p", "True", Some(CONDITION_STATUS_TRUE));
        let instances = vec![instance(NS, INSTANCE, json!({}))];
        let new_zone = zone(NS, "new", "edge", &[INSTANCE]);

        assert!(matches!(gate_step(&admitted), GateStep::Done(_)));
        assert!(gated_pods_for_zone(&[Arc::new(admitted)], &instances, &new_zone).is_empty());
    }

    /// A container restart keeps the pod object, its `emptyDir` zone data and
    /// its gate condition: the pod is not re-gated.
    #[test]
    fn a_container_restart_does_not_reopen_the_gate() {
        let restarting = pod("p", "False", Some(CONDITION_STATUS_TRUE));
        assert!(matches!(gate_step(&restarting), GateStep::Done(_)));
    }

    #[test]
    fn a_pod_whose_containers_are_not_ready_waits_for_its_next_event() {
        assert_eq!(
            gate_step(&pod("p", "False", None)),
            GateStep::WaitForContainers
        );
    }

    #[test]
    fn a_pod_without_the_gate_is_not_touched() {
        let mut old = pod("p", "True", None);
        if let Some(spec) = old.spec.as_mut() {
            spec.readiness_gates = None;
        }
        assert!(matches!(gate_step(&old), GateStep::Done(_)));
    }

    #[test]
    fn a_terminating_pod_is_not_touched() {
        let mut terminating = pod("p", "True", None);
        terminating.metadata.deletion_timestamp = Some(Time(k8s_openapi::jiff::Timestamp::now()));
        assert!(matches!(gate_step(&terminating), GateStep::Done(_)));
    }

    #[test]
    fn a_pod_without_an_instance_label_is_not_touched() {
        let mut unlabeled = pod("p", "True", None);
        unlabeled.metadata.labels = None;
        assert!(matches!(gate_step(&unlabeled), GateStep::Done(_)));
    }

    // ------------------------------------------------------------------
    // required_zones: which zones must be on the pod before it is Ready
    // ------------------------------------------------------------------

    #[test]
    fn no_zone_means_nothing_to_wait_for() {
        let inst = instance(NS, INSTANCE, json!({}));
        assert!(required_zones(&[], &inst).is_empty());
    }

    /// A new setup: zones exist, but none selects this instance. The pod has
    /// nothing to wait for and is admitted as soon as its containers are
    /// ready.
    #[test]
    fn zones_that_do_not_select_the_instance_are_not_required() {
        let inst = instance(NS, INSTANCE, json!({}));
        let zones = vec![
            Arc::new(zone(NS, "core-a", "core", &["primary-1"])),
            Arc::new(zone(NS, "core-b", "core", &["primary-2"])),
        ];

        assert!(required_zones(&zones, &inst).is_empty());
        assert_eq!(
            gate_outcome(&[]).0,
            CONDITION_STATUS_TRUE,
            "nothing to load admits the pod"
        );
    }

    #[test]
    fn every_live_zone_selecting_the_instance_is_required() {
        let inst = instance(NS, INSTANCE, json!({}));
        let zones = vec![
            Arc::new(zone(NS, "served", "edge", &[INSTANCE])),
            // Live on another instance: a scale-up's new pod must get it too.
            Arc::new(zone(NS, "served-elsewhere", "edge", &["primary-1"])),
            Arc::new(zone(NS, "other-tier", "core", &["primary-1"])),
        ];

        assert_eq!(
            names(&required_zones(&zones, &inst)),
            vec!["served", "served-elsewhere"]
        );
    }

    /// A zone no instance serves yet (brand new, or broken from birth) cannot
    /// regress by admitting the pod, and must not hold a shared instance out
    /// of its Service.
    #[test]
    fn a_zone_served_nowhere_is_not_required() {
        let inst = instance(NS, INSTANCE, json!({}));
        let zones = vec![Arc::new(zone(NS, "not-live", "edge", &[]))];

        assert!(required_zones(&zones, &inst).is_empty());
    }

    #[test]
    fn a_zone_being_deleted_is_not_required() {
        let inst = instance(NS, INSTANCE, json!({}));
        let mut deleting = zone(NS, "deleting", "edge", &[INSTANCE]);
        deleting.metadata.deletion_timestamp = Some(Time(k8s_openapi::jiff::Timestamp::now()));

        assert!(required_zones(&[Arc::new(deleting)], &inst).is_empty());
    }

    #[test]
    fn a_duplicate_zone_loser_is_not_required() {
        let inst = instance(NS, INSTANCE, json!({}));
        let mut loser = zone(NS, "loser", "edge", &[INSTANCE]);
        if let Some(status) = loser.status.as_mut() {
            status.conditions = vec![serde_json::from_value(json!({
                "type": "Ready",
                "status": "False",
                "reason": "DuplicateZone",
                "message": "claimed by another zone",
                "lastTransitionTime": "2026-10-06T10:00:00Z",
            }))
            .expect("condition")];
        }

        assert!(required_zones(&[Arc::new(loser)], &inst).is_empty());
    }

    /// The F-003 namespace gate applies: a zone in another namespace selects
    /// the instance only when the instance allows that namespace.
    #[test]
    fn a_cross_namespace_zone_is_required_only_when_the_instance_allows_it() {
        let zones = vec![Arc::new(zone("tenant", "cross", "edge", &[INSTANCE]))];

        let closed = instance(NS, INSTANCE, json!({}));
        assert!(required_zones(&zones, &closed).is_empty());

        let mut allow = serde_json::Map::new();
        allow.insert(
            crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES.to_string(),
            json!("tenant"),
        );
        let open = instance(NS, INSTANCE, Value::Object(allow));
        assert_eq!(names(&required_zones(&zones, &open)), vec!["cross"]);
    }

    // ------------------------------------------------------------------
    // gate_outcome: when a failed zone holds the pod back
    // ------------------------------------------------------------------

    #[test]
    fn every_zone_loaded_admits_the_pod() {
        let (status, reason, message) = gate_outcome(&[ZoneLoad::Loaded, ZoneLoad::Loaded]);
        assert_eq!(status, CONDITION_STATUS_TRUE);
        assert_eq!(reason, ZONES_LOADED_REASON_LOADED);
        assert_eq!(message, "2 zone(s) loaded");
    }

    /// The old pod of a rollout still serves the zone: admitting the new pod
    /// would lose it, so the rollout waits (the old pod keeps serving).
    #[test]
    fn a_failed_zone_served_by_another_pod_blocks_the_pod() {
        let (status, reason, message) = gate_outcome(&[
            ZoneLoad::Loaded,
            ZoneLoad::Blocking("a.example: refused".to_string()),
        ]);
        assert_eq!(status, CONDITION_STATUS_FALSE);
        assert_eq!(reason, ZONES_LOADED_REASON_FAILED);
        assert!(message.contains("a.example: refused"), "{message}");
    }

    /// A zone failing on every pod of the instance (an invalid zone, or the
    /// only replica restarting) must not keep the instance's other zones out
    /// of service: no pod could serve it anyway.
    #[test]
    fn a_failed_zone_served_by_no_other_pod_does_not_block() {
        let (status, reason, message) = gate_outcome(&[
            ZoneLoad::Loaded,
            ZoneLoad::NotServedElsewhere("bad.example: invalid".to_string()),
        ]);
        assert_eq!(status, CONDITION_STATUS_TRUE);
        assert_eq!(reason, ZONES_LOADED_REASON_PARTIAL);
        assert!(
            message.starts_with("1/2 zone(s) loaded") && message.contains("bad.example"),
            "{message}"
        );
    }

    #[test]
    fn a_blocking_failure_wins_over_a_non_blocking_one() {
        let (status, _, _) = gate_outcome(&[
            ZoneLoad::NotServedElsewhere("x".to_string()),
            ZoneLoad::Blocking("y".to_string()),
        ]);
        assert_eq!(status, CONDITION_STATUS_FALSE);
    }

    #[test]
    fn siblings_are_the_instance_ready_pods_other_than_this_one() {
        let ready = vec![
            EndpointAddress {
                ip: "10.1.0.1".to_string(),
                port: 8080,
            },
            EndpointAddress {
                ip: POD_IP.to_string(),
                port: 8080,
            },
        ];
        assert_eq!(sibling_addresses(&ready, POD_IP), vec!["10.1.0.1:8080"]);
        assert!(sibling_addresses(&ready[1..], POD_IP).is_empty());
    }

    // ------------------------------------------------------------------
    // The pod condition and its patch
    // ------------------------------------------------------------------

    #[test]
    fn the_patch_carries_only_the_gate_condition() {
        let now = Time(k8s_openapi::jiff::Timestamp::now());
        let patch = gate_patch(
            CONDITION_STATUS_TRUE,
            ZONES_LOADED_REASON_LOADED,
            "2 zone(s) loaded",
            &now,
        );

        let conditions = patch["status"]["conditions"]
            .as_array()
            .expect("conditions list");
        assert_eq!(
            conditions.len(),
            1,
            "a strategic merge by type leaves other conditions alone: {patch}"
        );
        assert_eq!(conditions[0]["type"], ZONES_LOADED_CONDITION_TYPE);
        assert_eq!(conditions[0]["status"], CONDITION_STATUS_TRUE);
        assert_eq!(conditions[0]["reason"], ZONES_LOADED_REASON_LOADED);
        assert_eq!(conditions[0]["message"], "2 zone(s) loaded");
        assert!(conditions[0]["lastTransitionTime"].is_string());
        assert!(patch.get("spec").is_none() && patch.get("metadata").is_none());
    }

    #[test]
    fn an_identical_condition_is_not_rewritten() {
        let current = pod("p", "True", Some(CONDITION_STATUS_FALSE));

        assert!(condition_matches(
            &current,
            CONDITION_STATUS_FALSE,
            ZONES_LOADED_REASON_LOADING,
            "loading"
        ));
        assert!(!condition_matches(
            &current,
            CONDITION_STATUS_TRUE,
            ZONES_LOADED_REASON_LOADED,
            "loaded"
        ));
        assert!(!condition_matches(
            &current,
            CONDITION_STATUS_FALSE,
            ZONES_LOADED_REASON_FAILED,
            "loading"
        ));
    }

    #[test]
    fn the_transition_time_moves_only_when_the_status_changes() {
        let current = pod("p", "True", Some(CONDITION_STATUS_FALSE));
        let now = Time(k8s_openapi::jiff::Timestamp::now());

        let same = transition_time(&current, CONDITION_STATUS_FALSE, &now);
        assert_eq!(same.0.to_string(), "2026-10-06T10:00:00Z");

        let flipped = transition_time(&current, CONDITION_STATUS_TRUE, &now);
        assert_eq!(flipped, now);

        let fresh = transition_time(&pod("p", "True", None), CONDITION_STATUS_FALSE, &now);
        assert_eq!(fresh, now);
    }

    #[test]
    fn a_long_failure_message_is_truncated() {
        let long = "x".repeat(MAX_GATE_MESSAGE_CHARS * 2);
        let short = truncate_message(&long);
        assert!(short.chars().count() <= MAX_GATE_MESSAGE_CHARS);
        assert_eq!(truncate_message("short"), "short");
    }

    // ------------------------------------------------------------------
    // Event wiring
    // ------------------------------------------------------------------

    #[test]
    fn a_zone_event_wakes_the_gated_pods_of_the_instances_it_selects() {
        let instances = vec![
            instance(NS, INSTANCE, json!({})),
            instance(NS, "primary-1", json!({})),
        ];
        let mut other_instance_pod = pod("other", "True", None);
        if let Some(labels) = other_instance_pod.metadata.labels.as_mut() {
            labels.insert("app.kubernetes.io/instance".into(), "unknown".into());
        }
        let pods = vec![
            Arc::new(pod("gated", "True", None)),
            Arc::new(pod("failing", "True", Some(CONDITION_STATUS_FALSE))),
            Arc::new(pod("admitted", "True", Some(CONDITION_STATUS_TRUE))),
            Arc::new(other_instance_pod),
        ];
        let live = zone(NS, "served", "edge", &[INSTANCE]);

        let mut got = gated_pods_for_zone(&pods, &instances, &live);
        got.sort_by_key(ToString::to_string);

        let mut want: Vec<ObjectRef<Pod>> = vec![
            ObjectRef::new("gated").within(NS),
            ObjectRef::new("failing").within(NS),
        ];
        want.sort_by_key(ToString::to_string);
        assert_eq!(got, want);
    }

    #[test]
    fn a_zone_selecting_no_instance_wakes_nothing() {
        let instances = vec![instance(NS, INSTANCE, json!({}))];
        let pods = vec![Arc::new(pod("gated", "True", None))];
        let unrelated = zone(NS, "core", "core", &["primary-1"]);

        assert!(gated_pods_for_zone(&pods, &instances, &unrelated).is_empty());
    }

    #[test]
    fn the_pod_filter_passes_containers_turning_ready_and_the_gate_turning_true() {
        let starting = pod("p", "False", None);
        let containers_up = pod("p", "True", None);
        let loading = pod("p", "True", Some(CONDITION_STATUS_FALSE));
        let loaded = pod("p", "True", Some(CONDITION_STATUS_TRUE));

        assert_ne!(pod_gate_key(&starting), pod_gate_key(&containers_up));
        assert_ne!(pod_gate_key(&containers_up), pod_gate_key(&loading));
        assert_ne!(pod_gate_key(&loading), pod_gate_key(&loaded));
    }

    #[test]
    fn the_pod_filter_drops_a_reason_change_while_the_gate_stays_false() {
        let loading = pod("p", "True", Some(CONDITION_STATUS_FALSE));
        let mut failed = loading.clone();
        if let Some(conditions) = failed.status.as_mut().and_then(|s| s.conditions.as_mut()) {
            for condition in conditions.iter_mut() {
                if condition.type_ == ZONES_LOADED_CONDITION_TYPE {
                    condition.reason = Some(ZONES_LOADED_REASON_FAILED.to_string());
                    condition.message = Some("boom".to_string());
                }
            }
        }

        assert_eq!(
            pod_gate_key(&loading),
            pod_gate_key(&failed),
            "a failed attempt retries with backoff, not on its own status write"
        );
    }

    #[test]
    fn the_zone_filter_passes_a_zone_becoming_live_and_drops_status_noise() {
        let new_zone = zone(NS, "z", "edge", &[]);
        let live = zone(NS, "z", "edge", &[INSTANCE]);
        let mut live_with_counts = live.clone();
        if let Some(status) = live_with_counts.status.as_mut() {
            status.records_count = 7;
        }

        assert_ne!(zone_gate_key(&new_zone), zone_gate_key(&live));
        assert_eq!(zone_gate_key(&live), zone_gate_key(&live_with_counts));
    }

    #[test]
    fn the_zone_filter_passes_a_selector_change() {
        let edge = zone(NS, "z", "edge", &[INSTANCE]);
        let core = zone(NS, "z", "core", &[INSTANCE]);

        assert_ne!(zone_gate_key(&edge), zone_gate_key(&core));
    }
}
