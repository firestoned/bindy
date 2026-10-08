// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Status condition helpers for Kubernetes resources.
//!
//! This module provides utility functions for creating and managing Kubernetes
//! status conditions following the standard conventions.
//!
//! # Condition Format
//!
//! Kubernetes conditions follow a standard format:
//! - `type`: The aspect of the resource being reported (e.g., "Ready", "Progressing")
//! - `status`: "True", "False", or "Unknown"
//! - `reason`: A programmatic identifier (CamelCase)
//! - `message`: A human-readable explanation
//! - `lastTransitionTime`: RFC3339 timestamp when the condition changed
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy_controller_sdk::status::create_condition;
//! use bindy_api::crd::Condition;
//!
//! let condition = create_condition(
//!     "Ready",
//!     "True",
//!     "DeploymentReady",
//!     "All replicas are running"
//! );
//! ```

use anyhow::Result;
use bindy_api::crd::{Condition, DNSZone, DNSZoneStatus, RecordReferenceWithTimestamp};
use chrono::Utc;
use kube::api::Patch;
use kube::{api::PatchParams, Api, Client, ResourceExt};
use serde_json::json;
use tracing::debug;

/// Create a new Kubernetes condition with the current timestamp.
///
/// This is a convenience function for creating conditions that follow Kubernetes
/// conventions. The `lastTransitionTime` is automatically set to the current time.
///
/// # Arguments
///
/// * `condition_type` - The type of condition (e.g., "Ready", "Progressing")
/// * `status` - The status: "True", "False", or "Unknown"
/// * `reason` - A programmatic identifier in `CamelCase` (e.g., "`DeploymentReady`")
/// * `message` - A human-readable explanation
///
/// # Returns
///
/// A new `Condition` with the current timestamp.
///
/// # Example
///
/// ```rust,no_run
/// # use bindy_controller_sdk::status::create_condition;
/// let condition = create_condition(
///     "Ready",
///     "True",
///     "AllPodsRunning",
///     "All 3 pods are running and ready"
/// );
/// assert_eq!(condition.r#type, "Ready");
/// assert_eq!(condition.status, "True");
/// ```
#[must_use]
pub fn create_condition(
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
) -> Condition {
    Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(Utc::now().to_rfc3339()),
    }
}

/// Check if a condition has changed compared to the existing status.
///
/// This function compares a new condition against an existing condition from the
/// resource's status. It returns `true` if the condition has changed and should
/// be updated, or `false` if it's unchanged.
///
/// A condition is considered changed if:
/// - The condition type is different
/// - The status value is different ("True" vs "False")
/// - The message is different
///
/// The `reason` and `lastTransitionTime` are not compared, as these typically
/// change with the condition itself.
///
/// # Arguments
///
/// * `existing` - The existing condition from the resource's status (if any)
/// * `new_condition` - The new condition to compare against
///
/// # Returns
///
/// * `true` - The condition has changed and should be updated
/// * `false` - The condition is unchanged, skip the update
///
/// # Example
///
/// ```rust,no_run
/// # use bindy_controller_sdk::status::{create_condition, condition_changed};
/// # use bindy_api::crd::Condition;
/// let existing = Some(create_condition("Ready", "False", "Pending", "Waiting"));
/// let new_cond = create_condition("Ready", "True", "Running", "All pods ready");
///
/// if condition_changed(&existing, &new_cond) {
///     // Update the status
/// }
/// ```
#[must_use]
pub fn condition_changed(existing: &Option<Condition>, new_condition: &Condition) -> bool {
    if let Some(current) = existing {
        current.r#type != new_condition.r#type
            || current.status != new_condition.status
            || current.message != new_condition.message
    } else {
        // No existing condition, so it has changed
        true
    }
}

