// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Draining shutdown for the controllers (ADR-0009 §5, amended 2026-10-05).
//!
//! One [`ShutdownTrigger`] is fired by SIGTERM, SIGINT or the loss of the
//! leader lease. Every controller holds a clone of the matching
//! [`ShutdownSignal`] (through the shared `Context`) and passes
//! [`ShutdownSignal::wait`] to kube's `Controller::graceful_shutdown_on`, so a
//! shutdown stops new reconciles, lets the running ones finish, and only then
//! ends the controller.
//!
//! [`supervise`] wraps each controller's future so the binary can join them
//! with `try_join_all`: a controller that returns after the trigger fired has
//! drained, one that returns before it has failed.

use anyhow::{anyhow, Result};
use futures::future::{BoxFuture, FutureExt, Shared};
use std::future::Future;
use tokio::sync::watch;
use tracing::{error, info};

/// Fires the shutdown. Cloneable; firing more than once is harmless.
#[derive(Clone, Debug)]
pub struct ShutdownTrigger {
    tx: std::sync::Arc<watch::Sender<bool>>,
}

/// Resolves once the shutdown fires. Cheap to clone.
///
/// Dropping every [`ShutdownTrigger`] also resolves it: with no trigger left,
/// nothing can keep the controllers running.
#[derive(Clone)]
pub struct ShutdownSignal {
    rx: watch::Receiver<bool>,
    fired: Shared<BoxFuture<'static, ()>>,
}

impl std::fmt::Debug for ShutdownSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShutdownSignal")
            .field("triggered", &self.is_triggered())
            .finish()
    }
}

/// Create a connected trigger and signal.
#[must_use]
pub fn channel() -> (ShutdownTrigger, ShutdownSignal) {
    let (tx, rx) = watch::channel(false);
    let mut waiter = rx.clone();
    let fired = async move {
        // An error means every sender is gone, which also ends the wait.
        let _ = waiter.wait_for(|triggered| *triggered).await;
    }
    .boxed()
    .shared();
    (
        ShutdownTrigger {
            tx: std::sync::Arc::new(tx),
        },
        ShutdownSignal { rx, fired },
    )
}

impl ShutdownTrigger {
    /// Start the shutdown. Every [`ShutdownSignal`] resolves.
    pub fn fire(&self) {
        self.tx.send_replace(true);
    }
}

impl ShutdownSignal {
    /// Whether the shutdown has fired.
    #[must_use]
    pub fn is_triggered(&self) -> bool {
        *self.rx.borrow() || self.rx.has_changed().is_err()
    }

    /// A future that resolves once the shutdown fires. It is
    /// `Send + Sync + 'static`, as `Controller::graceful_shutdown_on` requires.
    pub fn wait(&self) -> Shared<BoxFuture<'static, ()>> {
        self.fired.clone()
    }
}

/// Run one controller and judge how it ended.
///
/// # Arguments
/// * `name` - The controller's name, for logs and errors
/// * `controller` - The controller's future (its crate's `controller(ctx)`)
/// * `shutdown` - The shared shutdown signal
///
/// # Errors
/// Returns the controller's own error, with its name attached, or an error if
/// it returned before the shutdown fired: controllers run until they are
/// told to stop, so an early return is a failure and the process should exit
/// non-zero for Kubernetes to restart it.
pub async fn supervise<F>(name: &'static str, controller: F, shutdown: ShutdownSignal) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    match controller.await {
        Err(e) => {
            error!("CRITICAL: {name} controller failed: {e:#}");
            Err(e.context(format!("{name} controller failed")))
        }
        Ok(()) if shutdown.is_triggered() => {
            info!("{name} controller drained");
            Ok(())
        }
        Ok(()) => {
            error!("CRITICAL: {name} controller exited unexpectedly");
            Err(anyhow!("{name} controller exited unexpectedly"))
        }
    }
}

#[cfg(test)]
#[path = "shutdown_tests.rs"]
mod shutdown_tests;
