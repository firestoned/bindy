// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

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

/// Configuration validation failed.
pub const REASON_CONFIGURATION_INVALID: &str = "ConfigurationInvalid";

// ============================================================================
// Bind9Instance Specific Reasons
// ============================================================================

/// Deployment has exceeded its progress deadline.
///
/// This occurs when a Deployment fails to make progress within its configured
/// `progressDeadlineSeconds`. Common causes include image pull errors, insufficient
/// resources, or crashing containers.
pub const REASON_PROGRESS_DEADLINE_EXCEEDED: &str = "ProgressDeadlineExceeded";

/// Requested zone or resource not found in BIND9.
///
/// Maps to HTTP 404 Not Found.
/// This indicates the zone, record, or resource does not exist in the
/// BIND9 server configuration.
pub const REASON_ZONE_NOT_FOUND: &str = "ZoneNotFound";

// ============================================================================
// Condition Types
// ============================================================================

/// Primary condition type indicating overall resource readiness.
pub const CONDITION_TYPE_READY: &str = "Ready";

/// Condition type reporting that the resource works but not fully: some part
/// of the desired state could not be reached.
pub const CONDITION_TYPE_DEGRADED: &str = "Degraded";

/// Condition type reporting that a reconciliation is moving the resource
/// towards its desired state.
pub const CONDITION_TYPE_PROGRESSING: &str = "Progressing";

/// Condition type prefix for tracking individual `Bind9Instance` children.
///
/// Format: `Bind9Instance-{index}` (e.g., "Bind9Instance-0", "Bind9Instance-1")
pub const CONDITION_TYPE_BIND9_INSTANCE_PREFIX: &str = "Bind9Instance";

/// Condition type prefix for tracking individual `Pod` children.
///
/// Format: `Pod-{index}` (e.g., "Pod-0", "Pod-1")
pub const CONDITION_TYPE_POD_PREFIX: &str = "Pod";

/// Condition type of a `Bind9Instance` whose pod-template change is staggered
/// behind another instance's rollout (ADR-0018). Present only while there is
/// something to report.
pub const CONDITION_TYPE_ROLLOUT: &str = "Rollout";

/// `Rollout=False`: the instance has a pod-template change to roll out and
/// waits for another instance that shares a zone or a cluster with it. The
/// message names that instance; the instance's pods keep serving the previous
/// configuration and its `Ready` condition is unaffected (ADR-0018).
pub const REASON_ROLLOUT_QUEUED: &str = "RolloutQueued";

/// `Rollout=True`: the instance rolled out without waiting for a conflicting
/// instance whose own rollout exceeded its `progressDeadlineSeconds`
/// (ADR-0018). The message names that instance.
pub const REASON_ROLLOUT_PEER_STALLED: &str = "RolloutPeerStalled";

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
/// use bindy_api::status_reasons::bind9_instance_condition_type;
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
/// use bindy_api::status_reasons::pod_condition_type;
///
/// let condition_type = pod_condition_type(0);
/// assert_eq!(condition_type, "Pod-0");
/// ```
#[must_use]
pub fn pod_condition_type(index: usize) -> String {
    format!("{CONDITION_TYPE_POD_PREFIX}-{index}")
}
