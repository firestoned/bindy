// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Which BIND9 instances a zone targets, and how to reach them.
//!
//! Resolving a `DNSZone`'s `bind9InstancesFrom` selectors to instances (with
//! the cross-namespace gate), and walking each instance's ready pod endpoints
//! with its RNDC key. Both the zone and the record controllers address BIND9
//! this way, so it lives below them (ADR-0009 §2, amended 2026-10-05); it
//! moved here from the zone controller's `helpers` and `validation` modules.
//!
//! # API budget (ADR-0015)
//!
//! Every write to BIND9 needs the target instance's RNDC key (a Secret) and
//! its ready pod endpoints. Reading both from the API server for every record
//! and every instance made a reconcile cost scale with records x instances, so
//! a burst of records saturated the client rate limit. Writes now go through
//! an [`InstanceResolver`], which memoizes both lookups for one reconcile, and
//! the default [`KubeInstanceLookup`] behind it reads endpoints from the
//! shared `Endpoints` reflector store and keys from a short-lived process-wide
//! [`RndcKeyCache`].

use crate::bind9::RndcKeyData;
use crate::crd::DNSZone;
use anyhow::{anyhow, Context as AnyhowContext, Result};
use k8s_openapi::api::core::v1::{Endpoints, Pod, Secret};
use kube::{Api, Client, ResourceExt};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tracing::{debug, error, warn};

/// Information about a BIND9 pod discovered during reconciliation.
#[derive(Debug, Clone)]
pub struct PodInfo {
    /// Pod name
    pub name: String,
    /// Pod IP address
    pub ip: String,
    /// Name of the Bind9Instance this pod belongs to
    pub instance_name: String,
    /// Namespace of the pod
    pub namespace: String,
}

/// Endpoint address (IP + port) for connecting to BIND9 API.
#[derive(Debug, Clone)]
pub struct EndpointAddress {
    /// IP address of the pod
    pub ip: String,
    /// Container port number
    pub port: i32,
}

/// HTTP status code for "Not Found" responses from the Kubernetes API.
pub const HTTP_STATUS_NOT_FOUND: u16 = 404;

/// How [`for_each_instance_endpoint_with_policy`] treats per-instance
/// RNDC-key and endpoint lookup failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointFailurePolicy {
    /// Propagate RNDC-key and endpoint lookup failures immediately.
    /// This is the correct behavior for normal reconciliation, where the
    /// operation must be retried until the instance becomes reachable.
    Strict,
    /// Deletion-cleanup mode: an instance whose RNDC key Secret is gone or
    /// that has no ready endpoints is skipped with a loud warning (the DNS
    /// data on it is unreachable or already gone), while real API errors
    /// (timeouts, 429, 5xx, ...) still fail the call so the next reconcile
    /// retries. This prevents resources from being stuck Terminating behind
    /// an instance that can never be cleaned up.
    SkipUnavailable,
}

/// Classifies an error from `load_rndc_key`/`get_endpoint` during DELETION cleanup.
///
/// Returns `true` when the failure means the lookup target is gone or there is
/// nothing to operate on (safe to skip during deletion):
/// - a Kubernetes 404 (Secret or Endpoints object missing), or
/// - a non-Kubernetes error (e.g. "no ready endpoints found", malformed
///   Secret data) - conditions that will not be fixed by retrying the delete.
///
/// Returns `false` for any other Kubernetes API error (timeout, 429, 5xx, ...)
/// which is potentially transient and must be retried instead of skipped.
#[must_use]
pub fn is_unavailable_for_deletion(err: &anyhow::Error) -> bool {
    match err.downcast_ref::<kube::Error>() {
        Some(kube::Error::Api(ae)) => ae.code == HTTP_STATUS_NOT_FOUND,
        Some(_) => false,
        None => true,
    }
}

//
// ============================================================
// Endpoint and Instance Utilities
// ============================================================
//

