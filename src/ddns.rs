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
//! ```rust
//! use bindy::ddns::calculate_record_hash;
//! use bindy::crd::ARecordSpec;
//!
//! # fn main() {
//! let spec = ARecordSpec {
//!     name: "www".to_string(),
//!     ipv4_address: "192.0.2.1".to_string(),
//!     ttl: Some(300),
//! };
//!
//! // Calculate current hash
//! let current_hash = calculate_record_hash(&spec);
//! // hash is a 64-character hex string
//! assert_eq!(current_hash.len(), 64);
//! # }
//! ```

// Record types are used in tests and by the calculate_record_hash function via Serialize trait
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
#[cfg(test)]
#[path = "ddns_tests.rs"]
mod ddns_tests;
