// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Instance drift detection for `Bind9Cluster` resources.
//!
//! This module detects when the actual managed instances don't match
//! the desired replica counts in the cluster spec.

#[allow(clippy::wildcard_imports)]
use super::types::*;
use crate::reconcilers::pagination::list_all_paginated;

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
pub(super) async fn detect_instance_drift(
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
    let instances = list_all_paginated(&api, ListParams::default()).await?;

    // Filter for managed instances of this cluster
    let managed_instances: Vec<_> = instances
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

#[cfg(test)]
#[path = "drift_tests.rs"]
mod drift_tests;
