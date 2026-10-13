// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the condition helpers in `status.rs`: `is_ready`,
//! `ready_condition_from_counts`, `conditions_changed` and the
//! `DNSZoneStatusUpdater` condition setters.

#[cfg(test)]
mod tests {
    use crate::status::{
        conditions_changed, create_condition, find_condition, is_ready,
        ready_condition_from_counts, DNSZoneStatusUpdater, ReadyCountWording,
    };
    use bindy_api::constants::{
        CONDITION_STATUS_FALSE, CONDITION_STATUS_TRUE, CONDITION_STATUS_UNKNOWN,
    };
    use bindy_api::crd::{Condition, DNSZone};
    use bindy_api::status_reasons::{
        CONDITION_TYPE_DEGRADED, CONDITION_TYPE_PROGRESSING, CONDITION_TYPE_READY,
        REASON_ALL_READY, REASON_NOT_READY, REASON_NO_CHILDREN, REASON_PARTIALLY_READY,
    };

    /// The wording the two cluster controllers use for their instances.
    const CLUSTER_WORDING: ReadyCountWording<'static> = ReadyCountWording {
        empty_reason: REASON_NO_CHILDREN,
        empty_message: "No instances found for this cluster",
        none_ready_message: "No instances are ready",
    };

    /// The wording the instance controller uses for its pods.
    const INSTANCE_WORDING: ReadyCountWording<'static> = ReadyCountWording {
        empty_reason: REASON_NOT_READY,
        empty_message: "No pods are ready",
        none_ready_message: "Waiting for pods to become ready",
    };

    fn triple(c: &Condition) -> (&str, &str, &str, &str) {
        (
            c.r#type.as_str(),
            c.status.as_str(),
            c.reason.as_deref().unwrap_or_default(),
            c.message.as_deref().unwrap_or_default(),
        )
    }

    #[test]
    fn condition_constants_have_their_kubernetes_values() {
        assert_eq!(CONDITION_TYPE_READY, "Ready");
        assert_eq!(CONDITION_TYPE_DEGRADED, "Degraded");
        assert_eq!(CONDITION_TYPE_PROGRESSING, "Progressing");
        assert_eq!(CONDITION_STATUS_TRUE, "True");
        assert_eq!(CONDITION_STATUS_FALSE, "False");
        assert_eq!(CONDITION_STATUS_UNKNOWN, "Unknown");
    }

    // ------------------------------------------------------------- is_ready

    #[test]
    fn is_ready_finds_ready_by_type_not_position() {
        let conditions = vec![
            create_condition("Pod-0", "False", "NotReady", "pod 0"),
            create_condition("Ready", "True", "AllReady", "all"),
        ];
        assert!(is_ready(&conditions));
    }

    #[test]
    fn is_ready_is_false_for_false_unknown_or_missing() {
        assert!(!is_ready(&[]));
        assert!(!is_ready(&[create_condition("Ready", "False", "x", "y")]));
        assert!(!is_ready(&[create_condition("Ready", "Unknown", "x", "y")]));
        assert!(!is_ready(&[create_condition("Degraded", "True", "x", "y")]));
    }

    // ------------------------------------------------ ready_condition_from_counts

    #[test]
    fn counts_with_cluster_wording_match_the_cluster_controllers() {
        let cases = [
            (
                0,
                0,
                "False",
                REASON_NO_CHILDREN,
                "No instances found for this cluster",
            ),
            (3, 3, "True", REASON_ALL_READY, "All 3 instances are ready"),
            (
                1,
                3,
                "False",
                REASON_PARTIALLY_READY,
                "1/3 instances are ready",
            ),
            (0, 3, "False", REASON_NOT_READY, "No instances are ready"),
        ];
        for (ready, total, status, reason, message) in cases {
            let c = ready_condition_from_counts(ready, total, "instances", &CLUSTER_WORDING);
            assert_eq!(
                triple(&c),
                (CONDITION_TYPE_READY, status, reason, message),
                "{ready}/{total}"
            );
            assert!(c.last_transition_time.is_some());
        }
    }

    #[test]
    fn counts_with_instance_wording_match_the_instance_controller() {
        let cases = [
            (0, 0, "False", REASON_NOT_READY, "No pods are ready"),
            (2, 2, "True", REASON_ALL_READY, "All 2 pods are ready"),
            (1, 2, "False", REASON_PARTIALLY_READY, "1/2 pods are ready"),
            (
                0,
                2,
                "False",
                REASON_NOT_READY,
                "Waiting for pods to become ready",
            ),
            // More ready pods than desired replicas (a scale-down in flight)
            (3, 2, "False", REASON_PARTIALLY_READY, "3/2 pods are ready"),
            // Ready pods while zero replicas are desired
            (1, 0, "False", REASON_PARTIALLY_READY, "1/0 pods are ready"),
        ];
        for (ready, total, status, reason, message) in cases {
            let c = ready_condition_from_counts(ready, total, "pods", &INSTANCE_WORDING);
            assert_eq!(
                triple(&c),
                (CONDITION_TYPE_READY, status, reason, message),
                "{ready}/{total}"
            );
        }
    }

    // ------------------------------------------------------- conditions_changed

    #[test]
    fn conditions_changed_ignores_order_and_transition_time() {
        let mut a = create_condition("Ready", "True", "R", "m");
        let b = create_condition("Degraded", "False", "D", "");
        let old = vec![a.clone(), b.clone()];
        a.last_transition_time = Some("2000-01-01T00:00:00Z".to_string());
        let new = vec![b, a];
        assert!(!conditions_changed(&old, &new));
    }

    #[test]
    fn conditions_changed_sees_status_reason_message_and_count() {
        let base = vec![create_condition("Ready", "True", "R", "m")];
        for changed in [
            vec![create_condition("Ready", "False", "R", "m")],
            vec![create_condition("Ready", "True", "Other", "m")],
            vec![create_condition("Ready", "True", "R", "other")],
            vec![create_condition("Progressing", "True", "R", "m")],
            vec![],
            vec![
                create_condition("Ready", "True", "R", "m"),
                create_condition("Degraded", "False", "D", ""),
            ],
        ] {
            assert!(conditions_changed(&base, &changed), "{changed:?}");
        }
    }

    // ------------------------------------------- DNSZoneStatusUpdater setters

    fn zone() -> DNSZone {
        serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": { "name": "example", "namespace": "dns", "generation": 1 },
            "spec": {
                "zoneName": "example.com",
                "soaRecord": {
                    "primaryNs": "ns1.example.com.",
                    "adminEmail": "admin.example.com.",
                    "serial": 1,
                    "refresh": 3600,
                    "retry": 600,
                    "expire": 604_800,
                    "negativeTtl": 300,
                },
            },
        }))
        .expect("test DNSZone deserializes")
    }

    #[test]
    fn set_degraded_writes_degraded_true_and_is_tracked() {
        let mut updater = DNSZoneStatusUpdater::new(&zone());
        updater.set_degraded("SecondaryFailed", "secondary broke");

        let c = find_condition(updater.conditions(), CONDITION_TYPE_DEGRADED).expect("set");
        assert_eq!(
            triple(c),
            ("Degraded", "True", "SecondaryFailed", "secondary broke")
        );
        assert!(updater.has_degraded_condition());
        assert_eq!(updater.degraded_reason(), Some("SecondaryFailed"));
        assert!(updater.has_changes());
    }

    #[test]
    fn set_progressing_writes_progressing_true() {
        let mut updater = DNSZoneStatusUpdater::new(&zone());
        updater.set_progressing("PrimaryReconciling", "Configuring zone on primary servers");

        let c = find_condition(updater.conditions(), CONDITION_TYPE_PROGRESSING).expect("set");
        assert_eq!(
            triple(c),
            (
                "Progressing",
                "True",
                "PrimaryReconciling",
                "Configuring zone on primary servers"
            )
        );
        assert!(!updater.has_degraded_condition());
    }

    #[test]
    fn set_not_ready_writes_ready_false() {
        let mut updater = DNSZoneStatusUpdater::new(&zone());
        updater.set_not_ready("PartialReconciliation", "1/2 pending");

        let c = find_condition(updater.conditions(), CONDITION_TYPE_READY).expect("set");
        assert_eq!(
            triple(c),
            ("Ready", "False", "PartialReconciliation", "1/2 pending")
        );
        assert!(!is_ready(updater.conditions()));
        assert!(!updater.has_degraded_condition());
    }

    #[test]
    fn setters_match_the_set_condition_literals_they_replace() {
        let mut by_setter = DNSZoneStatusUpdater::new(&zone());
        by_setter.set_degraded("D", "d");
        by_setter.set_progressing("P", "p");
        by_setter.set_not_ready("N", "n");

        let mut by_literal = DNSZoneStatusUpdater::new(&zone());
        by_literal.set_condition("Degraded", "True", "D", "d");
        by_literal.set_condition("Progressing", "True", "P", "p");
        by_literal.set_condition("Ready", "False", "N", "n");

        assert!(!conditions_changed(
            by_setter.conditions(),
            by_literal.conditions()
        ));
        assert_eq!(
            by_setter.has_degraded_condition(),
            by_literal.has_degraded_condition()
        );
    }
}
