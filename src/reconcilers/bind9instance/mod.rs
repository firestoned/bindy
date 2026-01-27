// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 instance reconciliation logic.
//!
//! This module handles the lifecycle of BIND9 DNS server deployments in Kubernetes.
//! It creates and manages Deployments, `ConfigMaps`, and Services for each `Bind9Instance`.
//!
//! ## Module Structure
//!
//! - [`cluster_helpers`] - Cluster integration and reference management
//! - [`config`] - RNDC configuration precedence resolution
//! - [`resources`] - Resource lifecycle (`ConfigMap`, Deployment, Service)
//! - [`status_helpers`] - Status calculation and updates
//! - [`types`] - Shared types and imports
//! - [`zones`] - Zone reconciliation logic

// Submodules
pub mod cluster_helpers;
pub mod config;
pub mod resources;
pub mod status_helpers;
pub mod types;
pub mod zones;

// Re-export public APIs for external use
pub use zones::reconcile_instance_zones;

// Internal imports
use cluster_helpers::{build_cluster_reference, fetch_cluster_info};
use resources::{create_or_update_resources, delete_resources};
use status_helpers::{update_status, update_status_from_deployment};
#[allow(clippy::wildcard_imports)]
use types::*;
use zones::reconcile_instance_zones as reconcile_zones_internal;

use crate::reconcilers::finalizers::{ensure_finalizer, handle_deletion, FinalizerCleanup};

/// Calculate the requeue duration for the next reconciliation based on RNDC rotation schedule.
///
/// If auto-rotation is enabled and a rotation time is scheduled, this function calculates
/// the duration until that rotation time. If the rotation is overdue, it returns a minimal
/// duration to trigger immediate reconciliation.
///
/// # Arguments
///
/// * `config` - RNDC configuration with rotation settings
/// * `secret` - The RNDC Secret with rotation annotations
///
/// # Returns
///
/// Duration until next reconciliation. Returns `None` if rotation is disabled or Secret
/// has no rotation annotations.
///
/// # Examples
///
/// ```rust,ignore
/// use bindy::crd::RndcKeyConfig;
/// use k8s_openapi::api::core::v1::Secret;
/// use bindy::reconcilers::bind9instance::calculate_requeue_duration;
///
/// let config = RndcKeyConfig {
///     auto_rotate: true,
///     rotate_after: "720h".to_string(),
///     ..Default::default()
/// };
///
/// // Create a secret with rotation annotations
/// let secret = Secret {
///     metadata: ObjectMeta {
///         annotations: Some(BTreeMap::from([
///             ("bindy.firestoned.io/rotation-created-at".to_string(), "2025-01-01T00:00:00Z".to_string()),
///             ("bindy.firestoned.io/rotation-rotate-at".to_string(), "2025-02-01T00:00:00Z".to_string()),
///         ])),
///         ..Default::default()
///     },
///     ..Default::default()
/// };
///
/// // Returns duration until rotate_at timestamp
/// let duration = calculate_requeue_duration(&config, &secret);
/// ```
#[allow(dead_code)] // Will be used when requeue logic is integrated
fn calculate_requeue_duration(
    config: &crate::crd::RndcKeyConfig,
    secret: &Secret,
) -> Option<std::time::Duration> {
    use chrono::Utc;

    // Only calculate requeue if auto-rotation is enabled
    if !config.auto_rotate {
        return None;
    }

    // Extract rotation annotations from Secret
    let annotations = secret.metadata.annotations.as_ref()?;
    let (_created_at, rotate_at, _rotation_count) =
        crate::bind9::rndc::parse_rotation_annotations(annotations).ok()?;

    // If no rotation scheduled, no need for specific requeue
    let rotate_at = rotate_at?;

    let now = Utc::now();
    let time_until_rotation = rotate_at.signed_duration_since(now);

    // If rotation is overdue or very soon, reconcile quickly (30 seconds)
    if time_until_rotation.num_seconds() <= 0 {
        return Some(std::time::Duration::from_secs(30));
    }

    // Otherwise, schedule reconciliation slightly before rotation time (5 minutes early)
    let requeue_secs = time_until_rotation
        .num_seconds()
        .saturating_sub(300) // 5 minutes early
        .max(30); // At least 30 seconds

    #[allow(clippy::cast_sign_loss)] // Value is guaranteed non-negative by max(30)
    Some(std::time::Duration::from_secs(requeue_secs as u64))
}

