// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! BIND9 management via HTTP API sidecar.
//!
//! This module provides functionality for managing BIND9 servers using an
//! HTTP API sidecar container that executes rndc commands locally. It handles:
//!
//! - Creating and managing DNS zones via the HTTP API
//! - Adding and updating DNS records via dynamic updates (nsupdate protocol)
//! - Managing zone transfers
//! - RNDC key generation and management
//!
//! # Architecture
//!
//! The `Bind9Manager` communicates with BIND9 instances via an HTTP API sidecar
//! running in the same pod. The sidecar executes rndc commands locally and manages
//! zone files. Authentication uses Kubernetes `ServiceAccount` tokens.
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy_bind9::bind9::Bind9Manager;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let manager = Bind9Manager::new();
//!
//! // Manage zones via HTTP API
//! manager.notify_zone(
//!     "example.com",
//!     "bind9-primary-api.bindy-system.svc.cluster.local:8080"
//! ).await?;
//! # Ok(())
//! # }
//! ```

// Module declarations
pub mod duration;
pub mod records;
pub mod rndc;
pub mod types;
pub mod zone_ops;

// Re-export public types and functions for backwards compatibility
pub use rndc::{
    create_rndc_secret_data, create_tsig_signer, generate_rndc_key, parse_rndc_secret_data,
};
pub use types::{
    PTRRecordData, RndcKeyData, SRVRecordData, BINDCAR_TOKEN_PATH, SERVICE_ACCOUNT_TOKEN_PATH,
};

use anyhow::{Context, Result};
use k8s_openapi::api::apps::v1::Deployment;
pub mod tls_client;

/// Default key read from a CA bundle ConfigMap or Secret.
const DEFAULT_CA_BUNDLE_KEY: &str = "ca.crt";

/// `kube::Client` wrapper that satisfies the `Debug` derive on `Bind9Manager`.
#[derive(Clone)]
struct KubeClientDebug(kube::Client);

impl std::fmt::Debug for KubeClientDebug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("kube::Client")
    }
}

use reqwest::Client as HttpClient;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, error, warn};

/// Environment variable name that indicates bindcar authentication is enabled.
/// If this env var is present in the bindcar container, authentication is required.
const BINDCAR_AUTH_ENV_VAR: &str = "BIND_ALLOWED_SERVICE_ACCOUNTS";

/// Name of the bindcar sidecar container in `Bind9Instance` deployments.
///
/// Must match [`crate::constants::CONTAINER_NAME_BINDCAR`] — the operator names
/// the sidecar `api` (not `bindcar`), so this is what `is_auth_enabled` looks
/// for when inspecting the operand Deployment.
const BINDCAR_CONTAINER_NAME: &str = crate::constants::CONTAINER_NAME_BINDCAR;

/// How long a `ServiceAccount` token read from disk may be served from the
/// in-memory cache before it is re-read (seconds).
///
/// The operator's bindcar-audience token is projected with
/// `expirationSeconds: 3600` (see `deploy/operator/deployment.yaml`) and the
/// kubelet rewrites the token file at roughly 80% of that lifetime. Caching
/// the token for the process lifetime therefore presents an **expired** token
/// after ~1h, which bindcar's TokenReview rejects with a non-retryable 401.
/// Re-reading every 5 minutes keeps the presented token far fresher than the
/// rotation window; the file lives on a tmpfs projected volume, so the re-read
/// is cheap.
const TOKEN_CACHE_TTL_SECS: u64 = 300;

/// Connect timeout for bindcar HTTP API requests.
///
/// Without a connect timeout, a blackholed pod IP (e.g. a stale Endpoints
/// entry) hangs a reconcile task indefinitely — the retry/backoff in
/// `zone_ops::bindcar_request` never fires because attempt #1 never returns.
const BINDCAR_HTTP_CONNECT_TIMEOUT_SECS: u64 = 5;

/// Total request timeout (connect + transfer) for bindcar HTTP API requests.
///
/// Bounded well below `zone_ops::bindcar_request`'s 120s max retry window so
/// a slow attempt still leaves room for retries.
const BINDCAR_HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;

/// A `ServiceAccount` token (or the absence of one) read from disk, together
/// with the time it was read.
///
/// `token: None` means the token file could not be read (e.g. auth-disabled
/// or out-of-cluster deployments); the negative result is cached too, so a
/// missing file is not re-read on every request within the TTL.
struct CachedToken {
    /// The token contents, or `None` if no token file was readable.
    token: Option<String>,
    /// When the token file was last read.
    read_at: Instant,
}

// Custom Debug implementation to prevent logging the token in cleartext
impl std::fmt::Debug for CachedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedToken")
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("read_at", &self.read_at)
            .finish()
    }
}

