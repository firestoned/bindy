// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

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
//! use bindy::metrics::{METRICS_REGISTRY, record_reconciliation_success};
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
// Helper Functions
// ============================================================================

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

/// Record leader election acquired
///
/// # Arguments
/// * `pod_name` - Name of the pod that acquired leadership
pub fn record_leader_elected(pod_name: &str) {
    LEADER_ELECTIONS_TOTAL
        .with_label_values(&["acquired"])
        .inc();
    LEADER_STATUS.with_label_values(&[pod_name]).set(1.0);
}

/// Record leader election lost
///
/// # Arguments
/// * `pod_name` - Name of the pod that lost leadership
pub fn record_leader_lost(pod_name: &str) {
    LEADER_ELECTIONS_TOTAL.with_label_values(&["lost"]).inc();
    LEADER_STATUS.with_label_values(&[pod_name]).set(0.0);
}

/// Record leader election renewed
pub fn record_leader_renewed() {
    LEADER_ELECTIONS_TOTAL.with_label_values(&["renewed"]).inc();
}

/// Record generation observation lag
///
/// # Arguments
/// * `resource_type` - The kind of resource
/// * `lag` - Duration between generation change and observation
pub fn record_generation_lag(resource_type: &str, lag: Duration) {
    GENERATION_OBSERVATION_LAG_SECONDS
        .with_label_values(&[resource_type])
        .observe(lag.as_secs_f64());
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
