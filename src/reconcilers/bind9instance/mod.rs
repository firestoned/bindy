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
//! - [`resources`] - Resource lifecycle (`ConfigMap`, Deployment, Service)
//! - [`status_helpers`] - Status calculation and updates
//! - [`types`] - Shared types and imports
//! - [`zones`] - Zone reconciliation logic

// Submodules
pub mod cluster_helpers;
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
/// * `ctx` - Controller context with Kubernetes client and reflector stores
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

    // Check if resources actually exist (drift detection)
    let deployment_exists = {
        let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
        deployment_api.get(&name).await.is_ok()
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

    if !should_reconcile && deployment_exists {
        debug!(
            "Spec unchanged (generation={:?}) and resources exist, skipping resource reconciliation",
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

    if !should_reconcile && !deployment_exists {
        info!(
            "Drift detected for Bind9Instance {}/{}: Deployment missing, will recreate",
            namespace, name
        );
    }

    debug!(
        "Reconciliation needed: current_generation={:?}, observed_generation={:?}",
        current_generation, observed_generation
    );

    // Create or update resources
    match create_or_update_resources(&client, &namespace, &name, &instance).await {
        Ok((cluster, cluster_provider)) => {
            info!(
                "Successfully created/updated resources for {}/{}",
                namespace, name
            );

            // Build cluster reference for status
            let cluster_ref = build_cluster_reference(cluster.as_ref(), cluster_provider.as_ref());

            // Update status based on actual deployment state
            update_status_from_deployment(&client, &namespace, &name, &instance, cluster_ref)
                .await?;

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
