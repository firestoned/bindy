// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! TXT record management.

use super::super::types::RndcKeyData;
use super::{effective_record_ttl, write_rrset};
use anyhow::{anyhow, Result};
use hickory_proto::rr::{rdata, RData, RecordType};

/// The longest character-string a TXT record may carry (RFC 1035 3.3).
const MAX_TXT_STRING_BYTES: usize = 255;

/// Convert a `TXTRecord` spec's strings to the TXT `RRset` value: ONE TXT
/// record carrying every string, in order (as the writer always did).
///
/// # Errors
///
/// Returns an error if a string is longer than 255 bytes, which the wire
/// format cannot encode.
pub(crate) fn txt_rdata(texts: &[String]) -> Result<Vec<RData>> {
    if let Some(long) = texts.iter().find(|t| t.len() > MAX_TXT_STRING_BYTES) {
        return Err(anyhow!(
            "TXT string of {} bytes exceeds the {MAX_TXT_STRING_BYTES}-byte limit",
            long.len()
        ));
    }
    Ok(vec![RData::TXT(rdata::TXT::new(texts.to_vec()))])
}

/// Write the TXT `RRset` for a name using dynamic DNS update (RFC 2136).
///
/// The strings are validated before any DNS traffic, compared with the live
/// `RRset` by value, and if they differ the `RRset` is replaced in one
/// UPDATE (ADR-0021), so stale rdata never lingers.
///
/// # Errors
///
/// Returns an error if a string is too long (nothing is sent), the DNS
/// update fails, or the server rejects it.
pub async fn add_txt_record(
    zone_name: &str,
    name: &str,
    texts: &[String],
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = txt_rdata(texts)?;
    write_rrset(
        zone_name,
        name,
        RecordType::TXT,
        effective_record_ttl(ttl),
        desired,
        server,
        key_data,
    )
    .await
}

#[cfg(test)]
#[path = "txt_tests.rs"]
mod txt_tests;
