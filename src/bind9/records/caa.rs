// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! CAA record management.

use super::super::types::RndcKeyData;
use super::{build_authenticated_client, build_record_fqdn, should_update_record};
use anyhow::{Context, Result};
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;
use url::Url;

use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Add a CAA record using dynamic DNS update (RFC 2136).
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
    let tag_for_comparison = tag.to_string();
    let value_for_comparison = value.to_string();

    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::CAA,
        "CAA",
        server,
        |existing_records| {
            if existing_records.len() == 1 {
                if let RData::CAA(existing_caa) = &existing_records[0].data {
                    let flags_match = existing_caa.issuer_critical == issuer_critical;
                    let tag_match = existing_caa.tag == tag_for_comparison;

                    let value_match = match tag_for_comparison.as_str() {
                        "issue" | "issuewild" => existing_caa
                            .value_as_issue()
                            .ok()
                            .map(|(name, _opts)| name.map(|n| n.to_string()).unwrap_or_default())
                            .is_some_and(|s| s == value_for_comparison),
                        "iodef" => existing_caa
                            .value_as_iodef()
                            .ok()
                            .is_some_and(|url| url.as_str() == value_for_comparison),
                        _ => false,
                    };

                    return flags_match && tag_match && value_match;
                }
            }
            false
        },
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

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
