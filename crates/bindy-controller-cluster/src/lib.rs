// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The `Bind9Cluster` and `ClusterBind9Provider` controllers (ADR-0009,
//! roadmap 01 Phase D).
//!
//! A `ClusterBind9Provider` (cluster-scoped) owns one `Bind9Cluster` per
//! namespace it targets; a `Bind9Cluster` owns its `Bind9Instance`s. Both
//! controllers subscribe to the shared `WatchSet` in the [`Context`] and drain
//! on its shutdown signal. The one public entry point is [`controller`].

use bindy_controller_sdk::context::Context;
use std::sync::Arc;

// The API and BIND9 modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::{constants, crd, labels, status_reasons};
pub(crate) use bindy_bind9::{bind9_resources, context};

mod bind9cluster;
mod clusterbind9provider;
mod watch;

#[cfg(test)]
mod clusterbind9provider_tests;

/// Run the `ClusterBind9Provider` and `Bind9Cluster` controllers until the
/// shutdown signal in `ctx` fires and both have drained.
///
/// # Errors
/// Returns an error if either controller fails.
pub async fn controller(ctx: Arc<Context>) -> anyhow::Result<()> {
    futures::try_join!(
        watch::run_clusterbind9provider_controller(ctx.clone()),
        watch::run_bind9cluster_controllers(ctx),
    )?;
    Ok(())
}
