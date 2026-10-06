// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for record reconciliation wrapper helpers.

#[cfg(test)]
mod tests {
    use super::super::record_wrappers::*;
    use crate::crd::{Condition, RecordStatus};
    use bindy_controller_sdk::retry::{RECONCILE_BACKOFF_INITIAL, REJECTED_WRITE_COOLDOWN};
    use kube::runtime::controller::Action;
    use std::time::Duration;

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

    // ========== Tests for action_for_outcome() (ADR-0016) ==========

    fn arecord(name: &str) -> crate::crd::ARecord {
        serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "ARecord",
            "metadata": {"name": name, "namespace": "outcome-tests"},
            "spec": {"name": "www", "ipv4Addresses": ["192.0.2.1"]}
        }))
        .expect("valid ARecord fixture")
    }

    fn reset(record: &crate::crd::ARecord) {
        bindy_controller_sdk::retry::reset_reconcile_backoff(
            &bindy_controller_sdk::error::backoff_key(record),
        );
    }

    #[test]
    fn a_published_record_awaits_the_next_change() {
        let record = arecord("published");
        assert_eq!(
            action_for_outcome(&record, &RecordOutcome::Published),
            Action::await_change(),
            "no periodic resync: a Ready record is not reconciled again until something changes"
        );
    }

    #[test]
    fn every_wait_on_another_object_awaits_the_next_change() {
        let record = arecord("waiting");
        for reason in [
            REASON_NOT_SELECTED,
            REASON_ZONE_NOT_FOUND,
            REASON_ZONE_NOT_CONFIGURED,
            REASON_NO_PRIMARY_INSTANCES,
        ] {
            assert_eq!(
                action_for_outcome(&record, &RecordOutcome::Waiting { reason }),
                Action::await_change(),
                "{reason} is woken by the awaited object's event, not by a timer"
            );
        }
    }

    #[test]
    fn a_failure_retries_on_the_per_object_backoff() {
        let record = arecord("failed");
        reset(&record);

        let first = action_for_outcome(
            &record,
            &RecordOutcome::Failed {
                reason: REASON_INSTANCE_FILTER_ERROR,
            },
        );
        let second = action_for_outcome(
            &record,
            &RecordOutcome::Failed {
                reason: REASON_INSTANCE_FILTER_ERROR,
            },
        );

        assert_eq!(first, Action::requeue(RECONCILE_BACKOFF_INITIAL));
        assert_eq!(second, Action::requeue(RECONCILE_BACKOFF_INITIAL * 2));
    }

    #[test]
    fn a_rejected_write_retries_no_sooner_than_the_cooldown() {
        let record = arecord("rejected");
        reset(&record);

        assert_eq!(
            action_for_outcome(&record, &RecordOutcome::WriteRejected),
            Action::requeue(REJECTED_WRITE_COOLDOWN)
        );
    }

    #[test]
    fn a_reconcile_inside_the_cooldown_requeues_for_what_remains() {
        let record = arecord("cooling");
        let remaining = Duration::from_secs(7);

        assert_eq!(
            action_for_outcome(&record, &RecordOutcome::CoolingDown { remaining }),
            Action::requeue(remaining)
        );
    }

    #[test]
    fn convergence_after_failures_resets_the_backoff() {
        let record = arecord("recovered");
        reset(&record);
        let failed = RecordOutcome::Failed {
            reason: REASON_INSTANCE_FILTER_ERROR,
        };
        let _ = action_for_outcome(&record, &failed);
        let _ = action_for_outcome(&record, &failed);

        let _ = action_for_outcome(&record, &RecordOutcome::Published);

        assert_eq!(
            action_for_outcome(&record, &failed),
            Action::requeue(RECONCILE_BACKOFF_INITIAL)
        );
    }

    #[test]
    fn only_a_published_record_is_ready() {
        assert!(RecordOutcome::Published.is_ready());
        assert!(!RecordOutcome::WriteRejected.is_ready());
        assert!(!RecordOutcome::Waiting {
            reason: REASON_NOT_SELECTED
        }
        .is_ready());
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
    }
}
