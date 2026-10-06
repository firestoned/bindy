// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The bookkeeping every reconcile wrapper repeated (roadmap 01 Phase D).
//!
//! [`instrumented`] times a reconcile, records the success or failure
//! metrics, and turns success into `await_change`: there is no periodic
//! resync, the next reconcile comes from a watch event (ADR-0016).
//! [`instrumented_scheduled`] does the same for a reconcile that knows of a
//! future instant it must act at (an RNDC key rotation). [`finalizer_error`]
//! flattens kube's `finalizer::Error` into a [`ReconcileError`] for the
//! controllers that use kube's `finalizer()` helper.

use crate::error::ReconcileError;
use crate::metrics::{record_error, record_reconciliation_error, record_reconciliation_success};
use anyhow::anyhow;
use kube::runtime::controller::Action;
use kube::runtime::finalizer;
use std::future::Future;
use std::time::{Duration, Instant};
use tracing::{debug, error, info};

/// The `error_type` label recorded when a reconcile fails.
const ERROR_TYPE_RECONCILE: &str = "reconcile_error";

/// Seconds in one day, for [`MAX_SCHEDULED_WAKE`].
const SECONDS_PER_DAY: u64 = 86_400;

/// Days a scheduled wake may lie ahead before it is capped.
const MAX_SCHEDULED_WAKE_DAYS: u64 = 30;

/// Longest delay a scheduled wake may ask for.
///
/// kube-runtime keeps scheduled reconciles in a tokio `DelayQueue`, which
/// panics on a deadline more than about two years out. A KSK rollover a year
/// away is legitimate, so a wake further than this is capped: the object is
/// reconciled at the cap and schedules the remainder again. Not a resync, it
/// fires once a month at most and only for an object with a pending instant.
pub const MAX_SCHEDULED_WAKE: Duration =
    Duration::from_secs(MAX_SCHEDULED_WAKE_DAYS * SECONDS_PER_DAY);

/// An `Action` that wakes the object after `delay`, capped at
/// [`MAX_SCHEDULED_WAKE`].
///
/// For an instant the Kubernetes API cannot announce (an RNDC key falling
/// due, a DNSSEC KSK rollover); never for a fixed-interval resync (ADR-0016).
///
/// # Arguments
/// * `delay` - How long until the object must be reconciled
#[must_use]
pub fn scheduled_action(delay: Duration) -> Action {
    Action::requeue(delay.min(MAX_SCHEDULED_WAKE))
}

/// Run one reconcile and record how it went.
///
/// On success: logs, counts a success for `kind`, and returns
/// `Action::await_change()`. There is no periodic resync: changes to watched
/// objects trigger the next reconcile (ADR-0016). On failure: logs, counts an
/// error, and returns it for the controller's `error_policy`, which retries
/// with per-object backoff.
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
    instrumented_scheduled(kind, name, async { reconcile.await.map(|()| None) }).await
}

/// [`instrumented`] for a reconcile that may know of a future instant it must
/// act at.
///
/// On success the reconcile's `Some(delay)` becomes [`scheduled_action`];
/// `None` becomes `Action::await_change()`. Failure is handled as in
/// [`instrumented`].
///
/// # Arguments
/// * `kind` - The resource kind, used as the metrics label
/// * `name` - The object's name, for the log line
/// * `reconcile` - The reconcile, returning the delay to its next scheduled
///   instant, if any
///
/// # Errors
/// Returns the reconcile's error as a [`ReconcileError`].
pub async fn instrumented_scheduled<F>(
    kind: &'static str,
    name: &str,
    reconcile: F,
) -> Result<Action, ReconcileError>
where
    F: Future<Output = anyhow::Result<Option<Duration>>>,
{
    let start = Instant::now();
    let result = reconcile.await;
    let duration = start.elapsed();

    match result {
        Ok(next_wake) => {
            info!("Successfully reconciled {kind}: {name}");
            record_reconciliation_success(kind, duration);
            let Some(delay) = next_wake else {
                debug!("{kind} {name} reconciled, awaiting the next change");
                return Ok(Action::await_change());
            };
            debug!("{kind} {name} reconciled, next scheduled wake in {delay:?}");
            Ok(scheduled_action(delay))
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