/// Get the last transition time from an existing condition, or current time if none exists.
///
/// When updating a condition, we want to preserve the `lastTransitionTime` if the
/// condition status hasn't actually changed. This function retrieves the existing
/// timestamp if available, or returns the current time for new conditions.
///
/// This is useful for preserving transition times when only the message changes
/// but the overall status remains the same.
///
/// # Arguments
///
/// * `existing_conditions` - The existing conditions from the resource's status
/// * `condition_type` - The type of condition to look for
///
/// # Returns
///
/// The existing `lastTransitionTime` if found, otherwise the current time as RFC3339.
///
/// # Example
///
/// ```rust,no_run
/// # use bindy_controller_sdk::status::get_last_transition_time;
/// # use bindy_api::crd::Condition;
/// let existing_conditions = vec![]; // From resource status
/// let time = get_last_transition_time(&existing_conditions, "Ready");
/// ```
#[must_use]
pub fn get_last_transition_time(existing_conditions: &[Condition], condition_type: &str) -> String {
    existing_conditions
        .iter()
        .find(|c| c.r#type == condition_type)
        .and_then(|c| c.last_transition_time.as_ref())
        .map_or_else(|| Utc::now().to_rfc3339(), std::string::ToString::to_string)
}

/// Find a condition by type in a list of conditions.
///
/// This is a convenience function for finding a specific condition type
/// in a resource's status conditions.
///
/// # Arguments
///
/// * `conditions` - The list of conditions to search
/// * `condition_type` - The type of condition to find (e.g., "Ready")
///
/// # Returns
///
/// The matching condition if found, otherwise `None`.
///
/// # Example
///
/// ```rust,no_run
/// # use bindy_controller_sdk::status::find_condition;
/// # use bindy_api::crd::Condition;
/// let conditions = vec![]; // From resource status
/// if let Some(ready_condition) = find_condition(&conditions, "Ready") {
///     println!("Ready status: {}", ready_condition.status);
/// }
/// ```
#[must_use]
pub fn find_condition<'a>(
    conditions: &'a [Condition],
    condition_type: &str,
) -> Option<&'a Condition> {
    conditions.iter().find(|c| c.r#type == condition_type)
}

/// Update or add a condition in a mutable conditions list (in-memory, no API call).
///
/// This function modifies the conditions list in-place by either updating an existing
/// condition or adding a new one. It preserves the `lastTransitionTime` if the status
/// hasn't changed, or sets a new timestamp if it has.
///
/// **Important:** This function does NOT make any Kubernetes API calls. It only modifies
/// the in-memory conditions list. You must call `patch_status()` separately to persist
/// the changes.
///
/// # Arguments
///
/// * `conditions` - Mutable reference to the conditions list
/// * `condition_type` - The type of condition (e.g., "Ready", "Progressing")
/// * `status` - The status: "True", "False", or "Unknown"
/// * `reason` - A programmatic identifier in `CamelCase`
/// * `message` - A human-readable explanation
///
/// # Example
///
/// ```rust,ignore
/// use bindy_controller_sdk::status::update_condition_in_memory;
/// use bindy_api::crd::DNSZoneStatus;
///
/// let mut status = DNSZoneStatus::default();
/// update_condition_in_memory(
///     &mut status.conditions,
///     "Ready",
///     "True",
///     "ZoneConfigured",
///     "Zone configured on 3 servers"
/// );
/// ```
pub fn update_condition_in_memory(
    conditions: &mut Vec<Condition>,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
) {
    // Find existing condition
    if let Some(existing) = conditions.iter_mut().find(|c| c.r#type == condition_type) {
        // Preserve lastTransitionTime if status hasn't changed
        let last_transition_time = if existing.status == status {
            existing
                .last_transition_time
                .clone()
                .unwrap_or_else(|| Utc::now().to_rfc3339())
        } else {
            Utc::now().to_rfc3339()
        };

        existing.status = status.to_string();
        existing.reason = Some(reason.to_string());
        existing.message = Some(message.to_string());
        existing.last_transition_time = Some(last_transition_time);
    } else {
        // Create new condition
        conditions.push(create_condition(condition_type, status, reason, message));
    }
}

/// Compare two condition lists to check if they are semantically equal.
///
/// This function compares two lists of conditions to determine if they represent
/// the same state. It ignores `lastTransitionTime` differences and only compares
/// the semantic content (type, status, reason, message).
///
/// # Arguments
///
/// * `current` - The current conditions list
/// * `new` - The new conditions list to compare
///
/// # Returns
///
/// * `true` - The conditions are semantically equal (no update needed)
/// * `false` - The conditions differ (update needed)
///
/// # Example
///
/// ```rust,ignore
/// use bindy_controller_sdk::status::conditions_equal;
///
/// let current_conditions = vec![/* ... */];
/// let new_conditions = vec![/* ... */];
///
/// if !conditions_equal(&current_conditions, &new_conditions) {
///     // Conditions changed, update status
/// }
/// ```
#[must_use]
pub fn conditions_equal(current: &[Condition], new: &[Condition]) -> bool {
    if current.len() != new.len() {
        return false;
    }

    for new_cond in new {
        match current.iter().find(|c| c.r#type == new_cond.r#type) {
            None => return false,
            Some(curr_cond) => {
                if curr_cond.status != new_cond.status
                    || curr_cond.reason != new_cond.reason
                    || curr_cond.message != new_cond.message
                {
                    return false;
                }
            }
        }
    }

    true
}

