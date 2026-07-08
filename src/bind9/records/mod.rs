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
use hickory_net::client::{Client, ClientHandle};
use hickory_net::runtime::TokioRuntimeProvider;
use hickory_net::udp::UdpClientStream;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};
use std::net::SocketAddr;
use std::str::FromStr;
use tracing::{info, warn};

use crate::bind9::rndc::create_tsig_signer;
use crate::bind9::types::RndcKeyData;
use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Fallback TTL (seconds) used only if [`DEFAULT_DNS_RECORD_TTL_SECS`] cannot be
/// converted to `u32`.
const FALLBACK_DNS_RECORD_TTL_SECS: u32 = 300;

/// Resolve the effective TTL for a DNS record from an optional spec value.
///
/// Returns the spec TTL when present and representable as `u32`; otherwise falls
/// back to [`DEFAULT_DNS_RECORD_TTL_SECS`].
///
/// This is the single source of truth for the TTL written to DNS, and is also
/// used when diffing desired state against existing records so that TTL-only
/// spec changes trigger an update.
pub(crate) fn effective_record_ttl(ttl: Option<i32>) -> u32 {
    u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS)).unwrap_or_else(|_| {
        u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(FALLBACK_DNS_RECORD_TTL_SECS)
    })
}

/// Check whether every record in an existing `RRset` carries the desired TTL.
///
/// Used by the per-record-type compare functions so that a TTL-only spec change
/// is detected as a mismatch and triggers the update path.
pub(crate) fn rrset_ttl_matches(existing_records: &[Record], desired_ttl: u32) -> bool {
    existing_records
        .iter()
        .all(|record| record.ttl == desired_ttl)
}

/// Build the placeholder record used by RFC 2136 "delete `RRset`" operations.
///
/// `delete_rrset()` overwrites class/TTL/data based on `record_type`, so the
/// returned record only needs to carry the FQDN and the record type.
pub(crate) fn build_delete_rrset_record(fqdn: &Name, record_type: RecordType) -> Record {
    Record::from_rdata(fqdn.clone(), 0, RData::Update0(record_type))
}

/// Response codes that indicate an RFC 2136 delete succeeded (`NoError`) or the
/// target records already did not exist (`NXDomain`, `NXRRSet`) — idempotent
/// success.
///
/// Every other code (e.g. `Refused`, `NotAuth`, `NotZone`, `ServFail`) is a real
/// failure and must be surfaced to the caller.
pub(crate) fn is_idempotent_delete_response_code(code: ResponseCode) -> bool {
    matches!(
        code,
        ResponseCode::NoError | ResponseCode::NXDomain | ResponseCode::NXRRSet
    )
}

/// Build an unauthenticated UDP DNS client for read-only queries.
async fn build_query_client(server_str: &str) -> Result<Client<TokioRuntimeProvider>> {
    let server_addr: SocketAddr = server_str
        .parse()
        .with_context(|| format!("Invalid server address: {server_str}"))?;
    let stream = UdpClientStream::builder(server_addr, TokioRuntimeProvider::default()).build();
    let (client, bg) = Client::<TokioRuntimeProvider>::from_sender(stream);
    tokio::spawn(bg);
    Ok(client)
}

/// Build a TSIG-authenticated UDP DNS client for RFC 2136 dynamic updates.
pub(crate) async fn build_authenticated_client(
    server_str: &str,
    key_data: &RndcKeyData,
) -> Result<Client<TokioRuntimeProvider>> {
    let server_addr: SocketAddr = server_str
        .parse()
        .with_context(|| format!("Invalid server address: {server_str}"))?;
    let signer = create_tsig_signer(key_data)?;
    let stream = UdpClientStream::builder(server_addr, TokioRuntimeProvider::default())
        .with_signer(Some(signer))
        .build();
    let (client, bg) = Client::<TokioRuntimeProvider>::from_sender(stream);
    tokio::spawn(bg);
    Ok(client)
}

