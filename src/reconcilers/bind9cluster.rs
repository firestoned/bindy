// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 cluster reconciliation logic.
//!
//! This module handles the lifecycle of BIND9 cluster resources in Kubernetes.
//! It manages the `Bind9Instance` resources that belong to a cluster and updates
//! the cluster status to reflect the overall health.

use crate::constants::{API_GROUP_VERSION, KIND_BIND9_CLUSTER, KIND_BIND9_INSTANCE};
use crate::context::Context;
use crate::crd::{
    Bind9Cluster, Bind9ClusterStatus, Bind9Instance, Bind9InstanceSpec, Condition, ServerRole,
};
use crate::labels::{
    BINDY_CLUSTER_LABEL, BINDY_INSTANCE_INDEX_ANNOTATION, BINDY_MANAGED_BY_LABEL,
    BINDY_RECONCILE_TRIGGER_ANNOTATION, BINDY_ROLE_LABEL, FINALIZER_BIND9_CLUSTER, K8S_PART_OF,
    MANAGED_BY_BIND9_CLUSTER, PART_OF_BINDY, ROLE_PRIMARY, ROLE_SECONDARY,
};
use crate::reconcilers::finalizers::{ensure_finalizer, handle_deletion, FinalizerCleanup};
use crate::status_reasons::{
    bind9_instance_condition_type, CONDITION_TYPE_READY, REASON_ALL_READY, REASON_NOT_READY,
    REASON_NO_CHILDREN, REASON_PARTIALLY_READY, REASON_READY,
};
use anyhow::Result;
use chrono::Utc;
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{ConfigMap, Secret, Service},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{
    api::{DeleteParams, ListParams, Patch, PatchParams, PostParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Implement cleanup trait for `Bind9Cluster` finalizer management
#[async_trait::async_trait]
impl FinalizerCleanup for Bind9Cluster {
    async fn cleanup(&self, client: &Client) -> Result<()> {
        let namespace = self.namespace().unwrap_or_default();
        let name = self.name_any();
        delete_cluster_instances(client, &namespace, &name).await
    }
}

/// Reconciles a `Bind9Cluster` resource.
///
/// This function:
/// 1. Checks if the cluster is being deleted and handles cleanup
/// 2. Adds finalizer if not present
/// 3. Creates/updates cluster `ConfigMap`
/// 4. Reconciles managed instances
/// 5. Updates cluster status based on instance health
///
/// # Arguments
///
/// * `ctx` - Controller context with Kubernetes client and reflector stores
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

/// List all `Bind9Instance` resources that reference a cluster
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

/// Calculate cluster status from instance health
///
/// Analyzes instance list to determine cluster readiness.
///
/// # Arguments
///
/// * `instances` - List of `Bind9Instance` resources for the cluster
/// * `namespace` - Cluster namespace (for logging)
/// * `name` - Cluster name (for logging)
///
/// # Returns
///
/// Tuple of:
/// - `instance_count` - Total number of instances
/// - `ready_instances` - Number of ready instances
/// - `instance_names` - Names of all instances
/// - `status` - Cluster status ("True" or "False")
/// - `message` - Status message
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub(crate) fn calculate_cluster_status(
    instances: &[Bind9Instance],
    namespace: &str,
    name: &str,
) -> (i32, i32, Vec<String>, Vec<Condition>) {
    // Count total instances and ready instances
    let instance_count = instances.len() as i32;
    let instance_names: Vec<String> = instances.iter().map(ResourceExt::name_any).collect();

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

    // Create instance-level conditions
    let mut instance_conditions = Vec::new();
    for (index, instance) in instances.iter().enumerate() {
        let instance_name = instance.name_any();
        let is_instance_ready = instance
            .status
            .as_ref()
            .and_then(|status| status.conditions.first())
            .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

        let (status, reason, message) = if is_instance_ready {
            (
                "True",
                REASON_READY,
                format!("Instance {instance_name} is ready"),
            )
        } else {
            (
                "False",
                REASON_NOT_READY,
                format!("Instance {instance_name} is not ready"),
            )
        };

        instance_conditions.push(Condition {
            r#type: bind9_instance_condition_type(index),
            status: status.to_string(),
            reason: Some(reason.to_string()),
            message: Some(message),
            last_transition_time: Some(Utc::now().to_rfc3339()),
        });
    }

    // Create encompassing Ready condition
    let (encompassing_status, encompassing_reason, encompassing_message) = if instance_count == 0 {
        debug!("No instances found for cluster");
        (
            "False",
            REASON_NO_CHILDREN,
            "No instances found for this cluster".to_string(),
        )
    } else if ready_instances == instance_count {
        debug!("All instances ready");
        (
            "True",
            REASON_ALL_READY,
            format!("All {instance_count} instances are ready"),
        )
    } else if ready_instances > 0 {
        debug!(ready_instances, instance_count, "Cluster progressing");
        (
            "False",
            REASON_PARTIALLY_READY,
            format!("{ready_instances}/{instance_count} instances are ready"),
        )
    } else {
        debug!("Waiting for instances to become ready");
        (
            "False",
            REASON_NOT_READY,
            "No instances are ready".to_string(),
        )
    };

    let encompassing_condition = Condition {
        r#type: CONDITION_TYPE_READY.to_string(),
        status: encompassing_status.to_string(),
        reason: Some(encompassing_reason.to_string()),
        message: Some(encompassing_message.clone()),
        last_transition_time: Some(Utc::now().to_rfc3339()),
    };

    // Combine encompassing condition + instance-level conditions
    let mut all_conditions = vec![encompassing_condition];
    all_conditions.extend(instance_conditions);

    debug!(
        status = %encompassing_status,
        message = %encompassing_message,
        num_conditions = all_conditions.len(),
        "Determined cluster status"
    );

    (
        instance_count,
        ready_instances,
        instance_names,
        all_conditions,
    )
}

/// Update the status of a `Bind9Cluster` with multiple conditions
async fn update_status(
    client: &Client,
    cluster: &Bind9Cluster,
    conditions: Vec<Condition>,
    instance_count: i32,
    ready_instances: i32,
    instances: Vec<String>,
) -> Result<()> {
    let api: Api<Bind9Cluster> =
        Api::namespaced(client.clone(), &cluster.namespace().unwrap_or_default());

    // Check if status has actually changed
    let current_status = &cluster.status;
    let status_changed =
        if let Some(current) = current_status {
            // Check if counts changed
            if current.instance_count != Some(instance_count)
                || current.ready_instances != Some(ready_instances)
                || current.instances != instances
            {
                true
            } else {
                // Check if any condition changed
                if current.conditions.len() == conditions.len() {
                    // Compare each condition
                    current.conditions.iter().zip(conditions.iter()).any(
                        |(current_cond, new_cond)| {
                            current_cond.r#type != new_cond.r#type
                                || current_cond.status != new_cond.status
                                || current_cond.message != new_cond.message
                                || current_cond.reason != new_cond.reason
                        },
                    )
                } else {
                    true
                }
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
        instance_count,
        ready_instances,
        instances_count = instances.len(),
        num_conditions = conditions.len(),
        "Preparing status update"
    );

    let new_status = Bind9ClusterStatus {
        conditions,
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

/// Detects if the actual managed instances match the desired replica counts.
///
/// Compares the number of primary and secondary instances that exist against
/// the desired replica counts in the cluster spec.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `Bind9Cluster` to check
/// * `namespace` - Cluster namespace
/// * `name` - Cluster name
///
/// # Returns
///
/// * `Ok(true)` - Drift detected (instances don't match desired state)
/// * `Ok(false)` - No drift (instances match desired state)
/// * `Err(_)` - Failed to check drift
///
/// # Errors
///
/// Returns an error if listing instances fails.
async fn detect_instance_drift(
    client: &Client,
    cluster: &Bind9Cluster,
    namespace: &str,
    name: &str,
) -> Result<bool> {
    // Get desired replica counts from spec
    let desired_primary = cluster
        .spec
        .common
        .primary
        .as_ref()
        .and_then(|p| p.replicas)
        .unwrap_or(0);

    let desired_secondary = cluster
        .spec
        .common
        .secondary
        .as_ref()
        .and_then(|s| s.replicas)
        .unwrap_or(0);

    // List existing managed instances
    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let instances = api.list(&ListParams::default()).await?;

    // Filter for managed instances of this cluster
    let managed_instances: Vec<_> = instances
        .items
        .into_iter()
        .filter(|instance| {
            instance.metadata.labels.as_ref().is_some_and(|labels| {
                labels.get(BINDY_MANAGED_BY_LABEL) == Some(&MANAGED_BY_BIND9_CLUSTER.to_string())
                    && labels.get(BINDY_CLUSTER_LABEL) == Some(&name.to_string())
            })
        })
        .collect();

    // Count by role
    let actual_primary = managed_instances
        .iter()
        .filter(|i| i.spec.role == ServerRole::Primary)
        .count();

    let actual_secondary = managed_instances
        .iter()
        .filter(|i| i.spec.role == ServerRole::Secondary)
        .count();

    // Drift detected if counts don't match
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let drift = actual_primary != desired_primary as usize
        || actual_secondary != desired_secondary as usize;

    if drift {
        info!(
            "Instance drift detected for cluster {}/{}: desired (primary={}, secondary={}), actual (primary={}, secondary={})",
            namespace, name, desired_primary, desired_secondary, actual_primary, actual_secondary
        );
    }

    Ok(drift)
}

/// Reconcile managed `Bind9Instance` resources for a cluster
///
/// This function ensures the correct number of primary and secondary instances exist
/// based on the cluster spec. It creates missing instances and adds management labels.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `Bind9Cluster` resource
///
/// # Errors
///
/// Returns an error if:
/// - Failed to list existing instances
/// - Failed to create new instances
#[allow(clippy::too_many_lines)]
async fn reconcile_managed_instances(ctx: &Context, cluster: &Bind9Cluster) -> Result<()> {
    let client = ctx.client.clone();
    let namespace = cluster.namespace().unwrap_or_default();
    let cluster_name = cluster.name_any();

    info!(
        "Reconciling managed instances for cluster {}/{}",
        namespace, cluster_name
    );

    // Get desired replica counts from spec
    let primary_replicas = cluster
        .spec
        .common
        .primary
        .as_ref()
        .and_then(|p| p.replicas)
        .unwrap_or(0);

    let secondary_replicas = cluster
        .spec
        .common
        .secondary
        .as_ref()
        .and_then(|s| s.replicas)
        .unwrap_or(0);

    debug!(
        "Desired replicas: {} primary, {} secondary",
        primary_replicas, secondary_replicas
    );

    if primary_replicas == 0 && secondary_replicas == 0 {
        debug!(
            "No instances requested for cluster {}/{}",
            namespace, cluster_name
        );
        return Ok(());
    }

    // List existing managed instances
    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), &namespace);
    let instances = api.list(&ListParams::default()).await?;

    // Filter for managed instances of this cluster
    let managed_instances: Vec<_> = instances
        .items
        .into_iter()
        .filter(|instance| {
            // Check if instance has management labels
            instance.metadata.labels.as_ref().is_some_and(|labels| {
                labels.get(BINDY_MANAGED_BY_LABEL) == Some(&MANAGED_BY_BIND9_CLUSTER.to_string())
                    && labels.get(BINDY_CLUSTER_LABEL) == Some(&cluster_name)
            })
        })
        .collect();

    debug!(
        "Found {} managed instances for cluster {}/{}",
        managed_instances.len(),
        namespace,
        cluster_name
    );

    // Separate by role
    let existing_primary: Vec<_> = managed_instances
        .iter()
        .filter(|i| i.spec.role == ServerRole::Primary)
        .collect();

    let existing_secondary: Vec<_> = managed_instances
        .iter()
        .filter(|i| i.spec.role == ServerRole::Secondary)
        .collect();

    debug!(
        "Existing instances: {} primary, {} secondary",
        existing_primary.len(),
        existing_secondary.len()
    );

    // Create ownerReference to the Bind9Cluster
    let owner_ref = k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference {
        api_version: API_GROUP_VERSION.to_string(),
        kind: KIND_BIND9_CLUSTER.to_string(),
        name: cluster_name.clone(),
        uid: cluster.metadata.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    };

    // Handle scale-up: Create missing primary instances
    // CRITICAL: Compare desired vs current state to find missing instances
    // Build set of desired instance names, compare with existing, create the difference
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let mut primaries_to_create = 0;
    {
        // Build set of desired primary instance names based on replica count
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let desired_primary_names: std::collections::HashSet<String> = (0..(primary_replicas
            as usize))
            .map(|i| format!("{cluster_name}-primary-{i}"))
            .collect();

        // Build set of existing primary instance names
        let existing_primary_names: std::collections::HashSet<String> = existing_primary
            .iter()
            .map(|instance| instance.name_any())
            .collect();

        // Find missing instances (desired - existing)
        let missing_primaries: Vec<_> = desired_primary_names
            .difference(&existing_primary_names)
            .collect();

        // Create each missing instance
        for instance_name in missing_primaries {
            // Extract index from name (e.g., "production-dns-primary-0" -> 0)
            let index = instance_name
                .rsplit('-')
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);

            create_managed_instance_with_owner(
                &client,
                &namespace,
                &cluster_name,
                ServerRole::Primary,
                index,
                &cluster.spec.common,
                Some(owner_ref.clone()),
            )
            .await?;
            primaries_to_create += 1;
        }
    }

    // Handle scale-down: Delete excess primary instances
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let primaries_to_delete = existing_primary
        .len()
        .saturating_sub(primary_replicas as usize);
    if primaries_to_delete > 0 {
        // Sort by index descending to delete highest-indexed instances first
        let mut sorted_primary: Vec<_> = existing_primary.iter().collect();
        sorted_primary.sort_by_key(|instance| {
            instance
                .metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get(BINDY_INSTANCE_INDEX_ANNOTATION))
                .and_then(|idx| idx.parse::<usize>().ok())
                .unwrap_or(0)
        });
        sorted_primary.reverse();

        for instance in sorted_primary.iter().take(primaries_to_delete) {
            let instance_name = instance.name_any();
            delete_managed_instance(&client, &namespace, &instance_name).await?;
        }
    }

    // Handle scale-up: Create missing secondary instances
    // CRITICAL: Compare desired vs current state to find missing instances
    // Build set of desired instance names, compare with existing, create the difference
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let mut secondaries_to_create = 0;
    {
        // Build set of desired secondary instance names based on replica count
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let desired_secondary_names: std::collections::HashSet<String> = (0..(secondary_replicas
            as usize))
            .map(|i| format!("{cluster_name}-secondary-{i}"))
            .collect();

        // Build set of existing secondary instance names
        let existing_secondary_names: std::collections::HashSet<String> = existing_secondary
            .iter()
            .map(|instance| instance.name_any())
            .collect();

        // Find missing instances (desired - existing)
        let missing_secondaries: Vec<_> = desired_secondary_names
            .difference(&existing_secondary_names)
            .collect();

        // Create each missing instance
        for instance_name in missing_secondaries {
            // Extract index from name (e.g., "production-dns-secondary-0" -> 0)
            let index = instance_name
                .rsplit('-')
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);

            create_managed_instance_with_owner(
                &client,
                &namespace,
                &cluster_name,
                ServerRole::Secondary,
                index,
                &cluster.spec.common,
                Some(owner_ref.clone()),
            )
            .await?;
            secondaries_to_create += 1;
        }
    }

    // Handle scale-down: Delete excess secondary instances
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let secondaries_to_delete = existing_secondary
        .len()
        .saturating_sub(secondary_replicas as usize);
    if secondaries_to_delete > 0 {
        // Sort by index descending to delete highest-indexed instances first
        let mut sorted_secondary: Vec<_> = existing_secondary.iter().collect();
        sorted_secondary.sort_by_key(|instance| {
            instance
                .metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get(BINDY_INSTANCE_INDEX_ANNOTATION))
                .and_then(|idx| idx.parse::<usize>().ok())
                .unwrap_or(0)
        });
        sorted_secondary.reverse();

        for instance in sorted_secondary.iter().take(secondaries_to_delete) {
            let instance_name = instance.name_any();
            delete_managed_instance(&client, &namespace, &instance_name).await?;
        }
    }

    if primaries_to_create > 0
        || secondaries_to_create > 0
        || primaries_to_delete > 0
        || secondaries_to_delete > 0
    {
        info!(
            "Scaled cluster {}/{}: created {} primary, {} secondary; deleted {} primary, {} secondary",
            namespace,
            cluster_name,
            primaries_to_create,
            secondaries_to_create,
            primaries_to_delete,
            secondaries_to_delete
        );
    } else {
        debug!(
            "Cluster {}/{} already at desired scale",
            namespace, cluster_name
        );
    }

    // Update existing managed instances to match cluster spec (declarative reconciliation)
    update_existing_managed_instances(
        &client,
        &namespace,
        &cluster_name,
        &cluster.spec.common,
        &managed_instances,
    )
    .await?;

    // Ensure child resources (ConfigMaps, Secrets, Services, Deployments) exist for all managed instances
    ensure_managed_instance_resources(&client, cluster, &managed_instances).await?;

    Ok(())
}