/// Returns `true` while a token read at `read_at` may still be served from
/// cache at `now` (i.e. it is younger than `TOKEN_CACHE_TTL_SECS`).
fn is_token_cache_fresh(read_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(read_at) < Duration::from_secs(TOKEN_CACHE_TTL_SECS)
}

/// Build the shared bindcar HTTP client with connect and request timeouts.
///
/// # Errors
/// Fails when the HTTP client cannot be built (see
/// [`build_http_client_with_timeouts`]).
fn build_http_client() -> Result<HttpClient> {
    HTTP_CLIENT_BUILDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    build_http_client_with_timeouts(
        Duration::from_secs(BINDCAR_HTTP_CONNECT_TIMEOUT_SECS),
        Duration::from_secs(BINDCAR_HTTP_REQUEST_TIMEOUT_SECS),
    )
}

/// Build an HTTP client with the given connect and total-request timeouts.
///
/// There is deliberately no fallback to an untimed client: a client without
/// timeouts hangs a reconcile task forever on a blackholed endpoint.
///
/// # Errors
/// Fails when the `reqwest` builder fails (e.g. no TLS crypto provider is
/// installed).
fn build_http_client_with_timeouts(
    connect_timeout: Duration,
    request_timeout: Duration,
) -> Result<HttpClient> {
    HttpClient::builder()
        .connect_timeout(connect_timeout)
        .timeout(request_timeout)
        .build()
        .context("failed to build the bindcar HTTP client with connect/request timeouts")
}

/// The shared plaintext HTTP client, or the reason it could not be built.
///
/// Construction of a [`Bind9Manager`] is infallible, so a build failure is
/// kept here and surfaced by every request instead of being papered over with
/// an untimed client.
type SharedHttpClient = std::result::Result<Arc<HttpClient>, Arc<str>>;

/// How many times [`build_http_client`] ran: once per process, since every
/// [`Bind9Manager`] shares [`SHARED_HTTP_CLIENT`].
pub(crate) static HTTP_CLIENT_BUILDS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// The one plaintext bindcar HTTP client (and connection pool) every
/// [`Bind9Manager`] uses. A manager is created per instance per reconcile; a
/// client per manager rebuilt the pool and the TLS setup each time.
static SHARED_HTTP_CLIENT: std::sync::LazyLock<SharedHttpClient> =
    std::sync::LazyLock::new(|| shared_client_from(build_http_client()));

/// Wraps a client build result as a [`SharedHttpClient`], logging a failure.
fn shared_client_from(client: Result<HttpClient>) -> SharedHttpClient {
    client.map(Arc::new).map_err(|e| {
        let reason = format!("{e:#}");
        error!(
            error = %reason,
            "Failed to build the bindcar HTTP client; every bindcar request will fail"
        );
        Arc::<str>::from(reason)
    })
}

/// Manager for BIND9 servers via HTTP API sidecar.
///
/// The `Bind9Manager` provides methods for managing BIND9 servers running in Kubernetes
/// pods via an HTTP API sidecar. The API sidecar executes rndc commands locally and
/// manages zone files. Authentication uses Kubernetes `ServiceAccount` tokens.
///
/// # Examples
///
/// ```rust,no_run
/// use bindy_bind9::bind9::Bind9Manager;
///
/// let manager = Bind9Manager::new();
/// ```
#[derive(Debug, Clone)]
pub struct Bind9Manager {
    /// HTTP client for API requests, or why it could not be built
    client: SharedHttpClient,
    /// Cached `ServiceAccount` token for authentication (only used if auth is
    /// enabled). Re-read from disk when older than `TOKEN_CACHE_TTL_SECS`
    /// because the kubelet rotates the projected token file; caching it for
    /// the process lifetime would present an expired token after ~1h.
    token_cache: Arc<RwLock<Option<CachedToken>>>,
    /// Deployment for the `Bind9Instance` (used to check auth status)
    deployment: Option<Arc<Deployment>>,
    /// Instance name (for auth checking)
    instance_name: Option<String>,
    /// Instance namespace (for auth checking)
    instance_namespace: Option<String>,
    /// Resolved TLS configuration for this instance's sidecar, if any.
    tls: Option<Arc<crate::crd::BindcarTlsConfig>>,
    /// Kubernetes client used to read the CA bundle referenced by [`Self::tls`].
    ///
    /// `kube::Client` is not `Debug`, so it is skipped in the derive.
    kube_client: Option<KubeClientDebug>,
    /// Lazily built, then cached, CA-pinned HTTPS client.
    ///
    /// Built on first use rather than at construction because reading the CA
    /// bundle is an API call and the manager is created synchronously per
    /// reconcile.
    tls_client: Arc<tokio::sync::RwLock<Option<Arc<HttpClient>>>>,
}

