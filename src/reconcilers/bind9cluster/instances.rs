// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Instance lifecycle management for `Bind9Cluster` resources.
//!
//! This module handles creating, updating, and deleting `Bind9Instance`
//! resources that are managed by a `Bind9Cluster`.

#[allow(clippy::wildcard_imports)]
use super::types::*;
use crate::constants::{API_GROUP_VERSION, KIND_BIND9_CLUSTER, KIND_BIND9_INSTANCE};
use crate::reconcilers::pagination::list_all_paginated;

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
pub(super) async fn reconcile_managed_instances(
    ctx: &Context,
    cluster: &Bind9Cluster,
) -> Result<()> {
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
    let instances = list_all_paginated(&api, ListParams::default()).await?;

    // Filter for managed instances of this cluster
    let managed_instances: Vec<_> = instances
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
pub(super) async fn update_existing_managed_instances(
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
pub(super) async fn ensure_managed_instance_resources(
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

    // Managed instances share the cluster ConfigMap, not instance-specific ones
    let cluster_configmap_name = format!("{cluster_name}-config");

    for instance in managed_instances {
        let instance_name = instance.name_any();
        let mut missing_resources = Vec::new();

        // Check ConfigMap - managed instances use the shared cluster ConfigMap
        if configmap_api.get(&cluster_configmap_name).await.is_err() {
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

/// Delete a single managed `Bind9Instance` resource.
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
pub(super) async fn delete_cluster_instances(
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
    let instances = list_all_paginated(&api, ListParams::default()).await?;

    // Filter instances that reference this cluster
    let cluster_instances: Vec<_> = instances
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

#[cfg(test)]
#[path = "instances_tests.rs"]
mod instances_tests;
