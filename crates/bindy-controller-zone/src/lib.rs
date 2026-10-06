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
//!
//! The crate also runs the zones-loaded readiness gate (ADR-0017): a Pod
//! controller that loads every live zone onto a new BIND9 pod, with the same
//! write paths, before Kubernetes lets the pod into its Service.

use bindy_controller_sdk::context::Context;
use std::sync::Arc;

// The API and BIND9 modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::{constants, crd, labels};
pub(crate) use bindy_bind9::{bind9, bind9_resources, context};

mod dnszone;
mod watch;
mod zones_gate;

/// Run the `DNSZone` controller and the zones-loaded readiness gate
/// controller (ADR-0017), one each per namespace target, until the shutdown
/// signal in `ctx` fires and every reconcile has drained.
///
/// # Errors
/// Returns an error if either controller fails.
pub async fn controller(ctx: Arc<Context>) -> anyhow::Result<()> {
    let (zones, gate) = futures::join!(
        watch::run_dnszone_controllers(ctx.clone()),
        zones_gate::run_zones_gate_controllers(ctx)
    );
    zones?;
    gate
}