impl Bind9Manager {
    /// Create a new `Bind9Manager` without deployment information.
    ///
    /// Creates an HTTP client (with connect/request timeouts) for API
    /// requests. The `ServiceAccount` token is read lazily at request time and
    /// cached for `TOKEN_CACHE_TTL_SECS` so kubelet token rotation is picked
    /// up. Without deployment information, auth is always assumed to be
    /// enabled (backward compatible behavior).
    ///
    /// For proper auth status detection, use `new_with_deployment()` instead.
    ///
    /// A manager built this way has no instance namespace, so it cannot read a
    /// TLS CA bundle; use [`Self::new_for_instance`] for a specific instance.
    #[must_use]
    pub fn new() -> Self {
        Self::from_shared_client(SHARED_HTTP_CLIENT.clone(), None, None, None)
    }

    /// Create a `Bind9Manager` for a specific instance whose Deployment is
    /// not known (e.g. not in the reflector store yet).
    ///
    /// Auth is assumed enabled, as with [`Self::new`], but the instance name
    /// and namespace are kept so the TLS CA bundle is read from the
    /// instance's own namespace.
    ///
    /// # Arguments
    /// * `instance_name` - Name of the `Bind9Instance`
    /// * `instance_namespace` - Namespace of the instance
    #[must_use]
    pub fn new_for_instance(instance_name: String, instance_namespace: String) -> Self {
        Self::from_shared_client(
            SHARED_HTTP_CLIENT.clone(),
            None,
            Some(instance_name),
            Some(instance_namespace),
        )
    }

    /// Assemble a manager from an HTTP client build result.
    ///
    /// A failed build is logged once and kept, so every request fails with
    /// the cause rather than running without timeouts.
    #[cfg(test)]
    fn from_client_result(
        client: Result<HttpClient>,
        deployment: Option<Arc<Deployment>>,
        instance_name: Option<String>,
        instance_namespace: Option<String>,
    ) -> Self {
        Self::from_shared_client(
            shared_client_from(client),
            deployment,
            instance_name,
            instance_namespace,
        )
    }

