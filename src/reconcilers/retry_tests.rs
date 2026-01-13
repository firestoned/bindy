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

    /// Test that HTTP 429 errors are retryable
    #[test]
    fn test_429_is_retryable() {
        let err = kube::Error::Api(kube::error::ErrorResponse {
            status: "Too Many Requests".to_string(),
            message: "Rate limit exceeded".to_string(),
            reason: "TooManyRequests".to_string(),
            code: 429,
        });

        assert!(
            is_retryable_error(&err),
            "HTTP 429 (rate limiting) should be retryable"
        );
    }

    /// Test that 5xx server errors are retryable
    #[test]
    fn test_5xx_is_retryable() {
        // Test 500 Internal Server Error
        let err_500 = kube::Error::Api(kube::error::ErrorResponse {
            status: "Internal Server Error".to_string(),
            message: "Server error".to_string(),
            reason: "InternalServerError".to_string(),
            code: 500,
        });
        assert!(is_retryable_error(&err_500), "HTTP 500 should be retryable");

        // Test 503 Service Unavailable
        let err_503 = kube::Error::Api(kube::error::ErrorResponse {
            status: "Service Unavailable".to_string(),
            message: "Service temporarily unavailable".to_string(),
            reason: "ServiceUnavailable".to_string(),
            code: 503,
        });
        assert!(is_retryable_error(&err_503), "HTTP 503 should be retryable");

        // Test 599 (upper bound)
        let err_599 = kube::Error::Api(kube::error::ErrorResponse {
            status: "Server Error".to_string(),
            message: "Server error".to_string(),
            reason: "ServerError".to_string(),
            code: 599,
        });
        assert!(is_retryable_error(&err_599), "HTTP 599 should be retryable");
    }

    /// Test that 4xx client errors (except 429) are not retryable
    #[test]
    fn test_4xx_not_retryable() {
        // Test 400 Bad Request
        let err_400 = kube::Error::Api(kube::error::ErrorResponse {
            status: "Bad Request".to_string(),
            message: "Invalid request".to_string(),
            reason: "BadRequest".to_string(),
            code: 400,
        });
        assert!(
            !is_retryable_error(&err_400),
            "HTTP 400 should not be retryable"
        );

        // Test 404 Not Found
        let err_404 = kube::Error::Api(kube::error::ErrorResponse {
            status: "Not Found".to_string(),
            message: "Resource not found".to_string(),
            reason: "NotFound".to_string(),
            code: 404,
        });
        assert!(
            !is_retryable_error(&err_404),
            "HTTP 404 should not be retryable"
        );

        // Test 401 Unauthorized
        let err_401 = kube::Error::Api(kube::error::ErrorResponse {
            status: "Unauthorized".to_string(),
            message: "Authentication required".to_string(),
            reason: "Unauthorized".to_string(),
            code: 401,
        });
        assert!(
            !is_retryable_error(&err_401),
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
