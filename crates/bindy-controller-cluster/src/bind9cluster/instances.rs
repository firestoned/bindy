// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Instance lifecycle management for `Bind9Cluster` resources.
//!
//! This module handles creating, updating, and deleting `Bind9Instance`
//! resources that are managed by a `Bind9Cluster`.

#[allow(clippy::wildcard_imports)]
use super::types::*;
use crate::constants::{API_GROUP_VERSION, KIND_BIND9_INSTANCE};
use crate::FIELD_MANAGER;
use bindy_controller_sdk::pagination::list_all_paginated;
use bindy_controller_sdk::resources::{create_or_force_apply, delete_if_present};

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

    // Desired instance count per role, in `ServerRole::ALL` order
    let desired: Vec<(ServerRole, usize)> = ServerRole::ALL
        .iter()
        .map(|&role| (role, desired_replicas(&cluster.spec.common, role)))
        .collect();

    debug!(
        "Desired replicas for cluster {}/{}: {:?}",
        namespace, cluster_name, desired
    );

    if desired.iter().all(|&(_, count)| count == 0) {
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

    // Create ownerReference to the Bind9Cluster
    // Fails fast when the cluster has no UID: an ownerReference with an
    // empty UID is invalid.
    let owner_ref = crate::bind9_resources::controller_owner_reference(cluster)?;

    let scope = ManagedScope {
        client: &client,
        namespace: &namespace,
        cluster_name: &cluster_name,
        common_spec: &cluster.spec.common,
        owner_ref: &owner_ref,
    };
    let mut outcomes: Vec<(ServerRole, ScaleOutcome)> = Vec::with_capacity(desired.len());
    for &(role, desired_count) in &desired {
        let existing: Vec<&Bind9Instance> = managed_instances
            .iter()
            .filter(|i| i.spec.role == role)
            .collect();
        outcomes.push((
            role,
            scale_role(&scope, role, desired_count, &existing).await?,
        ));
    }

    let outcome_of = |role: ServerRole| {
        outcomes
            .iter()
            .find(|(r, _)| *r == role)
            .map_or_else(ScaleOutcome::default, |&(_, o)| o)
    };
    let (primary, secondary) = (
        outcome_of(ServerRole::Primary),
        outcome_of(ServerRole::Secondary),
    );
    if outcomes.iter().any(|(_, o)| o.created > 0 || o.deleted > 0) {
        info!(
            "Scaled cluster {}/{}: created {} primary, {} secondary; deleted {} primary, {} secondary",
            namespace,
            cluster_name,
            primary.created,
            secondary.created,
            primary.deleted,
            secondary.deleted
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

/// The desired number of managed instances for `role`: the role block's
/// `replicas`, or 0 when it is unset. The CRD schema bounds `replicas` to
/// `0..=100`, so a negative value (unreachable through the API) is treated
/// as 0.
pub(super) fn desired_replicas(
    common: &crate::crd::Bind9ClusterCommonSpec,
    role: ServerRole,
) -> usize {
    common
        .role_replicas(role)
        .and_then(|r| usize::try_from(r).ok())
        .unwrap_or(0)
}

/// The name of the managed instance of `cluster_name` at `index` for `role`,
/// e.g. `production-dns-primary-0`.
pub(super) fn managed_instance_name(cluster_name: &str, role: ServerRole, index: usize) -> String {
    format!("{cluster_name}-{}-{index}", role.as_str())
}

/// The instance index a managed instance carries in its
/// `BINDY_INSTANCE_INDEX_ANNOTATION`, or 0 when it is absent or unparseable.
pub(super) fn instance_index(instance: &Bind9Instance) -> usize {
    instance
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get(BINDY_INSTANCE_INDEX_ANNOTATION))
        .and_then(|idx| idx.parse::<usize>().ok())
        .unwrap_or(0)
}

/// The indices in `0..desired` that have no instance in `existing` named
/// [`managed_instance_name`], in ascending order. These are the instances a
/// scale-up creates.
pub(super) fn missing_indices(
    cluster_name: &str,
    role: ServerRole,
    desired: usize,
    existing: &[&Bind9Instance],
) -> Vec<usize> {
    let existing_names: std::collections::HashSet<String> = existing
        .iter()
        .map(|instance| instance.name_any())
        .collect();
    (0..desired)
        .filter(|&index| {
            !existing_names.contains(&managed_instance_name(cluster_name, role, index))
        })
        .collect()
}

/// The names of the instances a scale-down to `desired` deletes, in deletion
/// order: the `existing.len() - desired` instances with the highest
/// [`instance_index`]. Instances with the same index are taken last-listed
/// first.
pub(super) fn scale_down_victims(existing: &[&Bind9Instance], desired: usize) -> Vec<String> {
    let excess = existing.len().saturating_sub(desired);
    // Reversing before the stable sort takes equal indices last-listed first
    let mut by_index_desc: Vec<&Bind9Instance> = existing.iter().rev().copied().collect();
    by_index_desc.sort_by_key(|instance| std::cmp::Reverse(instance_index(instance)));
    by_index_desc
        .into_iter()
        .take(excess)
        .map(ResourceExt::name_any)
        .collect()
}

/// What the cluster's managed instances belong to, shared by every role's
/// scaling step.
struct ManagedScope<'a> {
    /// Kubernetes API client
    client: &'a Client,
    /// The cluster's namespace
    namespace: &'a str,
    /// The cluster's name
    cluster_name: &'a str,
    /// The cluster's common spec (source of truth for instance specs)
    common_spec: &'a crate::crd::Bind9ClusterCommonSpec,
    /// The controller ownerReference to the `Bind9Cluster`
    owner_ref: &'a k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference,
}

/// How many instances [`scale_role`] created and deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ScaleOutcome {
    /// Instances created by the scale-up
    created: usize,
    /// Instances deleted by the scale-down
    deleted: usize,
}

