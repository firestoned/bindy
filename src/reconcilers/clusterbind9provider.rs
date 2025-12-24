// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 global cluster (cluster-scoped) reconciliation logic.
//!
//! This module handles the lifecycle of cluster-scoped BIND9 cluster resources.
//! It manages `Bind9Instance` resources across all namespaces that reference this
//! global cluster.
//!
//! The key difference from namespace-scoped `Bind9Cluster` is that:
//! - `ClusterBind9Provider` resources are cluster-scoped (no namespace)
//! - Instances can be created in any namespace and reference the global cluster
//! - The reconciler must list instances across all namespaces

use crate::constants::{API_GROUP_VERSION, KIND_BIND9_CLUSTER, KIND_CLUSTER_BIND9_PROVIDER};
use crate::crd::{
    Bind9Cluster, Bind9ClusterStatus, Bind9Instance, ClusterBind9Provider, Condition,
};
use crate::labels::FINALIZER_BIND9_CLUSTER;
use crate::reconcilers::finalizers::{
    ensure_cluster_finalizer, handle_cluster_deletion, FinalizerCleanup,
};
use crate::status_reasons::{
    CONDITION_TYPE_READY, REASON_ALL_READY, REASON_NOT_READY, REASON_NO_CHILDREN,
    REASON_PARTIALLY_READY,
};
use anyhow::Result;
use chrono::Utc;
use kube::{
    api::{ListParams, Patch, PatchParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use tracing::{debug, error, info, warn};

/// Implement finalizer cleanup for `ClusterBind9Provider`.
///
/// This handles deletion of all managed `Bind9Cluster` resources when the
/// global cluster is deleted.
#[async_trait::async_trait]
impl FinalizerCleanup for ClusterBind9Provider {
    async fn cleanup(&self, client: &Client) -> Result<()> {
        use crate::labels::{
            BINDY_CLUSTER_LABEL, BINDY_MANAGED_BY_LABEL, MANAGED_BY_CLUSTER_BIND9_PROVIDER,
        };
        use kube::api::DeleteParams;

        let name = self.name_any();

        // Step 1: Delete all managed Bind9Cluster resources
        info!(
            "Deleting managed Bind9Cluster resources for global cluster {}",
            name
        );

        let clusters_api: Api<Bind9Cluster> = Api::all(client.clone());
        let all_clusters = clusters_api.list(&ListParams::default()).await?;

        // Filter clusters managed by this global cluster
        let managed_clusters: Vec<_> = all_clusters
            .items
            .iter()
            .filter(|c| {
                c.metadata.labels.as_ref().is_some_and(|labels| {
                    labels.get(BINDY_MANAGED_BY_LABEL)
                        == Some(&MANAGED_BY_CLUSTER_BIND9_PROVIDER.to_string())
                        && labels.get(BINDY_CLUSTER_LABEL) == Some(&name.clone())
                })
            })
            .collect();

        if !managed_clusters.is_empty() {
            info!(
                "Found {} managed Bind9Cluster resources to delete for global cluster {}",
                managed_clusters.len(),
                name
            );

            for managed_cluster in managed_clusters {
                let cluster_name = managed_cluster.name_any();
                let cluster_namespace = managed_cluster.namespace().unwrap_or_default();

                info!(
                    "Deleting managed Bind9Cluster {}/{} for global cluster {}",
                    cluster_namespace, cluster_name, name
                );

                let api: Api<Bind9Cluster> = Api::namespaced(client.clone(), &cluster_namespace);
                match api.delete(&cluster_name, &DeleteParams::default()).await {
                    Ok(_) => {
                        info!(
                            "Successfully deleted Bind9Cluster {}/{}",
                            cluster_namespace, cluster_name
                        );
                    }
                    Err(e) => {
                        // If already deleted or not found, that's fine
                        if e.to_string().contains("NotFound") {
                            debug!(
                                "Bind9Cluster {}/{} already deleted",
                                cluster_namespace, cluster_name
                            );
                        } else {
                            error!(
                                "Failed to delete Bind9Cluster {}/{}: {}",
                                cluster_namespace, cluster_name, e
                            );
                            return Err(e.into());
                        }
                    }
                }
            }
        }

        // Step 2: Check for orphaned Bind9Instance resources (warn only, don't delete)
        // Note: Instances will be cleaned up by their parent Bind9Cluster's finalizer
        let instances_api: Api<Bind9Instance> = Api::all(client.clone());
        let instances = instances_api.list(&ListParams::default()).await?;

        let referencing_instances: Vec<_> = instances
            .items
            .iter()
            .filter(|inst| inst.spec.cluster_ref == name)
            .collect();

        if !referencing_instances.is_empty() {
            warn!(
                "ClusterBind9Provider {} still has {} referencing instances. \
                These will be cleaned up by their parent Bind9Cluster finalizers.",
                name,
                referencing_instances.len()
            );
        }

        Ok(())
    }
}

/// Reconciles a cluster-scoped `ClusterBind9Provider` resource.
///
/// This function:
/// 1. Checks if the cluster is being deleted and handles cleanup
/// 2. Adds finalizer if not present
/// 3. Lists all `Bind9Instance` resources across all namespaces that reference this global cluster
/// 4. Updates cluster status based on instance health
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `ClusterBind9Provider` resource to reconcile
///
/// # Returns
///
/// * `Ok(())` - If reconciliation succeeded
/// * `Err(_)` - If status update failed
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or status update fails.
pub async fn reconcile_clusterbind9provider(
    client: Client,
    cluster: ClusterBind9Provider,
) -> Result<()> {
    let name = cluster.name_any();

    info!("Reconciling ClusterBind9Provider: {}", name);
    debug!(
        name = %name,
        generation = ?cluster.metadata.generation,
        "Starting ClusterBind9Provider reconciliation (cluster-scoped)"
    );

    // Handle deletion if cluster is being deleted
    if cluster.metadata.deletion_timestamp.is_some() {
        return handle_cluster_deletion(&client, &cluster, FINALIZER_BIND9_CLUSTER).await;
    }

    // Ensure finalizer is present
    ensure_cluster_finalizer(&client, &cluster, FINALIZER_BIND9_CLUSTER).await?;

    // Check if spec has changed using the standard generation check
    let current_generation = cluster.metadata.generation;
    let observed_generation = cluster.status.as_ref().and_then(|s| s.observed_generation);

    // Only reconcile resources if spec changed or we haven't processed this resource yet
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping resource reconciliation",
            current_generation
        );
        // Update status from current instance states (only patches if status changed)
        update_cluster_status(&client, &cluster).await?;
        return Ok(());
    }

    debug!(
        "Reconciliation needed: current_generation={:?}, observed_generation={:?}",
        current_generation, observed_generation
    );

    // Reconcile namespace-scoped Bind9Cluster resources
    // The Bind9Cluster reconciler will handle creating Bind9Instance resources
    // This ensures proper delegation: GlobalCluster → Cluster → Instance
    reconcile_namespace_clusters(&client, &cluster).await?;

    // Update cluster status based on instances across all namespaces
    update_cluster_status(&client, &cluster).await?;

    Ok(())
}