/// Execute an operation on all endpoints for a list of instance references.
///
/// This is the event-driven instance-based approach that operates on instances
/// discovered via spec.bind9InstancesFrom selectors. Each instance's RNDC key
/// and endpoints come from `resolver`, so a caller that writes many records in
/// one reconcile reads them once per instance (ADR-0015).
///
/// # Arguments
///
/// * `resolver` - Per-reconcile resolver for instance keys and endpoints
/// * `instance_refs` - List of instance references to process
/// * `with_rndc_key` - Whether to load and pass RNDC keys for each instance
/// * `port_name` - Port name to use for endpoints (e.g., "rndc-api", "dns-tcp")
/// * `operation` - Async closure to execute for each endpoint
///
/// # Returns
///
/// * `Ok((first_endpoint, total_endpoints))` - First endpoint found and total count
///
/// # Errors
///
/// Returns an error if all operations fail or if critical API calls fail.
pub async fn for_each_instance_endpoint<F, Fut>(
    resolver: &InstanceResolver,
    instance_refs: &[crate::crd::InstanceReference],
    with_rndc_key: bool,
    port_name: &str,
    operation: F,
) -> Result<(Option<String>, usize)>
where
    F: Fn(String, String, Option<RndcKeyData>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    for_each_instance_endpoint_with_policy(
        resolver,
        instance_refs,
        with_rndc_key,
        port_name,
        EndpointFailurePolicy::Strict,
        operation,
    )
    .await
}

/// Execute an operation on all endpoints for a list of instance references,
/// with an explicit failure policy for per-instance lookups.
///
/// Same as [`for_each_instance_endpoint`], but the caller chooses how to treat
/// RNDC-key and endpoint lookup failures (see [`EndpointFailurePolicy`]).
/// Deletion cleanup paths should use [`EndpointFailurePolicy::SkipUnavailable`]
/// so that a missing RNDC Secret or an instance with zero ready endpoints does
/// not block finalizer removal forever.
///
/// When an operation fails on any endpoint of an instance, that instance's
/// RNDC key is forgotten (see [`InstanceResolver::forget_rndc_key`]), so a
/// rotated key is re-read on the next write instead of being reused.
///
/// # Arguments
///
/// * `resolver` - Per-reconcile resolver for instance keys and endpoints
/// * `instance_refs` - List of instance references to process
/// * `with_rndc_key` - Whether to load and pass RNDC keys for each instance
/// * `port_name` - Port name to use for endpoints (e.g., "rndc-api", "dns-tcp")
/// * `policy` - How to treat per-instance RNDC-key/endpoint lookup failures
/// * `operation` - Async closure to execute for each endpoint
///
/// # Returns
///
/// * `Ok((first_endpoint, total_endpoints))` - First endpoint found and total count
///
/// # Errors
///
/// Returns an error if all operations fail, or if RNDC-key/endpoint lookups
/// fail and the policy does not allow skipping them.
pub async fn for_each_instance_endpoint_with_policy<F, Fut>(
    resolver: &InstanceResolver,
    instance_refs: &[crate::crd::InstanceReference],
    with_rndc_key: bool,
    port_name: &str,
    policy: EndpointFailurePolicy,
    operation: F,
) -> Result<(Option<String>, usize)>
where
    F: Fn(String, String, Option<RndcKeyData>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut first_endpoint: Option<String> = None;
    let mut total_endpoints = 0;
    let mut errors: Vec<String> = Vec::new();

    for instance_ref in instance_refs {
        debug!(
            "Processing endpoints for instance {}/{}",
            instance_ref.namespace, instance_ref.name
        );

        // Load RNDC key for this specific instance if requested
        let key_data = if with_rndc_key {
            match resolver
                .rndc_key(&instance_ref.namespace, &instance_ref.name)
                .await
            {
                Ok(key) => Some(key),
                Err(e)
                    if policy == EndpointFailurePolicy::SkipUnavailable
                        && is_unavailable_for_deletion(&e) =>
                {
                    warn!(
                        "SKIPPING instance {}/{} during deletion cleanup: RNDC key unavailable ({e:#}). \
                         DNS data on this instance cannot be cleaned up and may be orphaned.",
                        instance_ref.namespace, instance_ref.name
                    );
                    continue;
                }
                Err(e) => return Err(e),
            }
        } else {
            None
        };

        // Get all endpoints for this instance's service
        let endpoints = match resolver
            .endpoints(&instance_ref.namespace, &instance_ref.name, port_name)
            .await
        {
            Ok(eps) => eps,
            Err(e)
                if policy == EndpointFailurePolicy::SkipUnavailable
                    && is_unavailable_for_deletion(&e) =>
            {
                warn!(
                    "SKIPPING instance {}/{} during deletion cleanup: no reachable endpoints ({e:#}). \
                     DNS data on this instance cannot be cleaned up and may be orphaned.",
                    instance_ref.namespace, instance_ref.name
                );
                continue;
            }
            Err(e) => return Err(e),
        };

        debug!(
            "Found {} endpoint(s) for instance {}/{}",
            endpoints.len(),
            instance_ref.namespace,
            instance_ref.name
        );

        let mut instance_failed = false;
        for endpoint in &endpoints {
            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

            // Save the first endpoint
            if first_endpoint.is_none() {
                first_endpoint = Some(pod_endpoint.clone());
            }

            // Execute the operation on this endpoint
            if let Err(e) = operation(
                pod_endpoint.clone(),
                instance_ref.name.clone(),
                key_data.clone(),
            )
            .await
            {
                // `{e:#}` renders the whole anyhow context chain. Plain `{e}`
                // shows only the outermost context, which hid the reason BIND9
                // rejected an update behind "Failed to add MX record ...".
                error!(
                    "Failed operation on endpoint {} (instance {}/{}): {e:#}",
                    pod_endpoint, instance_ref.namespace, instance_ref.name
                );
                errors.push(format!(
                    "endpoint {pod_endpoint} (instance {}/{}): {e:#}",
                    instance_ref.namespace, instance_ref.name
                ));
                instance_failed = true;
            } else {
                total_endpoints += 1;
            }
        }

        // A key that was just rejected may have been rotated: re-read it on
        // the next write rather than reuse it until the cache TTL runs out.
        if instance_failed && with_rndc_key {
            resolver.forget_rndc_key(&instance_ref.namespace, &instance_ref.name);
        }
    }

    // If ALL operations failed, return an error
    if total_endpoints == 0 && !errors.is_empty() {
        return Err(anyhow!(
            "All operations failed. Errors: {}",
            errors.join("; ")
        ));
    }

    Ok((first_endpoint, total_endpoints))
}