/// Scale the managed instances of one role to `desired`.
///
/// Scale-up creates every [`missing_indices`] instance, lowest index first;
/// scale-down then deletes the [`scale_down_victims`] of `existing` (the
/// instances listed before any were created).
///
/// # Arguments
///
/// * `scope` - The cluster the instances belong to
/// * `role` - The role being scaled
/// * `desired` - The desired number of instances of `role`
/// * `existing` - The cluster's managed instances of `role`
///
/// # Errors
///
/// Returns an error if creating or deleting an instance fails.
async fn scale_role(
    scope: &ManagedScope<'_>,
    role: ServerRole,
    desired: usize,
    existing: &[&Bind9Instance],
) -> Result<ScaleOutcome> {
    let to_create = missing_indices(scope.cluster_name, role, desired, existing);
    for &index in &to_create {
        create_managed_instance_with_owner(
            scope.client,
            scope.namespace,
            scope.cluster_name,
            role,
            index,
            scope.common_spec,
            Some(scope.owner_ref.clone()),
        )
        .await?;
    }

    let to_delete = scale_down_victims(existing, desired);
    for instance_name in &to_delete {
        delete_managed_instance(scope.client, scope.namespace, instance_name).await?;
    }

    Ok(ScaleOutcome {
        created: to_create.len(),
        deleted: to_delete.len(),
    })
}

/// The `bindcarConfig` every managed instance inherits: the cluster's
/// `spec.global.bindcarConfig`.
pub(super) fn managed_bindcar_config(
    common: &crate::crd::Bind9ClusterCommonSpec,
) -> Option<&crate::crd::BindcarConfig> {
    common
        .global
        .as_ref()
        .and_then(|g| g.bindcar_config.as_ref())
}

/// The spec a managed instance should have: the cluster-inherited fields
/// (`version`, `image`, `configMapRefs`, `volumes`, `volumeMounts`,
/// `bindcarConfig`, and `config` cleared) taken from the cluster, every other
/// field kept from `instance_fields`.
///
/// Cluster- and role-level placement is not copied down: it is resolved
/// against the live cluster when the Deployment is built (see
/// `crate::placement::resolve_placement`), so a cluster-level change
/// converges without rewriting every managed instance spec.
///
/// # Arguments
///
/// * `common` - The cluster's common spec
/// * `bindcar_config` - [`managed_bindcar_config`] of `common`
/// * `instance_fields` - The instance-specific fields: the existing spec on
///   update, [`new_managed_instance_fields`] on create
pub(super) fn desired_managed_spec(
    common: &crate::crd::Bind9ClusterCommonSpec,
    bindcar_config: Option<&crate::crd::BindcarConfig>,
    instance_fields: &Bind9InstanceSpec,
) -> Bind9InstanceSpec {
    Bind9InstanceSpec {
        version: common.version.clone(),
        image: common.image.clone(),
        config_map_refs: common.config_map_refs.clone(),
        config: None, // Managed instances inherit from cluster
        volumes: common.volumes.clone(),
        volume_mounts: common.volume_mounts.clone(),
        bindcar_config: bindcar_config.cloned(),
        ..instance_fields.clone()
    }
}

/// The instance-specific fields of a newly created managed instance: one
/// replica, everything else left to inherit from the cluster and role.
pub(super) fn new_managed_instance_fields(
    cluster_name: &str,
    role: ServerRole,
) -> Bind9InstanceSpec {
    #[allow(deprecated)] // Backward compatibility: the deprecated rndc_secret_ref field
    Bind9InstanceSpec {
        cluster_ref: cluster_name.to_string(),
        role,
        replicas: Some(1), // Each managed instance has 1 replica
        version: None,
        image: None,
        config_map_refs: None,
        config: None,          // Inherit from cluster
        primary_servers: None, // TODO: Could populate for secondaries
        volumes: None,
        volume_mounts: None,
        rndc_secret_ref: None, // Inherit from cluster/role config (deprecated)
        rndc_key: None,        // Inherit from cluster/role config
        storage: None,         // Use default (emptyDir)
        placement: None,       // Inherit from role / cluster level at build time
        bindcar_config: None,
    }
}

