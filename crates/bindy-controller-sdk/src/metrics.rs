// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Prometheus metrics for the Bindy DNS operator.
//!
//! This module provides comprehensive metrics collection with the namespace prefix
//! `bindy_firestoned_io_` (prometheus-safe version of "bindy.firestoned.io").
//!
//! # Metrics Categories
//!
//! - **Reconciliation Metrics** - Track reconciliation operations and their outcomes
//! - **Resource Lifecycle Metrics** - Track resource creation, updates, and deletions
//! - **Error Metrics** - Track error conditions and types
//! - **Leader Election Metrics** - Track leadership state changes
//! - **Performance Metrics** - Track duration and latency
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy_controller_sdk::metrics::{METRICS_REGISTRY, record_reconciliation_success};
//!
//! // Record a successful reconciliation
//! record_reconciliation_success("DNSZone", std::time::Duration::from_secs(1));
//! ```

use prometheus::{
    CounterVec, Encoder, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};
use std::sync::LazyLock;
use std::time::Duration;

// ============================================================================
// Metric Name Constants
// ============================================================================

/// Namespace prefix for all Bindy metrics (prometheus-safe)
const METRICS_NAMESPACE: &str = "bindy_firestoned_io";

// ============================================================================
// Global Metrics Registry
// ============================================================================

/// Global Prometheus metrics registry
///
/// All metrics are registered in this registry and exposed via `/metrics` endpoint.
pub static METRICS_REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

// ============================================================================
// Reconciliation Metrics
// ============================================================================

/// Total number of reconciliations by resource type and status
///
/// Labels:
/// - `resource_type`: Kind of resource (e.g., `DNSZone`, `ARecord`)
/// - `status`: Outcome (`success`, `error`, `requeue`)
pub static RECONCILIATION_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_reconciliations_total"),
        "Total number of reconciliations by resource type and status",
    );
    let counter = CounterVec::new(opts, &["resource_type", "status"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Duration of reconciliations in seconds
///
/// Labels:
/// - `resource_type`: Kind of resource (e.g., `DNSZone`, `ARecord`)
pub static RECONCILIATION_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    let opts = HistogramOpts::new(
        format!("{METRICS_NAMESPACE}_reconciliation_duration_seconds"),
        "Duration of reconciliations in seconds by resource type",
    )
    .buckets(vec![0.001, 0.01, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]);
    let histogram = HistogramVec::new(opts, &["resource_type"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(histogram.clone()))
        .unwrap();
    histogram
});

/// Total number of requeue operations
///
/// Labels:
/// - `resource_type`: Kind of resource
/// - `reason`: Reason for requeue (`error`, `rate_limit`, `dependency_wait`)
pub static REQUEUE_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_requeues_total"),
        "Total number of requeue operations by resource type and reason",
    );
    let counter = CounterVec::new(opts, &["resource_type", "reason"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

// ============================================================================
// Resource Lifecycle Metrics
// ============================================================================

/// Total number of resources created
///
/// Labels:
/// - `resource_type`: Kind of resource created
pub static RESOURCES_CREATED_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_resources_created_total"),
        "Total number of resources created by type",
    );
    let counter = CounterVec::new(opts, &["resource_type"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Total number of resources updated
///
/// Labels:
/// - `resource_type`: Kind of resource updated
pub static RESOURCES_UPDATED_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_resources_updated_total"),
        "Total number of resources updated by type",
    );
    let counter = CounterVec::new(opts, &["resource_type"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Total number of resources deleted
///
/// Labels:
/// - `resource_type`: Kind of resource deleted
pub static RESOURCES_DELETED_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_resources_deleted_total"),
        "Total number of resources deleted by type",
    );
    let counter = CounterVec::new(opts, &["resource_type"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Number of currently active resources being tracked
///
/// Labels:
/// - `resource_type`: Kind of resource
pub static RESOURCES_ACTIVE: LazyLock<GaugeVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_resources_active"),
        "Number of currently active resources by type",
    );
    let gauge = GaugeVec::new(opts, &["resource_type"]).unwrap();
    METRICS_REGISTRY.register(Box::new(gauge.clone())).unwrap();
    gauge
});

// ============================================================================
// Error Metrics
// ============================================================================

/// Total number of errors by resource type and error category
///
/// Labels:
/// - `resource_type`: Kind of resource
/// - `error_type`: Category of error (`api_error`, `validation_error`, `network_error`, `timeout`)
pub static ERRORS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_errors_total"),
        "Total number of errors by resource type and error category",
    );
    let counter = CounterVec::new(opts, &["resource_type", "error_type"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

// ============================================================================
// Leader Election Metrics
// ============================================================================

/// Total number of leader election events
///
/// Labels:
/// - `status`: Event type (`acquired`, `lost`, `renewed`)
pub static LEADER_ELECTIONS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_leader_elections_total"),
        "Total number of leader election events by status",
    );
    let counter = CounterVec::new(opts, &["status"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Current leader election status
///
/// Labels:
/// - `pod_name`: Name of the pod
///
/// Value: 1 if leader, 0 if follower
pub static LEADER_STATUS: LazyLock<GaugeVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_leader_status"),
        "Current leader election status (1 = leader, 0 = follower)",
    );
    let gauge = GaugeVec::new(opts, &["pod_name"]).unwrap();
    METRICS_REGISTRY.register(Box::new(gauge.clone())).unwrap();
    gauge
});

// ============================================================================
// Performance Metrics
// ============================================================================

/// Lag between resource generation change and observation
///
/// Labels:
/// - `resource_type`: Kind of resource
pub static GENERATION_OBSERVATION_LAG_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    let opts = HistogramOpts::new(
        format!("{METRICS_NAMESPACE}_generation_observation_lag_seconds"),
        "Lag between spec generation change and controller observation",
    )
    .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0]);
    let histogram = HistogramVec::new(opts, &["resource_type"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(histogram.clone()))
        .unwrap();
    histogram
});

