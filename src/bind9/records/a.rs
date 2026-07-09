// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! A and AAAA record management.

use super::super::types::RndcKeyData;
use super::{
    build_authenticated_client, build_delete_rrset_record, build_record_fqdn, effective_record_ttl,
    rrset_ttl_matches, should_update_record,
};
use anyhow::{Context, Result};
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};
use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use tracing::{error, info, warn};

/// Compare existing DNS `RRset` with desired IPv4 addresses and TTL.
///
/// This function implements declarative reconciliation for A records by comparing
/// the current state (existing DNS records) with desired state (spec).
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `desired_ips` - IP addresses from `ARecordSpec`
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if existing `RRset` matches desired state exactly (no changes needed),
/// `false` if update required (add/remove IPs or TTL change needed).
fn compare_ip_rrset(existing_records: &[Record], desired_ips: &[String], desired_ttl: u32) -> bool {
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }

    let existing_ips: HashSet<String> = existing_records
        .iter()
        .filter_map(|record| {
            if let RData::A(ipv4) = &record.data {
                Some(ipv4.to_string())
            } else {
                None
            }
        })
        .collect();

    let desired_set: HashSet<String> = desired_ips.iter().cloned().collect();
    existing_ips == desired_set
}

/// Compare existing DNS `RRset` with desired IPv6 addresses and TTL.
///
/// This function implements declarative reconciliation for AAAA records by comparing
/// the current state (existing DNS records) with desired state (spec).
///
/// Both sides are parsed to [`Ipv6Addr`] before comparison so that equivalent
/// textual forms (e.g. `2001:DB8::1`, uncompressed notation) do not cause
/// endless delete/recreate churn.
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `desired_ips` - IP addresses from `AAAARecordSpec`
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if existing `RRset` matches desired state exactly (no changes needed),
/// `false` if update required (add/remove IPs or TTL change needed). A desired
/// IP that cannot be parsed is treated as a mismatch (with a warning), never a
/// panic.
fn compare_ipv6_rrset(
    existing_records: &[Record],
    desired_ips: &[String],
    desired_ttl: u32,
) -> bool {
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }

    let existing_ips: HashSet<Ipv6Addr> = existing_records
        .iter()
        .filter_map(|record| {
            if let RData::AAAA(ipv6) = &record.data {
                Some(ipv6.0)
            } else {
                None
            }
        })
        .collect();

    let mut desired_set: HashSet<Ipv6Addr> = HashSet::with_capacity(desired_ips.len());
    for ip_str in desired_ips {
        match Ipv6Addr::from_str(ip_str) {
            Ok(addr) => {
                desired_set.insert(addr);
            }
            Err(e) => {
                warn!(
                    "Invalid IPv6 address '{}' in desired spec (treating as mismatch): {}",
                    ip_str, e
                );
                return false;
            }
        }
    }

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
    let ttl_value = effective_record_ttl(ttl);
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::A,
        "A",
        server,
        |existing_records| compare_ip_rrset(existing_records, ipv4_addresses, ttl_value),
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone =
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    let mut client = build_authenticated_client(server, key_data).await?;

    // Step 1: delete existing RRset (ignore errors — may not exist).
    let delete_record = build_delete_rrset_record(&fqdn, RecordType::A);
    let _ = client.delete_rrset(delete_record, zone.clone()).await;

    info!(
        "Adding A record RRset: {} -> {:?} (TTL: {}, {} addresses)",
        fqdn,
        ipv4_addresses,
        ttl_value,
        ipv4_addresses.len()
    );

    // Step 2: append all desired IPs to create the new RRset.
    for ip_str in ipv4_addresses {
        let ipv4_addr = Ipv4Addr::from_str(ip_str)
            .with_context(|| format!("Invalid IPv4 address: {ip_str}"))?;

        let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::A(ipv4_addr.into()));
        record.dns_class = DNSClass::IN;

        let response = client
            .append(record, zone.clone(), false)
            .await
            .with_context(|| format!("Failed to add A record for {fqdn} -> {ip_str}"))?;

        match response.metadata.response_code {
            ResponseCode::NoError => {
                info!("Successfully added A record: {} -> {}", name, ip_str);
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
    let ttl_value = effective_record_ttl(ttl);
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::AAAA,
        "AAAA",
        server,
        |existing_records| compare_ipv6_rrset(existing_records, ipv6_addresses, ttl_value),
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone =
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    let mut client = build_authenticated_client(server, key_data).await?;

    let delete_record = build_delete_rrset_record(&fqdn, RecordType::AAAA);
    let _ = client.delete_rrset(delete_record, zone.clone()).await;

    info!(
        "Adding AAAA record RRset: {} -> {:?} (TTL: {}, {} addresses)",
        fqdn,
        ipv6_addresses,
        ttl_value,
        ipv6_addresses.len()
    );

    for ip_str in ipv6_addresses {
        let ipv6_addr = Ipv6Addr::from_str(ip_str)
            .with_context(|| format!("Invalid IPv6 address: {ip_str}"))?;

        let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::AAAA(ipv6_addr.into()));
        record.dns_class = DNSClass::IN;

        let response = client
            .append(record, zone.clone(), false)
            .await
            .with_context(|| format!("Failed to add AAAA record for {fqdn} -> {ip_str}"))?;

        match response.metadata.response_code {
            ResponseCode::NoError => {
                info!("Successfully added AAAA record: {} -> {}", name, ip_str);
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
}

#[cfg(test)]
#[path = "a_tests.rs"]
mod a_tests;
