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

use crate::crd::Condition;
use chrono::Utc;

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
