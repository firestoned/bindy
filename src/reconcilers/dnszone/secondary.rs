// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Secondary zone instance operations.
//!
//! This module handles all operations specific to SECONDARY BIND9 instances,
//! including:
//! - Filtering instance references to only secondary instances
//! - Finding secondary pods across instances
//! - Collecting secondary pod IPs
//! - Executing operations on all secondary endpoints

use anyhow::{anyhow, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::ListParams, Api, Client};
use tracing::{debug, error, info, warn};

use super::helpers::{get_endpoint, load_rndc_key};
use super::types::PodInfo;
use crate::bind9::RndcKeyData;

/// Filters a list of instance references to only SECONDARY instances.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// Vector of instance references that have role=Secondary
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail.
pub async fn filter_secondary_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    use crate::crd::{Bind9Instance, ServerRole};

    let mut secondary_refs = Vec::new();

    for instance_ref in instance_refs {
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        match instance_api.get(&instance_ref.name).await {
            Ok(instance) => {
                if instance.spec.role == ServerRole::Secondary {
                    secondary_refs.push(instance_ref.clone());
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

    Ok(secondary_refs)
}

/// Finds all pod IPs from a list of instance references, filtering by role.
///
/// Queries each `Bind9Instance` resource to determine its role, then collects
/// pod IPs only from secondary instances. This is event-driven as it reacts
/// to the current state of `Bind9Instance` resources rather than caching.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Instance references to query
///
/// # Returns
///
/// Vector of pod IP addresses from secondary instances only
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail.
pub async fn find_secondary_pod_ips_from_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<String>> {
    use crate::crd::{Bind9Instance, ServerRole};
    use k8s_openapi::api::core::v1::Pod;

    let mut secondary_ips = Vec::new();

    for instance_ref in instance_refs {
        // Query the Bind9Instance resource to check its role
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        let instance = match instance_api.get(&instance_ref.name).await {
            Ok(inst) => inst,
            Err(e) => {
                warn!(
                    "Failed to get Bind9Instance {}/{}: {}. Skipping.",
                    instance_ref.namespace, instance_ref.name, e
                );
                continue;
            }
        };

        // Only collect IPs from secondary instances
        if instance.spec.role != ServerRole::Secondary {
            debug!(
                "Skipping instance {}/{} - role is {:?}, not Secondary",
                instance_ref.namespace, instance_ref.name, instance.spec.role
            );
            continue;
        }

        // Find pods for this secondary instance
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
                            secondary_ips.push(pod_ip.clone());
                        } else {
                            debug!(
                                "Skipping pod {} in phase {} for instance {}/{}",
                                pod.metadata.name.as_ref().unwrap_or(&"unknown".to_string()),
                                phase,
                                instance_ref.namespace,
                                instance_ref.name
                            );
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to list pods for instance {}/{}: {}. Skipping.",
                    instance_ref.namespace, instance_ref.name, e
                );
            }
        }
    }

    Ok(secondary_ips)
}

