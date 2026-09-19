// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for record reconciliation wrapper helpers.

#[cfg(test)]
mod tests {
    use super::super::record_wrappers::*;
    use crate::crd::{Condition, RecordStatus};

    // Helper to create a condition
    fn create_condition(condition_type: &str, status: &str) -> Condition {
        Condition {
            r#type: condition_type.to_string(),
            status: status.to_string(),
            reason: Some("TestReason".to_string()),
            message: Some("Test message".to_string()),
            last_transition_time: Some("2024-01-01T00:00:00Z".to_string()),
        }
    }

    // Helper to create RecordStatus
    fn create_status(conditions: Vec<Condition>) -> RecordStatus {
        RecordStatus {
            observed_generation: Some(1),
            conditions,
            last_updated: Some("2024-01-01T00:00:00Z".to_string()),
            #[allow(deprecated)]
            zone: None,
            zone_ref: None,
            record_hash: Some("hash123".to_string()),
            addresses: None,
            published_name: None,
        }
    }

    // ========== Tests for is_resource_ready() ==========

    #[test]
    fn test_is_resource_ready_with_ready_condition() {
        // Arrange: Status with Ready=True condition
        let status = Some(create_status(vec![create_condition(
            CONDITION_TYPE_READY,
            CONDITION_STATUS_TRUE,
        )]));

        // Act
        let is_ready = is_resource_ready(&status);

        // Assert
        assert!(
            is_ready,
            "Should be ready when condition type=Ready and status=True"
        );
    }

    #[test]
    fn test_is_resource_ready_with_ready_false() {
        // Arrange: Status with Ready=False condition
        let status = Some(create_status(vec![create_condition(
            CONDITION_TYPE_READY,
            "False",
        )]));

        // Act
        let is_ready = is_resource_ready(&status);

        // Assert
        assert!(!is_ready, "Should not be ready when condition status=False");
    }

    #[test]
    fn test_is_resource_ready_with_wrong_condition_type() {
        // Arrange: Status with different condition type
        let status = Some(create_status(vec![create_condition(
            "Progressing",
            CONDITION_STATUS_TRUE,
        )]));

        // Act
        let is_ready = is_resource_ready(&status);

        // Assert
        assert!(
            !is_ready,
            "Should not be ready when condition type is not 'Ready'"
        );
    }

    #[test]
    fn test_is_resource_ready_with_empty_conditions() {
        // Arrange: Status with no conditions
        let status = Some(create_status(vec![]));

        // Act
        let is_ready = is_resource_ready(&status);

        // Assert
        assert!(
            !is_ready,
            "Should not be ready when conditions list is empty"
        );
    }

    #[test]
    fn test_is_resource_ready_with_none_status() {
        // Arrange: No status
        let status = None;

        // Act
        let is_ready = is_resource_ready(&status);

        // Assert
        assert!(!is_ready, "Should not be ready when status is None");
    }

    #[test]
    fn test_is_resource_ready_with_multiple_conditions() {
        // Arrange: Status with multiple conditions, first one is Ready=True
        let status = Some(create_status(vec![
            create_condition(CONDITION_TYPE_READY, CONDITION_STATUS_TRUE),
            create_condition("Progressing", "False"),
        ]));

        // Act
        let is_ready = is_resource_ready(&status);

        // Assert
        assert!(
            is_ready,
            "Should be ready when first condition is Ready=True"
        );
    }

    #[test]
    fn test_is_resource_ready_checks_first_condition_only() {
        // Arrange: Status with Ready=False as first, Ready=True as second
        let status = Some(create_status(vec![
            create_condition(CONDITION_TYPE_READY, "False"),
            create_condition(CONDITION_TYPE_READY, CONDITION_STATUS_TRUE),
        ]));

        // Act
        let is_ready = is_resource_ready(&status);

        // Assert
        assert!(!is_ready, "Should check only first condition (Ready=False)");
    }

    // ========== Tests for requeue_based_on_readiness() ==========

    #[test]
    fn test_requeue_based_on_readiness_when_ready() {
        // Arrange
        let is_ready = true;

        // Act
        let action = requeue_based_on_readiness(is_ready);

        // Assert
        // Action doesn't provide accessors, so we verify via Debug format
        let debug_str = format!("{action:?}");
        assert!(
            debug_str.contains("300s"),
            "Expected 300s requeue duration, got: {debug_str}"
        );
    }

    #[test]
    fn test_requeue_based_on_readiness_when_not_ready() {
        // Arrange
        let is_ready = false;

        // Act
        let action = requeue_based_on_readiness(is_ready);

        // Assert
        let debug_str = format!("{action:?}");
        assert!(
            debug_str.contains("30s"),
            "Expected 30s requeue duration, got: {debug_str}"
        );
    }

