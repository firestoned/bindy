// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Primary zone instance operations.
//!
//! This module handles all operations specific to PRIMARY BIND9 instances,
//! including:
//! - Filtering instance references to only primary instances
//! - Finding primary pods across instances
//! - Collecting primary pod IPs
//! - Executing operations on all primary endpoints

use anyhow::{anyhow, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::ListParams, Api, Client};
use tracing::{debug, error, warn};

use crate::bind9::RndcKeyData;
use crate::instances::PodInfo;

/// Filters a list of instance references to only PRIMARY instances.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// Vector of instance references that have role=Primary
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail.
pub async fn filter_primary_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    use crate::crd::{Bind9Instance, ServerRole};

    let mut primary_refs = Vec::new();

    for instance_ref in instance_refs {
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        match instance_api.get(&instance_ref.name).await {
            Ok(instance) => {
                if instance.spec.role == ServerRole::Primary {
                    primary_refs.push(instance_ref.clone());
                }
            }
            Err(e) => {
                warn!(
                    "Failed to get instance {}/{}: {}. Skipping.",
                    instance_ref.namespace, instance_ref.name, e
                );
            }
        }
    }

    Ok(primary_refs)
}

/// The role of the instance behind `instance_ref`, according to the
/// `Bind9Instance` reflector store.
///
/// # Arguments
///
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_ref` - The instance to look up
///
/// # Returns
///
/// The instance's `spec.role`, or `None` when the store does not hold the
/// instance (the caller decides whether to fall back to the API server).
#[must_use]
pub fn instance_role_in_store(
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_ref: &crate::crd::InstanceReference,
) -> Option<crate::crd::ServerRole> {
    use kube::ResourceExt;
    store
        .state()
        .iter()
        .find(|instance| {
            instance.name_any() == instance_ref.name
                && instance.namespace().as_deref() == Some(instance_ref.namespace.as_str())
        })
        .map(|instance| instance.spec.role)
}

/// Whether the instance behind `instance_ref` is a PRIMARY, according to the
/// `Bind9Instance` reflector store.
///
/// # Arguments
///
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_ref` - The instance to look up
///
/// # Returns
///
/// `Some(true)` for a primary, `Some(false)` for any other role, and `None`
/// when the store does not hold the instance (the caller decides whether to
/// fall back to the API server).
#[must_use]
pub fn primary_role_in_store(
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_ref: &crate::crd::InstanceReference,
) -> Option<bool> {
    instance_role_in_store(store, instance_ref).map(|role| role == crate::crd::ServerRole::Primary)
}

/// The role of an instance: from the reflector store, or with a GET for an
/// instance the store does not hold yet.
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_ref` - The instance to look up
///
/// # Returns
///
/// The instance's role, or `None` (with a warning) when it is neither cached
/// nor readable.
pub async fn instance_role_cached(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_ref: &crate::crd::InstanceReference,
) -> Option<crate::crd::ServerRole> {
    if let Some(role) = instance_role_in_store(store, instance_ref) {
        return Some(role);
    }
    let api: Api<crate::crd::Bind9Instance> =
        Api::namespaced(client.clone(), &instance_ref.namespace);
    match api.get(&instance_ref.name).await {
        Ok(instance) => Some(instance.spec.role),
        Err(e) => {
            warn!(
                "Failed to get instance {}/{}: {}. Skipping.",
                instance_ref.namespace, instance_ref.name, e
            );
            None
        }
    }
}

/// Filters instance references to those with `role`, reading each role from
/// the `Bind9Instance` reflector store (ADR-0015, ADR-0016).
///
/// An instance the store holds costs no API call; one it does not hold yet
/// is read with a GET and skipped, with a warning, if that fails.
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_refs` - Instance references to filter
/// * `role` - The role to keep
///
/// # Returns
///
/// The references whose instance has `role`, in input order.
pub async fn filter_instances_by_role_cached(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[crate::crd::InstanceReference],
    role: &crate::crd::ServerRole,
) -> Vec<crate::crd::InstanceReference> {
    let mut matching = Vec::new();
    for instance_ref in instance_refs {
        if instance_role_cached(client, store, instance_ref)
            .await
            .as_ref()
            == Some(role)
        {
            matching.push(instance_ref.clone());
        }
    }
    matching
}

