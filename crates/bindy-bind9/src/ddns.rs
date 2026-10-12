// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Record hashing for change detection.
//!
//! Record reconcilers hash a record's spec with [`calculate_record_hash`] and
//! compare it with the last known hash in `status.record_hash`; only a changed
//! hash leads to an RFC 2136 update (sent by [`crate::bind9::records`]).
//!
//! # Example
//!
//! ```rust
//! use bindy_bind9::ddns::calculate_record_hash;
//! use bindy_api::crd::ARecordSpec;
//!
//! # fn main() {
//! let spec = ARecordSpec {
//!     name: "www".to_string(),
//!     ipv4_addresses: vec!["192.0.2.1".to_string()],
//!     ttl: Some(300),
//! };
//!
//! // Calculate current hash
//! let current_hash = calculate_record_hash(&spec);
//! // hash is a 64-character hex string
//! assert_eq!(current_hash.len(), 64);
//! # }
//! ```

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
/// use bindy_bind9::ddns::calculate_record_hash;
/// use bindy_api::crd::{ARecord, ARecordSpec};
/// use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
///
/// let record = ARecord {
///     metadata: ObjectMeta::default(),
///     spec: ARecordSpec {
///         name: "www".to_string(),
///         ipv4_addresses: vec!["192.0.2.1".to_string()],
///         ttl: Some(300),
///     },
///     status: None,
/// };
///
/// let hash = calculate_record_hash(&record.spec);
/// assert_eq!(hash.len(), 64); // SHA-256 produces 64 hex chars
/// ```
#[must_use]
pub fn calculate_record_hash<T: Serialize>(data: &T) -> String {
    let json = serde_json::to_string(data).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    // sha2 0.11 returns `Array<u8, _>` from hybrid-array, which does not
    // implement `LowerHex` like the old `GenericArray<u8, U32>` did.
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

#[cfg(test)]
#[path = "ddns_tests.rs"]
mod ddns_tests;
