// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The bookkeeping every reconcile wrapper repeated (roadmap 01 Phase D).
//!
//! [`instrumented`] times a reconcile, records the success or failure
//! metrics, and turns success into the ready requeue. [`finalizer_error`]
//! flattens kube's `finalizer::Error` into a [`ReconcileError`] for the
//! controllers that use kube's `finalizer()` helper.

use crate::error::ReconcileError;
use crate::metrics::{record_error, record_reconciliation_error, record_reconciliation_success};
use crate::requeue::REQUEUE_WHEN_READY_SECS;
use anyhow::anyhow;
use kube::runtime::controller::Action;
use kube::runtime::finalizer;
use std::future::Future;
use std::time::{Duration, Instant};
use tracing::{debug, error, info};

/// The `error_type` label recorded when a reconcile fails.
const ERROR_TYPE_RECONCILE: &str = "reconcile_error";

/// Run one reconcile and record how it went.
///
/// On success: logs, counts a success for `kind`, and requeues after
/// [`REQUEUE_WHEN_READY_SECS`]. Changes to watched objects trigger a
/// reconcile immediately, so the requeue is only a backstop. On failure:
/// logs, counts an error, and returns it for the controller's `error_policy`.
///
/// # Arguments
/// * `kind` - The resource kind, used as the metrics label
/// * `name` - The object's name, for the log line
/// * `reconcile` - The reconcile itself
///
/// # Errors
/// Returns the reconcile's error as a [`ReconcileError`].
pub async fn instrumented<F>(
    kind: &'static str,
    name: &str,
    reconcile: F,
) -> Result<Action, ReconcileError>
where
    F: Future<Output = anyhow::Result<()>>,
{
    let start = Instant::now();
    let result = reconcile.await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled {kind}: {name}");
            record_reconciliation_success(kind, duration);
            debug!("{kind} {name} reconciled, requeueing in {REQUEUE_WHEN_READY_SECS}s");
            Ok(Action::requeue(Duration::from_secs(
                REQUEUE_WHEN_READY_SECS,
            )))
        }
        Err(e) => {
            error!("Failed to reconcile {kind} {name}: {e}");
            record_reconciliation_error(kind, duration);
            record_error(kind, ERROR_TYPE_RECONCILE);
            Err(e.into())
        }
    }
}

/// Flatten a `finalizer::Error` into a [`ReconcileError`].
///
/// An apply or cleanup failure is the reconcile's own error and passes
/// through unchanged; the finalizer bookkeeping failures are wrapped with the
/// kind so the log says which controller hit them.
///
/// # Arguments
/// * `kind` - The resource kind, for the message
/// * `err` - The error from kube's `finalizer()` helper
#[must_use]
pub fn finalizer_error(kind: &str, err: finalizer::Error<ReconcileError>) -> ReconcileError {
    match err {
        finalizer::Error::ApplyFailed(e) | finalizer::Error::CleanupFailed(e) => e,
        finalizer::Error::AddFinalizer(e) | finalizer::Error::RemoveFinalizer(e) => {
            ReconcileError::from(anyhow!("Finalizer error on {kind}: {e}"))
        }
        finalizer::Error::UnnamedObject => ReconcileError::from(anyhow!("{kind} has no name")),
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow!("Invalid finalizer for {kind}"))
        }
    }
}

#[cfg(test)]
#[path = "reconcile_tests.rs"]
mod reconcile_tests;
