// Copyright (c) 2025 Erick Bourgeois, firestoned
#![allow(dead_code)]
// SPDX-License-Identifier: MIT

//! DNS zone reconciliation logic.
//!
//! This module handles the creation and management of DNS zones on BIND9 servers.
//! It supports both primary (master) and secondary (slave) zone configurations.

use crate::bind9::RndcKeyData;
use crate::crd::{Condition, DNSZone, DNSZoneStatus};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use k8s_openapi::api::core::v1::{Pod, Secret};
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

    // Find ALL PRIMARY instance pods from the cluster
    debug!(cluster_ref = %spec.cluster_ref, "Finding PRIMARY pods for cluster");
    let primary_pods = find_all_primary_pods(&client, &namespace, &spec.cluster_ref).await?;

    info!(
        "Found {} PRIMARY pod(s) for cluster {}",
        primary_pods.len(),
        spec.cluster_ref
    );

    // Load RNDC key from the first primary instance's secret
    // All instances in the cluster share the same RNDC key
    let first_instance_name = &primary_pods
        .first()
        .ok_or_else(|| {
            anyhow!(
                "No PRIMARY instances found for cluster {}",
                spec.cluster_ref
            )
        })?
        .instance_name;

    let key_data = load_rndc_key(&client, &namespace, first_instance_name).await?;

    // For now, we use rndc addzone to dynamically add the zone.
    // The zone type will be "master" (primary) and we'll use a default zone file location.
    // BIND9 will create the zone file in /var/cache/bind when records are added via dynamic updates.
    // This directory is mounted as an EmptyDir volume (or PVC for persistence).
    let zone_file = format!("/var/cache/bind/{}.zone", spec.zone_name);

    // Add zone via Service endpoint (not individual pods)
    // This approach works best with shared storage (ReadWriteMany PVC):
    // 1. Service load balancer routes to one pod
    // 2. That pod creates the zone on shared storage
    // 3. We then reload all pods so they pick up the new zone
    // This avoids concurrent writes and file locking issues across pods
    // Use the instance name (not cluster name) as each instance has its own service
    //
    // When running outside the cluster (e.g., local development with kubectl port-forward),
    // use the pod IP directly since service DNS won't resolve
    let service_endpoint = if is_running_in_cluster() {
        format!("{first_instance_name}.{namespace}.svc.cluster.local:953")
    } else {
        // Running outside cluster - use first pod IP directly
        let first_pod_ip = &primary_pods
            .first()
            .ok_or_else(|| anyhow!("No PRIMARY pods found for cluster {}", spec.cluster_ref))?
            .ip;
        format!("{first_pod_ip}:953")
    };

    zone_manager
        .add_zone(
            &spec.zone_name,
            "master",
            &zone_file,
            &service_endpoint,
            &key_data,
        )
        .await?;

    info!(
        "Added zone {} via service endpoint {}",
        spec.zone_name, service_endpoint
    );

    // Note: We don't need to reload after addzone because:
    // 1. rndc addzone immediately adds the zone to BIND9's running config
    // 2. The zone file will be created automatically when records are added via dynamic updates
    // 3. Reloading would fail if the zone file doesn't exist yet

    // Notify secondaries about the new zone
    // This triggers zone transfer (AXFR) from primary to secondaries
    info!(
        "Notifying secondaries about new zone {} for cluster {}",
        spec.zone_name, spec.cluster_ref
    );
    if let Err(e) = zone_manager
        .notify_zone(&spec.zone_name, &service_endpoint, &key_data)
        .await
    {
        // Don't fail if NOTIFY fails - the zone was successfully created
        // Secondaries will sync via SOA refresh timer
        warn!(
            "Failed to notify secondaries for zone {}: {}. Secondaries will sync via SOA refresh timer.",
            spec.zone_name, e
        );
    }

    info!(
        "Successfully added zone {} to cluster {} ({} primary pod(s))",
        spec.zone_name,
        spec.cluster_ref,
        primary_pods.len()
    );

    // Update status to success
    update_status(
        &client,
        &dnszone,
        "Ready",
        "True",
        &format!("Zone created for cluster: {}", spec.cluster_ref),
    )
    .await?;

    Ok(())
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

    // Find PRIMARY instance pod from the cluster
    let primary_pods = find_all_primary_pods(&client, &namespace, &spec.cluster_ref).await?;

    info!(
        "Found {} PRIMARY pod(s) for cluster {}",
        primary_pods.len(),
        spec.cluster_ref
    );

    // Load RNDC key from the first primary instance's secret
    // All instances in the cluster share the same RNDC key
    let first_instance_name = &primary_pods
        .first()
        .ok_or_else(|| {
            anyhow!(
                "No PRIMARY instances found for cluster {}",
                spec.cluster_ref
            )
        })?
        .instance_name;

    let key_data = load_rndc_key(&client, &namespace, first_instance_name).await?;

    // Delete zone via Service endpoint
    // With shared storage, we delete from one pod and the file is removed from shared storage
    // Use the instance name (not cluster name) as each instance has its own service
    //
    // When running outside the cluster, use the pod IP directly since service DNS won't resolve
    let service_endpoint = if is_running_in_cluster() {
        format!("{first_instance_name}.{namespace}.svc.cluster.local:953")
    } else {
        // Running outside cluster - use first pod IP directly
        let first_pod_ip = &primary_pods
            .first()
            .ok_or_else(|| anyhow!("No PRIMARY pods found for cluster {}", spec.cluster_ref))?
            .ip;
        format!("{first_pod_ip}:953")
    };

    zone_manager
        .delete_zone(&spec.zone_name, &service_endpoint, &key_data)
        .await?;

    info!(
        "Successfully deleted zone {} from cluster {} via service endpoint {}",
        spec.zone_name, spec.cluster_ref, service_endpoint
    );

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

/// Update the status of a `DNSZone`
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
