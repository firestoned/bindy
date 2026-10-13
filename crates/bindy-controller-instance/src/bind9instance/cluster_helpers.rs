// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Cluster integration helpers for `Bind9Instance` resources.
//!
//! This module handles fetching cluster information and building cluster
//! references for instances.

#[allow(clippy::wildcard_imports)]
use super::types::*;

use crate::constants::API_GROUP_VERSION;
use bindy_controller_sdk::kube_errors::is_not_found_anyhow;
use bindy_controller_sdk::retry::retry_api_call;

/// Classify the result of reading an instance's parent cluster or provider.
///
/// Only "not found" (HTTP 404, also through `anyhow` context) means the
/// instance has no such parent. Every other error (an expired token, RBAC,
/// throttling, a 5xx, retries exhausted) is returned: treating it as "no
/// parent" would build the Deployment without the cluster's image, version,
/// RNDC key and volumes and roll the pods onto that wrong spec.
///
/// # Returns
///
/// `Ok(Some(parent))` when it was read, `Ok(None)` when it does not exist.
///
/// # Errors
///
/// Returns every error other than not found.
pub(super) fn parent_or_none<T>(result: anyhow::Result<T>) -> anyhow::Result<Option<T>> {
    match result {
        Ok(parent) => Ok(Some(parent)),
        Err(err) if is_not_found_anyhow(&err) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Read the parent `Bind9Cluster` or `ClusterBind9Provider` of an instance.
///
/// The owner references are tried first, then `spec.clusterRef` (instances
/// created by hand). A parent that does not exist (404) is skipped.
///
/// # Errors
///
/// Returns an error when a parent cannot be read for any reason other than
/// not found ([`parent_or_none`]), so the reconcile retries instead of
/// running without the parent's configuration.
pub(super) async fn fetch_cluster_info(
    client: &Client,
    namespace: &str,
    instance: &Bind9Instance,
) -> anyhow::Result<(
    Option<Bind9Cluster>,
    Option<crate::crd::ClusterBind9Provider>,
)> {
    // First, try to get cluster from ownerReferences (preferred)
    if let Some(owner_refs) = &instance.metadata.owner_references {
        for owner_ref in owner_refs {
            if owner_ref.api_version != API_GROUP_VERSION {
                continue;
            }
            // Check if owner is a Bind9Cluster
            if owner_ref.kind == "Bind9Cluster" {
                let cluster_api: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
                let cluster = parent_or_none(
                    retry_api_call(|| cluster_api.get(&owner_ref.name), "get Bind9Cluster").await,
                )?;
                if let Some(cluster) = cluster {
                    debug!(
                        "Found cluster from ownerReference: Bind9Cluster/{}",
                        owner_ref.name
                    );
                    return Ok((Some(cluster), None));
                }
            }
            // Check if owner is a ClusterBind9Provider
            else if owner_ref.kind == "ClusterBind9Provider" {
                let provider_api: Api<crate::crd::ClusterBind9Provider> = Api::all(client.clone());
                let provider = parent_or_none(
                    retry_api_call(
                        || provider_api.get(&owner_ref.name),
                        "get ClusterBind9Provider",
                    )
                    .await,
                )?;
                if let Some(provider) = provider {
                    debug!(
                        "Found cluster from ownerReference: ClusterBind9Provider/{}",
                        owner_ref.name
                    );
                    return Ok((None, Some(provider)));
                }
            }
        }
    }

    // Fallback: Use spec.clusterRef for backward compatibility with manually-created instances
    if instance.spec.cluster_ref.is_empty() {
        debug!("Instance has neither ownerReferences nor spec.clusterRef");
        return Ok((None, None));
    }
    debug!(
        "No ownerReference found, falling back to spec.clusterRef: {}",
        instance.spec.cluster_ref
    );

    // Try namespace-scoped cluster first
    let cluster_api: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
    let cluster = parent_or_none(
        retry_api_call(
            || cluster_api.get(&instance.spec.cluster_ref),
            "get Bind9Cluster",
        )
        .await,
    )?;
    if cluster.is_some() {
        return Ok((cluster, None));
    }

    // If not found, try cluster-scoped provider
    let provider_api: Api<crate::crd::ClusterBind9Provider> = Api::all(client.clone());
    let cluster_provider = parent_or_none(
        retry_api_call(
            || provider_api.get(&instance.spec.cluster_ref),
            "get ClusterBind9Provider",
        )
        .await,
    )?;
    Ok((None, cluster_provider))
}

/// Creates a `ClusterReference` from either a `Bind9Cluster` or `ClusterBind9Provider`.
///
/// This helper function creates a full Kubernetes object reference with kind, apiVersion,
/// name, and namespace (for namespace-scoped clusters). This enables proper object
/// references and provides backward compatibility with `spec.clusterRef` (string name).
///
/// # Arguments
///
/// * `cluster` - Optional namespace-scoped `Bind9Cluster`
/// * `cluster_provider` - Optional cluster-scoped `ClusterBind9Provider`
///
/// # Returns
///
/// * `Some(ClusterReference)` - If either cluster or `cluster_provider` is provided
/// * `None` - If neither is provided
pub(super) fn build_cluster_reference(
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Option<ClusterReference> {
    if let Some(c) = cluster {
        Some(ClusterReference {
            api_version: API_GROUP_VERSION.to_string(),
            kind: "Bind9Cluster".to_string(),
            name: c.name_any(),
            namespace: c.namespace(),
        })
    } else {
        cluster_provider.map(|cp| ClusterReference {
            api_version: API_GROUP_VERSION.to_string(),
            kind: "ClusterBind9Provider".to_string(),
            name: cp.name_any(),
            namespace: None, // Cluster-scoped resource has no namespace
        })
    }
}

#[cfg(test)]
#[path = "cluster_helpers_tests.rs"]
mod cluster_helpers_tests;