    #[test]
    fn test_requeue_intervals_match_constants() {
        // Verify the constants match expected durations
        assert_eq!(
            REQUEUE_WHEN_READY_SECS, 300,
            "Ready requeue should be 5 minutes (300 seconds)"
        );
        assert_eq!(
            REQUEUE_WHEN_NOT_READY_SECS, 30,
            "Not ready requeue should be 30 seconds"
        );
    }

    // ========== Tests for constants ==========

    #[test]
    fn test_condition_type_ready_constant() {
        assert_eq!(CONDITION_TYPE_READY, "Ready");
    }

    #[test]
    fn test_condition_status_true_constant() {
        assert_eq!(CONDITION_STATUS_TRUE, "True");
    }

    #[test]
    fn test_error_type_reconcile_constant() {
        assert_eq!(ERROR_TYPE_RECONCILE, "reconcile_error");
    }

    // ========== Tests for ready_state() ==========

    #[test]
    fn test_ready_state_reports_ready() {
        // Arrange
        let status = Some(create_status(vec![create_condition(
            CONDITION_TYPE_READY,
            CONDITION_STATUS_TRUE,
        )]));

        // Act
        let state = ready_state(&status);

        // Assert
        assert_eq!(state, ReadyState::Ready);
    }

    #[test]
    fn test_ready_state_carries_reason_and_message_when_not_ready() {
        // Arrange: the shape the record reconciler writes when an add fails
        let mut condition = create_condition(CONDITION_TYPE_READY, "False");
        condition.reason = Some("ReconcileFailed".to_string());
        condition.message = Some("Failed to add record to zone: Refused".to_string());
        let status = Some(create_status(vec![condition]));

        // Act
        let state = ready_state(&status);

        // Assert
        assert_eq!(
            state,
            ReadyState::NotReady {
                reason: "ReconcileFailed",
                message: "Failed to add record to zone: Refused",
            },
            "the log line must be able to name why the record is not ready"
        );
    }

    #[test]
    fn test_ready_state_substitutes_placeholders_for_absent_reason_and_message() {
        // Arrange: reason and message are both optional in the CRD
        let mut condition = create_condition(CONDITION_TYPE_READY, "False");
        condition.reason = None;
        condition.message = None;
        let status = Some(create_status(vec![condition]));

        // Act
        let state = ready_state(&status);

        // Assert
        assert_eq!(
            state,
            ReadyState::NotReady {
                reason: UNKNOWN_CONDITION_FIELD,
                message: UNKNOWN_CONDITION_FIELD,
            }
        );
    }

    #[test]
    fn test_ready_state_is_unknown_without_a_ready_condition() {
        // Arrange
        let status = Some(create_status(vec![create_condition(
            "Progressing",
            CONDITION_STATUS_TRUE,
        )]));

        // Act / Assert
        assert_eq!(ready_state(&status), ReadyState::Unknown);
    }

    #[test]
    fn test_ready_state_is_unknown_for_empty_conditions_and_no_status() {
        // Arrange / Act / Assert
        assert_eq!(
            ready_state(&Some(create_status(vec![]))),
            ReadyState::Unknown
        );
        assert_eq!(ready_state(&None), ReadyState::Unknown);
    }

    #[test]
    fn test_ready_state_finds_the_ready_condition_in_any_position() {
        // Arrange: Ready is not first. A status writer is free to order these,
        // so position must not decide readiness.
        let status = Some(create_status(vec![
            create_condition("Progressing", "False"),
            create_condition(CONDITION_TYPE_READY, CONDITION_STATUS_TRUE),
        ]));

        // Act / Assert
        assert_eq!(ready_state(&status), ReadyState::Ready);
        assert!(
            is_resource_ready(&status),
            "is_resource_ready must agree with ready_state"
        );
    }

    // NOTE: The generate_record_wrapper! macro cannot be directly unit tested
    // because it generates code at compile time. Instead:
    //
    // 1. The macro is tested through its usage in reconcilers/records.rs where it
    //    generates wrapper functions for all 9 DNS record types
    //
    // 2. Integration tests in tests/ directory verify the generated wrappers work
    //    correctly for each record type
    //
    // 3. The macro's output (generated functions) is tested indirectly through:
    //    - reconcile_arecord_wrapper, reconcile_aaaarecord_wrapper, etc.
    //    - These are tested in reconcilers/records_tests.rs
    //
    // 4. Manual verification can be done with `cargo expand` to inspect generated code
    //
    // If we need explicit macro testing, we would create a test module that invokes
    // the macro with test types and verifies the generated function signatures and
    // behavior, but this is typically overkill for procedural generation like this.
}
