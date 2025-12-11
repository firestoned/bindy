// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Global constants for the Bindy operator.
//!
//! This module contains all numeric and string constants used throughout the codebase.
//! Constants are organized by category for easy maintenance.

// ============================================================================
// API Constants
// ============================================================================

/// API group for all Bindy DNS CRDs
pub const API_GROUP: &str = "bindy.firestoned.io";

/// API version for all Bindy DNS CRDs
pub const API_VERSION: &str = "v1alpha1";

/// Fully qualified API version (group/version)
pub const API_GROUP_VERSION: &str = "bindy.firestoned.io/v1alpha1";

/// Kind name for `DNSZone` resource
pub const KIND_DNS_ZONE: &str = "DNSZone";

/// Kind name for `ARecord` resource
pub const KIND_A_RECORD: &str = "ARecord";

/// Kind name for `AAAARecord` resource
pub const KIND_AAAA_RECORD: &str = "AAAARecord";

/// Kind name for `TXTRecord` resource
pub const KIND_TXT_RECORD: &str = "TXTRecord";

/// Kind name for `CNAMERecord` resource
pub const KIND_CNAME_RECORD: &str = "CNAMERecord";

/// Kind name for `MXRecord` resource
pub const KIND_MX_RECORD: &str = "MXRecord";

/// Kind name for `NSRecord` resource
pub const KIND_NS_RECORD: &str = "NSRecord";

/// Kind name for `SRVRecord` resource
pub const KIND_SRV_RECORD: &str = "SRVRecord";

/// Kind name for `CAARecord` resource
pub const KIND_CAA_RECORD: &str = "CAARecord";

/// Kind name for `Bind9Cluster` resource
pub const KIND_BIND9_CLUSTER: &str = "Bind9Cluster";

/// Kind name for `Bind9Instance` resource
pub const KIND_BIND9_INSTANCE: &str = "Bind9Instance";

// ============================================================================
// DNS Protocol Constants
// ============================================================================

/// Standard DNS port for queries and zone transfers
pub const DNS_PORT: u16 = 53;

/// Standard RNDC control port
pub const RNDC_PORT: u16 = 953;

/// Default bindcar HTTP API port
pub const BINDCAR_API_PORT: u16 = 8080;

/// Default TTL for DNS records (5 minutes)
pub const DEFAULT_DNS_RECORD_TTL_SECS: i32 = 300;

/// Default TTL for zone files (1 hour)
pub const DEFAULT_ZONE_TTL_SECS: u32 = 3600;

/// Default SOA refresh interval (1 hour)
pub const DEFAULT_SOA_REFRESH_SECS: u32 = 3600;

/// Default SOA retry interval (10 minutes)
pub const DEFAULT_SOA_RETRY_SECS: u32 = 600;

/// Default SOA expire time (7 days)
pub const DEFAULT_SOA_EXPIRE_SECS: u32 = 604_800;

/// Default SOA negative TTL (1 day)
pub const DEFAULT_SOA_NEGATIVE_TTL_SECS: u32 = 86400;

/// TSIG fudge time in seconds (allows for clock skew)
pub const TSIG_FUDGE_TIME_SECS: u64 = 300;

// ============================================================================
// Kubernetes Health Check Constants
// ============================================================================

/// Liveness probe initial delay (wait for BIND9 to start)
pub const LIVENESS_INITIAL_DELAY_SECS: i32 = 30;

/// Liveness probe period (how often to check)
pub const LIVENESS_PERIOD_SECS: i32 = 10;

/// Liveness probe timeout
pub const LIVENESS_TIMEOUT_SECS: i32 = 5;

/// Liveness probe failure threshold
pub const LIVENESS_FAILURE_THRESHOLD: i32 = 3;

/// Readiness probe initial delay
pub const READINESS_INITIAL_DELAY_SECS: i32 = 10;

/// Readiness probe period
pub const READINESS_PERIOD_SECS: i32 = 5;

/// Readiness probe timeout
pub const READINESS_TIMEOUT_SECS: i32 = 3;

/// Readiness probe failure threshold
pub const READINESS_FAILURE_THRESHOLD: i32 = 3;

// ============================================================================
// Controller Error Handling Constants
// ============================================================================

/// Requeue duration for controller errors (30 seconds)
pub const ERROR_REQUEUE_DURATION_SECS: u64 = 30;

// ============================================================================
// Leader Election Constants
// ============================================================================

/// Default leader election lease duration (15 seconds)
pub const DEFAULT_LEASE_DURATION_SECS: u64 = 15;

/// Default leader election renew deadline (10 seconds)
pub const DEFAULT_LEASE_RENEW_DEADLINE_SECS: u64 = 10;

/// Default leader election retry period (2 seconds)
pub const DEFAULT_LEASE_RETRY_PERIOD_SECS: u64 = 2;

// ============================================================================
// BIND9 Version Constants
// ============================================================================

/// Default BIND9 version tag
pub const DEFAULT_BIND9_VERSION: &str = "9.18";

/// `ServiceAccount` name for BIND9 pods
pub const BIND9_SERVICE_ACCOUNT: &str = "bind9";

// ============================================================================
// Runtime Constants
// ============================================================================

/// Number of worker threads for Tokio runtime
pub const TOKIO_WORKER_THREADS: usize = 4;

// ============================================================================
// Replica Count Constants
// ============================================================================

/// Minimum number of replicas for testing
pub const MIN_TEST_REPLICAS: i32 = 2;

/// Maximum reasonable number of replicas for testing
pub const MAX_TEST_REPLICAS: i32 = 10;

// ============================================================================
// Metrics Server Constants
// ============================================================================

/// Port for Prometheus metrics HTTP server
pub const METRICS_SERVER_PORT: u16 = 8080;

/// Path for Prometheus metrics endpoint
pub const METRICS_SERVER_PATH: &str = "/metrics";

/// Bind address for metrics HTTP server
pub const METRICS_SERVER_BIND_ADDRESS: &str = "0.0.0.0";