/// Build the fully-qualified record name for a given (zone, name).
///
/// `@` or empty `name` produces the zone apex; otherwise the name is concatenated with the zone.
pub(crate) fn build_record_fqdn(zone_name: &str, name: &str) -> Result<Name> {
    if name == "@" || name.is_empty() {
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))
    } else {
        Name::from_str(&format!("{name}.{zone_name}"))
            .with_context(|| format!("Invalid record name: {name}.{zone_name}"))
    }
}

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
    record_type: RecordType,
    server: &str,
) -> Result<Vec<Record>> {
    let mut client = build_query_client(server).await?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    let response = client
        .query(fqdn.clone(), DNSClass::IN, record_type)
        .await
        .with_context(|| format!("Failed to query {record_type:?} record for {fqdn}"))?;

    let records: Vec<Record> = response
        .answers
        .iter()
        .filter(|r| r.record_type() == record_type)
        .cloned()
        .collect();

    Ok(records)
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
    record_type: RecordType,
    record_type_name: &str,
    server: &str,
    compare_fn: F,
) -> Result<bool>
where
    F: FnOnce(&[Record]) -> bool,
{
    match query_dns_record(zone_name, name, record_type, server).await {
        Ok(existing_records) if !existing_records.is_empty() => {
            if compare_fn(&existing_records) {
                info!(
                    "{} record {} already exists with correct value - no changes needed",
                    record_type_name, name
                );
                Ok(false)
            } else {
                info!(
                    "{} record {} exists with different value(s), updating",
                    record_type_name, name
                );
                Ok(true)
            }
        }
        Ok(_) => {
            info!(
                "{} record {} does not exist, creating",
                record_type_name, name
            );
            Ok(true)
        }
        Err(e) => {
            warn!(
                "Failed to query existing {} record {} (will attempt update anyway): {}",
                record_type_name, name, e
            );
            Ok(true)
        }
    }
}

/// Delete a DNS record of any type using dynamic DNS update (RFC 2136).
///
/// This function sends an RFC 2136 DELETE operation to remove ALL records
/// of the specified type for the given name.
///
/// # Arguments
///
/// * `zone_name` - The DNS zone name (e.g., "example.com")
/// * `name` - The record name (e.g., "www" for www.example.com, or "@" for apex)
/// * `record_type` - The DNS record type to delete (A, AAAA, TXT, MX, etc.)
/// * `server` - The DNS server address (IP:port, e.g., "10.0.0.1:53")
/// * `key_data` - TSIG key for authentication
///
/// # Returns
///
/// Returns `Ok(())` if deletion succeeded (`NoError`) or the record already did
/// not exist (`NXDomain`/`NXRRSet` — idempotent success).
///
/// # Errors
///
/// Returns an error if the connection fails or the DNS server rejects the
/// update with any other response code (e.g. `Refused`, `NotAuth`, `NotZone`,
/// `ServFail`), so TSIG/ACL failures are never silently swallowed.
pub async fn delete_dns_record(
    zone_name: &str,
    name: &str,
    record_type: RecordType,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let mut client = build_authenticated_client(server, key_data).await?;
    let zone =
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    info!(
        "Deleting {:?} record: {} from zone {}",
        record_type, fqdn, zone_name
    );

    // Build a placeholder record. delete_rrset() overwrites class/ttl/data based on record_type.
    let dummy_record = build_delete_rrset_record(&fqdn, record_type);

    let response = client
        .delete_rrset(dummy_record, zone)
        .await
        .with_context(|| {
            format!("Failed to send DNS UPDATE to delete {record_type:?} record {fqdn}")
        })?;

    let code = response.metadata.response_code;
    if !is_idempotent_delete_response_code(code) {
        return Err(anyhow::anyhow!(
            "DNS DELETE for {record_type:?} record {fqdn} in zone {zone_name} \
             rejected with response code: {code:?}"
        ));
    }

    if code == ResponseCode::NoError {
        info!(
            "Successfully deleted {:?} record: {} from zone {}",
            record_type, name, zone_name
        );
        return Ok(());
    }

    // NXDomain/NXRRSet: the record did not exist — deletion is idempotent.
    warn!(
        "DNS DELETE for {:?} record {fqdn} returned code: {:?} (record did not exist)",
        record_type, code
    );
    Ok(())
}

#[cfg(test)]
mod mod_tests;
