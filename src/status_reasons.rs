// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Standard Kubernetes status condition reasons for Bindy resources.
//!
//! This module defines constants for condition reasons following Kubernetes conventions.
//! Reasons are programmatic identifiers in CamelCase that explain why a condition has
//! a particular status.
//!
//! # Condition Hierarchy
//!
//! Bindy uses a hierarchical status tracking system where each resource tracks its children:
//!
//! - **`ClusterBind9Provider`** → tracks `Bind9Cluster` resources
//! - **`Bind9Cluster`** → tracks `Bind9Instance` resources
//! - **`Bind9Instance`** → tracks `Pod` replicas
//!
//! # Condition Types
//!
//! ## Primary Condition
//!
//! All resources have a single encompassing `type: Ready` condition that indicates
//! the overall health of the resource.
//!
//! ## Child Conditions
//!
//! Resources also track individual child resource status with indexed conditions:
//!
//! - `Bind9Cluster`: conditions like `Bind9Instance-0`, `Bind9Instance-1`, etc.
//! - `Bind9Instance`: conditions like `Pod-0`, `Pod-1`, etc.
//!
//! # Example Status
//!
//! ```yaml
//! status:
//!   conditions:
//!     - type: Ready
//!       status: "True"
//!       reason: AllReady
//!       message: "All 3 instances are ready"
//!     - type: Bind9Instance-0
//!       status: "True"
//!       reason: AllReady
//!       message: "Instance my-cluster-primary-0 is ready (2/2 pods)"
//!     - type: Bind9Instance-1
//!       status: "True"
//!       reason: AllReady
//!       message: "Instance my-cluster-primary-1 is ready (2/2 pods)"
//!     - type: Bind9Instance-2
//!       status: "False"
//!       reason: PartiallyReady
//!       message: "Instance my-cluster-secondary-0 is progressing (1/2 pods)"
//! ```

// ============================================================================
// Common Reasons (All Resources)
// ============================================================================

/// All child resources are ready and healthy.
///
/// **Usage:**
/// - Use for the **encompassing `type: Ready` condition** when all children are ready
/// - For **child conditions** (e.g., `Bind9Instance-0`, `Pod-1`), use `REASON_READY` instead
pub const REASON_ALL_READY: &str = "AllReady";

/// Resource is ready and operational.
///
/// **Usage:**
/// - Use for **child conditions** (e.g., `Bind9Instance-0`, `Pod-1`) when they are ready
/// - For the **encompassing `type: Ready` condition**, use `REASON_ALL_READY` instead
///
/// **Example:**
/// ```yaml
/// conditions:
///   - type: Ready                    # Encompassing condition
///     status: "True"
///     reason: AllReady               # <- Use REASON_ALL_READY
///     message: "All 3 instances are ready"
///
///   - type: Bind9Instance-0          # Child condition
///     status: "True"
///     reason: Ready                  # <- Use REASON_READY
///     message: "Instance my-cluster-primary-0 is ready (2/2 pods)"
/// ```
pub const REASON_READY: &str = "Ready";

/// Some but not all child resources are ready.
pub const REASON_PARTIALLY_READY: &str = "PartiallyReady";

/// No child resources are ready.
pub const REASON_NOT_READY: &str = "NotReady";

/// No child resources found (expected children missing).
pub const REASON_NO_CHILDREN: &str = "NoChildren";

/// Resources are being created or updated.
pub const REASON_PROGRESSING: &str = "Progressing";

/// Configuration has been validated successfully.
pub const REASON_CONFIGURATION_VALID: &str = "ConfigurationValid";

/// Configuration validation failed.
pub const REASON_CONFIGURATION_INVALID: &str = "ConfigurationInvalid";

// ============================================================================
// Bind9Instance Specific Reasons
// ============================================================================

/// Minimum number of replicas are available (but not all).
///
/// This indicates the instance has met its minimum replica threshold for availability
/// but has not yet reached full desired replica count.
pub const REASON_MINIMUM_REPLICAS_AVAILABLE: &str = "MinimumReplicasAvailable";

/// Deployment has exceeded its progress deadline.
///
/// This occurs when a Deployment fails to make progress within its configured
/// `progressDeadlineSeconds`. Common causes include image pull errors, insufficient
/// resources, or crashing containers.
pub const REASON_PROGRESS_DEADLINE_EXCEEDED: &str = "ProgressDeadlineExceeded";

