// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Which BIND9 instances a zone targets, and how to reach them.
//!
//! Resolving a `DNSZone`'s `bind9InstancesFrom` selectors to instances (with
//! the cross-namespace gate), and walking each instance's ready pod endpoints
//! with its RNDC key. Both the zone and the record controllers address BIND9
//! this way, so it lives below them (ADR-0009 §2, amended 2026-10-05); it
//! moved here from the zone controller's `helpers` and `validation` modules.

use crate::bind9::RndcKeyData;
use crate::crd::DNSZone;
use anyhow::{anyhow, Context as AnyhowContext, Result};
use k8s_openapi::api::core::v1::{Endpoints, Pod, Secret};
use kube::{Api, Client, ResourceExt};
use tracing::{debug, error, info, warn};

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
/// discovered via spec.bind9InstancesFrom selectors.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
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
    client: &Client,
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
        client,
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
/// # Arguments
///
/// * `client` - Kubernetes API client
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
    client: &Client,
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
        info!(
            "Processing endpoints for instance {}/{}",
            instance_ref.namespace, instance_ref.name
        );

        // Load RNDC key for this specific instance if requested
        let key_data = if with_rndc_key {
            match load_rndc_key(client, &instance_ref.namespace, &instance_ref.name).await {
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
        let endpoints = match get_endpoint(
            client,
            &instance_ref.namespace,
            &instance_ref.name,
            port_name,
        )
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

        info!(
            "Found {} endpoint(s) for instance {}/{}",
            endpoints.len(),
            instance_ref.namespace,
            instance_ref.name
        );

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
            } else {
                total_endpoints += 1;
            }
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

    let mut result = Vec::new();

    // Endpoints are organized into subsets. Each subset has:
    // - addresses: List of ready pod IPs
    // - ports: List of container ports
    if let Some(subsets) = endpoints.subsets {
        for subset in subsets {
            // Find the port in this subset
            if let Some(ports) = subset.ports {
                if let Some(endpoint_port) = ports
                    .iter()
                    .find(|p| p.name.as_ref().is_some_and(|name| name == port_name))
                {
                    let port = endpoint_port.port;

                    // Get all ready addresses for this subset
                    if let Some(addresses) = subset.addresses {
                        for addr in addresses {
                            result.push(EndpointAddress {
                                ip: addr.ip.clone(),
                                port,
                            });
                        }
                    }
                }
            }
        }
    }

    if result.is_empty() {
        return Err(anyhow!(
            "No ready endpoints found for service {service_name} with port '{port_name}'"
        ));
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