//
// ============================================================
// Per-reconcile resolution of instance keys and endpoints (ADR-0015)
// ============================================================
//

/// Seconds a loaded RNDC key is reused across reconciles before its Secret is
/// read again.
const RNDC_KEY_CACHE_TTL_SECS: u64 = 60;

/// How long a loaded RNDC key is reused across reconciles (ADR-0015).
///
/// Short on purpose: the operator rotates keys itself (and invalidates the
/// entry when it does), and a write that fails drops the entry at once, so
/// the TTL only bounds how stale a key changed by someone else can get.
pub const RNDC_KEY_CACHE_TTL: Duration = Duration::from_secs(RNDC_KEY_CACHE_TTL_SECS);

/// Boxed future returned by the [`InstanceLookup`] methods.
pub type LookupFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// Where an [`InstanceResolver`] gets an instance's RNDC key and endpoints.
///
/// The production implementation is [`KubeInstanceLookup`]; tests substitute
/// a counting fake to pin how many lookups a reconcile performs.
pub trait InstanceLookup: Send + Sync {
    /// Load the RNDC key of `instance_name` in `namespace`.
    fn rndc_key<'a>(
        &'a self,
        namespace: &'a str,
        instance_name: &'a str,
    ) -> LookupFuture<'a, RndcKeyData>;

    /// The ready endpoints of `service_name` in `namespace` for `port_name`.
    fn endpoints<'a>(
        &'a self,
        namespace: &'a str,
        service_name: &'a str,
        port_name: &'a str,
    ) -> LookupFuture<'a, Vec<EndpointAddress>>;

    /// Drop any longer-lived copy of the instance's RNDC key. Called after a
    /// write with that key failed. The default does nothing.
    fn forget_rndc_key(&self, _namespace: &str, _instance_name: &str) {}
}

