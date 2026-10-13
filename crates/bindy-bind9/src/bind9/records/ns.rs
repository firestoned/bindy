// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! NS record management.

use super::super::types::RndcKeyData;
use super::{effective_record_ttl, write_rrset};
use anyhow::{Context, Result};
use hickory_proto::rr::{rdata, Name, RData, RecordType};
use std::str::FromStr;

/// Convert an `NSRecord` spec's nameserver to the NS `RRset` value.
///
/// # Errors
///
/// Returns an error if `nameserver` is not a valid domain name.
pub(crate) fn ns_rdata(nameserver: &str) -> Result<Vec<RData>> {
    let ns_name = Name::from_str(nameserver)
        .with_context(|| format!("Invalid NS nameserver: {nameserver}"))?;
    Ok(vec![RData::NS(rdata::NS(ns_name))])
}

/// Write the NS `RRset` for a name using dynamic DNS update (RFC 2136).
///
/// NS records managed here are used for delegations. The nameserver is
/// validated before any DNS traffic, compared with the live `RRset` by value,
/// and if it differs the `RRset` is replaced in one UPDATE (ADR-0021), so
/// stale delegation rdata never lingers.
///
/// # Errors
///
/// Returns an error if the nameserver is invalid (nothing is sent), the DNS
/// update fails, or the server rejects it.
pub async fn add_ns_record(
    zone_name: &str,
    name: &str,
    nameserver: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = ns_rdata(nameserver)?;
    write_rrset(
        zone_name,
        name,
        RecordType::NS,
        effective_record_ttl(ttl),
        desired,
        server,
        key_data,
    )
    .await
}

#[cfg(test)]
#[path = "ns_tests.rs"]
mod ns_tests;
