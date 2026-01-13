// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Cluster integration helpers for `Bind9Instance` resources.
//!
//! This module handles fetching cluster information and building cluster
//! references for instances.

#[allow(clippy::wildcard_imports)]
use super::types::*;

use crate::constants::API_GROUP_VERSION;

pub(super) async fn fetch_cluster_info(
    client: &Client,
    namespace: &str,
    instance: &Bind9Instance,
) -> (
    Option<Bind9Cluster>,
    Option<crate::crd::ClusterBind9Provider>,
) {
    // First, try to get cluster from ownerReferences (preferred)
    if let Some(owner_refs) = &instance.metadata.owner_references {
        for owner_ref in owner_refs {
            // Check if owner is a Bind9Cluster
            if owner_ref.kind == "Bind9Cluster" && owner_ref.api_version == API_GROUP_VERSION {
                let cluster_api: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
                if let Ok(cluster) = cluster_api.get(&owner_ref.name).await {
                    debug!(
                        "Found cluster from ownerReference: Bind9Cluster/{}",
                        owner_ref.name
                    );
                    return (Some(cluster), None);
                }
            }
            // Check if owner is a ClusterBind9Provider
            else if owner_ref.kind == "ClusterBind9Provider"
                && owner_ref.api_version == API_GROUP_VERSION
            {
                let provider_api: Api<crate::crd::ClusterBind9Provider> = Api::all(client.clone());
                if let Ok(provider) = provider_api.get(&owner_ref.name).await {
                    debug!(
                        "Found cluster from ownerReference: ClusterBind9Provider/{}",
                        owner_ref.name
                    );
                    return (None, Some(provider));
                }
            }
        }
    }

    // Fallback: Use spec.clusterRef for backward compatibility with manually-created instances
    if !instance.spec.cluster_ref.is_empty() {
        debug!(
            "No ownerReference found, falling back to spec.clusterRef: {}",
            instance.spec.cluster_ref
        );

        // Try namespace-scoped cluster first
        let cluster_api: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
        let cluster = cluster_api.get(&instance.spec.cluster_ref).await.ok();

        // If not found, try cluster-scoped provider
        let cluster_provider = if cluster.is_none() {
            let provider_api: Api<crate::crd::ClusterBind9Provider> = Api::all(client.clone());
            provider_api.get(&instance.spec.cluster_ref).await.ok()
        } else {
            None
        };

        return (cluster, cluster_provider);
    }

    debug!("Instance has neither ownerReferences nor spec.clusterRef");
    (None, None)
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
