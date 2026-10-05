// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

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

    // --- TLS transport (ADR-0004) -------------------------------------------

    /// With TLS on, a bare `ip:port` endpoint must become an `https://` URL —
    /// this is what actually switches the operator onto the encrypted
    /// transport, since every call site passes a bare pod address.
    #[test]
    fn test_build_api_url_uses_https_when_tls_enabled() {
        let url = crate::bind9::zone_ops::build_api_url_with_scheme("10.1.2.3:8080", true);
        assert_eq!(url, "https://10.1.2.3:8080");
    }

    /// With TLS off the behaviour is byte-for-byte what it was before.
    #[test]
    fn test_build_api_url_uses_http_when_tls_disabled() {
        let url = crate::bind9::zone_ops::build_api_url_with_scheme("10.1.2.3:8080", false);
        assert_eq!(url, "http://10.1.2.3:8080");
    }

    /// An explicit scheme on the endpoint always wins, in either direction, so
    /// an operator can override per-endpoint without fighting the flag.
    #[test]
    fn test_explicit_scheme_is_never_rewritten() {
        assert_eq!(
            crate::bind9::zone_ops::build_api_url_with_scheme("http://host:8080", true),
            "http://host:8080",
            "an explicit http:// must not be silently upgraded"
        );
        assert_eq!(
            crate::bind9::zone_ops::build_api_url_with_scheme("https://host:8443", false),
            "https://host:8443",
            "an explicit https:// must not be silently downgraded"
        );
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
        let result = manager
            .delete_zone("nonexistent.com", "localhost:8080")
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
    // delete_zone: no freeze, absent zone is already deleted, bounded retries
    //
    // bug-192: deleting a zone burned up to ~130s per HTTP call. bindcar maps
    // rndc's "not found" on freeze/delzone to 500, which is retryable, so a
    // zone that was not on an endpoint, or an endpoint whose pod was gone, held
    // the zone's reconcile slot for minutes, and a recreated zone of the same
    // name waited behind it.
    // =====================================================

    /// Short budget so these tests stay fast.
    const TEST_DELETE_BUDGET: std::time::Duration = std::time::Duration::from_millis(300);
    /// Generous ceiling for "gave up promptly" on a slow CI runner.
    const PROMPT: std::time::Duration = std::time::Duration::from_secs(5);

    #[tokio::test]
    async fn test_delete_zone_absent_zone_is_already_deleted() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/zones/gone.example.com/status"))
            .respond_with(ResponseTemplate::new(404).set_body_string("zone not found"))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        super::super::delete_zone_within(
            &client,
            None,
            "gone.example.com",
            &server.uri(),
            TEST_DELETE_BUDGET,
        )
        .await
        .expect("a zone that is not there is already deleted");
    }

    #[tokio::test]
    async fn test_delete_zone_deletes_without_freezing() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/zones/present.example.com/status"))
            .respond_with(ResponseTemplate::new(200).set_body_string("zone is loaded"))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/api/v1/zones/present.example.com"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/zones/present.example.com/freeze"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        super::super::delete_zone_within(
            &client,
            None,
            "present.example.com",
            &server.uri(),
            TEST_DELETE_BUDGET,
        )
        .await
        .expect("delete of a present zone succeeds");
    }

    #[tokio::test]
    async fn test_delete_zone_gives_up_within_its_budget_on_server_errors() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let started = std::time::Instant::now();
        let result = super::super::delete_zone_within(
            &client,
            None,
            "stuck.example.com",
            &server.uri(),
            TEST_DELETE_BUDGET,
        )
        .await;

        assert!(result.is_err(), "a delete that keeps failing must fail");
        assert!(
            started.elapsed() < PROMPT,
            "gave up after {:?}; the budget is {TEST_DELETE_BUDGET:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn test_delete_zone_gives_up_within_its_budget_on_a_dead_endpoint() {
        ensure_crypto_provider();
        // Nothing listens here: a pod that is gone.
        let client = Arc::new(reqwest::Client::new());
        let started = std::time::Instant::now();
        let result = super::super::delete_zone_within(
            &client,
            None,
            "orphan.example.com",
            "http://127.0.0.1:1",
            TEST_DELETE_BUDGET,
        )
        .await;

        assert!(result.is_err());
        assert!(
            started.elapsed() < PROMPT,
            "gave up after {:?}; the budget is {TEST_DELETE_BUDGET:?}",
            started.elapsed()
        );
    }

    #[test]
    fn test_delete_retry_budget_is_short() {
        // Long enough to ride out a bindcar restart, short enough that a dead
        // endpoint cannot hold a zone's reconcile slot for minutes.
        assert!(super::super::DELETE_RETRY_BUDGET <= std::time::Duration::from_secs(15));
    }

    // =====================================================
    // add_primary_zone on a zone that already exists
    // =====================================================

    fn test_soa_record() -> crate::crd::SOARecord {
        crate::crd::SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin.example.com.".to_string(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604_800,
            negative_ttl: 300,
        }
    }

    /// A zone created before its DNSSEC policy was set (or before the
    /// cluster enabled signing) must still be signed: on an existing zone the
    /// policy goes out in the PATCH, even with no secondaries to update.
    #[tokio::test]
    async fn test_add_primary_zone_patches_dnssec_policy_onto_existing_zone() {
        use wiremock::matchers::body_partial_json;
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/zones"))
            .respond_with(ResponseTemplate::new(409).set_body_string("zone already exists"))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/api/v1/zones/example.com"))
            .and(body_partial_json(
                serde_json::json!({"dnssecPolicy": "core-dns"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let added = super::super::add_primary_zone(
            &client,
            None,
            "example.com",
            &server.uri(),
            &test_key_data(),
            &test_soa_record(),
            None,
            None,
            None,
            Some("core-dns"),
        )
        .await
        .expect("an existing zone is not an error");

        assert!(!added, "the zone already existed");
    }

    /// No policy and no secondaries: nothing to change on an existing zone,
    /// so no PATCH (which would also cost bindcar an `rndc modzone`).
    #[tokio::test]
    async fn test_add_primary_zone_existing_zone_without_policy_sends_no_patch() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/zones"))
            .respond_with(ResponseTemplate::new(409).set_body_string("zone already exists"))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(0)
            .mount(&server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        super::super::add_primary_zone(
            &client,
            None,
            "example.com",
            &server.uri(),
            &test_key_data(),
            &test_soa_record(),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("an existing zone is not an error");
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

    // ------------------------------------------------------------------
    // ADR-0006: DS record extraction (roadmap 07 Phase 5)
    // ------------------------------------------------------------------

    use hickory_proto::dnssec::rdata::DNSKEY;
    use hickory_proto::dnssec::{Algorithm, PublicKeyBuf};

    /// Fixed key material so key tags and digests are deterministic. The DS
    /// derivation hashes the RDATA; it never validates the key, so any bytes
    /// of a plausible P-256 length work.
    const TEST_P256_KEY_BYTES: [u8; 64] = [0xAB; 64];

    fn test_dnskey(secure_entry_point: bool, revoke: bool) -> DNSKEY {
        DNSKEY::new(
            true,
            secure_entry_point,
            revoke,
            PublicKeyBuf::new(TEST_P256_KEY_BYTES.to_vec(), Algorithm::ECDSAP256SHA256),
        )
    }

    #[test]
    fn test_dns_query_endpoint_swaps_api_port_for_dns_port() {
        assert_eq!(
            super::super::dns_query_endpoint("10.1.2.3:8080"),
            "10.1.2.3:5353"
        );
        assert_eq!(
            super::super::dns_query_endpoint("[2001:db8::1]:8080"),
            "[2001:db8::1]:5353"
        );
        // No port at all: the DNS port is appended
        assert_eq!(
            super::super::dns_query_endpoint("10.1.2.3"),
            "10.1.2.3:5353"
        );
    }

    #[test]
    fn test_ds_records_from_dnskeys_only_derives_from_ksk() {
        let zsk = test_dnskey(false, false);
        let ksk = test_dnskey(true, false);

        let out = super::super::ds_records_from_dnskeys("example.com", &[zsk, ksk])
            .expect("DS derivation should succeed");

        assert_eq!(out.len(), 1, "only the KSK (SEP flag) yields a DS record");
    }

    #[test]
    fn test_ds_records_from_dnskeys_skips_revoked_keys() {
        let revoked_ksk = test_dnskey(true, true);
        let out = super::super::ds_records_from_dnskeys("example.com", &[revoked_ksk])
            .expect("DS derivation should succeed");
        assert!(out.is_empty(), "revoked keys must not produce DS records");
    }

    #[test]
    fn test_ds_records_from_dnskeys_empty_for_unsigned_zone() {
        let out = super::super::ds_records_from_dnskeys("example.com", &[])
            .expect("DS derivation should succeed");
        assert!(out.is_empty());
    }

    #[test]
    fn test_ds_record_presentation_format() {
        let ksk = test_dnskey(true, false);
        let expected_tag = ksk.calculate_key_tag().expect("key tag");

        let out = super::super::ds_records_from_dnskeys("example.com", &[ksk])
            .expect("DS derivation should succeed");
        let info = &out[0];

        assert_eq!(info.key_tag, expected_tag);
        assert_eq!(info.algorithm, "ECDSAP256SHA256");

        // "<zone>. IN DS <keytag> <alg> 2 <sha256-hex>"
        let parts: Vec<&str> = info.presentation.split_whitespace().collect();
        assert_eq!(parts.len(), 7, "presentation: {}", info.presentation);
        assert_eq!(parts[0], "example.com.");
        assert_eq!(parts[1], "IN");
        assert_eq!(parts[2], "DS");
        assert_eq!(parts[3], expected_tag.to_string());
        assert_eq!(parts[4], "13", "ECDSAP256SHA256 is DNSSEC algorithm 13");
        assert_eq!(parts[5], "2", "digest type is SHA-256");
        assert_eq!(parts[6].len(), 64, "SHA-256 digest is 32 bytes hex-encoded");
        assert!(
            parts[6]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()),
            "digest must be uppercase hex: {}",
            parts[6]
        );
    }

    #[test]
    fn test_ds_record_zone_name_trailing_dot_is_normalized() {
        let with_dot =
            super::super::ds_records_from_dnskeys("example.com.", &[test_dnskey(true, false)])
                .expect("DS derivation should succeed");
        let without_dot =
            super::super::ds_records_from_dnskeys("example.com", &[test_dnskey(true, false)])
                .expect("DS derivation should succeed");
        assert_eq!(with_dot[0].presentation, without_dot[0].presentation);
    }

    // ------------------------------------------------------------------
    // Roadmap 26 (bindcar 0.8.1+): nextKeyRollover from zone status
    // ------------------------------------------------------------------

    fn key_status(
        role: &str,
        key_signing: bool,
        removed: bool,
        next: Option<&str>,
    ) -> bindcar::DnssecKeyStatus {
        bindcar::DnssecKeyStatus {
            tag: 12345,
            role: role.to_string(),
            key_signing,
            removed,
            next_rollover: next.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn test_parse_zone_status_dnssec_extracts_block() {
        let body = r#"{
            "success": true,
            "message": "ok",
            "dnssec": {
                "policy": "default",
                "signed": true,
                "keys": [
                    {"tag": 12345, "algorithm": "ECDSAP256SHA256", "role": "KSK",
                     "published": true, "keySigning": true, "zoneSigning": false,
                     "removed": false, "nextRollover": "2027-09-27T00:00:00"}
                ]
            }
        }"#;

        let dnssec = super::super::parse_zone_status_dnssec(body).expect("dnssec block must parse");
        assert!(dnssec.signed);
        assert_eq!(dnssec.keys.len(), 1);
        assert_eq!(
            dnssec.keys[0].next_rollover.as_deref(),
            Some("2027-09-27T00:00:00")
        );
    }

    #[test]
    fn test_parse_zone_status_dnssec_absent_block_or_garbage_is_none() {
        assert!(
            super::super::parse_zone_status_dnssec(r#"{"success": true, "message": "ok"}"#)
                .is_none()
        );
        assert!(super::super::parse_zone_status_dnssec("rndc: not json").is_none());
    }

    #[test]
    fn test_next_ksk_rollover_from_key_signing_key() {
        let status = bindcar::DnssecStatus {
            policy: Some("default".to_string()),
            signed: true,
            keys: vec![
                key_status("ZSK", false, false, Some("2026-12-01T00:00:00")),
                key_status("KSK", true, false, Some("2027-09-27T00:00:00")),
            ],
        };
        assert_eq!(
            super::super::next_ksk_rollover(&status).as_deref(),
            Some("2027-09-27T00:00:00"),
            "only key-signing keys drive nextKeyRollover (ZSK events are not KSK rollovers)"
        );
    }

    #[test]
    fn test_next_ksk_rollover_earliest_wins_and_removed_skipped() {
        let status = bindcar::DnssecStatus {
            policy: None,
            signed: true,
            keys: vec![
                key_status("KSK", true, true, Some("2026-10-01T00:00:00")),
                key_status("CSK", true, false, Some("2027-01-01T00:00:00")),
                key_status("KSK", true, false, Some("2027-06-01T00:00:00")),
            ],
        };
        assert_eq!(
            super::super::next_ksk_rollover(&status).as_deref(),
            Some("2027-01-01T00:00:00"),
            "removed keys are skipped; the earliest remaining event wins"
        );
    }

    #[test]
    fn test_next_ksk_rollover_none_when_no_ksk_event() {
        let status = bindcar::DnssecStatus {
            policy: Some("default".to_string()),
            signed: true,
            keys: vec![key_status("ZSK", false, false, Some("2026-12-01T00:00:00"))],
        };
        assert!(super::super::next_ksk_rollover(&status).is_none());
    }
}
