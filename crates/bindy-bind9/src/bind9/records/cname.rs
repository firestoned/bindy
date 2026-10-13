// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! CNAME record management.

use super::super::types::RndcKeyData;
use super::{effective_record_ttl, write_rrset};
use anyhow::{Context, Result};
use hickory_proto::rr::{rdata, Name, RData, RecordType};
use std::str::FromStr;

/// Convert a `CNAMERecord` spec's target to the CNAME `RRset` value.
///
/// # Errors
///
/// Returns an error if `target` is not a valid domain name.
pub(crate) fn cname_rdata(target: &str) -> Result<Vec<RData>> {
    let target_name =
        Name::from_str(target).with_context(|| format!("Invalid CNAME target: {target}"))?;
    Ok(vec![RData::CNAME(rdata::CNAME(target_name))])
}

/// Write the CNAME for a name using dynamic DNS update (RFC 2136).
///
/// The target is validated before any DNS traffic and compared with the live
/// value case-insensitively and with or without a trailing dot. If it
/// differs, the CNAME `RRset` is replaced in one UPDATE (delete then add,
/// ADR-0021). Before ADR-0021 a CNAME was appended without a delete; a
/// CNAME `RRset` holds one value, so the replacement is what an append
/// meant, and a changed target can no longer be refused next to the old one.
///
/// # Errors
///
/// Returns an error if the target is invalid (nothing is sent), the DNS
/// update fails, or the server rejects it.
pub async fn add_cname_record(
    zone_name: &str,
    name: &str,
    target: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = cname_rdata(target)?;
    write_rrset(
        zone_name,
        name,
        RecordType::CNAME,
        effective_record_ttl(ttl),
        desired,
        server,
        key_data,
    )
    .await
}

#[cfg(test)]
#[path = "cname_tests.rs"]
mod cname_tests;
