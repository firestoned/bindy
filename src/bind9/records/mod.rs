// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! DNS record management functions using dynamic DNS updates (RFC 2136).
//!
//! This module provides functions for managing DNS records via the nsupdate protocol.
//! Each record type has its own submodule with specialized functions.

pub mod a;
pub mod caa;
pub mod cname;
pub mod mx;
pub mod ns;
pub mod srv;
pub mod txt;

use anyhow::{Context, Result};
use hickory_client::client::{Client, SyncClient};
use hickory_client::rr::Name;
use hickory_client::rr::{DNSClass, Record};
use hickory_client::udp::UdpClientConnection;
use std::str::FromStr;
use tracing::{info, warn};

/// Generic DNS record query function.
///
/// Queries a DNS server for records of a specific type and returns the results.
///
/// # Arguments
///
/// * `zone_name` - The DNS zone name
/// * `name` - The record name (e.g., "www" for www.example.com, or "@" for apex)
/// * `record_type` - The DNS record type (A, AAAA, TXT, MX, etc.)
/// * `server` - The DNS server address (IP:port)
///
/// # Returns
///
/// Returns `Ok(vec)` with matching records (empty if none exist),
/// or an error if the query fails.
///
/// # Errors
///
/// Returns an error if the DNS query fails or cannot be parsed.
pub async fn query_dns_record(
    zone_name: &str,
    name: &str,
    record_type: hickory_client::rr::RecordType,
    server: &str,
) -> Result<Vec<Record>> {
    let zone_name_str = zone_name.to_string();
    let name_str = name.to_string();
    let server_str = server.to_string();

    tokio::task::spawn_blocking(move || {
        // Parse server address
        let server_addr = server_str
            .parse::<std::net::SocketAddr>()
            .with_context(|| format!("Invalid server address: {server_str}"))?;

        // Create UDP connection for query
        let conn = UdpClientConnection::new(server_addr)
            .context("Failed to create UDP connection for query")?;
        let client = SyncClient::new(conn);

        // Build full record name
        let fqdn = if name_str == "@" || name_str.is_empty() {
            Name::from_str(&zone_name_str)
                .with_context(|| format!("Invalid zone name: {zone_name_str}"))?
        } else {
            Name::from_str(&format!("{name_str}.{zone_name_str}"))
                .with_context(|| format!("Invalid record name: {name_str}.{zone_name_str}"))?
        };

        // Query for records
        let response = client
            .query(&fqdn, DNSClass::IN, record_type)
            .with_context(|| format!("Failed to query {record_type:?} record for {fqdn}"))?;

        // Extract matching records from response
        let records: Vec<Record> = response
            .answers()
            .iter()
            .filter(|r| r.record_type() == record_type)
            .cloned()
            .collect();

        Ok(records)
    })
    .await
    .context("DNS query task failed")?
}

/// Helper for declarative record reconciliation.
///
/// Implements the observe → diff → act pattern for DNS records:
/// 1. Query existing record
/// 2. Compare with desired state using provided callback
/// 3. Skip if already correct, otherwise proceed with update
///
/// # Arguments
///
/// * `zone_name` - The DNS zone name
/// * `name` - The record name
/// * `record_type` - The DNS record type
/// * `record_type_name` - Human-readable name (e.g., "A", "AAAA")
/// * `server` - The DNS server address
/// * `compare_fn` - Callback to compare existing records with desired state
///
/// # Returns
///
/// Returns `Ok(true)` if update is needed, `Ok(false)` if record already matches.
///
/// # Errors
///
/// Returns an error only if the query fails critically.
pub async fn should_update_record<F>(
    zone_name: &str,
    name: &str,
    record_type: hickory_client::rr::RecordType,
    record_type_name: &str,
    server: &str,
    compare_fn: F,
) -> Result<bool>
where
    F: FnOnce(&[Record]) -> bool,
{
    match query_dns_record(zone_name, name, record_type, server).await {
        Ok(existing_records) if !existing_records.is_empty() => {
            // Records exist - use callback to compare
            if compare_fn(&existing_records) {
                info!(
                    "{} record {} already exists with correct value - no changes needed",
                    record_type_name, name
                );
                Ok(false) // Skip update
            } else {
                info!(
                    "{} record {} exists with different value(s), updating",
                    record_type_name, name
                );
                Ok(true) // Need update
            }
        }
        Ok(_) => {
            // No records exist
            info!(
                "{} record {} does not exist, creating",
                record_type_name, name
            );
            Ok(true) // Need creation
        }
        Err(e) => {
            // Query failed - log warning but allow update attempt
            warn!(
                "Failed to query existing {} record {} (will attempt update anyway): {}",
                record_type_name, name, e
            );
            Ok(true) // Proceed with update
        }
    }
}
