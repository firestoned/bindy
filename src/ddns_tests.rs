// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `ddns.rs`

use super::*;
use crate::crd::{
    AAAARecord, AAAARecordSpec, ARecord, ARecordSpec, CAARecord, CAARecordSpec, CNAMERecord,
    CNAMERecordSpec, MXRecord, MXRecordSpec, NSRecord, NSRecordSpec, SRVRecord, SRVRecordSpec,
    TXTRecord, TXTRecordSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

#[test]
fn test_calculate_record_hash_consistency() {
    // Same data should produce same hash
    let spec1 = ARecordSpec {
        name: "www".to_string(),
        ipv4_address: "192.0.2.1".to_string(),
        ttl: Some(300),
    };
    let spec2 = ARecordSpec {
        name: "www".to_string(),
        ipv4_address: "192.0.2.1".to_string(),
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
        ipv4_address: "192.0.2.1".to_string(),
        ttl: Some(300),
    };
    let spec2 = ARecordSpec {
        name: "www".to_string(),
        ipv4_address: "192.0.2.2".to_string(), // Different IP
        ttl: Some(300),
    };

    let hash1 = calculate_record_hash(&spec1);
    let hash2 = calculate_record_hash(&spec2);

    assert_ne!(hash1, hash2);
}

#[test]
fn test_generate_a_record_update() {
    let record = ARecord {
        metadata: ObjectMeta::default(),
        spec: ARecordSpec {
            name: "www".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: Some(300),
        },
        status: None,
    };

    let commands = generate_a_record_update(&record, "example.com.");

    assert!(commands.contains("update delete www.example.com. A"));
    assert!(commands.contains("update add www.example.com. 300 A 192.0.2.1"));
    assert!(commands.contains("send"));
}

#[test]
fn test_generate_a_record_update_default_ttl() {
    let record = ARecord {
        metadata: ObjectMeta::default(),
        spec: ARecordSpec {
            name: "www".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: None, // No TTL specified
        },
        status: None,
    };

    let commands = generate_a_record_update(&record, "example.com.");

    // Should use default TTL of 300
    assert!(commands.contains("update add www.example.com. 300 A 192.0.2.1"));
}

#[test]
fn test_generate_aaaa_record_update() {
    let record = AAAARecord {
        metadata: ObjectMeta::default(),
        spec: AAAARecordSpec {
            name: "www".to_string(),
            ipv6_address: "2001:db8::1".to_string(),
            ttl: Some(600),
        },
        status: None,
    };

    let commands = generate_aaaa_record_update(&record, "example.com.");

    assert!(commands.contains("update delete www.example.com. AAAA"));
    assert!(commands.contains("update add www.example.com. 600 AAAA 2001:db8::1"));
    assert!(commands.contains("send"));
}

#[test]
fn test_generate_cname_record_update() {
    let record = CNAMERecord {
        metadata: ObjectMeta::default(),
        spec: CNAMERecordSpec {
            name: "www".to_string(),
            target: "example.com.".to_string(),
            ttl: Some(300),
        },
        status: None,
    };

    let commands = generate_cname_record_update(&record, "example.com.");

    assert!(commands.contains("update delete www.example.com. CNAME"));
    assert!(commands.contains("update add www.example.com. 300 CNAME example.com."));
    assert!(commands.contains("send"));
}

#[test]
fn test_generate_mx_record_update() {
    let record = MXRecord {
        metadata: ObjectMeta::default(),
        spec: MXRecordSpec {
            name: "@".to_string(),
            priority: 10,
            mail_server: "mail.example.com.".to_string(),
            ttl: Some(300),
        },
        status: None,
    };

    let commands = generate_mx_record_update(&record, "example.com.");

    assert!(commands.contains("update delete @.example.com. MX"));
    assert!(commands.contains("update add @.example.com. 300 MX 10 mail.example.com."));
    assert!(commands.contains("send"));
}

#[test]
fn test_generate_ns_record_update() {
    let record = NSRecord {
        metadata: ObjectMeta::default(),
        spec: NSRecordSpec {
            name: "@".to_string(),
            nameserver: "ns1.example.com.".to_string(),
            ttl: Some(300),
        },
        status: None,
    };

    let commands = generate_ns_record_update(&record, "example.com.");

    assert!(commands.contains("update delete @.example.com. NS"));
    assert!(commands.contains("update add @.example.com. 300 NS ns1.example.com."));
    assert!(commands.contains("send"));
}

#[test]
fn test_generate_txt_record_update() {
    let record = TXTRecord {
        metadata: ObjectMeta::default(),
        spec: TXTRecordSpec {
            name: "_dmarc".to_string(),
            text: vec!["v=DMARC1; p=none".to_string()],
            ttl: Some(300),
        },
        status: None,
    };

    let commands = generate_txt_record_update(&record, "example.com.");

    assert!(commands.contains("update delete _dmarc.example.com. TXT"));
    assert!(commands.contains("update add _dmarc.example.com. 300 TXT \"v=DMARC1; p=none\""));
    assert!(commands.contains("send"));
}

#[test]
fn test_generate_srv_record_update() {
    let record = SRVRecord {
        metadata: ObjectMeta::default(),
        spec: SRVRecordSpec {
            name: "_http._tcp".to_string(),
            priority: 10,
            weight: 60,
            port: 80,
            target: "www.example.com.".to_string(),
            ttl: Some(300),
        },
        status: None,
    };

    let commands = generate_srv_record_update(&record, "example.com.");

    assert!(commands.contains("update delete _http._tcp.example.com. SRV"));
    assert!(
        commands.contains("update add _http._tcp.example.com. 300 SRV 10 60 80 www.example.com.")
    );
    assert!(commands.contains("send"));
}

#[test]
fn test_generate_caa_record_update() {
    let record = CAARecord {
        metadata: ObjectMeta::default(),
        spec: CAARecordSpec {
            name: "@".to_string(),
            flags: 0,
            tag: "issue".to_string(),
            value: "letsencrypt.org".to_string(),
            ttl: Some(300),
        },
        status: None,
    };

    let commands = generate_caa_record_update(&record, "example.com.");

    assert!(commands.contains("update delete @.example.com. CAA"));
    assert!(commands.contains("update add @.example.com. 300 CAA 0 issue \"letsencrypt.org\""));
    assert!(commands.contains("send"));
}