/// The production [`InstanceLookup`]: endpoints from the shared `Endpoints`
/// reflector store (falling back to a GET for an object the store does not
/// hold), RNDC keys through the process-wide [`RndcKeyCache`].
pub struct KubeInstanceLookup {
    client: Client,
    endpoints: Option<crate::context::MultiStore<Endpoints>>,
}

impl KubeInstanceLookup {
    /// Build a lookup over `client`, reading endpoints from `endpoints` when
    /// given.
    ///
    /// # Arguments
    ///
    /// * `client` - Kubernetes API client for Secret reads and fallback GETs
    /// * `endpoints` - The shared `Endpoints` reflector store, if available
    #[must_use]
    pub fn new(client: Client, endpoints: Option<crate::context::MultiStore<Endpoints>>) -> Self {
        Self { client, endpoints }
    }
}

impl InstanceLookup for KubeInstanceLookup {
    fn rndc_key<'a>(
        &'a self,
        namespace: &'a str,
        instance_name: &'a str,
    ) -> LookupFuture<'a, RndcKeyData> {
        Box::pin(load_rndc_key_cached(&self.client, namespace, instance_name))
    }

    fn endpoints<'a>(
        &'a self,
        namespace: &'a str,
        service_name: &'a str,
        port_name: &'a str,
    ) -> LookupFuture<'a, Vec<EndpointAddress>> {
        Box::pin(async move {
            let cached = self
                .endpoints
                .as_ref()
                .and_then(|store| cached_endpoints(store, namespace, service_name, port_name));
            let Some(addresses) = cached else {
                return get_endpoint(&self.client, namespace, service_name, port_name).await;
            };
            if addresses.is_empty() {
                return Err(no_ready_endpoints_error(service_name, port_name));
            }
            Ok(addresses)
        })
    }

    fn forget_rndc_key(&self, namespace: &str, instance_name: &str) {
        invalidate_cached_rndc_key(namespace, instance_name);
    }
}

/// Resolves instance RNDC keys and endpoints, memoized for one reconcile.
///
/// Build one per reconcile and pass it to every write that reconcile makes:
/// each instance's key and each `(instance, port)`'s endpoints are then looked
/// up once, however many records are written. Only successful lookups are
/// remembered; a failed one is retried on the next call.
pub struct InstanceResolver {
    lookup: Box<dyn InstanceLookup>,
    keys: Mutex<HashMap<(String, String), RndcKeyData>>,
    endpoints: Mutex<HashMap<(String, String, String), Vec<EndpointAddress>>>,
}

impl InstanceResolver {
    /// A resolver over an arbitrary lookup.
    ///
    /// # Arguments
    ///
    /// * `lookup` - Where keys and endpoints come from on a memo miss
    #[must_use]
    pub fn new(lookup: impl InstanceLookup + 'static) -> Self {
        Self {
            lookup: Box::new(lookup),
            keys: Mutex::new(HashMap::new()),
            endpoints: Mutex::new(HashMap::new()),
        }
    }

    /// The production resolver: a [`KubeInstanceLookup`] over `client` and
    /// the shared `Endpoints` store in `stores`.
    ///
    /// # Arguments
    ///
    /// * `client` - Kubernetes API client
    /// * `stores` - The shared reflector stores
    #[must_use]
    pub fn for_kube(client: &Client, stores: &crate::context::Stores) -> Self {
        Self::new(KubeInstanceLookup::new(
            client.clone(),
            Some(stores.endpoints.clone()),
        ))
    }

    /// The RNDC key of an instance, looked up at most once per resolver.
    ///
    /// # Errors
    ///
    /// Returns the lookup's error when the key is not memoized and cannot be
    /// loaded.
    pub async fn rndc_key(&self, namespace: &str, instance_name: &str) -> Result<RndcKeyData> {
        let id = (namespace.to_string(), instance_name.to_string());
        if let Some(key) = lock(&self.keys).get(&id) {
            return Ok(key.clone());
        }
        let key = self.lookup.rndc_key(namespace, instance_name).await?;
        lock(&self.keys).insert(id, key.clone());
        Ok(key)
    }

