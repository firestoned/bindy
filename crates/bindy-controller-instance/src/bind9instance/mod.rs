// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

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
//! - `template_drift` - Semantic comparison of the pod-template fields bindy
//!   owns, absorbing the API server's defaulting
//! - [`types`] - Shared types and imports
//! - [`zones`] - Zone reconciliation logic

// Submodules
pub mod cluster_helpers;
pub mod config;
pub mod resources;
pub mod status_helpers;
mod template_drift;
pub mod types;
pub mod zones;

// Re-export public APIs for external use

// Internal imports
use cluster_helpers::{build_cluster_reference, fetch_cluster_info};
use resources::{create_or_update_resources, delete_resources};
use status_helpers::{
    observed_generations, update_status, update_status_from_deployment, ObservedGenerations,
};
#[allow(clippy::wildcard_imports)]
use types::*;
use zones::reconcile_instance_zones as reconcile_zones_internal;

use crate::rollout::{rollout_condition, rollout_queued, InstanceId, RolloutGate, RolloutQueue};
use bindy_controller_sdk::finalizers::{ensure_finalizer, handle_deletion};

/// Re-check delay for a rotation that is already due.
///
/// The reconcile that sees an overdue key rotates it, and the new Secret's
/// watch event schedules the next wake. This short re-check only matters if
/// that event is lost.
pub const ROTATION_OVERDUE_RECHECK: std::time::Duration = std::time::Duration::from_secs(30);

/// Margin past the instant a key falls due, so the wake never lands a moment
/// before it and finds nothing to do.
pub const ROTATION_DUE_MARGIN: std::time::Duration = std::time::Duration::from_secs(1);

/// Milliseconds per second, for rounding the wake up to whole seconds.
const MILLIS_PER_SECOND: i64 = 1000;

