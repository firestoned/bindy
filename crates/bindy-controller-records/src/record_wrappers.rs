// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Record reconciliation wrapper helpers and macro.
//!
//! This module provides helper functions and a macro to generate reconciliation
//! wrapper functions for all DNS record types, eliminating ~900 lines of duplicate code.

use crate::crd::RecordStatus;
use kube::runtime::controller::Action;
use std::time::Duration;

/// Condition type for resource readiness
pub const CONDITION_TYPE_READY: &str = "Ready";

/// Condition status indicating ready state
pub const CONDITION_STATUS_TRUE: &str = "True";

/// Error type label for reconciliation errors
pub const ERROR_TYPE_RECONCILE: &str = "reconcile_error";

/// Stand-in for a condition that omits its optional `reason` or `message`.
pub const UNKNOWN_CONDITION_FIELD: &str = "<none>";

/// What a record's own status says about whether it reached BIND9.
///
/// A reconcile pass can finish without the record being published — the add is
/// reported through `status.conditions`, not through the reconcile's return
/// value — so callers that want to describe the outcome need the reason, not
/// just a boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyState<'a> {
    /// The `Ready` condition is `True`: the record is published.
    Ready,
    /// The `Ready` condition is not `True`, with the reason it reports.
    NotReady {
        /// CamelCase reason from the condition, e.g. `ReconcileFailed`.
        reason: &'a str,
        /// Human-readable detail from the condition.
        message: &'a str,
    },
    /// No status, or no `Ready` condition to read.
    Unknown,
}

/// Read the `Ready` condition of a record's status.
///
/// The condition is located by type rather than by position: a status writer is
/// free to order conditions, so position must not decide readiness.
///
/// # Arguments
///
/// * `status` - Optional status containing conditions
///
/// # Returns
///
/// [`ReadyState::Ready`], [`ReadyState::NotReady`] carrying the condition's
/// reason and message, or [`ReadyState::Unknown`] when there is no `Ready`
/// condition to read.
#[must_use]
pub fn ready_state(status: &Option<RecordStatus>) -> ReadyState<'_> {
    let Some(status) = status.as_ref() else {
        return ReadyState::Unknown;
    };

    let Some(condition) = status
        .conditions
        .iter()
        .find(|condition| condition.r#type == CONDITION_TYPE_READY)
    else {
        return ReadyState::Unknown;
    };

    if condition.status == CONDITION_STATUS_TRUE {
        return ReadyState::Ready;
    }

    ReadyState::NotReady {
        reason: condition
            .reason
            .as_deref()
            .unwrap_or(UNKNOWN_CONDITION_FIELD),
        message: condition
            .message
            .as_deref()
            .unwrap_or(UNKNOWN_CONDITION_FIELD),
    }
}

/// Status reason: no `DNSZone` selects the record (no `status.zoneRef`).
pub const REASON_NOT_SELECTED: &str = "NotSelected";

/// Status reason: the zone named by `status.zoneRef` does not exist.
pub const REASON_ZONE_NOT_FOUND: &str = "ZoneNotFound";

/// Status reason: the zone selects no `Bind9Instance`.
pub const REASON_ZONE_NOT_CONFIGURED: &str = "ZoneNotConfigured";

/// Status reason: the zone has instances but none is a primary.
pub const REASON_NO_PRIMARY_INSTANCES: &str = "NoPrimaryInstances";

/// Status reason: the primary instances could not be determined.
pub const REASON_INSTANCE_FILTER_ERROR: &str = "InstanceFilterError";

/// Status reason: a BIND9 write (or the rename delete before it) failed.
pub const REASON_RECONCILE_FAILED: &str = "ReconcileFailed";

/// Status reason: the record is published to every primary.
pub const REASON_RECONCILE_SUCCEEDED: &str = "ReconcileSucceeded";

/// How one record reconcile ended, returned to the controller wrapper so it
/// can log and pick its `Action` without re-reading the record (ADR-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    /// Published to every primary; the record is Ready.
    Published,
    /// Not Ready, waiting on another object whose event wakes the record:
    /// a zone tagging it, the zone appearing, the zone gaining (primary)
    /// instances.
    Waiting {
        /// The status reason, e.g. [`REASON_NOT_SELECTED`].
        reason: &'static str,
    },
    /// Not Ready because a lookup failed; retried with backoff.
    Failed {
        /// The status reason, e.g. [`REASON_INSTANCE_FILTER_ERROR`].
        reason: &'static str,
    },
    /// BIND9 rejected the write or could not be reached; retried with
    /// backoff, never inside the rejected-write cooldown.
    WriteRejected,
    /// The identical spec was rejected moments ago; the write is skipped
    /// until the cooldown ends.
    CoolingDown {
        /// Time left in the cooldown.
        remaining: Duration,
    },
}

impl RecordOutcome {
    /// Whether the record is published and Ready.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Published)
    }

    /// A short description for the controller's log line.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Published => REASON_RECONCILE_SUCCEEDED.to_string(),
            Self::Waiting { reason } => format!("{reason} (waiting for an event)"),
            Self::Failed { reason } => format!("{reason} (retrying with backoff)"),
            Self::WriteRejected => format!("{REASON_RECONCILE_FAILED} (retrying with backoff)"),
            Self::CoolingDown { remaining } => {
                format!(
                    "{REASON_RECONCILE_FAILED} (rejected write cooling down, {remaining:?} left)"
                )
            }
        }
    }
}

/// The controller `Action` for a record reconcile's outcome (ADR-0016).
///
/// There is no periodic resync: a published record, and a record waiting on
/// another object, wait for the next watch event. A failure retries on the
/// per-object backoff; a rejected write never sooner than
/// [`bindy_controller_sdk::retry::REJECTED_WRITE_COOLDOWN`].
///
/// # Arguments
///
/// * `record` - The record that was reconciled (keys the backoff)
/// * `outcome` - How the reconcile ended
///
/// # Returns
///
/// `await_change` for [`RecordOutcome::Published`] (clearing the backoff) and
/// [`RecordOutcome::Waiting`]; a backing-off requeue otherwise.
#[must_use]
pub fn action_for_outcome<T: kube::ResourceExt>(record: &T, outcome: &RecordOutcome) -> Action {
    match outcome {
        RecordOutcome::Published => bindy_controller_sdk::error::converged_action(record),
        RecordOutcome::Waiting { .. } => Action::await_change(),
        RecordOutcome::Failed { .. } => bindy_controller_sdk::error::retry_action(record),
        RecordOutcome::WriteRejected => bindy_controller_sdk::error::retry_action_at_least(
            record,
            bindy_controller_sdk::retry::REJECTED_WRITE_COOLDOWN,
        ),
        RecordOutcome::CoolingDown { remaining } => Action::requeue(*remaining),
    }
}
