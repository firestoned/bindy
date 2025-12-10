// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! A and AAAA record management.

use super::super::types::RndcKeyData;
use super::should_update_record;
use anyhow::{Context, Result};
use hickory_client::client::{Client, SyncClient};
use hickory_client::op::ResponseCode;
use hickory_client::rr::{DNSClass, Name, RData, Record};
use hickory_client::udp::UdpClientConnection;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use tracing::{error, info};

use crate::bind9::rndc::create_tsig_signer;
use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Add an A record using dynamic DNS update (RFC 2136).
///
/// # Arguments
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `name` - Record name (e.g., "www" for www.example.com, or "@" for apex)
/// * `ipv4` - IPv4 address
/// * `ttl` - Time to live in seconds (None = use zone default)
/// * `server` - DNS server address with port (e.g., "10.0.0.1:53")
/// * `key_data` - TSIG key for authentication
///
/// # Errors
///
/// Returns an error if the DNS update fails or the server rejects it.
#[allow(clippy::too_many_arguments)]
pub async fn add_a_record(
    zone_name: &str,
    name: &str,
    ipv4: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    use hickory_client::rr::RecordType;

    // Check if update is needed using declarative reconciliation pattern
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::A,
        "A",
        server,
        |existing_records| {
            // Compare: should return true if records match desired state
            if existing_records.len() == 1 {
                if let Some(RData::A(existing_ip)) = existing_records[0].data() {
                    return existing_ip.to_string() == ipv4;
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
    let ipv4_str = ipv4.to_string();
    let server_str = server.to_string();
    let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

    // Clone key_data for the blocking task
    let key_data = key_data.clone();

    // Execute DNS update in blocking thread (hickory-client is sync)
    tokio::task::spawn_blocking(move || {
        // Parse server address
        let server_addr = server_str
            .parse::<std::net::SocketAddr>()
            .with_context(|| format!("Invalid server address: {server_str}"))?;

        // Create UDP connection
        let conn =
            UdpClientConnection::new(server_addr).context("Failed to create UDP connection")?;

        // Create TSIG signer
        let signer = create_tsig_signer(&key_data)?;

        // Create client with TSIG
        let client = SyncClient::with_tsigner(conn, signer);

        // Parse zone name
        let zone = Name::from_str(&zone_name_str)
            .with_context(|| format!("Invalid zone name: {zone_name_str}"))?;

        // Build full record name
        let fqdn = if name_str == "@" || name_str.is_empty() {
            zone.clone()
        } else {
            Name::from_str(&format!("{name_str}.{zone_name_str}"))
                .with_context(|| format!("Invalid record name: {name_str}.{zone_name_str}"))?
        };

        // Parse IPv4 address
        let ipv4_addr = Ipv4Addr::from_str(&ipv4_str)
            .with_context(|| format!("Invalid IPv4 address: {ipv4_str}"))?;

        // Create A record
        let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::A(ipv4_addr.into()));
        record.set_dns_class(DNSClass::IN);

        // Send update using append for idempotent operation
        // append() adds the record to the RRset, or creates a new RRset if none exists
        // must_exist=false means no prerequisite check - truly idempotent
        info!(
            "Adding A record: {} -> {} (TTL: {})",
            fqdn, ipv4_str, ttl_value
        );
        let response = client
            .append(record, zone.clone(), false)
            .with_context(|| format!("Failed to send DNS UPDATE for A record {fqdn}"))?;

        // Check response code
        match response.response_code() {
            ResponseCode::NoError => {
                info!("Successfully added A record: {} -> {}", name_str, ipv4_str);
                Ok(())
            }
            code => {
                error!(
                    "DNS UPDATE rejected by server for {} -> {} with response code: {:?}",
                    fqdn, ipv4_str, code
                );
                Err(anyhow::anyhow!(
                    "DNS update failed with response code: {code:?}"
                ))
            }
        }
    })
    .await
    .with_context(|| format!("DNS update task panicked or failed for A record {name} -> {ipv4}"))?
}

/// Add an AAAA record using dynamic DNS update (RFC 2136).
///
/// # Errors
///
/// Returns an error if the DNS update fails or the server rejects it.
#[allow(clippy::too_many_arguments)]
pub async fn add_aaaa_record(
    zone_name: &str,
    name: &str,
    ipv6: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    use hickory_client::rr::RecordType;

    // Check if update is needed using declarative reconciliation pattern
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::AAAA,
        "AAAA",
        server,
        |existing_records| {
            // Compare: should return true if records match desired state
            if existing_records.len() == 1 {
                if let Some(RData::AAAA(existing_ip)) = existing_records[0].data() {
                    return existing_ip.to_string() == ipv6;
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
    let ipv6_str = ipv6.to_string();
    let server_str = server.to_string();
    let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
    let key_data = key_data.clone();

    tokio::task::spawn_blocking(move || {
        let server_addr = server_str
            .parse::<std::net::SocketAddr>()
            .with_context(|| format!("Invalid server address: {server_str}"))?;
        let conn =
            UdpClientConnection::new(server_addr).context("Failed to create UDP connection")?;
        let signer = create_tsig_signer(&key_data)?;
        let client = SyncClient::with_tsigner(conn, signer);

        let zone = Name::from_str(&zone_name_str)
            .with_context(|| format!("Invalid zone name: {zone_name_str}"))?;
        let fqdn = if name_str == "@" || name_str.is_empty() {
            zone.clone()
        } else {
            Name::from_str(&format!("{name_str}.{zone_name_str}"))
                .with_context(|| format!("Invalid record name: {name_str}.{zone_name_str}"))?
        };

        let ipv6_addr = Ipv6Addr::from_str(&ipv6_str)
            .with_context(|| format!("Invalid IPv6 address: {ipv6_str}"))?;
        let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::AAAA(ipv6_addr.into()));
        record.set_dns_class(DNSClass::IN);

        // Use append for idempotent operation (must_exist=false for no prerequisites)
        info!(
            "Adding AAAA record: {} -> {} (TTL: {})",
            fqdn, ipv6_str, ttl_value
        );
        let response = client
            .append(record, zone.clone(), false)
            .with_context(|| format!("Failed to add AAAA record for {fqdn}"))?;

        match response.response_code() {
            ResponseCode::NoError => {
                info!(
                    "Successfully added AAAA record: {} -> {}",
                    name_str, ipv6_str
                );
                Ok(())
            }
            code => Err(anyhow::anyhow!(
                "DNS update failed with response code: {code:?}"
            )),
        }
    })
    .await
    .context("DNS update task failed")?
}

#[cfg(test)]
#[path = "a_tests.rs"]
mod a_tests;
