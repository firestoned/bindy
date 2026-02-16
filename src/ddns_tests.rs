// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `ddns.rs`

use super::*;
use crate::crd::ARecordSpec;

#[test]
fn test_calculate_record_hash_consistency() {
    // Same data should produce same hash
    let spec1 = ARecordSpec {
        name: "www".to_string(),
        ipv4_addresses: vec!["192.0.2.1".to_string()],
        ttl: Some(300),
    };
    let spec2 = ARecordSpec {
        name: "www".to_string(),
        ipv4_addresses: vec!["192.0.2.1".to_string()],
        ttl: Some(300),
    };

    let hash1 = calculate_record_hash(&spec1);
    let hash2 = calculate_record_hash(&spec2);

    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 64); // SHA-256 = 64 hex chars
}

#[test]
fn test_calculate_record_hash_changes() {
    // Different data should produce different hashes
    let spec1 = ARecordSpec {
        name: "www".to_string(),
        ipv4_addresses: vec!["192.0.2.1".to_string()],
        ttl: Some(300),
    };
    let spec2 = ARecordSpec {
        name: "www".to_string(),
        ipv4_addresses: vec!["192.0.2.2".to_string()], // Different IP
        ttl: Some(300),
    };

    let hash1 = calculate_record_hash(&spec1);
    let hash2 = calculate_record_hash(&spec2);

    assert_ne!(hash1, hash2);
}
