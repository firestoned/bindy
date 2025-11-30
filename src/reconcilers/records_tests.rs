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

    #[test]
    fn test_arecord_spec_creation() {
        let spec = ARecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "www".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: Some(300),
        };

        assert_eq!(spec.zone, Some("example.com".to_string()));
        assert_eq!(spec.name, "www");
        assert_eq!(spec.ipv4_address, "192.0.2.1");
        assert_eq!(spec.ttl, Some(300));
    }

    #[test]
    fn test_arecord_without_ttl() {
        let spec = ARecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "mail".to_string(),
            ipv4_address: "192.0.2.2".to_string(),
            ttl: None,
        };

        assert_eq!(spec.ttl, None);
    }

    #[test]
    fn test_aaaa_record_spec_creation() {
        let spec = AAAARecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "www".to_string(),
            ipv6_address: "2001:db8::1".to_string(),
            ttl: Some(300),
        };

        assert_eq!(spec.zone, Some("example.com".to_string()));
        assert_eq!(spec.ipv6_address, "2001:db8::1");
        assert_eq!(spec.ttl, Some(300));
    }

    #[test]
    fn test_cname_record_spec_creation() {
        let spec = CNAMERecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "blog".to_string(),
            target: "www.example.com.".to_string(),
            ttl: Some(600),
        };

        assert_eq!(spec.target, "www.example.com.");
        assert_eq!(spec.ttl, Some(600));
    }

    #[test]
    fn test_mx_record_spec_creation() {
        let spec = MXRecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
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
                zone: Some("example.com".to_string()),
                zone_ref: None,
                name: "@".to_string(),
                priority,
                mail_server: format!("mx{priority}.example.com."),
                ttl: None,
            };

            assert_eq!(spec.priority, priority);
        }
    }

    #[test]
    fn test_txt_record_spec_single_string() {
        let spec = TXTRecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
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
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "_dmarc".to_string(),
            text: vec!["v=DMARC1".to_string(), "p=reject".to_string()],
            ttl: None,
        };

        assert_eq!(spec.text.len(), 2);
        assert!(spec.text.contains(&"v=DMARC1".to_string()));
        assert!(spec.text.contains(&"p=reject".to_string()));
    }

    #[test]
    fn test_ns_record_spec_creation() {
        let spec = NSRecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "@".to_string(),
            nameserver: "ns1.example.com.".to_string(),
            ttl: Some(86400),
        };

        assert_eq!(spec.nameserver, "ns1.example.com.");
        assert_eq!(spec.ttl, Some(86400));
    }

    #[test]
    fn test_srv_record_spec_creation() {
        let spec = SRVRecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
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
            zone: Some("example.com".to_string()),
            zone_ref: None,
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
    fn test_caa_record_spec_issue() {
        let spec = CAARecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
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
            zone: Some("example.com".to_string()),
            zone_ref: None,
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
            zone: Some("example.com".to_string()),
            zone_ref: None,
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
    fn test_record_with_metadata() {
        let record = ARecord {
            metadata: ObjectMeta {
                name: Some("www-example-com".to_string()),
                namespace: Some("default".to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: ARecordSpec {
                zone: Some("example.com".to_string()),
                zone_ref: None,
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
    fn test_various_ttl_values() {
        let ttl_values = vec![None, Some(60), Some(300), Some(3600), Some(86400)];

        for ttl in ttl_values {
            let spec = ARecordSpec {
                zone: Some("example.com".to_string()),
                zone_ref: None,
                name: "test".to_string(),
                ipv4_address: "192.0.2.1".to_string(),
                ttl,
            };

            assert_eq!(spec.ttl, ttl);
        }
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
                zone: Some("example.com".to_string()),
                zone_ref: None,
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
        };

        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.conditions[0].r#type, "Ready");
        assert_eq!(status.observed_generation, Some(1));
    }

    #[test]
    fn test_txt_record_empty_strings() {
        let spec = TXTRecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "test".to_string(),
            text: vec![],
            ttl: None,
        };

        assert_eq!(spec.text.len(), 0);
    }

    #[test]
    fn test_apex_vs_subdomain_naming() {
        let apex_record = ARecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "@".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: None,
        };

        let subdomain_record = ARecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "www".to_string(),
            ipv4_address: "192.0.2.2".to_string(),
            ttl: None,
        };

        assert_eq!(apex_record.name, "@");
        assert_eq!(subdomain_record.name, "www");
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
                zone: Some("example.com".to_string()),
                zone_ref: None,
                name: "@".to_string(),
                priority,
                mail_server: server.to_string(),
                ttl: Some(3600),
            };

            assert_eq!(spec.priority, priority);
            assert_eq!(spec.mail_server, server);
        }
    }

    #[test]
    fn test_cname_fqdn_requirement() {
        let spec = CNAMERecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "alias".to_string(),
            target: "target.example.com.".to_string(),
            ttl: None,
        };

        // CNAME targets should end with a dot for FQDN
        assert!(spec.target.ends_with('.'));
    }

    #[test]
    fn test_srv_record_zero_priority_weight() {
        let spec = SRVRecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
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
    fn test_ns_record_delegation() {
        let spec = NSRecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "subdomain".to_string(),
            nameserver: "ns1.subdomain.example.com.".to_string(),
            ttl: Some(86400),
        };

        assert_eq!(spec.name, "subdomain");
        assert!(spec.nameserver.contains("subdomain"));
    }

    #[test]
    fn test_txt_record_long_string() {
        let long_value = "v".repeat(255); // 255 characters is max for a single TXT string
        let spec = TXTRecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "test".to_string(),
            text: vec![long_value.clone()],
            ttl: None,
        };

        assert_eq!(spec.text[0].len(), 255);
    }

    #[test]
    fn test_caa_record_critical_flag() {
        let spec = CAARecordSpec {
            zone: Some("example.com".to_string()),
            zone_ref: None,
            name: "@".to_string(),
            flags: 128, // Critical flag
            tag: "issue".to_string(),
            value: "ca.example.com".to_string(),
            ttl: None,
        };

        // 128 indicates critical flag
        assert_eq!(spec.flags, 128);
    }
}
