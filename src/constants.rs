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
pub const API_VERSION: &str = "v1beta1";

/// Fully qualified API version (group/version)
pub const API_GROUP_VERSION: &str = "bindy.firestoned.io/v1beta1";

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

/// Kind name for `ClusterBind9Provider` resource
pub const KIND_CLUSTER_BIND9_PROVIDER: &str = "ClusterBind9Provider";

/// Kind name for `Bind9Instance` resource
pub const KIND_BIND9_INSTANCE: &str = "Bind9Instance";

// ============================================================================
// DNS Protocol Constants
// ============================================================================

/// Standard DNS service port exposed externally
pub const DNS_PORT: u16 = 53;

/// DNS container port (non-privileged port for non-root execution)
pub const DNS_CONTAINER_PORT: u16 = 5353;

/// Standard RNDC control port (non-privileged)
pub const RNDC_PORT: u16 = 9530;

/// Default bindcar HTTP API container port
pub const BINDCAR_API_PORT: u16 = 8080;

/// Default bindcar HTTP API service port (exposed via Kubernetes Service)
pub const BINDCAR_SERVICE_PORT: u16 = 80;

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

/// `MALLOC_CONF` environment variable value for BIND9 containers
///
/// Optimizes jemalloc memory decay for containerized environments:
/// - `dirty_decay_ms:0` - Immediately return dirty pages to OS
/// - `muzzy_decay_ms:0` - Immediately return muzzy pages to OS
///
/// This enables more aggressive memory reclamation in environments where
/// memory pressure is monitored closely.
pub const BIND9_MALLOC_CONF: &str = "dirty_decay_ms:0,muzzy_decay_ms:0";

/// UID for running BIND9 and bindcar containers as non-root
///
/// This UID corresponds to the 'bind' or 'named' user in most BIND9 images.
/// Running as non-root improves container security by following the principle
/// of least privilege.
pub const BIND9_NONROOT_UID: i64 = 101;

// ============================================================================
// Bindcar Container Constants
// ============================================================================

/// Default bindcar sidecar container image
///
/// This is the default image used for the bindcar HTTP API sidecar container
/// when no image is specified in the `BindcarConfig` of a `Bind9Instance`,
/// `Bind9Cluster`, or `ClusterBind9Provider`.
pub const DEFAULT_BINDCAR_IMAGE: &str = "ghcr.io/firestoned/bindcar:v0.6.0";

// ============================================================================
// Container Name Constants
// ============================================================================

/// Name of the BIND9 container in the pod
pub const CONTAINER_NAME_BIND9: &str = "bind9";

/// Name of the bindcar API sidecar container in the pod
pub const CONTAINER_NAME_BINDCAR: &str = "api";

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

// ============================================================================
// DNSZone Record Ownership Constants
// ============================================================================

/// Annotation key for marking which zone owns a DNS record
///
/// When a `DNSZone`'s label selector matches a DNS record, the `DNSZone` controller
/// sets this annotation on the record with the value being the zone's FQDN.
/// Record reconcilers read this annotation to determine which zone to update.
pub const ANNOTATION_ZONE_OWNER: &str = "bindy.firestoned.io/zone";

/// Annotation key for marking which zone previously owned a record
///
/// When a record stops matching a zone's selector, the `DNSZone` controller sets
/// this annotation before removing the zone ownership. This helps track orphaned
/// records and enables cleanup workflows.
pub const ANNOTATION_ZONE_PREVIOUS_OWNER: &str = "bindy.firestoned.io/previous-zone";

/// Annotation key on `Bind9Instance` that lists namespaces from which a
/// `DNSZone` (in a *different* namespace) is permitted to target this
/// instance via `spec.bind9InstancesFrom` selectors.
///
/// **F-003 mitigation.** A label-selector match alone is not enough to
/// enrol a cross-namespace `Bind9Instance` in a zone — the platform admin
/// who owns the instance must also annotate it with the zone's namespace.
/// Same-namespace targeting (zone and instance in the same namespace) is
/// always permitted and does not require this annotation.
///
/// Value format: comma-separated list of namespace names. The literal
/// value `*` re-enables the pre-F-003 cluster-wide behaviour for
/// platform admins who explicitly accept the risk.
///
/// Examples:
/// - `"tenant-a,tenant-b"` — only zones in tenant-a or tenant-b may
///   claim this instance.
/// - `"*"` — any namespace may claim (back to pre-F-003 behaviour).
/// - annotation absent — only same-namespace zones may claim.
///
/// Why an annotation rather than a CRD field on `ClusterBind9Provider`?
/// The platform-admin contract for a cluster-wide operator is "platform
/// admin labels their instances; tenants match those labels." The
/// security gate must live on the side the tenant cannot forge — i.e.
/// metadata on the platform-owned `Bind9Instance` — and an annotation
/// keeps the admin's mental model intact without requiring tenants to
/// add a `clusterRef` they had no reason to set previously.
pub const ANNOTATION_ALLOW_ZONE_NAMESPACES: &str = "bindy.firestoned.io/allow-zone-namespaces";

/// Wildcard value for [`ANNOTATION_ALLOW_ZONE_NAMESPACES`] meaning "any
/// namespace may target this instance." Use with care — restores the
/// pre-F-003 cluster-wide behaviour.
pub const ALLOW_ZONE_NAMESPACES_WILDCARD: &str = "*";

// ============================================================================
// RNDC Key Rotation Constants
// ============================================================================

/// Annotation key for RNDC key creation timestamp (ISO 8601 format)
///
/// Tracks when the current RNDC key was created or last rotated.
/// Used by the rotation reconciler to determine when rotation is due.
///
/// Example value: `"2025-01-26T10:00:00Z"`
pub const ANNOTATION_RNDC_CREATED_AT: &str = "bindy.firestoned.io/rndc-created-at";

