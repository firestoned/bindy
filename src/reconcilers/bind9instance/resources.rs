// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Kubernetes resource lifecycle management for `Bind9Instance` resources.
//!
//! This module handles creating, updating, and deleting all Kubernetes resources
//! needed to run a BIND9 DNS server (`ConfigMap`, Deployment, Service, etc.).

#[allow(clippy::wildcard_imports)]
use super::types::*;

use crate::bind9::Bind9Manager;
use crate::bind9_resources::{
    build_configmap, build_deployment, build_service, build_service_account,
};
use crate::constants::{API_GROUP_VERSION, KIND_BIND9_INSTANCE};
use crate::reconcilers::resources::create_or_apply;
use anyhow::Context as _;

/// Resolve RNDC configuration from instance and cluster levels.
///
/// Applies the precedence order: Instance > Role > Default
///
/// # Arguments
///
/// * `instance` - The `Bind9Instance` being reconciled
/// * `cluster` - Optional `Bind9Cluster` (namespace-scoped)
/// * `cluster_provider` - Optional `ClusterBind9Provider` (cluster-scoped)
///
/// # Returns
///
/// Resolved `RndcKeyConfig` with highest-precedence configuration applied.
pub(super) fn resolve_full_rndc_config(
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    _cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> crate::crd::RndcKeyConfig {
    use super::config::{resolve_rndc_config, resolve_rndc_config_from_deprecated};

    // Extract instance-level config
    let instance_config = instance.spec.rndc_key.as_ref();

    // Extract role-level config (from cluster primary/secondary config)
    // Note: Although serde flattens, the Rust struct still has the common field
    let role_config = cluster.and_then(|c| match instance.spec.role {
        crate::crd::ServerRole::Primary => c
            .spec
            .common
            .primary
            .as_ref()
            .and_then(|p| p.rndc_key.as_ref()),
        crate::crd::ServerRole::Secondary => c
            .spec
            .common
            .secondary
            .as_ref()
            .and_then(|s| s.rndc_key.as_ref()),
    });

    // No global-level RNDC config is supported in the current design
    // RNDC keys are instance-specific or role-specific only

    // Handle backward compatibility with deprecated fields
    #[allow(deprecated)]
    let deprecated_instance_ref = instance.spec.rndc_secret_ref.as_ref();

    // First, resolve from new fields (no global level for RNDC keys)
    let resolved = resolve_rndc_config(instance_config, role_config, None);

    // Then, apply backward compatibility if needed
    if instance_config.is_none() && role_config.is_none() {
        // Only use deprecated fields if no new fields are present
        if deprecated_instance_ref.is_some() {
            return resolve_rndc_config_from_deprecated(
                None,
                deprecated_instance_ref,
                instance.spec.role.clone(),
            );
        }
    }

    resolved
}

#[allow(clippy::too_many_lines)] // Function orchestrates multiple resource creation steps
pub(super) async fn create_or_update_resources(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
) -> Result<(
    Option<Bind9Cluster>,
    Option<crate::crd::ClusterBind9Provider>,
    Option<Secret>, // Added: return Secret for rotation status updates
)> {
    debug!(
        namespace = %namespace,
        name = %name,
        "Creating or updating Kubernetes resources"
    );

    // Fetch the Bind9Cluster (namespace-scoped) if referenced
    let cluster = if instance.spec.cluster_ref.is_empty() {
        debug!("No cluster reference, proceeding with standalone instance");
        None
    } else {
        debug!(cluster_ref = %instance.spec.cluster_ref, "Fetching Bind9Cluster");
        let cluster_api: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
        match cluster_api.get(&instance.spec.cluster_ref).await {
            Ok(cluster) => {
                debug!(
                    cluster_name = %instance.spec.cluster_ref,
                    "Successfully fetched Bind9Cluster"
                );
                info!(
                    "Found Bind9Cluster: {}/{}",
                    namespace, instance.spec.cluster_ref
                );
                Some(cluster)
            }
            Err(e) => {
                warn!(
                    "Failed to fetch Bind9Cluster {}/{}: {}. Proceeding with instance-only config.",
                    namespace, instance.spec.cluster_ref, e
                );
                None
            }
        }
    };

    // Fetch the ClusterBind9Provider (cluster-scoped) if no namespace-scoped cluster was found
    let cluster_provider = if cluster.is_none() && !instance.spec.cluster_ref.is_empty() {
        debug!(cluster_ref = %instance.spec.cluster_ref, "Fetching ClusterBind9Provider");
        let cluster_provider_api: Api<crate::crd::ClusterBind9Provider> = Api::all(client.clone());
        match cluster_provider_api.get(&instance.spec.cluster_ref).await {
            Ok(gc) => {
                debug!(
                    cluster_name = %instance.spec.cluster_ref,
                    "Successfully fetched ClusterBind9Provider"
                );
                info!("Found ClusterBind9Provider: {}", instance.spec.cluster_ref);
                Some(gc)
            }
            Err(e) => {
                warn!(
                    "Failed to fetch ClusterBind9Provider {}: {}. Proceeding with instance-only config.",
                    instance.spec.cluster_ref, e
                );
                None
            }
        }
    } else {
        None
    };

    // Resolve RNDC configuration with proper precedence
    let rndc_config =
        resolve_full_rndc_config(instance, cluster.as_ref(), cluster_provider.as_ref());
    debug!(
        "Resolved RNDC config: auto_rotate={}, rotate_after={}",
        rndc_config.auto_rotate, rndc_config.rotate_after
    );

    // 1. Create/update ServiceAccount (must be first, as deployment will reference it)
    debug!("Step 1: Creating/updating ServiceAccount");
    create_or_update_service_account(client, namespace, instance).await?;

    // 2. Create/update RNDC Secret with rotation support (must be before deployment, as it will be mounted)
    debug!("Step 2: Creating/updating RNDC Secret with rotation support");
    let secret_name =
        create_or_update_rndc_secret_with_config(client, namespace, name, instance, &rndc_config)
            .await?;

    // Fetch the Secret for rotation status updates (only if rotation is enabled)
    let secret = if rndc_config.auto_rotate {
        let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
        secret_api.get(&secret_name).await.ok()
    } else {
        None
    };

    // 3. Create/update ConfigMap
    debug!("Step 3: Creating/updating ConfigMap");
    create_or_update_configmap(
        client,
        namespace,
        name,
        instance,
        cluster.as_ref(),
        cluster_provider.as_ref(),
    )
    .await?;

    // 4. Create/update Deployment
    debug!("Step 4: Creating/updating Deployment");
    create_or_update_deployment(
        client,
        namespace,
        name,
        instance,
        cluster.as_ref(),
        cluster_provider.as_ref(),
    )
    .await?;

    // 5. Create/update Service
    debug!("Step 5: Creating/updating Service");
    create_or_update_service(
        client,
        namespace,
        name,
        instance,
        cluster.as_ref(),
        cluster_provider.as_ref(),
    )
    .await?;

    debug!("Successfully created/updated all resources");
    Ok((cluster, cluster_provider, secret))
}

/// Create or update the `ServiceAccount` for BIND9 pods
async fn create_or_update_service_account(
    client: &Client,
    namespace: &str,
    instance: &Bind9Instance,
) -> Result<()> {
    let service_account = build_service_account(namespace, instance);
    create_or_apply(client, namespace, &service_account, "bindy-controller").await
}

/// Create or update the RNDC Secret for BIND9 remote control
/// Creates or updates RNDC `Secret` based on configuration.
///
/// Supports three modes:
/// 1. **Auto-generated**: Operator creates and optionally rotates RNDC keys
/// 2. **Secret reference**: Use existing `Secret` (no operator management)
/// 3. **Inline spec**: Create `Secret` from inline specification
///
/// # Arguments
///
/// * `client` - Kubernetes client
/// * `namespace` - Namespace for the `Secret`
/// * `name` - Instance name (used for `Secret` naming)
/// * `instance` - `Bind9Instance` resource
/// * `config` - RNDC configuration (resolved via precedence)
///
/// # Returns
///
/// Returns the `Secret` name to use in `Deployment` configuration.
///
/// # Errors
///
/// Returns error if `Secret` creation/update fails or API call fails.
#[allow(dead_code)] // Will be used when integrated into reconciler
#[allow(clippy::too_many_lines)] // Function implements three Secret modes
async fn create_or_update_rndc_secret_with_config(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    config: &crate::crd::RndcKeyConfig,
) -> Result<String> {
    use chrono::Utc;

    // Mode 1: Use existing Secret via secret_ref
    if let Some(ref secret_ref) = config.secret_ref {
        info!(
            "Using existing Secret reference: {}/{}",
            namespace, secret_ref.name
        );
        return Ok(secret_ref.name.clone());
    }

    // Mode 2 & 3: Create/manage Secret (inline spec or auto-generated)
    let secret_name = if let Some(ref secret_spec) = config.secret {
        // Use name from inline spec
        secret_spec.metadata.name.clone()
    } else {
        // Default name for auto-generated
        format!("{name}-rndc-key")
    };

    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);

    // Check if Secret exists and if rotation is due
    match secret_api.get(&secret_name).await {
        Ok(existing_secret) => {
            // Verify Secret has required keys first
            if let Some(ref data) = existing_secret.data {
                if !data.contains_key("key-name")
                    || !data.contains_key("algorithm")
                    || !data.contains_key("secret")
                {
                    warn!(
                        "RNDC Secret {}/{} is missing required keys, will recreate",
                        namespace, secret_name
                    );
                    secret_api
                        .delete(&secret_name, &kube::api::DeleteParams::default())
                        .await?;
                    // Fall through to create new Secret
                }
            } else {
                warn!(
                    "RNDC Secret {}/{} has no data, will recreate",
                    namespace, secret_name
                );
                secret_api
                    .delete(&secret_name, &kube::api::DeleteParams::default())
                    .await?;
                // Fall through to create new Secret
            }

            // Check if rotation annotations need to be added or updated
            let has_annotations = existing_secret
                .metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get(crate::constants::ANNOTATION_RNDC_CREATED_AT))
                .is_some();

            if config.auto_rotate && !has_annotations {
                info!(
                    "RNDC Secret {}/{} missing rotation annotations, adding them",
                    namespace, secret_name
                );
                add_rotation_annotations_to_secret(&secret_api, &secret_name, config).await?;
                return Ok(secret_name);
            }

            // Check if rotation is needed
            if config.auto_rotate && should_rotate_secret(&existing_secret, config)? {
                info!(
                    "RNDC Secret {}/{} rotation is due, rotating",
                    namespace, secret_name
                );
                rotate_rndc_secret(
                    client,
                    namespace,
                    &secret_name,
                    config,
                    instance,
                    &existing_secret,
                )
                .await?;
                return Ok(secret_name);
            }

            // Check for configuration drift (algorithm changed)
            if let Some(ref data) = existing_secret.data {
                if data.contains_key("algorithm") {
                    let current_algorithm =
                        std::str::from_utf8(&data.get("algorithm").unwrap().0).unwrap_or("unknown");
                    let desired_algorithm = config.algorithm.as_str();

                    if current_algorithm == desired_algorithm {
                        info!(
                            "RNDC Secret {}/{} exists and is valid, skipping creation",
                            namespace, secret_name
                        );
                        return Ok(secret_name);
                    }
                    warn!(
                        "RNDC Secret {}/{} algorithm mismatch (current: {}, desired: {}), will recreate",
                        namespace, secret_name, current_algorithm, desired_algorithm
                    );
                    secret_api
                        .delete(&secret_name, &kube::api::DeleteParams::default())
                        .await?;
                    // Fall through to create new Secret
                }
            }
        }
        Err(_) => {
            info!(
                "RNDC Secret {}/{} does not exist, creating",
                namespace, secret_name
            );
        }
    }

    // Mode 2: Create from inline spec
    if let Some(_secret_spec) = &config.secret {
        // TODO: Implement inline Secret creation from SecretSpec
        // For now, fall through to auto-generated
        info!("Creating RNDC Secret from inline spec with rotation enabled");
    }

    // Mode 3: Auto-generate Secret
    let mut key_data = Bind9Manager::generate_rndc_key();
    key_data.name = "bindy-operator".to_string();
    key_data.algorithm = config.algorithm.clone();

    // Calculate rotation timestamps if enabled
    let created_at = Utc::now();
    let rotate_after = if config.auto_rotate {
        crate::bind9::duration::parse_duration(&config.rotate_after).ok()
    } else {
        None
    };

    // Create Secret with annotations using helper function
    let secret = crate::bind9::rndc::create_rndc_secret_with_annotations(
        namespace,
        &secret_name,
        &key_data,
        created_at,
        rotate_after,
        0, // Initial rotation count
    );

    // Add owner reference
    let owner_ref = OwnerReference {
        api_version: API_GROUP_VERSION.to_string(),
        kind: KIND_BIND9_INSTANCE.to_string(),
        name: name.to_string(),
        uid: instance.metadata.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    };

    let mut secret_with_owner = secret;
    secret_with_owner
        .metadata
        .owner_references
        .get_or_insert_with(Vec::new)
        .push(owner_ref);

    // Create the Secret
    if config.auto_rotate {
        info!(
            "Creating RNDC Secret {}/{} with rotation enabled (rotate after: {})",
            namespace, secret_name, config.rotate_after
        );
    } else {
        info!(
            "Creating RNDC Secret {}/{} without rotation",
            namespace, secret_name
        );
    }

    secret_api
        .create(&PostParams::default(), &secret_with_owner)
        .await?;

    Ok(secret_name)
}

