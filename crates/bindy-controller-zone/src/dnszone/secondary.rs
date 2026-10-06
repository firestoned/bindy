// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Secondary zone instance operations.
//!
//! This module handles all operations specific to SECONDARY BIND9 instances,
//! including:
//! - Filtering instance references to only secondary instances
//! - Finding secondary pods across instances
//! - Collecting secondary pod IPs
//! - Executing operations on all secondary endpoints

use anyhow::Result;
use kube::{api::ListParams, Api, Client};
use tracing::{debug, warn};

/// Filters a list of instance references to only SECONDARY instances.
///
/// Roles come from the `Bind9Instance` reflector store, with a GET only for
/// an instance the store does not hold yet (ADR-0015, ADR-0016).
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// Vector of instance references that have role=Secondary
///
/// # Errors
///
/// Does not currently fail; an instance that is neither cached nor readable
/// is skipped with a warning.
pub async fn filter_secondary_instances(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    Ok(bindy_bind9::primary::filter_instances_by_role_cached(
        client,
        store,
        instance_refs,
        &crate::crd::ServerRole::Secondary,
    )
    .await)
}

/// Finds all pod IPs from a list of instance references, filtering by role.
///
/// Reads each instance's role from the `Bind9Instance` store (GET fallback),
/// then lists pod IPs only for secondary instances.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_refs` - Instance references to query
///
/// # Returns
///
/// Vector of pod IP addresses from secondary instances only
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail.
pub async fn find_secondary_pod_ips_from_instances(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<String>> {
    use crate::crd::ServerRole;
    use k8s_openapi::api::core::v1::Pod;

    let mut secondary_ips = Vec::new();

    for instance_ref in instance_refs {
        // The role comes from the Bind9Instance store (ADR-0016)
        let role = bindy_bind9::primary::instance_role_cached(client, store, instance_ref).await;
        if role != Some(ServerRole::Secondary) {
            debug!(
                "Skipping instance {}/{} - role is {:?}, not Secondary",
                instance_ref.namespace, instance_ref.name, role
            );
            continue;
        }

        // Find pods for this secondary instance
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), &instance_ref.namespace);
        let label_selector = format!("app=bind9,instance={}", instance_ref.name);
        let lp = ListParams::default().labels(&label_selector);

        match pod_api.list(&lp).await {
            Ok(pods) => {
                for pod in pods.items {
                    if let Some(pod_ip) = pod.status.as_ref().and_then(|s| s.pod_ip.as_ref()) {
                        // Check if pod is running
                        let phase = pod
                            .status
                            .as_ref()
                            .and_then(|s| s.phase.as_ref())
                            .map_or("Unknown", std::string::String::as_str);

                        if phase == "Running" {
                            secondary_ips.push(pod_ip.clone());
                        } else {
                            debug!(
                                "Skipping pod {} in phase {} for instance {}/{}",
                                pod.metadata.name.as_ref().unwrap_or(&"unknown".to_string()),
                                phase,
                                instance_ref.namespace,
                                instance_ref.name
                            );
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to list pods for instance {}/{}: {}. Skipping.",
                    instance_ref.namespace, instance_ref.name, e
                );
            }
        }
    }

    Ok(secondary_ips)
}

#[cfg(test)]
#[path = "secondary_tests.rs"]
mod secondary_tests;
