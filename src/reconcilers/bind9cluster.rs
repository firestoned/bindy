// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 cluster reconciliation logic.
//!
//! This module handles the lifecycle of BIND9 cluster resources in Kubernetes.
//! It manages the `Bind9Instance` resources that belong to a cluster and updates
//! the cluster status to reflect the overall health.

use crate::crd::{
    Bind9Cluster, Bind9ClusterStatus, Bind9Instance, Bind9InstanceSpec, Condition, ServerRole,
};
use crate::labels::{
    BINDY_CLUSTER_LABEL, BINDY_INSTANCE_INDEX_ANNOTATION, BINDY_MANAGED_BY_LABEL,
    BINDY_RECONCILE_TRIGGER_ANNOTATION, BINDY_ROLE_LABEL, FINALIZER_BIND9_CLUSTER, K8S_PART_OF,
    MANAGED_BY_BIND9_CLUSTER, PART_OF_BINDY, ROLE_PRIMARY, ROLE_SECONDARY,
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
use tracing::{debug, error, info, warn};

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

    // Handle deletion if cluster is being deleted
    if cluster.metadata.deletion_timestamp.is_some() {
        return handle_cluster_deletion(&client, &cluster, &namespace, &name).await;
    }

    // Ensure finalizer is present
    ensure_finalizer(&client, &cluster, &namespace, &name).await?;

    // Create or update shared cluster ConfigMap
    create_or_update_cluster_configmap(&client, &cluster).await?;

    // Reconcile managed instances (create/update as needed)
    reconcile_managed_instances(&client, &cluster).await?;

    // List and analyze cluster instances
    let instances: Vec<Bind9Instance> =
        list_cluster_instances(&client, &cluster, &namespace, &name).await?;

    // Calculate cluster status from instances
    let (instance_count, ready_instances, instance_names, status, message) =
        calculate_cluster_status(&instances, &namespace, &name);

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

/// Handle deletion of a `Bind9Cluster`
///
/// Deletes all managed `Bind9Instance` resources and removes the finalizer.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `Bind9Cluster` being deleted
/// * `namespace` - Cluster namespace
/// * `name` - Cluster name
///
/// # Errors
///
/// Returns an error if:
/// - Failed to delete managed instances
/// - Failed to remove finalizer
async fn handle_cluster_deletion(
    client: &Client,
    cluster: &Bind9Cluster,
    namespace: &str,
    name: &str,
) -> Result<()> {
    info!("Bind9Cluster {}/{} is being deleted", namespace, name);

    // Check if our finalizer is present
    if cluster
        .metadata
        .finalizers
        .as_ref()
        .is_some_and(|f| f.contains(&FINALIZER_BIND9_CLUSTER.to_string()))
    {
        info!("Running cleanup for Bind9Cluster {}/{}", namespace, name);

        // Delete all Bind9Instance resources with matching clusterRef
        if let Err(e) = delete_cluster_instances(client, namespace, name).await {
            error!(
                "Failed to delete instances for cluster {}/{}: {}",
                namespace, name, e
            );
            return Err(e);
        }

        // Remove our finalizer
        info!(
            "Removing finalizer from Bind9Cluster {}/{}",
            namespace, name
        );
        let mut finalizers = cluster.metadata.finalizers.clone().unwrap_or_default();
        finalizers.retain(|f| f != FINALIZER_BIND9_CLUSTER);

        let api: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
        let patch = json!({
            "metadata": {
                "finalizers": finalizers
            }
        });

        api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))
            .await?;

        info!(
            "Successfully removed finalizer from Bind9Cluster {}/{}",
            namespace, name
        );
    }

    Ok(())
}

