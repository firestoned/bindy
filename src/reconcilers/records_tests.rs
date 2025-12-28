// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for DNS record reconcilers
//!
//! These tests verify the logic and data structures used by record reconcilers.

#[cfg(test)]
mod tests {
    use crate::bind9::SRVRecordData;
    use crate::crd::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    // ========================================================================
    // Data Structure Tests - A Records
    // ========================================================================

    #[test]
    fn test_arecord_spec_creation() {
        let spec = ARecordSpec {
            name: "www".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: Some(300),
        };

        assert_eq!(spec.name, "www");
        assert_eq!(spec.ipv4_address, "192.0.2.1");
        assert_eq!(spec.ttl, Some(300));
    }

    #[test]
    fn test_arecord_without_ttl() {
        let spec = ARecordSpec {
            name: "mail".to_string(),
            ipv4_address: "192.0.2.2".to_string(),
            ttl: None,
        };

        assert_eq!(spec.ttl, None);
    }

    #[test]
    fn test_arecord_ipv4_validation() {
        // Valid IPv4 addresses
        let valid_ips = vec![
            "0.0.0.0",
            "127.0.0.1",
            "192.168.1.1",
            "10.0.0.1",
            "172.16.0.1",
            "255.255.255.255",
        ];

        for ip in valid_ips {
            let spec = ARecordSpec {
                name: "test".to_string(),
                ipv4_address: ip.to_string(),
                ttl: None,
            };

            assert_eq!(spec.ipv4_address, ip);
        }
    }

    #[test]
    fn test_arecord_name() {
        // Test that name field works correctly
        let spec = ARecordSpec {
            name: "www".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: None,
        };

        assert_eq!(spec.name, "www");
    }

    // ========================================================================
    // Data Structure Tests - AAAA Records
    // ========================================================================

    #[test]
    fn test_aaaa_record_spec_creation() {
        let spec = AAAARecordSpec {
            name: "www".to_string(),
            ipv6_address: "2001:db8::1".to_string(),
            ttl: Some(300),
        };

        assert_eq!(spec.ipv6_address, "2001:db8::1");
        assert_eq!(spec.ttl, Some(300));
    }

    #[test]
    fn test_aaaa_record_various_ipv6_formats() {
        let ipv6_addresses = vec![
            "2001:db8::1",
            "2001:0db8:0000:0000:0000:0000:0000:0001",
            "::1",
            "::",
            "fe80::1",
            "::ffff:192.0.2.1", // IPv4-mapped IPv6
        ];

        for ipv6 in ipv6_addresses {
            let spec = AAAARecordSpec {
                name: "test".to_string(),
                ipv6_address: ipv6.to_string(),
                ttl: None,
            };

            assert_eq!(spec.ipv6_address, ipv6);
        }
    }

    // ========================================================================
    // Data Structure Tests - CNAME Records
    // ========================================================================

    #[test]
    fn test_cname_record_spec_creation() {
        let spec = CNAMERecordSpec {
            name: "blog".to_string(),
            target: "www.example.com.".to_string(),
            ttl: Some(600),
        };

        assert_eq!(spec.target, "www.example.com.");
        assert_eq!(spec.ttl, Some(600));
    }

    #[test]
    fn test_cname_fqdn_requirement() {
        let spec = CNAMERecordSpec {
            name: "alias".to_string(),
            target: "target.example.com.".to_string(),
            ttl: None,
        };

        // CNAME targets should end with a dot for FQDN
        assert!(spec.target.ends_with('.'));
    }

    // ========================================================================
    // Data Structure Tests - MX Records
    // ========================================================================