    /// The ready endpoints of an instance's Service for one port, looked up at
    /// most once per resolver.
    ///
    /// # Errors
    ///
    /// Returns the lookup's error when the endpoints are not memoized and
    /// cannot be resolved (including when no pod is ready).
    pub async fn endpoints(
        &self,
        namespace: &str,
        service_name: &str,
        port_name: &str,
    ) -> Result<Vec<EndpointAddress>> {
        let id = (
            namespace.to_string(),
            service_name.to_string(),
            port_name.to_string(),
        );
        if let Some(addresses) = lock(&self.endpoints).get(&id) {
            return Ok(addresses.clone());
        }
        let addresses = self
            .lookup
            .endpoints(namespace, service_name, port_name)
            .await?;
        lock(&self.endpoints).insert(id, addresses.clone());
        Ok(addresses)
    }

    /// Forget an instance's RNDC key, here and in the lookup's longer-lived
    /// cache, so the next write re-reads it.
    ///
    /// # Arguments
    ///
    /// * `namespace` - Namespace of the instance
    /// * `instance_name` - Name of the instance
    pub fn forget_rndc_key(&self, namespace: &str, instance_name: &str) {
        lock(&self.keys).remove(&(namespace.to_string(), instance_name.to_string()));
        self.lookup.forget_rndc_key(namespace, instance_name);
    }
}

/// Lock a memo map, recovering the data if a panicking thread poisoned it: the
/// maps hold plain values, so a poisoned one is still consistent.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// RNDC keys loaded from their Secrets, reused across reconciles for
/// [`RNDC_KEY_CACHE_TTL`] (ADR-0015).
///
/// Keyed by instance namespace and name. The operator holds `get` on these
/// Secrets already; caching changes how often they are read, not who can read
/// them, and no Secret list or watch is involved.
#[derive(Default)]
pub struct RndcKeyCache {
    entries: Mutex<HashMap<(String, String), (RndcKeyData, Instant)>>,
}

impl RndcKeyCache {
    /// The cached key, if one was loaded less than [`RNDC_KEY_CACHE_TTL`]
    /// before `now`.
    #[must_use]
    pub fn get_at(
        &self,
        namespace: &str,
        instance_name: &str,
        now: Instant,
    ) -> Option<RndcKeyData> {
        let entries = lock(&self.entries);
        let (key, loaded_at) = entries.get(&(namespace.to_string(), instance_name.to_string()))?;
        if now.saturating_duration_since(*loaded_at) >= RNDC_KEY_CACHE_TTL {
            return None;
        }
        Some(key.clone())
    }

    /// Remember `key`, loaded at `loaded_at`.
    pub fn insert_at(
        &self,
        namespace: &str,
        instance_name: &str,
        key: RndcKeyData,
        loaded_at: Instant,
    ) {
        lock(&self.entries).insert(
            (namespace.to_string(), instance_name.to_string()),
            (key, loaded_at),
        );
    }

    /// Drop the cached key of an instance.
    pub fn invalidate(&self, namespace: &str, instance_name: &str) {
        lock(&self.entries).remove(&(namespace.to_string(), instance_name.to_string()));
    }
}

/// The process-wide RNDC key cache behind [`load_rndc_key_cached`].
static RNDC_KEY_CACHE: LazyLock<RndcKeyCache> = LazyLock::new(RndcKeyCache::default);

