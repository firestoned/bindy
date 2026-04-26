// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Strict validator for BIND9 `address_match_list` entries used in
//! `allow-query`, `allow-transfer`, and related ACL directives.
//!
//! CRD-supplied values flow directly into `named.conf`. Without validation a
//! malicious or compromised CRD author could close the enclosing `{ … }` block
//! and append arbitrary BIND9 directives. This module implements a strict
//! whitelist of the address-match-list forms bindy supports, rejecting
//! anything else with a structured error that reconcilers propagate to the
//! resource status.
//!
//! Accepted forms (optionally prefixed with `!` for negation):
//! - keywords: `any`, `none`, `localhost`, `localnets`
//! - IPv4 address with optional `/prefix` (0..=32)
//! - IPv6 address with optional `/prefix` (0..=128)
//! - `key <name>` where `<name>` matches `[A-Za-z0-9._-]{1,253}`

use std::net::IpAddr;
use thiserror::Error;

/// Maximum accepted length of a single ACL entry, in bytes. Any reasonable
/// address-match token is well under this; the cap is defensive against
/// pathologically large CRD inputs.
pub const MAX_ACL_ENTRY_LEN: usize = 256;

/// Maximum accepted length of a TSIG/RNDC key name in an `key <name>` entry.
/// Matches the DNS name length limit from RFC 1035.
const MAX_KEY_NAME_LEN: usize = 253;

const ACL_KEYWORDS: [&str; 4] = ["any", "none", "localhost", "localnets"];

const IPV4_MAX_PREFIX: u8 = 32;
const IPV6_MAX_PREFIX: u8 = 128;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AclError {
    #[error("ACL entry must not be empty")]
    Empty,

    #[error("ACL entry exceeds {MAX_ACL_ENTRY_LEN}-byte limit: {0:?}")]
    TooLong(String),

    #[error(
        "ACL entry {0:?} is not a recognized address-match token. \
         Accepted: any, none, localhost, localnets, an IPv4 or IPv6 address \
         (optionally with /prefix), or \"key <name>\" — each optionally \
         prefixed with '!'"
    )]
    InvalidToken(String),
}

/// Validate a single `address_match_list` entry.
///
/// # Errors
/// Returns [`AclError`] if the entry is empty, exceeds
/// [`MAX_ACL_ENTRY_LEN`], or does not match one of the accepted forms.
pub fn validate_acl_entry(entry: &str) -> Result<(), AclError> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return Err(AclError::Empty);
    }
    if trimmed.len() > MAX_ACL_ENTRY_LEN {
        return Err(AclError::TooLong(trimmed.to_string()));
    }

    let core = trimmed.strip_prefix('!').unwrap_or(trimmed).trim_start();
    if core.is_empty() {
        return Err(AclError::InvalidToken(trimmed.to_string()));
    }

    if ACL_KEYWORDS.contains(&core) {
        return Ok(());
    }

    if let Some(key_name) = core.strip_prefix("key ") {
        if is_valid_key_name(key_name.trim()) {
            return Ok(());
        }
        return Err(AclError::InvalidToken(trimmed.to_string()));
    }

    if is_valid_ip_or_cidr(core) {
        return Ok(());
    }

    Err(AclError::InvalidToken(trimmed.to_string()))
}

/// Validate each entry in `entries` and return the `; `-joined payload that
/// goes between the `{ }` of an `allow-query` / `allow-transfer` block.
///
/// # Errors
/// Returns [`AclError`] on the first invalid entry; the index is encoded in
/// the message via the entry itself so operators can fix the offending CRD.
pub fn build_acl_list(entries: &[String]) -> Result<String, AclError> {
    let mut pieces = Vec::with_capacity(entries.len());
    for entry in entries {
        validate_acl_entry(entry)?;
        pieces.push(entry.trim().to_string());
    }
    Ok(pieces.join("; "))
}

fn is_valid_key_name(name: &str) -> bool {
    let unquoted = name
        .strip_prefix('"')
        .and_then(|n| n.strip_suffix('"'))
        .unwrap_or(name);
    if unquoted.is_empty() || unquoted.len() > MAX_KEY_NAME_LEN {
        return false;
    }
    unquoted
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn is_valid_ip_or_cidr(s: &str) -> bool {
    let Some((addr_str, prefix_str)) = s.split_once('/') else {
        return s.parse::<IpAddr>().is_ok();
    };
    let Ok(addr) = addr_str.parse::<IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix_str.parse::<u8>() else {
        return false;
    };
    let max = if addr.is_ipv4() {
        IPV4_MAX_PREFIX
    } else {
        IPV6_MAX_PREFIX
    };
    prefix <= max
}