/// Failed to authenticate with RNDC (Remote Name Daemon Control).
///
/// This indicates the RNDC key authentication failed when attempting to connect
/// to the BIND9 server. Possible causes:
/// - Incorrect RNDC key in Secret
/// - RNDC key mismatch between controller and server
/// - RNDC disabled in BIND9 configuration
pub const REASON_RNDC_AUTHENTICATION_FAILED: &str = "RNDCAuthenticationFailed";

/// Cannot connect to Bindcar API container.
///
/// This indicates the Bindcar sidecar container is unreachable via HTTP.
/// Maps to HTTP connection errors or gateway errors (502, 503, 504).
///
/// Possible causes:
/// - Bindcar container not running
/// - Network policy blocking traffic
/// - Bindcar listening on wrong port
/// - Bindcar container crashed
/// - Gateway timeout reaching the pod
pub const REASON_BINDCAR_UNREACHABLE: &str = "BindcarUnreachable";

/// Secondary instance successfully transferred zones from primary.
///
/// This confirms that an AXFR (full zone transfer) or IXFR (incremental zone transfer)
/// completed successfully from the primary server to this secondary instance.
pub const REASON_ZONE_TRANSFER_COMPLETE: &str = "ZoneTransferComplete";

/// Zone transfer failed or is in progress.
///
/// This indicates a zone transfer (AXFR/IXFR) failed or has not completed yet.
/// Common causes:
/// - Primary server unreachable
/// - TSIG authentication failure
/// - Network issues
/// - Zone serial number mismatch
pub const REASON_ZONE_TRANSFER_FAILED: &str = "ZoneTransferFailed";

/// Deployment is waiting for pods to be scheduled.
pub const REASON_PODS_PENDING: &str = "PodsPending";

/// One or more pods are crashing or in `CrashLoopBackOff`.
pub const REASON_PODS_CRASHING: &str = "PodsCrashing";

/// Bindcar API returned an invalid or malformed request error.
///
/// Maps to HTTP 400 Bad Request.
/// This indicates the request sent to Bindcar was malformed or contained
/// invalid parameters. This is typically a bug in the controller.
pub const REASON_BINDCAR_BAD_REQUEST: &str = "BindcarBadRequest";

/// Bindcar API authentication or authorization failed.
///
/// Maps to HTTP 401 Unauthorized or 403 Forbidden.
/// This indicates the controller lacks proper credentials or permissions
/// to interact with the Bindcar API.
pub const REASON_BINDCAR_AUTH_FAILED: &str = "BindcarAuthFailed";

/// Requested zone or resource not found in BIND9.
///
/// Maps to HTTP 404 Not Found.
/// This indicates the zone, record, or resource does not exist in the
/// BIND9 server configuration.
pub const REASON_ZONE_NOT_FOUND: &str = "ZoneNotFound";

/// Bindcar API encountered an internal server error.
///
/// Maps to HTTP 500 Internal Server Error.
/// This indicates the Bindcar API encountered an unexpected error while
/// processing the request. Check Bindcar container logs for details.
pub const REASON_BINDCAR_INTERNAL_ERROR: &str = "BindcarInternalError";

/// Bindcar API feature not implemented.
///
/// Maps to HTTP 501 Not Implemented.
/// This indicates the requested operation is not supported by the current
/// version of Bindcar.
pub const REASON_BINDCAR_NOT_IMPLEMENTED: &str = "BindcarNotImplemented";

/// Gateway error reaching Bindcar pod.
///
/// Maps to HTTP 502 Bad Gateway, 503 Service Unavailable, 504 Gateway Timeout.
/// This indicates a network gateway or load balancer cannot reach the Bindcar
/// pod, even though the pod might be running. Check network policies, service
/// mesh configuration, and pod readiness probes.
pub const REASON_GATEWAY_ERROR: &str = "GatewayError";

// ============================================================================
// Bind9Cluster Specific Reasons
// ============================================================================

/// All managed `Bind9Instance` resources have been created.
pub const REASON_INSTANCES_CREATED: &str = "InstancesCreated";

/// Scaling instances up or down to match desired replica count.
pub const REASON_INSTANCES_SCALING: &str = "InstancesScaling";

/// Waiting for instances to be created or updated.
pub const REASON_INSTANCES_PENDING: &str = "InstancesPending";

// ============================================================================
// ClusterBind9Provider Specific Reasons
// ============================================================================

/// All namespace-scoped `Bind9Cluster` resources are ready.
pub const REASON_CLUSTERS_READY: &str = "ClustersReady";

/// Some namespace-scoped `Bind9Cluster` resources are not ready.
pub const REASON_CLUSTERS_PROGRESSING: &str = "ClustersProgressing";

