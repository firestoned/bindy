// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `rollout.rs`: the pure decisions of staggered BIND9
//! rollouts (ADR-0018).

#[cfg(test)]
mod tests {
    use super::super::{
        conflict_set, decide_rollout, deployment_rollout_key, peer_rollout, peer_states,
        pod_rollout_key, rollout_condition, rollout_queued, waiters_to_wake, InstanceId,
        PeerRollout, RolloutDecision, RolloutQueue, RolloutStatus, WaitReason,
    };
    use crate::crd::{Bind9Instance, DNSZone};
    use crate::status_reasons::{
        CONDITION_TYPE_ROLLOUT, REASON_ROLLOUT_PEER_STALLED, REASON_ROLLOUT_QUEUED,
    };
    use k8s_openapi::api::apps::v1::Deployment;
    use k8s_openapi::api::core::v1::Pod;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
    use kube::runtime::reflector::ObjectRef;
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    const NS: &str = "dns";
    const OTHER_NS: &str = "tenant";
    const CLUSTER: &str = "prod";
    const GENERATION: i64 = 7;

    // ------------------------------------------------------------------
    // Fixtures
    // ------------------------------------------------------------------

    fn id(namespace: &str, name: &str) -> InstanceId {
        InstanceId::new(namespace, name)
    }

