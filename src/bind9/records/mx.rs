// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! MX record management.

use super::super::types::RndcKeyData;
use super::should_update_record;
use anyhow::Result;
use hickory_client::client::{Client, SyncClient};
use hickory_client::op::ResponseCode;
use hickory_client::rr::{rdata, DNSClass, Name, RData, Record};
use hickory_client::udp::UdpClientConnection;
use std::str::FromStr;
use tracing::info;

use crate::bind9::rndc::create_tsig_signer;
use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Add an MX record using dynamic DNS update (RFC 2136).
///
/// # Errors
///
/// Returns an error if the DNS update fails or the server rejects it.
#[allow(clippy::too_many_arguments)]
pub async fn add_mx_record(
    zone_name: &str,
    name: &str,
    priority: i32,
    mail_server: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    use hickory_client::rr::RecordType;

    // Check if update is needed using declarative reconciliation pattern
    let mail_server_for_comparison = mail_server.to_string();
    let priority_u16 = u16::try_from(priority).unwrap_or(10);
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::MX,
        "MX",
        server,
        |existing_records| {
            // Compare: should return true if records match desired state
            if existing_records.len() == 1 {
                if let Some(RData::MX(existing_mx)) = existing_records[0].data() {
                    return existing_mx.preference() == priority_u16
                        && existing_mx.exchange().to_string() == mail_server_for_comparison;
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
    let mail_server_str = mail_server.to_string();
    let server_str = server.to_string();
    let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
    let key_data = key_data.clone();

    tokio::task::spawn_blocking(move || {
        let server_addr = server_str.parse::<std::net::SocketAddr>()?;
        let conn = UdpClientConnection::new(server_addr)?;
        let signer = create_tsig_signer(&key_data)?;
        let client = SyncClient::with_tsigner(conn, signer);

        let zone = Name::from_str(&zone_name_str)?;
        let fqdn = if name_str == "@" || name_str.is_empty() {
            zone.clone()
        } else {
            Name::from_str(&format!("{name_str}.{zone_name_str}"))?
        };

        let mx_name = Name::from_str(&mail_server_str)?;
        let mx_rdata = rdata::MX::new(priority_u16, mx_name);
        let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::MX(mx_rdata));
        record.set_dns_class(DNSClass::IN);

        // Use append for idempotent operation (must_exist=false for no prerequisites)
        info!(
            "Adding MX record: {} -> {} (priority: {}, TTL: {})",
            fqdn, mail_server_str, priority_u16, ttl_value
        );
        let response = client.append(record, zone, false)?;

        match response.response_code() {
            ResponseCode::NoError => {
                info!(
                    "Successfully added MX record: {} -> {}",
                    name_str, mail_server_str
                );
                Ok(())
            }
            code => Err(anyhow::anyhow!(
                "DNS update failed with response code: {code:?}"
            )),
        }
    })
    .await?
}

#[cfg(test)]
#[path = "mx_tests.rs"]
mod mx_tests;