/// Update existing managed instances to match the cluster's current spec.
///
/// This implements true declarative reconciliation - comparing the desired state (from cluster spec)
/// with the actual state (existing instance specs) and updating any instances that have drifted.
///
/// This ensures that when the cluster's `spec.common` changes (e.g., bindcar version, volumes,
/// config references), all managed instances are updated to reflect the new configuration.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace containing the instances
/// * `cluster_name` - Name of the parent cluster
/// * `common_spec` - The cluster's common spec (source of truth)
/// * `managed_instances` - List of existing managed instances to check
///
/// # Errors
///
/// Returns an error if patching instances fails
async fn update_existing_managed_instances(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    common_spec: &crate::crd::Bind9ClusterCommonSpec,
    managed_instances: &[Bind9Instance],
) -> Result<()> {
    if managed_instances.is_empty() {
        return Ok(());
    }

    let instance_api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let mut updated_count = 0;

    for instance in managed_instances {
        let instance_name = instance.name_any();

        // Build the desired spec based on current cluster configuration
        let desired_bindcar_config = common_spec
            .global
            .as_ref()
            .and_then(|g| g.bindcar_config.clone());

        // Check if instance spec needs updating by comparing key fields
        let needs_update = instance.spec.version != common_spec.version
            || instance.spec.image != common_spec.image
            || instance.spec.config_map_refs != common_spec.config_map_refs
            || instance.spec.volumes != common_spec.volumes
            || instance.spec.volume_mounts != common_spec.volume_mounts
            || instance.spec.bindcar_config != desired_bindcar_config;

        if needs_update {
            debug!(
                "Instance {}/{} spec differs from cluster spec, updating",
                namespace, instance_name
            );

            // Build updated instance spec - preserve instance-specific fields, update cluster-inherited fields
            let updated_spec = Bind9InstanceSpec {
                cluster_ref: instance.spec.cluster_ref.clone(),
                role: instance.spec.role.clone(),
                replicas: instance.spec.replicas, // Preserve instance replicas (always 1 for managed)
                version: common_spec.version.clone(),
                image: common_spec.image.clone(),
                config_map_refs: common_spec.config_map_refs.clone(),
                config: None, // Managed instances inherit from cluster
                primary_servers: instance.spec.primary_servers.clone(), // Preserve if set
                volumes: common_spec.volumes.clone(),
                volume_mounts: common_spec.volume_mounts.clone(),
                rndc_secret_ref: instance.spec.rndc_secret_ref.clone(), // Preserve if set
                storage: instance.spec.storage.clone(),                 // Preserve if set
                bindcar_config: desired_bindcar_config,
            };

            // Use server-side apply to update the instance spec
            let patch = serde_json::json!({
                "apiVersion": API_GROUP_VERSION,
                "kind": KIND_BIND9_INSTANCE,
                "metadata": {
                    "name": instance_name,
                    "namespace": namespace,
                },
                "spec": updated_spec,
            });

            match instance_api
                .patch(
                    &instance_name,
                    &PatchParams::apply("bindy-controller").force(),
                    &Patch::Apply(&patch),
                )
                .await
            {
                Ok(_) => {
                    info!(
                        "Updated managed instance {}/{} to match cluster spec",
                        namespace, instance_name
                    );
                    updated_count += 1;
                }
                Err(e) => {
                    error!(
                        "Failed to update managed instance {}/{}: {}",
                        namespace, instance_name, e
                    );
                    return Err(e.into());
                }
            }
        } else {
            debug!(
                "Instance {}/{} spec matches cluster spec, no update needed",
                namespace, instance_name
            );
        }
    }

    if updated_count > 0 {
        info!(
            "Updated {} managed instances in cluster {}/{} to match current spec",
            updated_count, namespace, cluster_name
        );
    }

    Ok(())
}

