// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

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
//! use bindy::reconcilers::status::create_condition;
//! use bindy::crd::Condition;
//!
//! let condition = create_condition(
//!     "Ready",
//!     "True",
//!     "DeploymentReady",
//!     "All replicas are running"
//! );
//! ```

use crate::crd::{Condition, DNSZone, DNSZoneStatus, RecordReferenceWithTimestamp};
use anyhow::Result;
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
/// # use bindy::reconcilers::status::create_condition;
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
/// # use bindy::reconcilers::status::{create_condition, condition_changed};
/// # use bindy::crd::Condition;
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
/// # use bindy::reconcilers::status::get_last_transition_time;
/// # use bindy::crd::Condition;
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
/// # use bindy::reconcilers::status::find_condition;
/// # use bindy::crd::Condition;
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
/// use bindy::reconcilers::status::update_condition_in_memory;
/// use bindy::crd::DNSZoneStatus;
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
/// use bindy::reconcilers::status::conditions_equal;
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
/// use bindy::reconcilers::status::DNSZoneStatusUpdater;
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
        status: crate::crd::InstanceStatus,
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
                .push(crate::crd::InstanceReferenceWithStatus {
                    api_version: crate::constants::API_GROUP_VERSION.to_string(),
                    kind: crate::constants::KIND_BIND9_INSTANCE.to_string(),
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

    /// Get a reference to the conditions list (for testing).
    ///
    /// # Returns
    ///
    /// A reference to the conditions vector in the new status.
    #[cfg(test)]
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

        api.patch_status(&self.name, &PatchParams::default(), &Patch::Merge(&patch))
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