/// Legacy Secret creation function (backward compatibility).
///
/// This function maintains the original behavior for existing reconciler code.
/// New code should use `create_or_update_rndc_secret_with_config` instead.
#[allow(dead_code)] // Kept for backward compatibility, may be removed in future
async fn create_or_update_rndc_secret(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
) -> Result<()> {
    let secret_name = format!("{name}-rndc-key");
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);

    // Check if secret already exists
    match secret_api.get(&secret_name).await {
        Ok(existing_secret) => {
            // Secret exists, don't regenerate the key
            info!(
                "RNDC Secret {}/{} already exists, skipping",
                namespace, secret_name
            );
            // Verify it has the required keys
            if let Some(ref data) = existing_secret.data {
                if !data.contains_key("key-name")
                    || !data.contains_key("algorithm")
                    || !data.contains_key("secret")
                {
                    warn!(
                        "RNDC Secret {}/{} is missing required keys, will recreate",
                        namespace, secret_name
                    );
                    // Delete and recreate
                    secret_api
                        .delete(&secret_name, &kube::api::DeleteParams::default())
                        .await?;
                } else {
                    return Ok(());
                }
            } else {
                warn!(
                    "RNDC Secret {}/{} has no data, will recreate",
                    namespace, secret_name
                );
                secret_api
                    .delete(&secret_name, &kube::api::DeleteParams::default())
                    .await?;
            }
        }
        Err(_) => {
            info!(
                "RNDC Secret {}/{} does not exist, creating",
                namespace, secret_name
            );
        }
    }

    // Generate new RNDC key
    let mut key_data = Bind9Manager::generate_rndc_key();
    key_data.name = "bindy-operator".to_string();

    // Create Secret data
    let secret_data = Bind9Manager::create_rndc_secret_data(&key_data);

    // Create owner reference to the Bind9Instance
    let owner_ref = OwnerReference {
        api_version: API_GROUP_VERSION.to_string(),
        kind: KIND_BIND9_INSTANCE.to_string(),
        name: name.to_string(),
        uid: instance.metadata.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    };

    // Build Secret object
    let secret = Secret {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(secret_name.clone()),
            namespace: Some(namespace.to_string()),
            owner_references: Some(vec![owner_ref]),
            ..Default::default()
        },
        string_data: Some(secret_data),
        ..Default::default()
    };

    // Create the secret
    info!("Creating RNDC Secret {}/{}", namespace, secret_name);
    secret_api.create(&PostParams::default(), &secret).await?;

    Ok(())
}