/// Ensure finalizer is present on a `Bind9Cluster`
///
/// Adds the cluster finalizer if not already present.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `Bind9Cluster` to add finalizer to
/// * `namespace` - Cluster namespace
/// * `name` - Cluster name
///
/// # Errors
///
/// Returns an error if the finalizer patch fails
async fn ensure_finalizer(
    client: &Client,
    cluster: &Bind9Cluster,
    namespace: &str,
    name: &str,
) -> Result<()> {
    // Add finalizer if not present
    if cluster
        .metadata
        .finalizers
        .as_ref()
        .is_none_or(|f| !f.contains(&FINALIZER_BIND9_CLUSTER.to_string()))
    {
        info!("Adding finalizer to Bind9Cluster {}/{}", namespace, name);

        let mut finalizers = cluster.metadata.finalizers.clone().unwrap_or_default();
        finalizers.push(FINALIZER_BIND9_CLUSTER.to_string());

        let api: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
        let patch = json!({
            "metadata": {
                "finalizers": finalizers
            }
        });

        api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))
            .await?;

        info!(
            "Successfully added finalizer to Bind9Cluster {}/{}",
            namespace, name
        );
    }

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
            update_status(
                client,
                cluster,
                "Ready",
                "False",
                &format!("Failed to list instances: {e}"),
                0,
                0,
                vec![],
            )
            .await?;

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
) -> (i32, i32, Vec<String>, &'static str, String) {
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

    (
        instance_count,
        ready_instances,
        instance_names,
        status,
        message,
    )
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
async fn reconcile_managed_instances(client: &Client, cluster: &Bind9Cluster) -> Result<()> {
    let namespace = cluster.namespace().unwrap_or_default();
    let cluster_name = cluster.name_any();

    info!(
        "Reconciling managed instances for cluster {}/{}",
        namespace, cluster_name
    );

    // Get desired replica counts from spec
    let primary_replicas = cluster
        .spec
        .primary
        .as_ref()
        .and_then(|p| p.replicas)
        .unwrap_or(0);

    let secondary_replicas = cluster
        .spec
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

    // Handle scale-up: Create missing primary instances
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let primaries_to_create = (primary_replicas as usize).saturating_sub(existing_primary.len());
    for i in 0..primaries_to_create {
        let index = existing_primary.len() + i;
        create_managed_instance(
            client,
            cluster,
            &namespace,
            &cluster_name,
            ServerRole::Primary,
            index,
        )
        .await?;
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
            delete_managed_instance(client, &namespace, &instance_name).await?;
        }
    }

    // Handle scale-up: Create missing secondary instances
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let secondaries_to_create =
        (secondary_replicas as usize).saturating_sub(existing_secondary.len());
    for i in 0..secondaries_to_create {
        let index = existing_secondary.len() + i;
        create_managed_instance(
            client,
            cluster,
            &namespace,
            &cluster_name,
            ServerRole::Secondary,
            index,
        )
        .await?;
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
            delete_managed_instance(client, &namespace, &instance_name).await?;
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

    // Ensure child resources (ConfigMaps, Secrets, Services, Deployments) exist for all managed instances
    ensure_managed_instance_resources(client, cluster, &managed_instances).await?;

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
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The parent `Bind9Cluster`
/// * `namespace` - Namespace for the instance
/// * `cluster_name` - Name of the cluster
/// * `role` - Role of the instance (Primary or Secondary)
/// * `index` - Index of this instance within its role
///
/// # Errors
///
/// Returns an error if instance creation fails
#[allow(clippy::too_many_lines)]
async fn create_managed_instance(
    client: &Client,
    cluster: &Bind9Cluster,
    namespace: &str,
    cluster_name: &str,
    role: ServerRole,
    index: usize,
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

    // Create annotations
    let mut annotations = BTreeMap::new();
    annotations.insert(
        BINDY_INSTANCE_INDEX_ANNOTATION.to_string(),
        index.to_string(),
    );

    // Build instance spec
    let instance_spec = Bind9InstanceSpec {
        cluster_ref: cluster_name.to_string(),
        role,
        replicas: Some(1), // Each managed instance has 1 replica
        version: cluster.spec.version.clone(),
        image: cluster.spec.image.clone(),
        config_map_refs: cluster.spec.config_map_refs.clone(),
        config: None,          // Inherit from cluster
        primary_servers: None, // TODO: Could populate for secondaries
        volumes: cluster.spec.volumes.clone(),
        volume_mounts: cluster.spec.volume_mounts.clone(),
        rndc_secret_ref: None, // Inherit from cluster/role config
        storage: None,         // Use default (emptyDir)
        bindcar_config: None,  // Use default Bindcar configuration
    };

    let instance = Bind9Instance {
        metadata: ObjectMeta {
            name: Some(instance_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            annotations: Some(annotations),
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
            // If already exists, update labels/annotations if missing
            if e.to_string().contains("AlreadyExists") {
                warn!(
                    "Managed instance {}/{} already exists, checking labels/annotations",
                    namespace, instance_name
                );

                // Fetch existing instance
                let existing = api.get(&instance_name).await?;

                // Check if labels/annotations need updating
                let needs_label_update = existing.metadata.labels.as_ref().is_none_or(|l| {
                    l.get(BINDY_MANAGED_BY_LABEL) != Some(&MANAGED_BY_BIND9_CLUSTER.to_string())
                        || l.get(BINDY_CLUSTER_LABEL) != Some(&cluster_name.to_string())
                        || l.get(BINDY_ROLE_LABEL) != Some(&role_str.to_string())
                });

                let needs_annotation_update = existing
                    .metadata
                    .annotations
                    .as_ref()
                    .is_none_or(|a| a.get(BINDY_INSTANCE_INDEX_ANNOTATION).is_none());

                if needs_label_update || needs_annotation_update {
                    info!(
                        "Updating labels/annotations for existing instance {}/{}",
                        namespace, instance_name
                    );

                    // Patch the instance with updated labels and annotations
                    let patch = serde_json::json!({
                        "metadata": {
                            "labels": {
                                BINDY_MANAGED_BY_LABEL: MANAGED_BY_BIND9_CLUSTER,
                                BINDY_CLUSTER_LABEL: cluster_name,
                                BINDY_ROLE_LABEL: role_str,
                                K8S_PART_OF: PART_OF_BINDY,
                            },
                            "annotations": {
                                BINDY_INSTANCE_INDEX_ANNOTATION: index.to_string(),
                            }
                        }
                    });

                    api.patch(
                        &instance_name,
                        &PatchParams::default(),
                        &Patch::Merge(&patch),
                    )
                    .await?;

                    info!(
                        "Successfully updated labels/annotations for instance {}/{}",
                        namespace, instance_name
                    );
                }

                Ok(())
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
    if let Some(refs) = &cluster.spec.config_map_refs {
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
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the instance
/// * `instance_name` - Name of the instance to delete
///
/// # Errors
///
/// Returns an error if deletion fails (except for `NotFound` errors, which are treated as success)
async fn delete_managed_instance(
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
