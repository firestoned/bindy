// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The DNS record controllers (ADR-0009, roadmap 01 Phase D).
//!
//! One generic controller (`record_operator`) runs for each of the nine
//! record kinds (A, AAAA, CNAME, MX, NS, PTR, SRV, TXT, CAA). A record is
//! published to the primaries of the zones that select it, through the BIND9
//! write path in `bindy_bind9::record_push`. Every stream comes from the
//! shared `WatchSet` in the [`Context`], and every controller drains on the
//! context's shutdown signal. The one public entry point is [`controller`].

use bindy_controller_sdk::context::Context;
use std::sync::Arc;

use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, PTRRecord, SRVRecord,
    TXTRecord,
};
use crate::record_operator::run_generic_record_operator;

// The API and BIND9 modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::{crd, labels};
pub(crate) use bindy_bind9::{context, ddns};
pub(crate) use bindy_controller_sdk::metrics;

mod record_impls;
mod record_operator;
mod record_wrappers;
mod records;

#[cfg(test)]
mod record_impls_tests;
#[cfg(test)]
mod record_operator_tests;
#[cfg(test)]
mod record_wrappers_tests;
#[cfg(test)]
mod records_tests;

/// Run the controllers for all nine record kinds until the shutdown signal in
/// `ctx` fires and every reconcile has drained.
///
/// # Errors
/// Returns an error if any record controller fails.
pub async fn controller(ctx: Arc<Context>) -> anyhow::Result<()> {
    futures::try_join!(
        run_generic_record_operator::<ARecord>(ctx.clone()),
        run_generic_record_operator::<AAAARecord>(ctx.clone()),
        run_generic_record_operator::<TXTRecord>(ctx.clone()),
        run_generic_record_operator::<CNAMERecord>(ctx.clone()),
        run_generic_record_operator::<MXRecord>(ctx.clone()),
        run_generic_record_operator::<NSRecord>(ctx.clone()),
        run_generic_record_operator::<SRVRecord>(ctx.clone()),
        run_generic_record_operator::<CAARecord>(ctx.clone()),
        run_generic_record_operator::<PTRRecord>(ctx),
    )?;
    Ok(())
}