/// Adds rotation annotations to an existing RNDC `Secret` without regenerating the key.
///
/// This is used when rotation is enabled for a Secret that was created without rotation.
///
/// # Arguments
///
/// * `secret_api` - Kubernetes API client for Secrets
/// * `secret_name` - Name of the Secret to update
/// * `config` - RNDC configuration with rotation settings
///
/// # Errors
///
/// Returns error if Secret patch fails or duration parsing fails.
async fn add_rotation_annotations_to_secret(
    secret_api: &Api<Secret>,
    secret_name: &str,
    config: &crate::crd::RndcKeyConfig,
) -> Result<()> {
    use chrono::Utc;
    use kube::api::{Patch, PatchParams};
    use std::collections::BTreeMap;

    let created_at = Utc::now();
    let rotate_after = crate::bind9::duration::parse_duration(&config.rotate_after)?;
    let rotate_at = created_at + chrono::Duration::from_std(rotate_after)?;

    let mut annotations = BTreeMap::new();
    annotations.insert(
        crate::constants::ANNOTATION_RNDC_CREATED_AT.to_string(),
        created_at.to_rfc3339(),
    );
    annotations.insert(
        crate::constants::ANNOTATION_RNDC_ROTATE_AT.to_string(),
        rotate_at.to_rfc3339(),
    );
    annotations.insert(
        crate::constants::ANNOTATION_RNDC_ROTATION_COUNT.to_string(),
        "0".to_string(),
    );

    let patch = serde_json::json!({
        "metadata": {
            "annotations": annotations
        }
    });

    info!(
        "Adding rotation annotations to existing Secret {} (rotate at: {})",
        secret_name,
        rotate_at.to_rfc3339()
    );

    secret_api
        .patch(
            secret_name,
            &PatchParams::apply("bindy-operator"),
            &Patch::Merge(&patch),
        )
        .await?;

    Ok(())
}

