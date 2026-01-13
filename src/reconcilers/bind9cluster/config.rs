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

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;
