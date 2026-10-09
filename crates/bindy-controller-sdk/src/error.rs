// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The reconcile error type and error policy every controller shares, and
//! the retry and convergence actions a reconcile returns itself (ADR-0016).

use kube::runtime::controller::Action;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tracing::error;

/// Reconciliation error wrapper: any [`anyhow::Error`] a reconciler returns.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ReconcileError(#[from] anyhow::Error);

/// Error policy for every controller.
///
/// Requeues the failed object after its per-object exponential backoff
/// ([`crate::retry::reconcile_error_backoff`]), so a resource that keeps
/// failing is retried less often instead of on a fixed short interval. The
/// backoff key includes the resource's type as well as its namespace and
/// name: two different kinds can share a namespaced name, and they must not
/// share a failure counter.
///
/// This delay is what actually bounds recovery. When an operand Pod is
/// replaced, the Endpoints watch does not reliably pull the DNSZone forward
/// (a retry is already scheduled, and the pending requeue wins), so the zone
/// waits out this timer. With the previous flat 30s requeue a zone was
/// measured idling for 57 seconds after its Pod was back and Ready, then
/// reconciling once and serving in about 1 second. Records need the same
/// treatment: they are re-pushed into a replaced Pod just like the zone.
///
/// # Arguments
///
/// * `resource` - The object whose reconciliation failed
/// * `err` - The reconciliation error
/// * `_ctx` - The controller context (unused)
///
/// # Returns
///
/// An `Action` requeueing the object after its current backoff.
#[allow(clippy::needless_pass_by_value)] // Signature required by kube::runtime::Controller
pub fn error_policy<T, C>(resource: Arc<T>, err: &ReconcileError, _ctx: Arc<C>) -> Action
where
    T: Debug + kube::ResourceExt,
{
    let delay = crate::retry::reconcile_error_backoff(&backoff_key(resource.as_ref()));

    error!(
        error = %err,
        resource = ?resource,
        "Reconciliation error - will retry in {:?}",
        delay
    );
    Action::requeue(delay)
}

/// The per-object backoff key: the resource's type, namespace and name.
///
/// Two different kinds can share a namespaced name, and they must not share a
/// failure counter. [`error_policy`], [`retry_action`] and
/// [`converged_action`] all use this key, so a failure reported as `Err` and
/// one reported from an `Ok` outcome advance the same counter.
///
/// # Arguments
///
/// * `resource` - The object being reconciled
#[must_use]
pub fn backoff_key<T: kube::ResourceExt>(resource: &T) -> String {
    format!(
        "{}/{}/{}",
        std::any::type_name::<T>(),
        resource.namespace().unwrap_or_default(),
        resource.name_any()
    )
}

/// Retry this object after its per-object backoff.
///
/// For a reconcile that finished `Ok` (its status already says what went
/// wrong) but failed against BIND9 or bindcar: a degraded zone, a record write
/// that could not reach a primary. The delay is the same capped exponential
/// backoff [`error_policy`] uses, so a failure is a retry, never a
/// fixed-interval resync (ADR-0016).
///
/// # Arguments
///
/// * `resource` - The object whose reconcile failed
#[must_use]
pub fn retry_action<T: kube::ResourceExt>(resource: &T) -> Action {
    Action::requeue(crate::retry::reconcile_error_backoff(&backoff_key(
        resource,
    )))
}

/// Retry this object on a short interval for a bounded number of times, then
/// on its backoff (see [`crate::retry::bounded_fast_retry`]).
///
/// For a wait that no Kubernetes event ends, such as a secondary's zone
/// transfer completing inside BIND9.
///
/// # Arguments
///
/// * `resource` - The object to recheck
/// * `interval` - The short recheck interval
/// * `budget` - How many consecutive rechecks may use `interval`
#[must_use]
pub fn fast_retry_action<T: kube::ResourceExt>(
    resource: &T,
    interval: Duration,
    budget: u32,
) -> Action {
    Action::requeue(crate::retry::bounded_fast_retry(
        &backoff_key(resource),
        interval,
        budget,
    ))
}

/// [`retry_action`], but never sooner than `floor`.
///
/// A record write BIND9 rejected must not be re-issued inside its cooldown
/// ([`crate::retry::REJECTED_WRITE_COOLDOWN`]); the retry is scheduled for
/// whichever is later, the backoff or the floor.
///
/// # Arguments
///
/// * `resource` - The object whose reconcile failed
/// * `floor` - The shortest acceptable delay
#[must_use]
pub fn retry_action_at_least<T: kube::ResourceExt>(resource: &T, floor: Duration) -> Action {
    let delay = crate::retry::reconcile_error_backoff(&backoff_key(resource));
    Action::requeue(delay.max(floor))
}

/// The object converged: clear its backoff and wait for the next change.
///
/// Clearing the counter makes the next failure start from the fast initial
/// interval rather than wherever an earlier run of failures left it.
///
/// # Arguments
///
/// * `resource` - The object that converged
#[must_use]
pub fn converged_action<T: kube::ResourceExt>(resource: &T) -> Action {
    crate::retry::reset_reconcile_backoff(&backoff_key(resource));
    Action::await_change()
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;