/// Ensure child resources exist for all managed instances
///
/// This function verifies that all Kubernetes resources (`ConfigMap`, `Secret`, `Service`, `Deployment`)
/// exist for each managed instance. If any resource is missing, it triggers reconciliation
/// by updating the instance's annotations to force the `Bind9Instance` controller to recreate them.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The parent `Bind9Cluster`
/// * `managed_instances` - List of managed `Bind9Instance` resources
///
/// # Errors
///
/// Returns an error if resource checking or instance update fails
async fn ensure_managed_instance_resources(
    client: &Client,
    cluster: &Bind9Cluster,
    managed_instances: &[Bind9Instance],
) -> Result<()> {
    let namespace = cluster.namespace().unwrap_or_default();
    let cluster_name = cluster.name_any();

    if managed_instances.is_empty() {
        return Ok(());
    }

    debug!(
        "Ensuring child resources exist for {} managed instances in cluster {}/{}",
        managed_instances.len(),
        namespace,
        cluster_name
    );

    let configmap_api: Api<ConfigMap> = Api::namespaced(client.clone(), &namespace);
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), &namespace);
    let service_api: Api<Service> = Api::namespaced(client.clone(), &namespace);
    let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
    let instance_api: Api<Bind9Instance> = Api::namespaced(client.clone(), &namespace);

    for instance in managed_instances {
        let instance_name = instance.name_any();
        let mut missing_resources = Vec::new();

        // Check ConfigMap
        let configmap_name = format!("{instance_name}-config");
        if configmap_api.get(&configmap_name).await.is_err() {
            missing_resources.push("ConfigMap");
        }

        // Check RNDC Secret
        let secret_name = format!("{instance_name}-rndc-key");
        if secret_api.get(&secret_name).await.is_err() {
            missing_resources.push("Secret");
        }

        // Check Service
        if service_api.get(&instance_name).await.is_err() {
            missing_resources.push("Service");
        }

        // Check Deployment
        if deployment_api.get(&instance_name).await.is_err() {
            missing_resources.push("Deployment");
        }

        // If any resources are missing, trigger instance reconciliation
        if missing_resources.is_empty() {
            debug!(
                "All child resources exist for managed instance {}/{}",
                namespace, instance_name
            );
        } else {
            warn!(
                "Missing resources for managed instance {}/{}: {}. Triggering reconciliation.",
                namespace,
                instance_name,
                missing_resources.join(", ")
            );

            // Force reconciliation by updating an annotation
            let patch = json!({
                "metadata": {
                    "annotations": {
                        BINDY_RECONCILE_TRIGGER_ANNOTATION: Utc::now().to_rfc3339()
                    }
                }
            });

            instance_api
                .patch(
                    &instance_name,
                    &PatchParams::apply("bindy-cluster-controller"),
                    &Patch::Merge(&patch),
                )
                .await?;

            info!(
                "Triggered reconciliation for instance {}/{} to recreate: {}",
                namespace,
                instance_name,
                missing_resources.join(", ")
            );
        }
    }

    Ok(())
}

