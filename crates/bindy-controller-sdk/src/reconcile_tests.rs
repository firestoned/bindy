// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `reconcile.rs`

#[cfg(test)]
mod tests {
    use crate::metrics::{ERRORS_TOTAL, RECONCILIATION_TOTAL};
    use crate::reconcile::{
        finalizer_error, instrumented, instrumented_scheduled, scheduled_action, MAX_SCHEDULED_WAKE,
    };
    use kube::runtime::controller::Action;
    use kube::runtime::finalizer;
    use std::time::Duration;

    #[tokio::test]
    async fn a_successful_reconcile_awaits_the_next_change_and_counts_success() {
        // A kind label used by no other test, so the counter delta is ours.
        const KIND: &str = "InstrumentedOkTest";
        let before = RECONCILIATION_TOTAL
            .with_label_values(&[KIND, "success"])
            .get();

        let action = instrumented(KIND, "obj", async { Ok(()) })
            .await
            .expect("success passes through");

        // ADR-0016: no periodic resync. The next reconcile comes from a watch event.
        assert_eq!(action, Action::await_change());
        let after = RECONCILIATION_TOTAL
            .with_label_values(&[KIND, "success"])
            .get();
        assert!((after - before - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn a_failed_reconcile_is_returned_and_counted_as_an_error() {
        const KIND: &str = "InstrumentedErrTest";
        let errors_before = ERRORS_TOTAL
            .with_label_values(&[KIND, "reconcile_error"])
            .get();

        let result = instrumented(KIND, "obj", async { Err(anyhow::anyhow!("boom")) }).await;

        assert!(result.unwrap_err().to_string().contains("boom"));
        let failures = RECONCILIATION_TOTAL
            .with_label_values(&[KIND, "error"])
            .get();
        assert!((failures - 1.0).abs() < f64::EPSILON);
        let errors_after = ERRORS_TOTAL
            .with_label_values(&[KIND, "reconcile_error"])
            .get();
        assert!((errors_after - errors_before - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn a_scheduled_wake_requeues_for_exactly_that_delay() {
        const KIND: &str = "InstrumentedScheduledTest";
        let wake = Duration::from_secs(3600);

        let action = instrumented_scheduled(KIND, "obj", async move { Ok(Some(wake)) })
            .await
            .expect("success passes through");

        assert_eq!(action, Action::requeue(wake));
    }

    #[tokio::test]
    async fn no_scheduled_wake_awaits_the_next_change() {
        const KIND: &str = "InstrumentedUnscheduledTest";

        let action = instrumented_scheduled(KIND, "obj", async { Ok(None) })
            .await
            .expect("success passes through");

        assert_eq!(action, Action::await_change());
    }

    #[tokio::test]
    async fn a_failed_scheduled_reconcile_is_returned_as_an_error() {
        const KIND: &str = "InstrumentedScheduledErrTest";

        let result = instrumented_scheduled(KIND, "obj", async {
            Err::<Option<Duration>, _>(anyhow::anyhow!("rotation broke"))
        })
        .await;

        assert!(result.unwrap_err().to_string().contains("rotation broke"));
    }

    #[test]
    fn a_far_off_wake_is_capped_so_the_delay_queue_cannot_overflow() {
        let a_year = Duration::from_secs(365 * 24 * 3600);

        assert_eq!(
            scheduled_action(a_year),
            Action::requeue(MAX_SCHEDULED_WAKE)
        );
        assert_eq!(
            scheduled_action(Duration::from_secs(60)),
            Action::requeue(Duration::from_secs(60))
        );
    }

    #[test]
    fn finalizer_apply_and_cleanup_failures_keep_the_reconcile_error() {
        let apply = finalizer_error(
            "DNSZone",
            finalizer::Error::ApplyFailed(anyhow::anyhow!("apply broke").into()),
        );
        assert!(apply.to_string().contains("apply broke"));

        let cleanup = finalizer_error(
            "DNSZone",
            finalizer::Error::CleanupFailed(anyhow::anyhow!("cleanup broke").into()),
        );
        assert!(cleanup.to_string().contains("cleanup broke"));
    }

    #[test]
    fn finalizer_bookkeeping_failures_name_the_kind() {
        let unnamed = finalizer_error("ARecord", finalizer::Error::UnnamedObject);
        assert!(unnamed.to_string().contains("ARecord"), "{unnamed}");

        let invalid = finalizer_error("ARecord", finalizer::Error::InvalidFinalizer);
        assert!(invalid.to_string().contains("ARecord"), "{invalid}");
    }

    // ========== instrumented_action: reconciles that return their own Action ==========

    use crate::reconcile::instrumented_action;

    #[tokio::test]
    async fn instrumented_action_passes_the_action_through_and_counts_success() {
        const KIND: &str = "InstrumentedActionOkTest";
        let wake = Duration::from_secs(5);

        let action = instrumented_action(KIND, async move {
            Ok::<_, anyhow::Error>(Action::requeue(wake))
        })
        .await
        .expect("success passes through");

        assert_eq!(
            action,
            Action::requeue(wake),
            "the reconcile's own action is kept"
        );
        let successes = RECONCILIATION_TOTAL
            .with_label_values(&[KIND, "success"])
            .get();
        assert!((successes - 1.0).abs() < f64::EPSILON);
        let failures = RECONCILIATION_TOTAL
            .with_label_values(&[KIND, "error"])
            .get();
        assert!(failures.abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn instrumented_action_returns_the_error_unchanged_and_counts_it() {
        const KIND: &str = "InstrumentedActionErrTest";

        /// An error type that is not anyhow: the helper must not convert it.
        #[derive(Debug, PartialEq)]
        struct Custom(u8);
        const CUSTOM_CODE: u8 = 7;

        let result =
            instrumented_action(KIND, async { Err::<Action, _>(Custom(CUSTOM_CODE)) }).await;

        assert_eq!(result, Err(Custom(CUSTOM_CODE)));
        let failures = RECONCILIATION_TOTAL
            .with_label_values(&[KIND, "error"])
            .get();
        assert!((failures - 1.0).abs() < f64::EPSILON);
        let errors = ERRORS_TOTAL
            .with_label_values(&[KIND, "reconcile_error"])
            .get();
        assert!((errors - 1.0).abs() < f64::EPSILON);
        let successes = RECONCILIATION_TOTAL
            .with_label_values(&[KIND, "success"])
            .get();
        assert!(successes.abs() < f64::EPSILON);
    }
}
