// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 cluster reconciliation logic.
//!
//! This module handles the lifecycle of BIND9 cluster resources in Kubernetes.
//! It manages the `Bind9Instance` resources that belong to a cluster and updates
//! the cluster status to reflect the overall health.
//!
//! ## Module Structure
//!
//! - [`config`] - Cluster `ConfigMap` management
//! - [`drift`] - Instance drift detection
//! - [`instances`] - Instance lifecycle management
//! - [`status_helpers`] - Status calculation and updates
//! - [`types`] - Shared types and imports

// Submodules
pub mod config;
pub mod drift;
pub mod instances;
pub mod status_helpers;
pub mod types;

// Re-export public APIs for external use
pub use instances::{create_managed_instance, delete_bind9cluster, delete_managed_instance};
pub use status_helpers::calculate_cluster_status;

// Internal imports
use config::create_or_update_cluster_configmap;
use drift::detect_instance_drift;
use instances::reconcile_managed_instances;
use status_helpers::update_status;
#[allow(clippy::wildcard_imports)]
use types::*;

use crate::labels::FINALIZER_BIND9_CLUSTER;
use crate::reconcilers::finalizers::{ensure_finalizer, handle_deletion, FinalizerCleanup};

/// Implement cleanup trait for `Bind9Cluster` finalizer management.
#[async_trait::async_trait]
impl FinalizerCleanup for Bind9Cluster {
    async fn cleanup(&self, client: &Client) -> Result<()> {
        let namespace = self.namespace().unwrap_or_default();
        let name = self.name_any();
        instances::delete_cluster_instances(client, &namespace, &name).await
    }
}

/// Reconciles a `Bind9Cluster` resource.
///
/// This function orchestrates the complete cluster reconciliation workflow:
/// 1. Checks if the cluster is being deleted and handles cleanup
/// 2. Adds finalizer if not present
/// 3. Detects drift in managed instances
/// 4. Creates/updates cluster `ConfigMap`
/// 5. Reconciles managed instances
/// 6. Updates cluster status based on instance health
///
/// # Arguments
///
/// * `ctx` - Operator context with Kubernetes client and reflector stores
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
pub async fn reconcile_bind9cluster(ctx: Arc<Context>, cluster: Bind9Cluster) -> Result<()> {
    let client = ctx.client.clone();
    let namespace = cluster.namespace().unwrap_or_default();
    let name = cluster.name_any();

    info!("Reconciling Bind9Cluster: {}/{}", namespace, name);
    debug!(
        namespace = %namespace,
        name = %name,
        generation = ?cluster.metadata.generation,
        "Starting Bind9Cluster reconciliation"
    );

    // Handle deletion if cluster is being deleted
    if cluster.metadata.deletion_timestamp.is_some() {
        return handle_deletion(&client, &cluster, FINALIZER_BIND9_CLUSTER).await;
    }

    // Ensure finalizer is present
    ensure_finalizer(&client, &cluster, FINALIZER_BIND9_CLUSTER).await?;

    // Check if spec has changed using the standard generation check
    let current_generation = cluster.metadata.generation;
    let observed_generation = cluster.status.as_ref().and_then(|s| s.observed_generation);

    // Only reconcile spec-related resources if spec changed OR drift detected
    let spec_changed =
        crate::reconcilers::should_reconcile(current_generation, observed_generation);

    // DRIFT DETECTION: Check if managed instances match desired state
    let drift_detected = if spec_changed {
        false
    } else {
        detect_instance_drift(&client, &cluster, &namespace, &name).await?
    };

    if spec_changed || drift_detected {
        if drift_detected {
            info!(
                "Spec unchanged but instance drift detected for cluster {}/{}",
                namespace, name
            );
        } else {
            debug!(
                "Reconciliation needed: current_generation={:?}, observed_generation={:?}",
                current_generation, observed_generation
            );
        }

        // Create or update shared cluster ConfigMap
        create_or_update_cluster_configmap(&client, &cluster).await?;

        // Reconcile managed instances (create/update as needed)
        reconcile_managed_instances(&ctx, &cluster).await?;
    } else {
        debug!(
            "Spec unchanged (generation={:?}) and no drift detected, skipping resource reconciliation",
            current_generation
        );
    }

    // ALWAYS list and analyze cluster instances to update status
    // This ensures status reflects current instance health even when spec hasn't changed
    let instances: Vec<Bind9Instance> =
        list_cluster_instances(&client, &cluster, &namespace, &name).await?;

    // Calculate cluster status from instances
    let (instance_count, ready_instances, instance_names, conditions) =
        calculate_cluster_status(&instances, &namespace, &name);

    // Update cluster status with all conditions
    update_status(
        &client,
        &cluster,
        conditions,
        instance_count,
        ready_instances,
        instance_names,
    )
    .await?;

    Ok(())
}

/// List all `Bind9Instance` resources that reference a cluster.
///
/// Filters instances in the namespace that have `clusterRef` matching the cluster name.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `Bind9Cluster` to find instances for
/// * `namespace` - Cluster namespace
/// * `name` - Cluster name
///
/// # Returns
///
/// Vector of `Bind9Instance` resources that reference this cluster
///
/// # Errors
///
/// Returns an error if:
/// - Failed to list instances
/// - Failed to update cluster status on error
async fn list_cluster_instances(
    client: &Client,
    cluster: &Bind9Cluster,
    namespace: &str,
    name: &str,
) -> Result<Vec<Bind9Instance>> {
    // List all Bind9Instance resources in the namespace that reference this cluster
    let instances_api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let list_params = ListParams::default();
    debug!(namespace = %namespace, "Listing Bind9Instance resources");

    match instances_api.list(&list_params).await {
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
            Ok(filtered)
        }
        Err(e) => {
            error!(
                "Failed to list Bind9Instance resources for cluster {}/{}: {}",
                namespace, name, e
            );

            // Update status to show error
            let error_condition = Condition {
                r#type: CONDITION_TYPE_READY.to_string(),
                status: "False".to_string(),
                reason: Some(REASON_NOT_READY.to_string()),
                message: Some(format!("Failed to list instances: {e}")),
                last_transition_time: Some(Utc::now().to_rfc3339()),
            };
            update_status(client, cluster, vec![error_condition], 0, 0, vec![]).await?;

            Err(e.into())
        }
    }
}
