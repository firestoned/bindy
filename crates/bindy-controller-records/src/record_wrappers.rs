// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Record reconciliation wrapper helpers and macro.
//!
//! This module provides helper functions and a macro to generate reconciliation
//! wrapper functions for all DNS record types, eliminating ~900 lines of duplicate code.

use crate::crd::RecordStatus;
use kube::runtime::controller::Action;
use std::time::Duration;

// The requeue policy is shared by every controller (bindy-controller-sdk).
pub use bindy_controller_sdk::requeue::{REQUEUE_WHEN_NOT_READY_SECS, REQUEUE_WHEN_READY_SECS};

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

/// Determine requeue action based on readiness status.
///
/// # Arguments
///
/// * `is_ready` - Whether the resource is ready
///
/// # Returns
///
/// * `Action::requeue(5 minutes)` if ready
/// * `Action::requeue(30 seconds)` if not ready
#[must_use]
pub fn requeue_based_on_readiness(is_ready: bool) -> Action {
    if is_ready {
        Action::requeue(Duration::from_secs(REQUEUE_WHEN_READY_SECS))
    } else {
        Action::requeue(Duration::from_secs(REQUEUE_WHEN_NOT_READY_SECS))
    }
}
