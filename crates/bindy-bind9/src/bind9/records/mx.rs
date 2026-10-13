// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! MX record management.

use super::super::types::RndcKeyData;
use super::{effective_record_ttl, write_rrset};
use anyhow::{Context, Result};
use hickory_proto::rr::{rdata, Name, RData, RecordType};
use std::str::FromStr;

/// Convert an `MXRecord` spec's priority and mail server to the MX `RRset`
/// value.
///
/// ADR-0021: a priority outside `0..=65535` is rejected (it used to be
/// silently replaced by a default preference), the same rule SRV's fields
/// follow.
///
/// # Errors
///
/// Returns an error if `priority` does not fit in a `u16` or `mail_server`
/// is not a valid domain name.
pub(crate) fn mx_rdata(priority: i32, mail_server: &str) -> Result<Vec<RData>> {
    let preference =
        u16::try_from(priority).with_context(|| format!("Invalid MX priority: {priority}"))?;
    let exchange = Name::from_str(mail_server)
        .with_context(|| format!("Invalid MX mail server: {mail_server}"))?;
    Ok(vec![RData::MX(rdata::MX::new(preference, exchange))])
}

/// Write the MX `RRset` for a name using dynamic DNS update (RFC 2136).
///
/// The spec is validated before any DNS traffic, compared with the live
/// `RRset` by value, and if it differs the `RRset` is replaced in one UPDATE
/// (ADR-0021), so an old mail server never lingers.
///
/// # Errors
///
/// Returns an error if the priority or mail server is invalid (nothing is
/// sent), the DNS update fails, or the server rejects it.
pub async fn add_mx_record(
    zone_name: &str,
    name: &str,
    priority: i32,
    mail_server: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = mx_rdata(priority, mail_server)?;
    write_rrset(
        zone_name,
        name,
        RecordType::MX,
        effective_record_ttl(ttl),
        desired,
        server,
        key_data,
    )
    .await
}

#[cfg(test)]
#[path = "mx_tests.rs"]
mod mx_tests;
