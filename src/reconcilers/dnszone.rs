//! DNS zone reconciliation logic.
//!
//! This module handles the creation and management of DNS zones on BIND9 servers.
//! It supports both primary (master) and secondary (slave) zone configurations.

use crate::crd::{Condition, DNSZone, DNSZoneStatus};
use anyhow::Result;
use chrono::Utc;
use kube::{
    api::{Patch, PatchParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use tracing::info;

/// Reconciles a DNSZone resource.
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
/// * `client` - Kubernetes API client for finding matching Bind9Instances
/// * `dnszone` - The DNSZone resource to reconcile
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
///     let manager = Bind9Manager::new("/etc/bind/zones".to_string());
///     reconcile_dnszone(client, zone, &manager).await?;
///     Ok(())
/// }
/// ```
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

    // Find matching Bind9Instance resources using label selector
    let instances = find_matching_instances(&client, &namespace, &spec.instance_selector).await?;

    if instances.is_empty() {
        update_status(
            &client,
            &dnszone,
            "NoMatchingInstances",
            "Warning",
            "No Bind9Instance resources match the instance selector",
        )
        .await?;
        return Ok(());
    }

    // Determine zone type (default to "primary")
    let zone_type = spec.zone_type.as_deref().unwrap_or("primary");

    match zone_type {
        "primary" => {
            // Create primary zone file
            if let Some(soa) = &spec.soa_record {
                zone_manager.create_zone_file(&spec.zone_name, soa, spec.ttl.unwrap_or(3600))?;

                info!(
                    "Created primary zone file for {} with {} matching instances",
                    spec.zone_name,
                    instances.len()
                );
            } else {
                update_status(
                    &client,
                    &dnszone,
                    "ConfigurationError",
                    "False",
                    "Primary zone requires soa_record configuration",
                )
                .await?;
                return Ok(());
            }
        }
        "secondary" => {
            // Create secondary zone configuration
            if let Some(secondary_config) = &spec.secondary_config {
                zone_manager
                    .create_secondary_zone(&spec.zone_name, &secondary_config.primary_servers)?;

                info!(
                    "Created secondary zone configuration for {} with {} primary servers and {} matching instances",
                    spec.zone_name,
                    secondary_config.primary_servers.len(),
                    instances.len()
                );
            } else {
                update_status(
                    &client,
                    &dnszone,
                    "ConfigurationError",
                    "False",
                    "Secondary zone requires secondary_config with primary_servers",
                )
                .await?;
                return Ok(());
            }
        }
        _ => {
            update_status(
                &client,
                &dnszone,
                "ConfigurationError",
                "False",
                &format!(
                    "Invalid zone type: {}. Must be 'primary' or 'secondary'",
                    zone_type
                ),
            )
            .await?;
            return Ok(());
        }
    }

    // Update status to success
    update_status(
        &client,
        &dnszone,
        "Ready",
        "True",
        &format!(
            "{} zone created for {} instances",
            zone_type,
            instances.len()
        ),
    )
    .await?;

    Ok(())
}

/// Deletes a DNS zone and its associated zone files.
///
/// # Arguments
///
/// * `_client` - Kubernetes API client (unused, for future extensions)
/// * `dnszone` - The DNSZone resource to delete
/// * `zone_manager` - BIND9 manager for removing zone files
///
/// # Returns
///
/// * `Ok(())` - If zone was deleted successfully
/// * `Err(_)` - If zone deletion failed
pub async fn delete_dnszone(
    _client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    info!("Deleting DNSZone: {}", name);

    zone_manager.delete_zone(&spec.zone_name)?;

    Ok(())
}

/// Find Bind9Instance resources matching a label selector
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

    let instance_names: Vec<String> = instances.items.iter().map(|i| i.name_any()).collect();

    Ok(instance_names)
}

/// Build a Kubernetes label selector string from our LabelSelector
fn build_label_selector(selector: &crate::crd::LabelSelector) -> Option<String> {
    let mut parts = Vec::new();

    // Add match labels
    if let Some(labels) = &selector.match_labels {
        for (key, value) in labels.iter() {
            parts.push(format!("{}={}", key, value));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Update the status of a DNSZone
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