    #[test]
    fn test_mx_record_spec_creation() {
        let spec = MXRecordSpec {
            name: "@".to_string(),
            priority: 10,
            mail_server: "mail.example.com.".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(spec.priority, 10);
        assert_eq!(spec.mail_server, "mail.example.com.");
        assert_eq!(spec.ttl, Some(3600));
    }

    #[test]
    fn test_mx_record_priority_values() {
        let priorities = vec![0, 5, 10, 20, 50, 100];

        for priority in priorities {
            let spec = MXRecordSpec {
                name: "@".to_string(),
                priority,
                mail_server: format!("mx{priority}.example.com."),
                ttl: None,
            };

            assert_eq!(spec.priority, priority);
        }
    }

    #[test]
    fn test_mx_record_multiple_servers() {
        let mx_records = vec![
            (10, "mx1.example.com."),
            (20, "mx2.example.com."),
            (30, "mx3.example.com."),
        ];

        for (priority, server) in mx_records {
            let spec = MXRecordSpec {
                name: "@".to_string(),
                priority,
                mail_server: server.to_string(),
                ttl: Some(3600),
            };

            assert_eq!(spec.priority, priority);
            assert_eq!(spec.mail_server, server);
        }
    }

    // ========================================================================
    // Data Structure Tests - TXT Records
    // ========================================================================

    #[test]
    fn test_txt_record_spec_single_string() {
        let spec = TXTRecordSpec {
            name: "@".to_string(),
            text: vec!["v=spf1 mx ~all".to_string()],
            ttl: Some(3600),
        };

        assert_eq!(spec.text.len(), 1);
        assert_eq!(spec.text[0], "v=spf1 mx ~all");
    }

    #[test]
    fn test_txt_record_spec_multiple_strings() {
        let spec = TXTRecordSpec {
            name: "_dmarc".to_string(),
            text: vec!["v=DMARC1".to_string(), "p=reject".to_string()],
            ttl: None,
        };

        assert_eq!(spec.text.len(), 2);
        assert!(spec.text.contains(&"v=DMARC1".to_string()));
        assert!(spec.text.contains(&"p=reject".to_string()));
    }

    #[test]
    fn test_txt_record_empty_strings() {
        let spec = TXTRecordSpec {
            name: "test".to_string(),
            text: vec![],
            ttl: None,
        };

        assert_eq!(spec.text.len(), 0);
    }

    #[test]
    fn test_txt_record_long_string() {
        let long_value = "v".repeat(255); // 255 characters is max for a single TXT string
        let spec = TXTRecordSpec {
            name: "test".to_string(),
            text: vec![long_value.clone()],
            ttl: None,
        };

        assert_eq!(spec.text[0].len(), 255);
    }

    #[test]
    fn test_txt_record_common_use_cases() {
        // SPF
        let spf = TXTRecordSpec {
            name: "@".to_string(),
            text: vec!["v=spf1 include:_spf.google.com ~all".to_string()],
            ttl: None,
        };
        assert!(spf.text[0].starts_with("v=spf1"));

        // DKIM
        let dkim = TXTRecordSpec {
            name: "default._domainkey".to_string(),
            text: vec!["v=DKIM1; k=rsa; p=MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQ...".to_string()],
            ttl: None,
        };
        assert!(dkim.text[0].starts_with("v=DKIM1"));

        // DMARC
        let dmarc = TXTRecordSpec {
            name: "_dmarc".to_string(),
            text: vec!["v=DMARC1; p=quarantine; rua=mailto:dmarc@example.com".to_string()],
            ttl: None,
        };
        assert!(dmarc.text[0].starts_with("v=DMARC1"));

        // Domain verification
        let verification = TXTRecordSpec {
            name: "@".to_string(),
            text: vec!["google-site-verification=abc123def456".to_string()],
            ttl: None,
        };
        assert!(verification.text[0].contains("verification"));
    }

    // ========================================================================
    // Data Structure Tests - NS Records
    // ========================================================================

    #[test]
    fn test_ns_record_spec_creation() {
        let spec = NSRecordSpec {
            name: "@".to_string(),
            nameserver: "ns1.example.com.".to_string(),
            ttl: Some(86400),
        };

        assert_eq!(spec.nameserver, "ns1.example.com.");
        assert_eq!(spec.ttl, Some(86400));
    }

    #[test]
    fn test_ns_record_delegation() {
        let spec = NSRecordSpec {
            name: "subdomain".to_string(),
            nameserver: "ns1.subdomain.example.com.".to_string(),
            ttl: Some(86400),
        };

        assert_eq!(spec.name, "subdomain");
        assert!(spec.nameserver.contains("subdomain"));
    }

    // ========================================================================
    // Data Structure Tests - SRV Records
    // ========================================================================

    #[test]
    fn test_srv_record_spec_creation() {
        let spec = SRVRecordSpec {
            name: "_sip._tcp".to_string(),
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sipserver.example.com.".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(spec.priority, 10);
        assert_eq!(spec.weight, 60);
        assert_eq!(spec.port, 5060);
        assert_eq!(spec.target, "sipserver.example.com.");
    }

    #[test]
    fn test_srv_record_data_from_spec() {
        let spec = SRVRecordSpec {
            name: "_ldap._tcp".to_string(),
            priority: 0,
            weight: 100,
            port: 389,
            target: "ldap.example.com.".to_string(),
            ttl: Some(7200),
        };

        let srv_data = SRVRecordData {
            priority: spec.priority,
            weight: spec.weight,
            port: spec.port,
            target: spec.target.clone(),
            ttl: spec.ttl,
        };

        assert_eq!(srv_data.priority, 0);
        assert_eq!(srv_data.weight, 100);
        assert_eq!(srv_data.port, 389);
        assert_eq!(srv_data.target, "ldap.example.com.");
        assert_eq!(srv_data.ttl, Some(7200));
    }

    #[test]
    fn test_srv_record_service_names() {
        let services = vec![
            "_sip._tcp",
            "_ldap._tcp",
            "_kerberos._tcp",
            "_http._tcp",
            "_xmpp-server._tcp",
        ];

        for service in services {
            let spec = SRVRecordSpec {
                name: service.to_string(),
                priority: 10,
                weight: 50,
                port: 5060,
                target: "server.example.com.".to_string(),
                ttl: None,
            };

            assert_eq!(spec.name, service);
        }
    }

    #[test]
    fn test_srv_record_zero_priority_weight() {
        let spec = SRVRecordSpec {
            name: "_service._tcp".to_string(),
            priority: 0,
            weight: 0,
            port: 443,
            target: "server.example.com.".to_string(),
            ttl: None,
        };

        assert_eq!(spec.priority, 0);
        assert_eq!(spec.weight, 0);
    }

    #[test]
    fn test_srv_record_data_clone() {
        let srv_data1 = SRVRecordData {
            priority: 10,
            weight: 50,
            port: 5060,
            target: "server.example.com.".to_string(),
            ttl: Some(3600),
        };

        // Test that Clone trait is implemented correctly
        let srv_data2 = srv_data1.clone();

        assert_eq!(srv_data1.priority, srv_data2.priority);
        assert_eq!(srv_data1.weight, srv_data2.weight);
        assert_eq!(srv_data1.port, srv_data2.port);
        assert_eq!(srv_data1.target, srv_data2.target);
        assert_eq!(srv_data1.ttl, srv_data2.ttl);
    }

    // ========================================================================
    // Data Structure Tests - CAA Records
    // ========================================================================

    #[test]
    fn test_caa_record_spec_issue() {
        let spec = CAARecordSpec {
            name: "@".to_string(),
            flags: 0,
            tag: "issue".to_string(),
            value: "letsencrypt.org".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(spec.flags, 0);
        assert_eq!(spec.tag, "issue");
        assert_eq!(spec.value, "letsencrypt.org");
    }

    #[test]
    fn test_caa_record_spec_issuewild() {
        let spec = CAARecordSpec {
            name: "@".to_string(),
            flags: 0,
            tag: "issuewild".to_string(),
            value: "letsencrypt.org".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(spec.tag, "issuewild");
    }

    #[test]
    fn test_caa_record_spec_iodef() {
        let spec = CAARecordSpec {
            name: "@".to_string(),
            flags: 128,
            tag: "iodef".to_string(),
            value: "mailto:security@example.com".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(spec.flags, 128);
        assert_eq!(spec.tag, "iodef");
        assert!(spec.value.starts_with("mailto:"));
    }

    #[test]
    fn test_caa_record_critical_flag() {
        let spec = CAARecordSpec {
            name: "@".to_string(),
            flags: 128, // Critical flag
            tag: "issue".to_string(),
            value: "ca.example.com".to_string(),
            ttl: None,
        };

        // 128 indicates critical flag
        assert_eq!(spec.flags, 128);
    }

    #[test]
    fn test_caa_record_all_tags() {
        let tags = vec!["issue", "issuewild", "iodef"];

        for tag in tags {
            let spec = CAARecordSpec {
                name: "@".to_string(),
                flags: 0,
                tag: tag.to_string(),
                value: "example.org".to_string(),
                ttl: None,
            };

            assert_eq!(spec.tag, tag);
        }
    }

    // ========================================================================
    // Record Status Tests
    // ========================================================================

    #[test]
    fn test_record_status_default() {
        let status = RecordStatus::default();

        assert!(status.conditions.is_empty());
        assert_eq!(status.observed_generation, None);
    }

    #[test]
    fn test_record_status_with_condition() {
        let condition = Condition {
            r#type: "Ready".to_string(),
            status: "True".to_string(),
            reason: Some("RecordCreated".to_string()),
            message: Some("A record created successfully".to_string()),
            last_transition_time: Some("2024-11-27T00:00:00Z".to_string()),
        };

        let status = RecordStatus {
            conditions: vec![condition],
            observed_generation: Some(1),
            zone: None,
            record_hash: None,
            last_updated: None,
        };

        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.conditions[0].r#type, "Ready");
        assert_eq!(status.observed_generation, Some(1));
    }

    #[test]
    fn test_record_status_multiple_conditions() {
        let conditions = vec![
            Condition {
                r#type: "Ready".to_string(),
                status: "True".to_string(),
                reason: Some("RecordCreated".to_string()),
                message: Some("Record created".to_string()),
                last_transition_time: Some("2024-11-27T00:00:00Z".to_string()),
            },
            Condition {
                r#type: "Synced".to_string(),
                status: "True".to_string(),
                reason: Some("SyncSuccessful".to_string()),
                message: Some("Record synced to all endpoints".to_string()),
                last_transition_time: Some("2024-11-27T00:00:01Z".to_string()),
            },
        ];

        let status = RecordStatus {
            conditions,
            observed_generation: Some(2),
            zone: None,
            record_hash: None,
            last_updated: None,
        };

        assert_eq!(status.conditions.len(), 2);
        assert_eq!(status.conditions[0].r#type, "Ready");
        assert_eq!(status.conditions[1].r#type, "Synced");
    }

    #[test]
    fn test_record_status_failure_condition() {
        let condition = Condition {
            r#type: "Ready".to_string(),
            status: "False".to_string(),
            reason: Some("ReconcileFailed".to_string()),
            message: Some("Failed to connect to BIND9 server".to_string()),
            last_transition_time: Some("2024-11-27T00:00:00Z".to_string()),
        };

        let status = RecordStatus {
            conditions: vec![condition],
            observed_generation: Some(1),
            zone: None,
            record_hash: None,
            last_updated: None,
        };

        assert_eq!(status.conditions[0].status, "False");
        assert_eq!(
            status.conditions[0].reason.as_deref(),
            Some("ReconcileFailed")
        );
    }

    // ========================================================================
    // Metadata and Resource Tests
    // ========================================================================

    #[test]
    fn test_record_with_metadata() {
        let record = ARecord {
            metadata: ObjectMeta {
                name: Some("www-example-com".to_string()),
                namespace: Some("default".to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: ARecordSpec {
                name: "www".to_string(),
                ipv4_address: "192.0.2.1".to_string(),
                ttl: Some(300),
            },
            status: None,
        };

        assert_eq!(record.metadata.name.as_deref(), Some("www-example-com"));
        assert_eq!(record.metadata.namespace.as_deref(), Some("default"));
        assert_eq!(record.metadata.generation, Some(1));
    }

    #[test]
    fn test_record_with_labels() {
        use std::collections::BTreeMap;

        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "bindy".to_string());
        labels.insert("environment".to_string(), "production".to_string());

        let record = ARecord {
            metadata: ObjectMeta {
                name: Some("www-example-com".to_string()),
                namespace: Some("default".to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: ARecordSpec {
                name: "www".to_string(),
                ipv4_address: "192.0.2.1".to_string(),
                ttl: None,
            },
            status: None,
        };

        assert_eq!(record.metadata.labels, Some(labels));
    }

    #[test]
    fn test_record_with_annotations() {
        use std::collections::BTreeMap;

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/cluster".to_string(),
            "cluster-1".to_string(),
        );
        annotations.insert(
            "bindy.firestoned.io/zone".to_string(),
            "example.com".to_string(),
        );

        let record = ARecord {
            metadata: ObjectMeta {
                name: Some("www-example-com".to_string()),
                namespace: Some("default".to_string()),
                annotations: Some(annotations.clone()),
                ..Default::default()
            },
            spec: ARecordSpec {
                name: "www".to_string(),
                ipv4_address: "192.0.2.1".to_string(),
                ttl: None,
            },
            status: None,
        };

        assert_eq!(record.metadata.annotations, Some(annotations));
    }

    // ========================================================================
    // Common Pattern Tests
    // ========================================================================

    #[test]
    fn test_various_ttl_values() {
        let ttl_values = vec![None, Some(60), Some(300), Some(3600), Some(86400)];

        for ttl in ttl_values {
            let spec = ARecordSpec {
                name: "test".to_string(),
                ipv4_address: "192.0.2.1".to_string(),
                ttl,
            };

            assert_eq!(spec.ttl, ttl);
        }
    }

    #[test]
    fn test_apex_vs_subdomain_naming() {
        let apex_record = ARecordSpec {
            name: "@".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: None,
        };

        let subdomain_record = ARecordSpec {
            name: "www".to_string(),
            ipv4_address: "192.0.2.2".to_string(),
            ttl: None,
        };

        assert_eq!(apex_record.name, "@");
        assert_eq!(subdomain_record.name, "www");
    }

    #[test]
    fn test_all_record_types_with_same_name() {
        // Test that different record types can coexist with the same name
        let name = "test".to_string();
        let a_spec = ARecordSpec {
            name: name.clone(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: None,
        };

        let aaaa_spec = AAAARecordSpec {
            name: name.clone(),
            ipv6_address: "2001:db8::1".to_string(),
            ttl: None,
        };

        let txt_spec = TXTRecordSpec {
            name: name.clone(),
            text: vec!["verification=abc123".to_string()],
            ttl: None,
        };

        assert_eq!(a_spec.name, aaaa_spec.name);
        assert_eq!(a_spec.name, txt_spec.name);
    }

    // ========================================================================
    // Edge Case Tests
    // ========================================================================

    #[test]
    fn test_empty_record_name() {
        // Empty name is allowed by the type system but would fail at runtime
        let spec = ARecordSpec {
            name: "".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: None,
        };

        assert_eq!(spec.name, "");
    }

    #[test]
    fn test_very_long_record_name() {
        let long_name = "a".repeat(63); // Maximum label length in DNS
        let spec = ARecordSpec {
            name: long_name.clone(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: None,
        };

        assert_eq!(spec.name.len(), 63);
    }

    #[test]
    fn test_wildcard_record_name() {
        let spec = ARecordSpec {
            name: "*".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: None,
        };

        assert_eq!(spec.name, "*");
    }

    #[test]
    fn test_subdomain_levels() {
        let names = vec!["www", "www.sub", "www.sub.deep", "www.sub.deep.deeper"];

        for name in names {
            let spec = ARecordSpec {
                name: name.to_string(),
                ipv4_address: "192.0.2.1".to_string(),
                ttl: None,
            };

            assert_eq!(spec.name, name);
        }
    }

    #[test]
    fn test_mx_record_priority_edge_cases() {
        // Test minimum and maximum priority values
        let priorities = vec![0, i32::MAX];

        for priority in priorities {
            let spec = MXRecordSpec {
                name: "@".to_string(),
                priority,
                mail_server: "mail.example.com.".to_string(),
                ttl: None,
            };

            assert_eq!(spec.priority, priority);
        }
    }

    #[test]
    fn test_srv_record_port_range() {
        // Test various port numbers
        let ports = vec![0, 80, 443, 5060, 8080, 65535];

        for port in ports {
            let spec = SRVRecordSpec {
                name: "_service._tcp".to_string(),
                priority: 10,
                weight: 50,
                port,
                target: "server.example.com.".to_string(),
                ttl: None,
            };

            assert_eq!(spec.port, port);
        }
    }

    #[test]
    fn test_caa_record_flag_values() {
        // Test various flag values
        let flags = vec![0, 1, 128, 255];

        for flag in flags {
            let spec = CAARecordSpec {
                name: "@".to_string(),
                flags: flag,
                tag: "issue".to_string(),
                value: "ca.example.com".to_string(),
                ttl: None,
            };

            assert_eq!(spec.flags, flag);
        }
    }

    #[test]
    fn test_txt_record_special_characters() {
        let special_texts = vec![
            "contains space",
            "contains\"quotes\"",
            "contains;semicolon",
            "contains=equals",
            "contains\ttab",
        ];

        for text in special_texts {
            let spec = TXTRecordSpec {
                name: "test".to_string(),
                text: vec![text.to_string()],
                ttl: None,
            };

            assert_eq!(spec.text[0], text);
        }
    }

    // ========================================================================
    // Negative Tests (Invalid Data - Should Still Parse)
    // ========================================================================

    #[test]
    fn test_invalid_ipv4_format_still_parses() {
        // The struct will accept invalid IPs - validation happens at reconciliation time
        let spec = ARecordSpec {
            name: "test".to_string(),
            ipv4_address: "not-an-ip".to_string(),
            ttl: None,
        };

        assert_eq!(spec.ipv4_address, "not-an-ip");
    }

    #[test]
    fn test_invalid_ipv6_format_still_parses() {
        let spec = AAAARecordSpec {
            name: "test".to_string(),
            ipv6_address: "not-an-ipv6".to_string(),
            ttl: None,
        };

        assert_eq!(spec.ipv6_address, "not-an-ipv6");
    }

    #[test]
    fn test_cname_without_trailing_dot() {
        // CNAME targets should have trailing dot, but struct accepts without
        let spec = CNAMERecordSpec {
            name: "alias".to_string(),
            target: "target.example.com".to_string(), // Missing trailing dot
            ttl: None,
        };

        assert!(!spec.target.ends_with('.'));
    }

    #[test]
    fn test_negative_ttl_value() {
        // Negative TTL is technically invalid, but struct accepts it
        let spec = ARecordSpec {
            name: "test".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: Some(-1),
        };

        assert_eq!(spec.ttl, Some(-1));
    }

    // ========================================================================
    // Status Records Feature Tests
    // ========================================================================

    mod status_records_tests {
        use crate::constants::{
            API_GROUP_VERSION, KIND_AAAA_RECORD, KIND_A_RECORD, KIND_CAA_RECORD, KIND_CNAME_RECORD,
            KIND_MX_RECORD, KIND_NS_RECORD, KIND_SRV_RECORD, KIND_TXT_RECORD,
        };
        use crate::crd::{DNSZoneStatus, RecordReference};
        use serde_json::json;

        // ====================================================================
        // RecordReference Struct Tests
        // ====================================================================

        #[test]
        fn test_record_reference_creation() {
            let record_ref = RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_A_RECORD.to_string(),
                name: "test-a-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            assert_eq!(record_ref.api_version, "bindy.firestoned.io/v1beta1");
            assert_eq!(record_ref.kind, "ARecord");
            assert_eq!(record_ref.name, "test-a-record");
        }

        #[test]
        fn test_record_reference_equality() {
            let ref1 = RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_A_RECORD.to_string(),
                name: "test-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            let ref2 = RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_A_RECORD.to_string(),
                name: "test-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            let ref3 = RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_AAAA_RECORD.to_string(),
                name: "test-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            assert_eq!(ref1, ref2);
            assert_ne!(ref1, ref3);
        }

        #[test]
        fn test_record_reference_serialization() {
            let record_ref = RecordReference {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "ARecord".to_string(),
                name: "test-a-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            let json = serde_json::to_value(&record_ref).unwrap();
            assert_eq!(json["apiVersion"], "bindy.firestoned.io/v1beta1");
            assert_eq!(json["kind"], "ARecord");
            assert_eq!(json["name"], "test-a-record");
        }

        #[test]
        fn test_record_reference_deserialization() {
            let json = json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "CNAMERecord",
                "name": "test-cname-record",
                "namespace": "dns-system"
            });

            let record_ref: RecordReference = serde_json::from_value(json).unwrap();
            assert_eq!(record_ref.api_version, "bindy.firestoned.io/v1beta1");
            assert_eq!(record_ref.kind, "CNAMERecord");
            assert_eq!(record_ref.name, "test-cname-record");
        }

        // ====================================================================
        // DNSZoneStatus.records Field Tests
        // ====================================================================

        #[test]
        fn test_dns_zone_status_with_empty_records() {
            let status = DNSZoneStatus {
                conditions: vec![],
                observed_generation: None,
                record_count: None,
                secondary_ips: None,
                records: vec![],
            };

            assert!(status.records.is_empty());
        }

        #[test]
        fn test_dns_zone_status_with_multiple_records() {
            let records = vec![
                RecordReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: KIND_A_RECORD.to_string(),
                    name: "web-a-record".to_string(),
                    namespace: "dns-system".to_string(),
                },
                RecordReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: KIND_AAAA_RECORD.to_string(),
                    name: "web-aaaa-record".to_string(),
                    namespace: "dns-system".to_string(),
                },
                RecordReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: KIND_CNAME_RECORD.to_string(),
                    name: "www-cname-record".to_string(),
                    namespace: "dns-system".to_string(),
                },
            ];

            let status = DNSZoneStatus {
                conditions: vec![],
                observed_generation: None,
                record_count: None,
                secondary_ips: None,
                records: records.clone(),
            };

            assert_eq!(status.records.len(), 3);
            assert_eq!(status.records[0].kind, "ARecord");
            assert_eq!(status.records[1].kind, "AAAARecord");
            assert_eq!(status.records[2].kind, "CNAMERecord");
        }

        #[test]
        fn test_dns_zone_status_serialization_skips_empty_records() {
            let status = DNSZoneStatus {
                conditions: vec![],
                observed_generation: None,
                record_count: None,
                secondary_ips: None,
                records: vec![],
            };

            let json = serde_json::to_value(&status).unwrap();
            // records field should be omitted when empty due to skip_serializing_if
            assert!(!json.as_object().unwrap().contains_key("records"));
        }

        #[test]
        fn test_dns_zone_status_serialization_includes_non_empty_records() {
            let records = vec![RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_A_RECORD.to_string(),
                name: "test-a-record".to_string(),
                namespace: "dns-system".to_string(),
            }];

            let status = DNSZoneStatus {
                conditions: vec![],
                observed_generation: None,
                record_count: None,
                secondary_ips: None,
                records: records.clone(),
            };

            let json = serde_json::to_value(&status).unwrap();
            // records field should be present when non-empty
            assert!(json.as_object().unwrap().contains_key("records"));
            assert_eq!(json["records"].as_array().unwrap().len(), 1);
        }

        // ====================================================================
        // All Record Types Constant Tests
        // ====================================================================

        #[test]
        fn test_all_record_kind_constants() {
            let record_kinds = vec![
                (KIND_A_RECORD, "ARecord"),
                (KIND_AAAA_RECORD, "AAAARecord"),
                (KIND_TXT_RECORD, "TXTRecord"),
                (KIND_CNAME_RECORD, "CNAMERecord"),
                (KIND_MX_RECORD, "MXRecord"),
                (KIND_NS_RECORD, "NSRecord"),
                (KIND_SRV_RECORD, "SRVRecord"),
                (KIND_CAA_RECORD, "CAARecord"),
            ];

            for (constant, expected) in record_kinds {
                assert_eq!(constant, expected);
            }
        }

        #[test]
        fn test_api_group_version_constant() {
            assert_eq!(API_GROUP_VERSION, "bindy.firestoned.io/v1beta1");
        }

        // ====================================================================
        // Record Reference Creation for Each Record Type
        // ====================================================================

        #[test]
        fn test_create_record_reference_for_all_types() {
            let test_cases = vec![
                (KIND_A_RECORD, "test-a-record"),
                (KIND_AAAA_RECORD, "test-aaaa-record"),
                (KIND_TXT_RECORD, "test-txt-record"),
                (KIND_CNAME_RECORD, "test-cname-record"),
                (KIND_MX_RECORD, "test-mx-record"),
                (KIND_NS_RECORD, "test-ns-record"),
                (KIND_SRV_RECORD, "test-srv-record"),
                (KIND_CAA_RECORD, "test-caa-record"),
            ];

            for (kind, name) in test_cases {
                let record_ref = RecordReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: kind.to_string(),
                    name: name.to_string(),
                    namespace: "dns-system".to_string(),
                };

                assert_eq!(record_ref.api_version, "bindy.firestoned.io/v1beta1");
                assert_eq!(record_ref.kind, kind);
                assert_eq!(record_ref.name, name);
            }
        }

        // ====================================================================
        // Duplicate Record Detection Tests
        // ====================================================================

        #[test]
        fn test_duplicate_record_detection() {
            let records = [
                RecordReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: KIND_A_RECORD.to_string(),
                    name: "web-a-record".to_string(),
                    namespace: "dns-system".to_string(),
                },
                RecordReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: KIND_AAAA_RECORD.to_string(),
                    name: "web-aaaa-record".to_string(),
                    namespace: "dns-system".to_string(),
                },
            ];

            let new_record = RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_A_RECORD.to_string(),
                name: "web-a-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            // Check if record already exists
            let exists = records.iter().any(|r| r == &new_record);
            assert!(exists);

            let different_record = RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_MX_RECORD.to_string(),
                name: "mail-mx-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            let exists = records.iter().any(|r| r == &different_record);
            assert!(!exists);
        }

        #[test]
        fn test_prevent_duplicate_records_in_status() {
            let mut records = vec![RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_A_RECORD.to_string(),
                name: "web-a-record".to_string(),
                namespace: "dns-system".to_string(),
            }];

            let new_record = RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_A_RECORD.to_string(),
                name: "web-a-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            // Simulate the duplicate check from add_record_to_zone_status
            if !records.iter().any(|r| r == &new_record) {
                records.push(new_record.clone());
            }

            // Should still have only 1 record
            assert_eq!(records.len(), 1);

            let different_record = RecordReference {
                api_version: API_GROUP_VERSION.to_string(),
                kind: KIND_AAAA_RECORD.to_string(),
                name: "web-aaaa-record".to_string(),
                namespace: "dns-system".to_string(),
            };

            if !records.iter().any(|r| r == &different_record) {
                records.push(different_record.clone());
            }

            // Should now have 2 records
            assert_eq!(records.len(), 2);
        }

        // ====================================================================
        // Status Preservation Tests
        // ====================================================================

        #[test]
        fn test_preserve_records_on_status_update() {
            let existing_records = vec![
                RecordReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: KIND_A_RECORD.to_string(),
                    name: "existing-a-record".to_string(),
                    namespace: "dns-system".to_string(),
                },
                RecordReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: KIND_AAAA_RECORD.to_string(),
                    name: "existing-aaaa-record".to_string(),
                    namespace: "dns-system".to_string(),
                },
            ];

            let current_status = Some(DNSZoneStatus {
                conditions: vec![],
                observed_generation: Some(1),
                record_count: Some(5),
                secondary_ips: Some(vec!["10.0.0.1".to_string()]),
                records: existing_records.clone(),
            });

            // Simulate DNSZone reconciler creating new status
            let new_status = DNSZoneStatus {
                conditions: vec![],
                observed_generation: current_status.as_ref().and_then(|s| s.observed_generation),
                record_count: current_status.as_ref().and_then(|s| s.record_count),
                secondary_ips: current_status
                    .as_ref()
                    .and_then(|s| s.secondary_ips.clone()),
                records: current_status
                    .map(|s| s.records.clone())
                    .unwrap_or_default(),
            };

            // Verify all fields preserved
            assert_eq!(new_status.observed_generation, Some(1));
            assert_eq!(new_status.record_count, Some(5));
            assert_eq!(new_status.secondary_ips, Some(vec!["10.0.0.1".to_string()]));
            assert_eq!(new_status.records.len(), 2);
            assert_eq!(new_status.records[0].kind, "ARecord");
            assert_eq!(new_status.records[1].kind, "AAAARecord");
        }

        #[test]
        fn test_initialize_empty_records_when_no_current_status() {
            let current_status: Option<DNSZoneStatus> = None;

            let new_status = DNSZoneStatus {
                conditions: vec![],
                observed_generation: None,
                record_count: None,
                secondary_ips: None,
                records: current_status
                    .map(|s| s.records.clone())
                    .unwrap_or_default(),
            };

            assert!(new_status.records.is_empty());
        }
    }
}