    fn instance(namespace: &str, name: &str, cluster_ref: &str) -> Arc<Bind9Instance> {
        Arc::new(
            serde_json::from_value(json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "Bind9Instance",
                "metadata": {"name": name, "namespace": namespace},
                "spec": {"clusterRef": cluster_ref, "role": "primary"}
            }))
            .expect("valid Bind9Instance"),
        )
    }

    /// A zone whose status lists `served_by` as its instances.
    fn zone(name: &str, served_by: &[(&str, &str)]) -> Arc<DNSZone> {
        let instances: Vec<Value> = served_by
            .iter()
            .map(|(namespace, inst)| {
                json!({
                    "apiVersion": "bindy.firestoned.io/v1beta1",
                    "kind": "Bind9Instance",
                    "name": inst,
                    "namespace": namespace,
                    "status": "Configured",
                })
            })
            .collect();
        Arc::new(
            serde_json::from_value(json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "DNSZone",
                "metadata": {"name": name, "namespace": NS},
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
                    }
                },
                "status": {"conditions": [], "bind9Instances": instances},
            }))
            .expect("valid DNSZone"),
        )
    }

    /// A one-replica Deployment of instance `name` whose rollout completed.
    fn settled(name: &str) -> Deployment {
        deployment(
            name,
            GENERATION,
            GENERATION,
            (1, 1, 1, 1),
            ("True", "NewReplicaSetAvailable"),
        )
    }

    /// A Deployment of instance `name` (one replica wanted) with the given
    /// generation, observed generation, status counts
    /// `(replicas, updated, ready, available)` and `Progressing` condition.
    fn deployment(
        name: &str,
        generation: i64,
        observed: i64,
        counts: (i32, i32, i32, i32),
        progressing: (&str, &str),
    ) -> Deployment {
        let (replicas, updated, ready, available) = counts;
        serde_json::from_value(json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {
                "name": name,
                "namespace": NS,
                "generation": generation,
                "ownerReferences": [{
                    "apiVersion": "bindy.firestoned.io/v1beta1",
                    "kind": "Bind9Instance",
                    "name": name,
                    "uid": "u",
                }],
            },
            "spec": {
                "replicas": 1,
                "selector": {"matchLabels": {"app.kubernetes.io/instance": name}},
                "template": {"spec": {"containers": []}},
            },
            "status": {
                "observedGeneration": observed,
                "replicas": replicas,
                "updatedReplicas": updated,
                "readyReplicas": ready,
                "availableReplicas": available,
                "conditions": [{
                    "type": "Progressing",
                    "status": progressing.0,
                    "reason": progressing.1,
                }],
            },
        }))
        .expect("valid Deployment")
    }

    fn rolling(name: &str) -> Deployment {
        // maxSurge 1: the old pod and the new, gated one.
        deployment(
            name,
            GENERATION,
            GENERATION,
            (2, 1, 1, 1),
            ("True", "ReplicaSetUpdated"),
        )
    }

    fn pod(instance: &str, name: &str, ready: bool) -> Arc<Pod> {
        Arc::new(
            serde_json::from_value(json!({
                "apiVersion": "v1",
                "kind": "Pod",
                "metadata": {
                    "name": name,
                    "namespace": NS,
                    "labels": {"app.kubernetes.io/instance": instance},
                },
                "spec": {"containers": []},
                "status": {"conditions": [
                    {"type": "Ready", "status": if ready { "True" } else { "False" }},
                ]},
            }))
            .expect("valid Pod"),
        )
    }

    fn no_providers() -> BTreeSet<String> {
        BTreeSet::new()
    }

    // ------------------------------------------------------------------
    // conflict_set
    // ------------------------------------------------------------------

    #[test]
    fn instances_serving_a_common_zone_conflict() {
        let me = instance(NS, "a", "");
        let instances = vec![me.clone(), instance(NS, "b", ""), instance(NS, "c", "")];
        let zones = vec![zone("z", &[(NS, "a"), (NS, "b")])];

        let got = conflict_set(&me, &instances, &zones, &no_providers());
        assert_eq!(got, BTreeSet::from([id(NS, "b")]));
    }

    #[test]
    fn instances_of_the_same_cluster_conflict_without_a_common_zone() {
        let me = instance(NS, "a", CLUSTER);
        let instances = vec![
            me.clone(),
            instance(NS, "b", CLUSTER),
            instance(NS, "other-cluster", "dev"),
            // Same name, other namespace: another Bind9Cluster.
            instance(OTHER_NS, "c", CLUSTER),
        ];

        let got = conflict_set(&me, &instances, &[], &no_providers());
        assert_eq!(got, BTreeSet::from([id(NS, "b")]));
    }

    #[test]
    fn instances_of_one_provider_conflict_across_namespaces() {
        let me = instance(NS, "a", CLUSTER);
        let instances = vec![me.clone(), instance(OTHER_NS, "c", CLUSTER)];
        let providers = BTreeSet::from([CLUSTER.to_string()]);

        let got = conflict_set(&me, &instances, &[], &providers);
        assert_eq!(got, BTreeSet::from([id(OTHER_NS, "c")]));
    }

    #[test]
    fn standalone_instances_sharing_nothing_do_not_conflict() {
        let me = instance(NS, "a", "");
        let instances = vec![me.clone(), instance(NS, "b", "")];
        let zones = vec![zone("z", &[(NS, "b")])];

        assert!(conflict_set(&me, &instances, &zones, &no_providers()).is_empty());
    }

    #[test]
    fn an_instance_being_deleted_is_not_in_the_conflict_set() {
        let me = instance(NS, "a", CLUSTER);
        let mut leaving = (*instance(NS, "b", CLUSTER)).clone();
        leaving.metadata.deletion_timestamp = Some(Time(k8s_openapi::jiff::Timestamp::now()));
        let instances = vec![me.clone(), Arc::new(leaving)];

        assert!(conflict_set(&me, &instances, &[], &no_providers()).is_empty());
    }

    #[test]
    fn the_conflict_relation_is_symmetric() {
        let a = instance(NS, "a", "");
        let b = instance(NS, "b", "");
        let instances = vec![a.clone(), b.clone()];
        let zones = vec![zone("z", &[(NS, "a"), (NS, "b")])];

        assert!(conflict_set(&a, &instances, &zones, &no_providers()).contains(&id(NS, "b")));
        assert!(conflict_set(&b, &instances, &zones, &no_providers()).contains(&id(NS, "a")));
    }

    // ------------------------------------------------------------------
    // peer_rollout: what counts as mid-rollout
    // ------------------------------------------------------------------

    #[test]
    fn no_deployment_is_idle() {
        assert_eq!(peer_rollout(None, &[]), PeerRollout::Idle);
    }

    #[test]
    fn a_completed_rollout_is_idle() {
        let pods = vec![pod("b", "b-1", true)];
        assert_eq!(peer_rollout(Some(&settled("b")), &pods), PeerRollout::Idle);
    }

    #[test]
    fn an_unobserved_generation_is_rolling() {
        let d = deployment(
            "b",
            GENERATION + 1,
            GENERATION,
            (1, 1, 1, 1),
            ("True", "NewReplicaSetAvailable"),
        );
        assert!(matches!(
            peer_rollout(Some(&d), &[]),
            PeerRollout::Rolling(_)
        ));
    }

    #[test]
    fn a_surge_pod_waiting_for_its_gate_is_rolling() {
        let pods = vec![pod("b", "b-old", true), pod("b", "b-new", false)];
        assert!(matches!(
            peer_rollout(Some(&rolling("b")), &pods),
            PeerRollout::Rolling(_)
        ));
    }

    #[test]
    fn a_rollout_with_fewer_ready_replicas_is_rolling() {
        let d = deployment(
            "b",
            GENERATION,
            GENERATION,
            (1, 1, 0, 0),
            ("True", "ReplicaSetUpdated"),
        );
        assert!(matches!(
            peer_rollout(Some(&d), &[]),
            PeerRollout::Rolling(_)
        ));
    }

    /// A pod held back by its zones-loaded gate keeps the rollout going even
    /// when the counts look complete.
    #[test]
    fn a_gated_pod_during_a_rollout_is_rolling() {
        let d = deployment(
            "b",
            GENERATION,
            GENERATION,
            (1, 1, 1, 1),
            ("True", "ReplicaSetUpdated"),
        );
        let pods = vec![pod("b", "b-new", false)];
        assert!(matches!(
            peer_rollout(Some(&d), &pods),
            PeerRollout::Rolling(_)
        ));
    }

    #[test]
    fn a_rollout_past_its_progress_deadline_is_stalled_not_rolling() {
        let d = deployment(
            "b",
            GENERATION,
            GENERATION,
            (2, 1, 1, 1),
            ("False", "ProgressDeadlineExceeded"),
        );
        let pods = vec![pod("b", "b-new", false)];
        assert_eq!(peer_rollout(Some(&d), &pods), PeerRollout::Stalled);
    }

    /// A pod lost after the rollout completed is a degraded instance, which
    /// Kubernetes never times out: blocking on it could last forever.
    #[test]
    fn a_degraded_instance_outside_a_rollout_is_idle() {
        let d = deployment(
            "b",
            GENERATION,
            GENERATION,
            (1, 1, 0, 0),
            ("True", "NewReplicaSetAvailable"),
        );
        let pods = vec![pod("b", "b-crashing", false)];
        assert_eq!(peer_rollout(Some(&d), &pods), PeerRollout::Idle);
    }

    #[test]
    fn a_terminating_old_pod_does_not_hold_a_completed_rollout() {
        let mut old = (*pod("b", "b-old", false)).clone();
        old.metadata.deletion_timestamp = Some(Time(k8s_openapi::jiff::Timestamp::now()));
        let pods = vec![Arc::new(old), pod("b", "b-new", true)];
        assert_eq!(peer_rollout(Some(&settled("b")), &pods), PeerRollout::Idle);
    }

    #[test]
    fn an_instance_scaled_to_zero_is_idle() {
        let mut d = rolling("b");
        if let Some(spec) = d.spec.as_mut() {
            spec.replicas = Some(0);
        }
        assert_eq!(peer_rollout(Some(&d), &[]), PeerRollout::Idle);
    }

    #[test]
    fn peer_states_read_each_peers_deployment_and_pods() {
        let me = instance(NS, "a", CLUSTER);
        let instances = vec![
            me.clone(),
            instance(NS, "b", CLUSTER),
            instance(NS, "c", CLUSTER),
        ];
        let deployments = vec![Arc::new(rolling("b")), Arc::new(settled("c"))];
        let pods = vec![pod("b", "b-new", false), pod("c", "c-1", true)];

        let got = peer_states(&me, &instances, &[], &no_providers(), &deployments, &pods);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, id(NS, "b"));
        assert!(matches!(got[0].1, PeerRollout::Rolling(_)));
        assert_eq!(got[1], (id(NS, "c"), PeerRollout::Idle));
    }

    // ------------------------------------------------------------------
    // decide_rollout: ordering
    // ------------------------------------------------------------------

    fn none_claimed() -> BTreeSet<InstanceId> {
        BTreeSet::new()
    }

    fn none_waiting() -> BTreeMap<InstanceId, u64> {
        BTreeMap::new()
    }

    #[test]
    fn no_conflicting_rollout_proceeds() {
        let peers = vec![(id(NS, "b"), PeerRollout::Idle)];
        assert_eq!(
            decide_rollout(0, &peers, &none_claimed(), &none_waiting()),
            RolloutDecision::Proceed {
                stalled_peers: vec![]
            }
        );
    }

    #[test]
    fn a_conflicting_instance_mid_rollout_defers_with_its_name() {
        let peers = vec![
            (id(NS, "b"), PeerRollout::Idle),
            (
                id(NS, "c"),
                PeerRollout::Rolling("1/2 pods ready".to_string()),
            ),
        ];
        let decision = decide_rollout(0, &peers, &none_claimed(), &none_waiting());
        assert_eq!(
            decision,
            RolloutDecision::Wait {
                blocker: id(NS, "c"),
                reason: WaitReason::Rolling("1/2 pods ready".to_string()),
            }
        );
    }

    #[test]
    fn a_stalled_peer_does_not_block_and_is_reported() {
        let peers = vec![(id(NS, "b"), PeerRollout::Stalled)];
        assert_eq!(
            decide_rollout(0, &peers, &none_claimed(), &none_waiting()),
            RolloutDecision::Proceed {
                stalled_peers: vec![id(NS, "b")]
            }
        );
    }

    #[test]
    fn a_peer_holding_a_claim_defers() {
        let peers = vec![(id(NS, "b"), PeerRollout::Idle)];
        let claimed = BTreeSet::from([id(NS, "b")]);
        assert_eq!(
            decide_rollout(0, &peers, &claimed, &none_waiting()),
            RolloutDecision::Wait {
                blocker: id(NS, "b"),
                reason: WaitReason::Claimed
            }
        );
    }

    /// Two candidates never defer on each other: the one queued first goes.
    #[test]
    fn two_waiting_candidates_never_both_defer() {
        let a = id(NS, "a");
        let b = id(NS, "b");
        let waiting = BTreeMap::from([(a.clone(), 1), (b.clone(), 2)]);

        let for_a = decide_rollout(
            1,
            &[(b.clone(), PeerRollout::Idle)],
            &none_claimed(),
            &waiting,
        );
        let for_b = decide_rollout(
            2,
            &[(a.clone(), PeerRollout::Idle)],
            &none_claimed(),
            &waiting,
        );

        assert_eq!(
            for_a,
            RolloutDecision::Proceed {
                stalled_peers: vec![]
            }
        );
        assert_eq!(
            for_b,
            RolloutDecision::Wait {
                blocker: a,
                reason: WaitReason::QueuedEarlier
            }
        );
    }

    #[test]
    fn the_blocker_named_is_deterministic() {
        let peers = vec![
            (id(NS, "b"), PeerRollout::Rolling("x".to_string())),
            (id(NS, "c"), PeerRollout::Rolling("y".to_string())),
        ];
        let reversed: Vec<_> = peers.iter().rev().cloned().collect();
        assert_eq!(
            decide_rollout(0, &peers, &none_claimed(), &none_waiting()),
            decide_rollout(0, &reversed, &none_claimed(), &none_waiting()),
        );
    }

    // ------------------------------------------------------------------
    // RolloutQueue: claims close the race between concurrent reconciles
    // ------------------------------------------------------------------

    #[test]
    fn two_instances_deciding_at_once_do_not_both_roll() {
        let queue = RolloutQueue::new();
        let a = id(NS, "a");
        let b = id(NS, "b");
        let store_generation = |_: &InstanceId| Some(GENERATION);

        // Both read an idle store.
        let first = queue.try_start(
            &a,
            GENERATION,
            &[(b.clone(), PeerRollout::Idle)],
            store_generation,
        );
        let second = queue.try_start(
            &b,
            GENERATION,
            &[(a.clone(), PeerRollout::Idle)],
            store_generation,
        );

        assert_eq!(
            first,
            RolloutDecision::Proceed {
                stalled_peers: vec![]
            }
        );
        assert_eq!(
            second,
            RolloutDecision::Wait {
                blocker: a,
                reason: WaitReason::Claimed
            }
        );
        assert_eq!(queue.waiting(), vec![b]);
    }

    #[test]
    fn a_claim_ends_once_the_store_shows_the_patch() {
        let queue = RolloutQueue::new();
        let a = id(NS, "a");
        let b = id(NS, "b");
        let _ = queue.try_start(&a, GENERATION, &[], |_| Some(GENERATION));

        // The store now holds a's patched Deployment, which is rolling.
        let patched = |_: &InstanceId| Some(GENERATION + 1);
        let decision = queue.try_start(
            &b,
            GENERATION,
            &[(a.clone(), PeerRollout::Rolling("rolling".to_string()))],
            patched,
        );
        assert!(matches!(
            decision,
            RolloutDecision::Wait {
                reason: WaitReason::Rolling(_),
                ..
            }
        ));
        // And once a finishes, b goes.
        let decision = queue.try_start(&b, GENERATION, &[(a, PeerRollout::Idle)], patched);
        assert_eq!(
            decision,
            RolloutDecision::Proceed {
                stalled_peers: vec![]
            }
        );
        assert!(queue.waiting().is_empty());
    }

    #[test]
    fn a_released_claim_wakes_the_waiters() {
        let queue = RolloutQueue::new();
        let mut wakes = queue.subscribe();
        let a = id(NS, "a");
        let b = id(NS, "b");
        let store = |_: &InstanceId| Some(GENERATION);
        let _ = queue.try_start(&a, GENERATION, &[(b.clone(), PeerRollout::Idle)], store);
        let _ = queue.try_start(&b, GENERATION, &[(a.clone(), PeerRollout::Idle)], store);

        // a's patch failed: b must not wait for an event that never comes.
        queue.release(&a);

        assert_eq!(wakes.try_recv().ok(), Some(b.clone()));
        assert_eq!(
            queue.try_start(&b, GENERATION, &[(a, PeerRollout::Idle)], store),
            RolloutDecision::Proceed {
                stalled_peers: vec![]
            }
        );
    }

    #[test]
    fn a_waiter_leaving_the_queue_wakes_the_others() {
        let queue = RolloutQueue::new();
        let mut wakes = queue.subscribe();
        let a = id(NS, "a");
        let b = id(NS, "b");
        let c = id(NS, "c");
        let store = |_: &InstanceId| Some(GENERATION);
        let rolling_c = (c.clone(), PeerRollout::Rolling("r".to_string()));
        // a and b both wait on c; b also waits behind a.
        let _ = queue.try_start(&a, GENERATION, std::slice::from_ref(&rolling_c), store);
        let _ = queue.try_start(
            &b,
            GENERATION,
            &[rolling_c, (a.clone(), PeerRollout::Idle)],
            store,
        );
        assert_eq!(queue.waiting(), vec![a.clone(), b.clone()]);

        // a's change went away.
        queue.leave(&a);

        assert_eq!(queue.waiting(), vec![b.clone()]);
        assert_eq!(wakes.try_recv().ok(), Some(b));
    }

    #[test]
    fn leaving_when_not_queued_wakes_nobody() {
        let queue = RolloutQueue::new();
        let mut wakes = queue.subscribe();
        queue.leave(&id(NS, "a"));
        assert!(wakes.try_recv().is_err());
    }

    #[test]
    fn a_waiter_keeps_its_place_in_the_queue() {
        let queue = RolloutQueue::new();
        let a = id(NS, "a");
        let b = id(NS, "b");
        let c = id(NS, "c");
        let store = |_: &InstanceId| Some(GENERATION);
        let rolling_c = (c.clone(), PeerRollout::Rolling("r".to_string()));

        let _ = queue.try_start(&a, GENERATION, std::slice::from_ref(&rolling_c), store);
        let _ = queue.try_start(
            &b,
            GENERATION,
            &[rolling_c, (a.clone(), PeerRollout::Idle)],
            store,
        );
        // c finishes; b is woken first but a was queued before it.
        let for_b = queue.try_start(
            &b,
            GENERATION,
            &[
                (c.clone(), PeerRollout::Idle),
                (a.clone(), PeerRollout::Idle),
            ],
            store,
        );
        assert_eq!(
            for_b,
            RolloutDecision::Wait {
                blocker: a.clone(),
                reason: WaitReason::QueuedEarlier
            }
        );
        let for_a = queue.try_start(&a, GENERATION, &[(c, PeerRollout::Idle)], store);
        assert_eq!(
            for_a,
            RolloutDecision::Proceed {
                stalled_peers: vec![]
            }
        );
    }

    // ------------------------------------------------------------------
    // Event wiring
    // ------------------------------------------------------------------

    #[test]
    fn a_conflicting_deployment_change_wakes_its_waiting_peers_only() {
        let instances = vec![
            instance(NS, "a", CLUSTER),
            instance(NS, "b", CLUSTER),
            instance(NS, "unrelated", "dev"),
        ];
        let waiting = vec![id(NS, "b"), id(NS, "unrelated")];

        let got = waiters_to_wake(&id(NS, "a"), &waiting, &instances, &[], &no_providers());
        assert_eq!(got, vec![ObjectRef::new("b").within(NS)]);
    }

    #[test]
    fn nothing_is_woken_when_nobody_waits() {
        let instances = vec![instance(NS, "a", CLUSTER), instance(NS, "b", CLUSTER)];
        assert!(waiters_to_wake(&id(NS, "a"), &[], &instances, &[], &no_providers()).is_empty());
    }

    #[test]
    fn a_deleted_instance_wakes_every_waiter() {
        let instances = vec![instance(NS, "b", CLUSTER)];
        let waiting = vec![id(NS, "b")];
        let got = waiters_to_wake(&id(NS, "gone"), &waiting, &instances, &[], &no_providers());
        assert_eq!(got, vec![ObjectRef::new("b").within(NS)]);
    }

    #[test]
    fn the_deployment_filter_passes_rollout_progress_and_completion() {
        let rolling_d = rolling("b");
        let done = settled("b");
        let stalled = deployment(
            "b",
            GENERATION,
            GENERATION,
            (2, 1, 1, 1),
            ("False", "ProgressDeadlineExceeded"),
        );
        assert_ne!(
            deployment_rollout_key(&rolling_d),
            deployment_rollout_key(&done)
        );
        assert_ne!(
            deployment_rollout_key(&rolling_d),
            deployment_rollout_key(&stalled)
        );
    }

    #[test]
    fn the_deployment_filter_drops_unrelated_changes() {
        let a = settled("b");
        let mut b = a.clone();
        b.metadata.annotations = Some(BTreeMap::from([("x".to_string(), "y".to_string())]));
        assert_eq!(deployment_rollout_key(&a), deployment_rollout_key(&b));
    }

    #[test]
    fn the_pod_filter_passes_a_pod_turning_ready() {
        assert_ne!(
            pod_rollout_key(&pod("b", "b-1", false)),
            pod_rollout_key(&pod("b", "b-1", true))
        );
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    #[test]
    fn a_queued_instance_reports_the_instance_it_waits_for() {
        let decision = RolloutDecision::Wait {
            blocker: id(NS, "b"),
            reason: WaitReason::Rolling("1/2 pods ready".to_string()),
        };
        let status = RolloutStatus::from_decision(&decision);
        let condition = rollout_condition(&status).expect("a condition");
        assert_eq!(condition.r#type, CONDITION_TYPE_ROLLOUT);
        assert_eq!(condition.status, "False");
        assert_eq!(condition.reason.as_deref(), Some(REASON_ROLLOUT_QUEUED));
        let message = condition.message.unwrap_or_default();
        assert!(message.contains("dns/b"), "{message}");
    }

    #[test]
    fn rolling_past_a_stalled_peer_is_reported() {
        let decision = RolloutDecision::Proceed {
            stalled_peers: vec![id(NS, "b")],
        };
        let condition =
            rollout_condition(&RolloutStatus::from_decision(&decision)).expect("a condition");
        assert_eq!(condition.status, "True");
        assert_eq!(
            condition.reason.as_deref(),
            Some(REASON_ROLLOUT_PEER_STALLED)
        );
        assert!(condition.message.unwrap_or_default().contains("dns/b"));
    }

    #[test]
    fn a_plain_rollout_adds_no_condition() {
        let decision = RolloutDecision::Proceed {
            stalled_peers: vec![],
        };
        assert!(rollout_condition(&RolloutStatus::from_decision(&decision)).is_none());
        assert!(rollout_condition(&RolloutStatus::None).is_none());
    }

    #[test]
    fn a_queued_condition_in_status_is_recognised() {
        let mut inst = (*instance(NS, "a", CLUSTER)).clone();
        assert!(!rollout_queued(&inst));
        let decision = RolloutDecision::Wait {
            blocker: id(NS, "b"),
            reason: WaitReason::Claimed,
        };
        let condition =
            rollout_condition(&RolloutStatus::from_decision(&decision)).expect("a condition");
        inst.status = Some(crate::crd::Bind9InstanceStatus {
            conditions: vec![condition],
            ..Default::default()
        });
        assert!(rollout_queued(&inst));
    }
}
