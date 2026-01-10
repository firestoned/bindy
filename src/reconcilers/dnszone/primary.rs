// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

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
use tracing::{debug, error, info, warn};

use super::helpers::{get_endpoint, load_rndc_key};
use super::types::PodInfo;
use crate::bind9::RndcKeyData;

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

    info!(
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
            let pod_name = pod
                .metadata
                .name
                .as_ref()
                .ok_or_else(|| anyhow!("Pod has no name"))?
                .clone();

            // Get pod IP
            let pod_ip = pod
                .status
                .as_ref()
                .and_then(|s| s.pod_ip.as_ref())
                .ok_or_else(|| anyhow!("Pod {pod_name} has no IP address"))?
                .clone();

            // Check if pod is running
            let phase = pod
                .status
                .as_ref()
                .and_then(|s| s.phase.as_ref())
                .map(String::as_str);

            if phase == Some("Running") {
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
            } else {
                debug!(
                    "Skipping pod {} (phase: {:?}, not running)",
                    pod_name, phase
                );
            }
        }
    }

    if all_pod_infos.is_empty() {
        return Err(anyhow!(
            "No running PRIMARY pods found for cluster {cluster_name} in namespace {namespace}"
        ));
    }

    info!(
        "Found {} running PRIMARY pod(s) across {} instance(s) for cluster {}",
        all_pod_infos.len(),
        primary_instances.len(),
        cluster_name
    );

    Ok(all_pod_infos)
}

/// Find primary server IPs from a list of instance references.
///
/// This is the NEW instance-based approach that replaces cluster-based lookup.
/// It filters the instance refs to only PRIMARY instances, then gets their pod IPs.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - List of instance references to search
///
/// # Returns
///
/// A vector of IP addresses for all running PRIMARY pods across all primary instances
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail or no primary pods are found
pub async fn find_primary_ips_from_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<String>> {
    use crate::crd::{Bind9Instance, ServerRole};
    use k8s_openapi::api::core::v1::Pod;

    info!(
        "Finding PRIMARY pod IPs from {} instance reference(s)",
        instance_refs.len()
    );

    let mut primary_ips = Vec::new();

    for instance_ref in instance_refs {
        // Get the Bind9Instance to check its role
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        let instance = match instance_api.get(&instance_ref.name).await {
            Ok(inst) => inst,
            Err(e) => {
                warn!(
                    "Failed to get instance {}/{}: {}",
                    instance_ref.namespace, instance_ref.name, e
                );
                continue;
            }
        };

        // Skip if not a PRIMARY instance
        if instance.spec.role != ServerRole::Primary {
            continue;
        }

        // Get running pod IPs for this primary instance
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), &instance_ref.namespace);
        let label_selector = format!("app=bind9,instance={}", instance_ref.name);
        let lp = ListParams::default().labels(&label_selector);

        match pod_api.list(&lp).await {
            Ok(pods) => {
                for pod in pods.items {
                    if let Some(pod_ip) = pod.status.as_ref().and_then(|s| s.pod_ip.as_ref()) {
                        // Check if pod is running
                        let phase = pod
                            .status
                            .as_ref()
                            .and_then(|s| s.phase.as_ref())
                            .map_or("Unknown", std::string::String::as_str);

                        if phase == "Running" {
                            primary_ips.push(pod_ip.clone());
                            debug!(
                                "Added IP {} from running PRIMARY pod {} (instance {}/{})",
                                pod_ip,
                                pod.metadata.name.as_ref().unwrap_or(&"unknown".to_string()),
                                instance_ref.namespace,
                                instance_ref.name
                            );
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to list pods for PRIMARY instance {}/{}: {}",
                    instance_ref.namespace, instance_ref.name, e
                );
            }
        }
    }

    info!(
        "Found total of {} PRIMARY pod IP(s) across all instances: {:?}",
        primary_ips.len(),
        primary_ips
    );

    Ok(primary_ips)
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
pub async fn for_each_primary_endpoint<F, Fut>(
    client: &Client,
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

    info!(
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

    info!(
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
        info!(
            "Getting endpoints for instance {}/{} in cluster {}",
            instance_namespace, instance_name, cluster_ref
        );

        // Load RNDC key for this specific instance if requested
        // Each instance has its own RNDC secret for security isolation
        let key_data = if with_rndc_key {
            Some(load_rndc_key(client, instance_namespace, instance_name).await?)
        } else {
            None
        };

        // Get all endpoints for this instance's service
        // The Endpoints API gives us pod IPs with their container ports (not service ports)
        let endpoints = get_endpoint(client, instance_namespace, instance_name, port_name).await?;

        info!(
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