    fn from_shared_client(
        client: SharedHttpClient,
        deployment: Option<Arc<Deployment>>,
        instance_name: Option<String>,
        instance_namespace: Option<String>,
    ) -> Self {
        Self {
            client,
            token_cache: Arc::new(RwLock::new(None)),
            deployment,
            instance_name,
            instance_namespace,
            tls: None,
            kube_client: None,
            tls_client: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// The namespace of the instance this manager talks to, when known.
    #[must_use]
    pub fn instance_namespace(&self) -> Option<&str> {
        self.instance_namespace.as_deref()
    }

    /// The shared plaintext client, or an error if it could not be built.
    fn plain_client(&self) -> Result<&Arc<HttpClient>> {
        self.client
            .as_ref()
            .map_err(|reason| anyhow::anyhow!("bindcar HTTP client unavailable: {reason}"))
    }

    /// Create a new `Bind9Manager` with deployment information for auth checking.
    ///
    /// Creates an HTTP client (with connect/request timeouts) for API
    /// requests. The `ServiceAccount` token is read lazily at request time and
    /// cached for `TOKEN_CACHE_TTL_SECS` so kubelet token rotation is picked
    /// up. The deployment is used to determine if authentication is enabled or
    /// disabled by checking for the presence of the
    /// `BIND_ALLOWED_SERVICE_ACCOUNTS` environment variable in the bindcar container.
    ///
    /// # Arguments
    /// * `deployment` - The Deployment for the `Bind9Instance`
    /// * `instance_name` - Name of the `Bind9Instance`
    /// * `instance_namespace` - Namespace of the instance
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use bindy_bind9::bind9::Bind9Manager;
    /// use std::sync::Arc;
    ///
    /// # fn example(deployment: Arc<k8s_openapi::api::apps::v1::Deployment>) {
    /// let manager = Bind9Manager::new_with_deployment(
    ///     deployment,
    ///     "my-instance".to_string(),
    ///     "bindy-system".to_string()
    /// );
    /// # }
    /// ```
    #[must_use]
    pub fn new_with_deployment(
        deployment: Arc<Deployment>,
        instance_name: String,
        instance_namespace: String,
    ) -> Self {
        Self::from_shared_client(
            SHARED_HTTP_CLIENT.clone(),
            Some(deployment),
            Some(instance_name),
            Some(instance_namespace),
        )
    }

    /// Attach the sidecar TLS configuration for this instance.
    ///
    /// Returns the manager unchanged when `tls` is `None` or disabled, so the
    /// plaintext path costs nothing.
    #[must_use]
    pub fn with_tls(mut self, tls: Option<crate::crd::BindcarTlsConfig>) -> Self {
        self.tls = tls.map(Arc::new);
        self
    }

    /// Attach the Kubernetes client used to read the configured CA bundle.
    #[must_use]
    pub fn with_kube_client(mut self, client: kube::Client) -> Self {
        self.kube_client = Some(KubeClientDebug(client));
        self
    }

    /// Whether this instance's sidecar is reached over TLS.
    #[must_use]
    pub fn tls_enabled(&self) -> bool {
        self.tls.as_ref().is_some_and(|t| t.is_enabled())
    }

    /// Qualify a bare `host:port` endpoint with the scheme in use.
    ///
    /// Call sites build endpoints as `<pod-ip>:<port>`, so this is what moves
    /// the operator onto HTTPS. An endpoint that already carries a scheme is
    /// returned unchanged.
    #[must_use]
    pub fn qualify_server(&self, server: &str) -> String {
        zone_ops::build_api_url_with_scheme(server, self.tls_enabled())
    }

    /// The HTTP client to use for this instance.
    ///
    /// Returns the shared plaintext client when TLS is off. When TLS is on,
    /// builds a CA-pinned client on first call and caches it.
    ///
    /// # Errors
    /// Fails when the plaintext client could not be built (TLS off), or when
    /// TLS is enabled but the CA bundle cannot be resolved: no Kubernetes
    /// client, no instance namespace, no `caBundle` configured, the referenced
    /// object or key is missing, or the bundle does not parse.
    ///
    /// It deliberately does **not** fall back to the plaintext client on
    /// error. Doing so would send the ServiceAccount token in the clear on a
    /// deployment whose operator believes TLS is on.
    pub async fn resolve_client(&self) -> Result<Arc<HttpClient>> {
        let Some(tls) = self.tls.as_ref().filter(|t| t.is_enabled()) else {
            return self.plain_client().map(Arc::clone);
        };

        if let Some(cached) = self.tls_client.read().await.as_ref() {
            return Ok(Arc::clone(cached));
        }

        let ca_pem = self.read_ca_bundle(tls).await?;
        let built = Arc::new(crate::bind9::tls_client::build_tls_client(
            &ca_pem,
            tls.server_name.clone(),
            Duration::from_secs(BINDCAR_HTTP_CONNECT_TIMEOUT_SECS),
            Duration::from_secs(BINDCAR_HTTP_REQUEST_TIMEOUT_SECS),
        )?);

        *self.tls_client.write().await = Some(Arc::clone(&built));
        Ok(built)
    }

    /// Read the PEM CA bundle referenced by the TLS configuration.
    async fn read_ca_bundle(&self, tls: &crate::crd::BindcarTlsConfig) -> Result<Vec<u8>> {
        use k8s_openapi::api::core::v1::{ConfigMap, Secret};
        use kube::Api;

        let source = tls.ca_bundle.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "bindcarConfig.tls.enabled is true but no caBundle is configured; \
                 refusing to connect without a trust anchor"
            )
        })?;

        let kube_client = self.kube_client.clone().map(|c| c.0).ok_or_else(|| {
            anyhow::anyhow!(
                "TLS is enabled but this Bind9Manager has no Kubernetes client, \
                 so the CA bundle cannot be read"
            )
        })?;

