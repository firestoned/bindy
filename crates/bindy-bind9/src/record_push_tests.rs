// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `record_push.rs`

#[cfg(test)]
mod tests {
    // ========================================================================
    // Record replay after a BIND9 pod or Deployment is wiped (issue #486)
    // ========================================================================

    mod record_replay {
        use crate::record_push::RecordReplayOutcome;

        #[test]
        fn test_replay_outcome_default_is_complete() {
            // A zone with no records has nothing to replay and must not be
            // held back from Ready.
            let outcome = RecordReplayOutcome::default();

            assert!(outcome.is_complete());
            assert_eq!(outcome.attempted, 0);
            assert_eq!(outcome.succeeded, 0);
        }

        #[test]
        fn test_replay_outcome_with_failures_is_not_complete() {
            // Any record that did not reach BIND9 must keep the zone Degraded:
            // the server is authoritative and would answer NXDOMAIN for it.
            let outcome = RecordReplayOutcome {
                attempted: 3,
                succeeded: 2,
                skipped: 0,
                failures: vec!["ARecord default/www: connection refused".to_string()],
            };

            assert!(!outcome.is_complete());
        }

        #[test]
        fn test_replay_outcome_summary_reports_progress_on_success() {
            let outcome = RecordReplayOutcome {
                attempted: 4,
                succeeded: 4,
                skipped: 0,
                failures: vec![],
            };

            let summary = outcome.summary("example.com");

            assert!(summary.contains("4/4"), "summary was: {summary}");
            assert!(summary.contains("example.com"), "summary was: {summary}");
        }

        #[test]
        fn test_replay_outcome_summary_names_the_failures() {
            // The message lands in the Degraded condition, so it has to say
            // which records are missing from the zone.
            let outcome = RecordReplayOutcome {
                attempted: 2,
                succeeded: 1,
                skipped: 0,
                failures: vec!["ARecord default/www: connection refused".to_string()],
            };

            let summary = outcome.summary("example.com");

            assert!(summary.contains("1/2"), "summary was: {summary}");
            assert!(
                summary.contains("ARecord default/www"),
                "summary was: {summary}"
            );
            assert!(
                summary.contains("connection refused"),
                "summary was: {summary}"
            );
        }

        #[test]
        fn test_replay_outcome_counts_skipped_records_as_done() {
            // A record deleted while the zone was being recreated is not
            // missing data: it must not keep the zone Degraded.
            let outcome = RecordReplayOutcome {
                attempted: 3,
                succeeded: 2,
                skipped: 1,
                failures: vec![],
            };

            assert!(outcome.is_complete());
            assert!(outcome.summary("example.com").contains("3/3"));
        }

        #[test]
        fn test_should_replay_a_live_record() {
            let meta = k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta::default();

            assert!(crate::record_push::should_replay(&meta));
        }

        #[test]
        fn test_should_not_replay_a_terminating_record() {
            // Replaying a record whose finalizer is running would re-publish
            // the RRset after the finalizer deleted it (load test rc.2:
            // records still served after deletion).
            let meta = k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                deletion_timestamp: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                    k8s_openapi::jiff::Timestamp::now(),
                )),
                ..Default::default()
            };

            assert!(!crate::record_push::should_replay(&meta));
        }
    }
}