/// When this instance must next be reconciled for its RNDC key rotation.
///
/// With no periodic resync (ADR-0016) nothing else would notice that a key
/// has fallen due: the Secret does not change until it is rotated. The key is
/// due at `rotate-at`, but never sooner than
/// `MIN_TIME_BETWEEN_ROTATIONS_HOURS` after it was created (the rate limit
/// `should_rotate_secret` enforces), so the wake is set for the later of the
/// two plus [`ROTATION_DUE_MARGIN`].
///
/// # Arguments
///
/// * `config` - RNDC configuration with rotation settings
/// * `secret` - The RNDC Secret with rotation annotations
/// * `now` - The current time
///
/// # Returns
///
/// The delay until the key falls due, [`ROTATION_OVERDUE_RECHECK`] when it
/// already has, or `None` when auto-rotation is off or no rotation is
/// scheduled.
#[must_use]
pub fn calculate_requeue_duration(
    config: &crate::crd::RndcKeyConfig,
    secret: &Secret,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<std::time::Duration> {
    if !config.auto_rotate {
        return None;
    }

    let annotations = secret.metadata.annotations.as_ref()?;
    let (created_at, rotate_at, _rotation_count) =
        crate::bind9::rndc::parse_rotation_annotations(annotations).ok()?;
    let rotate_at = rotate_at?;

    let earliest_allowed =
        created_at + chrono::Duration::hours(crate::constants::MIN_TIME_BETWEEN_ROTATIONS_HOURS);
    let due_at = rotate_at.max(earliest_allowed);

    let millis_until_due = due_at.signed_duration_since(now).num_milliseconds();
    if millis_until_due <= 0 {
        return Some(ROTATION_OVERDUE_RECHECK);
    }

    let secs_until_due = (millis_until_due + MILLIS_PER_SECOND - 1) / MILLIS_PER_SECOND;
    let secs_until_due = u64::try_from(secs_until_due).ok()?;
    Some(std::time::Duration::from_secs(secs_until_due) + ROTATION_DUE_MARGIN)
}

/// Detects whether the parent cluster's configuration changed since it was last observed.
///
/// Compares the parent's current `metadata.generation` against the parent
/// generation recorded in the instance's `status.observedParentGeneration`.
/// These are the ONLY two values that may be compared: the instance's own
/// `observed_generation` tracks a different, unrelated counter.
///
/// # Arguments
///
/// * `parent_generation` - Current `metadata.generation` of the referenced
///   `Bind9Cluster`/`ClusterBind9Provider` (`None` if no parent exists)
/// * `observed_parent_generation` - Parent generation recorded during the last
///   successful reconciliation (`None` if never recorded)
///
/// # Returns
///
/// `true` if the parent exists and its generation differs from the recorded
/// value (or was never recorded), `false` otherwise.
#[must_use]
pub fn parent_generation_changed(
    parent_generation: Option<i64>,
    observed_parent_generation: Option<i64>,
) -> bool {
    match (parent_generation, observed_parent_generation) {
        (Some(parent), Some(observed)) => parent != observed,
        (Some(_), None) => true, // Parent exists but was never observed
        (None, _) => false,      // No parent - nothing to track
    }
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
    let patch_params = PatchParams::default();
    let patch = kube::api::Patch::Merge(&status);
    bindy_controller_sdk::retry::retry_api_call(
        || api.patch_status(&name, &patch_params, &patch),
        "patch Bind9Instance status",
    )
    .await?;

    debug!(
        "Updated rotation status for {}/{}: rotation_count={}, rotate_at={:?}",
        namespace, name, rotation_count, rotate_at
    );

    Ok(())
}

/// Implement cleanup trait for `Bind9Instance` finalizer management.
/// Removes what a `Bind9Instance` owns before its finalizer is dropped.
async fn cleanup_bind9instance(resource: &Bind9Instance, client: &Client) -> Result<()> {
    let namespace = resource.namespace().unwrap_or_default();
    let name = resource.name_any();

    // Check if this instance is managed by a Bind9Cluster
    let is_managed: bool = resource
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
/// * `rollouts` - The process-wide queue that staggers pod-template changes
///   across instances sharing a zone or a cluster (ADR-0018)
///
/// # Returns
///
/// * `Ok(next_wake)` - Reconciliation succeeded. `next_wake` is the delay
///   until the instance's RNDC key falls due for rotation, when auto-rotation
///   is on ([`calculate_requeue_duration`]); `None` otherwise. There is no
///   periodic resync: every other change arrives as a watch event (ADR-0016).
/// * `Err(_)` - If resource creation/update failed
///
/// # Example
///
/// ```text
/// use bindy::reconcilers::reconcile_bind9instance;
/// use bindy::crd::Bind9Instance;
/// use bindy::context::Context;
/// use std::sync::Arc;
///
/// async fn handle_instance(ctx: Arc<Context>, instance: Bind9Instance) -> anyhow::Result<()> {
///     let _next_wake = reconcile_bind9instance(ctx, instance, &rollouts).await?;
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or resource creation/update fails.
#[allow(clippy::too_many_lines)]
pub(crate) async fn reconcile_bind9instance(
    ctx: Arc<Context>,
    instance: Bind9Instance,
    rollouts: &RolloutQueue,
) -> Result<Option<std::time::Duration>> {
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
        // A deleted instance waits for nothing and must not hold a place in
        // the rollout queue (ADR-0018), nor a known no-op patch.
        rollouts.forget(&InstanceId::of(&instance));
        handle_deletion(&client, &instance, FINALIZER_BIND9_INSTANCE, || {
            cleanup_bind9instance(&instance, &client)
        })
        .await?;
        return Ok(None);
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

    // Fetch cluster information early for rotation checking and zone reconciliation
    // We need this to set the cluster reference in DNSZone status
    let (cluster, cluster_provider) = fetch_cluster_info(&client, &namespace, &instance).await;

    // Check if parent cluster configuration has changed since last reconciliation
    // This is critical for detecting when RNDC config is added/changed at the cluster level.
    //
    // The parent's generation is tracked SEPARATELY from the instance's own
    // observed_generation via status.observedParentGeneration - the two counters
    // are unrelated and must never be compared against each other.
    let parent_generation = cluster
        .as_ref()
        .and_then(|c| c.metadata.generation)
        .or_else(|| {
            cluster_provider
                .as_ref()
                .and_then(|cp| cp.metadata.generation)
        });
    let observed_parent_generation = instance
        .status
        .as_ref()
        .and_then(|s| s.observed_parent_generation);

    let parent_config_changed =
        parent_generation_changed(parent_generation, observed_parent_generation);

    if parent_config_changed {
        debug!(
            "Parent cluster generation ({:?}) differs from last observed parent generation ({:?})",
            parent_generation, observed_parent_generation
        );
    }

    if parent_config_changed {
        info!(
            "Parent cluster configuration may have changed for Bind9Instance {}/{}, will check for drift",
            namespace, name
        );
    }

    // Check if ALL required resources actually exist AND match desired state (drift detection)
    let (
        all_resources_exist,
        deployment_labels_match,
        rotation_needed,
        config_drifted,
        rotation_wake,
    ) = {
        let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
        let service_api: Api<Service> = Api::namespaced(client.clone(), &namespace);
        let configmap_api: Api<ConfigMap> = Api::namespaced(client.clone(), &namespace);
        let secret_api: Api<Secret> = Api::namespaced(client.clone(), &namespace);

        // Fetch deployment to check if it exists AND if OUR labels match
        let current_deployment = deployment_api.get(&name).await.ok();
        let (deployment_exists, labels_match) = match current_deployment.as_ref() {
            Some(deployment) => (
                true,
                deployment_labels_are_current(deployment, &name, &instance),
            ),
            None => (false, false),
        };

        let service_exists = service_api.get(&name).await.is_ok();

        // Check ConfigMap - managed instances use cluster ConfigMap, standalone use instance ConfigMap
        let configmap_name = if is_managed {
            format!("{}-config", spec.cluster_ref)
        } else {
            format!("{name}-config")
        };
        let current_configmap = configmap_api.get(&configmap_name).await.ok();
        let configmap_exists = current_configmap.is_some();

        // Config drift: the mounted config is behind what this operator renders
        // (e.g. after an upgrade), or the pods were never rolled onto it. A
        // render error counts as drift so the reconcile runs and reports it.
        let config_drifted = match resources::desired_configmap_for_instance(
            &name,
            &namespace,
            &instance,
            cluster.as_ref(),
        ) {
            Ok(desired) => resources::config_drifted(
                current_configmap.as_ref(),
                current_deployment.as_ref(),
                desired.as_ref(),
            ),
            Err(_) => true,
        };

        // Volume drift: the running Deployment lacks a pod volume or bind9
        // mount this operator renders (the DNSSEC key volume, once signing is
        // enabled on an existing cluster). Compared by name only, so the RNDC
        // Secret name passed here does not matter.
        let volumes_drifted = current_deployment.as_ref().is_some_and(|deployment| {
            let desired = crate::bind9_resources::build_deployment(
                &name,
                &namespace,
                &instance,
                cluster.as_ref(),
                cluster_provider.as_ref(),
                &format!("{name}-rndc-key"),
            );
            resources::volumes_missing(deployment, &desired)
        });
        let config_drifted = config_drifted || volumes_drifted;

        // Check Secret existence AND rotation status
        let secret_name = format!("{name}-rndc-key");
        let (secret_exists, needs_rotation, rotation_wake) =
            match secret_api.get(&secret_name).await {
                Ok(secret) => {
                    // Resolve RNDC config to check if rotation is due
                    let rndc_config = resources::resolve_full_rndc_config(
                        &instance,
                        cluster.as_ref(),
                        cluster_provider.as_ref(),
                    );

                    // Check if rotation is needed using the existing function
                    let needs_rotation =
                        resources::should_rotate_secret(&secret, &rndc_config).unwrap_or(false);

                    if needs_rotation {
                        debug!(
                            "RNDC Secret {}/{} rotation is due, will trigger reconciliation",
                            namespace, secret_name
                        );
                    }

                    // Nothing announces that a key has fallen due, so the instance
                    // schedules its own wake for that instant (ADR-0016).
                    let rotation_wake =
                        calculate_requeue_duration(&rndc_config, &secret, chrono::Utc::now());

                    (true, needs_rotation, rotation_wake)
                }
                Err(_) => (false, false, None),
            };

        let all_exist = deployment_exists && service_exists && configmap_exists && secret_exists;
        (
            all_exist,
            labels_match,
            needs_rotation,
            config_drifted,
            rotation_wake,
        )
    };
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
    // 3. Resources are missing (drift detected), OR
    // 4. RNDC Secret rotation is due, OR
    // 5. Parent cluster configuration has changed
    let should_reconcile =
        bindy_controller_sdk::status::should_reconcile(current_generation, observed_generation);

    // REMOVED: Zone discovery logic - instances no longer select zones
    // Zone selection is now reversed: DNSZone.spec.bind9_instances_from selects instances
    // This logic was removed as part of the architectural change to reverse selector direction

    // A pod-template change queued behind another rollout (ADR-0018) must be
    // applied when the instance is woken, whatever woke it.
    let rollout_pending = rollout_queued(&instance);

    if !should_reconcile
        && all_resources_exist
        && deployment_labels_match
        && !rotation_needed
        && !parent_config_changed
        && !config_drifted
        && !rollout_pending
    {
        debug!(
            "Spec unchanged (generation={:?}), all resources exist, deployment labels match, no rotation needed, and parent config unchanged - skipping resource reconciliation",
            current_generation
        );
        // Update status from current deployment state (only patches if status changed)
        // Preserve existing cluster_ref from instance status if available
        let cluster_ref = instance.status.as_ref().and_then(|s| s.cluster_ref.clone());
        update_status_from_deployment(
            &client,
            &namespace,
            &name,
            &instance,
            cluster_ref,
            ObservedGenerations {
                instance: instance.metadata.generation,
                parent: parent_generation,
            },
            None,
        )
        .await?;

        // Reconcile zones after status update
        reconcile_zones_internal(&client, &ctx.stores, &instance).await?;

        return Ok(rotation_wake);
    }

    // If we reach here, reconciliation is needed because:
    // - Spec changed (generation mismatch), OR
    // - Resources don't exist (drift), OR
    // - Deployment labels don't match desired state (drift), OR
    // - RNDC Secret rotation is due, OR
    // - Parent cluster configuration has changed
    if config_drifted && all_resources_exist {
        info!(
            "BIND config for {}/{} is behind what this operator renders, updating it and rolling the pods",
            namespace, name
        );
    }

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

    if rotation_needed {
        info!(
            "RNDC Secret rotation is due for Bind9Instance {}/{}, triggering reconciliation",
            namespace, name
        );
    }

    if parent_config_changed {
        info!(
            "Parent cluster configuration changed for Bind9Instance {}/{}, triggering reconciliation to apply new config",
            namespace, name
        );
    }

    debug!(
        "Reconciliation needed: current_generation={:?}, observed_generation={:?}",
        current_generation, observed_generation
    );

    // Create or update resources
    let mut next_wake = rotation_wake;
    let gate = RolloutGate {
        stores: &ctx.stores,
        queue: rollouts,
    };
    match create_or_update_resources(&client, &namespace, &name, &instance, &gate).await {
        Ok(resources::AppliedResources {
            cluster,
            cluster_provider,
            secret,
            rollout,
        }) => {
            info!(
                "Successfully created/updated resources for {}/{}",
                namespace, name
            );

            // Build cluster reference for status
            let cluster_ref = build_cluster_reference(cluster.as_ref(), cluster_provider.as_ref());

            // Record the parent generation observed during this successful
            // reconciliation. Use the freshly fetched parent (it may have been
            // re-fetched by create_or_update_resources).
            let observed_parent_generation = cluster
                .as_ref()
                .and_then(|c| c.metadata.generation)
                .or_else(|| {
                    cluster_provider
                        .as_ref()
                        .and_then(|cp| cp.metadata.generation)
                });

            // Update status based on actual deployment state. A queued
            // pod-template change keeps the previous observed generations
            // and adds the Rollout condition (ADR-0018).
            update_status_from_deployment(
                &client,
                &namespace,
                &name,
                &instance,
                cluster_ref,
                observed_generations(&instance, observed_parent_generation, &rollout),
                rollout_condition(&rollout),
            )
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

                // The Secret as written now (it may just have been rotated)
                // decides the next rotation wake.
                next_wake = calculate_requeue_duration(&rndc_config, secret, chrono::Utc::now());
            }

            // Reconcile zones after deployment creation/update
            reconcile_zones_internal(&client, &ctx.stores, &instance).await?;
        }
        Err(e) => {
            error!(
                "Failed to create/update resources for {}/{}: {}",
                namespace, name, e
            );

            // Update status to show error. A rendered configuration that does
            // not parse was refused before the ConfigMap was written (ADR-0013):
            // say so, so the pods still serving the last good one are explained.
            let (reason, message) = match bindy_bind9::config_check::find_invalid_config(&e) {
                Some(invalid) => (
                    crate::status_reasons::REASON_CONFIGURATION_INVALID,
                    format!("Configuration not published: {invalid}"),
                ),
                None => (REASON_NOT_READY, format!("Failed to create resources: {e}")),
            };
            let error_condition = Condition {
                r#type: CONDITION_TYPE_READY.to_string(),
                status: "False".to_string(),
                reason: Some(reason.to_string()),
                message: Some(message),
                last_transition_time: Some(Utc::now().to_rfc3339()),
            };
            // No cluster info available on error, pass None for cluster_ref.
            // Preserve the previously observed parent generation: the new parent
            // config was NOT applied, so it must not be recorded as observed.
            update_status(
                &client,
                &instance,
                vec![error_condition],
                None,
                ObservedGenerations {
                    instance: instance.metadata.generation,
                    parent: observed_parent_generation,
                },
            )
            .await?;

            return Err(e);
        }
    }

    Ok(next_wake)
}

/// Whether a Deployment already carries every label the operator manages.
///
/// Used by the reconcile short-circuit: when this returns `false`, resource
/// reconciliation runs even though nothing else looks stale.
///
/// Two label sets are checked, and the second is what makes an operator
/// upgrade converge. `spec.selector` is immutable, so labels added after a
/// Deployment exists can only go on the Pod template
/// (`build_pod_labels_from_instance`, a superset of the metadata set). A
/// Deployment created before topology spreading has correct *metadata* labels
/// but no `bindy.firestoned.io/cluster` on its Pod template; checking metadata
/// alone would let the short-circuit skip it forever, leaving it permanently
/// without spread constraints.
///
/// Both checks are subset tests, not equality: other controllers, `kubectl`,
/// and Helm add labels of their own, and those are none of our business.
fn deployment_labels_are_current(
    deployment: &Deployment,
    name: &str,
    instance: &Bind9Instance,
) -> bool {
    let desired_metadata = crate::bind9_resources::build_labels_from_instance(name, instance);
    let metadata_matches = deployment.metadata.labels.as_ref().is_some_and(|actual| {
        desired_metadata
            .iter()
            .all(|(key, value)| actual.get(key) == Some(value))
    });

    let desired_pod = crate::bind9_resources::build_pod_labels_from_instance(name, instance);
    let pod_matches = deployment
        .spec
        .as_ref()
        .and_then(|s| s.template.metadata.as_ref())
        .and_then(|m| m.labels.as_ref())
        .is_some_and(|actual| {
            desired_pod
                .iter()
                .all(|(key, value)| actual.get(key) == Some(value))
        });

    if metadata_matches && !pod_matches {
        debug!(
            "Deployment {name} Pod template labels are stale (missing operator-managed labels); \
             reconciling resources"
        );
    }

    metadata_matches && pod_matches
}

/// Test-only re-export of `deployment_labels_are_current`.
#[cfg(test)]
pub(crate) fn deployment_labels_are_current_for_test(
    deployment: &Deployment,
    name: &str,
    instance: &Bind9Instance,
) -> bool {
    deployment_labels_are_current(deployment, name, instance)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