/// Update the `Bind9Instance` status with RNDC key rotation information.
///
/// Reads rotation metadata from the RNDC Secret annotations and updates the instance
/// status with current rotation state. This provides visibility into key age and
/// rotation schedule.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance` - The `Bind9Instance` resource to update
/// * `secret` - The RNDC Secret containing rotation annotations
/// * `config` - RNDC configuration with rotation settings
///
/// # Returns
///
/// `Ok(())` on success, error if status update fails.
///
/// # Errors
///
/// Returns an error if:
/// - Secret annotations are missing or malformed
/// - Status patch API call fails
async fn update_rotation_status(
    client: &Client,
    instance: &Bind9Instance,
    secret: &Secret,
    config: &crate::crd::RndcKeyConfig,
) -> Result<()> {
    use crate::crd::RndcKeyRotationStatus;

    // Only update status if auto-rotation is enabled
    if !config.auto_rotate {
        return Ok(());
    }

    let Some(annotations) = &secret.metadata.annotations else {
        debug!("Secret has no annotations, skipping rotation status update");
        return Ok(());
    };

    let (created_at, rotate_at, rotation_count) =
        crate::bind9::rndc::parse_rotation_annotations(annotations)?;

    // Determine last_rotated_at: if rotation_count > 0, the current created_at is when it was last rotated
    let last_rotated_at = if rotation_count > 0 {
        Some(created_at.to_rfc3339())
    } else {
        None
    };

    let rotation_status = RndcKeyRotationStatus {
        created_at: created_at.to_rfc3339(),
        rotate_at: rotate_at.map(|dt| dt.to_rfc3339()),
        last_rotated_at,
        rotation_count,
    };

    // Prepare status update
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    let status = serde_json::json!({
        "status": {
            "rndcKeyRotation": rotation_status
        }
    });

    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), &namespace);
    api.patch_status(
        &name,
        &PatchParams::default(),
        &kube::api::Patch::Merge(&status),
    )
    .await?;

    debug!(
        "Updated rotation status for {}/{}: rotation_count={}, rotate_at={:?}",
        namespace, name, rotation_count, rotate_at
    );

    Ok(())
}

/// Implement cleanup trait for `Bind9Instance` finalizer management.
#[async_trait::async_trait]
impl FinalizerCleanup for Bind9Instance {
    async fn cleanup(&self, client: &Client) -> Result<()> {
        let namespace = self.namespace().unwrap_or_default();
        let name = self.name_any();

        // Check if this instance is managed by a Bind9Cluster
        let is_managed: bool = self
            .metadata
            .labels
            .as_ref()
            .and_then(|labels| labels.get(BINDY_MANAGED_BY_LABEL))
            .is_some();

        if is_managed {
            info!(
                "Bind9Instance {}/{} is managed by a Bind9Cluster, skipping resource cleanup (cluster will handle it)",
                namespace, name
            );
            Ok(())
        } else {
            info!(
                "Running cleanup for standalone Bind9Instance {}/{}",
                namespace, name
            );
            delete_resources(client, &namespace, &name).await
        }
    }
}

