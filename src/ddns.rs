// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#![allow(clippy::must_use_candidate)]

//! Dynamic DNS update utilities for BIND9 via RFC 2136.
//!
//! This module provides functionality for updating DNS records in BIND9 using:
//! - Direct DNS updates via hickory-client (TCP port 53, RFC 2136)
//! - Zone operations via bindcar HTTP API
//!
//! It includes:
//! - Hash calculation for change detection
//! - Direct DNS update functions using hickory-client
//! - Zone transfer triggering via bindcar API
//!
//! # Architecture
//!
//! Record reconcilers use this module to:
//! 1. Calculate hash of current record data
//! 2. Compare with last known hash in `status.record_hash`
//! 3. If hash changed, send RFC 2136 update to BIND9 via hickory-client
//! 4. Update sent directly to primary BIND9 instance (TCP port 53)
//! 5. BIND9 handles zone transfer to secondaries automatically
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy::ddns::{calculate_record_hash, generate_a_record_update};
//! use bindy::crd::{ARecord, ARecordSpec};
//! use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
//!
//! # async fn example() {
//! let record = ARecord {
//!     metadata: ObjectMeta::default(),
//!     spec: ARecordSpec {
//!         name: "www".to_string(),
//!         ipv4_address: "192.0.2.1".to_string(),
//!         ttl: Some(300),
//!     },
//!     status: None,
//! };
//!
//! // Calculate current hash
//! let current_hash = calculate_record_hash(&record.spec);
//!
//! // Check if changed (compare with status.record_hash)
//! if current_hash != record.status.as_ref().and_then(|s| s.record_hash.as_deref()).unwrap_or("") {
//!     // Generate nsupdate commands
//!     let commands = generate_a_record_update(&record, "example.com.");
//!     // Execute nsupdate...
//! }
//! # }
//! ```