/// Load an instance's RNDC key, from the process-wide cache when a copy less
/// than [`RNDC_KEY_CACHE_TTL`] old is held, otherwise from its Secret.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the instance
/// * `instance_name` - Name of the instance
///
/// # Errors
///
/// Returns an error if the key is not cached and the Secret is missing or
/// cannot be parsed.
pub async fn load_rndc_key_cached(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<RndcKeyData> {
    if let Some(key) = RNDC_KEY_CACHE.get_at(namespace, instance_name, Instant::now()) {
        return Ok(key);
    }
    let key = load_rndc_key(client, namespace, instance_name).await?;
    RNDC_KEY_CACHE.insert_at(namespace, instance_name, key.clone(), Instant::now());
    Ok(key)
}

/// Drop an instance's RNDC key from the process-wide cache.
///
/// Called when the operator rotates the key, and after a write with the key
/// failed, so the next write reads the Secret again.
///
/// # Arguments
///
/// * `namespace` - Namespace of the instance
/// * `instance_name` - Name of the instance
pub fn invalidate_cached_rndc_key(namespace: &str, instance_name: &str) {
    RNDC_KEY_CACHE.invalidate(namespace, instance_name);
}

/// The ready endpoints of a Service as recorded in the `Endpoints` store.
///
/// # Arguments
///
/// * `store` - The shared `Endpoints` reflector store
/// * `namespace` - Namespace of the Service
/// * `service_name` - Name of the Service (the instance name)
/// * `port_name` - Name of the port to read
///
/// # Returns
///
/// `None` when the store does not hold the object (the caller should fall back
/// to a GET), otherwise its ready addresses for `port_name`, possibly empty.
#[must_use]
pub fn cached_endpoints(
    store: &crate::context::MultiStore<Endpoints>,
    namespace: &str,
    service_name: &str,
    port_name: &str,
) -> Option<Vec<EndpointAddress>> {
    store
        .state()
        .iter()
        .find(|ep| {
            ep.metadata.name.as_deref() == Some(service_name)
                && ep.metadata.namespace.as_deref() == Some(namespace)
        })
        .map(|ep| ready_endpoint_addresses(ep, port_name))
}

/// The ready addresses of an `Endpoints` object for the port named
/// `port_name`.
///
/// Endpoints are organized into subsets. Each subset has `addresses` (ready
/// pod IPs) and `ports` (container ports); only subsets that expose
/// `port_name` contribute.
#[must_use]
pub fn ready_endpoint_addresses(endpoints: &Endpoints, port_name: &str) -> Vec<EndpointAddress> {
    let mut result = Vec::new();
    for subset in endpoints.subsets.iter().flatten() {
        let Some(endpoint_port) = subset
            .ports
            .iter()
            .flatten()
            .find(|p| p.name.as_deref() == Some(port_name))
        else {
            continue;
        };
        for addr in subset.addresses.iter().flatten() {
            result.push(EndpointAddress {
                ip: addr.ip.clone(),
                port: endpoint_port.port,
            });
        }
    }
    result
}

/// The error reported when a Service has no ready address on `port_name`.
fn no_ready_endpoints_error(service_name: &str, port_name: &str) -> anyhow::Error {
    anyhow!("No ready endpoints found for service {service_name} with port '{port_name}'")
}

/// Extract the name and IP of a pod that is Running and has an IP assigned.
///
/// Used when listing BIND9 pods for zone operations: pods that are not yet
/// Running, or that are so freshly scheduled they have no IP, must be SKIPPED
/// rather than failing the entire pod listing - a single Pending pod must not
/// abort operations against the healthy pods.
///
/// # Arguments
///
/// * `pod` - The pod to inspect
///
/// # Returns
///
/// `Some((name, ip))` if the pod is Running with an IP, `None` otherwise.
#[must_use]
pub fn running_pod_name_and_ip(pod: &Pod) -> Option<(String, String)> {
    let pod_name = pod.metadata.name.as_deref().unwrap_or("unknown");

    let phase = pod
        .status
        .as_ref()
        .and_then(|s| s.phase.as_deref())
        .unwrap_or("Unknown");
    if phase != "Running" {
        tracing::debug!("Skipping pod {} (phase: {}, not running)", pod_name, phase);
        return None;
    }

    let Some(pod_ip) = pod.status.as_ref().and_then(|s| s.pod_ip.as_ref()) else {
        tracing::debug!(
            "Skipping pod {} without an IP address (likely just scheduled)",
            pod_name
        );
        return None;
    };

    Some((pod_name.to_string(), pod_ip.clone()))
}

/// Load RNDC key from the instance's secret.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the instance
/// * `instance_name` - Name of the instance
///
/// # Returns
///
/// Parsed RNDC key data
///
/// # Errors
///
/// Returns an error if the secret is not found or cannot be parsed
pub async fn load_rndc_key(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<RndcKeyData> {
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret_name = format!("{instance_name}-rndc-key");

    let secret = secret_api.get(&secret_name).await.context(format!(
        "Failed to get RNDC secret {secret_name} in namespace {namespace}"
    ))?;

    let data = secret
        .data
        .as_ref()
        .ok_or_else(|| anyhow!("Secret {secret_name} has no data"))?;

    // Convert ByteString to Vec<u8>
    let mut converted_data = std::collections::BTreeMap::new();
    for (key, value) in data {
        converted_data.insert(key.clone(), value.0.clone());
    }

    crate::bind9::Bind9Manager::parse_rndc_secret_data(&converted_data)
}

/// Get all ready endpoints for a service.
///
/// Queries the Kubernetes Endpoints API to find all ready pod IPs and ports
/// for a given service. The port_name must match the name field in the
/// service's port specification.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the service
/// * `service_name` - Name of the service (usually same as instance name)
/// * `port_name` - Name of the port to query (e.g., "rndc-api", "dns-tcp")
///
/// # Returns
///
/// Vector of endpoint addresses with IP and port
///
/// # Errors
///
/// Returns an error if:
/// - Failed to get endpoints from API
/// - No ready addresses found
pub async fn get_endpoint(
    client: &Client,
    namespace: &str,
    service_name: &str,
    port_name: &str,
) -> Result<Vec<EndpointAddress>> {
    let endpoints_api: Api<Endpoints> = Api::namespaced(client.clone(), namespace);
    let endpoints = endpoints_api.get(service_name).await.context(format!(
        "Failed to get endpoints for service {service_name}"
    ))?;

    let result = ready_endpoint_addresses(&endpoints, port_name);
    if result.is_empty() {
        return Err(no_ready_endpoints_error(service_name, port_name));
    }

    Ok(result)
}

/// Get instances from a DNSZone based on `bind9_instances_from` selectors.
///
/// This function:
/// - Uses the reflector store for O(1) lookups without API calls
/// - Single source of truth: `DNSZone` owns the zone-instance relationship
///
/// # F-003 mitigation: cross-namespace targeting requires platform-admin opt-in
///
/// A label selector match is *not* sufficient to enrol a `Bind9Instance` in
/// the zone. The instance is included only when **either**:
///
/// 1. The instance lives in the **same namespace** as the `DNSZone`, **or**
/// 2. The instance carries the
///    [`crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES`] annotation
///    whose value contains the zone's namespace (or the wildcard
///    [`crate::constants::ALLOW_ZONE_NAMESPACES_WILDCARD`]).
///
/// The annotation is metadata on the `Bind9Instance`, which is owned by
/// the platform admin (only they have RBAC on the namespace where the
/// instance lives). This preserves the cluster-wide-operator contract:
/// the platform admin keeps full control of who can claim their
/// instances, expressed through a platform-admin-controlled annotation,
/// while still preventing the F-003 hijack — labels on the instance side
/// are not a security boundary (they are discoverable via list/watch and
/// any tenant can write any matchLabels they want), but annotations on
/// the platform-owned instance are.
///
/// # Arguments
///
/// * `dnszone` - The `DNSZone` resource to get instances for
/// * `bind9_instances_store` - Reflector store of `Bind9Instance`
///
/// # Returns
///
/// * `Ok(Vec<InstanceReference>)` - List of instances serving this zone
/// * `Err(_)` - If no instances pass both the selector match and the
///   namespace gate
///
/// # Errors
///
/// Returns an error if no instances pass the selector + namespace gate, or
/// if `spec.bind9_instances_from` is missing or empty.
pub fn get_instances_from_zone(
    dnszone: &DNSZone,
    bind9_instances_store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
) -> Result<Vec<crate::crd::InstanceReference>> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();

    // Get bind9_instances_from selectors from zone spec
    let bind9_instances_from = match &dnszone.spec.bind9_instances_from {
        Some(sources) if !sources.is_empty() => sources,
        _ => {
            return Err(anyhow!(
                "DNSZone {namespace}/{name} has no bind9_instances_from selectors configured. \
                Add spec.bind9_instances_from[] with label selectors to target Bind9Instance resources."
            ));
        }
    };

    let mut cross_ns_denied: Vec<(String, String)> = Vec::new();
    let instances_with_zone: Vec<crate::crd::InstanceReference> = bind9_instances_store
        .state()
        .iter()
        .filter_map(|instance| {
            let instance_labels = instance.metadata.labels.as_ref()?;
            let instance_namespace = instance.namespace()?;
            let instance_name = instance.name_any();

            // Selector match (label-based) — necessary but not sufficient.
            let matches = bind9_instances_from
                .iter()
                .any(|source| source.selector.matches(instance_labels));
            if !matches {
                return None;
            }

            // F-003 namespace gate. Same-namespace always allowed; cross-
            // namespace requires the platform-admin annotation on the
            // instance.
            if instance_namespace != namespace
                && !instance_allows_zone_namespace(instance, &namespace)
            {
                cross_ns_denied.push((instance_namespace.clone(), instance_name.clone()));
                return None;
            }

            Some(crate::crd::InstanceReference {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: instance_name,
                namespace: instance_namespace,
                last_reconciled_at: None,
            })
        })
        .collect();

    if !cross_ns_denied.is_empty() {
        warn!(
            "DNSZone {}/{} label selectors matched {} cross-namespace Bind9Instance(s) \
             that were rejected by the F-003 namespace gate: {:?}. \
             To allow cross-namespace targeting, the platform admin must annotate the \
             target Bind9Instance with `{}: <comma-separated namespaces>` (or `*`).",
            namespace,
            name,
            cross_ns_denied.len(),
            cross_ns_denied,
            crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES,
        );
    }

    if !instances_with_zone.is_empty() {
        debug!(
            "DNSZone {}/{} matched {} instances via spec.bind9_instances_from selectors",
            namespace,
            name,
            instances_with_zone.len()
        );
        return Ok(instances_with_zone);
    }

    // No instances found — message distinguishes "no labels matched" from
    // "labels matched but cross-namespace gate denied them".
    if cross_ns_denied.is_empty() {
        Err(anyhow!(
            "DNSZone {namespace}/{name} has no instances matching spec.bind9_instances_from selectors. \
            Verify that Bind9Instance resources exist with matching labels."
        ))
    } else {
        Err(anyhow!(
            "DNSZone {namespace}/{name} matched only cross-namespace Bind9Instance(s) \
             that the F-003 namespace gate denied. Ask the platform admin to annotate \
             the target instance with `{annotation}: {namespace}` (or `{annotation}: *` \
             to allow any namespace).",
            annotation = crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES,
        ))
    }
}

/// Check whether `instance` carries an annotation that grants the named
/// `zone_namespace` permission to target it cross-namespace.
///
/// Returns `true` iff the instance's
/// [`crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES`] annotation is
/// set and its value, when parsed as a comma-separated list, contains
/// either `zone_namespace` or
/// [`crate::constants::ALLOW_ZONE_NAMESPACES_WILDCARD`].
///
/// Same-namespace matching is handled by the caller and does *not*
/// require this annotation.
#[must_use]
pub fn instance_allows_zone_namespace(
    instance: &crate::crd::Bind9Instance,
    zone_namespace: &str,
) -> bool {
    let Some(annotations) = instance.metadata.annotations.as_ref() else {
        return false;
    };
    let Some(value) = annotations.get(crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES) else {
        return false;
    };
    value.split(',').map(str::trim).any(|entry| {
        entry == crate::constants::ALLOW_ZONE_NAMESPACES_WILDCARD || entry == zone_namespace
    })
}

#[cfg(test)]
#[path = "instances_tests.rs"]
mod instances_tests;
