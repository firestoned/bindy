// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `status.rs`

#[cfg(test)]
mod tests {
    use crate::crd::{Condition, DNSZone, DNSZoneSpec};
    use crate::reconcilers::status::{
        condition_changed, create_condition, find_condition, get_last_transition_time,
        DNSZoneStatusUpdater,
    };

    const CONDITION_TYPE_READY: &str = "Ready";
    const STATUS_TRUE: &str = "True";
    const STATUS_FALSE: &str = "False";
    const REASON_READY: &str = "AllPodsReady";
    const MESSAGE_READY: &str = "All pods are running";

    #[test]
    fn test_create_condition_basic() {
        let condition = create_condition(
            CONDITION_TYPE_READY,
            STATUS_TRUE,
            REASON_READY,
            MESSAGE_READY,
        );

        assert_eq!(condition.r#type, CONDITION_TYPE_READY);
        assert_eq!(condition.status, STATUS_TRUE);
        assert_eq!(condition.reason, Some(REASON_READY.to_string()));
        assert_eq!(condition.message, Some(MESSAGE_READY.to_string()));
        assert!(condition.last_transition_time.is_some());
    }

    #[test]
    fn test_create_condition_with_different_statuses() {
        let true_cond = create_condition("Ready", STATUS_TRUE, "Ready", "Is ready");
        assert_eq!(true_cond.status, STATUS_TRUE);

        let false_cond = create_condition("Ready", STATUS_FALSE, "NotReady", "Not ready");
        assert_eq!(false_cond.status, STATUS_FALSE);

        let unknown_cond = create_condition("Ready", "Unknown", "Unknown", "Unknown state");
        assert_eq!(unknown_cond.status, "Unknown");
    }

    #[test]
    fn test_create_condition_with_different_types() {
        let ready = create_condition("Ready", STATUS_TRUE, "Ready", "Ready");
        assert_eq!(ready.r#type, "Ready");

        let progressing = create_condition("Progressing", STATUS_TRUE, "Rolling", "Rolling out");
        assert_eq!(progressing.r#type, "Progressing");

        let degraded = create_condition("Degraded", STATUS_FALSE, "Healthy", "Healthy");
        assert_eq!(degraded.r#type, "Degraded");
    }

    #[test]
    fn test_create_condition_timestamp_is_set() {
        let condition = create_condition("Ready", STATUS_TRUE, "Ready", "Ready");

        assert!(condition.last_transition_time.is_some());

        let timestamp = condition.last_transition_time.as_ref().unwrap();
        // Should be RFC3339 format
        assert!(timestamp.contains('T'));
        assert!(timestamp.contains('Z') || timestamp.contains('+') || timestamp.contains('-'));
    }

    #[test]
    fn test_condition_changed_detects_type_change() {
        let existing = Some(create_condition("Ready", STATUS_TRUE, "Ready", "Ready"));
        let new_cond = create_condition("Progressing", STATUS_TRUE, "Ready", "Ready");

        assert!(condition_changed(&existing, &new_cond));
    }

    #[test]
    fn test_condition_changed_detects_status_change() {
        let existing = Some(create_condition("Ready", STATUS_TRUE, "Ready", "Ready"));
        let new_cond = create_condition("Ready", STATUS_FALSE, "NotReady", "Not ready");

        assert!(condition_changed(&existing, &new_cond));
    }

    #[test]
    fn test_condition_changed_detects_message_change() {
        let existing = Some(create_condition(
            "Ready",
            STATUS_TRUE,
            "Ready",
            "Old message",
        ));
        let new_cond = create_condition("Ready", STATUS_TRUE, "Ready", "New message");

        assert!(condition_changed(&existing, &new_cond));
    }

    #[test]
    fn test_condition_changed_returns_true_when_no_existing() {
        let existing = None;
        let new_cond = create_condition("Ready", STATUS_TRUE, "Ready", "Ready");

        assert!(condition_changed(&existing, &new_cond));
    }

    #[test]
    fn test_condition_unchanged_when_same() {
        let existing = Some(create_condition("Ready", STATUS_TRUE, "Ready", "Message"));
        let new_cond = create_condition("Ready", STATUS_TRUE, "DifferentReason", "Message");

        // Should be unchanged because type, status, and message are the same
        // (reason is not compared)
        assert!(!condition_changed(&existing, &new_cond));
    }

    #[test]
    fn test_get_last_transition_time_with_existing_condition() {
        let timestamp = "2025-01-01T00:00:00Z";
        let existing_conditions = vec![Condition {
            r#type: "Ready".to_string(),
            status: STATUS_TRUE.to_string(),
            reason: Some("Ready".to_string()),
            message: Some("Ready".to_string()),
            last_transition_time: Some(timestamp.to_string()),
        }];

        let result = get_last_transition_time(&existing_conditions, "Ready");
        assert_eq!(result, timestamp);
    }

    #[test]
    fn test_get_last_transition_time_without_existing_condition() {
        let existing_conditions = vec![];

        let result = get_last_transition_time(&existing_conditions, "Ready");

        // Should return current time (RFC3339 format)
        assert!(result.contains('T'));
    }

    #[test]
    fn test_get_last_transition_time_with_different_condition_type() {
        let timestamp = "2025-01-01T00:00:00Z";
        let existing_conditions = vec![Condition {
            r#type: "Progressing".to_string(),
            status: STATUS_TRUE.to_string(),
            reason: Some("Rolling".to_string()),
            message: Some("Rolling".to_string()),
            last_transition_time: Some(timestamp.to_string()),
        }];

        // Looking for "Ready" but only "Progressing" exists
        let result = get_last_transition_time(&existing_conditions, "Ready");

        // Should return current time since "Ready" doesn't exist
        assert!(result.contains('T'));
        assert_ne!(result, timestamp);
    }

    #[test]
    fn test_find_condition_returns_matching_condition() {
        let conditions = vec![
            create_condition("Ready", STATUS_TRUE, "Ready", "Ready"),
            create_condition("Progressing", STATUS_FALSE, "Complete", "Complete"),
        ];

        let result = find_condition(&conditions, "Ready");

        assert!(result.is_some());
        assert_eq!(result.unwrap().r#type, "Ready");
        assert_eq!(result.unwrap().status, STATUS_TRUE);
    }

    #[test]
    fn test_find_condition_returns_none_when_not_found() {
        let conditions = vec![create_condition("Ready", STATUS_TRUE, "Ready", "Ready")];

        let result = find_condition(&conditions, "Progressing");

        assert!(result.is_none());
    }

    #[test]
    fn test_find_condition_with_empty_list() {
        let conditions: Vec<Condition> = vec![];

        let result = find_condition(&conditions, "Ready");

        assert!(result.is_none());
    }

    #[test]
    fn test_find_condition_with_multiple_types() {
        let conditions = vec![
            create_condition("Ready", STATUS_TRUE, "Ready", "Ready"),
            create_condition("Progressing", STATUS_FALSE, "Complete", "Complete"),
            create_condition("Degraded", STATUS_FALSE, "Healthy", "Healthy"),
        ];

        let ready = find_condition(&conditions, "Ready");
        let progressing = find_condition(&conditions, "Progressing");
        let degraded = find_condition(&conditions, "Degraded");

        assert!(ready.is_some());
        assert!(progressing.is_some());
        assert!(degraded.is_some());

        assert_eq!(ready.unwrap().r#type, "Ready");
        assert_eq!(progressing.unwrap().r#type, "Progressing");
        assert_eq!(degraded.unwrap().r#type, "Degraded");
    }

    #[test]
    fn test_condition_type_field_is_type() {
        let condition = create_condition("Ready", STATUS_TRUE, "Ready", "Ready");

        // Verify that the type field uses r#type (raw identifier)
        assert_eq!(condition.r#type, "Ready");
    }

    #[test]
    fn test_condition_with_empty_strings() {
        let condition = create_condition("", "", "", "");

        assert_eq!(condition.r#type, "");
        assert_eq!(condition.status, "");
        assert_eq!(condition.reason, Some(String::new()));
        assert_eq!(condition.message, Some(String::new()));
    }

    #[test]
    fn test_condition_with_long_strings() {
        let long_message = "A".repeat(1000);
        let condition = create_condition("Ready", STATUS_TRUE, "Ready", &long_message);

        assert_eq!(condition.message, Some(long_message));
    }

    #[test]
    fn test_multiple_conditions_have_different_timestamps() {
        let cond1 = create_condition("Ready", STATUS_TRUE, "Ready", "Ready");

        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));

        let cond2 = create_condition("Ready", STATUS_TRUE, "Ready", "Ready");

        // Timestamps should be different
        assert_ne!(cond1.last_transition_time, cond2.last_transition_time);
    }

    #[test]
    fn test_condition_status_values() {
        // Test standard Kubernetes condition status values
        let true_status = create_condition("Ready", "True", "Ready", "Ready");
        assert_eq!(true_status.status, "True");

        let false_status = create_condition("Ready", "False", "NotReady", "NotReady");
        assert_eq!(false_status.status, "False");

        let unknown_status = create_condition("Ready", "Unknown", "Unknown", "Unknown");
        assert_eq!(unknown_status.status, "Unknown");
    }

    #[test]
    fn test_condition_reason_camelcase_convention() {
        // Test that reasons follow CamelCase convention
        let reasons = vec![
            "Ready",
            "NotReady",
            "DeploymentReady",
            "AllPodsRunning",
            "ReconciliationFailed",
        ];

        for reason in reasons {
            let condition = create_condition("Ready", STATUS_TRUE, reason, "Message");
            assert_eq!(condition.reason, Some(reason.to_string()));

            // Verify it's CamelCase (starts with uppercase)
            assert!(reason.chars().next().unwrap().is_uppercase());
        }
    }

    #[test]
    fn test_get_last_transition_time_with_multiple_conditions() {
        let timestamp1 = "2025-01-01T00:00:00Z";
        let timestamp2 = "2025-01-02T00:00:00Z";

        let existing_conditions = vec![
            Condition {
                r#type: "Ready".to_string(),
                status: STATUS_TRUE.to_string(),
                reason: Some("Ready".to_string()),
                message: Some("Ready".to_string()),
                last_transition_time: Some(timestamp1.to_string()),
            },
            Condition {
                r#type: "Progressing".to_string(),
                status: STATUS_FALSE.to_string(),
                reason: Some("Complete".to_string()),
                message: Some("Complete".to_string()),
                last_transition_time: Some(timestamp2.to_string()),
            },
        ];

        let ready_time = get_last_transition_time(&existing_conditions, "Ready");
        let progressing_time = get_last_transition_time(&existing_conditions, "Progressing");

        assert_eq!(ready_time, timestamp1);
        assert_eq!(progressing_time, timestamp2);
    }

    #[test]
    fn test_condition_changed_ignores_reason_change() {
        let existing = Some(create_condition(
            "Ready",
            STATUS_TRUE,
            "OldReason",
            "Message",
        ));
        let new_cond = create_condition("Ready", STATUS_TRUE, "NewReason", "Message");

        // Should NOT be changed because type, status, and message are the same
        assert!(!condition_changed(&existing, &new_cond));
    }

    #[test]
    fn test_condition_changed_ignores_timestamp_change() {
        let existing = Some(Condition {
            r#type: "Ready".to_string(),
            status: STATUS_TRUE.to_string(),
            reason: Some("Ready".to_string()),
            message: Some("Message".to_string()),
            last_transition_time: Some("2025-01-01T00:00:00Z".to_string()),
        });

        let new_cond = Condition {
            r#type: "Ready".to_string(),
            status: STATUS_TRUE.to_string(),
            reason: Some("Ready".to_string()),
            message: Some("Message".to_string()),
            last_transition_time: Some("2025-01-02T00:00:00Z".to_string()),
        };

        // Should NOT be changed because type, status, and message are the same
        assert!(!condition_changed(&existing, &new_cond));
    }

    // Helper function to create a test DNSZone
    fn create_test_dnszone(name: &str, namespace: &str) -> DNSZone {
        use crate::crd::SOARecord;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        DNSZone {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: DNSZoneSpec {
                zone_name: "example.com".to_string(),
                cluster_ref: None,
                soa_record: SOARecord {
                    primary_ns: "ns1.example.com.".to_string(),
                    admin_email: "admin.example.com.".to_string(),
                    serial: 2_025_010_101,
                    refresh: 3600,
                    retry: 1800,
                    expire: 604_800,
                    negative_ttl: 86400,
                },
                ttl: None,
                name_server_ips: None,
                records_from: None,
                bind9_instances_from: None,
            },
            status: None,
        }
    }

    #[test]
    fn test_status_updater_clear_degraded_condition_when_none_exists() {
        let dnszone = create_test_dnszone("test-zone", "dns-system");
        let mut updater = DNSZoneStatusUpdater::new(&dnszone);

        // No degraded condition exists yet
        assert!(!updater.has_degraded_condition());

        // Clear degraded condition (should set it to False)
        updater.clear_degraded_condition();

        // Should now have a Degraded=False condition
        let degraded = find_condition(updater.conditions(), "Degraded");
        assert!(degraded.is_some());
        assert_eq!(degraded.unwrap().status, "False");
        assert_eq!(
            degraded.unwrap().reason.as_deref(),
            Some("ReconcileSucceeded")
        );
    }

    #[test]
    fn test_status_updater_clear_degraded_condition_when_true() {
        let dnszone = create_test_dnszone("test-zone", "dns-system");
        let mut updater = DNSZoneStatusUpdater::new(&dnszone);

        // Set a Degraded=True condition first
        updater.set_condition(
            "Degraded",
            "True",
            "PrimaryFailed",
            "Failed to configure primaries",
        );

        // Verify it's set to True
        assert!(updater.has_degraded_condition());

        // Clear the degraded condition
        updater.clear_degraded_condition();

        // Should now be Degraded=False
        assert!(!updater.has_degraded_condition());
        let degraded = find_condition(updater.conditions(), "Degraded");
        assert!(degraded.is_some());
        assert_eq!(degraded.unwrap().status, "False");
        assert_eq!(
            degraded.unwrap().reason.as_deref(),
            Some("ReconcileSucceeded")
        );
    }

    #[test]
    fn test_status_updater_has_degraded_condition_returns_false_initially() {
        let dnszone = create_test_dnszone("test-zone", "dns-system");
        let updater = DNSZoneStatusUpdater::new(&dnszone);

        assert!(!updater.has_degraded_condition());
    }

    #[test]
    fn test_status_updater_has_degraded_condition_returns_true_when_set() {
        let dnszone = create_test_dnszone("test-zone", "dns-system");
        let mut updater = DNSZoneStatusUpdater::new(&dnszone);

        updater.set_condition("Degraded", "True", "PrimaryFailed", "Primary failed");

        assert!(updater.has_degraded_condition());
    }

    #[test]
    fn test_status_updater_has_degraded_condition_returns_false_when_false() {
        let dnszone = create_test_dnszone("test-zone", "dns-system");
        let mut updater = DNSZoneStatusUpdater::new(&dnszone);

        updater.set_condition("Degraded", "False", "Healthy", "Healthy");

        assert!(!updater.has_degraded_condition());
    }

    #[test]
    fn test_status_updater_clear_degraded_preserves_other_conditions() {
        let dnszone = create_test_dnszone("test-zone", "dns-system");
        let mut updater = DNSZoneStatusUpdater::new(&dnszone);

        // Set multiple conditions
        updater.set_condition("Ready", "True", "ReconcileSucceeded", "Zone configured");
        updater.set_condition(
            "Progressing",
            "False",
            "Complete",
            "Reconciliation complete",
        );
        updater.set_condition("Degraded", "True", "PrimaryFailed", "Primary failed");

        // Clear degraded
        updater.clear_degraded_condition();

        // Ready and Progressing should still exist
        assert!(find_condition(updater.conditions(), "Ready").is_some());
        assert!(find_condition(updater.conditions(), "Progressing").is_some());

        // Degraded should be False
        let degraded = find_condition(updater.conditions(), "Degraded");
        assert!(degraded.is_some());
        assert_eq!(degraded.unwrap().status, "False");
    }

    #[test]
    fn test_status_updater_multiple_degraded_clears() {
        let dnszone = create_test_dnszone("test-zone", "dns-system");
        let mut updater = DNSZoneStatusUpdater::new(&dnszone);

        // Set degraded multiple times and clear each time
        for i in 0..3 {
            updater.set_condition("Degraded", "True", "TestFailed", &format!("Failure {i}"));
            assert!(updater.has_degraded_condition());

            updater.clear_degraded_condition();
            assert!(!updater.has_degraded_condition());
        }

        // Should end with Degraded=False
        let degraded = find_condition(updater.conditions(), "Degraded");
        assert!(degraded.is_some());
        assert_eq!(degraded.unwrap().status, "False");
    }
}
