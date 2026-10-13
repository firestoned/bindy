// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! A and AAAA record management.

use super::super::types::RndcKeyData;
use super::{effective_record_ttl, write_rrset};
use anyhow::{Context, Result};
use hickory_proto::rr::{RData, RecordType};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

/// Parse every desired IPv4 address, failing on the first invalid one.
///
/// Called before any DNS traffic so a spec with one bad entry never touches
/// the live `RRset`.
///
/// # Errors
///
/// Returns an error naming the first address that is not a valid IPv4 address.
pub(crate) fn parse_ipv4_addresses(addresses: &[String]) -> Result<Vec<Ipv4Addr>> {
    addresses
        .iter()
        .map(|ip_str| {
            Ipv4Addr::from_str(ip_str).with_context(|| format!("Invalid IPv4 address: {ip_str}"))
        })
        .collect()
}

/// Parse every desired IPv6 address, failing on the first invalid one.
///
/// Called before any DNS traffic so a spec with one bad entry never touches
/// the live `RRset`.
///
/// # Errors
///
/// Returns an error naming the first address that is not a valid IPv6 address.
pub(crate) fn parse_ipv6_addresses(addresses: &[String]) -> Result<Vec<Ipv6Addr>> {
    addresses
        .iter()
        .map(|ip_str| {
            Ipv6Addr::from_str(ip_str).with_context(|| format!("Invalid IPv6 address: {ip_str}"))
        })
        .collect()
}

/// Convert an `ARecord` spec's addresses to the A `RRset` values, in order.
///
/// # Errors
///
/// Returns an error naming the first address that is not a valid IPv4 address.
pub(crate) fn a_rdata(addresses: &[String]) -> Result<Vec<RData>> {
    Ok(parse_ipv4_addresses(addresses)?
        .into_iter()
        .map(|ip| RData::A(ip.into()))
        .collect())
}

/// Convert an `AAAARecord` spec's addresses to the AAAA `RRset` values, in
/// order. Equivalent textual forms (case, compression) become the same value.
///
/// # Errors
///
/// Returns an error naming the first address that is not a valid IPv6 address.
pub(crate) fn aaaa_rdata(addresses: &[String]) -> Result<Vec<RData>> {
    Ok(parse_ipv6_addresses(addresses)?
        .into_iter()
        .map(|ip| RData::AAAA(ip.into()))
        .collect())
}

/// Write the A `RRset` for a name using dynamic DNS update (RFC 2136).
///
/// Validates every address before any DNS traffic, compares the live `RRset`
/// by value, and if it differs replaces it in one UPDATE (ADR-0021).
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
/// Returns an error if any address is not valid IPv4 (nothing is sent), the
/// DNS update fails, or the server rejects it.
pub async fn add_a_record(
    zone_name: &str,
    name: &str,
    ipv4_addresses: &[String],
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = a_rdata(ipv4_addresses)?;
    write_rrset(
        zone_name,
        name,
        RecordType::A,
        effective_record_ttl(ttl),
        desired,
        server,
        key_data,
    )
    .await
}

/// Write the AAAA `RRset` for a name using dynamic DNS update (RFC 2136).
///
/// Validates every address before any DNS traffic, compares the live `RRset`
/// by value, and if it differs replaces it in one UPDATE (ADR-0021).
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
/// Returns an error if any address is not valid IPv6 (nothing is sent), the
/// DNS update fails, or the server rejects it.
pub async fn add_aaaa_record(
    zone_name: &str,
    name: &str,
    ipv6_addresses: &[String],
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let desired = aaaa_rdata(ipv6_addresses)?;
    write_rrset(
        zone_name,
        name,
        RecordType::AAAA,
        effective_record_ttl(ttl),
        desired,
        server,
        key_data,
    )
    .await
}

#[cfg(test)]
#[path = "a_tests.rs"]
mod a_tests;