/// Whether `current` differs from the cluster in a field the cluster
/// reconciler drives updates on: `version`, `image`, `configMapRefs`,
/// `volumes`, `volumeMounts` or `bindcarConfig`.
///
/// `config` is not compared, although [`desired_managed_spec`] clears it: an
/// instance whose only difference is a `config` is left alone until one of
/// these fields drifts.
pub(super) fn managed_fields_differ(
    current: &Bind9InstanceSpec,
    common: &crate::crd::Bind9ClusterCommonSpec,
    bindcar_config: Option<&crate::crd::BindcarConfig>,
) -> bool {
    current.version != common.version
        || current.image != common.image
        || current.config_map_refs != common.config_map_refs
        || current.volumes != common.volumes
        || current.volume_mounts != common.volume_mounts
        || current.bindcar_config.as_ref() != bindcar_config
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

    let desired_bindcar_config = managed_bindcar_config(common_spec);

    for instance in managed_instances {
        let instance_name = instance.name_any();

        if managed_fields_differ(&instance.spec, common_spec, desired_bindcar_config) {
            debug!(
                "Instance {}/{} spec differs from cluster spec, updating",
                namespace, instance_name
            );

            // Preserve instance-specific fields, update cluster-inherited fields
            let updated_spec =
                desired_managed_spec(common_spec, desired_bindcar_config, &instance.spec);

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
                    &PatchParams::apply(FIELD_MANAGER).force(),
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
    let secret_api = bindy_controller_sdk::audit::SecretApi::namespaced(
        client.clone(),
        &namespace,
        bindy_controller_sdk::audit::Actor::Bind9Cluster,
    );
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

        // Check Deployment
        let deployment = deployment_api.get(&instance_name).await.ok();
        if deployment.is_none() {
            missing_resources.push("Deployment");
        }

        // Check RNDC Secret: the one the Deployment mounts, else the
        // default. Existence only, so its metadata: no key is read
        // (ADR-0024 Decision 3).
        let secret_name =
            bindy_bind9::instances::effective_rndc_secret_name(&instance_name, deployment.as_ref());
        if !matches!(secret_api.get_metadata_opt(&secret_name).await, Ok(Some(_))) {
            missing_resources.push("Secret");
        }

        // Check Service
        if service_api.get(&instance_name).await.is_err() {
            missing_resources.push("Service");
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

/// Create a managed `Bind9Instance` with optional ownerReference.
///
/// This is the internal implementation that supports setting ownerReferences.
/// Use `create_managed_instance()` for backward compatibility without ownerReferences.
///
/// # Arguments
///
/// * `owner_ref` - Optional ownerReference to the parent `Bind9Cluster`
async fn create_managed_instance_with_owner(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    role: ServerRole,
    index: usize,
    common_spec: &crate::crd::Bind9ClusterCommonSpec,
    owner_ref: Option<k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference>,
) -> Result<()> {
    let role_str = role.as_str();
    let instance_name = managed_instance_name(cluster_name, role, index);

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

    // Propagate custom labels from the role block of the cluster spec
    if let Some(custom_labels) = common_spec.role_labels(role) {
        labels.extend(custom_labels.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    // Create annotations
    let mut annotations = BTreeMap::new();
    annotations.insert(
        BINDY_INSTANCE_INDEX_ANNOTATION.to_string(),
        index.to_string(),
    );

    // Build instance spec - copy configuration from cluster
    let instance_spec = desired_managed_spec(
        common_spec,
        managed_bindcar_config(common_spec),
        &new_managed_instance_fields(cluster_name, role),
    );

    let instance = Bind9Instance {
        metadata: ObjectMeta {
            name: Some(instance_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            annotations: Some(annotations),
            owner_references: owner_ref.map(|r| vec![r]),
            ..Default::default()
        },
        spec: instance_spec,
        status: None,
    };

    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    // POST, and on AlreadyExists force-apply the spec, labels, annotations and
    // owner references so an existing instance converges on the cluster spec.
    create_or_force_apply(&api, &instance, FIELD_MANAGER)
        .await
        .map(|_| ())
        .inspect_err(|e| {
            error!(
                "Failed to create or apply managed instance {}/{}: {:#}",
                namespace, instance_name, e
            );
        })
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

    if delete_if_present(&api, instance_name).await? {
        info!(
            "Successfully deleted managed instance {}/{}",
            namespace, instance_name
        );
        return Ok(());
    }
    debug!(
        "Managed instance {}/{} already deleted",
        namespace, instance_name
    );
    Ok(())
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

        if delete_if_present(&api, &instance_name).await? {
            info!(
                "Successfully deleted Bind9Instance {}/{}",
                namespace, instance_name
            );
        } else {
            warn!(
                "Bind9Instance {}/{} already deleted",
                namespace, instance_name
            );
        }
    }

    info!(
        "Successfully deleted all Bind9Instance resources for cluster {}/{}",
        namespace, cluster_name
    );

    Ok(())
}

#[cfg(test)]
#[path = "instances_tests.rs"]
mod instances_tests;
