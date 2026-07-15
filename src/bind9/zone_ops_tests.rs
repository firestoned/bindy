// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for zone operations (`add_zones`, `add_primary_zone`, `add_secondary_zone`, `delete_zone`, `reload_zone`, `zone_exists`).

#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData};
    use bindcar::ZONE_TYPE_PRIMARY;

    /// Install the ring TLS crypto provider for this test process.
    ///
    /// reqwest is compiled with `rustls-no-provider` and relies on the
    /// process-default `CryptoProvider`, which `main.rs` installs at startup
    /// but unit tests do not. `install_default` returns `Err` once a provider
    /// is already set, so calling it from every test is safe and idempotent.
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

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
        let url = Bind9Manager::build_api_url("bind9-api.bindy-system.svc.cluster.local:8080");
        assert_eq!(url, "http://bind9-api.bindy-system.svc.cluster.local:8080");
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
            serial: 2_024_010_101,
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
                None, // no name_servers
                None, // no name_server_ips
                None, // no secondary IPs
                None, // no primary IPs for primary zones
                None, // no DNSSEC policy for this test
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
                None, // no name_servers
                None, // no name_server_ips
                None, // no secondary IPs
                None, // no primary IPs for primary zones
                None, // no DNSSEC policy for this test
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
        // Test with freeze_before_delete=true (primary zone behavior)
        let result = manager
            .delete_zone("nonexistent.com", "localhost:8080", true)
            .await;

        // Should either succeed or return specific "not found" error
        if let Err(e) = result {
            let err_msg = e.to_string();
            assert!(err_msg.contains("not found") || err_msg.contains("404"));
        }
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_zone_exists_connection_error() {
        let manager = Bind9Manager::new();

        // Should return Err on connection error, not Ok(false)
        let result = manager
            .zone_exists("example.com", "invalid-host:99999")
            .await;

        assert!(result.is_err());
    }

    // =====================================================
    // HTTP error inspection helpers (zone_exists 404 fix)
    // =====================================================

    use super::super::{is_http_conflict, is_http_not_found, HttpError};
    use reqwest::StatusCode;

    fn http_error(status: StatusCode, message: &str) -> anyhow::Error {
        anyhow::Error::from(HttpError {
            status,
            message: message.to_string(),
        })
    }

    #[test]
    fn test_is_http_not_found_on_bare_error() {
        let err = http_error(StatusCode::NOT_FOUND, "zone not found");
        assert!(is_http_not_found(&err));
    }

    #[test]
    fn test_is_http_not_found_through_context_layers() {
        // zone_status() wraps the HttpError with anyhow context; the helper
        // must see through the context layers (string-matching on
        // e.to_string() only prints the outermost context and made the 404
        // branch unreachable).
        let err = http_error(StatusCode::NOT_FOUND, "zone not found")
            .context("Failed to get zone status")
            .context("outer context");
        assert!(is_http_not_found(&err));
    }

    #[test]
    fn test_is_http_not_found_rejects_other_statuses() {
        let err = http_error(StatusCode::INTERNAL_SERVER_ERROR, "boom")
            .context("Failed to get zone status");
        assert!(!is_http_not_found(&err));
    }

    #[test]
    fn test_is_http_not_found_rejects_non_http_errors() {
        let err = anyhow::anyhow!("connection reset by peer");
        assert!(!is_http_not_found(&err));
    }

    #[test]
    fn test_is_http_conflict_on_bare_error() {
        let err = http_error(StatusCode::CONFLICT, "zone already exists");
        assert!(is_http_conflict(&err));
    }

    #[test]
    fn test_is_http_conflict_through_context_layers() {
        let err =
            http_error(StatusCode::CONFLICT, "zone already exists").context("Failed to add zone");
        assert!(is_http_conflict(&err));
    }

    #[test]
    fn test_is_http_conflict_rejects_not_found() {
        let err = http_error(StatusCode::NOT_FOUND, "zone not found");
        assert!(!is_http_conflict(&err));
    }

    // =====================================================
    // zone_exists against a mock bindcar server
    // =====================================================

    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_zone_exists_returns_false_on_404() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/zones/missing.example.com/status"))
            .respond_with(ResponseTemplate::new(404).set_body_string("zone not found"))
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let result = super::super::zone_exists(&client, None, "missing.example.com", &server.uri())
            .await
            .expect("404 must map to Ok(false), not Err");

        assert!(!result, "a 404 from bindcar means the zone does not exist");
    }

    #[tokio::test]
    async fn test_zone_exists_returns_true_on_200() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/zones/present.example.com/status"))
            .respond_with(ResponseTemplate::new(200).set_body_string("zone is loaded"))
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let result = super::super::zone_exists(&client, None, "present.example.com", &server.uri())
            .await
            .expect("200 must map to Ok(true)");

        assert!(result);
    }

    // =====================================================
    // create_zone_http via the shared retry path
    // =====================================================

    fn test_key_data() -> RndcKeyData {
        RndcKeyData {
            name: "test-key".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdHNlY3JldA==".to_string(),
        }
    }

    fn test_zone_config() -> bindcar::ZoneConfig {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604_800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
            dnssec_policy: None,
            inline_signing: None,
        }
    }

    #[tokio::test]
    async fn test_create_zone_http_success() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/zones"))
            .respond_with(
                ResponseTemplate::new(201).set_body_string(
                    r#"{"success": true, "message": "Zone created successfully"}"#,
                ),
            )
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let result = super::super::create_zone_http(
            &client,
            None,
            "new.example.com",
            ZONE_TYPE_PRIMARY,
            test_zone_config(),
            &server.uri(),
            &test_key_data(),
        )
        .await;

        assert!(
            result.is_ok(),
            "successful creation must return Ok: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_create_zone_http_treats_409_as_success() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/zones"))
            .respond_with(ResponseTemplate::new(409).set_body_string("zone already exists"))
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let result = super::super::create_zone_http(
            &client,
            None,
            "dup.example.com",
            ZONE_TYPE_PRIMARY,
            test_zone_config(),
            &server.uri(),
            &test_key_data(),
        )
        .await;

        assert!(
            result.is_ok(),
            "409 Conflict means the zone already exists and must be idempotent: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_create_zone_http_fails_on_bad_request() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/zones"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid zone name"))
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let result = super::super::create_zone_http(
            &client,
            None,
            "bad zone",
            ZONE_TYPE_PRIMARY,
            test_zone_config(),
            &server.uri(),
            &test_key_data(),
        )
        .await;

        assert!(result.is_err(), "a 400 must not be swallowed");
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
                serial: 2_025_010_101,
                refresh: 3600,
                retry: 600,
                expire: 604_800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
            dnssec_policy: None,
            inline_signing: None,
        };

        let zone_file = zone_config.to_zone_file();

        assert!(zone_file.contains("$TTL 3600"));
        assert!(zone_file.contains("@ IN SOA ns1.example.com. admin.example.com."));
        #[allow(clippy::unreadable_literal)]
        {
            assert!(zone_file.contains("2025010101"));
        }
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
                expire: 604_800,
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
            dnssec_policy: None,
            inline_signing: None,
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
                expire: 604_800,
                negative_ttl: 86400,
            },
            name_servers: vec![],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
            dnssec_policy: None,
            inline_signing: None,
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
                expire: 604_800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
            dnssec_policy: None,
            inline_signing: None,
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
            expire: 604_800,
            negative_ttl: 86400,
        };

        assert_eq!(soa.refresh, 3600);
        assert_eq!(soa.retry, 600);
        assert_eq!(soa.expire, 604_800);
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

    // =====================================================
    // DNSSEC Zone Configuration Tests (Phase 4)
    // =====================================================

    #[test]
    fn test_zone_config_with_dnssec_policy() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        // Test that ZoneConfig correctly includes DNSSEC policy and inline signing
        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604_800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
            dnssec_policy: Some("default".to_string()),
            inline_signing: Some(true),
        };

        // Verify DNSSEC fields are set correctly
        assert_eq!(zone_config.dnssec_policy, Some("default".to_string()));
        assert_eq!(zone_config.inline_signing, Some(true));
    }

    #[test]
    fn test_zone_config_without_dnssec() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        // Test that ZoneConfig works without DNSSEC (backward compatibility)
        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604_800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
            dnssec_policy: None,
            inline_signing: None,
        };

        // Verify DNSSEC fields are None
        assert_eq!(zone_config.dnssec_policy, None);
        assert_eq!(zone_config.inline_signing, None);
    }

    #[test]
    fn test_zone_config_dnssec_policy_names() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        // Test various DNSSEC policy names
        let test_policies = vec!["default", "custom", "high-security", "fast-rotation"];

        for policy_name in test_policies {
            let zone_config = ZoneConfig {
                ttl: 3600,
                soa: SoaRecord {
                    primary_ns: "ns1.example.com.".to_string(),
                    admin_email: "admin.example.com.".to_string(),
                    serial: 1,
                    refresh: 3600,
                    retry: 600,
                    expire: 604_800,
                    negative_ttl: 86400,
                },
                name_servers: vec!["ns1.example.com.".to_string()],
                name_server_ips: HashMap::new(),
                records: vec![],
                also_notify: None,
                allow_transfer: None,
                primaries: None,
                dnssec_policy: Some(policy_name.to_string()),
                inline_signing: Some(true),
            };

            assert_eq!(
                zone_config.dnssec_policy,
                Some(policy_name.to_string()),
                "Policy name {policy_name} should be preserved"
            );
            assert_eq!(
                zone_config.inline_signing,
                Some(true),
                "Inline signing should be enabled for DNSSEC policy {policy_name}"
            );
        }
    }

    #[test]
    fn test_zone_config_inline_signing_without_policy() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        // Test that inline signing can be set independently (edge case)
        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604_800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            primaries: None,
            dnssec_policy: None,
            inline_signing: Some(true),
        };

        // Verify fields
        assert_eq!(zone_config.dnssec_policy, None);
        assert_eq!(zone_config.inline_signing, Some(true));
    }

    #[test]
    fn test_zone_config_secondary_no_dnssec() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        // Test that secondary zones should NOT have DNSSEC policy
        // (they receive signed zones via zone transfer)
        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604_800,
                negative_ttl: 86400,
            },
            name_servers: vec![],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
            // named binds the unprivileged 5353, so primaries are port-qualified
            // (`<ip>:5353`); bindcar (0.7.2+) parses this compact form.
            primaries: Some(super::super::with_transfer_port(&["10.0.0.1".to_string()])),
            dnssec_policy: None, // Secondary zones should not have DNSSEC policy
            inline_signing: None,
        };

        // Verify secondary zone has primaries but no DNSSEC
        assert!(zone_config.primaries.is_some());
        assert_eq!(
            zone_config.primaries.as_ref().unwrap(),
            &vec!["10.0.0.1:5353".to_string()],
            "primaries must carry the operand's unprivileged transfer port"
        );
        assert_eq!(zone_config.dnssec_policy, None);
        assert_eq!(zone_config.inline_signing, None);
    }

    #[test]
    fn test_with_transfer_port_ipv4_and_ipv6() {
        let out =
            super::super::with_transfer_port(&["10.0.0.1".to_string(), "2001:db8::1".to_string()]);
        assert_eq!(
            out,
            vec![
                "10.0.0.1:5353".to_string(),
                "[2001:db8::1]:5353".to_string()
            ],
            "IPv4 uses ip:port; IPv6 is bracketed [ip]:port"
        );
    }

    #[test]
    fn test_with_transfer_port_empty() {
        assert!(super::super::with_transfer_port(&[]).is_empty());
    }
}
