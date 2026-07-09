// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! CAA record management.

use super::super::types::RndcKeyData;
use super::{
    build_authenticated_client, build_delete_rrset_record, build_record_fqdn, effective_record_ttl,
    rrset_ttl_matches, should_update_record,
};
use anyhow::{Context, Result};
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;
use url::Url;

/// Compare existing DNS `RRset` with the desired CAA fields and TTL.
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `issuer_critical` - Desired issuer-critical flag from the spec
/// * `tag` - Desired CAA tag (`issue`, `issuewild`, or `iodef`) from the spec
/// * `value` - Desired CAA value from the spec
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if the existing `RRset` matches the desired state exactly (no changes
/// needed), `false` if an update is required (rdata or TTL differ).
fn compare_caa_rrset(
    existing_records: &[Record],
    issuer_critical: bool,
    tag: &str,
    value: &str,
    desired_ttl: u32,
) -> bool {
    if existing_records.len() != 1 {
        return false;
    }
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }
    let RData::CAA(existing_caa) = &existing_records[0].data else {
        return false;
    };

    let flags_match = existing_caa.issuer_critical == issuer_critical;
    let tag_match = existing_caa.tag == tag;

    let value_match = match tag {
        "issue" | "issuewild" => existing_caa
            .value_as_issue()
            .ok()
            .map(|(name, _opts)| name.map(|n| n.to_string()).unwrap_or_default())
            .is_some_and(|s| s == value),
        "iodef" => existing_caa
            .value_as_iodef()
            .ok()
            .is_some_and(|url| url.as_str() == value),
        _ => false,
    };

    flags_match && tag_match && value_match
}

/// Add a CAA record using dynamic DNS update (RFC 2136) with `RRset` synchronization.
///
/// If the existing `RRset` differs from the desired state, the entire CAA
/// `RRset` for the name is deleted and recreated so stale rdata never lingers.
///
/// # Errors
///
/// Returns an error if:
/// - DNS server connection fails
/// - TSIG signer creation fails
/// - DNS update is rejected by the server
/// - Invalid domain name, flags, tag, or value
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
    let issuer_critical = flags != 0;
    let ttl_value = effective_record_ttl(ttl);

    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::CAA,
        "CAA",
        server,
        |existing_records| {
            compare_caa_rrset(existing_records, issuer_critical, tag, value, ttl_value)
        },
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone =
        Name::from_str(zone_name).context(format!("Invalid zone name for CAA: {zone_name}"))?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    let record_data = match tag {
        "issue" => {
            let ca_name = if value.is_empty() {
                None
            } else {
                Some(Name::from_str(value).context(format!("Invalid CA domain name: {value}"))?)
            };
            rdata::CAA::new_issue(issuer_critical, ca_name, Vec::new())
        }
        "issuewild" => {
            let ca_name = if value.is_empty() {
                None
            } else {
                Some(Name::from_str(value).context(format!("Invalid CA domain name: {value}"))?)
            };
            rdata::CAA::new_issuewild(issuer_critical, ca_name, Vec::new())
        }
        "iodef" => {
            let url = Url::parse(value).context(format!("Invalid iodef URL: {value}"))?;
            rdata::CAA::new_iodef(issuer_critical, url)
        }
        _ => anyhow::bail!("Unsupported CAA tag: {tag}. Supported tags: issue, issuewild, iodef"),
    };

    let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::CAA(record_data));
    record.dns_class = DNSClass::IN;

    let mut client = build_authenticated_client(server, key_data).await?;

    // Step 1: delete existing RRset (ignore errors — may not exist).
    let delete_record = build_delete_rrset_record(&fqdn, RecordType::CAA);
    let _ = client.delete_rrset(delete_record, zone.clone()).await;

    // Step 2: append the desired record to create the new RRset.
    let response = client
        .append(record, zone, false)
        .await
        .context(format!("Failed to send CAA record update for {fqdn}"))?;

    match response.metadata.response_code {
        ResponseCode::NoError => {
            info!(
                "Successfully added CAA record: {} -> {} {} \"{}\" (TTL: {})",
                fqdn, flags, tag, value, ttl_value
            );
            Ok(())
        }
        code => {
            anyhow::bail!("DNS server rejected CAA record update for {fqdn}: {code:?}");
        }
    }
}

#[cfg(test)]
#[path = "caa_tests.rs"]
mod caa_tests;
