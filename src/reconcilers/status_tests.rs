// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `status.rs`

#[cfg(test)]
mod tests {
    use super::{condition_changed, create_condition, find_condition, get_last_transition_time};
    use crate::crd::Condition;

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
        let existing = Some(create_condition("Ready", STATUS_TRUE, "Ready", "Old message"));
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
        assert_ne!(
            cond1.last_transition_time,
            cond2.last_transition_time
        );
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
        let existing = Some(create_condition("Ready", STATUS_TRUE, "OldReason", "Message"));
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
}
