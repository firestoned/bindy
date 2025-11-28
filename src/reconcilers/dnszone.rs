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
use tracing::info;

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

    // Extract spec
    let spec = &dnszone.spec;

    // Find PRIMARY instance pod from the cluster
    let primary_pod = find_primary_pod(&client, &namespace, &spec.cluster_ref).await?;

    info!(
        "Found PRIMARY pod {} for cluster {}",
        primary_pod.name, spec.cluster_ref
    );

    // Load RNDC key from instance secret
    let key_data = load_rndc_key(&client, &namespace, &spec.cluster_ref).await?;

    // Build server address (using the instance service)
    let server = format!("{}.{}.svc.cluster.local:953", spec.cluster_ref, namespace);

    // For now, we use rndc addzone to dynamically add the zone.
    // The zone type will be "master" (primary) and we'll use a default zone file location.
    // In production, you may want to pre-configure zones in named.conf and just reload.
    let zone_file = format!("/var/lib/bind/{}.zone", spec.zone_name);

    zone_manager
        .add_zone(&spec.zone_name, "master", &zone_file, &server, &key_data)
        .await?;

    info!(
        "Added zone {} on PRIMARY instance {} (cluster: {})",
        spec.zone_name, spec.cluster_ref, spec.cluster_ref
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
    let primary_pod = find_primary_pod(&client, &namespace, &spec.cluster_ref).await?;

    info!(
        "Found PRIMARY pod {} for cluster {}",
        primary_pod.name, spec.cluster_ref
    );

    // Load RNDC key from instance secret
    let key_data = load_rndc_key(&client, &namespace, &spec.cluster_ref).await?;

    // Build server address
    let server = format!("{}.{}.svc.cluster.local:953", spec.cluster_ref, namespace);

    // Delete zone using rndc
    zone_manager
        .delete_zone(&spec.zone_name, &server, &key_data)
        .await?;

    info!(
        "Deleted zone {} from cluster {}",
        spec.zone_name, spec.cluster_ref
    );

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
struct PodInfo {
    name: String,
}

/// Find a PRIMARY pod for the given `Bind9Instance`
async fn find_primary_pod(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<PodInfo> {
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);

    // List pods with label selector matching the instance
    let label_selector = format!("app=bind9,instance={instance_name}");
    let lp = ListParams::default().labels(&label_selector);

    let pods = pod_api.list(&lp).await?;

    if pods.items.is_empty() {
        return Err(anyhow!(
            "No pods found for Bind9Instance {instance_name} in namespace {namespace}"
        ));
    }

    // For now, just use the first pod. In the future, we could look for
    // a pod with a specific label like "role=primary" or check if it's
    // running and ready.
    let pod = &pods.items[0];
    let pod_name = pod
        .metadata
        .name
        .as_ref()
        .ok_or_else(|| anyhow!("Pod has no name"))?
        .clone();

    info!(
        "Found pod {} for instance {} (total pods: {})",
        pod_name,
        instance_name,
        pods.items.len()
    );

    Ok(PodInfo { name: pod_name })
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

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(condition_type.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(Utc::now().to_rfc3339()),
    };

    let status = DNSZoneStatus {
        conditions: vec![condition],
        observed_generation: dnszone.metadata.generation,
        record_count: None,
    };

    let patch = json!({ "status": status });
    api.patch_status(
        &dnszone.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(())
}
