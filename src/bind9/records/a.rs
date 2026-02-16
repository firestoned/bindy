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
use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use tracing::{error, info};

use crate::bind9::rndc::create_tsig_signer;
use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Compare existing DNS `RRset` with desired IPv4 addresses.
///
/// This function implements declarative reconciliation for A records by comparing
/// the current state (existing DNS records) with desired state (spec).
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `desired_ips` - IP addresses from `ARecordSpec`
///
/// # Returns
///
/// `true` if existing `RRset` matches desired state exactly (no changes needed),
/// `false` if update required (add/remove IPs needed).
fn compare_ip_rrset(existing_records: &[Record], desired_ips: &[String]) -> bool {
    // Extract IPs from existing DNS records
    let existing_ips: HashSet<String> = existing_records
        .iter()
        .filter_map(|record| {
            // Extract IP from RData
            if let Some(RData::A(ipv4)) = record.data() {
                Some(ipv4.to_string())
            } else {
                None // Ignore non-A records (shouldn't happen)
            }
        })
        .collect();

    // Convert desired IPs to HashSet for comparison
    // HashSet automatically handles:
    // - Duplicates (deduplication)
    // - Order independence
    let desired_set: HashSet<String> = desired_ips.iter().cloned().collect();

    // Compare sets - true if identical (same IPs, no extras, no missing)
    existing_ips == desired_set
}

/// Compare existing DNS `RRset` with desired IPv6 addresses.
///
/// This function implements declarative reconciliation for AAAA records by comparing
/// the current state (existing DNS records) with desired state (spec).
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `desired_ips` - IP addresses from `AAAARecordSpec`
///
/// # Returns
///
/// `true` if existing `RRset` matches desired state exactly (no changes needed),
/// `false` if update required (add/remove IPs needed).
fn compare_ipv6_rrset(existing_records: &[Record], desired_ips: &[String]) -> bool {
    // Extract IPs from existing DNS records
    let existing_ips: HashSet<String> = existing_records
        .iter()
        .filter_map(|record| {
            // Extract IP from RData
            if let Some(RData::AAAA(ipv6)) = record.data() {
                Some(ipv6.to_string())
            } else {
                None
            }
        })
        .collect();

    // Convert desired IPs to set
    let desired_set: HashSet<String> = desired_ips.iter().cloned().collect();

    // Compare sets
    existing_ips == desired_set
}

/// Add A records using dynamic DNS update (RFC 2136) with `RRset` synchronization.
///
/// This function implements declarative `RRset` management:
/// 1. Compares existing DNS records with desired IPs
/// 2. If mismatch, deletes entire `RRset` and recreates with desired IPs
/// 3. If match, skips update (idempotent)
///
/// # Arguments
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `name` - Record name (e.g., "www" for www.example.com, or "@" for apex)
/// * `ipv4_addresses` - List of IPv4 addresses for round-robin DNS
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
    ipv4_addresses: &[String],
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
            // Compare RRsets: returns true if existing matches desired state
            compare_ip_rrset(existing_records, ipv4_addresses)
        },
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone_name_str = zone_name.to_string();
    let name_str = name.to_string();
    let ipv4_addresses_vec: Vec<String> = ipv4_addresses.to_vec();
    let server_str = server.to_string();
    let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

    // Clone key_data for the blocking task
    let key_data = key_data.clone();

    // Clone for error message (ipv4_addresses_vec will be moved into closure)
    let ipv4_addresses_for_error = ipv4_addresses_vec.clone();

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

        // Step 1: Delete existing RRset (ignore errors - may not exist)
        let mut delete_record = Record::new();
        delete_record.set_name(fqdn.clone());
        delete_record.set_record_type(RecordType::A);
        delete_record.set_dns_class(DNSClass::IN);
        let _ = client.delete_rrset(delete_record, zone.clone());

        // Step 2: Add all desired IPs to create new RRset
        info!(
            "Adding A record RRset: {} -> {:?} (TTL: {}, {} addresses)",
            fqdn,
            ipv4_addresses_vec,
            ttl_value,
            ipv4_addresses_vec.len()
        );

        for ip_str in &ipv4_addresses_vec {
            // Parse IPv4 address
            let ipv4_addr = Ipv4Addr::from_str(ip_str)
                .with_context(|| format!("Invalid IPv4 address: {ip_str}"))?;

            // Create A record
            let mut record =
                Record::from_rdata(fqdn.clone(), ttl_value, RData::A(ipv4_addr.into()));
            record.set_dns_class(DNSClass::IN);

            // Append record to RRset
            let response = client
                .append(record, zone.clone(), false)
                .with_context(|| format!("Failed to add A record for {fqdn} -> {ip_str}"))?;

            // Check response code
            match response.response_code() {
                ResponseCode::NoError => {
                    info!("Successfully added A record: {} -> {}", name_str, ip_str);
                }
                code => {
                    error!(
                        "DNS UPDATE rejected by server for {} -> {} with response code: {:?}",
                        fqdn, ip_str, code
                    );
                    return Err(anyhow::anyhow!(
                        "DNS update failed with response code: {code:?}"
                    ));
                }
            }
        }

        Ok(())
    })
    .await
    .with_context(|| format!("DNS update task panicked or failed for A record {name} -> {ipv4_addresses_for_error:?}"))?
}