// ============================================================================
// Kubernetes API Client Metrics (ADR-0005, roadmap 05)
// ============================================================================

/// Total Kubernetes API requests issued by the client, by resource and verb
///
/// Labels:
/// - `resource`: Resource plural parsed from the request path (e.g., `dnszones`)
/// - `verb`: Lowercase HTTP method (`get`, `post`, `patch`, ...)
/// - `status`: Outcome (`success` for 2xx/3xx, `error` otherwise)
pub static KUBE_API_REQUESTS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_kube_api_requests_total"),
        "Total Kubernetes API requests by resource, verb, and status",
    );
    let counter = CounterVec::new(opts, &["resource", "verb", "status"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Kubernetes API request duration in seconds, by resource and verb
///
/// Measured from dispatch into the HTTP stack to response headers; time spent
/// queued behind the client-side rate limiter is NOT included.
pub static KUBE_API_REQUEST_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    let opts = HistogramOpts::new(
        format!("{METRICS_NAMESPACE}_kube_api_request_duration_seconds"),
        "Kubernetes API request duration in seconds by resource and verb",
    )
    .buckets(vec![0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]);
    let histogram = HistogramVec::new(opts, &["resource", "verb"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(histogram.clone()))
        .unwrap();
    histogram
});

/// Total HTTP 429 (Too Many Requests) responses from the API server
///
/// Under the client-side rate limiter this should stay at zero; a non-zero
/// rate means the configured QPS/burst exceeds what the API server tolerates.
pub static KUBE_API_RATE_LIMIT_HITS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_kube_api_rate_limit_hits_total"),
        "Total HTTP 429 rate-limit responses from the Kubernetes API server",
    );
    let counter = CounterVec::new(opts, &["resource", "verb"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Total Kubernetes API call retries performed by `crate::retry`
///
/// Labels:
/// - `operation`: Low-cardinality operation name (e.g., `get Bind9Cluster`) —
///   callers must NOT embed object names in it
pub static KUBE_API_RETRIES_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_kube_api_retries_total"),
        "Total Kubernetes API call retries by operation",
    );
    let counter = CounterVec::new(opts, &["operation"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Pages fetched per paginated list operation, by resource kind
pub static KUBE_API_PAGINATION_PAGES: LazyLock<HistogramVec> = LazyLock::new(|| {
    let opts = HistogramOpts::new(
        format!("{METRICS_NAMESPACE}_kube_api_pagination_pages"),
        "Number of pages fetched per paginated Kubernetes list operation",
    )
    .buckets(vec![1.0, 2.0, 3.0, 5.0, 10.0, 25.0, 50.0, 100.0]);
    let histogram = HistogramVec::new(opts, &["resource"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(histogram.clone()))
        .unwrap();
    histogram
});

// ============================================================================
// Helper Functions
// ============================================================================

/// Record a completed Kubernetes API request
///
/// # Arguments
/// * `resource` - Resource plural from the request path (e.g., `dnszones`)
/// * `verb` - Lowercase HTTP method
/// * `success` - Whether the response status was 2xx/3xx
/// * `duration` - Time from dispatch to response headers
pub fn record_kube_api_request(resource: &str, verb: &str, success: bool, duration: Duration) {
    let status = if success { "success" } else { "error" };
    KUBE_API_REQUESTS_TOTAL
        .with_label_values(&[resource, verb, status])
        .inc();
    KUBE_API_REQUEST_DURATION_SECONDS
        .with_label_values(&[resource, verb])
        .observe(duration.as_secs_f64());
}

/// Record an HTTP 429 rate-limit response from the API server
///
/// # Arguments
/// * `resource` - Resource plural from the request path
/// * `verb` - Lowercase HTTP method
pub fn record_kube_api_rate_limit_hit(resource: &str, verb: &str) {
    KUBE_API_RATE_LIMIT_HITS_TOTAL
        .with_label_values(&[resource, verb])
        .inc();
}

/// Record one retry of a Kubernetes API call
///
/// # Arguments
/// * `operation` - Low-cardinality operation name (no object names)
pub fn record_kube_api_retry(operation: &str) {
    KUBE_API_RETRIES_TOTAL.with_label_values(&[operation]).inc();
}

/// Record the page count of a completed paginated list operation
///
/// # Arguments
/// * `resource` - Resource kind listed (e.g., `DNSZone`)
/// * `pages` - Number of pages fetched
pub fn record_kube_api_pagination_pages(resource: &str, pages: usize) {
    // usize→f64 is lossless for any realistic page count (< 2^52).
    #[allow(clippy::cast_precision_loss)]
    KUBE_API_PAGINATION_PAGES
        .with_label_values(&[resource])
        .observe(pages as f64);
}

/// Record a successful reconciliation
///
/// # Arguments
/// * `resource_type` - The kind of resource reconciled (e.g., `DNSZone`)
/// * `duration` - Duration of the reconciliation
pub fn record_reconciliation_success(resource_type: &str, duration: Duration) {
    RECONCILIATION_TOTAL
        .with_label_values(&[resource_type, "success"])
        .inc();
    RECONCILIATION_DURATION_SECONDS
        .with_label_values(&[resource_type])
        .observe(duration.as_secs_f64());
}

/// Record a failed reconciliation
///
/// # Arguments
/// * `resource_type` - The kind of resource reconciled
/// * `duration` - Duration of the reconciliation before failure
pub fn record_reconciliation_error(resource_type: &str, duration: Duration) {
    RECONCILIATION_TOTAL
        .with_label_values(&[resource_type, "error"])
        .inc();
    RECONCILIATION_DURATION_SECONDS
        .with_label_values(&[resource_type])
        .observe(duration.as_secs_f64());
}

/// Record a reconciliation requeue
///
/// # Arguments
/// * `resource_type` - The kind of resource reconciled
/// * `reason` - Reason for requeue (e.g., `error`, `rate_limit`)
pub fn record_reconciliation_requeue(resource_type: &str, reason: &str) {
    RECONCILIATION_TOTAL
        .with_label_values(&[resource_type, "requeue"])
        .inc();
    REQUEUE_TOTAL
        .with_label_values(&[resource_type, reason])
        .inc();
}

/// Record resource creation
///
/// # Arguments
/// * `resource_type` - The kind of resource created
pub fn record_resource_created(resource_type: &str) {
    RESOURCES_CREATED_TOTAL
        .with_label_values(&[resource_type])
        .inc();
    RESOURCES_ACTIVE.with_label_values(&[resource_type]).inc();
}

/// Record resource update
///
/// # Arguments
/// * `resource_type` - The kind of resource updated
pub fn record_resource_updated(resource_type: &str) {
    RESOURCES_UPDATED_TOTAL
        .with_label_values(&[resource_type])
        .inc();
}

/// Record resource deletion
///
/// # Arguments
/// * `resource_type` - The kind of resource deleted
pub fn record_resource_deleted(resource_type: &str) {
    RESOURCES_DELETED_TOTAL
        .with_label_values(&[resource_type])
        .inc();
    RESOURCES_ACTIVE.with_label_values(&[resource_type]).dec();
}

/// Record an error
///
/// # Arguments
/// * `resource_type` - The kind of resource where error occurred
/// * `error_type` - Category of error (e.g., `api_error`, `validation_error`)
pub fn record_error(resource_type: &str, error_type: &str) {
    ERRORS_TOTAL
        .with_label_values(&[resource_type, error_type])
        .inc();
}

/// Gather and encode all metrics in Prometheus text format
///
/// # Returns
/// Prometheus-formatted metrics as a String
///
/// # Errors
/// Returns error if encoding fails
pub fn gather_metrics() -> Result<String, prometheus::Error> {
    let encoder = TextEncoder::new();
    let metric_families = METRICS_REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    String::from_utf8(buffer).map_err(|e| prometheus::Error::Msg(format!("UTF-8 error: {e}")))
}

// ============================================================================
// Shared watch metrics (ADR-0009 §3)
// ============================================================================

/// Watcher events applied by the shared `WatchSet`, by kind and namespace
/// target (`<all>` when cluster-wide).
pub static WATCH_EVENTS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_watch_events_total"),
        "Watcher events applied by the shared watch layer, by kind and namespace",
    );
    let counter = CounterVec::new(opts, &["kind", "namespace"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Watcher errors seen by the shared `WatchSet`, by kind and namespace.
pub static WATCH_ERRORS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_watch_errors_total"),
        "Watcher errors seen by the shared watch layer, by kind and namespace",
    );
    let counter = CounterVec::new(opts, &["kind", "namespace"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Times a shared watch stream ended and was restarted, by kind and namespace.
pub static WATCH_RESTARTS_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_watch_restarts_total"),
        "Shared watch streams that ended and were restarted, by kind and namespace",
    );
    let counter = CounterVec::new(opts, &["kind", "namespace"]).unwrap();
    METRICS_REGISTRY
        .register(Box::new(counter.clone()))
        .unwrap();
    counter
});

/// Unix time of the last event a shared watch applied, by kind and
/// namespace. A value that stops advancing while objects are changing means
/// that kind's cache is stale.
pub static WATCH_LAST_EVENT_TIMESTAMP_SECONDS: LazyLock<GaugeVec> = LazyLock::new(|| {
    let opts = Opts::new(
        format!("{METRICS_NAMESPACE}_watch_last_event_timestamp_seconds"),
        "Unix time of the last event applied by a shared watch, by kind and namespace",
    );
    let gauge = GaugeVec::new(opts, &["kind", "namespace"]).unwrap();
    METRICS_REGISTRY.register(Box::new(gauge.clone())).unwrap();
    gauge
});

/// Record one watcher event applied by the shared watch layer.
///
/// # Arguments
/// * `kind` - Resource kind (e.g. `DNSZone`)
/// * `namespace` - Namespace target, or `<all>` when cluster-wide
pub fn record_watch_event(kind: &str, namespace: &str) {
    WATCH_EVENTS_TOTAL
        .with_label_values(&[kind, namespace])
        .inc();
    // Seconds since the epoch fit an f64 exactly for the foreseeable future.
    #[allow(clippy::cast_precision_loss)]
    let now = chrono::Utc::now().timestamp() as f64;
    WATCH_LAST_EVENT_TIMESTAMP_SECONDS
        .with_label_values(&[kind, namespace])
        .set(now);
}

/// Record one watcher error seen by the shared watch layer.
///
/// # Arguments
/// * `kind` - Resource kind
/// * `namespace` - Namespace target, or `<all>` when cluster-wide
pub fn record_watch_error(kind: &str, namespace: &str) {
    WATCH_ERRORS_TOTAL
        .with_label_values(&[kind, namespace])
        .inc();
}

/// Record that a shared watch stream ended and is being restarted.
///
/// # Arguments
/// * `kind` - Resource kind
/// * `namespace` - Namespace target, or `<all>` when cluster-wide
pub fn record_watch_restart(kind: &str, namespace: &str) {
    WATCH_RESTARTS_TOTAL
        .with_label_values(&[kind, namespace])
        .inc();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_reconciliation_success() {
        let resource_type = "TestResource";
        let duration = Duration::from_millis(500);

        // Record success
        record_reconciliation_success(resource_type, duration);

        // Verify counter incremented
        let counter = RECONCILIATION_TOTAL.with_label_values(&[resource_type, "success"]);
        assert!(counter.get() > 0.0);

        // Verify histogram recorded
        let histogram = RECONCILIATION_DURATION_SECONDS.with_label_values(&[resource_type]);
        assert!(histogram.get_sample_count() > 0);
    }

    #[test]
    fn test_record_reconciliation_error() {
        let resource_type = "TestResourceError";
        let duration = Duration::from_millis(250);

        // Record error
        record_reconciliation_error(resource_type, duration);

        // Verify counter incremented
        let counter = RECONCILIATION_TOTAL.with_label_values(&[resource_type, "error"]);
        assert!(counter.get() > 0.0);

        // Verify histogram recorded
        let histogram = RECONCILIATION_DURATION_SECONDS.with_label_values(&[resource_type]);
        assert!(histogram.get_sample_count() > 0);
    }

    #[test]
    fn test_gather_metrics() {
        // Record some metrics to initialize them
        record_reconciliation_success("GatherTest", Duration::from_millis(100));

        // Gather metrics
        let result = gather_metrics();
        assert!(result.is_ok(), "Gathering metrics should succeed");

        let metrics_text = result.unwrap();
        assert!(
            metrics_text.contains("bindy_firestoned_io"),
            "Metrics should contain namespace prefix"
        );
        assert!(
            metrics_text.contains("reconciliations_total"),
            "Metrics should contain reconciliation counter"
        );
    }
}
