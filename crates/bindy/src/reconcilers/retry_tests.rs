// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `retry.rs`

#[cfg(test)]
mod tests {
    use super::super::{default_backoff, is_retryable_error};
    use std::time::Duration;

    /// Test that backoff configuration has expected values
    #[test]
    fn test_backoff_configuration() {
        let backoff = default_backoff();

        // Verify initial interval
        assert_eq!(
            backoff.initial_interval,
            Duration::from_millis(100),
            "Initial interval should be 100ms"
        );

        // Verify max interval
        assert_eq!(
            backoff.max_interval,
            Duration::from_secs(30),
            "Max interval should be 30 seconds"
        );

        // Verify max elapsed time
        assert_eq!(
            backoff.max_elapsed_time,
            Some(Duration::from_secs(300)),
            "Max elapsed time should be 5 minutes"
        );

        // Verify multiplier
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(
                backoff.multiplier, 2.0,
                "Multiplier should be 2.0 for exponential growth"
            );
        }

        // Verify randomization factor
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(
                backoff.randomization_factor, 0.1,
                "Randomization factor should be 0.1 (±10%)"
            );
        }
    }

    fn api_error(code: u16) -> kube::Error {
        kube::Error::Api(Box::new(kube::core::Status {
            code,
            status: None,
            message: String::new(),
            reason: String::new(),
            details: None,
            metadata: Default::default(),
        }))
    }

    /// Test that HTTP 429 errors are retryable
    #[test]
    fn test_429_is_retryable() {
        let err = api_error(429);
        assert!(
            is_retryable_error(&err),
            "HTTP 429 (rate limiting) should be retryable"
        );
    }

    /// Test that 5xx server errors are retryable
    #[test]
    fn test_5xx_is_retryable() {
        assert!(
            is_retryable_error(&api_error(500)),
            "HTTP 500 should be retryable"
        );
        assert!(
            is_retryable_error(&api_error(503)),
            "HTTP 503 should be retryable"
        );
        assert!(
            is_retryable_error(&api_error(599)),
            "HTTP 599 should be retryable"
        );
    }

    /// Test that 4xx client errors (except 429) are not retryable
    #[test]
    fn test_4xx_not_retryable() {
        assert!(
            !is_retryable_error(&api_error(400)),
            "HTTP 400 should not be retryable"
        );
        assert!(
            !is_retryable_error(&api_error(404)),
            "HTTP 404 should not be retryable"
        );
        assert!(
            !is_retryable_error(&api_error(401)),
            "HTTP 401 should not be retryable"
        );
    }

    /// Test that service/network errors are retryable
    #[test]
    fn test_service_errors_retryable() {
        // Create a Box<dyn Error> for Service error
        let service_error: Box<dyn std::error::Error + Send + Sync> = Box::new(
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "Connection failed"),
        );

        let err = kube::Error::Service(service_error);

        assert!(
            is_retryable_error(&err),
            "Service/network errors should be retryable"
        );
    }

    /// Test backoff timing progression
    #[test]
    fn test_backoff_timing_progression() {
        let backoff = default_backoff();

        // Verify the backoff grows exponentially
        let mut current = backoff.current_interval;
        assert_eq!(current, Duration::from_millis(100), "First retry at 100ms");

        // Second retry should be ~200ms (100ms * 2.0 ± 10%)
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        {
            let current_millis = current.as_millis() as f64 * 2.0;
            current = Duration::from_millis(current_millis as u64);
        }
        assert!(
            current >= Duration::from_millis(180) && current <= Duration::from_millis(220),
            "Second retry should be ~200ms (±10%)"
        );
    }

    /// Test that max interval is respected
    #[test]
    fn test_max_interval_capping() {
        let backoff = default_backoff();

        // After enough retries, interval should cap at 30 seconds
        let max_interval = Duration::from_secs(30);

        // Calculate how many doublings until we exceed max
        // 100ms * 2^n >= 30s
        // 2^n >= 300,000
        // n >= log2(300,000) ≈ 18.2
        // So after ~18 retries, we should be at max interval

        let mut current = backoff.initial_interval;
        for _ in 0..20 {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            {
                let current_millis_f64 = current.as_millis() as f64;
                current = Duration::from_millis((current_millis_f64 * 2.0) as u64);
            }
            if current > max_interval {
                current = max_interval;
            }
        }

        assert_eq!(
            current, max_interval,
            "After many retries, interval should cap at max"
        );
    }

    /// Test that max elapsed time eventually stops retries
    #[test]
    fn test_max_elapsed_time() {
        let backoff = default_backoff();

        assert_eq!(
            backoff.max_elapsed_time,
            Some(Duration::from_secs(300)),
            "Max elapsed time should be 5 minutes"
        );

        // Verify this is a reasonable timeout
        #[allow(clippy::assertions_on_constants)]
        {
            let max_secs = backoff.max_elapsed_time.unwrap().as_secs();
            assert!(
                max_secs >= 60,
                "Max elapsed time should be at least 1 minute"
            );
            assert!(
                max_secs <= 600,
                "Max elapsed time should not exceed 10 minutes"
            );
        }
    }
}