/// Annotation key for RNDC key rotation timestamp (ISO 8601 format)
///
/// Tracks when the RNDC key should be rotated next.
/// Calculated as: `created_at + rotate_after`
///
/// Only present when `auto_rotate` is enabled.
///
/// Example value: `"2025-02-25T10:00:00Z"` (30 days after creation)
pub const ANNOTATION_RNDC_ROTATE_AT: &str = "bindy.firestoned.io/rndc-rotate-at";

/// Annotation key for RNDC key rotation count
///
/// Tracks the number of times the RNDC key has been rotated.
/// Starts at `0` for newly-created keys and increments on each rotation.
///
/// Example value: `"5"` (key has been rotated 5 times)
pub const ANNOTATION_RNDC_ROTATION_COUNT: &str = "bindy.firestoned.io/rndc-rotation-count";

/// Annotation key for tracking pod restarts after RNDC rotation
///
/// Added to Deployment pod template to trigger rolling restart when RNDC key is rotated.
/// Value is the timestamp when rotation occurred (ISO 8601 format).
///
/// Example value: `"2025-01-26T10:30:00Z"`
pub const ANNOTATION_RNDC_ROTATED_AT: &str = "bindy.firestoned.io/rndc-rotated-at";

/// Minimum rotation interval in hours (1 hour)
///
/// RNDC keys cannot be rotated more frequently than once per hour.
/// This prevents infinite reconciliation loops and rate-limits rotation operations.
pub const MIN_ROTATION_INTERVAL_HOURS: u64 = 1;

/// Maximum rotation interval in hours (8760 hours = 365 days = 1 year)
///
/// RNDC keys must be rotated at least once per year for security compliance.
/// This is the upper bound for the `rotate_after` configuration.
pub const MAX_ROTATION_INTERVAL_HOURS: u64 = 8760;

/// Default rotation interval (720 hours = 30 days)
///
/// Default value for the `rotate_after` field when `auto_rotate` is enabled.
/// Balances security (regular rotation) with operational stability (not too frequent).
///
/// This is specified as a Go duration string: `"720h"`
pub const DEFAULT_ROTATION_INTERVAL: &str = "720h";

/// Minimum time between rotations in hours (1 hour)
///
/// Even if rotation is due (based on `rotate_at` timestamp), the reconciler
/// will not rotate a key if it was created or rotated within the last hour.
///
/// This prevents rapid successive rotations in edge cases (e.g., clock skew,
/// manual timestamp manipulation, reconciliation loops).
pub const MIN_TIME_BETWEEN_ROTATIONS_HOURS: i64 = 1;

// ============================================================================
// Kubernetes API Client Rate Limiting Constants
// ============================================================================

/// Kubernetes API client queries per second (sustained rate)
///
/// This matches kubectl default rate limits and has been tested at scale.
/// Prevents overwhelming the API server with too many requests.
/// Can be overridden via `BINDY_KUBE_QPS` environment variable.
pub const KUBE_CLIENT_QPS: f32 = 20.0;

/// Kubernetes API client burst size (max concurrent requests)
///
/// Allows temporary bursts above the QPS limit for reconciliation spikes.
/// Matches kubectl defaults for optimal API server behavior.
/// Can be overridden via `BINDY_KUBE_BURST` environment variable.
pub const KUBE_CLIENT_BURST: u32 = 30;

/// Page size for Kubernetes API list operations
///
/// Balances memory usage vs. number of API calls.
/// Limits each list response to 100 items, reducing memory pressure
/// when listing large resource sets (e.g., 1000+ `DNSZone`s).
///
/// With 100 items per page:
/// - 1000 resources = 10 API calls
/// - Memory usage remains constant (O(1) relative to total count)
/// - Reduces API server load per request
pub const KUBE_LIST_PAGE_SIZE: u32 = 100;

// ============================================================================
// User-volume Allow-list (F-001 mitigation)
// ============================================================================
//
// `Bind9Instance` and `Bind9Cluster` accept user-supplied `volumes` and
// `volumeMounts` fields that flow into the managed Pod spec. To prevent a
// namespace-tenant from mounting `hostPath`, `csi`, foreign Secrets, or any
// volume into a container the operator stamps with cluster-wide RBAC, we
// validate every user-supplied volume against the constants below before
// constructing the Pod. See `src/safe_volume.rs`.

/// Mount-path prefixes allowed for user-supplied `volumeMounts`.
///
/// Anything outside these prefixes is rejected at reconcile time. Operator-
/// managed mounts (`/etc/bind/...`, `/var/cache/bind`) are added by the
/// resource builder and bypass this check.
pub const ALLOWED_USER_MOUNT_PREFIXES: &[&str] = &["/data/", "/var/log/bind/"];

/// Required name prefix for any Secret that the user references via a
/// `secret:` volume. Prevents the user from mounting an arbitrary Secret
/// (including the operator's own credentials) into the BIND9/bindcar pod.
pub const ALLOWED_USER_SECRET_PREFIX: &str = "bindy-";

/// Required name prefix for any PVC that the user references via a
/// `persistentVolumeClaim:` volume. Same rationale as
/// [`ALLOWED_USER_SECRET_PREFIX`].
pub const ALLOWED_USER_PVC_PREFIX: &str = "bindy-";

/// Required name prefix for any ConfigMap that the user references via a
/// `configMap:` volume. Same rationale as [`ALLOWED_USER_SECRET_PREFIX`].
pub const ALLOWED_USER_CONFIGMAP_PREFIX: &str = "bindy-";