/// Centralized status updater for `DNSZone` resources.
///
/// This struct collects all status changes during reconciliation and applies them
/// atomically in a single Kubernetes API call. This prevents the tight reconciliation
/// loop caused by multiple status updates triggering multiple "object updated" events.
///
/// **Pattern aligns with kube-condition project for future migration.**
///
/// # Example
///
/// ```rust,ignore
/// use bindy_controller_sdk::status::DNSZoneStatusUpdater;
///
/// async fn reconcile(client: Client, zone: DNSZone) -> Result<()> {
///     let mut status_updater = DNSZoneStatusUpdater::new(&zone);
///
///     // Collect status changes in memory
///     status_updater.set_condition("Progressing", "True", "Configuring", "Setting up zone");
///     status_updater.set_records(vec![/* discovered records */]);
///
///     // Single atomic update at the end
///     status_updater.apply(&client).await?;
///     Ok(())
/// }
/// ```
pub struct DNSZoneStatusUpdater {
    namespace: String,
    name: String,
    current_status: Option<DNSZoneStatus>,
    new_status: DNSZoneStatus,
    has_changes: bool,
    degraded_set_this_reconciliation: bool,
}

impl DNSZoneStatusUpdater {
    /// Create a new status updater for a `DNSZone`.
    ///
    /// Initializes with the current status from the zone, or creates a new empty status.
    #[must_use]
    pub fn new(dnszone: &DNSZone) -> Self {
        let current_status = dnszone.status.clone();
        let new_status = current_status.clone().unwrap_or_default();

        Self {
            namespace: dnszone.namespace().unwrap_or_default(),
            name: dnszone.name_any(),
            current_status,
            new_status,
            has_changes: false,
            degraded_set_this_reconciliation: false,
        }
    }

    /// Update or add a condition (in-memory only, no API call).
    ///
    /// Marks the status as changed if the condition differs from the current state.
    pub fn set_condition(
        &mut self,
        condition_type: &str,
        status: &str,
        reason: &str,
        message: &str,
    ) {
        // Track if we're setting Degraded=True during this reconciliation
        if condition_type == "Degraded" && status == "True" {
            self.degraded_set_this_reconciliation = true;
        }

        update_condition_in_memory(
            &mut self.new_status.conditions,
            condition_type,
            status,
            reason,
            message,
        );
        self.has_changes = true;
    }

    /// Set the discovered DNS records list (in-memory only, no API call).
    pub fn set_records(&mut self, records: &[RecordReferenceWithTimestamp]) {
        records.clone_into(&mut self.new_status.records);
        // Update records_count whenever records changes
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        {
            self.new_status.records_count =
                i32::try_from(self.new_status.records.len()).unwrap_or(0);
        }
        self.has_changes = true;
    }

    /// Record whether this zone still owes BIND9 a full record replay
    /// (in-memory only, no API call).
    ///
    /// Set to `true` as soon as the controller creates the zone on any server
    /// endpoint - a created zone holds only SOA and NS records, so every record
    /// CR is missing from it. Set to `false` only once every record has been
    /// pushed to every primary endpoint successfully.
    ///
    /// # Arguments
    ///
    /// * `pending` - Whether a full record replay is still outstanding
    pub fn set_records_resync_pending(&mut self, pending: bool) {
        if self.new_status.records_resync_pending == pending {
            return;
        }
        self.new_status.records_resync_pending = pending;
        self.has_changes = true;
    }

    /// Whether a full record replay is currently marked as outstanding.
    #[must_use]
    pub fn records_resync_pending(&self) -> bool {
        self.new_status.records_resync_pending
    }

