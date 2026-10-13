// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The DNS record controllers (ADR-0009, roadmap 01 Phase D).
//!
//! One generic controller (`record_operator`) runs for each record kind in
//! the record-kind table (ADR-0020): A, AAAA, TXT, CNAME, MX, NS, SRV, CAA,
//! PTR. A record is published to the primaries of the zones that select it,
//! through the BIND9 write path in `bindy_bind9::record_push`. Every stream
//! comes from the shared `WatchSet` in the [`Context`], and every controller
//! drains on the context's shutdown signal. The one public entry point is
//! [`controller`].

use bindy_controller_sdk::context::Context;
use futures::future::{BoxFuture, FutureExt};
use std::sync::Arc;

use crate::record_operator::run_generic_record_operator;

// The API and BIND9 modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::crd;
pub(crate) use bindy_bind9::{context, ddns};
pub(crate) use bindy_controller_sdk::metrics;

mod record_operator;
mod record_wrappers;
mod records;

#[cfg(test)]
mod record_operator_tests;
#[cfg(test)]
mod record_wrappers_tests;
#[cfg(test)]
mod records_tests;

/// Run the controller of every record kind until the shutdown signal in
/// `ctx` fires and every reconcile has drained.
///
/// # Errors
/// Returns an error if any record controller fails.
pub async fn controller(ctx: Arc<Context>) -> anyhow::Result<()> {
    let mut controllers: Vec<BoxFuture<'static, anyhow::Result<()>>> = Vec::new();
    bindy_api::each_record_kind!(K => controllers.push(run_generic_record_operator::<K>(ctx.clone()).boxed()));
    futures::future::try_join_all(controllers).await?;
    Ok(())
}
