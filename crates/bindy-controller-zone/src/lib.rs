// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The `DNSZone` controller (ADR-0009, roadmap 01 Phase D).
//!
//! A zone selects the `Bind9Instance`s that serve it and the records it
//! holds, creates itself on the primaries (secondaries transfer it), and
//! replays its records when a BIND9 pod comes back empty. Every stream comes
//! from the shared `WatchSet` in the [`Context`]; the primary stream drops the
//! controller's own status writes (ADR-0009 §4), and the controller drains on
//! the context's shutdown signal. The one public entry point is
//! [`controller`].

use bindy_controller_sdk::context::Context;
use std::sync::Arc;

// The API and BIND9 modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::{constants, crd, labels};
pub(crate) use bindy_bind9::{bind9, bind9_resources, context};

mod dnszone;
mod watch;

/// Run the `DNSZone` controller, one per namespace target, until the shutdown
/// signal in `ctx` fires and every reconcile has drained.
///
/// # Errors
/// Returns an error if the controller fails.
pub async fn controller(ctx: Arc<Context>) -> anyhow::Result<()> {
    watch::run_dnszone_controllers(ctx).await
}