/// Reconciles namespace-scoped `Bind9Cluster` resources for this global cluster.
///
/// This function creates or updates a namespace-scoped `Bind9Cluster` resource in each
/// namespace where this global cluster has instances. The namespace-scoped cluster
/// will then create the `ConfigMap` that instances need.
///
/// This delegation pattern ensures:
/// - `ConfigMaps` exist before instances try to mount them
/// - The `Bind9Cluster` reconciler handles `ConfigMap` creation logic
/// - No duplication of `ConfigMap` creation code
///
/// # Errors
///
/// Returns an error if listing instances or creating/updating clusters fails.
#[allow(clippy::too_many_lines)]
async fn reconcile_namespace_clusters(
    client: &Client,
    cluster_provider: &ClusterBind9Provider,
) -> Result<()> {
    use crate::crd::{Bind9Cluster, Bind9ClusterSpec};
    use crate::labels::{
        BINDY_CLUSTER_LABEL, BINDY_MANAGED_BY_LABEL, MANAGED_BY_CLUSTER_BIND9_PROVIDER,
    };
    use kube::api::{ListParams, PostParams};
    use std::collections::{BTreeMap, HashSet};

    let cluster_provider_name = cluster_provider.name_any();

    // Get target namespace from spec or default to operator's namespace
    let target_namespace = cluster_provider.spec.namespace.as_ref().map_or_else(
        || std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "dns-system".to_string()),
        std::clone::Clone::clone,
    );

    debug!(
        "Reconciling namespace-scoped Bind9Cluster for global cluster {} in namespace {}",
        cluster_provider_name, target_namespace
    );

    // List all instances across all namespaces that reference this global cluster
    let instances_api: Api<Bind9Instance> = Api::all(client.clone());
    let all_instances = instances_api.list(&ListParams::default()).await?;

    // Find unique namespaces that have instances referencing this global cluster
    let namespaces: HashSet<String> = all_instances
        .items
        .iter()
        .filter(|inst| inst.spec.cluster_ref == cluster_provider_name)
        .filter_map(kube::ResourceExt::namespace)
        .collect();

    // If no instances reference this global cluster, still create cluster in target namespace
    let namespaces_to_reconcile: HashSet<String> = if namespaces.is_empty() {
        let mut set = HashSet::new();
        set.insert(target_namespace);
        set
    } else {
        namespaces
    };

    debug!(
        "Found {} namespace(s) needing Bind9Cluster for global cluster {}",
        namespaces_to_reconcile.len(),
        cluster_provider_name
    );

    // For each namespace, create or update a namespace-scoped Bind9Cluster
    for namespace in namespaces_to_reconcile {
        // Use the global cluster name directly (don't append "-cluster")
        let cluster_name = cluster_provider_name.clone();

        info!(
            "Creating/updating Bind9Cluster {}/{} for global cluster {}",
            namespace, cluster_name, cluster_provider_name
        );

        // Create labels to mark this as managed by the global cluster
        let mut labels = BTreeMap::new();
        labels.insert(
            BINDY_MANAGED_BY_LABEL.to_string(),
            MANAGED_BY_CLUSTER_BIND9_PROVIDER.to_string(),
        );
        labels.insert(
            BINDY_CLUSTER_LABEL.to_string(),
            cluster_provider_name.clone(),
        );

        // Create ownerReference to global cluster (cluster-scoped can own namespace-scoped)
        let owner_ref = k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference {
            api_version: API_GROUP_VERSION.to_string(),
            kind: KIND_CLUSTER_BIND9_PROVIDER.to_string(),
            name: cluster_provider_name.clone(),
            uid: cluster_provider.metadata.uid.clone().unwrap_or_default(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        };

        // Build the Bind9Cluster spec by cloning the global cluster's common spec
        let cluster_spec = Bind9ClusterSpec {
            common: cluster_provider.spec.common.clone(),
        };

        let cluster = Bind9Cluster {
            metadata: kube::api::ObjectMeta {
                name: Some(cluster_name.clone()),
                namespace: Some(namespace.clone()),
                labels: Some(labels),
                owner_references: Some(vec![owner_ref]),
                ..Default::default()
            },
            spec: cluster_spec,
            status: None,
        };

        let api: Api<Bind9Cluster> = Api::namespaced(client.clone(), &namespace);

        // Try to create the Bind9Cluster
        match api.create(&PostParams::default(), &cluster).await {
            Ok(_) => {
                info!(
                    "Successfully created Bind9Cluster {}/{}",
                    namespace, cluster_name
                );
            }
            Err(e) => {
                // If already exists, PATCH it to ensure spec is up to date
                if e.to_string().contains("AlreadyExists") {
                    debug!(
                        "Bind9Cluster {}/{} already exists, patching with updated spec",
                        namespace, cluster_name
                    );

                    // Build a complete patch object for server-side apply
                    let patch = serde_json::json!({
                        "apiVersion": API_GROUP_VERSION,
                        "kind": KIND_BIND9_CLUSTER,
                        "metadata": {
                            "name": cluster_name,
                            "namespace": namespace,
                            "ownerReferences": cluster.metadata.owner_references,
                        },
                        "spec": cluster.spec,
                    });

                    // Apply the patch to update the spec
                    match api
                        .patch(
                            &cluster_name,
                            &PatchParams::apply("bindy-controller").force(),
                            &Patch::Apply(&patch),
                        )
                        .await
                    {
                        Ok(_) => {
                            info!(
                                "Successfully patched Bind9Cluster {}/{} with updated spec",
                                namespace, cluster_name
                            );
                        }
                        Err(patch_err) => {
                            warn!(
                                "Failed to patch Bind9Cluster {}/{}: {}",
                                namespace, cluster_name, patch_err
                            );
                            return Err(patch_err.into());
                        }
                    }
                } else {
                    warn!(
                        "Failed to create Bind9Cluster {}/{}: {}",
                        namespace, cluster_name, e
                    );
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}

/// Updates the global cluster status based on instances across all namespaces.
///
/// # Errors
///
/// Returns an error if status update fails.
async fn update_cluster_status(client: &Client, cluster: &ClusterBind9Provider) -> Result<()> {
    let name = cluster.name_any();

    // List all Bind9Instance resources across all namespaces
    let instances_api: Api<Bind9Instance> = Api::all(client.clone());
    let lp = ListParams::default();
    let all_instances = instances_api.list(&lp).await?;

    // Filter instances that reference this global cluster
    let instances: Vec<_> = all_instances
        .items
        .into_iter()
        .filter(|inst| inst.spec.cluster_ref == name)
        .collect();

    debug!(
        "Found {} instances referencing ClusterBind9Provider {}",
        instances.len(),
        name
    );

    // Calculate cluster status based on instances
    let new_status = calculate_cluster_status(&instances, cluster.metadata.generation);

    // Check if status has actually changed before patching
    let status_changed = if let Some(current_status) = &cluster.status {
        // Check if instance count or ready count changed
        if current_status.instance_count != new_status.instance_count
            || current_status.ready_instances != new_status.ready_instances
        {
            true
        } else if let Some(current_condition) = current_status.conditions.first() {
            // Check if condition changed
            let new_condition = new_status.conditions.first();
            match new_condition {
                Some(new_cond) => {
                    current_condition.r#type != new_cond.r#type
                        || current_condition.status != new_cond.status
                        || current_condition.message != new_cond.message
                }
                None => true, // New status has no condition, definitely changed
            }
        } else {
            // Current status has no condition but new status might
            !new_status.conditions.is_empty()
        }
    } else {
        // No current status, definitely need to update
        true
    };

    // Only update if status has changed
    if !status_changed {
        debug!(
            "Status unchanged for ClusterBind9Provider {}, skipping patch",
            name
        );
        return Ok(());
    }

    // Update status
    let api: Api<ClusterBind9Provider> = Api::all(client.clone());
    let status_patch = json!({
        "status": new_status
    });

    api.patch_status(&name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await?;

    debug!("Updated status for ClusterBind9Provider: {}", name);
    Ok(())
}

/// Calculates the cluster status based on instance states.
///
/// # Arguments
///
/// * `instances` - List of instances belonging to this cluster
/// * `generation` - Current generation of the cluster resource
///
/// # Returns
///
/// A `Bind9ClusterStatus` with calculated conditions and instance list
#[must_use]
pub fn calculate_cluster_status(
    instances: &[Bind9Instance],
    generation: Option<i64>,
) -> Bind9ClusterStatus {
    let now = Utc::now();

    // Count ready instances
    let ready_instances = instances
        .iter()
        .filter(|inst| {
            inst.status
                .as_ref()
                .and_then(|s| s.conditions.iter().find(|c| c.r#type == "Ready"))
                .is_some_and(|c| c.status == "True")
        })
        .count();

    let total_instances = instances.len();

    // Determine cluster ready condition using standard reasons
    let (status, reason, message) = if total_instances == 0 {
        (
            "False",
            REASON_NO_CHILDREN,
            "No instances found for this cluster".to_string(),
        )
    } else if ready_instances == total_instances {
        (
            "True",
            REASON_ALL_READY,
            format!("All {total_instances} instances are ready"),
        )
    } else if ready_instances > 0 {
        (
            "False",
            REASON_PARTIALLY_READY,
            format!("{ready_instances}/{total_instances} instances are ready"),
        )
    } else {
        (
            "False",
            REASON_NOT_READY,
            "No instances are ready".to_string(),
        )
    };

    // Collect instance names (with namespace for global clusters)
    let instance_names: Vec<String> = instances
        .iter()
        .map(|inst| {
            let name = inst.name_any();
            let ns = inst.namespace().unwrap_or_default();
            format!("{ns}/{name}")
        })
        .collect();

    Bind9ClusterStatus {
        conditions: vec![Condition {
            r#type: CONDITION_TYPE_READY.to_string(),
            status: status.to_string(),
            reason: Some(reason.to_string()),
            message: Some(message.clone()),
            last_transition_time: Some(now.to_rfc3339()),
        }],
        instances: instance_names,
        observed_generation: generation,
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        instance_count: Some(total_instances as i32),
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        ready_instances: Some(ready_instances as i32),
    }
}

/// Deletes a cluster-scoped `ClusterBind9Provider` resource.
///
/// This is called when the cluster resource is explicitly deleted by the user.
/// It delegates to the reconciler which handles the deletion via finalizers.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `ClusterBind9Provider` resource being deleted
///
/// # Returns
///
/// * `Ok(())` - If deletion handling succeeded
/// * `Err(_)` - If deletion failed
///
/// # Errors
///
/// Returns an error if finalizer cleanup or API operations fail.
pub async fn delete_clusterbind9provider(
    client: Client,
    cluster: ClusterBind9Provider,
) -> Result<()> {
    let name = cluster.name_any();
    info!("Deleting ClusterBind9Provider: {}", name);

    // Deletion is handled via the reconciler through finalizers
    reconcile_clusterbind9provider(client, cluster).await
}
