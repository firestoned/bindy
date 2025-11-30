// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 cluster reconciliation logic.
//!
//! This module handles the lifecycle of BIND9 cluster resources in Kubernetes.
//! It manages the `Bind9Instance` resources that belong to a cluster and updates
//! the cluster status to reflect the overall health.

use crate::crd::{Bind9Cluster, Bind9ClusterStatus, Bind9Instance, Condition};
use anyhow::Result;
use chrono::Utc;
use kube::{
    api::{ListParams, Patch, PatchParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use tracing::{debug, error, info};

/// Reconciles a `Bind9Cluster` resource.
///
/// This function:
/// 1. Lists all `Bind9Instance` resources that reference this cluster
/// 2. Checks their status to determine cluster health
/// 3. Updates the cluster status with instance information
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `Bind9Cluster` resource to reconcile
///
/// # Returns
///
/// * `Ok(())` - If reconciliation succeeded
/// * `Err(_)` - If status update failed
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or status update fails.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_bind9cluster(client: Client, cluster: Bind9Cluster) -> Result<()> {
    let namespace = cluster.namespace().unwrap_or_default();
    let name = cluster.name_any();

    info!("Reconciling Bind9Cluster: {}/{}", namespace, name);
    debug!(
        namespace = %namespace,
        name = %name,
        generation = ?cluster.metadata.generation,
        "Starting Bind9Cluster reconciliation"
    );

    // List all Bind9Instance resources in the namespace that reference this cluster
    let instances_api: Api<Bind9Instance> = Api::namespaced(client.clone(), &namespace);
    let list_params = ListParams::default();
    debug!(namespace = %namespace, "Listing Bind9Instance resources");

    let instances = match instances_api.list(&list_params).await {
        Ok(list) => {
            debug!(
                total_instances_in_ns = list.items.len(),
                "Listed Bind9Instance resources"
            );
            // Filter instances that reference this cluster
            let filtered: Vec<_> = list
                .items
                .into_iter()
                .filter(|instance| instance.spec.cluster_ref == name)
                .collect();
            debug!(
                filtered_instances = filtered.len(),
                cluster_ref = %name,
                "Filtered instances by cluster reference"
            );
            filtered
        }
        Err(e) => {
            error!(
                "Failed to list Bind9Instance resources for cluster {}/{}: {}",
                namespace, name, e
            );

            // Update status to show error
            update_status(
                &client,
                &cluster,
                "Ready",
                "False",
                &format!("Failed to list instances: {e}"),
                0,
                0,
                vec![],
            )
            .await?;

            return Err(e.into());
        }
    };

    // Count total instances and ready instances
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let instance_count = instances.len() as i32;
    let instance_names: Vec<String> = instances.iter().map(ResourceExt::name_any).collect();

    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let ready_instances = instances
        .iter()
        .filter(|instance| {
            instance
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True")
        })
        .count() as i32;

    info!(
        "Bind9Cluster {}/{} has {} instances, {} ready",
        namespace, name, instance_count, ready_instances
    );

    // Determine cluster status
    let (status, message) = if instance_count == 0 {
        debug!("No instances found for cluster");
        ("False", "No instances found for this cluster".to_string())
    } else if ready_instances == instance_count {
        debug!("All instances ready");
        ("True", format!("All {instance_count} instances are ready"))
    } else if ready_instances > 0 {
        debug!(ready_instances, instance_count, "Cluster progressing");
        (
            "False",
            format!("Progressing: {ready_instances}/{instance_count} instances ready"),
        )
    } else {
        debug!("Waiting for instances to become ready");
        ("False", "Waiting for instances to become ready".to_string())
    };

    debug!(
        status = %status,
        message = %message,
        "Determined cluster status"
    );

    // Update cluster status
    update_status(
        &client,
        &cluster,
        "Ready",
        status,
        &message,
        instance_count,
        ready_instances,
        instance_names,
    )
    .await?;

    Ok(())
}

/// Update the status of a `Bind9Cluster`
#[allow(clippy::too_many_arguments)]
async fn update_status(
    client: &Client,
    cluster: &Bind9Cluster,
    condition_type: &str,
    status: &str,
    message: &str,
    instance_count: i32,
    ready_instances: i32,
    instances: Vec<String>,
) -> Result<()> {
    let api: Api<Bind9Cluster> =
        Api::namespaced(client.clone(), &cluster.namespace().unwrap_or_default());

    // Check if status has actually changed
    let current_status = &cluster.status;
    let status_changed = if let Some(current) = current_status {
        // Check if counts changed
        if current.instance_count != Some(instance_count)
            || current.ready_instances != Some(ready_instances)
            || current.instances != instances
        {
            true
        } else if let Some(current_condition) = current.conditions.first() {
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
            namespace = %cluster.namespace().unwrap_or_default(),
            name = %cluster.name_any(),
            "Status unchanged, skipping update"
        );
        info!(
            "Bind9Cluster {}/{} status unchanged, skipping update",
            cluster.namespace().unwrap_or_default(),
            cluster.name_any()
        );
        return Ok(());
    }

    debug!(
        condition_type = %condition_type,
        status = %status,
        message = %message,
        instance_count,
        ready_instances,
        instances_count = instances.len(),
        "Preparing status update"
    );

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(condition_type.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(Utc::now().to_rfc3339()),
    };

    let new_status = Bind9ClusterStatus {
        conditions: vec![condition],
        observed_generation: cluster.metadata.generation,
        instance_count: Some(instance_count),
        ready_instances: Some(ready_instances),
        instances,
    };

    info!(
        "Updating Bind9Cluster {}/{} status: {} instances, {} ready",
        cluster.namespace().unwrap_or_default(),
        cluster.name_any(),
        instance_count,
        ready_instances
    );

    let patch = json!({ "status": new_status });
    api.patch_status(
        &cluster.name_any(),
        &PatchParams::apply("bindy-controller"),
        &Patch::Merge(&patch),
    )
    .await?;

    Ok(())
}

/// Delete handler for `Bind9Cluster` resources (cleanup logic)
///
/// Currently a no-op as instances have owner references and will be cleaned up automatically.
///
/// # Errors
///
/// This function currently never returns an error, but returns `Result` for API consistency.
pub async fn delete_bind9cluster(_client: Client, _cluster: Bind9Cluster) -> Result<()> {
    // No cleanup needed - instances have owner references and will be deleted automatically
    Ok(())
}