use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, TXTRecord,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Calculate SHA-256 hash of a record's data fields.
///
/// This function serializes the record's spec to JSON and calculates a SHA-256
/// hash. The hash is used to detect actual data changes and avoid unnecessary
/// DNS updates.
///
/// # Arguments
///
/// * `record` - The record to hash (must implement Serialize)
///
/// # Returns
///
/// A hexadecimal string representation of the SHA-256 hash.
///
/// # Example
///
/// ```rust
/// use bindy::ddns::calculate_record_hash;
/// use bindy::crd::{ARecord, ARecordSpec};
/// use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
///
/// let record = ARecord {
///     metadata: ObjectMeta::default(),
///     spec: ARecordSpec {
///         name: "www".to_string(),
///         ipv4_address: "192.0.2.1".to_string(),
///         ttl: Some(300),
///     },
///     status: None,
/// };
///
/// let hash = calculate_record_hash(&record.spec);
/// assert_eq!(hash.len(), 64); // SHA-256 produces 64 hex chars
/// ```
pub fn calculate_record_hash<T: Serialize>(data: &T) -> String {
    let json = serde_json::to_string(data).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Generate nsupdate commands for an A record.
///
/// Creates DNS update commands in nsupdate format to delete existing records
/// and add the new record.
///
/// # Arguments
///
/// * `record` - The A record to update
/// * `zone_fqdn` - Fully qualified domain name of the zone (e.g., "example.com.")
///
/// # Returns
///
/// A string containing nsupdate commands.
///
/// # Example
///
/// ```rust
/// use bindy::ddns::generate_a_record_update;
/// use bindy::crd::{ARecord, ARecordSpec};
/// use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
///
/// let record = ARecord {
///     metadata: ObjectMeta::default(),
///     spec: ARecordSpec {
///         name: "www".to_string(),
///         ipv4_address: "192.0.2.1".to_string(),
///         ttl: Some(300),
///     },
///     status: None,
/// };
///
/// let commands = generate_a_record_update(&record, "example.com.");
/// assert!(commands.contains("update delete www.example.com. A"));
/// assert!(commands.contains("update add www.example.com. 300 A 192.0.2.1"));
/// ```
pub fn generate_a_record_update(record: &ARecord, zone_fqdn: &str) -> String {
    let fqdn = format!("{}.{}", record.spec.name, zone_fqdn);
    let ttl = record.spec.ttl.unwrap_or(300);

    format!(
        "update delete {fqdn} A\nupdate add {fqdn} {ttl} A {}\nsend\n",
        record.spec.ipv4_address
    )
}

/// Generate nsupdate commands for an AAAA record.
pub fn generate_aaaa_record_update(record: &AAAARecord, zone_fqdn: &str) -> String {
    let fqdn = format!("{}.{}", record.spec.name, zone_fqdn);
    let ttl = record.spec.ttl.unwrap_or(300);

    format!(
        "update delete {fqdn} AAAA\nupdate add {fqdn} {ttl} AAAA {}\nsend\n",
        record.spec.ipv6_address
    )
}

/// Generate nsupdate commands for a CNAME record.
pub fn generate_cname_record_update(record: &CNAMERecord, zone_fqdn: &str) -> String {
    let fqdn = format!("{}.{}", record.spec.name, zone_fqdn);
    let ttl = record.spec.ttl.unwrap_or(300);
    let target = &record.spec.target;

    format!("update delete {fqdn} CNAME\nupdate add {fqdn} {ttl} CNAME {target}\nsend\n")
}

/// Generate nsupdate commands for an MX record.
pub fn generate_mx_record_update(record: &MXRecord, zone_fqdn: &str) -> String {
    let fqdn = format!("{}.{}", record.spec.name, zone_fqdn);
    let ttl = record.spec.ttl.unwrap_or(300);
    let priority = record.spec.priority;
    let mail_server = &record.spec.mail_server;

    format!("update delete {fqdn} MX\nupdate add {fqdn} {ttl} MX {priority} {mail_server}\nsend\n")
}

/// Generate nsupdate commands for an NS record.
pub fn generate_ns_record_update(record: &NSRecord, zone_fqdn: &str) -> String {
    let fqdn = format!("{}.{}", record.spec.name, zone_fqdn);
    let ttl = record.spec.ttl.unwrap_or(300);
    let nameserver = &record.spec.nameserver;

    format!("update delete {fqdn} NS\nupdate add {fqdn} {ttl} NS {nameserver}\nsend\n")
}

/// Generate nsupdate commands for a TXT record.
pub fn generate_txt_record_update(record: &TXTRecord, zone_fqdn: &str) -> String {
    let fqdn = format!("{}.{}", record.spec.name, zone_fqdn);
    let ttl = record.spec.ttl.unwrap_or(300);

    // TXT records can have multiple strings - each needs to be quoted
    let text_values: Vec<String> = record
        .spec
        .text
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect();
    let text = text_values.join(" ");

    format!("update delete {fqdn} TXT\nupdate add {fqdn} {ttl} TXT {text}\nsend\n")
}

/// Generate nsupdate commands for an SRV record.
pub fn generate_srv_record_update(record: &SRVRecord, zone_fqdn: &str) -> String {
    let fqdn = format!("{}.{}", record.spec.name, zone_fqdn);
    let ttl = record.spec.ttl.unwrap_or(300);
    let priority = record.spec.priority;
    let weight = record.spec.weight;
    let port = record.spec.port;
    let target = &record.spec.target;

    format!(
        "update delete {fqdn} SRV\nupdate add {fqdn} {ttl} SRV {priority} {weight} {port} {target}\nsend\n"
    )
}

/// Generate nsupdate commands for a CAA record.
pub fn generate_caa_record_update(record: &CAARecord, zone_fqdn: &str) -> String {
    let fqdn = format!("{}.{}", record.spec.name, zone_fqdn);
    let ttl = record.spec.ttl.unwrap_or(300);
    let flags = record.spec.flags;
    let tag = &record.spec.tag;
    let value = &record.spec.value;

    // CAA format: flags tag "value"
    format!(
        "update delete {fqdn} CAA\nupdate add {fqdn} {ttl} CAA {flags} {tag} \"{value}\"\nsend\n"
    )
}

#[cfg(test)]
#[path = "ddns_tests.rs"]
mod ddns_tests;
