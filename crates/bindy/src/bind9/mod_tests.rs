// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `Bind9Manager`.

#[cfg(test)]
#[allow(unexpected_cfgs)]
mod tests {
    use crate::bind9::Bind9Manager;

    /// Install the ring TLS crypto provider for this test process.
    ///
    /// `Bind9Manager::new()` builds a `reqwest` client, and reqwest is compiled
    /// with `rustls-no-provider` (so bindy stays ring-only and never pulls in
    /// aws-lc-sys). reqwest therefore relies on the process-default
    /// `CryptoProvider`, which `main.rs` installs at startup but unit tests do
    /// not. Install it here; `install_default` returns `Err` once a provider is
    /// already set, so calling it from every test is safe and idempotent.
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn test_bind9_manager_creation() {
        ensure_crypto_provider();
        let manager = Bind9Manager::new();
        // Verify manager can be created
        let debug_output = format!("{manager:?}");
        assert!(debug_output.starts_with("Bind9Manager"));
        assert!(debug_output.contains("client"));
        assert!(debug_output.contains("token"));
    }

    #[test]
    fn test_bind9_manager_default() {
        ensure_crypto_provider();
        let manager = Bind9Manager::new();
        // Verify default implementation works
        let debug_output = format!("{manager:?}");
        assert!(debug_output.starts_with("Bind9Manager"));
    }

    // =====================================================
    // ServiceAccount token cache (stale-token fix)
    // =====================================================

    #[test]
    fn test_token_cache_fresh_within_ttl() {
        let read_at = std::time::Instant::now();
        let now = read_at + std::time::Duration::from_secs(1);

        assert!(
            super::super::is_token_cache_fresh(read_at, now),
            "a just-read token must be served from cache"
        );
    }

    #[test]
    fn test_token_cache_stale_after_ttl() {
        let read_at = std::time::Instant::now();
        let now = read_at + std::time::Duration::from_secs(super::super::TOKEN_CACHE_TTL_SECS + 1);

        assert!(
            !super::super::is_token_cache_fresh(read_at, now),
            "a token older than the TTL must be re-read from disk"
        );
    }

    #[test]
    fn test_token_cache_ttl_well_below_projected_token_expiry() {
        // deploy/operator/deployment.yaml projects the bindcar-audience token
        // with expirationSeconds: 3600 and kubelet rewrites the file at ~80%
        // of that lifetime. The cache TTL must stay well below the rotation
        // window so the operator never presents an expired token.
        let projected_token_expiry_secs: u64 = 3600;
        assert!(super::super::TOKEN_CACHE_TTL_SECS * 2 < projected_token_expiry_secs);
    }

    /// Build a Deployment whose bindcar container has auth disabled
    /// (no `BIND_ALLOWED_SERVICE_ACCOUNTS` env var).
    fn deployment_without_auth_env() -> k8s_openapi::api::apps::v1::Deployment {
        use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
        use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};

        Deployment {
            spec: Some(DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: crate::constants::CONTAINER_NAME_BINDCAR.to_string(),
                            env: None,
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_get_token_none_when_auth_disabled() {
        ensure_crypto_provider();
        let manager = Bind9Manager::new_with_deployment(
            std::sync::Arc::new(deployment_without_auth_env()),
            "test-instance".to_string(),
            "bindy-system".to_string(),
        );

        assert!(!manager.is_auth_enabled());
        assert_eq!(
            manager.get_token(),
            None,
            "auth-disabled instances must never present a token"
        );
    }

    // =====================================================
    // HTTP client timeouts
    // =====================================================

    #[tokio::test]
    async fn test_http_client_request_times_out_on_unresponsive_server() {
        ensure_crypto_provider();

        // A server that accepts connections but never responds: without a
        // request timeout, this hangs a reconcile task forever.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let mut held_sockets = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held_sockets.push(stream); // hold the connection open, never reply
            }
        });

        let client = super::super::build_http_client_with_timeouts(
            std::time::Duration::from_millis(500),
            std::time::Duration::from_millis(500),
        );

        let result = client
            .get(format!("http://{addr}/api/v1/server/status"))
            .send()
            .await;

        server.abort();

        let err = result.expect_err("request to an unresponsive server must fail");
        assert!(err.is_timeout(), "expected a timeout error, got: {err}");
    }
}

