// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `metrics.rs`

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn test_record_reconciliation_success() {
        let resource_type = "TestResource";
        let duration = Duration::from_millis(500);

        record_reconciliation_success(resource_type, duration);

        let counter = RECONCILIATION_TOTAL.with_label_values(&[resource_type, "success"]);
        assert!(counter.get() > 0.0);
        let histogram = RECONCILIATION_DURATION_SECONDS.with_label_values(&[resource_type]);
        assert!(histogram.get_sample_count() > 0);
    }

    #[test]
    fn test_record_reconciliation_error() {
        let resource_type = "TestResourceError";
        let duration = Duration::from_millis(250);

        record_reconciliation_error(resource_type, duration);

        let counter = RECONCILIATION_TOTAL.with_label_values(&[resource_type, "error"]);
        assert!(counter.get() > 0.0);
        let histogram = RECONCILIATION_DURATION_SECONDS.with_label_values(&[resource_type]);
        assert!(histogram.get_sample_count() > 0);
    }

    #[test]
    fn test_record_resource_deleted_counts_each_deletion() {
        let resource_type = "DeletedOnlyResource";
        let counter = RESOURCES_DELETED_TOTAL.with_label_values(&[resource_type]);
        let before = counter.get();

        record_resource_deleted(resource_type);
        record_resource_deleted(resource_type);

        assert!((counter.get() - before - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_gather_metrics() {
        record_reconciliation_success("GatherTest", Duration::from_millis(100));

        let metrics_text = gather_metrics().expect("gathering metrics succeeds");

        assert!(
            metrics_text.contains("bindy_firestoned_io"),
            "Metrics should contain namespace prefix"
        );
        assert!(
            metrics_text.contains("reconciliations_total"),
            "Metrics should contain reconciliation counter"
        );
    }

    // ========== Leader election metrics ==========
    //
    // LEADER_ELECTIONS_TOTAL is labelled by event only, so tests running in
    // parallel share its series: assert it grew by at least what this test
    // recorded. LEADER_STATUS is per pod, so each test uses its own pod name.

    #[test]
    fn leadership_acquired_sets_leading_and_counts_an_election() {
        let pod = "pod-acquired";
        let acquired = LEADER_ELECTIONS_TOTAL.with_label_values(&[LEADER_EVENT_ACQUIRED]);
        let before = acquired.get();

        record_leadership_acquired(pod);

        assert!(acquired.get() >= before + 1.0);
        let status = LEADER_STATUS.with_label_values(&[pod]).get();
        assert!((status - 1.0).abs() < f64::EPSILON, "status {status}");
    }

    #[test]
    fn leadership_lost_sets_following_and_counts_a_loss() {
        let pod = "pod-lost";
        let lost = LEADER_ELECTIONS_TOTAL.with_label_values(&[LEADER_EVENT_LOST]);
        record_leadership_acquired(pod);
        let before = lost.get();

        record_leadership_lost(pod);

        assert!(lost.get() >= before + 1.0);
        let status = LEADER_STATUS.with_label_values(&[pod]).get();
        assert!(status.abs() < f64::EPSILON, "status {status}");
    }

    #[test]
    fn leader_status_reports_leading_and_following() {
        let pod = "pod-no-election";

        record_leader_status(pod, true);
        let leading = LEADER_STATUS.with_label_values(&[pod]).get();
        record_leader_status(pod, false);
        let following = LEADER_STATUS.with_label_values(&[pod]).get();

        assert!((leading - 1.0).abs() < f64::EPSILON, "leading {leading}");
        assert!(following.abs() < f64::EPSILON, "following {following}");
    }

    #[test]
    fn leader_metrics_are_exported() {
        record_leadership_acquired("pod-exported");

        let text = gather_metrics().expect("gathering metrics succeeds");

        assert!(text.contains("bindy_firestoned_io_leader_elections_total"));
        assert!(text.contains("bindy_firestoned_io_leader_status"));
    }
}