/// Create a managed `Bind9Instance` resource
///
/// This function is public to allow reuse by `ClusterBind9Provider` reconciler.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace for the instance
/// * `cluster_name` - Name of the cluster (namespace-scoped or global)
/// * `role` - Role of the instance (Primary or Secondary)
/// * `index` - Index of this instance within its role
/// * `common_spec` - The cluster's common specification
/// * `is_global` - Whether this is for a global cluster
///
/// # Errors
///
/// Returns an error if instance creation fails
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn create_managed_instance(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    role: ServerRole,
    index: usize,
    common_spec: &crate::crd::Bind9ClusterCommonSpec,
    _is_global: bool,
) -> Result<()> {
    create_managed_instance_with_owner(
        client,
        namespace,
        cluster_name,
        role,
        index,
        common_spec,
        None, // No owner reference - for backward compatibility
    )
    .await
}

/// Create a managed `Bind9Instance` with optional ownerReference.
///
/// This is the internal implementation that supports setting ownerReferences.
/// Use `create_managed_instance()` for backward compatibility without ownerReferences.
///
/// # Arguments
///
/// * `owner_ref` - Optional ownerReference to the parent `Bind9Cluster`
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn create_managed_instance_with_owner(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    role: ServerRole,
    index: usize,
    common_spec: &crate::crd::Bind9ClusterCommonSpec,
    owner_ref: Option<k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference>,
) -> Result<()> {
    let role_str = match role {
        ServerRole::Primary => ROLE_PRIMARY,
        ServerRole::Secondary => ROLE_SECONDARY,
    };

    let instance_name = format!("{cluster_name}-{role_str}-{index}");

    info!(
        "Creating managed instance {}/{} for cluster {} (role: {:?}, index: {})",
        namespace, instance_name, cluster_name, role, index
    );

    // Create labels
    let mut labels = BTreeMap::new();
    labels.insert(
        BINDY_MANAGED_BY_LABEL.to_string(),
        MANAGED_BY_BIND9_CLUSTER.to_string(),
    );
    labels.insert(BINDY_CLUSTER_LABEL.to_string(), cluster_name.to_string());
    labels.insert(BINDY_ROLE_LABEL.to_string(), role_str.to_string());
    labels.insert(K8S_PART_OF.to_string(), PART_OF_BINDY.to_string());

    // Propagate custom labels from cluster spec based on role
    match role {
        ServerRole::Primary => {
            if let Some(primary_config) = &common_spec.primary {
                if let Some(custom_labels) = &primary_config.labels {
                    for (key, value) in custom_labels {
                        labels.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        ServerRole::Secondary => {
            if let Some(secondary_config) = &common_spec.secondary {
                if let Some(custom_labels) = &secondary_config.labels {
                    for (key, value) in custom_labels {
                        labels.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }

    // Create annotations
    let mut annotations = BTreeMap::new();
    annotations.insert(
        BINDY_INSTANCE_INDEX_ANNOTATION.to_string(),
        index.to_string(),
    );

    // Build instance spec - copy configuration from cluster
    let instance_spec = Bind9InstanceSpec {
        cluster_ref: cluster_name.to_string(),
        role,
        replicas: Some(1), // Each managed instance has 1 replica
        version: common_spec.version.clone(),
        image: common_spec.image.clone(),
        config_map_refs: common_spec.config_map_refs.clone(),
        config: None,          // Inherit from cluster
        primary_servers: None, // TODO: Could populate for secondaries
        volumes: common_spec.volumes.clone(),
        volume_mounts: common_spec.volume_mounts.clone(),
        rndc_secret_ref: None, // Inherit from cluster/role config
        storage: None,         // Use default (emptyDir)
        bindcar_config: common_spec
            .global
            .as_ref()
            .and_then(|g| g.bindcar_config.clone()),
    };

    let instance = Bind9Instance {
        metadata: ObjectMeta {
            name: Some(instance_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            annotations: Some(annotations),
            owner_references: owner_ref.map(|r| vec![r]),
            ..Default::default()
        },
        spec: instance_spec,
        status: None,
    };

    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    match api.create(&PostParams::default(), &instance).await {
        Ok(_) => {
            info!(
                "Successfully created managed instance {}/{}",
                namespace, instance_name
            );
            Ok(())
        }
        Err(e) => {
            // If already exists, patch it to ensure spec is up to date
            if e.to_string().contains("AlreadyExists") {
                debug!(
                    "Managed instance {}/{} already exists, patching with updated spec",
                    namespace, instance_name
                );

                // Build a complete patch object for server-side apply
                // Convert BTreeMap labels to serde_json::Value for patch
                let labels_json: serde_json::Map<String, serde_json::Value> = labels
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();

                let patch = serde_json::json!({
                    "apiVersion": API_GROUP_VERSION,
                    "kind": KIND_BIND9_INSTANCE,
                    "metadata": {
                        "name": instance_name,
                        "namespace": namespace,
                        "labels": labels_json,
                        "annotations": {
                            BINDY_INSTANCE_INDEX_ANNOTATION: index.to_string(),
                        },
                        "ownerReferences": instance.metadata.owner_references,
                    },
                    "spec": instance.spec,
                });

                // Apply the patch to update the spec, labels, annotations, and owner references
                match api
                    .patch(
                        &instance_name,
                        &PatchParams::apply("bindy-controller").force(),
                        &Patch::Apply(&patch),
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            "Successfully patched managed instance {}/{} with updated spec",
                            namespace, instance_name
                        );
                        Ok(())
                    }
                    Err(patch_err) => {
                        error!(
                            "Failed to patch managed instance {}/{}: {}",
                            namespace, instance_name, patch_err
                        );
                        Err(patch_err.into())
                    }
                }
            } else {
                error!(
                    "Failed to create managed instance {}/{}: {}",
                    namespace, instance_name, e
                );
                Err(e.into())
            }
        }
    }
}

/// Create or update the shared cluster-level `ConfigMap`
///
/// This `ConfigMap` contains BIND9 configuration that is shared across all instances
/// in the cluster. It is created from `spec.global` configuration.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `Bind9Cluster` resource
///
/// # Errors
///
/// Returns an error if:
/// - Failed to create or update the `ConfigMap`
/// - Kubernetes API operations fail
async fn create_or_update_cluster_configmap(client: &Client, cluster: &Bind9Cluster) -> Result<()> {
    use crate::bind9_resources::build_cluster_configmap;

    let namespace = cluster.namespace().unwrap_or_default();
    let name = cluster.name_any();

    // Check if custom ConfigMaps are referenced at the cluster level
    if let Some(refs) = &cluster.spec.common.config_map_refs {
        if refs.named_conf.is_some() || refs.named_conf_options.is_some() {
            info!(
                "Cluster {}/{} uses custom ConfigMaps, skipping cluster ConfigMap creation",
                namespace, name
            );
            return Ok(());
        }
    }

    info!(
        "Creating/updating shared ConfigMap for cluster {}/{}",
        namespace, name
    );

    // Build the cluster ConfigMap
    let configmap = build_cluster_configmap(&name, &namespace, cluster)?;

    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), &namespace);
    let cm_name = format!("{name}-config");

    if (cm_api.get(&cm_name).await).is_ok() {
        // ConfigMap exists, update it
        info!("Updating cluster ConfigMap {}/{}", namespace, cm_name);
        cm_api
            .replace(&cm_name, &PostParams::default(), &configmap)
            .await?;
    } else {
        // ConfigMap doesn't exist, create it
        info!("Creating cluster ConfigMap {}/{}", namespace, cm_name);
        cm_api.create(&PostParams::default(), &configmap).await?;
    }

    Ok(())
}

/// Delete a single managed `Bind9Instance` resource
///
/// This function is public to allow reuse by `ClusterBind9Provider` reconciler.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the instance
/// * `instance_name` - Name of the instance to delete
///
/// # Errors
///
/// Returns an error if deletion fails (except for `NotFound` errors, which are treated as success)
pub async fn delete_managed_instance(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<()> {
    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    match api.delete(instance_name, &DeleteParams::default()).await {
        Ok(_) => {
            info!(
                "Successfully deleted managed instance {}/{}",
                namespace, instance_name
            );
            Ok(())
        }
        Err(e) if e.to_string().contains("NotFound") => {
            debug!(
                "Managed instance {}/{} already deleted",
                namespace, instance_name
            );
            Ok(())
        }
        Err(e) => {
            error!(
                "Failed to delete managed instance {}/{}: {}",
                namespace, instance_name, e
            );
            Err(e.into())
        }
    }
}

/// Delete all `Bind9Instance` resources that reference the given cluster
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace containing the instances
/// * `cluster_name` - Name of the cluster being deleted
///
/// # Errors
///
/// Returns an error if:
/// - Failed to list `Bind9Instance` resources
/// - Failed to delete any `Bind9Instance` resource
async fn delete_cluster_instances(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
) -> Result<()> {
    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    info!(
        "Finding all Bind9Instance resources for cluster {}/{}",
        namespace, cluster_name
    );

    // List all instances in the namespace
    let instances = api.list(&ListParams::default()).await?;

    // Filter instances that reference this cluster
    let cluster_instances: Vec<_> = instances
        .items
        .into_iter()
        .filter(|instance| instance.spec.cluster_ref == cluster_name)
        .collect();

    if cluster_instances.is_empty() {
        info!(
            "No Bind9Instance resources found for cluster {}/{}",
            namespace, cluster_name
        );
        return Ok(());
    }

    info!(
        "Found {} Bind9Instance resources for cluster {}/{}, deleting...",
        cluster_instances.len(),
        namespace,
        cluster_name
    );

    // Delete each instance
    for instance in cluster_instances {
        let instance_name = instance.name_any();
        info!(
            "Deleting Bind9Instance {}/{} (clusterRef: {})",
            namespace, instance_name, cluster_name
        );

        match api.delete(&instance_name, &DeleteParams::default()).await {
            Ok(_) => {
                info!(
                    "Successfully deleted Bind9Instance {}/{}",
                    namespace, instance_name
                );
            }
            Err(e) => {
                // If the resource is already deleted, treat it as success
                if e.to_string().contains("NotFound") {
                    warn!(
                        "Bind9Instance {}/{} already deleted",
                        namespace, instance_name
                    );
                } else {
                    error!(
                        "Failed to delete Bind9Instance {}/{}: {}",
                        namespace, instance_name, e
                    );
                    return Err(e.into());
                }
            }
        }
    }

    info!(
        "Successfully deleted all Bind9Instance resources for cluster {}/{}",
        namespace, cluster_name
    );

    Ok(())
}

/// Delete handler for `Bind9Cluster` resources (cleanup logic)
///
/// This function is no longer used as deletion is handled by the finalizer in `reconcile_bind9cluster`.
/// Kept for backward compatibility.
///
/// # Errors
///
/// This function currently never returns an error, but returns `Result` for API consistency.
pub async fn delete_bind9cluster(_client: Client, _cluster: Bind9Cluster) -> Result<()> {
    // Deletion is now handled by the finalizer in reconcile_bind9cluster
    Ok(())
}