/// Checks if RNDC `Secret` rotation is due.
///
/// # Arguments
///
/// * `secret` - The RNDC `Secret` to check
/// * `config` - RNDC configuration with rotation settings
///
/// # Returns
///
/// Returns `true` if rotation is due, `false` otherwise.
///
/// # Rotation Criteria
///
/// - Auto-rotation must be enabled in config
/// - `rotate_at` annotation must be in the past
/// - At least 1 hour must have passed since last rotation (rate limit)
///
/// # Errors
///
/// Returns error if annotation parsing fails.
pub(super) fn should_rotate_secret(
    secret: &Secret,
    config: &crate::crd::RndcKeyConfig,
) -> Result<bool> {
    use chrono::Utc;

    // Auto-rotation must be enabled
    if !config.auto_rotate {
        return Ok(false);
    }

    // Parse rotation annotations
    let Some(annotations) = &secret.metadata.annotations else {
        debug!("Secret has no annotations, rotation not due");
        return Ok(false);
    };

    let (created_at, rotate_at, _rotation_count) =
        crate::bind9::rndc::parse_rotation_annotations(annotations)?;

    let now = Utc::now();

    // Rate limit: Ensure at least 1 hour has passed since creation/last rotation
    let time_since_creation = now.signed_duration_since(created_at);
    if time_since_creation.num_hours() < crate::constants::MIN_TIME_BETWEEN_ROTATIONS_HOURS {
        debug!(
            "Skipping rotation - Secret was created/rotated {} minutes ago (min 1 hour required)",
            time_since_creation.num_minutes()
        );
        return Ok(false);
    }

    // Check if rotation is due based on rotate_at annotation
    Ok(crate::bind9::rndc::is_rotation_due(rotate_at, now))
}

