// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Retry logic with exponential backoff for Kubernetes API calls.
//!
//! This module provides utilities for retrying transient API errors (429, 5xx)
//! with exponential backoff, while failing fast on permanent errors (4xx client errors).

use anyhow::Result;
use rand::Rng;
use reqwest::StatusCode;
use std::time::{Duration, Instant};
use tracing::{debug, error, warn};

/// Maximum total time to spend retrying (5 minutes)
const MAX_ELAPSED_TIME_SECS: u64 = 300;

/// Initial retry interval (100ms)
const INITIAL_INTERVAL_MILLIS: u64 = 100;

/// Maximum interval between retries (30 seconds)
const MAX_INTERVAL_SECS: u64 = 30;

/// Backoff multiplier (exponential growth factor)
const BACKOFF_MULTIPLIER: f64 = 2.0;

/// Randomization factor to prevent thundering herd (±10%)
const RANDOMIZATION_FACTOR: f64 = 0.1;

/// HTTP retry initial interval (50ms) - faster than Kubernetes API
const HTTP_INITIAL_INTERVAL_MILLIS: u64 = 50;

/// HTTP retry maximum interval (10 seconds) - shorter than Kubernetes API
const HTTP_MAX_INTERVAL_SECS: u64 = 10;

/// HTTP retry maximum elapsed time (2 minutes) - shorter than Kubernetes API
const HTTP_MAX_ELAPSED_TIME_SECS: u64 = 120;

/// Simple exponential backoff implementation.
///
/// Provides exponential backoff with randomization (jitter) to prevent thundering herd.
pub struct ExponentialBackoff {
    /// Current interval duration
    pub current_interval: Duration,
    /// Initial interval duration (stored for potential reset functionality)
    #[allow(dead_code)]
    pub initial_interval: Duration,
    /// Maximum interval duration
    pub max_interval: Duration,
    /// Maximum total elapsed time
    pub max_elapsed_time: Option<Duration>,
    /// Backoff multiplier (typically 2.0 for doubling)
    pub multiplier: f64,
    /// Randomization factor (e.g., 0.1 for ±10%)
    pub randomization_factor: f64,
    /// Start time for tracking total elapsed time
    start_time: Instant,
}

impl ExponentialBackoff {
    /// Create a new exponential backoff with specified parameters.
    fn new(
        initial_interval: Duration,
        max_interval: Duration,
        max_elapsed_time: Option<Duration>,
        multiplier: f64,
        randomization_factor: f64,
    ) -> Self {
        Self {
            current_interval: initial_interval,
            initial_interval,
            max_interval,
            max_elapsed_time,
            multiplier,
            randomization_factor,
            start_time: Instant::now(),
        }
    }

    /// Get the next backoff interval, or None if max elapsed time exceeded.
    pub fn next_backoff(&mut self) -> Option<Duration> {
        // Check if we've exceeded max elapsed time
        if let Some(max_elapsed) = self.max_elapsed_time {
            if self.start_time.elapsed() >= max_elapsed {
                return None;
            }
        }

        // Get current interval with jitter
        let interval = self.current_interval;
        let jittered = self.apply_jitter(interval);

        // Calculate next interval (exponential growth)
        let next = interval.as_secs_f64() * self.multiplier;
        self.current_interval = Duration::from_secs_f64(next).min(self.max_interval);

        Some(jittered)
    }

    /// Apply randomization (jitter) to an interval.
    fn apply_jitter(&self, interval: Duration) -> Duration {
        if self.randomization_factor == 0.0 {
            return interval;
        }

        let secs = interval.as_secs_f64();
        let delta = secs * self.randomization_factor;
        let min = secs - delta;
        let max = secs + delta;

        let mut rng = rand::thread_rng();
        let jittered = rng.gen_range(min..=max);

        Duration::from_secs_f64(jittered.max(0.0))
    }
}

/// Create default exponential backoff configuration for Kubernetes API retries.
///
/// # Configuration
///
/// - **Initial interval**: 100ms
/// - **Max interval**: 30 seconds
/// - **Max elapsed time**: 5 minutes total
/// - **Multiplier**: 2.0 (exponential growth)
/// - **Randomization**: ±10% (prevents thundering herd)
///
/// # Retry Schedule
///
/// With these settings, retries occur at approximately:
///
/// 1. 100ms
/// 2. 200ms
/// 3. 400ms
/// 4. 800ms
/// 5. 1.6s
/// 6. 3.2s
/// 7. 6.4s
/// 8. 12.8s
/// 9. 25.6s
/// 10. 30s (capped at max interval)
///     11-30. 30s intervals until 5 minutes elapsed
///
/// # Returns
///
/// Configured `ExponentialBackoff` instance
#[must_use]
pub fn default_backoff() -> ExponentialBackoff {
    ExponentialBackoff::new(
        Duration::from_millis(INITIAL_INTERVAL_MILLIS),
        Duration::from_secs(MAX_INTERVAL_SECS),
        Some(Duration::from_secs(MAX_ELAPSED_TIME_SECS)),
        BACKOFF_MULTIPLIER,
        RANDOMIZATION_FACTOR,
    )
}

