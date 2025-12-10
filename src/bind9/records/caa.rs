// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! CAA record management.

use super::super::types::RndcKeyData;
use super::should_update_record;
use anyhow::{Context, Result};
use hickory_client::client::{Client, SyncClient};
use hickory_client::op::ResponseCode;
use hickory_client::rr::{rdata, DNSClass, Name, RData, Record};
use hickory_client::udp::UdpClientConnection;
use std::str::FromStr;
use tracing::info;
use url::Url;

use crate::bind9::rndc::create_tsig_signer;
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
#[allow(clippy::too_many_lines)]
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
    use hickory_client::rr::RecordType;

    // Check if update is needed using declarative reconciliation pattern
    let flags_for_comparison = flags;
    let tag_for_comparison = tag.to_string();
    let value_for_comparison = value.to_string();
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::CAA,
        "CAA",
        server,
        |existing_records| {
            // Compare: should return true if records match desired state
            if existing_records.len() == 1 {
                if let Some(RData::CAA(existing_caa)) = existing_records[0].data() {
                    let issuer_critical = flags_for_comparison != 0;
                    let flags_match = existing_caa.issuer_critical() == issuer_critical;
                    let tag_match = existing_caa.tag().to_string() == tag_for_comparison;

                    // Compare value based on tag type
                    let value_match = match tag_for_comparison.as_str() {
                        "issue" | "issuewild" => {
                            // Compare CA name (may be None for empty/deny policy)
                            let existing_value = existing_caa.value().to_string();
                            existing_value == value_for_comparison
                        }
                        "iodef" => {
                            let existing_value = existing_caa.value().to_string();
                            existing_value == value_for_comparison
                        }
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

    let zone_name_str = zone_name.to_string();
    let name_str = name.to_string();
    let tag_str = tag.to_string();
    let value_str = value.to_string();
    let server_str = server.to_string();
    let key_data = key_data.clone();

    tokio::task::spawn_blocking(move || {
        let server_addr = server_str.parse::<std::net::SocketAddr>().context(format!(
            "Invalid server address for CAA record update: {server_str}"
        ))?;

        let conn = UdpClientConnection::new(server_addr)
            .context("Failed to create UDP connection for CAA record")?;

        let signer =
            create_tsig_signer(&key_data).context("Failed to create TSIG signer for CAA record")?;

        let client = SyncClient::with_tsigner(conn, signer);

        let fqdn_str = if name_str.is_empty() || name_str == "@" {
            zone_name_str.clone()
        } else {
            format!("{name_str}.{zone_name_str}")
        };

        let fqdn = Name::from_str(&fqdn_str)
            .context(format!("Invalid FQDN for CAA record: {fqdn_str}"))?;

        let zone = Name::from_str(&zone_name_str)
            .context(format!("Invalid zone name for CAA: {zone_name_str}"))?;

        let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
            .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

        // CAA flags: 0 = not critical, 128 = critical
        let issuer_critical = flags != 0;

        // Create CAA record based on tag type
        let record_data = match tag_str.as_str() {
            "issue" => {
                // Parse value as domain name
                let ca_name = if value_str.is_empty() {
                    None
                } else {
                    Some(
                        Name::from_str(&value_str)
                            .context(format!("Invalid CA domain name: {value_str}"))?,
                    )
                };
                rdata::CAA::new_issue(issuer_critical, ca_name, Vec::new())
            }
            "issuewild" => {
                let ca_name = if value_str.is_empty() {
                    None
                } else {
                    Some(
                        Name::from_str(&value_str)
                            .context(format!("Invalid CA domain name: {value_str}"))?,
                    )
                };
                rdata::CAA::new_issuewild(issuer_critical, ca_name, Vec::new())
            }
            "iodef" => {
                let url =
                    Url::parse(&value_str).context(format!("Invalid iodef URL: {value_str}"))?;
                rdata::CAA::new_iodef(issuer_critical, url)
            }
            _ => anyhow::bail!(
                "Unsupported CAA tag: {tag_str}. Supported tags: issue, issuewild, iodef"
            ),
        };

        let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::CAA(record_data));
        record.set_dns_class(DNSClass::IN);

        // Use append for idempotent operation
        let response = client
            .append(record, zone.clone(), false)
            .context(format!("Failed to send CAA record update for {fqdn_str}"))?;

        match response.response_code() {
            ResponseCode::NoError => {
                info!(
                    "Successfully added CAA record: {} -> {} {} \"{}\" (TTL: {})",
                    fqdn_str, flags, tag_str, value_str, ttl_value
                );
            }
            code => {
                anyhow::bail!("DNS server rejected CAA record update for {fqdn_str}: {code:?}");
            }
        }

        Ok(())
    })
    .await
    .context("CAA record update task panicked")??;

    Ok(())
}

#[cfg(test)]
#[path = "caa_tests.rs"]
mod caa_tests;
