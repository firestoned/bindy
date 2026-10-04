// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! The reconcile error type and error policy every controller shares.

use kube::runtime::controller::Action;
use std::fmt::Debug;
use std::sync::Arc;
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
    let key = format!(
        "{}/{}/{}",
        std::any::type_name::<T>(),
        resource.namespace().unwrap_or_default(),
        resource.name_any()
    );
    let delay = crate::retry::reconcile_error_backoff(&key);

    error!(
        error = %err,
        resource = ?resource,
        "Reconciliation error - will retry in {:?}",
        delay
    );
    Action::requeue(delay)
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;