/// Add AAAA records using dynamic DNS update (RFC 2136) with `RRset` synchronization.
///
/// This function implements declarative `RRset` management:
/// 1. Compares existing DNS records with desired IPv6 addresses
/// 2. If mismatch, deletes entire `RRset` and recreates with desired IPs
/// 3. If match, skips update (idempotent)
///
/// # Arguments
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `name` - Record name (e.g., "www" for www.example.com, or "@" for apex)
/// * `ipv6_addresses` - List of IPv6 addresses for round-robin DNS
/// * `ttl` - Time to live in seconds (None = use zone default)
/// * `server` - DNS server address with port (e.g., "10.0.0.1:53")
/// * `key_data` - TSIG key for authentication
///
/// # Errors
///
/// Returns an error if the DNS update fails or the server rejects it.
#[allow(clippy::too_many_arguments)]
pub async fn add_aaaa_record(
    zone_name: &str,
    name: &str,
    ipv6_addresses: &[String],
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
            // Compare RRsets: returns true if existing matches desired state
            compare_ipv6_rrset(existing_records, ipv6_addresses)
        },
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone_name_str = zone_name.to_string();
    let name_str = name.to_string();
    let ipv6_addresses_vec: Vec<String> = ipv6_addresses.to_vec();
    let server_str = server.to_string();
    let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
    let key_data = key_data.clone();

    // Clone for error message (ipv6_addresses_vec will be moved into closure)
    let ipv6_addresses_for_error = ipv6_addresses_vec.clone();

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

        // Step 1: Delete existing RRset (ignore errors - may not exist)
        let mut delete_record = Record::new();
        delete_record.set_name(fqdn.clone());
        delete_record.set_record_type(RecordType::AAAA);
        delete_record.set_dns_class(DNSClass::IN);
        let _ = client.delete_rrset(delete_record, zone.clone());

        // Step 2: Add all desired IPv6 addresses to create new RRset
        info!(
            "Adding AAAA record RRset: {} -> {:?} (TTL: {}, {} addresses)",
            fqdn,
            ipv6_addresses_vec,
            ttl_value,
            ipv6_addresses_vec.len()
        );

        for ip_str in &ipv6_addresses_vec {
            let ipv6_addr = Ipv6Addr::from_str(ip_str)
                .with_context(|| format!("Invalid IPv6 address: {ip_str}"))?;
            let mut record =
                Record::from_rdata(fqdn.clone(), ttl_value, RData::AAAA(ipv6_addr.into()));
            record.set_dns_class(DNSClass::IN);

            let response = client
                .append(record, zone.clone(), false)
                .with_context(|| format!("Failed to add AAAA record for {fqdn} -> {ip_str}"))?;

            match response.response_code() {
                ResponseCode::NoError => {
                    info!("Successfully added AAAA record: {} -> {}", name_str, ip_str);
                }
                code => {
                    error!(
                        "DNS UPDATE rejected by server for {} -> {} with response code: {:?}",
                        fqdn, ip_str, code
                    );
                    return Err(anyhow::anyhow!(
                        "DNS update failed with response code: {code:?}"
                    ));
                }
            }
        }

        Ok(())
    })
    .await
    .with_context(|| format!("DNS update task panicked or failed for AAAA record {name} -> {ipv6_addresses_for_error:?}"))?
}

#[cfg(test)]
#[path = "a_tests.rs"]
mod a_tests;