/// Filters instance references to PRIMARY instances, reading each role from
/// the `Bind9Instance` reflector store (ADR-0015).
///
/// Same result as [`filter_primary_instances`], but an instance the store
/// holds costs no API call, so a record write no longer spends one GET per
/// instance. An instance the store does not hold yet (a watch that has not
/// caught up) falls back to a GET, and is skipped with a warning if that
/// fails, exactly as [`filter_primary_instances`] does.
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// The references whose instance has role=Primary.
///
/// # Errors
///
/// Does not currently fail; the `Result` keeps the signature of
/// [`filter_primary_instances`].
pub async fn filter_primary_instances_cached(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    let mut primary_refs = Vec::new();
    let mut uncached = Vec::new();

    for instance_ref in instance_refs {
        match primary_role_in_store(store, instance_ref) {
            Some(true) => primary_refs.push(instance_ref.clone()),
            Some(false) => {}
            None => uncached.push(instance_ref.clone()),
        }
    }

    if !uncached.is_empty() {
        debug!(
            "{} instance(s) not in the Bind9Instance store yet, reading their role from the API",
            uncached.len()
        );
        primary_refs.extend(filter_primary_instances(client, &uncached).await?);
    }

    Ok(primary_refs)
}

/// Find all PRIMARY pods for a given cluster or cluster provider.
///
/// Returns pod information including name, IP, instance name, and namespace
/// for all running PRIMARY pods in the cluster.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search in (if not cluster provider)
/// * `cluster_name` - Name of the cluster
/// * `is_cluster_provider` - Whether to search across all namespaces
///
/// # Returns
///
/// Vector of PodInfo for all running PRIMARY pods
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail
pub async fn find_all_primary_pods(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    is_cluster_provider: bool,
) -> Result<Vec<PodInfo>> {
    use crate::crd::{Bind9Instance, ServerRole};

    // First, find all Bind9Instance resources that belong to this cluster and have role=primary
    let instance_api: Api<Bind9Instance> = if is_cluster_provider {
        Api::all(client.clone())
    } else {
        Api::namespaced(client.clone(), namespace)
    };
    let instances = instance_api.list(&ListParams::default()).await?;

    // Store tuples of (instance_name, instance_namespace)
    let mut primary_instances: Vec<(String, String)> = Vec::new();
    for instance in instances.items {
        if instance.spec.cluster_ref == cluster_name && instance.spec.role == ServerRole::Primary {
            if let (Some(name), Some(ns)) = (instance.metadata.name, instance.metadata.namespace) {
                primary_instances.push((name, ns));
            }
        }
    }

    if primary_instances.is_empty() {
        let search_scope = if is_cluster_provider {
            "all namespaces".to_string()
        } else {
            format!("namespace {namespace}")
        };
        return Err(anyhow!(
            "No PRIMARY Bind9Instance resources found for cluster {cluster_name} in {search_scope}"
        ));
    }

    debug!(
        "Found {} PRIMARY instance(s) for cluster {}: {:?}",
        primary_instances.len(),
        cluster_name,
        primary_instances
    );

    let mut all_pod_infos = Vec::new();

    for (instance_name, instance_namespace) in &primary_instances {
        // Now find all pods for this primary instance in its namespace
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), instance_namespace);
        // List pods with label selector matching the instance
        let label_selector = format!("app=bind9,instance={instance_name}");
        let lp = ListParams::default().labels(&label_selector);

        let pods = pod_api.list(&lp).await?;

        debug!(
            "Found {} pod(s) for PRIMARY instance {}",
            pods.items.len(),
            instance_name
        );

        for pod in &pods.items {
            // Skip pods that are not Running or have no IP yet (e.g. Pending
            // pods that were just scheduled) instead of failing the whole
            // listing - one new pod must not abort operations on healthy pods.
            let Some((pod_name, pod_ip)) = crate::instances::running_pod_name_and_ip(pod) else {
                continue;
            };

            all_pod_infos.push(PodInfo {
                name: pod_name.clone(),
                ip: pod_ip.clone(),
                instance_name: instance_name.clone(),
                namespace: instance_namespace.clone(),
            });
            debug!(
                "Found running pod {} with IP {} in namespace {}",
                pod_name, pod_ip, instance_namespace
            );
        }
    }

    if all_pod_infos.is_empty() {
        return Err(anyhow!(
            "No running PRIMARY pods found for cluster {cluster_name} in namespace {namespace}"
        ));
    }

    debug!(
        "Found {} running PRIMARY pod(s) across {} instance(s) for cluster {}",
        all_pod_infos.len(),
        primary_instances.len(),
        cluster_name
    );

    Ok(all_pod_infos)
}

