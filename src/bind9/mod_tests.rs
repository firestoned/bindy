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