/// Rotates RNDC `Secret` by generating new key and updating annotations.
///
/// # Arguments
///
/// * `client` - Kubernetes client
/// * `namespace` - `Secret` namespace
/// * `secret_name` - Name of the `Secret` to rotate
/// * `config` - RNDC configuration with rotation settings
/// * `instance` - `Bind9Instance` for owner reference
/// * `existing_secret` - Current `Secret` (for incrementing rotation count)
///
/// # Rotation Process
///
/// 1. Generate new RNDC key
/// 2. Increment rotation count from existing `Secret`
/// 3. Update `Secret` with new key data
/// 4. Update annotations: `created_at`, `rotate_at`, `rotation_count`
/// 5. Trigger `Deployment` rollout via annotation
///
/// # Errors
///
/// Returns error if `Secret` update fails or annotation parsing fails.
#[allow(dead_code)] // Will be used when integrated into reconciler
async fn rotate_rndc_secret(
    client: &Client,
    namespace: &str,
    secret_name: &str,
    config: &crate::crd::RndcKeyConfig,
    instance: &Bind9Instance,
    existing_secret: &Secret,
) -> Result<()> {
    use chrono::Utc;

    // Parse existing rotation annotations
    let annotations = existing_secret
        .metadata
        .annotations
        .as_ref()
        .context("Secret missing annotations")?;

    let (_created_at, _rotate_at, rotation_count) =
        crate::bind9::rndc::parse_rotation_annotations(annotations)?;

    // Increment rotation count
    let new_rotation_count = rotation_count + 1;

    info!(
        "Rotating RNDC Secret {}/{} (rotation #{})",
        namespace, secret_name, new_rotation_count
    );

    // Generate new RNDC key
    let mut key_data = Bind9Manager::generate_rndc_key();
    key_data.name = "bindy-operator".to_string();
    key_data.algorithm = config.algorithm.clone();

    // Calculate new rotation timestamps
    let created_at = Utc::now();
    let rotate_after = crate::bind9::duration::parse_duration(&config.rotate_after)?;

    // Create new Secret with updated annotations and data
    let new_secret = crate::bind9::rndc::create_rndc_secret_with_annotations(
        namespace,
        secret_name,
        &key_data,
        created_at,
        Some(rotate_after),
        new_rotation_count,
    );

    // Preserve owner references from existing Secret
    let mut updated_secret = new_secret;
    updated_secret
        .metadata
        .owner_references
        .clone_from(&existing_secret.metadata.owner_references);

    // Replace the Secret
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    secret_api
        .replace(secret_name, &PostParams::default(), &updated_secret)
        .await?;

    info!(
        "Successfully rotated RNDC Secret {}/{} (rotation #{})",
        namespace, secret_name, new_rotation_count
    );

    // Trigger Deployment rollout by patching pod template annotation
    trigger_deployment_rollout(client, namespace, &instance.name_any()).await?;

    Ok(())
}

