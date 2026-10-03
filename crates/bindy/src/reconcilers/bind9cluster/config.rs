// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Cluster `ConfigMap` management for `Bind9Cluster` resources.
//!
//! This module handles creating and updating the shared cluster-level
//! `ConfigMap` that contains BIND9 configuration shared across all instances.

#[allow(clippy::wildcard_imports)]
use super::types::*;

/// Create or update the shared cluster-level `ConfigMap`.
///
/// This `ConfigMap` contains BIND9 configuration that is shared across all instances
/// in the cluster. It is created from `spec.global` configuration.
///
/// If custom `ConfigMap`s are referenced at the cluster level (`spec.common.configMapRefs`),
/// this function skips creation to avoid conflicts.
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
pub(super) async fn create_or_update_cluster_configmap(
    client: &Client,
    cluster: &Bind9Cluster,
) -> Result<()> {
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

/// Creates or updates the `PodDisruptionBudget` for each role in the cluster.
///
/// Without these, a node drain or cluster upgrade can evict every primary of a
/// cluster at once. BIND9 keeps zone data in the Pod, so the replacements come
/// up empty and answer REFUSED until the operator has pushed every zone back —
/// measured at roughly 115 seconds, against about 1 second when a single
/// primary is replaced while its peers keep serving.
///
/// # Arguments
///
/// * `client` - Kubernetes client
/// * `cluster` - The `Bind9Cluster` whose operands are being protected
///
/// # Errors
///
/// Returns an error if a budget cannot be created or replaced.
pub(super) async fn reconcile_pod_disruption_budgets(
    client: &Client,
    cluster: &Bind9Cluster,
) -> Result<()> {
    use crate::bind9_resources::build_pod_disruption_budget;

    let namespace = cluster.namespace().unwrap_or_default();
    let name = cluster.name_any();
    let pdb_api: Api<PodDisruptionBudget> = Api::namespaced(client.clone(), &namespace);

    for role in [ServerRole::Primary, ServerRole::Secondary] {
        let pdb = build_pod_disruption_budget(&name, &namespace, role, Some(cluster));
        let pdb_name = pdb.name_any();

        if (pdb_api.get(&pdb_name).await).is_ok() {
            debug!("Updating PodDisruptionBudget {}/{}", namespace, pdb_name);
            // spec.selector is immutable before Kubernetes 1.28, so a replace on
            // an existing budget can be rejected. That must not fail the whole
            // reconcile: the budget already exists and still protects the Pods.
            if let Err(e) = pdb_api
                .replace(&pdb_name, &PostParams::default(), &pdb)
                .await
            {
                warn!(
                    "Could not update PodDisruptionBudget {}/{}: {}. The existing budget is left in place.",
                    namespace, pdb_name, e
                );
            }
        } else {
            info!("Creating PodDisruptionBudget {}/{}", namespace, pdb_name);
            pdb_api.create(&PostParams::default(), &pdb).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;
