// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! CAA record management.

use super::super::types::RndcKeyData;
use super::{effective_record_ttl, write_rrset, CAA_TAG_IODEF, CAA_TAG_ISSUE, CAA_TAG_ISSUEWILD};
use anyhow::{Context, Result};
use hickory_proto::rr::{rdata, Name, RData, RecordType};
use std::str::FromStr;
use url::Url;

/// Parse the issuer domain of an `issue` / `issuewild` value; an empty value
/// means "no CA may issue" (RFC 8659 4.2).
fn parse_issuer(value: &str) -> Result<Option<Name>> {
    if value.is_empty() {
        return Ok(None);
    }
    Name::from_str(value)
        .map(Some)
        .with_context(|| format!("Invalid CA domain name: {value}"))
}

/// Convert a `CAARecord` spec to the CAA `RRset` value.
///
/// Any non-zero `flags` sets the issuer-critical bit, as before.
///
/// # Errors
///
/// Returns an error if the tag is not `issue`, `issuewild` or `iodef`, the
/// issuer domain is invalid, or the `iodef` URL is invalid.
pub(crate) fn caa_rdata(flags: i32, tag: &str, value: &str) -> Result<Vec<RData>> {
    let issuer_critical = flags != 0;
    let caa = match tag {
        CAA_TAG_ISSUE => rdata::CAA::new_issue(issuer_critical, parse_issuer(value)?, Vec::new()),
        CAA_TAG_ISSUEWILD => {
            rdata::CAA::new_issuewild(issuer_critical, parse_issuer(value)?, Vec::new())
        }
        CAA_TAG_IODEF => {
            let url = Url::parse(value).with_context(|| format!("Invalid iodef URL: {value}"))?;
            rdata::CAA::new_iodef(issuer_critical, url)
        }
        _ => anyhow::bail!(
            "Unsupported CAA tag: {tag}. Supported tags: {CAA_TAG_ISSUE}, \
             {CAA_TAG_ISSUEWILD}, {CAA_TAG_IODEF}"
        ),
    };
    Ok(vec![RData::CAA(caa)])
}

/// Write the CAA `RRset` for a name using dynamic DNS update (RFC 2136).
///
/// The spec is validated before any DNS traffic, compared with the live
/// `RRset` by value, and if it differs the `RRset` is replaced in one UPDATE
/// (ADR-0021), so stale rdata never lingers.
///
/// # Errors
///
/// Returns an error if:
/// - The tag, issuer domain or `iodef` URL is invalid (nothing is sent)
/// - DNS server connection fails
/// - TSIG signer creation fails
/// - DNS update is rejected by the server
#[allow(clippy::too_many_arguments)]
pub async fn add_caa_record(
    zone_name: &str,
    name: &str,
    flags: i32,
    tag: &str,
    value: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = caa_rdata(flags, tag, value)?;
    write_rrset(
        zone_name,
        name,
        RecordType::CAA,
        effective_record_ttl(ttl),
        desired,
        server,
        key_data,
    )
    .await
}

#[cfg(test)]
#[path = "caa_tests.rs"]
mod caa_tests;