/// Create exponential backoff configuration for HTTP API retries.
///
/// HTTP API calls (e.g., bindcar sidecar) use faster retry cycles than Kubernetes API
/// since they target local/nearby services that should fail fast.
///
/// # Configuration
///
/// - **Initial interval**: 50ms
/// - **Max interval**: 10 seconds
/// - **Max elapsed time**: 2 minutes total
/// - **Multiplier**: 2.0 (exponential growth)
/// - **Randomization**: ±10% (prevents thundering herd)
///
/// # Retry Schedule
///
/// With these settings, retries occur at approximately:
///
/// 1. 50ms
/// 2. 100ms
/// 3. 200ms
/// 4. 400ms
/// 5. 800ms
/// 6. 1.6s
/// 7. 3.2s
/// 8. 6.4s
/// 9. 10s (capped at max interval)
///    10-12. 10s intervals until 2 minutes elapsed
///
/// # Returns
///
/// Configured `ExponentialBackoff` instance
#[must_use]
pub fn http_backoff() -> ExponentialBackoff {
    ExponentialBackoff::new(
        Duration::from_millis(HTTP_INITIAL_INTERVAL_MILLIS),
        Duration::from_secs(HTTP_MAX_INTERVAL_SECS),
        Some(Duration::from_secs(HTTP_MAX_ELAPSED_TIME_SECS)),
        BACKOFF_MULTIPLIER,
        RANDOMIZATION_FACTOR,
    )
}

/// Determine if an HTTP status code is retryable.
///
/// # Retryable Status Codes
///
/// - **429** (Too Many Requests) - Rate limiting
/// - **500** (Internal Server Error) - Server error
/// - **502** (Bad Gateway) - Proxy/gateway error
/// - **503** (Service Unavailable) - Temporary unavailability
/// - **504** (Gateway Timeout) - Gateway timeout
///
/// # Arguments
///
/// * `status` - The HTTP status code to check
///
/// # Returns
///
/// `true` if the status code indicates a transient error, `false` otherwise
#[must_use]
pub fn is_retryable_http_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

/// Retry a Kubernetes API call with exponential backoff.
///
/// Automatically retries on transient errors (HTTP 429, 5xx) and fails immediately
/// on permanent errors (4xx client errors except 429).
///
/// # Arguments
///
/// * `operation` - Async function that performs the API call
/// * `operation_name` - Human-readable name for logging (e.g., "get cluster")
///
/// # Returns
///
/// Result of the API call after retries
///
/// # Errors
///
/// Returns error if:
/// - Non-retryable error encountered (4xx client error)
/// - Max elapsed time exceeded (5 minutes)
/// - All retries exhausted
///
/// # Example
///
/// ```no_run
/// use kube::{Api, Client};
/// use bindy::crd::Bind9Cluster;
/// use bindy::reconcilers::retry::retry_api_call;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = Client::try_default().await?;
/// let api: Api<Bind9Cluster> = Api::namespaced(client, "default");
///
/// let cluster = retry_api_call(
///     || async { api.get("my-cluster").await.map_err(Into::into) },
///     "get cluster my-cluster"
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn retry_api_call<T, F, Fut>(mut operation: F, operation_name: &str) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, kube::Error>>,
{
    let mut backoff = default_backoff();
    let start_time = Instant::now();
    let mut attempt = 0;

    loop {
        attempt += 1;

        let result = operation().await;

        match result {
            Ok(value) => {
                if attempt > 1 {
                    debug!(
                        operation = operation_name,
                        attempt = attempt,
                        elapsed = ?start_time.elapsed(),
                        "Kubernetes API call succeeded after retries"
                    );
                } else {
                    debug!(operation = operation_name, "Kubernetes API call succeeded");
                }
                return Ok(value);
            }
            Err(e) => {
                // Check if error is retryable
                if !is_retryable_error(&e) {
                    error!(
                        operation = operation_name,
                        error = %e,
                        "Non-retryable Kubernetes API error, failing immediately"
                    );
                    return Err(e.into());
                }

                // Check if we've exceeded max elapsed time
                if let Some(max_elapsed) = backoff.max_elapsed_time {
                    if start_time.elapsed() >= max_elapsed {
                        error!(
                            operation = operation_name,
                            attempt = attempt,
                            elapsed = ?start_time.elapsed(),
                            error = %e,
                            "Max retry time exceeded, giving up"
                        );
                        return Err(anyhow::anyhow!(
                            "Max retry time exceeded after {attempt} attempts: {e}"
                        ));
                    }
                }

                // Calculate next backoff interval
                if let Some(duration) = backoff.next_backoff() {
                    warn!(
                        operation = operation_name,
                        attempt = attempt,
                        retry_after = ?duration,
                        error = %e,
                        "Retryable Kubernetes API error, will retry"
                    );
                    tokio::time::sleep(duration).await;
                } else {
                    error!(
                        operation = operation_name,
                        attempt = attempt,
                        elapsed = ?start_time.elapsed(),
                        error = %e,
                        "Backoff exhausted, giving up"
                    );
                    return Err(anyhow::anyhow!(
                        "Backoff exhausted after {attempt} attempts: {e}"
                    ));
                }
            }
        }
    }
}

/// Determine if a Kubernetes error is retryable.
///
/// # Retryable Errors
///
/// - **HTTP 429** (Too Many Requests) - Rate limiting
/// - **HTTP 5xx** (Server Errors) - Temporary API server issues
/// - **Service Errors** - Network/connection issues
///
/// # Non-Retryable Errors
///
/// - **HTTP 4xx** (Client Errors, except 429) - Invalid request, not found, unauthorized, etc.
/// - **Invalid Request** - Malformed data, schema violations
///
/// # Arguments
///
/// * `err` - The Kubernetes API error to check
///
/// # Returns
///
/// `true` if the error is transient and should be retried, `false` otherwise
fn is_retryable_error(err: &kube::Error) -> bool {
    match err {
        kube::Error::Api(api_err) => {
            // Retry on rate limiting (429) and server errors (5xx)
            api_err.code == 429 || (api_err.code >= 500 && api_err.code < 600)
        }
        kube::Error::Service(_) => {
            // Network/connection errors are retryable
            true
        }
        _ => {
            // Client errors (invalid request, not found, etc.) are not retryable
            false
        }
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod retry_tests;