    /// Set (or clear, with `None`) the zone's DNSSEC status (ADR-0006).
    ///
    /// A value equal to what the update already carries is a no-op, so DS
    /// reporting never dirties an otherwise unchanged status.
    ///
    /// # Arguments
    ///
    /// * `dnssec` - DS records and signing state, or `None` when the zone has
    ///   no effective DNSSEC policy
    pub fn set_dnssec(&mut self, dnssec: Option<bindy_api::crd::DNSSECStatus>) {
        if self.new_status.dnssec == dnssec {
            return;
        }
        self.new_status.dnssec = dnssec;
        self.has_changes = true;
    }

    /// The DNSSEC status this update currently carries.
    #[must_use]
    pub fn dnssec(&self) -> Option<&bindy_api::crd::DNSSECStatus> {
        self.new_status.dnssec.as_ref()
    }

    /// Record the zone-transfer peers just pushed to every server of the zone
    /// (ADR-0019). A value equal to what the update already carries is a
    /// no-op, so a reconcile with unchanged peers never dirties the status.
    ///
    /// # Arguments
    ///
    /// * `peers` - The peer sets every primary and secondary now names
    pub fn set_transfer_peers(&mut self, peers: bindy_api::crd::ZoneTransferPeers) {
        if self.new_status.transfer_peers.as_ref() == Some(&peers) {
            return;
        }
        self.new_status.transfer_peers = Some(peers);
        self.has_changes = true;
    }

    /// The zone-transfer peers this update currently carries: the ones last
    /// recorded, or the ones set in this reconcile.
    #[must_use]
    pub fn transfer_peers(&self) -> Option<&bindy_api::crd::ZoneTransferPeers> {
        self.new_status.transfer_peers.as_ref()
    }

    /// Set the observed generation to match the current generation.
    pub fn set_observed_generation(&mut self, generation: Option<i64>) {
        self.new_status.observed_generation = generation;
        self.has_changes = true;
    }

    /// Update instance status (in-memory only, no API call).
    ///
    /// Updates the status of a specific instance in the `status.bind9Instances` list.
    /// Creates a new entry if the instance doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `name` - Instance name
    /// * `namespace` - Instance namespace
    /// * `status` - New status (Claimed, Configured, Failed, Unclaimed)
    /// * `message` - Optional status message (error details, etc.)
    pub fn update_instance_status(
        &mut self,
        name: &str,
        namespace: &str,
        status: bindy_api::crd::InstanceStatus,
        message: Option<String>,
    ) {
        use chrono::Utc;
        let now = Utc::now().to_rfc3339();

        // Find existing instance or create new one
        if let Some(instance) = self
            .new_status
            .bind9_instances
            .iter_mut()
            .find(|i| i.namespace == namespace && i.name == name)
        {
            // Update existing instance
            instance.status = status;
            instance.last_reconciled_at = Some(now);
            instance.message = message;
        } else {
            // Add new instance
            self.new_status
                .bind9_instances
                .push(bindy_api::crd::InstanceReferenceWithStatus {
                    api_version: bindy_api::constants::API_GROUP_VERSION.to_string(),
                    kind: bindy_api::constants::KIND_BIND9_INSTANCE.to_string(),
                    name: name.to_string(),
                    namespace: namespace.to_string(),
                    status,
                    last_reconciled_at: Some(now),
                    message,
                });
        }
        // Update bind9_instances_count whenever bind9_instances changes
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        {
            self.new_status.bind9_instances_count =
                i32::try_from(self.new_status.bind9_instances.len()).ok();
        }
        self.has_changes = true;
    }

    /// Remove instance from the instances list (in-memory only, no API call).
    ///
    /// Removes an instance from `status.bind9Instances` when it no longer claims the zone
    /// or has been deleted.
    ///
    /// # Arguments
    ///
    /// * `name` - Instance name
    /// * `namespace` - Instance namespace
    pub fn remove_instance(&mut self, name: &str, namespace: &str) {
        let initial_len = self.new_status.bind9_instances.len();
        self.new_status
            .bind9_instances
            .retain(|i| !(i.namespace == namespace && i.name == name));

        if self.new_status.bind9_instances.len() != initial_len {
            // Update bind9_instances_count whenever bind9_instances changes
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            {
                self.new_status.bind9_instances_count =
                    i32::try_from(self.new_status.bind9_instances.len()).ok();
            }
            self.has_changes = true;
        }
    }