/// Triggers a `Deployment` rollout by updating pod template annotation.
///
/// # Arguments
///
/// * `client` - Kubernetes client
/// * `namespace` - Deployment namespace
/// * `instance_name` - Name of the `Bind9Instance` (= Deployment name)
///
/// # Errors
///
/// Returns error if Deployment patch fails.
async fn trigger_deployment_rollout(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<()> {
    use chrono::Utc;
    use serde_json::json;

    let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    // Patch deployment pod template annotation to trigger rolling restart
    let patch = json!({
        "spec": {
            "template": {
                "metadata": {
                    "annotations": {
                        crate::constants::ANNOTATION_RNDC_ROTATED_AT: Utc::now().to_rfc3339()
                    }
                }
            }
        }
    });

    deployment_api
        .patch(
            instance_name,
            &PatchParams::default(),
            &kube::api::Patch::Merge(&patch),
        )
        .await?;

    info!(
        "Triggered Deployment {}/{} rollout after RNDC rotation",
        namespace, instance_name
    );

    Ok(())
}

/// Create or update the `ConfigMap` for BIND9 configuration
///
/// **Note:** If the instance belongs to a cluster (has `spec.clusterRef`), this function
/// does NOT create an instance-specific `ConfigMap`. Instead, the instance will use the
/// cluster-level shared `ConfigMap` created by the `Bind9Cluster` reconciler.
async fn create_or_update_configmap(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    _cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Result<()> {
    // If instance belongs to a cluster, skip ConfigMap creation
    // The cluster creates a shared ConfigMap that all instances use
    if !instance.spec.cluster_ref.is_empty() {
        debug!(
            "Instance {}/{} belongs to cluster '{}', using cluster ConfigMap",
            namespace, name, instance.spec.cluster_ref
        );
        return Ok(());
    }

    // Instance is standalone (no clusterRef), create instance-specific ConfigMap
    info!(
        "Instance {}/{} is standalone, creating instance-specific ConfigMap",
        namespace, name
    );

    // Get role-specific allow-transfer override from cluster config
    // Note: We only reach this code for standalone instances (no clusterRef),
    // so we should only have a namespace-scoped cluster here, not a global cluster
    let role_allow_transfer = cluster.and_then(|c| match instance.spec.role {
        crate::crd::ServerRole::Primary => c
            .spec
            .common
            .primary
            .as_ref()
            .and_then(|p| p.allow_transfer.as_ref()),
        crate::crd::ServerRole::Secondary => c
            .spec
            .common
            .secondary
            .as_ref()
            .and_then(|s| s.allow_transfer.as_ref()),
    });

    // build_configmap returns None if custom ConfigMaps are referenced
    if let Some(configmap) =
        build_configmap(name, namespace, instance, cluster, role_allow_transfer)
    {
        let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
        let cm_name = format!("{name}-config");

        if (cm_api.get(&cm_name).await).is_ok() {
            // ConfigMap exists, update it
            info!("Updating ConfigMap {}/{}", namespace, cm_name);
            cm_api
                .replace(&cm_name, &PostParams::default(), &configmap)
                .await?;
        } else {
            // ConfigMap doesn't exist, create it
            info!("Creating ConfigMap {}/{}", namespace, cm_name);
            cm_api.create(&PostParams::default(), &configmap).await?;
        }
    } else {
        info!(
            "Using custom ConfigMaps for {}/{}, skipping ConfigMap creation",
            namespace, name
        );
    }

    Ok(())
}

/// Check if a deployment needs updating by comparing current and desired state.
///
/// Returns true if any of the following have changed:
/// - Replicas count
/// - API container image
/// - API container environment variables
/// - API container imagePullPolicy
/// - API container resources
fn deployment_needs_update(current: &Deployment, desired: &Deployment) -> bool {
    // Compare desired replicas with current replicas
    let desired_replicas = desired.spec.as_ref().and_then(|s| s.replicas);
    let current_replicas = current.spec.as_ref().and_then(|s| s.replicas);

    if desired_replicas != current_replicas {
        debug!(
            "Replicas changed: current={:?}, desired={:?}",
            current_replicas, desired_replicas
        );
        return true;
    }

    // Get the current api container
    let current_api_container = current
        .spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|pod_spec| {
            pod_spec
                .containers
                .iter()
                .find(|c| c.name == crate::constants::CONTAINER_NAME_BINDCAR)
        });

    // Get the desired api container
    let desired_api_container = desired
        .spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|pod_spec| {
            pod_spec
                .containers
                .iter()
                .find(|c| c.name == crate::constants::CONTAINER_NAME_BINDCAR)
        });

    // Check api container fields if both exist
    if let (Some(current_api), Some(desired_api)) = (current_api_container, desired_api_container) {
        // Check image
        if current_api.image != desired_api.image {
            debug!(
                "API container image changed: current={:?}, desired={:?}",
                current_api.image, desired_api.image
            );
            return true;
        }

        // Check env variables
        if current_api.env != desired_api.env {
            debug!("API container env changed");
            return true;
        }

        // Check imagePullPolicy
        if current_api.image_pull_policy != desired_api.image_pull_policy {
            debug!(
                "API container imagePullPolicy changed: current={:?}, desired={:?}",
                current_api.image_pull_policy, desired_api.image_pull_policy
            );
            return true;
        }

        // Check resources
        if current_api.resources != desired_api.resources {
            debug!("API container resources changed");
            return true;
        }
    } else if current_api_container.is_some() != desired_api_container.is_some() {
        // One exists but not the other - needs update
        debug!("API container existence changed");
        return true;
    }

    false
}

