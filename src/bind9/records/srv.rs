// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! SRV record management.

use super::super::types::{RndcKeyData, SRVRecordData};
use super::should_update_record;
use anyhow::{Context, Result};
use hickory_client::client::{Client, SyncClient};
use hickory_client::op::ResponseCode;
use hickory_client::rr::{rdata, DNSClass, Name, RData, Record};
use hickory_client::udp::UdpClientConnection;
use std::str::FromStr;
use tracing::info;

use crate::bind9::rndc::create_tsig_signer;
use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Add an SRV record using dynamic DNS update (RFC 2136).
///
/// # Errors
///
/// Returns an error if:
/// - DNS server connection fails
/// - TSIG signer creation fails
/// - DNS update is rejected by the server
/// - Invalid domain name or target
#[allow(clippy::too_many_arguments)]
pub async fn add_srv_record(
    zone_name: &str,
    name: &str,
    srv_data: &SRVRecordData,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    use hickory_client::rr::RecordType;

    // Check if update is needed using declarative reconciliation pattern
    let srv_data_for_comparison = srv_data.clone();
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::SRV,
        "SRV",
        server,
        |existing_records| {
            // Compare: should return true if records match desired state
            if existing_records.len() == 1 {
                if let Some(RData::SRV(existing_srv)) = existing_records[0].data() {
                    let priority_match = existing_srv.priority()
                        == u16::try_from(srv_data_for_comparison.priority).unwrap_or(0);
                    let weight_match = existing_srv.weight()
                        == u16::try_from(srv_data_for_comparison.weight).unwrap_or(0);
                    let port_match = existing_srv.port()
                        == u16::try_from(srv_data_for_comparison.port).unwrap_or(0);
                    let target_match =
                        existing_srv.target().to_string() == srv_data_for_comparison.target;
                    return priority_match && weight_match && port_match && target_match;
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
    let target_str = srv_data.target.clone();
    let port = srv_data.port;
    let priority = srv_data.priority;
    let weight = srv_data.weight;
    let ttl = srv_data.ttl;
    let server_str = server.to_string();
    let key_data = key_data.clone();

    tokio::task::spawn_blocking(move || {
        let server_addr = server_str.parse::<std::net::SocketAddr>().context(format!(
            "Invalid server address for SRV record update: {server_str}"
        ))?;

        let conn = UdpClientConnection::new(server_addr)
            .context("Failed to create UDP connection for SRV record")?;

        let signer = create_tsig_signer(&key_data)
            .context("Failed to create TSIG signer for SRV record")?;

        let client = SyncClient::with_tsigner(conn, signer);

        let fqdn_str = if name_str.is_empty() || name_str == "@" {
            zone_name_str.clone()
        } else {
            format!("{name_str}.{zone_name_str}")
        };

        let fqdn = Name::from_str(&fqdn_str)
            .context(format!("Invalid FQDN for SRV record: {fqdn_str}"))?;

        let zone = Name::from_str(&zone_name_str)
            .context(format!("Invalid zone name for SRV: {zone_name_str}"))?;

        let target_name = Name::from_str(&target_str)
            .context(format!("Invalid target for SRV record: {target_str}"))?;

        let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
            .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

        // Convert i32 to u16 for SRV record parameters
        let priority_u16 =
            u16::try_from(priority).context(format!("Invalid SRV priority: {priority}"))?;
        let weight_u16 =
            u16::try_from(weight).context(format!("Invalid SRV weight: {weight}"))?;
        let port_u16 = u16::try_from(port).context(format!("Invalid SRV port: {port}"))?;

        let record_data = rdata::SRV::new(priority_u16, weight_u16, port_u16, target_name);

        let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::SRV(record_data));
        record.set_dns_class(DNSClass::IN);

        // Use append for idempotent operation
        let response = client
            .append(record, zone.clone(), false)
            .context(format!("Failed to send SRV record update for {fqdn_str}"))?;

        match response.response_code() {
            ResponseCode::NoError => {
                info!(
                    "Successfully added SRV record: {} -> {}:{} (priority: {}, weight: {}, TTL: {})",
                    fqdn_str, target_str, port, priority, weight, ttl_value
                );
            }
            code => {
                anyhow::bail!("DNS server rejected SRV record update for {fqdn_str}: {code:?}");
            }
        }

        Ok(())
    })
    .await
    .context("SRV record update task panicked")??;

    Ok(())
}

#[cfg(test)]
#[path = "srv_tests.rs"]
mod srv_tests;
