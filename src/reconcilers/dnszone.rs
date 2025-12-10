// Copyright (c) 2025 Erick Bourgeois, firestoned
#![allow(dead_code)]
// SPDX-License-Identifier: MIT

//! DNS zone reconciliation logic.
//!
//! This module handles the creation and management of DNS zones on BIND9 servers.
//! It supports both primary and secondary zone configurations.

use crate::bind9::RndcKeyData;
use crate::crd::{Condition, DNSZone, DNSZoneStatus};
use anyhow::{anyhow, Context, Result};
use bindcar::{ZONE_TYPE_PRIMARY, ZONE_TYPE_SECONDARY};
use chrono::Utc;
use k8s_openapi::api::core::v1::{Endpoints, Pod, Secret};
use kube::{
    api::{ListParams, Patch, PatchParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use tracing::{debug, info, warn};

/// Reconciles a `DNSZone` resource.
///
/// Creates or updates DNS zone files on BIND9 instances that match the zone's
/// instance selector. Supports both primary and secondary zone types.
///
/// # Zone Types
///
/// - **Primary**: Authoritative zone with SOA record and local zone file
/// - **Secondary**: Replica zone that transfers from primary servers
///
/// # Arguments
///
/// * `client` - Kubernetes API client for finding matching `Bind9Instances`
/// * `dnszone` - The `DNSZone` resource to reconcile
/// * `zone_manager` - BIND9 manager for creating zone files
///
/// # Returns
///
/// * `Ok(())` - If zone was created/updated successfully
/// * `Err(_)` - If zone creation failed or configuration is invalid
///
/// # Example
///
/// ```rust,no_run
/// use bindy::reconcilers::reconcile_dnszone;
/// use bindy::crd::DNSZone;
/// use bindy::bind9::Bind9Manager;
/// use kube::Client;
///
/// async fn handle_zone(zone: DNSZone) -> anyhow::Result<()> {
///     let client = Client::try_default().await?;
///     let manager = Bind9Manager::new();
///     reconcile_dnszone(client, zone, &manager).await?;
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 zone operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_dnszone(
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();

    info!("Reconciling DNSZone: {}/{}", namespace, name);
    debug!(
        namespace = %namespace,
        name = %name,
        generation = ?dnszone.metadata.generation,
        "Starting DNSZone reconciliation"
    );

    // Extract spec
    let spec = &dnszone.spec;
    debug!(
        zone_name = %spec.zone_name,
        cluster_ref = %spec.cluster_ref,
        "DNSZone configuration"
    );

    // Determine if this is the first reconciliation or if spec has changed
    let current_generation = dnszone.metadata.generation;
    let observed_generation = dnszone.status.as_ref().and_then(|s| s.observed_generation);

    let first_reconciliation = observed_generation.is_none();
    let spec_changed =
        crate::reconcilers::should_reconcile(current_generation, observed_generation);

    // Early return if nothing to do
    if !first_reconciliation && !spec_changed {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    info!(
        "Reconciling zone {} (first_reconciliation={}, spec_changed={})",
        spec.zone_name, first_reconciliation, spec_changed
    );

    // Set initial Progressing status
    update_condition(
        &client,
        &dnszone,
        "Progressing",
        "True",
        "PrimaryReconciling",
        "Configuring zone on primary servers",
    )
    .await?;

    // Get current primary IPs for secondary zone configuration
    let primary_ips = match find_all_primary_pod_ips(&client, &namespace, &spec.cluster_ref).await {
        Ok(ips) if !ips.is_empty() => {
            info!(
                "Found {} primary server(s) for cluster {}: {:?}",
                ips.len(),
                spec.cluster_ref,
                ips
            );
            ips
        }
        Ok(_) => {
            update_condition(
                &client,
                &dnszone,
                "Degraded",
                "True",
                "PrimaryFailed",
                &format!("No primary servers found for cluster {}", spec.cluster_ref),
            )
            .await?;
            return Err(anyhow!(
                "No primary servers found for cluster {} - cannot configure zones",
                spec.cluster_ref
            ));
        }
        Err(e) => {
            update_condition(
                &client,
                &dnszone,
                "Degraded",
                "True",
                "PrimaryFailed",
                &format!("Failed to find primary servers: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Add/update zone on all primary instances
    let primary_count = match add_dnszone(client.clone(), dnszone.clone(), zone_manager).await {
        Ok(count) => {
            // Update status after successful primary reconciliation
            update_condition(
                &client,
                &dnszone,
                "Progressing",
                "True",
                "PrimaryReconciled",
                &format!(
                    "Zone {} configured on {} primary server(s)",
                    spec.zone_name, count
                ),
            )
            .await?;
            count
        }
        Err(e) => {
            update_condition(
                &client,
                &dnszone,
                "Degraded",
                "True",
                "PrimaryFailed",
                &format!("Failed to configure zone on primary servers: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Update to secondary reconciliation phase
    update_condition(
        &client,
        &dnszone,
        "Progressing",
        "True",
        "SecondaryReconciling",
        "Configuring zone on secondary servers",
    )
    .await?;

    // Add/update zone on all secondary instances with primaries configured
    let secondary_count = match add_dnszone_to_secondaries(
        client.clone(),
        dnszone.clone(),
        zone_manager,
        &primary_ips,
    )
    .await
    {
        Ok(count) => {
            // Update status after successful secondary reconciliation
            if count > 0 {
                update_condition(
                    &client,
                    &dnszone,
                    "Progressing",
                    "True",
                    "SecondaryReconciled",
                    &format!(
                        "Zone {} configured on {} secondary server(s)",
                        spec.zone_name, count
                    ),
                )
                .await?;
            }
            count
        }
        Err(e) => {
            // Secondary failure is non-fatal - primaries still work
            warn!(
                "Failed to configure zone on secondary servers: {}. Primary servers are still operational.",
                e
            );
            update_condition(
                &client,
                &dnszone,
                "Degraded",
                "True",
                "SecondaryFailed",
                &format!(
                    "Zone configured on {primary_count} primary server(s) but secondary configuration failed: {e}"
                ),
            )
            .await?;
            0
        }
    };

    // Re-fetch secondary IPs to store in status
    let secondary_ips = find_all_secondary_pod_ips(&client, &namespace, &spec.cluster_ref)
        .await
        .unwrap_or_default();

    // All reconciliation complete - set Ready status
    update_status_with_secondaries(
        &client,
        &dnszone,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "Zone {} configured on {} primary and {} secondary server(s) for cluster {}",
            spec.zone_name, primary_count, secondary_count, spec.cluster_ref
        ),
        secondary_ips,
    )
    .await?;

    Ok(())
}

/// Adds a DNS zone to all primary instances in the cluster.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `dnszone` - The `DNSZone` resource
/// * `zone_manager` - BIND9 manager for adding zone
///
/// # Returns
///
/// * `Ok(usize)` - Number of primary endpoints successfully configured
/// * `Err(_)` - If zone addition failed
///
/// # Errors
///
/// Returns an error if BIND9 zone addition fails.
///
/// # Panics
///
/// Panics if the RNDC key is not loaded by the helper function (should never happen in practice).
pub async fn add_dnszone(
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<usize> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    info!("Adding DNSZone: {}", name);

    // Find secondary pod IPs for zone transfer configuration
    let secondary_ips = find_all_secondary_pod_ips(&client, &namespace, &spec.cluster_ref).await?;

    if secondary_ips.is_empty() {
        warn!(
            "No secondary servers found for cluster {} - zone transfers will not be configured",
            spec.cluster_ref
        );
    } else {
        info!(
            "Found {} secondary server(s) for cluster {} - zone transfers will be configured: {:?}",
            secondary_ips.len(),
            spec.cluster_ref,
            secondary_ips
        );
    }

    // Use the common helper to iterate through all endpoints
    // Load RNDC key (true) since zone addition requires it
    // Use "http" port for HTTP API operations
    let (first_endpoint, total_endpoints) = for_each_primary_endpoint(
        &client,
        &namespace,
        &spec.cluster_ref,
        true, // with_rndc_key = true for zone addition
        "http", // Use HTTP API port for zone addition via bindcar API
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = spec.zone_name.clone();
            let soa_record = spec.soa_record.clone();
            let name_server_ips = spec.name_server_ips.clone();
            let zone_manager = zone_manager.clone();
            let secondary_ips_clone = secondary_ips.clone();

            async move {
                // SAFETY: RNDC key is guaranteed to be Some when with_rndc_key=true
                // The for_each_primary_endpoint helper loads the key when with_rndc_key=true
                let key_data = rndc_key.expect("RNDC key should be loaded when with_rndc_key=true");

                // Pass secondary IPs for zone transfer configuration
                let secondary_ips_ref = if secondary_ips_clone.is_empty() {
                    None
                } else {
                    Some(secondary_ips_clone.as_slice())
                };

                let was_added = zone_manager
                    .add_zones(
                        &zone_name,
                        ZONE_TYPE_PRIMARY,
                        &pod_endpoint,
                        &key_data,
                        Some(&soa_record),
                        name_server_ips.as_ref(),
                        secondary_ips_ref,
                        None, // primary_ips only for secondary zones
                    )
                    .await
                    .context(format!(
                        "Failed to add zone {zone_name} to endpoint {pod_endpoint} (instance: {instance_name})"
                    ))?;

                if was_added {
                    info!(
                        "Successfully added zone {} to endpoint {} (instance: {})",
                        zone_name, pod_endpoint, instance_name
                    );
                }

                Ok(())
            }
        },
    )
    .await?;

    info!(
        "Successfully added zone {} to {} endpoint(s) for cluster {}",
        spec.zone_name, total_endpoints, spec.cluster_ref
    );

    // Note: We don't need to reload after addzone because:
    // 1. rndc addzone immediately adds the zone to BIND9's running config
    // 2. The zone file will be created automatically when records are added via dynamic updates
    // 3. Reloading would fail if the zone file doesn't exist yet

    // Notify secondaries about the new zone via the first endpoint
    // This triggers zone transfer (AXFR) from primary to secondaries
    if let Some(first_pod_endpoint) = first_endpoint {
        info!(
            "Notifying secondaries about new zone {} for cluster {}",
            spec.zone_name, spec.cluster_ref
        );
        if let Err(e) = zone_manager
            .notify_zone(&spec.zone_name, &first_pod_endpoint)
            .await
        {
            // Don't fail if NOTIFY fails - the zone was successfully created
            // Secondaries will sync via SOA refresh timer
            warn!(
                "Failed to notify secondaries for zone {}: {}. Secondaries will sync via SOA refresh timer.",
                spec.zone_name, e
            );
        }
    } else {
        warn!(
            "No endpoints found for zone {}, cannot notify secondaries",
            spec.zone_name
        );
    }

    Ok(total_endpoints)
}

/// Adds a DNS zone to all secondary instances in the cluster with primaries configured.
///
/// Creates secondary zones on all secondary instances, configuring them to transfer
/// from the provided primary server IPs. If a zone already exists on a secondary,
/// it checks if the primaries list matches and updates it if necessary.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `dnszone` - The `DNSZone` resource
/// * `zone_manager` - BIND9 manager for adding zone
/// * `primary_ips` - List of primary server IPs to configure in the primaries field
///
/// # Returns
///
/// * `Ok(usize)` - Number of secondary endpoints successfully configured
/// * `Err(_)` - If zone addition failed
///
/// # Errors
///
/// Returns an error if BIND9 zone addition fails on any secondary instance.
pub async fn add_dnszone_to_secondaries(
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
    primary_ips: &[String],
) -> Result<usize> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    if primary_ips.is_empty() {
        warn!(
            "No primary IPs provided for secondary zone {} - skipping secondary configuration",
            spec.zone_name
        );
        return Ok(0);
    }

    info!(
        "Adding DNSZone {} to secondary instances with primaries: {:?}",
        name, primary_ips
    );

    // Find all secondary pods
    let secondary_pods = find_all_secondary_pods(&client, &namespace, &spec.cluster_ref).await?;

    if secondary_pods.is_empty() {
        info!(
            "No secondary servers found for cluster {} - skipping secondary zone configuration",
            spec.cluster_ref
        );
        return Ok(0);
    }

    info!(
        "Found {} secondary pod(s) for cluster {}",
        secondary_pods.len(),
        spec.cluster_ref
    );

    // Get unique instance names from secondary pods
    let mut instance_names: Vec<String> = secondary_pods
        .iter()
        .map(|pod| pod.instance_name.clone())
        .collect();
    instance_names.sort();
    instance_names.dedup();

    // Load RNDC key from the first secondary instance
    let key_data = if let Some(first_instance) = instance_names.first() {
        load_rndc_key(&client, &namespace, first_instance).await?
    } else {
        return Err(anyhow!(
            "No secondary instances found for cluster {}",
            spec.cluster_ref
        ));
    };

    let mut total_endpoints = 0;

    // Iterate through each secondary instance and add zone to all its endpoints
    for instance_name in &instance_names {
        info!(
            "Processing secondary instance {} for zone {}",
            instance_name, spec.zone_name
        );

        // Get all endpoints for this secondary instance
        let endpoints = get_endpoint(&client, &namespace, instance_name, "http").await?;

        info!(
            "Found {} endpoint(s) for secondary instance {}",
            endpoints.len(),
            instance_name
        );

        for endpoint in &endpoints {
            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

            info!(
                "Adding secondary zone {} to endpoint {} (instance: {}) with primaries: {:?}",
                spec.zone_name, pod_endpoint, instance_name, primary_ips
            );

            let was_added = zone_manager
                .add_zones(
                    &spec.zone_name,
                    ZONE_TYPE_SECONDARY,
                    &pod_endpoint,
                    &key_data,
                    None, // No SOA record for secondary zones
                    None, // No name_server_ips for secondary zones
                    None, // No secondary_ips for secondary zones
                    Some(primary_ips),
                )
                .await
                .context(format!(
                    "Failed to add secondary zone {} to endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
                ))?;

            if was_added {
                info!(
                    "Successfully added secondary zone {} to endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
                );
            } else {
                info!(
                    "Secondary zone {} already exists on endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
                );
            }

            total_endpoints += 1;
        }
    }

    info!(
        "Successfully configured secondary zone {} on {} endpoint(s) across {} instance(s) for cluster {}",
        spec.zone_name,
        total_endpoints,
        instance_names.len(),
        spec.cluster_ref
    );

    Ok(total_endpoints)
}

/// Deletes a DNS zone and its associated zone files.
///
/// # Arguments
///
/// * `_client` - Kubernetes API client (unused, for future extensions)
/// * `dnszone` - The `DNSZone` resource to delete
/// * `zone_manager` - BIND9 manager for removing zone files
///
/// # Returns
///
/// * `Ok(())` - If zone was deleted successfully
/// * `Err(_)` - If zone deletion failed
///
/// # Errors
///
/// Returns an error if BIND9 zone deletion fails.
pub async fn delete_dnszone(
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    info!("Deleting DNSZone: {}", name);

    // Use the common helper to iterate through all endpoints
    // Don't load RNDC key (false) since zone deletion doesn't require it
    // Use "http" port for HTTP API operations
    let (_first_endpoint, total_endpoints) = for_each_primary_endpoint(
        &client,
        &namespace,
        &spec.cluster_ref,
        false, // with_rndc_key = false for zone deletion
        "http", // Use HTTP API port for zone deletion via bindcar API
        |pod_endpoint, instance_name, _rndc_key| {
            let zone_name = spec.zone_name.clone();
            let zone_manager = zone_manager.clone();

            async move {
                info!(
                    "Deleting zone {} from endpoint {} (instance: {})",
                    zone_name, pod_endpoint, instance_name
                );

                zone_manager
                    .delete_zone(&zone_name, &pod_endpoint)
                    .await
                    .context(format!(
                        "Failed to delete zone {zone_name} from endpoint {pod_endpoint} (instance: {instance_name})"
                    ))?;

                debug!(
                    "Successfully deleted zone {} from endpoint {} (instance: {})",
                    zone_name, pod_endpoint, instance_name
                );

                Ok(())
            }
        },
    )
    .await?;

    info!(
        "Successfully deleted zone {} from {} primary endpoint(s) for cluster {}",
        spec.zone_name, total_endpoints, spec.cluster_ref
    );

    // Delete from all secondary instances
    let secondary_pods = find_all_secondary_pods(&client, &namespace, &spec.cluster_ref).await?;

    if !secondary_pods.is_empty() {
        // Get unique instance names
        let mut instance_names: Vec<String> = secondary_pods
            .iter()
            .map(|pod| pod.instance_name.clone())
            .collect();
        instance_names.sort();
        instance_names.dedup();

        let mut secondary_endpoints_deleted = 0;

        for instance_name in &instance_names {
            let endpoints = get_endpoint(&client, &namespace, instance_name, "http").await?;

            for endpoint in &endpoints {
                let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

                info!(
                    "Deleting zone {} from secondary endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
                );

                zone_manager
                    .delete_zone(&spec.zone_name, &pod_endpoint)
                    .await
                    .context(format!(
                        "Failed to delete zone {} from secondary endpoint {} (instance: {})",
                        spec.zone_name, pod_endpoint, instance_name
                    ))?;

                debug!(
                    "Successfully deleted zone {} from secondary endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
                );

                secondary_endpoints_deleted += 1;
            }
        }

        info!(
            "Successfully deleted zone {} from {} secondary endpoint(s) for cluster {}",
            spec.zone_name, secondary_endpoints_deleted, spec.cluster_ref
        );
    }

    // Note: We don't need to reload after delzone because:
    // 1. rndc delzone immediately removes the zone from BIND9's running config
    // 2. BIND9 will clean up the zone file and journal files automatically

    Ok(())
}

/// Find `Bind9Instance` resources matching a label selector
async fn find_matching_instances(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<String>> {
    use crate::crd::Bind9Instance;

    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    // Build label selector string
    let label_selector = build_label_selector(selector);

    let params = kube::api::ListParams::default();
    let params = if let Some(selector_str) = label_selector {
        params.labels(&selector_str)
    } else {
        params
    };

    let instances = api.list(&params).await?;

    let instance_names: Vec<String> = instances
        .items
        .iter()
        .map(kube::ResourceExt::name_any)
        .collect();

    Ok(instance_names)
}

/// Build a Kubernetes label selector string from our `LabelSelector`
pub(crate) fn build_label_selector(selector: &crate::crd::LabelSelector) -> Option<String> {
    let mut parts = Vec::new();

    // Add match labels
    if let Some(labels) = &selector.match_labels {
        for (key, value) in labels {
            parts.push(format!("{key}={value}"));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Helper struct for pod information
#[derive(Clone)]
struct PodInfo {
    name: String,
    ip: String,
    instance_name: String,
}

/// Find ALL PRIMARY pods for the given `Bind9Cluster`
///
/// Returns all running pods for PRIMARY instances in the cluster to ensure zone changes
/// are applied to all primary replicas consistently.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search in
/// * `cluster_name` - Name of the `Bind9Cluster`
///
/// # Returns
///
/// A vector of `PodInfo` containing all running PRIMARY pods
async fn find_all_primary_pods(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
) -> Result<Vec<PodInfo>> {
    use crate::crd::{Bind9Instance, ServerRole};

    // First, find all Bind9Instance resources that belong to this cluster and have role=primary
    let instance_api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let instances = instance_api.list(&ListParams::default()).await?;

    let mut primary_instances = Vec::new();
    for instance in instances.items {
        if instance.spec.cluster_ref == cluster_name && instance.spec.role == ServerRole::Primary {
            if let Some(name) = instance.metadata.name {
                primary_instances.push(name);
            }
        }
    }

    if primary_instances.is_empty() {
        return Err(anyhow!(
            "No PRIMARY Bind9Instance resources found for cluster {cluster_name} in namespace {namespace}"
        ));
    }

    info!(
        "Found {} PRIMARY instance(s) for cluster {}: {:?}",
        primary_instances.len(),
        cluster_name,
        primary_instances
    );

    // Now find all pods for these primary instances
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let mut all_pod_infos = Vec::new();

    for instance_name in &primary_instances {
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
                });
                debug!("Found running pod {} with IP {}", pod_name, pod_ip);
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

/// Find all SECONDARY pods for a given cluster.
///
/// Returns IP addresses of all running secondary pods in the cluster.
/// These IPs are used for configuring also-notify and allow-transfer on primary zones.
async fn find_all_secondary_pod_ips(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
) -> Result<Vec<String>> {
    info!("Finding SECONDARY pods for cluster {}", cluster_name);

    // Find all Bind9Instance resources with role=SECONDARY for this cluster
    let instance_api: Api<crate::crd::Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let lp = ListParams::default().labels(&format!("cluster={cluster_name},role=secondary"));

    let instances = instance_api.list(&lp).await.context(format!(
        "Failed to list SECONDARY Bind9Instance resources for cluster {cluster_name}"
    ))?;

    let secondary_instances: Vec<String> = instances
        .items
        .iter()
        .filter_map(|inst| inst.metadata.name.clone())
        .collect();

    if secondary_instances.is_empty() {
        info!("No SECONDARY instances found for cluster {cluster_name} - zone transfers will not be configured");
        return Ok(Vec::new());
    }

    info!(
        "Found {} SECONDARY instance(s) for cluster {}: {:?}",
        secondary_instances.len(),
        cluster_name,
        secondary_instances
    );

    // Find all pods for these secondary instances
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let mut secondary_ips = Vec::new();

    for instance_name in &secondary_instances {
        // List pods with label selector matching the instance
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
                .ok_or_else(|| anyhow!("Pod has no name"))?;

            // Get pod IP
            if let Some(pod_ip) = pod.status.as_ref().and_then(|s| s.pod_ip.as_ref()) {
                // Check if pod is running
                let phase = pod
                    .status
                    .as_ref()
                    .and_then(|s| s.phase.as_ref())
                    .map(String::as_str);

                if phase == Some("Running") {
                    secondary_ips.push(pod_ip.clone());
                    debug!(
                        "Found running secondary pod {} with IP {}",
                        pod_name, pod_ip
                    );
                } else {
                    debug!(
                        "Skipping secondary pod {} (phase: {:?}, not running)",
                        pod_name, phase
                    );
                }
            }
        }
    }

    info!(
        "Found {} running SECONDARY pod IP(s) for cluster {}: {:?}",
        secondary_ips.len(),
        cluster_name,
        secondary_ips
    );

    Ok(secondary_ips)
}

/// Find all PRIMARY pod IPs for a given cluster.
///
/// Returns IP addresses of all running primary pods in the cluster.
/// These IPs are used for configuring primaries on secondary zones.
async fn find_all_primary_pod_ips(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
) -> Result<Vec<String>> {
    info!("Finding PRIMARY pod IPs for cluster {}", cluster_name);

    let primary_pods = find_all_primary_pods(client, namespace, cluster_name).await?;

    let primary_ips: Vec<String> = primary_pods.iter().map(|pod| pod.ip.clone()).collect();

    info!(
        "Found {} running PRIMARY pod IP(s) for cluster {}: {:?}",
        primary_ips.len(),
        cluster_name,
        primary_ips
    );

    Ok(primary_ips)
}

/// Find all SECONDARY pods for a given cluster.
///
/// Returns structured pod information including IP, name, and instance name.
/// Similar to `find_all_primary_pods` but for secondary instances.
async fn find_all_secondary_pods(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
) -> Result<Vec<PodInfo>> {
    use crate::crd::{Bind9Instance, ServerRole};

    // Find all Bind9Instance resources with role=SECONDARY for this cluster
    let instance_api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let instances = instance_api.list(&ListParams::default()).await?;

    let mut secondary_instances = Vec::new();
    for instance in instances.items {
        if instance.spec.cluster_ref == cluster_name && instance.spec.role == ServerRole::Secondary
        {
            if let Some(name) = instance.metadata.name {
                secondary_instances.push(name);
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

    // Find all pods for these secondary instances
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let mut all_pod_infos = Vec::new();

    for instance_name in &secondary_instances {
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
                });
                debug!(
                    "Found running secondary pod {} with IP {}",
                    pod_name, pod_ip
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

/// Load RNDC key from the instance's secret
async fn load_rndc_key(
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

/// Check if the operator is running inside a Kubernetes cluster
///
/// Detects the environment by checking for the presence of the Kubernetes service account token,
/// which is automatically mounted in all pods running in the cluster.
///
/// Returns `true` if running in-cluster, `false` if running locally (e.g., via kubectl proxy)
fn is_running_in_cluster() -> bool {
    std::path::Path::new("/var/run/secrets/kubernetes.io/serviceaccount/token").exists()
}

/// Update a single condition on the `DNSZone` status
///
/// This is a lightweight status update that only modifies the conditions field.
/// Use this for intermediate status updates during reconciliation.
async fn update_condition(
    client: &Client,
    dnszone: &DNSZone,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
) -> Result<()> {
    let api: Api<DNSZone> =
        Api::namespaced(client.clone(), &dnszone.namespace().unwrap_or_default());

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        last_transition_time: Some(Utc::now().to_rfc3339()),
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
    };

    // Preserve existing status fields, only update conditions
    let current_status = dnszone.status.as_ref();
    let new_status = DNSZoneStatus {
        conditions: vec![condition],
        observed_generation: current_status
            .and_then(|s| s.observed_generation)
            .or(dnszone.metadata.generation),
        record_count: current_status.and_then(|s| s.record_count),
        secondary_ips: current_status.and_then(|s| s.secondary_ips.clone()),
    };

    let patch = json!({
        "status": new_status
    });

    api.patch_status(
        &dnszone.name_any(),
        &PatchParams::default(),
        &Patch::Merge(&patch),
    )
    .await?;

    debug!(
        "Updated DNSZone {}/{} condition: type={}, status={}, reason={}",
        dnszone.namespace().unwrap_or_default(),
        dnszone.name_any(),
        condition_type,
        status,
        reason
    );

    Ok(())
}

/// Update `DNSZone` status including secondary IPs
async fn update_status_with_secondaries(
    client: &Client,
    dnszone: &DNSZone,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
    secondary_ips: Vec<String>,
) -> Result<()> {
    let api: Api<DNSZone> =
        Api::namespaced(client.clone(), &dnszone.namespace().unwrap_or_default());

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        last_transition_time: Some(Utc::now().to_rfc3339()),
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
    };

    let new_status = DNSZoneStatus {
        conditions: vec![condition],
        observed_generation: dnszone.metadata.generation,
        record_count: dnszone.status.as_ref().and_then(|s| s.record_count),
        secondary_ips: if secondary_ips.is_empty() {
            None
        } else {
            Some(secondary_ips)
        },
    };

    let patch = json!({
        "status": new_status
    });

    api.patch_status(
        &dnszone.name_any(),
        &PatchParams::default(),
        &Patch::Merge(&patch),
    )
    .await?;

    info!(
        "Updated DNSZone {}/{} status: {}={}",
        dnszone.namespace().unwrap_or_default(),
        dnszone.name_any(),
        condition_type,
        status
    );

    Ok(())
}

async fn update_status(
    client: &Client,
    dnszone: &DNSZone,
    condition_type: &str,
    status: &str,
    message: &str,
) -> Result<()> {
    let api: Api<DNSZone> =
        Api::namespaced(client.clone(), &dnszone.namespace().unwrap_or_default());

    // Check if status has actually changed
    let current_status = &dnszone.status;
    let status_changed = if let Some(current) = current_status {
        if let Some(current_condition) = current.conditions.first() {
            // Check if condition changed
            current_condition.r#type != condition_type
                || current_condition.status != status
                || current_condition.message.as_deref() != Some(message)
        } else {
            // No conditions exist, need to update
            true
        }
    } else {
        // No status exists, need to update
        true
    };

    // Only update if status has changed
    if !status_changed {
        debug!(
            namespace = %dnszone.namespace().unwrap_or_default(),
            name = %dnszone.name_any(),
            "Status unchanged, skipping update"
        );
        info!(
            "DNSZone {}/{} status unchanged, skipping update",
            dnszone.namespace().unwrap_or_default(),
            dnszone.name_any()
        );
        return Ok(());
    }

    debug!(
        condition_type = %condition_type,
        status = %status,
        message = %message,
        "Preparing status update"
    );

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(condition_type.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(Utc::now().to_rfc3339()),
    };

    let new_status = DNSZoneStatus {
        conditions: vec![condition],
        observed_generation: dnszone.metadata.generation,
        record_count: None,
        secondary_ips: dnszone
            .status
            .as_ref()
            .and_then(|s| s.secondary_ips.clone()),
    };

    info!(
        "Updating DNSZone {}/{} status",
        dnszone.namespace().unwrap_or_default(),
        dnszone.name_any()
    );

    let patch = json!({ "status": new_status });
    api.patch_status(
        &dnszone.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(())
}

/// Structure representing an endpoint (pod IP and port)
#[derive(Debug, Clone)]
pub struct EndpointAddress {
    /// IP address of the pod
    pub ip: String,
    /// Container port number
    pub port: i32,
}

/// Execute an operation on all endpoints of all primary instances in a cluster.
///
/// This helper function handles the common pattern of:
/// 1. Finding all primary pods for a cluster
/// 2. Collecting unique instance names
/// 3. Optionally loading RNDC key from the first instance
/// 4. Getting endpoints for each instance
/// 5. Executing a provided operation on each endpoint
///
/// # Arguments
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the cluster
/// * `cluster_ref` - Name of the `Bind9Cluster`
/// * `with_rndc_key` - Whether to load RNDC key from first instance
/// * `operation` - Async closure to execute for each endpoint
///   - Arguments: `(pod_endpoint: String, instance_name: String, rndc_key: Option<RndcKeyData>)`
///   - Returns: `Result<()>`
///
/// # Returns
/// Returns `Ok((first_endpoint, total_count))` where:
/// - `first_endpoint` - Optional first endpoint encountered (useful for NOTIFY operations)
/// - `total_count` - Total number of endpoints processed
///
/// # Errors
/// Returns error if:
/// - No primary pods found for the cluster
/// - Failed to load RNDC key (if requested)
/// - Failed to get endpoints for any instance
/// - The operation closure returns an error for any endpoint
pub async fn for_each_primary_endpoint<F, Fut>(
    client: &Client,
    namespace: &str,
    cluster_ref: &str,
    with_rndc_key: bool,
    port_name: &str,
    operation: F,
) -> Result<(Option<String>, usize)>
where
    F: Fn(String, String, Option<RndcKeyData>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    // Find all PRIMARY pods to get the unique instance names
    let primary_pods = find_all_primary_pods(client, namespace, cluster_ref).await?;

    info!(
        "Found {} PRIMARY pod(s) for cluster {}",
        primary_pods.len(),
        cluster_ref
    );

    // Load RNDC key from the first primary instance's secret if requested
    // All instances in the cluster share the same RNDC key
    let key_data = if with_rndc_key {
        let first_instance_name = &primary_pods
            .first()
            .ok_or_else(|| anyhow!("No PRIMARY instances found for cluster {cluster_ref}"))?
            .instance_name;

        Some(load_rndc_key(client, namespace, first_instance_name).await?)
    } else {
        None
    };

    // Collect unique instance names from the primary pods
    // Each instance may have multiple pods (replicas)
    let mut instance_names: Vec<String> = primary_pods
        .iter()
        .map(|pod| pod.instance_name.clone())
        .collect();
    instance_names.sort();
    instance_names.dedup();

    info!(
        "Found {} primary instance(s) for cluster {}: {:?}",
        instance_names.len(),
        cluster_ref,
        instance_names
    );

    let mut first_endpoint: Option<String> = None;
    let mut total_endpoints = 0;

    // Loop through each primary instance and get its endpoints
    // Important: With EmptyDir storage (per-pod, non-shared), each primary pod maintains its own
    // zone files. We need to process ALL pods across ALL instances.
    for instance_name in &instance_names {
        info!(
            "Getting endpoints for instance {} in cluster {}",
            instance_name, cluster_ref
        );

        // Get all endpoints for this instance's service
        // The Endpoints API gives us pod IPs with their container ports (not service ports)
        let endpoints = get_endpoint(client, namespace, instance_name, port_name).await?;

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

            // Execute the operation on this endpoint
            operation(pod_endpoint, instance_name.clone(), key_data.clone()).await?;

            total_endpoints += 1;
        }
    }

    Ok((first_endpoint, total_endpoints))
}

/// Get all endpoints for a service with a specific port name
///
/// Looks up the Kubernetes Endpoints object associated with a service and returns
/// all pod IP addresses with their corresponding container ports.
///
/// When connecting directly to pod IPs, you must use the container port from the endpoints,
/// not the service port.
///
/// # Arguments
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the service/endpoints
/// * `service_name` - Name of the service (endpoints have the same name)
/// * `port_name` - Name of the port to lookup (e.g., "http", "dns-tcp")
///
/// # Returns
/// Vector of `EndpointAddress` containing pod IP and container port pairs
///
/// # Errors
/// Returns error if:
/// - Endpoints object doesn't exist
/// - Port name not found in any endpoint subset
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