/// Create or update the Deployment for BIND9
async fn create_or_update_deployment(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Result<()> {
    let deployment = build_deployment(name, namespace, instance, cluster, cluster_provider);
    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    // Check if deployment exists - if not, create it and return early
    if api.get(name).await.is_err() {
        info!("Creating Deployment {}/{}", namespace, name);
        api.create(&PostParams::default(), &deployment).await?;
        return Ok(());
    }

    // Deployment exists - check if it needs updating before patching
    debug!(
        "Checking if Deployment {}/{} needs updating",
        namespace, name
    );

    // Get the current deployment from the cluster
    let current_deployment = api.get(name).await?;

    // Compare current and desired state using helper function
    if !deployment_needs_update(&current_deployment, &deployment) {
        debug!(
            "Deployment {}/{} is up to date, skipping patch",
            namespace, name
        );
        return Ok(());
    }

    // Deployment needs updating - use strategic merge patch
    info!("Patching Deployment {}/{}", namespace, name);

    let api_container = deployment
        .spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|pod_spec| {
            pod_spec
                .containers
                .iter()
                .find(|c| c.name == crate::constants::CONTAINER_NAME_BINDCAR)
        });

    let mut patch_containers = vec![];

    // Add bind9 container name to preserve ordering (strategic merge needs this)
    patch_containers.push(json!({
        "name": crate::constants::CONTAINER_NAME_BIND9
    }));

    // Add api container with only the fields we want to update
    if let Some(api) = api_container {
        let mut api_patch = json!({
            "name": crate::constants::CONTAINER_NAME_BINDCAR
        });

        // Only include image if it exists (from bindcarConfig)
        if let Some(ref image) = api.image {
            api_patch["image"] = json!(image);
        }

        // Only include env if it exists (from bindcarConfig)
        if let Some(ref env) = api.env {
            api_patch["env"] = json!(env);
        }

        // Only include imagePullPolicy if it exists (from bindcarConfig)
        if let Some(ref pull_policy) = api.image_pull_policy {
            api_patch["imagePullPolicy"] = json!(pull_policy);
        }

        // Only include resources if they exist (from bindcarConfig)
        if let Some(ref resources) = api.resources {
            api_patch["resources"] = json!(resources);
        }

        patch_containers.push(api_patch);
    }

    // Get labels from desired deployment (includes role label if present on instance)
    let labels = deployment.metadata.labels.as_ref();
    let pod_labels = deployment
        .spec
        .as_ref()
        .and_then(|s| s.template.metadata.as_ref())
        .and_then(|m| m.labels.as_ref());

    // NOTE: We do NOT patch spec.selector because it is immutable in Kubernetes
    // Attempting to change selector labels will cause an API error: "field is immutable"

    let mut patch = json!({
        "spec": {
            "replicas": deployment.spec.as_ref().and_then(|s| s.replicas),
            "template": {
                "spec": {
                    "containers": patch_containers,
                    "$setElementOrder/containers": [
                        {"name": crate::constants::CONTAINER_NAME_BIND9},
                        {"name": crate::constants::CONTAINER_NAME_BINDCAR}
                    ]
                }
            }
        }
    });

    // Add metadata labels if present
    // NOTE: Strategic merge will update/add our labels but preserve any other labels
    // added by other controllers (e.g., kubectl, Helm, etc.)
    if let Some(labels) = labels {
        patch["metadata"] = json!({"labels": labels});
    }

    // Add pod template labels if present
    // When pod template labels change, Kubernetes will recreate pods with new labels
    if let Some(pod_labels) = pod_labels {
        patch["spec"]["template"]["metadata"] = json!({"labels": pod_labels});
    }

    api.patch(name, &PatchParams::default(), &Patch::Strategic(&patch))
        .await?;

    Ok(())
}