async fn find_all_secondary_pods(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    is_cluster_provider: bool,
) -> Result<Vec<PodInfo>> {
    use crate::crd::{Bind9Instance, ServerRole};

    // Find all Bind9Instance resources with role=SECONDARY for this cluster
    let instance_api: Api<Bind9Instance> = if is_cluster_provider {
        Api::all(client.clone())
    } else {
        Api::namespaced(client.clone(), namespace)
    };
    let instances = instance_api.list(&ListParams::default()).await?;

    // Store tuples of (instance_name, instance_namespace)
    let mut secondary_instances: Vec<(String, String)> = Vec::new();
    for instance in instances.items {
        if instance.spec.cluster_ref == cluster_name && instance.spec.role == ServerRole::Secondary
        {
            if let (Some(name), Some(ns)) = (instance.metadata.name, instance.metadata.namespace) {
                secondary_instances.push((name, ns));
            }
        }
    }

    if secondary_instances.is_empty() {
        info!("No SECONDARY instances found for cluster {cluster_name}");
        return Ok(Vec::new());
    }

    info!(
        "Found {} SECONDARY instance(s) for cluster {}: {:?}",
        secondary_instances.len(),
        cluster_name,
        secondary_instances
    );

    let mut all_pod_infos = Vec::new();

    for (instance_name, instance_namespace) in &secondary_instances {
        // Find all pods for this secondary instance in its namespace
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), instance_namespace);
        let label_selector = format!("app=bind9,instance={instance_name}");
        let lp = ListParams::default().labels(&label_selector);

        let pods = pod_api.list(&lp).await?;

        debug!(
            "Found {} pod(s) for SECONDARY instance {}",
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
                    "Found running secondary pod {} with IP {} in namespace {}",
                    pod_name, pod_ip, instance_namespace
                );
            } else {
                debug!(
                    "Skipping secondary pod {} (phase: {:?}, not running)",
                    pod_name, phase
                );
            }
        }
    }

    info!(
        "Found {} running SECONDARY pod(s) across {} instance(s) for cluster {}",
        all_pod_infos.len(),
        secondary_instances.len(),
        cluster_name
    );

    Ok(all_pod_infos)
}

/// Update `lastReconciledAt` timestamp for a zone in `Bind9Instance.status.selectedZones[]`.
///
/// This function implements the critical Phase 2 completion step: after successfully
/// configuring a zone on an instance, we update the instance's status to signal that
/// the zone is now reconciled and doesn't need reconfiguration on future reconciliations.
///
/// This prevents infinite reconciliation loops by ensuring the `DNSZone` watch mapper
/// only triggers reconciliation when `lastReconciledAt == None`.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_name` - Name of the `Bind9Instance`
/// * `instance_namespace` - Namespace of the `Bind9Instance`
/// * `zone_name` - Name of the `DNSZone` resource
/// * `zone_namespace` - Namespace of the `DNSZone` resource
///
/// Execute an operation on all SECONDARY endpoints for a cluster.
///
/// Similar to `for_each_primary_endpoint`, but operates on SECONDARY instances.
/// Useful for triggering zone transfers or other secondary-specific operations.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for instances
/// * `cluster_ref` - Cluster reference name
/// * `is_cluster_provider` - Whether this is a cluster provider (cluster-scoped)
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
/// Returns an error if:
/// - Failed to find secondary pods
/// - Failed to load RNDC keys
/// - Failed to get service endpoints
/// - The operation closure returns an error for any endpoint
pub async fn for_each_secondary_endpoint<F, Fut>(
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
    // Find all SECONDARY pods to get the unique instance names
    let secondary_pods =
        find_all_secondary_pods(client, namespace, cluster_ref, is_cluster_provider).await?;

    info!(
        "Found {} SECONDARY pod(s) for cluster {}",
        secondary_pods.len(),
        cluster_ref
    );

    // Collect unique (instance_name, namespace) tuples from the secondary pods
    // Each instance may have multiple pods (replicas)
    let mut instance_tuples: Vec<(String, String)> = secondary_pods
        .iter()
        .map(|pod| (pod.instance_name.clone(), pod.namespace.clone()))
        .collect();
    instance_tuples.sort();
    instance_tuples.dedup();

    info!(
        "Found {} secondary instance(s) for cluster {}: {:?}",
        instance_tuples.len(),
        cluster_ref,
        instance_tuples
    );

    let mut first_endpoint: Option<String> = None;
    let mut total_endpoints = 0;
    let mut errors: Vec<String> = Vec::new();

    // Loop through each secondary instance and get its endpoints
    for (instance_name, instance_namespace) in &instance_tuples {
        info!(
            "Getting endpoints for secondary instance {}/{} in cluster {}",
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
            "Found {} endpoint(s) for secondary instance {}",
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
                    "Failed operation on secondary endpoint {} (instance {}): {}",
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
            "Failed to process {} secondary endpoint(s): {}",
            errors.len(),
            errors.join("; ")
        ));
    }

    Ok((first_endpoint, total_endpoints))
}

#[cfg(test)]
#[path = "secondary_tests.rs"]
mod secondary_tests;
