// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `status_helpers.rs`
//!
//! These tests document expected behavior for status calculation and updates.
//! Full implementation requires Kubernetes API mocking infrastructure.

#[cfg(test)]
mod tests {
    // ========================================================================
    // Status patch decision (instance_status_changed)
    // ========================================================================

    use crate::bind9instance::status_helpers::instance_status_changed;
    use crate::crd::{Bind9InstanceStatus, Condition};

    fn ready_condition() -> Condition {
        Condition {
            r#type: "Ready".to_string(),
            status: "True".to_string(),
            reason: Some("AllReady".to_string()),
            message: Some("All 1 pods are ready".to_string()),
            last_transition_time: Some("2025-01-01T00:00:00Z".to_string()),
        }
    }

    fn build_current_status(
        generation: Option<i64>,
        parent_generation: Option<i64>,
    ) -> Bind9InstanceStatus {
        Bind9InstanceStatus {
            conditions: vec![ready_condition()],
            observed_generation: generation,
            observed_parent_generation: parent_generation,
            ..Default::default()
        }
    }

    #[test]
    fn test_instance_status_changed_no_current_status() {
        assert!(instance_status_changed(
            None,
            &[ready_condition()],
            None,
            &[],
            Some(1),
            None
        ));
    }

    #[test]
    fn test_instance_status_changed_generation_only_change_triggers_patch() {
        // THE BUG: a spec edit that does not change conditions must still
        // trigger the patch so observedGeneration advances - otherwise
        // should_reconcile() returns true on every requeue forever.
        let current = build_current_status(Some(1), None);

        assert!(
            instance_status_changed(
                Some(&current),
                &[ready_condition()],
                None,
                &[],
                Some(2),
                None
            ),
            "generation-only change must trigger a status patch"
        );
    }

    #[test]
    fn test_instance_status_changed_parent_generation_only_change_triggers_patch() {
        // A parent cluster config change must be recorded even when the
        // instance's own conditions and generation are unchanged
        let current = build_current_status(Some(2), Some(5));

        assert!(
            instance_status_changed(
                Some(&current),
                &[ready_condition()],
                None,
                &[],
                Some(2),
                Some(6)
            ),
            "parent-generation-only change must trigger a status patch"
        );
    }

    #[test]
    fn test_instance_status_changed_unchanged_skips_patch() {
        let current = build_current_status(Some(2), Some(5));

        assert!(!instance_status_changed(
            Some(&current),
            &[ready_condition()],
            None,
            &[],
            Some(2),
            Some(5)
        ));
    }

    #[test]
    fn test_instance_status_changed_ignores_condition_order() {
        // Behaviour change: conditions are matched by type, so the same set
        // in another order is no change (the pairwise comparison saw one).
        let pod = Condition {
            r#type: "Pod-0".to_string(),
            status: "True".to_string(),
            reason: Some("Ready".to_string()),
            message: Some("Pod a-0 is ready".to_string()),
            last_transition_time: None,
        };
        let mut current = build_current_status(Some(2), Some(5));
        current.conditions = vec![ready_condition(), pod.clone()];

        assert!(!instance_status_changed(
            Some(&current),
            &[pod, ready_condition()],
            None,
            &[],
            Some(2),
            Some(5)
        ));
    }

    // ------------------------------------------------------------------
    // The encompassing Ready condition from pod counts
    // ------------------------------------------------------------------

    mod pods_ready {
        use crate::bind9instance::status_helpers::pods_ready_condition;

        fn triple(ready: usize, replicas: i32) -> (String, String, String) {
            let c = pods_ready_condition(ready, replicas);
            assert_eq!(c.r#type, "Ready");
            (
                c.status,
                c.reason.unwrap_or_default(),
                c.message.unwrap_or_default(),
            )
        }

        fn expect(status: &str, reason: &str, message: &str) -> (String, String, String) {
            (status.to_string(), reason.to_string(), message.to_string())
        }

        #[test]
        fn no_replicas_and_no_pods_is_not_ready() {
            assert_eq!(
                triple(0, 0),
                expect("False", "NotReady", "No pods are ready")
            );
        }

        #[test]
        fn replicas_but_no_ready_pod_is_waiting() {
            assert_eq!(
                triple(0, 3),
                expect("False", "NotReady", "Waiting for pods to become ready")
            );
        }

        #[test]
        fn all_pods_ready_is_ready() {
            assert_eq!(
                triple(3, 3),
                expect("True", "AllReady", "All 3 pods are ready")
            );
        }

        #[test]
        fn some_pods_ready_is_partially_ready() {
            assert_eq!(
                triple(1, 3),
                expect("False", "PartiallyReady", "1/3 pods are ready")
            );
        }

        #[test]
        fn ready_pods_after_scale_to_zero_are_partially_ready() {
            assert_eq!(
                triple(2, 0),
                expect("False", "PartiallyReady", "2/0 pods are ready")
            );
        }
    }

    // ------------------------------------------------------------------
    // ADR-0018: a queued pod-template change does not advance the observed
    // generations, so the next reconcile still applies it.
    // ------------------------------------------------------------------

    mod queued_rollouts {
        use crate::bind9instance::status_helpers::{observed_generations, ObservedGenerations};
        use crate::crd::{Bind9Instance, Bind9InstanceStatus};
        use crate::rollout::RolloutStatus;

        const OLD_GENERATION: i64 = 3;
        const NEW_GENERATION: i64 = 4;
        const OLD_PARENT: i64 = 10;
        const NEW_PARENT: i64 = 11;

        fn instance() -> Bind9Instance {
            let mut inst: Bind9Instance = serde_json::from_value(serde_json::json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "Bind9Instance",
                "metadata": {"name": "a", "namespace": "dns", "generation": NEW_GENERATION},
                "spec": {"clusterRef": "prod", "role": "primary"}
            }))
            .expect("valid Bind9Instance");
            inst.status = Some(Bind9InstanceStatus {
                observed_generation: Some(OLD_GENERATION),
                observed_parent_generation: Some(OLD_PARENT),
                ..Default::default()
            });
            inst
        }

        #[test]
        fn an_applied_change_advances_both_generations() {
            assert_eq!(
                observed_generations(&instance(), Some(NEW_PARENT), &RolloutStatus::None),
                ObservedGenerations {
                    instance: Some(NEW_GENERATION),
                    parent: Some(NEW_PARENT),
                }
            );
        }

        #[test]
        fn a_queued_change_keeps_the_previous_generations() {
            let queued = RolloutStatus::Queued("waits for dns/b".to_string());
            assert_eq!(
                observed_generations(&instance(), Some(NEW_PARENT), &queued),
                ObservedGenerations {
                    instance: Some(OLD_GENERATION),
                    parent: Some(OLD_PARENT),
                }
            );
        }

        #[test]
        fn rolling_past_a_stalled_peer_counts_as_applied() {
            let past = RolloutStatus::ProceededPastStalled("dns/b".to_string());
            assert_eq!(
                observed_generations(&instance(), Some(NEW_PARENT), &past).instance,
                Some(NEW_GENERATION)
            );
        }
    }
}