/// Create or update the Service for BIND9
async fn create_or_update_service(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Result<()> {
    // Get custom service spec based on instance role from cluster (namespace-scoped or global)
    let custom_spec = cluster
        .and_then(|c| match instance.spec.role {
            crate::crd::ServerRole::Primary => c
                .spec
                .common
                .primary
                .as_ref()
                .and_then(|p| p.service.as_ref()),
            crate::crd::ServerRole::Secondary => c
                .spec
                .common
                .secondary
                .as_ref()
                .and_then(|s| s.service.as_ref()),
        })
        .or_else(|| {
            // Fall back to global cluster if no namespace-scoped cluster
            cluster_provider.and_then(|gc| match instance.spec.role {
                crate::crd::ServerRole::Primary => gc
                    .spec
                    .common
                    .primary
                    .as_ref()
                    .and_then(|p| p.service.as_ref()),
                crate::crd::ServerRole::Secondary => gc
                    .spec
                    .common
                    .secondary
                    .as_ref()
                    .and_then(|s| s.service.as_ref()),
            })
        });

    let service = build_service(name, namespace, instance, custom_spec);
    let svc_api: Api<Service> = Api::namespaced(client.clone(), namespace);

    if let Ok(existing) = svc_api.get(name).await {
        // Service exists, update it (preserve clusterIP)
        info!("Updating Service {}/{}", namespace, name);
        let mut updated_service = service;
        if let Some(ref mut spec) = updated_service.spec {
            if let Some(ref existing_spec) = existing.spec {
                spec.cluster_ip.clone_from(&existing_spec.cluster_ip);
                spec.cluster_ips.clone_from(&existing_spec.cluster_ips);
            }
        }
        svc_api
            .replace(name, &PostParams::default(), &updated_service)
            .await?;
    } else {
        // Service doesn't exist, create it
        info!("Creating Service {}/{}", namespace, name);
        svc_api.create(&PostParams::default(), &service).await?;
    }

    Ok(())
}

/// Deletes all resources associated with a `Bind9Instance`.
///
/// Cleans up Kubernetes resources in reverse order:
/// 1. Service
/// 2. Deployment
/// 3. `ConfigMap`
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance` - The `Bind9Instance` resource to delete
///
/// # Returns
///
/// * `Ok(())` - If deletion succeeded or resources didn't exist
/// * `Err(_)` - If a critical error occurred during deletion
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail during resource deletion.
pub async fn delete_bind9instance(ctx: Arc<Context>, instance: Bind9Instance) -> Result<()> {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    info!("Deleting Bind9Instance: {}/{}", namespace, name);

    // Delete resources in reverse order (Service, Deployment, ConfigMap)
    delete_resources(&ctx.client, &namespace, &name).await?;

    info!("Successfully deleted resources for {}/{}", namespace, name);

    Ok(())
}

/// Delete all Kubernetes resources for a `Bind9Instance`
pub(super) async fn delete_resources(client: &Client, namespace: &str, name: &str) -> Result<()> {
    let delete_params = kube::api::DeleteParams::default();

    // 1. Delete Service (if it exists)
    let svc_api: Api<Service> = Api::namespaced(client.clone(), namespace);
    match svc_api.delete(name, &delete_params).await {
        Ok(_) => info!("Deleted Service {}/{}", namespace, name),
        Err(e) => warn!("Failed to delete Service {}/{}: {}", namespace, name, e),
    }

    // 2. Delete Deployment (if it exists)
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    match deploy_api.delete(name, &delete_params).await {
        Ok(_) => info!("Deleted Deployment {}/{}", namespace, name),
        Err(e) => warn!("Failed to delete Deployment {}/{}: {}", namespace, name, e),
    }

    // 3. Delete ConfigMap (if it exists)
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let cm_name = format!("{name}-config");
    match cm_api.delete(&cm_name, &delete_params).await {
        Ok(_) => info!("Deleted ConfigMap {}/{}", namespace, cm_name),
        Err(e) => warn!(
            "Failed to delete ConfigMap {}/{}: {}",
            namespace, cm_name, e
        ),
    }

    // 4. Delete RNDC Secret (if it exists)
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret_name = format!("{name}-rndc-key");
    match secret_api.delete(&secret_name, &delete_params).await {
        Ok(_) => info!("Deleted Secret {}/{}", namespace, secret_name),
        Err(e) => warn!(
            "Failed to delete Secret {}/{}: {}",
            namespace, secret_name, e
        ),
    }

    // 5. Delete ServiceAccount (if it exists and is owned by this instance)
    let sa_api: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    let sa_name = crate::constants::BIND9_SERVICE_ACCOUNT;
    match sa_api.get(sa_name).await {
        Ok(sa) => {
            // Check if this instance owns the ServiceAccount
            let is_owner = sa
                .metadata
                .owner_references
                .as_ref()
                .is_some_and(|owners| owners.iter().any(|owner| owner.name == name));

            if is_owner {
                match sa_api.delete(sa_name, &delete_params).await {
                    Ok(_) => info!("Deleted ServiceAccount {}/{}", namespace, sa_name),
                    Err(e) => warn!(
                        "Failed to delete ServiceAccount {}/{}: {}",
                        namespace, sa_name, e
                    ),
                }
            } else {
                debug!(
                    "ServiceAccount {}/{} is not owned by this instance, skipping deletion",
                    namespace, sa_name
                );
            }
        }
        Err(e) => {
            debug!(
                "ServiceAccount {}/{} does not exist or cannot be retrieved: {}",
                namespace, sa_name, e
            );
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod resources_tests;