/// Execute an operation on all endpoints of all primary instances in a cluster.
///
/// This helper function handles the common pattern of:
/// 1. Finding all primary pods for a cluster
/// 2. Collecting unique instance names
/// 3. Optionally loading RNDC key from each instance
/// 4. Getting endpoints for each instance
/// 5. Executing a provided operation on each endpoint
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `resolver` - Per-reconcile resolver for instance RNDC keys and endpoints
///   (ADR-0015): each instance's key and endpoints are read once, endpoints
///   from the shared store
/// * `namespace` - Namespace of the cluster
/// * `cluster_ref` - Name of the `Bind9Cluster` or `ClusterBind9Provider`
/// * `is_cluster_provider` - Whether this is a cluster provider (cluster-scoped)
/// * `with_rndc_key` - Whether to load RNDC key from each instance
/// * `port_name` - Port name to use for endpoints (e.g., "rndc-api", "dns-tcp")
/// * `operation` - Async closure to execute for each endpoint
///   - Arguments: `(pod_endpoint: String, instance_name: String, rndc_key: Option<RndcKeyData>)`
///   - Returns: `Result<()>`
///
/// # Returns
///
/// Returns `Ok((first_endpoint, total_count))` where:
/// - `first_endpoint` - Optional first endpoint encountered (useful for NOTIFY operations)
/// - `total_count` - Total number of endpoints processed successfully
///
/// # Errors
///
/// Returns error if:
/// - No primary pods found for the cluster
/// - Failed to load RNDC key (if requested)
/// - Failed to get endpoints for any instance
/// - The operation closure returns an error for any endpoint
#[allow(clippy::too_many_arguments)]
pub async fn for_each_primary_endpoint<F, Fut>(
    client: &Client,
    resolver: &crate::instances::InstanceResolver,
    namespace: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    with_rndc_key: bool,
    port_name: &str,
    operation: F,
) -> Result<(Option<String>, usize)>
where
    F: Fn(String, String, Option<RndcKeyData>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    // Find all PRIMARY pods to get the unique instance names
    let primary_pods =
        find_all_primary_pods(client, namespace, cluster_ref, is_cluster_provider).await?;

    debug!(
        "Found {} PRIMARY pod(s) for cluster {}",
        primary_pods.len(),
        cluster_ref
    );

    // Collect unique (instance_name, namespace) tuples from the primary pods
    // Each instance may have multiple pods (replicas)
    let mut instance_tuples: Vec<(String, String)> = primary_pods
        .iter()
        .map(|pod| (pod.instance_name.clone(), pod.namespace.clone()))
        .collect();
    instance_tuples.sort();
    instance_tuples.dedup();

    debug!(
        "Found {} primary instance(s) for cluster {}: {:?}",
        instance_tuples.len(),
        cluster_ref,
        instance_tuples
    );

    let mut first_endpoint: Option<String> = None;
    let mut total_endpoints = 0;
    let mut errors: Vec<String> = Vec::new();

    // Loop through each primary instance and get its endpoints
    // Important: With EmptyDir storage (per-pod, non-shared), each primary pod maintains its own
    // zone files. We need to process ALL pods across ALL instances.
    for (instance_name, instance_namespace) in &instance_tuples {
        debug!(
            "Getting endpoints for instance {}/{} in cluster {}",
            instance_namespace, instance_name, cluster_ref
        );

        // Load RNDC key for this specific instance if requested
        // Each instance has its own RNDC secret for security isolation
        let key_data = if with_rndc_key {
            Some(resolver.rndc_key(instance_namespace, instance_name).await?)
        } else {
            None
        };

        // Get all endpoints for this instance's service
        // The Endpoints API gives us pod IPs with their container ports (not service ports)
        let endpoints = resolver
            .endpoints(instance_namespace, instance_name, port_name)
            .await?;

        debug!(
            "Found {} endpoint(s) for instance {}",
            endpoints.len(),
            instance_name
        );

        for endpoint in &endpoints {
            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

            // Save the first endpoint
            if first_endpoint.is_none() {
                first_endpoint = Some(pod_endpoint.clone());
            }

            // Execute the operation on this endpoint with this instance's RNDC key
            // Continue processing remaining endpoints even if this one fails
            if let Err(e) = operation(
                pod_endpoint.clone(),
                instance_name.clone(),
                key_data.clone(),
            )
            .await
            {
                error!(
                    "Failed operation on endpoint {} (instance {}): {}",
                    pod_endpoint, instance_name, e
                );
                errors.push(format!(
                    "endpoint {pod_endpoint} (instance {instance_name}): {e}"
                ));
            } else {
                total_endpoints += 1;
            }
        }
    }

    // If any operations failed, return an error with all failures listed
    if !errors.is_empty() {
        return Err(anyhow::anyhow!(
            "Failed to process {} endpoint(s): {}",
            errors.len(),
            errors.join("; ")
        ));
    }

    Ok((first_endpoint, total_endpoints))
}

#[cfg(test)]
#[path = "primary_tests.rs"]
mod primary_tests;
