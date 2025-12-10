// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for zone operations (add_zones, add_primary_zone, add_secondary_zone, delete_zone, reload_zone, zone_exists).

#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData};
    use bindcar::ZONE_TYPE_PRIMARY;

    // =====================================================
    // HTTP API URL Building Tests
    // =====================================================

    #[test]
    fn test_build_api_url_with_http() {
        let url = Bind9Manager::build_api_url("http://localhost:8080");
        assert_eq!(url, "http://localhost:8080");
    }

    #[test]
    fn test_build_api_url_without_scheme() {
        let url = Bind9Manager::build_api_url("localhost:8080");
        assert_eq!(url, "http://localhost:8080");
    }

    #[test]
    fn test_build_api_url_with_https() {
        let url = Bind9Manager::build_api_url("https://api.example.com:8443");
        assert_eq!(url, "https://api.example.com:8443");
    }

    #[test]
    fn test_build_api_url_trailing_slash() {
        let url = Bind9Manager::build_api_url("http://localhost:8080/");
        assert_eq!(url, "http://localhost:8080");
    }

    #[test]
    fn test_build_api_url_ipv4() {
        let url = Bind9Manager::build_api_url("192.168.1.1:8080");
        assert_eq!(url, "http://192.168.1.1:8080");
    }

    #[test]
    fn test_build_api_url_ipv6() {
        let url = Bind9Manager::build_api_url("[::1]:8080");
        assert_eq!(url, "http://[::1]:8080");
    }

    #[test]
    fn test_build_api_url_dns_name() {
        let url = Bind9Manager::build_api_url("bind9-api.dns-system.svc.cluster.local:8080");
        assert_eq!(url, "http://bind9-api.dns-system.svc.cluster.local:8080");
    }

    #[test]
    fn test_build_api_url_empty_string() {
        let url = Bind9Manager::build_api_url("");
        // Should handle empty string gracefully
        assert!(url.is_empty() || url == "http://");
    }

    #[test]
    fn test_build_api_url_only_port() {
        let url = Bind9Manager::build_api_url(":8080");
        assert_eq!(url, "http://:8080");
    }

    #[test]
    fn test_build_api_url_no_port() {
        let url = Bind9Manager::build_api_url("localhost");
        assert_eq!(url, "http://localhost");
    }

    #[test]
    fn test_build_api_url_multiple_slashes() {
        let url = Bind9Manager::build_api_url("http://localhost:8080///");
        assert_eq!(url, "http://localhost:8080");
    }

    // =====================================================
    // Negative Test Cases for HTTP API
    // =====================================================

    #[tokio::test]
    #[ignore = "Requires mock HTTP server or real server returning errors"]
    async fn test_reload_zone_not_found() {
        let manager = Bind9Manager::new();

        // Should return error when zone doesn't exist
        let result = manager
            .reload_zone("nonexistent.com", "localhost:8080")
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not found") || err_msg.contains("404"));
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server returning 500 error"]
    async fn test_server_status_500_error() {
        let manager = Bind9Manager::new();

        // Should handle 500 errors gracefully
        let result = manager.server_status("localhost:8080").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server with timeout"]
    async fn test_http_request_timeout() {
        let manager = Bind9Manager::new();

        // Should timeout if server is unresponsive
        let result = manager
            .reload_zone("example.com", "10.255.255.1:8080") // Non-routable IP
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("timeout") || err_msg.contains("connect"));
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server returning invalid JSON"]
    async fn test_invalid_json_response() {
        let manager = Bind9Manager::new();

        // Should handle malformed JSON responses
        let result = manager.server_status("localhost:8080").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_add_zone_duplicate() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test-key".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdHNlY3JldA==".to_string(),
        };

        let soa_record = crate::crd::SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin.example.com.".to_string(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604_800,
            negative_ttl: 86400,
        };

        // First add should succeed and return true (zone was added)
        let result1 = manager
            .add_zones(
                "example.com",
                ZONE_TYPE_PRIMARY,
                "localhost:8080",
                &key_data,
                Some(&soa_record),
                None, // no name_server_ips
                None, // no secondary IPs
                None, // no primary IPs for primary zones
            )
            .await;
        assert!(result1.is_ok());
        assert!(
            result1.unwrap(),
            "First add should return true (zone was added)"
        );

        // Second add of same zone should be idempotent and return false (zone already exists)
        let result2 = manager
            .add_zones(
                "example.com",
                ZONE_TYPE_PRIMARY,
                "localhost:8080",
                &key_data,
                Some(&soa_record),
                None, // no name_server_ips
                None, // no secondary IPs
                None, // no primary IPs for primary zones
            )
            .await;
        assert!(result2.is_ok());
        assert!(
            !result2.unwrap(),
            "Second add should return false (zone already exists)"
        );
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_delete_nonexistent_zone() {
        let manager = Bind9Manager::new();

        // Deleting non-existent zone should not error (idempotent)
        let result = manager
            .delete_zone("nonexistent.com", "localhost:8080")
            .await;

        // Should either succeed or return specific "not found" error
        if result.is_err() {
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("not found") || err_msg.contains("404"));
        }
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_zone_exists_false() {
        let manager = Bind9Manager::new();

        let exists = manager
            .zone_exists("definitely-does-not-exist-12345.com", "localhost:8080")
            .await;

        assert!(!exists);
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_zone_exists_connection_error() {
        let manager = Bind9Manager::new();

        // Should return false on connection error
        let exists = manager
            .zone_exists("example.com", "invalid-host:99999")
            .await;

        assert!(!exists);
    }

    // =====================================================
    // ZoneConfig and bindcar Integration Tests
    // =====================================================

    #[test]
    fn test_zone_config_to_zone_file_basic() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 2025010101,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
        };

        let zone_file = zone_config.to_zone_file();

        assert!(zone_file.contains("$TTL 3600"));
        assert!(zone_file.contains("@ IN SOA ns1.example.com. admin.example.com."));
        assert!(zone_file.contains("2025010101"));
        assert!(zone_file.contains("@ IN NS ns1.example.com."));
    }

    #[test]
    fn test_zone_config_with_dns_records() {
        use bindcar::{DnsRecord, SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![
                DnsRecord {
                    name: "@".to_string(),
                    record_type: "A".to_string(),
                    value: "192.0.2.1".to_string(),
                    ttl: None,
                    priority: None,
                },
                DnsRecord {
                    name: "www".to_string(),
                    record_type: "A".to_string(),
                    value: "192.0.2.2".to_string(),
                    ttl: Some(300),
                    priority: None,
                },
            ],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
        };

        let zone_file = zone_config.to_zone_file();

        assert!(zone_file.contains("@ IN A 192.0.2.1"));
        assert!(zone_file.contains("www 300 IN A 192.0.2.2"));
    }

    #[test]
    fn test_zone_config_minimal() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        let zone_config = ZoneConfig {
            ttl: 300,
            soa: SoaRecord {
                primary_ns: "ns.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            name_servers: vec![],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
        };

        let zone_file = zone_config.to_zone_file();

        assert!(zone_file.contains("$TTL 300"));
        assert!(zone_file.contains("@ IN SOA ns.example.com. admin.example.com."));
    }

    #[test]
    fn test_create_zone_request_serialization() {
        use bindcar::{CreateZoneRequest, SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
        };

        let request = CreateZoneRequest {
            zone_name: "example.com".to_string(),
            zone_type: ZONE_TYPE_PRIMARY.to_string(),
            zone_config,
            update_key_name: Some("bind9-key".to_string()),
        };

        // Verify it can be serialized to JSON
        let json = serde_json::to_string(&request);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("zoneName"));
        assert!(json_str.contains("example.com"));
        assert!(json_str.contains("zoneType"));
        assert!(json_str.contains(ZONE_TYPE_PRIMARY));
        assert!(json_str.contains("zoneConfig"));
        assert!(json_str.contains("updateKeyName"));
        assert!(json_str.contains("bind9-key"));
    }

    #[test]
    fn test_zone_response_deserialization() {
        use bindcar::ZoneResponse;

        let json = r#"{"success": true, "message": "Zone created successfully"}"#;

        let response: Result<ZoneResponse, _> = serde_json::from_str(json);
        assert!(response.is_ok());

        let response = response.unwrap();
        assert!(response.success);
        assert_eq!(response.message, "Zone created successfully");
        assert_eq!(response.details, None);
    }

    #[test]
    fn test_zone_response_deserialization_with_details() {
        use bindcar::ZoneResponse;

        let json = r#"{
            "success": false,
            "message": "Zone creation failed",
            "details": "Zone already exists"
        }"#;

        let response: Result<ZoneResponse, _> = serde_json::from_str(json);
        assert!(response.is_ok());

        let response = response.unwrap();
        assert!(!response.success);
        assert_eq!(response.message, "Zone creation failed");
        assert_eq!(response.details, Some("Zone already exists".to_string()));
    }

    #[test]
    fn test_soa_record_default_values() {
        use bindcar::SoaRecord;

        let soa = SoaRecord {
            primary_ns: "ns.example.com.".to_string(),
            admin_email: "admin.example.com.".to_string(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        assert_eq!(soa.refresh, 3600);
        assert_eq!(soa.retry, 600);
        assert_eq!(soa.expire, 604800);
        assert_eq!(soa.negative_ttl, 86400);
    }

    #[test]
    fn test_dns_record_with_mx_priority() {
        use bindcar::DnsRecord;

        let record = DnsRecord {
            name: "@".to_string(),
            record_type: "MX".to_string(),
            value: "mail.example.com.".to_string(),
            ttl: Some(3600),
            priority: Some(10),
        };

        assert_eq!(record.priority, Some(10));
        assert_eq!(record.record_type, "MX");
    }

    #[test]
    fn test_dns_record_without_priority() {
        use bindcar::DnsRecord;

        let record = DnsRecord {
            name: "www".to_string(),
            record_type: "A".to_string(),
            value: "192.0.2.1".to_string(),
            ttl: None,
            priority: None,
        };

        assert_eq!(record.priority, None);
    }
}
