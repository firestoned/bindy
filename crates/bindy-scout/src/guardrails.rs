// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Scout's zone and address allowlists (ADR-0025, decisions 1 and 2).
//!
//! `--allowed-zones` bounds the zones a Scout publishes into, whatever a
//! tenant writes in `bindy.firestoned.io/zone`; `--allowed-ip-ranges` bounds
//! the addresses it publishes, whatever a tenant writes in
//! `bindy.firestoned.io/ip`. Both are set by whoever deploys Scout, not by
//! the tenants whose objects it reads.

use anyhow::{anyhow, Result};
use std::net::Ipv4Addr;

/// RFC 1035 limit on a DNS name, in octets, without the trailing dot.
const MAX_DNS_NAME_LEN: usize = 253;

/// RFC 1035 limit on one DNS label, in octets.
const MAX_DNS_LABEL_LEN: usize = 63;

/// Number of bits in an IPv4 address, the longest valid prefix.
const IPV4_BITS: u32 = 32;

/// One IPv4 range from `--allowed-ip-ranges`, held as a network and a mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Range {
    network: u32,
    mask: u32,
}

impl Ipv4Range {
    /// Whether `addr` falls inside this range.
    fn contains(self, addr: Ipv4Addr) -> bool {
        u32::from(addr) & self.mask == self.network
    }
}

/// Lowercases a zone name and drops a trailing dot.
fn normalize_zone(zone: &str) -> String {
    zone.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Whether `name` (already normalized) is a DNS name Scout can use as a zone:
/// non-empty labels of letters, digits, `-` and `_`, no label starting or
/// ending with `-`, within the RFC 1035 lengths.
fn is_dns_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_DNS_NAME_LEN {
        return false;
    }
    name.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= MAX_DNS_LABEL_LEN
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Parses `--allowed-zones` / `BINDY_SCOUT_ALLOWED_ZONES`: a comma-separated
/// list of zones, each lowercased and stripped of a trailing dot.
///
/// # Errors
/// Returns an error when the list has no entries, contains `*` (a wildcard
/// would restore the unrestricted default under a name that suggests a
/// restriction), or contains an entry that is not a DNS name.
pub(crate) fn parse_allowed_zones(raw: &str) -> Result<Vec<String>> {
    let mut zones = Vec::new();
    for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        if entry == "*" {
            return Err(anyhow!(
                "BINDY_SCOUT_ALLOWED_ZONES (--allowed-zones) may not contain '*'; \
                 leave it unset to allow every zone"
            ));
        }
        let zone = normalize_zone(entry);
        if !is_dns_name(&zone) {
            return Err(anyhow!(
                "BINDY_SCOUT_ALLOWED_ZONES (--allowed-zones) entry {entry:?} is not a DNS name"
            ));
        }
        zones.push(zone);
    }
    if zones.is_empty() {
        return Err(anyhow!(
            "BINDY_SCOUT_ALLOWED_ZONES (--allowed-zones) is set but lists no zone"
        ));
    }
    Ok(zones)
}

/// Whether `zone` is one of `allowed` or a subzone of one, ignoring case and
/// a trailing dot.
///
/// `allowed` holds entries from [`parse_allowed_zones`], already normalized.
#[must_use]
pub(crate) fn zone_is_allowed(allowed: &[String], zone: &str) -> bool {
    let zone = normalize_zone(zone);
    allowed.iter().any(|entry| {
        zone == *entry
            || zone
                .strip_suffix(entry.as_str())
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

/// Parses one range: `a.b.c.d/n`, or a bare `a.b.c.d` meaning `/32`.
fn parse_ip_range(entry: &str) -> Result<Ipv4Range> {
    let (addr, prefix) = match entry.split_once('/') {
        Some((addr, prefix)) => (addr, prefix.parse::<u32>().ok()),
        None => (entry, Some(IPV4_BITS)),
    };
    let addr = addr.parse::<Ipv4Addr>().ok();
    let (Some(addr), Some(prefix)) = (addr, prefix.filter(|p| *p <= IPV4_BITS)) else {
        return Err(anyhow!(
            "BINDY_SCOUT_ALLOWED_IP_RANGES (--allowed-ip-ranges) entry {entry:?} is not an \
             IPv4 address or CIDR range"
        ));
    };
    let mask = u32::MAX.checked_shl(IPV4_BITS - prefix).unwrap_or(0);
    Ok(Ipv4Range {
        network: u32::from(addr) & mask,
        mask,
    })
}

/// Parses `--allowed-ip-ranges` / `BINDY_SCOUT_ALLOWED_IP_RANGES`: a
/// comma-separated list of IPv4 CIDR ranges (a bare address means `/32`).
///
/// # Errors
/// Returns an error when the list has no entries or an entry is not an IPv4
/// address or CIDR range.
pub(crate) fn parse_allowed_ip_ranges(raw: &str) -> Result<Vec<Ipv4Range>> {
    let ranges = raw
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(parse_ip_range)
        .collect::<Result<Vec<_>>>()?;
    if ranges.is_empty() {
        return Err(anyhow!(
            "BINDY_SCOUT_ALLOWED_IP_RANGES (--allowed-ip-ranges) is set but lists no range"
        ));
    }
    Ok(ranges)
}

/// The first of `ips` outside every range in `ranges`, if any. An address
/// that does not parse as IPv4 is outside.
#[must_use]
pub(crate) fn first_ip_outside(ranges: &[Ipv4Range], ips: &[String]) -> Option<String> {
    ips.iter()
        .find(|ip| {
            ip.trim()
                .parse::<Ipv4Addr>()
                .map_or(true, |addr| !ranges.iter().any(|r| r.contains(addr)))
        })
        .cloned()
}

#[cfg(test)]
#[path = "guardrails_tests.rs"]
mod guardrails_tests;