/// Reconciles a `Bind9Instance` resource.
///
/// Creates or updates all Kubernetes resources needed to run a BIND9 DNS server:
/// - `ConfigMap` with BIND9 configuration files
/// - Deployment with BIND9 container pods
/// - Service for DNS traffic (TCP/UDP port 53)
///
/// # Arguments
///
/// * `ctx` - Operator context with Kubernetes client and reflector stores
/// * `instance` - The `Bind9Instance` resource to reconcile
///
/// # Returns
///
/// * `Ok(())` - If reconciliation succeeded
/// * `Err(_)` - If resource creation/update failed
///
/// # Example
///
/// ```rust,no_run
/// use bindy::reconcilers::reconcile_bind9instance;
/// use bindy::crd::Bind9Instance;
/// use bindy::context::Context;
/// use std::sync::Arc;
///
/// async fn handle_instance(ctx: Arc<Context>, instance: Bind9Instance) -> anyhow::Result<()> {
///     reconcile_bind9instance(ctx, instance).await?;
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or resource creation/update fails.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_bind9instance(ctx: Arc<Context>, instance: Bind9Instance) -> Result<()> {
    let client = ctx.client.clone();
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    info!("Reconciling Bind9Instance: {}/{}", namespace, name);
    debug!(
        namespace = %namespace,
        name = %name,
        generation = ?instance.metadata.generation,
        "Starting Bind9Instance reconciliation"
    );

    // Check if the instance is being deleted
    if instance.metadata.deletion_timestamp.is_some() {
        return handle_deletion(&client, &instance, FINALIZER_BIND9_INSTANCE).await;
    }

    // Add finalizer if not present
    ensure_finalizer(&client, &instance, FINALIZER_BIND9_INSTANCE).await?;

    let spec = &instance.spec;
    let replicas = spec.replicas.unwrap_or(1);
    let version = spec
        .version
        .as_deref()
        .unwrap_or(crate::constants::DEFAULT_BIND9_VERSION);

    debug!(
        cluster_ref = %spec.cluster_ref,
        replicas,
        version = %version,
        role = ?spec.role,
        "Instance configuration"
    );

    info!(
        "Bind9Instance {} configured with {} replicas, version {}",
        name, replicas, version
    );

    // Check if spec has changed using the standard generation check
    let current_generation = instance.metadata.generation;
    let observed_generation = instance.status.as_ref().and_then(|s| s.observed_generation);

    // Check if this instance is managed by a Bind9Cluster
    let is_managed: bool = instance
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(BINDY_MANAGED_BY_LABEL))
        .is_some();

    // Check if ALL required resources actually exist AND match desired state (drift detection)
    let (all_resources_exist, deployment_labels_match) = {
        let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
        let service_api: Api<Service> = Api::namespaced(client.clone(), &namespace);
        let configmap_api: Api<ConfigMap> = Api::namespaced(client.clone(), &namespace);
        let secret_api: Api<Secret> = Api::namespaced(client.clone(), &namespace);

        // Fetch deployment to check if it exists AND if OUR labels match
        let (deployment_exists, labels_match) = match deployment_api.get(&name).await {
            Ok(deployment) => {
                // Build desired labels from instance - these are the labels WE manage
                let desired_labels =
                    crate::bind9_resources::build_labels_from_instance(&name, &instance);

                // Check if deployment has all OUR labels with correct values
                // IMPORTANT: Only check labels we explicitly set via build_labels_from_instance()
                // Other controllers or users may add additional labels - we don't care about those
                let labels_match = if let Some(actual_labels) = &deployment.metadata.labels {
                    desired_labels
                        .iter()
                        .all(|(key, value)| actual_labels.get(key) == Some(value))
                } else {
                    false // No labels at all = no match
                };

                (true, labels_match)
            }
            Err(_) => (false, false),
        };

        let service_exists = service_api.get(&name).await.is_ok();

        // Check ConfigMap - managed instances use cluster ConfigMap, standalone use instance ConfigMap
        let configmap_name = if is_managed {
            format!("{}-config", spec.cluster_ref)
        } else {
            format!("{name}-config")
        };
        let configmap_exists = configmap_api.get(&configmap_name).await.is_ok();

        let secret_name = format!("{name}-rndc-key");
        let secret_exists = secret_api.get(&secret_name).await.is_ok();

        let all_exist = deployment_exists && service_exists && configmap_exists && secret_exists;
        (all_exist, labels_match)
    };

    // Fetch cluster information early for zone reconciliation
    // We need this to set the cluster reference in DNSZone status
    let (cluster, cluster_provider) = fetch_cluster_info(&client, &namespace, &instance).await;
    let cluster_ref = build_cluster_reference(cluster.as_ref(), cluster_provider.as_ref());

    if let Some(ref cr) = cluster_ref {
        debug!(
            "Built cluster reference for instance {}/{}: {}/{} in namespace {:?}",
            namespace, name, cr.kind, cr.name, cr.namespace
        );
    } else {
        debug!(
            "No cluster reference built for instance {}/{} - spec.clusterRef may be empty or cluster not found",
            namespace, name
        );
    }

    // Only reconcile resources if:
    // 1. Spec changed (generation mismatch), OR
    // 2. We haven't processed this resource yet (no observed_generation), OR
    // 3. Resources are missing (drift detected)
    let should_reconcile =
        crate::reconcilers::should_reconcile(current_generation, observed_generation);

    // REMOVED: Zone discovery logic - instances no longer select zones
    // Zone selection is now reversed: DNSZone.spec.bind9_instances_from selects instances
    // This logic was removed as part of the architectural change to reverse selector direction

    if !should_reconcile && all_resources_exist && deployment_labels_match {
        debug!(
            "Spec unchanged (generation={:?}), all resources exist, and deployment labels match - skipping resource reconciliation",
            current_generation
        );
        // Update status from current deployment state (only patches if status changed)
        // Preserve existing cluster_ref from instance status if available
        let cluster_ref = instance.status.as_ref().and_then(|s| s.cluster_ref.clone());
        update_status_from_deployment(&client, &namespace, &name, &instance, cluster_ref).await?;

        // Reconcile zones after status update
        reconcile_zones_internal(&client, &ctx.stores, &instance).await?;

        return Ok(());
    }

    // If we reach here, reconciliation is needed because:
    // - Spec changed (generation mismatch), OR
    // - Resources don't exist (drift), OR
    // - Deployment labels don't match desired state (drift)
    if !deployment_labels_match && all_resources_exist {
        info!(
            "Deployment labels don't match desired state for {}/{}, triggering reconciliation to update labels",
            namespace, name
        );
    }

    if !should_reconcile && !all_resources_exist {
        info!(
            "Drift detected for Bind9Instance {}/{}: One or more resources missing, will recreate",
            namespace, name
        );
    }

    debug!(
        "Reconciliation needed: current_generation={:?}, observed_generation={:?}",
        current_generation, observed_generation
    );

    // Create or update resources
    match create_or_update_resources(&client, &namespace, &name, &instance).await {
        Ok((cluster, cluster_provider, secret)) => {
            info!(
                "Successfully created/updated resources for {}/{}",
                namespace, name
            );

            // Build cluster reference for status
            let cluster_ref = build_cluster_reference(cluster.as_ref(), cluster_provider.as_ref());

            // Update status based on actual deployment state
            update_status_from_deployment(&client, &namespace, &name, &instance, cluster_ref)
                .await?;

            // Update rotation status if Secret is available
            if let Some(ref secret) = secret {
                // Resolve RNDC config for rotation status update
                let rndc_config = resources::resolve_full_rndc_config(
                    &instance,
                    cluster.as_ref(),
                    cluster_provider.as_ref(),
                );

                if let Err(e) =
                    update_rotation_status(&client, &instance, secret, &rndc_config).await
                {
                    warn!(
                        "Failed to update rotation status for {}/{}: {}",
                        namespace, name, e
                    );
                    // Non-fatal error, continue reconciliation
                }
            }

            // Reconcile zones after deployment creation/update
            reconcile_zones_internal(&client, &ctx.stores, &instance).await?;
        }
        Err(e) => {
            error!(
                "Failed to create/update resources for {}/{}: {}",
                namespace, name, e
            );

            // Update status to show error
            let error_condition = Condition {
                r#type: CONDITION_TYPE_READY.to_string(),
                status: "False".to_string(),
                reason: Some(REASON_NOT_READY.to_string()),
                message: Some(format!("Failed to create resources: {e}")),
                last_transition_time: Some(Utc::now().to_rfc3339()),
            };
            // No cluster info available on error, pass None
            update_status(&client, &instance, vec![error_condition], None).await?;

            return Err(e);
        }
    }

    Ok(())
}

/// Delete handler for `Bind9Instance` resources (cleanup logic).
///
/// This function is kept for backward compatibility but deletion is now handled
/// by the finalizer in `reconcile_bind9instance`.
///
/// # Errors
///
/// This function currently never returns an error, but returns `Result` for API consistency.
pub async fn delete_bind9instance(ctx: Arc<Context>, instance: Bind9Instance) -> Result<()> {
    let _client = ctx.client.clone();
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    info!(
        "Delete called for Bind9Instance {}/{} (handled by finalizer)",
        namespace, name
    );

    // Deletion is now handled by the finalizer in reconcile_bind9instance
    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