        let namespace = self.instance_namespace.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "TLS is enabled but this Bind9Manager has no instance namespace, \
                 so the CA bundle location is unknown"
            )
        })?;

        if let Some(cm_ref) = source.config_map_ref.as_ref() {
            let key = cm_ref.key.as_deref().unwrap_or(DEFAULT_CA_BUNDLE_KEY);
            let api: Api<ConfigMap> = Api::namespaced(kube_client, namespace);
            let cm = api.get(&cm_ref.name).await.with_context(|| {
                format!(
                    "failed to read CA bundle ConfigMap {}/{}",
                    namespace, cm_ref.name
                )
            })?;
            let data = cm.data.unwrap_or_default();
            return data
                .get(key)
                .map(|v| v.clone().into_bytes())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "CA bundle ConfigMap {}/{} has no key {:?}",
                        namespace,
                        cm_ref.name,
                        key
                    )
                });
        }

        if let Some(sec_ref) = source.secret_ref.as_ref() {
            let key = sec_ref.key.as_deref().unwrap_or(DEFAULT_CA_BUNDLE_KEY);
            let api: Api<Secret> = Api::namespaced(kube_client, namespace);
            let secret = api.get(&sec_ref.name).await.with_context(|| {
                format!(
                    "failed to read CA bundle Secret {}/{}",
                    namespace, sec_ref.name
                )
            })?;
            let data = secret.data.unwrap_or_default();
            return data.get(key).map(|v| v.0.clone()).ok_or_else(|| {
                anyhow::anyhow!(
                    "CA bundle Secret {}/{} has no key {:?}",
                    namespace,
                    sec_ref.name,
                    key
                )
            });
        }

        Err(anyhow::anyhow!(
            "bindcarConfig.tls.caBundle must set either configMapRef or secretRef"
        ))
    }

    /// Read the operator's `ServiceAccount` token for bindcar authentication.
    ///
    /// bindcar `0.7.0` enforces the token audience (`status.audiences`) from the
    /// TokenReview response, so the operator must present a token minted with the
    /// `bindcar` audience. The projected audience-scoped token
    /// ([`BINDCAR_TOKEN_PATH`]) is read in preference to the default
    /// API-server-audience token ([`SERVICE_ACCOUNT_TOKEN_PATH`]), which is kept
    /// only as a backward-compatible fallback for clusters that have not yet
    /// projected the audience-scoped token.
    ///
    /// Called at request time (via [`Self::get_token`]) rather than once at
    /// construction, because the kubelet rotates the projected token file.
    fn read_service_account_token() -> Result<String> {
        match std::fs::read_to_string(BINDCAR_TOKEN_PATH) {
            Ok(token) => Ok(token),
            Err(bindcar_err) => {
                debug!(
                    path = BINDCAR_TOKEN_PATH,
                    error = %bindcar_err,
                    "bindcar-audience token not found; falling back to default ServiceAccount token"
                );
                std::fs::read_to_string(SERVICE_ACCOUNT_TOKEN_PATH)
                    .context("Failed to read ServiceAccount token file")
            }
        }
    }

    /// Check if authentication is enabled for the associated `Bind9Instance`.
    ///
    /// **Default behavior**: Returns `true` if no deployment is available (backward compat).
    ///
    /// **With deployment**: Checks if the `BIND_ALLOWED_SERVICE_ACCOUNTS` environment
    /// variable is set in the bindcar container. If the env var is present, auth is enabled.
    /// If absent, auth is disabled.
    ///
    /// # Returns
    /// * `true` - Authentication is enabled (default if no deployment info)
    /// * `false` - Authentication is explicitly disabled via env var absence
    #[must_use]
    pub fn is_auth_enabled(&self) -> bool {
        let Some(deployment) = &self.deployment else {
            // No deployment info - assume auth enabled for backward compatibility
            debug!("No deployment info available, assuming auth enabled");
            return true;
        };

        // Inspect the bindcar container's environment variables
        let pod_spec = deployment
            .spec
            .as_ref()
            .and_then(|spec| spec.template.spec.as_ref());

        let Some(pod_spec) = pod_spec else {
            warn!("Deployment has no pod template spec, assuming auth enabled");
            return true;
        };

        // Find the bindcar container (sidecar)
        let bindcar_container = pod_spec
            .containers
            .iter()
            .find(|c| c.name == BINDCAR_CONTAINER_NAME);

        let Some(bindcar_container) = bindcar_container else {
            warn!(
                container = BINDCAR_CONTAINER_NAME,
                "Deployment has no bindcar container, assuming auth enabled"
            );
            return true;
        };

        // Check if BIND_ALLOWED_SERVICE_ACCOUNTS is set (auth enabled)
        // If the env var is present, auth is enabled
        // If the env var is absent, auth is disabled
        let auth_enabled = bindcar_container
            .env
            .as_ref()
            .is_some_and(|env_vars| env_vars.iter().any(|env| env.name == BINDCAR_AUTH_ENV_VAR));

        debug!(
            instance = ?self.instance_name,
            namespace = ?self.instance_namespace,
            auth_enabled = %auth_enabled,
            env_var = BINDCAR_AUTH_ENV_VAR,
            "Checked auth status for Bind9Instance"
        );

        auth_enabled
    }

    /// Get the authentication token if available and auth is enabled.
    ///
    /// The token is read from disk at request time and cached for
    /// `TOKEN_CACHE_TTL_SECS`; a stale cache entry triggers a re-read so
    /// kubelet rotation of the projected token file is always picked up.
    ///
    /// Returns `None` if:
    /// - Auth is disabled for this instance
    /// - Token file couldn't be read
    ///
    /// This is a public method to allow external code to check auth status and get the token.
    #[must_use]
    pub fn get_token(&self) -> Option<String> {
        if !self.is_auth_enabled() {
            return None;
        }

        self.cached_or_fresh_token()
    }

    /// Return the cached token if still fresh, otherwise re-read it from disk.
    ///
    /// The negative result (no readable token file) is cached too, so an
    /// auth-disabled or out-of-cluster environment does not re-read the file
    /// on every request within the TTL. A poisoned lock is treated as a cache
    /// miss: the token is re-read from disk and returned even if the cache
    /// cannot be updated.
    fn cached_or_fresh_token(&self) -> Option<String> {
        // Fast path: serve a fresh cached token under the read lock.
        if let Ok(guard) = self.token_cache.read() {
            if let Some(cached) = guard.as_ref() {
                if is_token_cache_fresh(cached.read_at, Instant::now()) {
                    return cached.token.clone();
                }
            }
        }

        // Slow path: (re-)read the token file and refresh the cache.
        let token = Self::read_service_account_token().ok();
        if let Ok(mut guard) = self.token_cache.write() {
            *guard = Some(CachedToken {
                token: token.clone(),
                read_at: Instant::now(),
            });
        }

        token
    }

    // ===== Zone management methods =====

    /// Trigger zone transfer via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone transfer cannot be initiated.
    pub async fn retransfer_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        let client = self.resolve_client().await?;
        let server = &self.qualify_server(server);
        zone_ops::retransfer_zone(&client, self.get_token().as_deref(), zone_name, server).await
    }

    /// Get zone status via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone status cannot be retrieved.
    pub async fn zone_status(&self, zone_name: &str, server: &str) -> Result<String> {
        let client = self.resolve_client().await?;
        let server = &self.qualify_server(server);
        zone_ops::zone_status(&client, self.get_token().as_deref(), zone_name, server).await
    }

    /// Check if a zone exists by trying to get its status.
    ///
    /// Returns `Ok(true)` if the zone exists and can be queried, `Ok(false)` if the zone
    /// definitely does not exist (404), or `Err` for transient errors (rate limiting, network
    /// errors, server errors, etc.) that should be retried.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The server is rate limiting requests (429 Too Many Requests)
    /// - Network connectivity issues occur
    /// - The server returns a 5xx error
    /// - Any other non-404 error occurs
    pub async fn zone_exists(&self, zone_name: &str, server: &str) -> Result<bool> {
        let client = self.resolve_client().await?;
        let server = &self.qualify_server(server);
        zone_ops::zone_exists(&client, self.get_token().as_deref(), zone_name, server).await
    }

    /// Whether a zone is on a server and loaded (ADR-0019); see
    /// [`zone_ops::zone_presence`]. The DNS probe goes to the same pod's DNS
    /// port.
    ///
    /// # Errors
    ///
    /// Returns an error for a failure that is not absent, loaded or not
    /// loaded.
    pub async fn zone_presence(
        &self,
        zone_name: &str,
        server: &str,
    ) -> Result<zone_ops::ZonePresence> {
        let client = self.resolve_client().await?;
        let dns_server = zone_ops::dns_query_endpoint(server);
        let server = &self.qualify_server(server);
        zone_ops::zone_presence(
            &client,
            self.get_token().as_deref(),
            zone_name,
            server,
            &dns_server,
        )
        .await
    }

    /// Rewrite a primary zone's `allow-transfer` and `also-notify` in place
    /// (ADR-0019); see [`zone_ops::update_primary_transfer_peers`].
    ///
    /// # Errors
    ///
    /// Returns an error if even the `allow-transfer` rewrite fails.
    pub async fn update_primary_transfer_peers(
        &self,
        zone_name: &str,
        server: &str,
        allow_transfer: &[String],
        also_notify: &[String],
    ) -> Result<zone_ops::PeerUpdate> {
        let client = self.resolve_client().await?;
        let server = &self.qualify_server(server);
        zone_ops::update_primary_transfer_peers(
            &client,
            self.get_token().as_deref(),
            zone_name,
            server,
            allow_transfer,
            also_notify,
        )
        .await
    }

    /// Replace a secondary zone so its `primaries` are exactly
    /// `primary_ips` (ADR-0019); see [`zone_ops::replace_secondary_zone`].
    ///
    /// # Errors
    ///
    /// Returns an error if the delete or the create fails.
    pub async fn replace_secondary_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
        primary_ips: &[String],
    ) -> Result<()> {
        let client = self.resolve_client().await?;
        let server = &self.qualify_server(server);
        zone_ops::replace_secondary_zone(
            &client,
            self.get_token().as_deref(),
            zone_name,
            server,
            key_data,
            primary_ips,
        )
        .await
    }

    /// Add a zone via HTTP API (primary or secondary).
    ///
    /// This is the centralized zone addition method that dispatches to either
    /// `zone_ops::add_primary_zone` or `zone_ops::add_secondary_zone` based on the zone type.
    ///
    /// This operation is idempotent - if the zone already exists, it returns success
    /// without attempting to re-add it.
    ///
    /// # Arguments
    /// * `zone_name` - Name of the zone (e.g., "example.com")
    /// * `zone_type` - Zone type (use `ZONE_TYPE_PRIMARY` or `ZONE_TYPE_SECONDARY` constants)
    /// * `server` - API endpoint (e.g., "bind9-primary-api:8080" or "bind9-secondary-api:8080")
    /// * `key_data` - RNDC key data
    /// * `soa_record` - Optional SOA record data (required for primary zones, ignored for secondary)
    /// * `name_servers` - Optional list of ALL authoritative nameserver hostnames (for primary zones)
    /// * `name_server_ips` - Optional map of nameserver hostnames to IP addresses (for primary zones)
    /// * `secondary_ips` - Optional list of secondary pod IPs for allow-transfer (for primary zones)
    /// * `notify_targets` - Optional also-notify targets on port 53 (for primary zones, ADR-0019)
    /// * `primary_ips` - Optional list of primary server IPs to transfer from (for secondary zones)
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the zone was added, `Ok(false)` if it already existed.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be added.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_zones(
        &self,
        zone_name: &str,
        zone_type: &str,
        server: &str,
        key_data: &RndcKeyData,
        soa_record: Option<&crate::crd::SOARecord>,
        name_servers: Option<&[String]>,
        name_server_ips: Option<&HashMap<String, String>>,
        secondary_ips: Option<&[String]>,
        notify_targets: Option<&[String]>,
        primary_ips: Option<&[String]>,
        dnssec_policy: Option<&str>,
    ) -> Result<bool> {
        let token = self.get_token();
        let client = self.resolve_client().await?;
        let server = &self.qualify_server(server);
        zone_ops::add_zones(
            &client,
            token.as_deref(),
            zone_name,
            zone_type,
            server,
            key_data,
            soa_record,
            name_servers,
            name_server_ips,
            secondary_ips,
            notify_targets,
            primary_ips,
            dnssec_policy,
        )
        .await
    }

    /// Delete a zone via HTTP API. A zone that is not on the server counts as
    /// deleted; see [`zone_ops::delete_zone`].
    ///
    /// # Arguments
    /// * `zone_name` - Name of the zone to delete
    /// * `server` - API server address
    ///
    /// # Errors
    ///
    /// Returns an error if the zone is present and cannot be deleted within
    /// [`zone_ops::DELETE_RETRY_BUDGET`] per call.
    pub async fn delete_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        let client = self.resolve_client().await?;
        let server = &self.qualify_server(server);
        zone_ops::delete_zone(&client, self.get_token().as_deref(), zone_name, server).await
    }

    /// Notify secondaries about zone changes via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the notification cannot be sent.
    pub async fn notify_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        let client = self.resolve_client().await?;
        let server = &self.qualify_server(server);
        zone_ops::notify_zone(&client, self.get_token().as_deref(), zone_name, server).await
    }

    // ===== DNS record management methods =====

    /// Add A records using dynamic DNS update (RFC 2136) with `RRset` synchronization.
    ///
    /// # Arguments
    /// * `zone_name` - DNS zone name (e.g., "example.com")
    /// * `name` - Record name (e.g., "www" for www.example.com, or "@" for apex)
    /// * `ipv4_addresses` - List of IPv4 addresses for round-robin DNS
    /// * `ttl` - Time to live in seconds (None = use zone default)
    /// * `server` - DNS server address with port (e.g., "10.0.0.1:53")
    /// * `key_data` - TSIG key for authentication
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    pub async fn add_a_record(
        &self,
        zone_name: &str,
        name: &str,
        ipv4_addresses: &[String],
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::a::add_a_record(zone_name, name, ipv4_addresses, ttl, server, key_data).await
    }

    /// Add AAAA records using dynamic DNS update (RFC 2136) with `RRset` synchronization.
    ///
    /// # Arguments
    /// * `zone_name` - DNS zone name (e.g., "example.com")
    /// * `name` - Record name (e.g., "www" for www.example.com, or "@" for apex)
    /// * `ipv6_addresses` - List of IPv6 addresses for round-robin DNS
    /// * `ttl` - Time to live in seconds (None = use zone default)
    /// * `server` - DNS server address with port (e.g., "10.0.0.1:53")
    /// * `key_data` - TSIG key for authentication
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    pub async fn add_aaaa_record(
        &self,
        zone_name: &str,
        name: &str,
        ipv6_addresses: &[String],
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::a::add_aaaa_record(zone_name, name, ipv6_addresses, ttl, server, key_data).await
    }

    /// Add a CNAME record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    pub async fn add_cname_record(
        &self,
        zone_name: &str,
        name: &str,
        target: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::cname::add_cname_record(zone_name, name, target, ttl, server, key_data).await
    }

    /// Add a TXT record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    pub async fn add_txt_record(
        &self,
        zone_name: &str,
        name: &str,
        texts: &[String],
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::txt::add_txt_record(zone_name, name, texts, ttl, server, key_data).await
    }

    /// Add an MX record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_mx_record(
        &self,
        zone_name: &str,
        name: &str,
        priority: i32,
        mail_server: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::mx::add_mx_record(
            zone_name,
            name,
            priority,
            mail_server,
            ttl,
            server,
            key_data,
        )
        .await
    }

    /// Add an NS record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    pub async fn add_ns_record(
        &self,
        zone_name: &str,
        name: &str,
        nameserver: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::ns::add_ns_record(zone_name, name, nameserver, ttl, server, key_data).await
    }

    /// Add an SRV record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - DNS server connection fails
    /// - TSIG signer creation fails
    /// - DNS update is rejected by the server
    /// - Invalid domain name or target
    pub async fn add_srv_record(
        &self,
        zone_name: &str,
        name: &str,
        srv_data: &SRVRecordData,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::srv::add_srv_record(zone_name, name, srv_data, server, key_data).await
    }

    /// Add a CAA record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - DNS server connection fails
    /// - TSIG signer creation fails
    /// - DNS update is rejected by the server
    /// - Invalid domain name, flags, tag, or value
    #[allow(clippy::too_many_arguments)]
    pub async fn add_caa_record(
        &self,
        zone_name: &str,
        name: &str,
        flags: i32,
        tag: &str,
        value: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::caa::add_caa_record(zone_name, name, flags, tag, value, ttl, server, key_data)
            .await
    }

    /// Add a PTR record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - DNS server connection fails
    /// - TSIG signer creation fails
    /// - DNS update is rejected by the server
    /// - Invalid domain name or target
    pub async fn add_ptr_record(
        &self,
        zone_name: &str,
        name: &str,
        ptr_data: &PTRRecordData,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::ptr::add_ptr_record(zone_name, name, ptr_data, server, key_data).await
    }

    /// Delete a DNS record using dynamic DNS update (RFC 2136).
    ///
    /// This method deletes ALL records of the specified type for the given name.
    /// It's idempotent - deleting a non-existent record is a no-op.
    ///
    /// # Arguments
    ///
    /// * `zone_name` - The DNS zone name
    /// * `name` - The record name (e.g., "www" for www.example.com)
    /// * `record_type` - The type of record to delete (A, AAAA, CNAME, etc.)
    /// * `server` - The DNS server address (IP:port)
    /// * `key_data` - TSIG key for authentication
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS server rejects the update or connection fails.
    pub async fn delete_record(
        &self,
        zone_name: &str,
        name: &str,
        record_type: hickory_proto::rr::RecordType,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::delete_dns_record(zone_name, name, record_type, server, key_data).await
    }

    // ===== RNDC static methods (exposed through the struct for backwards compatibility) =====

    /// Generate a new RNDC key with HMAC-SHA256.
    ///
    /// Returns a base64-encoded 256-bit (32-byte) key suitable for rndc authentication.
    #[must_use]
    pub fn generate_rndc_key() -> RndcKeyData {
        rndc::generate_rndc_key()
    }

    /// Create a Kubernetes Secret manifest for an RNDC key.
    ///
    /// Returns a `BTreeMap` suitable for use as Secret data.
    #[must_use]
    pub fn create_rndc_secret_data(
        key_data: &RndcKeyData,
    ) -> std::collections::BTreeMap<String, String> {
        rndc::create_rndc_secret_data(key_data)
    }

    /// Parse RNDC key data from a Kubernetes Secret.
    ///
    /// Supports two Secret formats:
    /// 1. **Operator-generated** (all 4 fields): `key-name`, `algorithm`, `secret`, `rndc.key`
    /// 2. **External/user-managed** (minimal): `rndc.key` only - parses the BIND9 key file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Neither the metadata fields nor `rndc.key` are present
    /// - The `rndc.key` file cannot be parsed
    /// - Values are not valid UTF-8 strings
    pub fn parse_rndc_secret_data(
        data: &std::collections::BTreeMap<String, Vec<u8>>,
    ) -> Result<RndcKeyData> {
        rndc::parse_rndc_secret_data(data)
    }
}

impl Default for Bind9Manager {
    fn default() -> Self {
        Self::new()
    }
}

// Declare test modules
#[cfg(test)]
mod mod_tests;