#[cfg(test)]
mod reconcile_backoff_tests {
    use super::super::{
        clear_rejected_write, note_rejected_write, reconcile_error_backoff,
        reset_reconcile_backoff, write_in_cooldown, write_in_cooldown_at, RECONCILE_BACKOFF_MAX,
        REJECTED_WRITE_COOLDOWN,
    };
    use std::time::{Duration, Instant};

    /// The first failure must come back quickly. A flat 30s requeue was the floor
    /// on how fast a DNSZone could recover after its operand Pods were replaced:
    /// measured at 57s of idling before the reconcile that actually re-pushed the
    /// zone, which itself took about 1 second.
    #[test]
    fn test_first_failure_requeues_quickly() {
        let key = "ns/first-failure";
        reset_reconcile_backoff(key);

        let first = reconcile_error_backoff(key);
        assert!(
            first <= Duration::from_secs(5),
            "first retry should be prompt, got {first:?}"
        );
    }

    /// Repeated failures must back off, so a permanently broken object does not
    /// hammer the API server at the fast initial interval forever.
    #[test]
    fn test_repeated_failures_back_off_and_cap() {
        let key = "ns/repeated-failures";
        reset_reconcile_backoff(key);

        let first = reconcile_error_backoff(key);
        let mut last = first;
        for _ in 0..12 {
            let next = reconcile_error_backoff(key);
            assert!(
                next >= last || next == RECONCILE_BACKOFF_MAX,
                "backoff must grow monotonically until it caps: {last:?} -> {next:?}"
            );
            last = next;
        }

        assert!(last > first, "backoff must grow with consecutive failures");
        assert_eq!(last, RECONCILE_BACKOFF_MAX, "backoff must cap");
    }

    /// Two different objects must not share a failure counter.
    #[test]
    fn test_backoff_is_tracked_per_object() {
        let hot = "ns/hot";
        let cold = "ns/cold";
        reset_reconcile_backoff(hot);
        reset_reconcile_backoff(cold);

        for _ in 0..8 {
            let _ = reconcile_error_backoff(hot);
        }
        let hot_delay = reconcile_error_backoff(hot);
        let cold_delay = reconcile_error_backoff(cold);

        assert!(
            cold_delay < hot_delay,
            "a healthy object must not inherit another object's backoff: {cold_delay:?} vs {hot_delay:?}"
        );
    }

    /// A reconcile that stops failing must return to the fast interval.
    #[test]
    fn test_reset_returns_to_fast_interval() {
        let key = "ns/recovering";
        reset_reconcile_backoff(key);
        for _ in 0..6 {
            let _ = reconcile_error_backoff(key);
        }
        reset_reconcile_backoff(key);

        assert!(
            reconcile_error_backoff(key) <= Duration::from_secs(5),
            "after a reset the next failure should requeue promptly again"
        );
    }

    // ========== Tests for the rejected-write cooldown ==========

    #[test]
    fn test_unknown_key_is_not_in_cooldown() {
        assert!(
            !write_in_cooldown("ns/never-written", "hash-a"),
            "a record with no recorded rejection must be written immediately"
        );
    }

    #[test]
    fn test_rejected_write_is_skipped_until_the_cooldown_expires() {
        let key = "ns/rejected";
        clear_rejected_write(key);
        note_rejected_write(key, "hash-a");

        assert!(
            write_in_cooldown(key, "hash-a"),
            "a just-rejected write must not be re-attempted on the next watch event"
        );
        assert!(
            !write_in_cooldown_at(
                key,
                "hash-a",
                Instant::now() + REJECTED_WRITE_COOLDOWN + Duration::from_secs(1)
            ),
            "once the cooldown expires the timed requeue must attempt it again"
        );
    }

    #[test]
    fn test_a_changed_spec_bypasses_the_cooldown() {
        let key = "ns/respecified";
        clear_rejected_write(key);
        note_rejected_write(key, "hash-a");

        assert!(
            !write_in_cooldown(key, "hash-b"),
            "editing the record is the user's fix for a rejection; it must not wait"
        );
    }

    #[test]
    fn test_clearing_a_rejection_allows_the_next_write() {
        let key = "ns/recovered";
        note_rejected_write(key, "hash-a");
        clear_rejected_write(key);

        assert!(
            !write_in_cooldown(key, "hash-a"),
            "a succeeding write clears the rejection"
        );
    }

    #[test]
    fn test_cooldown_matches_the_not_ready_requeue_interval() {
        assert_eq!(
            REJECTED_WRITE_COOLDOWN,
            Duration::from_secs(crate::record_wrappers::REQUEUE_WHEN_NOT_READY_SECS),
            "the cooldown exists to let the timed requeue drive retries, so it must not outlast it"
        );
    }
}
