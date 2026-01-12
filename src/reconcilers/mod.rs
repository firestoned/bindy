// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Kubernetes reconciliation controllers for DNS resources.
//!
//! This module contains the reconciliation logic for all Bindy Custom Resources.
//! Each reconciler watches for changes to its respective resource type and updates
//! BIND9 configurations accordingly.
//!
//! # Reconciliation Architecture
//!
//! Bindy follows the standard Kubernetes controller pattern:
//!
//! 1. **Watch** - Monitor resource changes via Kubernetes API
//! 2. **Reconcile** - Compare desired state (CRD spec) with actual state
//! 3. **Update** - Modify BIND9 configuration to match desired state
//! 4. **Status** - Report reconciliation results back to Kubernetes
//!
//! # Available Reconcilers
//!
//! ## Infrastructure
//!
//! - [`reconcile_bind9cluster`] - Manages namespace-scoped BIND9 cluster status
//! - [`delete_bind9cluster`] - Cleans up namespace-scoped BIND9 cluster resources
//! - [`reconcile_clusterbind9provider`] - Manages cluster-scoped BIND9 provider status
//! - [`delete_clusterbind9provider`] - Cleans up cluster-scoped BIND9 provider resources
//! - [`reconcile_bind9instance`] - Creates/updates BIND9 server deployments
//! - [`delete_bind9instance`] - Cleans up BIND9 server resources
//!
//! ## DNS Zones
//!
//! - [`reconcile_dnszone`] - Creates/updates DNS zone files
//! - [`delete_dnszone`] - Removes DNS zones
//!
//! ## DNS Records
//!
//! - [`reconcile_a_record`] - Manages IPv4 address records
//! - [`reconcile_aaaa_record`] - Manages IPv6 address records
//! - [`reconcile_cname_record`] - Manages canonical name aliases
//! - [`reconcile_mx_record`] - Manages mail exchange records
//! - [`reconcile_txt_record`] - Manages text records
//! - [`reconcile_ns_record`] - Manages nameserver delegation
//! - [`reconcile_srv_record`] - Manages service location records
//! - [`reconcile_caa_record`] - Manages certificate authority authorization
//!
//! # Example: Using a Reconciler
//!
//! ```rust,no_run
//! use bindy::reconcilers::reconcile_dnszone;
//! use bindy::crd::DNSZone;
//! use bindy::bind9::Bind9Manager;
//! use bindy::context::Context;
//! use std::sync::Arc;
//!
//! async fn reconcile_zone(ctx: Arc<Context>, dnszone: DNSZone) -> anyhow::Result<()> {
//!     let zone_manager = Bind9Manager::new();
//!
//!     reconcile_dnszone(ctx, dnszone, &zone_manager).await?;
//!     Ok(())
//! }
//! ```

pub mod bind9cluster;
pub mod bind9instance;
pub mod clusterbind9provider;
pub mod dnszone;
pub mod finalizers;
pub mod records;
pub mod resources;
pub mod status;

#[cfg(test)]
mod bind9cluster_tests;
#[cfg(test)]
mod bind9instance_tests;
#[cfg(test)]
mod clusterbind9provider_tests;
#[cfg(test)]
mod dnszone_tests;
#[cfg(test)]
mod records_tests;
#[cfg(test)]
mod status_tests;

pub use bind9cluster::{delete_bind9cluster, reconcile_bind9cluster};
pub use bind9instance::{delete_bind9instance, reconcile_bind9instance, reconcile_instance_zones};
pub use clusterbind9provider::{delete_clusterbind9provider, reconcile_clusterbind9provider};
pub use dnszone::{delete_dnszone, discovery::find_zones_selecting_record, reconcile_dnszone};
pub use records::{
    delete_record, reconcile_a_record, reconcile_aaaa_record, reconcile_caa_record,
    reconcile_cname_record, reconcile_mx_record, reconcile_ns_record, reconcile_srv_record,
    reconcile_txt_record,
};

/// Check if a resource's spec has changed by comparing generation with `observed_generation`.
///
/// This is the standard Kubernetes pattern for determining if reconciliation is needed.
/// The `metadata.generation` field is incremented by Kubernetes only when the spec changes,
/// while `status.observed_generation` is set by the controller after processing a spec.
///
/// # Arguments
///
/// * `current_generation` - The resource's current `metadata.generation`
/// * `observed_generation` - The controller's last `status.observed_generation`
///
/// # Returns
///
/// * `true` - Reconciliation is needed (spec changed or first reconciliation)
/// * `false` - No reconciliation needed (spec unchanged, status-only update)
///
/// # Example
///
/// ```rust,ignore
/// use bindy::reconcilers::should_reconcile;
///
/// fn check_if_reconcile_needed(resource: &MyResource) -> bool {
///     let current = resource.metadata.generation;
///     let observed = resource.status.as_ref()
///         .and_then(|s| s.observed_generation);
///
///     should_reconcile(current, observed)
/// }
/// ```
///
/// # Kubernetes Generation Semantics
///
/// - **`metadata.generation`**: Incremented by Kubernetes API server when spec changes
/// - **`status.observed_generation`**: Set by controller to match `metadata.generation` after reconciliation
/// - When they match: spec hasn't changed since last reconciliation → skip work
/// - When they differ: spec has changed → reconcile
/// - When `observed_generation` is None: first reconciliation → reconcile
#[must_use]
pub fn should_reconcile(current_generation: Option<i64>, observed_generation: Option<i64>) -> bool {
    match (current_generation, observed_generation) {
        (Some(current), Some(observed)) => current != observed,
        (Some(_), None) => true, // First reconciliation
        _ => false,              // No generation tracking available
    }
}

/// Check if a status value has actually changed compared to the current status.
///
/// This helper prevents unnecessary status updates that would trigger reconciliation loops.
/// It compares a new status value with the existing status and returns `true` only if
/// they differ, indicating an update is needed.
///
/// # Arguments
///
/// * `current_value` - The current status value (from existing resource)
/// * `new_value` - The new status value to potentially set
///
/// # Returns
///
/// * `true` - Status has changed and needs updating
/// * `false` - Status is unchanged, skip the update
///
/// # Example
///
/// ```rust,ignore
/// use bindy::reconcilers::status_changed;
///
/// let current_ready = instance.status.as_ref()
///     .and_then(|s| s.ready_replicas);
/// let new_ready = Some(3);
///
/// if status_changed(&current_ready, &new_ready) {
///     // Status has changed, safe to update
///     update_status(client, instance, new_ready).await?;
/// }
/// ```
///
/// # Why This Matters
///
/// In kube-rs, status updates trigger "object updated" events which cause new reconciliations.
/// Without this check, updating status on every reconciliation creates a tight loop:
///
/// 1. Reconcile → Update status
/// 2. Status update → "object updated" event
/// 3. Event → New reconciliation
/// 4. Repeat from step 1 (infinite loop)
///
/// By only updating when status actually changes, we break this cycle.
#[must_use]
pub fn status_changed<T: PartialEq>(current_value: &Option<T>, new_value: &Option<T>) -> bool {
    current_value != new_value
}

#[cfg(test)]
mod mod_tests;