// ---------------------------------------------------------------------------
// TLS transport plumbing (ADR-0004)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tls_transport_tests {
    use crate::bind9::Bind9Manager;
    use crate::crd::{BindcarTlsConfig, CaBundleKeyRef, CaBundleSource};

    /// See the note on the identically named helper above: reqwest is built
    /// with `rustls-no-provider`, so a provider must be installed before any
    /// client is constructed. Idempotent.
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn tls(enabled: bool) -> BindcarTlsConfig {
        BindcarTlsConfig {
            enabled: Some(enabled),
            secret_name: Some("bindcar-tls".into()),
            ca_bundle: Some(CaBundleSource {
                config_map_ref: Some(CaBundleKeyRef {
                    name: "bindcar-ca".into(),
                    key: None,
                }),
                secret_ref: None,
            }),
            server_name: None,
            reload_interval_seconds: None,
        }
    }

    /// A manager with no TLS configuration behaves exactly as before.
    #[test]
    fn test_manager_without_tls_uses_plaintext() {
        ensure_crypto_provider();
        let m = Bind9Manager::new();
        assert!(!m.tls_enabled());
        assert_eq!(m.qualify_server("10.1.2.3:8080"), "http://10.1.2.3:8080");
    }

    /// `enabled: false` is an explicit opt-out and must match "absent".
    #[test]
    fn test_manager_with_tls_disabled_uses_plaintext() {
        ensure_crypto_provider();
        let m = Bind9Manager::new().with_tls(Some(tls(false)));
        assert!(!m.tls_enabled());
        assert_eq!(m.qualify_server("10.1.2.3:8080"), "http://10.1.2.3:8080");
    }

    /// Enabling TLS is what actually moves the operator onto https. Every call
    /// site passes a bare `ip:port`, so this is the switch.
    #[test]
    fn test_manager_with_tls_enabled_qualifies_https() {
        ensure_crypto_provider();
        let m = Bind9Manager::new().with_tls(Some(tls(true)));
        assert!(m.tls_enabled());
        assert_eq!(m.qualify_server("10.1.2.3:8080"), "https://10.1.2.3:8080");
    }

    /// An endpoint that already carries a scheme is never rewritten, so an
    /// operator can override per-endpoint without fighting the flag.
    #[test]
    fn test_manager_never_rewrites_an_explicit_scheme() {
        ensure_crypto_provider();
        let on = Bind9Manager::new().with_tls(Some(tls(true)));
        let off = Bind9Manager::new();
        assert_eq!(on.qualify_server("http://h:8080"), "http://h:8080");
        assert_eq!(off.qualify_server("https://h:8443"), "https://h:8443");
    }

    /// With TLS off, resolving the client must not touch the Kubernetes API or
    /// fail for want of one — the plaintext path stays completely independent.
    #[tokio::test]
    async fn test_resolve_client_without_tls_needs_no_kube_client() {
        ensure_crypto_provider();
        let m = Bind9Manager::new();
        assert!(
            m.resolve_client().await.is_ok(),
            "plaintext must not require a kube client"
        );
    }

    /// With TLS on but no kube client to read the CA bundle with, the manager
    /// must fail rather than silently fall back to the plaintext client. A
    /// fallback would send the token in the clear on a deployment the operator
    /// believes is encrypted.
    #[tokio::test]
    async fn test_resolve_client_fails_closed_without_a_kube_client() {
        ensure_crypto_provider();
        let m = Bind9Manager::new().with_tls(Some(tls(true)));
        let err = m
            .resolve_client()
            .await
            .expect_err("TLS without a way to read the CA bundle must fail");
        assert!(
            err.to_string().to_lowercase().contains("ca")
                || err.to_string().to_lowercase().contains("kube"),
            "error should say why: {err}"
        );
    }

    /// TLS enabled with no CA bundle configured is a configuration error, not a
    /// reason to trust the system roots.
    #[tokio::test]
    async fn test_resolve_client_requires_a_ca_bundle() {
        ensure_crypto_provider();
        let mut cfg = tls(true);
        cfg.ca_bundle = None;
        let m = Bind9Manager::new().with_tls(Some(cfg));
        assert!(m.resolve_client().await.is_err());
    }
}
