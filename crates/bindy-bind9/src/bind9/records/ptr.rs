// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! PTR record management.

use super::super::types::{PTRRecordData, RndcKeyData};
use super::{effective_record_ttl, write_rrset};
use anyhow::{Context, Result};
use hickory_proto::rr::{rdata, Name, RData, RecordType};
use std::str::FromStr;

/// Convert a `PTRRecord` spec's target to the PTR `RRset` value.
///
/// # Errors
///
/// Returns an error if the target is not a valid domain name.
pub(crate) fn ptr_rdata(ptr_data: &PTRRecordData) -> Result<Vec<RData>> {
    let target = Name::from_str(&ptr_data.target)
        .with_context(|| format!("Invalid target for PTR record: {}", ptr_data.target))?;
    Ok(vec![RData::PTR(rdata::PTR(target))])
}

/// Write the PTR `RRset` for a name using dynamic DNS update (RFC 2136).
///
/// The target is validated before any DNS traffic, compared with the live
/// `RRset` by value, and if it differs the `RRset` is replaced in one UPDATE
/// (ADR-0021), so stale rdata never lingers.
///
/// # Errors
///
/// Returns an error if:
/// - The target or a name is invalid (nothing is sent)
/// - DNS server connection fails
/// - TSIG signer creation fails
/// - DNS update is rejected by the server
pub async fn add_ptr_record(
    zone_name: &str,
    name: &str,
    ptr_data: &PTRRecordData,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = ptr_rdata(ptr_data)?;
    write_rrset(
        zone_name,
        name,
        RecordType::PTR,
        effective_record_ttl(ptr_data.ttl),
        desired,
        server,
        key_data,
    )
    .await
}

#[cfg(test)]
#[path = "ptr_tests.rs"]
mod ptr_tests;
