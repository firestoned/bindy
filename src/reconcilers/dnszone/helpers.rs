// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Helper functions for DNS zone reconciliation.
//!
//! This module contains the validation and change detection helper functions
//! extracted from the main reconcile_dnszone() function to improve maintainability.

use crate::crd::{DNSZone, InstanceReference};
use anyhow::{anyhow, Context as AnyhowContext, Result};
use k8s_openapi::api::core::v1::{Endpoints, Secret};
use kube::{Api, Client};
use std::collections::{HashMap, HashSet};
use tracing::{error, info};

use super::types::{DuplicateZoneInfo, EndpointAddress};
use crate::bind9::RndcKeyData;

/// Re-fetch a DNSZone to get the latest status.
///
/// The `dnszone` parameter from the watch event might have stale status from the cache.
/// We need the latest `status.bind9Instances` which may have been updated by the
/// Bind9Instance reconciler.
///
/// # Arguments
/// * `client` - Kubernetes client
/// * `namespace` - Namespace of the DNSZone
/// * `name` - Name of the DNSZone
///
/// # Returns
/// The freshly fetched DNSZone with current status
///
/// # Errors
/// Returns an error if the Kubernetes API call fails
pub async fn refetch_zone(client: &Client, namespace: &str, name: &str) -> Result<DNSZone> {
    let zones_api: kube::Api<DNSZone> = kube::Api::namespaced(client.clone(), namespace);
    let zone = zones_api.get(name).await?;
    Ok(zone)
}

/// Handle duplicate zone conflicts by setting Ready=False and stopping reconciliation.
///
/// When a duplicate zone is detected, this function:
/// 1. Logs a warning with details about the conflict
/// 2. Updates the status with Ready=False and DuplicateZone condition
/// 3. Applies the status to the API server
///
/// # Arguments
/// * `client` - Kubernetes client
/// * `namespace` - Namespace of the conflicting DNSZone
/// * `name` - Name of the conflicting DNSZone
/// * `duplicate_info` - Information about the duplicate zone conflict
/// * `status_updater` - Status updater to apply the condition
///
/// # Errors
/// Returns an error if the status update fails
pub async fn handle_duplicate_zone(
    client: &Client,
    namespace: &str,
    name: &str,
    duplicate_info: &DuplicateZoneInfo,
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
) -> Result<()> {
    tracing::warn!(
        "Duplicate zone detected: {}/{} cannot claim '{}' because it is already configured by: {:?}",
        namespace, name, duplicate_info.zone_name, duplicate_info.conflicting_zones
    );

    // Build list of conflicting zones in namespace/name format
    let conflicting_zone_refs: Vec<String> = duplicate_info
        .conflicting_zones
        .iter()
        .map(|z| format!("{}/{}", z.namespace, z.name))
        .collect();

    // Set Ready=False with DuplicateZone reason
    status_updater.set_duplicate_zone_condition(&duplicate_info.zone_name, &conflicting_zone_refs);

    // Apply status and stop processing
    status_updater.apply(client).await?;

    Ok(())
}

/// Detect if the zone spec has changed since last reconciliation.
///
/// Compares current generation with observed generation to determine
/// if this is first reconciliation or if spec changed.
///
/// # Arguments
///
/// * `zone` - The DNSZone resource
///
/// # Returns
///
/// Tuple of (first_reconciliation, spec_changed)
#[must_use]
pub fn detect_spec_changes(zone: &DNSZone) -> (bool, bool) {
    let current_generation = zone.metadata.generation;
    let observed_generation = zone.status.as_ref().and_then(|s| s.observed_generation);

    let first_reconciliation = observed_generation.is_none();
    let spec_changed =
        crate::reconcilers::should_reconcile(current_generation, observed_generation);

    (first_reconciliation, spec_changed)
}

/// Detect if the instance list changed between watch event and re-fetch.
///
/// This is critical for detecting when:
/// 1. New instances are added to `status.bind9Instances` (via `bind9InstancesFrom` selectors)
/// 2. Instance `lastReconciledAt` timestamps are cleared (e.g., instance deleted, needs reconfiguration)
///
/// NOTE: `InstanceReference` `PartialEq` ignores `lastReconciledAt`, so we must check timestamps separately!
///
/// # Arguments
///
/// * `namespace` - Namespace for logging
/// * `name` - Zone name for logging
/// * `watch_instances` - Instances from the watch event that triggered reconciliation
/// * `current_instances` - Instances after re-fetching (current state)
///
/// # Returns
///
/// `true` if instances changed (list or timestamps), `false` otherwise
pub fn detect_instance_changes(
    namespace: &str,
    name: &str,
    watch_instances: Option<&Vec<InstanceReference>>,
    current_instances: &[InstanceReference],
) -> bool {
    let Some(watch_instances) = watch_instances else {
        // No instances in watch event, first reconciliation or error
        return true;
    };

    // Get the instance names from the watch event (what triggered us)
    let watch_instance_names: HashSet<_> = watch_instances.iter().map(|r| &r.name).collect();

    // Get the instance names after re-fetching (current state)
    let current_instance_names: HashSet<_> = current_instances.iter().map(|r| &r.name).collect();

    // Check if instance list changed (added/removed instances)
    let list_changed = watch_instance_names != current_instance_names;

    if list_changed {
        info!(
            "Instance list changed during reconciliation for zone {}/{}: watch_event={:?}, current={:?}",
            namespace, name, watch_instance_names, current_instance_names
        );
        return true;
    }

    // List is the same, but check if any lastReconciledAt timestamps changed
    // Use InstanceReference as HashMap key (uses its Hash impl which hashes identity fields)
    let watch_timestamps: HashMap<&InstanceReference, Option<&str>> = watch_instances
        .iter()
        .map(|inst| (inst, inst.last_reconciled_at.as_deref()))
        .collect();

    let current_timestamps: HashMap<&InstanceReference, Option<&str>> = current_instances
        .iter()
        .map(|inst| (inst, inst.last_reconciled_at.as_deref()))
        .collect();

    let timestamps_changed = watch_timestamps.iter().any(|(inst_ref, watch_ts)| {
        current_timestamps
            .get(inst_ref)
            .is_some_and(|current_ts| current_ts != watch_ts)
    });

    if timestamps_changed {
        info!(
            "Instance lastReconciledAt timestamps changed for zone {}/{}",
            namespace, name
        );
    }

    timestamps_changed
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
            Some(load_rndc_key(client, &instance_ref.namespace, &instance_ref.name).await?)
        } else {
            None
        };

        // Get all endpoints for this instance's service
        let endpoints = get_endpoint(
            client,
            &instance_ref.namespace,
            &instance_ref.name,
            port_name,
        )
        .await?;

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
                error!(
                    "Failed operation on endpoint {} (instance {}/{}): {}",
                    pod_endpoint, instance_ref.namespace, instance_ref.name, e
                );
                errors.push(format!(
                    "endpoint {pod_endpoint} (instance {}/{}): {e}",
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