// ============================================================================
// Network and External Service Reasons
// ============================================================================

/// Cannot reach upstream or external services.
///
/// This is used when a secondary instance cannot reach its configured primary servers
/// for zone transfers, or when any resource cannot reach required external dependencies.
pub const REASON_UPSTREAM_UNREACHABLE: &str = "UpstreamUnreachable";

// ============================================================================
// Condition Types
// ============================================================================

/// Primary condition type indicating overall resource readiness.
pub const CONDITION_TYPE_READY: &str = "Ready";

/// Condition type prefix for tracking individual `Bind9Instance` children.
///
/// Format: `Bind9Instance-{index}` (e.g., "Bind9Instance-0", "Bind9Instance-1")
pub const CONDITION_TYPE_BIND9_INSTANCE_PREFIX: &str = "Bind9Instance";

/// Condition type prefix for tracking individual `Pod` children.
///
/// Format: `Pod-{index}` (e.g., "Pod-0", "Pod-1")
pub const CONDITION_TYPE_POD_PREFIX: &str = "Pod";

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a condition type for a specific `Bind9Instance` child.
///
/// # Arguments
///
/// * `index` - The index of the `Bind9Instance` (e.g., 0, 1, 2)
///
/// # Returns
///
/// A condition type string like "Bind9Instance-0"
///
/// # Example
///
/// ```rust
/// use bindy::status_reasons::bind9_instance_condition_type;
///
/// let condition_type = bind9_instance_condition_type(0);
/// assert_eq!(condition_type, "Bind9Instance-0");
/// ```
#[must_use]
pub fn bind9_instance_condition_type(index: usize) -> String {
    format!("{CONDITION_TYPE_BIND9_INSTANCE_PREFIX}-{index}")
}

/// Create a condition type for a specific Pod child.
///
/// # Arguments
///
/// * `index` - The index of the Pod (e.g., 0, 1, 2)
///
/// # Returns
///
/// A condition type string like "Pod-0"
///
/// # Example
///
/// ```rust
/// use bindy::status_reasons::pod_condition_type;
///
/// let condition_type = pod_condition_type(0);
/// assert_eq!(condition_type, "Pod-0");
/// ```
#[must_use]
pub fn pod_condition_type(index: usize) -> String {
    format!("{CONDITION_TYPE_POD_PREFIX}-{index}")
}

/// Extract the index from a child condition type.
///
/// # Arguments
///
/// * `condition_type` - A condition type like "Bind9Instance-0" or "Pod-2"
///
/// # Returns
///
/// The index as `Option<usize>`, or `None` if the format is invalid.
///
/// # Example
///
/// ```rust
/// use bindy::status_reasons::extract_child_index;
///
/// assert_eq!(extract_child_index("Bind9Instance-0"), Some(0));
/// assert_eq!(extract_child_index("Pod-5"), Some(5));
/// assert_eq!(extract_child_index("Ready"), None);
/// assert_eq!(extract_child_index("Invalid-Format"), None);
/// ```
#[must_use]
pub fn extract_child_index(condition_type: &str) -> Option<usize> {
    condition_type
        .rsplit_once('-')
        .and_then(|(_, index_str)| index_str.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind9_instance_condition_type() {
        assert_eq!(bind9_instance_condition_type(0), "Bind9Instance-0");
        assert_eq!(bind9_instance_condition_type(5), "Bind9Instance-5");
        assert_eq!(bind9_instance_condition_type(99), "Bind9Instance-99");
    }

    #[test]
    fn test_pod_condition_type() {
        assert_eq!(pod_condition_type(0), "Pod-0");
        assert_eq!(pod_condition_type(3), "Pod-3");
        assert_eq!(pod_condition_type(10), "Pod-10");
    }

    #[test]
    fn test_extract_child_index() {
        // Valid formats
        assert_eq!(extract_child_index("Bind9Instance-0"), Some(0));
        assert_eq!(extract_child_index("Bind9Instance-15"), Some(15));
        assert_eq!(extract_child_index("Pod-0"), Some(0));
        assert_eq!(extract_child_index("Pod-9"), Some(9));

        // Invalid formats
        assert_eq!(extract_child_index("Ready"), None);
        assert_eq!(extract_child_index("Bind9Instance"), None);
        assert_eq!(extract_child_index("Bind9Instance-"), None);
        assert_eq!(extract_child_index("Bind9Instance-abc"), None);
        assert_eq!(extract_child_index(""), None);
    }
}
