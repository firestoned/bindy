// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The `Bind9Instance` controller (ADR-0009, roadmap 01 Phase D).
//!
//! An instance owns its Deployment, Service, ConfigMap, Secrets and
//! ServiceAccount, inherits configuration from its `Bind9Cluster` or
//! `ClusterBind9Provider`, and lists the zones it serves in `status.zones`.
//! Every stream comes from the shared `WatchSet` in the [`Context`] (or, for
//! owned kinds that are never cached, from this controller's own watch), and
//! the controller drains on the context's shutdown signal. The one public
//! entry point is [`controller`].

use bindy_controller_sdk::context::Context;
use std::sync::Arc;

// The API and BIND9 modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::{constants, crd, labels, status_reasons};
pub(crate) use bindy_bind9::{bind9, bind9_resources, context, placement, safe_volume};

mod bind9instance;
mod watch;

/// Run the `Bind9Instance` controller, one per namespace target, until the
/// shutdown signal in `ctx` fires and every reconcile has drained.
///
/// # Errors
/// Returns an error if the controller fails.
pub async fn controller(ctx: Arc<Context>) -> anyhow::Result<()> {
    watch::run_bind9instance_controllers(ctx).await
}
