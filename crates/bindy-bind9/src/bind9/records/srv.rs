// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! SRV record management.

use super::super::types::{RndcKeyData, SRVRecordData};
use super::{effective_record_ttl, write_rrset};
use anyhow::{Context, Result};
use hickory_proto::rr::{rdata, Name, RData, RecordType};
use std::str::FromStr;

/// Convert an `SRVRecord` spec to the SRV `RRset` value.
///
/// # Errors
///
/// Returns an error if the priority, weight or port does not fit in a `u16`,
/// or the target is not a valid domain name.
pub(crate) fn srv_rdata(srv_data: &SRVRecordData) -> Result<Vec<RData>> {
    let priority = u16::try_from(srv_data.priority)
        .with_context(|| format!("Invalid SRV priority: {}", srv_data.priority))?;
    let weight = u16::try_from(srv_data.weight)
        .with_context(|| format!("Invalid SRV weight: {}", srv_data.weight))?;
    let port = u16::try_from(srv_data.port)
        .with_context(|| format!("Invalid SRV port: {}", srv_data.port))?;
    let target = Name::from_str(&srv_data.target)
        .with_context(|| format!("Invalid target for SRV record: {}", srv_data.target))?;
    Ok(vec![RData::SRV(rdata::SRV::new(
        priority, weight, port, target,
    ))])
}

/// Write the SRV `RRset` for a name using dynamic DNS update (RFC 2136).
///
/// The spec is validated before any DNS traffic, compared with the live
/// `RRset` by value, and if it differs the `RRset` is replaced in one UPDATE
/// (ADR-0021), so stale rdata never lingers.
///
/// # Errors
///
/// Returns an error if:
/// - A field is out of range or the target is invalid (nothing is sent)
/// - DNS server connection fails
/// - TSIG signer creation fails
/// - DNS update is rejected by the server
pub async fn add_srv_record(
    zone_name: &str,
    name: &str,
    srv_data: &SRVRecordData,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = srv_rdata(srv_data)?;
    write_rrset(
        zone_name,
        name,
        RecordType::SRV,
        effective_record_ttl(srv_data.ttl),
        desired,
        server,
        key_data,
    )
    .await
}

#[cfg(test)]
#[path = "srv_tests.rs"]
mod srv_tests;