    /// Check if the status has actually changed compared to the current status.
    ///
    /// Returns `true` if there are semantic changes that warrant an API update.
    ///
    /// **CRITICAL**: The comparison uses `InstanceReferenceWithStatus::eq()` which excludes
    /// `last_reconciled_at` timestamps. Without this, nanosecond precision differences would
    /// cause infinite reconciliation loops.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        if !self.has_changes {
            return false;
        }

        match &self.current_status {
            None => true, // First status update
            Some(current) => {
                current.records != self.new_status.records
                    || current.observed_generation != self.new_status.observed_generation
                    || !conditions_equal(&current.conditions, &self.new_status.conditions)
                    || current.bind9_instances != self.new_status.bind9_instances
                    || current.bind9_instances_count != self.new_status.bind9_instances_count
                    || current.records_resync_pending != self.new_status.records_resync_pending
                    || current.dnssec != self.new_status.dnssec
                    || current.transfer_peers != self.new_status.transfer_peers
            }
        }
    }

    /// Check if a Degraded condition was set during **this** reconciliation.
    ///
    /// Returns `true` only if `set_condition("Degraded", "True", ...)` was called
    /// during this reconciliation, not if a Degraded condition existed from a previous reconciliation.
    #[must_use]
    pub fn has_degraded_condition(&self) -> bool {
        self.degraded_set_this_reconciliation
    }

    /// Clear any Degraded condition by setting it to False (in-memory only, no API call).
    ///
    /// This method should be called when reconciliation succeeds to ensure stale
    /// Degraded conditions from previous failures are cleared.
    ///
    /// If no Degraded condition exists, this method does nothing.
    pub fn clear_degraded_condition(&mut self) {
        self.set_condition("Degraded", "False", "ReconcileSucceeded", "");
        // Reset the tracking flag since we're explicitly clearing the condition
        self.degraded_set_this_reconciliation = false;
    }

    /// Set the Ready condition to False with `DuplicateZone` reason (in-memory only, no API call).
    ///
    /// This method should be called when a duplicate zone is detected to signal that
    /// this zone cannot be reconciled because another zone already claims the same zone name.
    ///
    /// # Arguments
    ///
    /// * `zone_name` - The zone name that has a conflict
    /// * `conflicting_zones` - List of conflicting zone identifiers (namespace/name)
    pub fn set_duplicate_zone_condition(&mut self, zone_name: &str, conflicting_zones: &[String]) {
        let message = format!(
            "A zone with this name '{}' has already been declared in these BIND9 Instances: {}",
            zone_name,
            conflicting_zones.join(", ")
        );
        self.set_condition("Ready", "False", "DuplicateZone", &message);
    }

    /// The conditions collected so far, before they are applied.
    ///
    /// Public rather than test-only so the zone reconciler's tests, in another
    /// crate since the SDK split, can inspect them.
    ///
    /// # Returns
    ///
    /// A reference to the conditions vector in the new status.
    #[must_use]
    pub fn conditions(&self) -> &Vec<Condition> {
        &self.new_status.conditions
    }

    /// Apply the collected status changes to Kubernetes (single atomic API call).
    ///
    /// Only makes the API call if there are actual changes. Skips the update if
    /// the status is semantically unchanged, preventing unnecessary reconciliation loops.
    ///
    /// # Errors
    ///
    /// Returns an error if the Kubernetes API call fails.
    pub async fn apply(&self, client: &Client) -> Result<()> {
        if !self.has_changes() {
            debug!(
                "DNSZone {}/{} status unchanged, skipping update",
                self.namespace, self.name
            );
            return Ok(());
        }

        let api: Api<DNSZone> = Api::namespaced(client.clone(), &self.namespace);

        let patch = json!({
            "status": self.new_status
        });

        let patch_params = PatchParams::default();
        let merge_patch = Patch::Merge(&patch);
        crate::retry::retry_api_call(
            || api.patch_status(&self.name, &patch_params, &merge_patch),
            "patch DNSZone status",
        )
        .await?;

        debug!(
            "Updated DNSZone {}/{} status: {} condition(s), {} record(s)",
            self.namespace,
            self.name,
            self.new_status.conditions.len(),
            self.new_status.records.len()
        );

        Ok(())
    }
}

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
/// use bindy_controller_sdk::status::should_reconcile;
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
/// use bindy_controller_sdk::status::status_changed;
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
#[path = "status_generation_tests.rs"]
mod status_generation_tests;
