// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Retry logic with exponential backoff for Kubernetes API calls.
//!
//! This module provides utilities for retrying transient API errors (429, 5xx)
//! with exponential backoff, while failing fast on permanent errors (4xx client errors).

use anyhow::Result;
use rand::RngExt;
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

        let mut rng = rand::rng();
        let jittered = rng.random_range(min..=max);

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

// ============================================================================
// Per-object reconcile backoff
// ============================================================================

/// Delay before the first retry of a failing reconcile.
///
/// Deliberately short. A fixed 30s requeue used to set the floor on how fast a
/// DNSZone could recover after its operand Pods were replaced: the Endpoints
/// watch does not reliably pull the object forward when a retry is already
/// scheduled, so the pending requeue decides recovery time. Measured on a kind
/// cluster, a zone sat idle for 57 seconds after its Pod was back and Ready,
/// then reconciled once and served in about 1 second.
pub const RECONCILE_BACKOFF_INITIAL: Duration = Duration::from_secs(2);

/// Ceiling for the per-object reconcile backoff.
///
/// A permanently broken object must not hammer the API server at
/// [`RECONCILE_BACKOFF_INITIAL`] forever.
pub const RECONCILE_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// How long an object must go without a failure before its backoff decays.
///
/// The controller only calls the error policy on failure, so there is no success
/// hook to clear the counter from. Decaying on age gets the same result without
/// threading a reset through every reconcile's happy path: an object that stops
/// failing simply ages out and starts from the fast interval next time.
const RECONCILE_BACKOFF_RESET_AFTER: Duration = Duration::from_secs(300);

/// Consecutive-failure counters, keyed by namespaced object name.
static RECONCILE_FAILURES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, (u32, Instant)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Returns how long to wait before retrying a failed reconcile of `key`.
///
/// Doubles on each consecutive failure, capped at [`RECONCILE_BACKOFF_MAX`], and
/// decays back to [`RECONCILE_BACKOFF_INITIAL`] once the object has gone
/// [`RECONCILE_BACKOFF_RESET_AFTER`] without failing.
///
/// # Arguments
///
/// * `key` - Stable identity for the object, e.g. `"namespace/name"`
///
/// # Returns
///
/// The requeue delay for this failure.
#[must_use]
pub fn reconcile_error_backoff(key: &str) -> Duration {
    let now = Instant::now();
    let Ok(mut failures) = RECONCILE_FAILURES.lock() else {
        // A poisoned lock must not take the operator down; fall back to the
        // initial interval, which is always a safe requeue.
        return RECONCILE_BACKOFF_INITIAL;
    };

    let entry = failures.entry(key.to_string()).or_insert((0, now));
    if now.duration_since(entry.1) >= RECONCILE_BACKOFF_RESET_AFTER {
        entry.0 = 0;
    }
    entry.1 = now;

    let delay = RECONCILE_BACKOFF_INITIAL
        .checked_mul(1_u32.checked_shl(entry.0).unwrap_or(u32::MAX))
        .unwrap_or(RECONCILE_BACKOFF_MAX)
        .min(RECONCILE_BACKOFF_MAX);

    entry.0 = entry.0.saturating_add(1);
    delay
}

/// Shortest interval between re-attempts of a record write BIND9 rejected.
///
/// A rejected write must not be re-issued on every watch event. BIND9 rejects
/// some updates for reasons no amount of retrying changes — an MX whose exchange
/// has no address record comes back `Refused` every time — and the record
/// reconciler is woken by far more than its own timer: a status patch on the
/// owning zone or on any primary instance re-runs it. Left alone that produced a
/// sustained several-updates-per-second delete/add storm against named for a
/// single bad record.
///
/// Matching [`crate::record_wrappers::REQUEUE_WHEN_NOT_READY_SECS`] means a
/// failing record is re-attempted by its own timed requeue and by nothing else.
pub const REJECTED_WRITE_COOLDOWN: Duration =
    Duration::from_secs(crate::record_wrappers::REQUEUE_WHEN_NOT_READY_SECS);

/// Rejected writes, keyed by object, holding the spec hash and when it failed.
static REJECTED_WRITES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, (String, Instant)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Record that writing `spec_hash` for `key` was rejected.
///
/// # Arguments
///
/// * `key` - Stable identity for the object, e.g. `"ARecord/namespace/name"`
/// * `spec_hash` - Hash of the spec that was rejected
pub fn note_rejected_write(key: &str, spec_hash: &str) {
    if let Ok(mut rejected) = REJECTED_WRITES.lock() {
        rejected.insert(key.to_string(), (spec_hash.to_string(), Instant::now()));
    }
}

/// Clears any rejection recorded for `key`.
///
/// Call this when a write succeeds or the object goes away, so the next failure
/// is treated as the first one.
///
/// # Arguments
///
/// * `key` - Stable identity for the object
pub fn clear_rejected_write(key: &str) {
    if let Ok(mut rejected) = REJECTED_WRITES.lock() {
        rejected.remove(key);
    }
}

/// Whether writing `spec_hash` for `key` should be skipped right now.
///
/// # Arguments
///
/// * `key` - Stable identity for the object
/// * `spec_hash` - Hash of the spec about to be written
///
/// # Returns
///
/// `true` while the identical spec is inside [`REJECTED_WRITE_COOLDOWN`] of its
/// last rejection. A changed spec is never held back: editing the record is how
/// a user fixes a permanent rejection, and that fix must take effect at once.
#[must_use]
pub fn write_in_cooldown(key: &str, spec_hash: &str) -> bool {
    write_in_cooldown_at(key, spec_hash, Instant::now())
}

/// [`write_in_cooldown`] with the current time supplied, so expiry is testable.
///
/// # Arguments
///
/// * `key` - Stable identity for the object
/// * `spec_hash` - Hash of the spec about to be written
/// * `now` - The instant to judge the cooldown against
///
/// # Returns
///
/// `true` if the write should be skipped at `now`.
#[must_use]
pub fn write_in_cooldown_at(key: &str, spec_hash: &str, now: Instant) -> bool {
    let Ok(rejected) = REJECTED_WRITES.lock() else {
        // A poisoned lock must not stall writes; attempting one is always safe.
        return false;
    };

    let Some((rejected_hash, rejected_at)) = rejected.get(key) else {
        return false;
    };

    if rejected_hash != spec_hash {
        return false;
    }

    now.duration_since(*rejected_at) < REJECTED_WRITE_COOLDOWN
}

/// Clears the failure counter for `key`, so its next failure requeues promptly.
///
/// # Arguments
///
/// * `key` - Stable identity for the object, e.g. `"namespace/name"`
pub fn reset_reconcile_backoff(key: &str) {
    if let Ok(mut failures) = RECONCILE_FAILURES.lock() {
        failures.remove(key);
    }
}
