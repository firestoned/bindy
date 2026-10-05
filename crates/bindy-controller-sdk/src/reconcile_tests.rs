// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `reconcile.rs`

#[cfg(test)]
mod tests {
    use crate::metrics::{ERRORS_TOTAL, RECONCILIATION_TOTAL};
    use crate::reconcile::{finalizer_error, instrumented};
    use crate::requeue::REQUEUE_WHEN_READY_SECS;
    use kube::runtime::controller::Action;
    use kube::runtime::finalizer;
    use std::time::Duration;

    #[tokio::test]
    async fn a_successful_reconcile_requeues_on_the_ready_interval_and_counts_success() {
        // A kind label used by no other test, so the counter delta is ours.
        const KIND: &str = "InstrumentedOkTest";
        let before = RECONCILIATION_TOTAL
            .with_label_values(&[KIND, "success"])
            .get();

        let action = instrumented(KIND, "obj", async { Ok(()) })
            .await
            .expect("success passes through");

        assert_eq!(
            action,
            Action::requeue(Duration::from_secs(REQUEUE_WHEN_READY_SECS))
        );
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
}
