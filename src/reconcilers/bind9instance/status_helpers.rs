// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Status calculation and update helpers for `Bind9Instance` resources.
//!
//! This module handles computing instance status from deployment/pod health and
//! patching the instance status in Kubernetes.

#[allow(clippy::wildcard_imports)]
use super::types::*;
use crate::reconcilers::pagination::list_all_paginated;

/// Update instance status from deployment pod health.
///
/// Queries the Deployment and its Pods to determine readiness, then updates
/// the instance status with detailed per-pod conditions.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Instance namespace
/// * `name` - Instance name
/// * `instance` - The `Bind9Instance` resource
/// * `cluster_ref` - Optional cluster reference to include in status
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or status patching fails.
#[allow(clippy::too_many_lines)]
pub(super) async fn update_status_from_deployment(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    cluster_ref: Option<ClusterReference>,
) -> Result<()> {
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);

    match deploy_api.get(name).await {
        Ok(deployment) => {
            let actual_replicas = deployment
                .spec
                .as_ref()
                .and_then(|spec| spec.replicas)
                .unwrap_or(0);

            // List pods for this deployment using label selector
            // Use the standard Kubernetes label for instance matching
            let label_selector = format!("{}={}", crate::labels::K8S_INSTANCE, name);
            let list_params = ListParams::default().labels(&label_selector);
            let all_pods = list_all_paginated(&pod_api, list_params).await?;

            // Filter to only non-terminating pods (exclude pods with deletionTimestamp)
            // This prevents counting old pods during rollouts
            let pods: Vec<_> = all_pods
                .into_iter()
                .filter(|pod| pod.metadata.deletion_timestamp.is_none())
                .collect();

            // Create pod-level conditions
            let mut pod_conditions = Vec::new();
            let mut ready_pod_count = 0;

            for (index, pod) in pods.iter().enumerate() {
                let pod_name = pod.metadata.name.as_deref().unwrap_or("unknown");
                // Using map_or for explicit false default on None - more readable than is_some_and
                #[allow(clippy::unnecessary_map_or)]
                let is_pod_ready = pod
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.as_ref())
                    .map_or(false, |conditions| {
                        conditions
                            .iter()
                            .any(|c| c.type_ == "Ready" && c.status == "True")
                    });

                if is_pod_ready {
                    ready_pod_count += 1;
                }

                let (status, reason, message) = if is_pod_ready {
                    ("True", REASON_READY, format!("Pod {pod_name} is ready"))
                } else {
                    (
                        "False",
                        REASON_NOT_READY,
                        format!("Pod {pod_name} is not ready"),
                    )
                };

                pod_conditions.push(Condition {
                    r#type: pod_condition_type(index),
                    status: status.to_string(),
                    reason: Some(reason.to_string()),
                    message: Some(message),
                    last_transition_time: Some(Utc::now().to_rfc3339()),
                });
            }

            // Create encompassing Ready condition
            let (encompassing_status, encompassing_reason, encompassing_message) =
                if ready_pod_count == 0 && actual_replicas > 0 {
                    (
                        "False",
                        REASON_NOT_READY,
                        "Waiting for pods to become ready".to_string(),
                    )
                } else if ready_pod_count == actual_replicas && actual_replicas > 0 {
                    (
                        "True",
                        REASON_ALL_READY,
                        format!("All {ready_pod_count} pods are ready"),
                    )
                } else if ready_pod_count > 0 {
                    (
                        "False",
                        REASON_PARTIALLY_READY,
                        format!("{ready_pod_count}/{actual_replicas} pods are ready"),
                    )
                } else {
                    ("False", REASON_NOT_READY, "No pods are ready".to_string())
                };

            let encompassing_condition = Condition {
                r#type: CONDITION_TYPE_READY.to_string(),
                status: encompassing_status.to_string(),
                reason: Some(encompassing_reason.to_string()),
                message: Some(encompassing_message),
                last_transition_time: Some(Utc::now().to_rfc3339()),
            };

            // Combine encompassing condition + pod-level conditions
            let mut all_conditions = vec![encompassing_condition];
            all_conditions.extend(pod_conditions);

            // Update status with all conditions
            update_status(client, instance, all_conditions, cluster_ref).await?;
        }
        Err(e) => {
            warn!(
                "Failed to get Deployment status for {}/{}: {}",
                namespace, name, e
            );
            // Set status as unknown if we can't check deployment
            let unknown_condition = Condition {
                r#type: CONDITION_TYPE_READY.to_string(),
                status: "Unknown".to_string(),
                reason: Some(REASON_NOT_READY.to_string()),
                message: Some("Unable to determine deployment status".to_string()),
                last_transition_time: Some(Utc::now().to_rfc3339()),
            };
            update_status(client, instance, vec![unknown_condition], cluster_ref).await?;
        }
    }

    Ok(())
}

/// Update the status of a `Bind9Instance` with multiple conditions.
///
/// NOTE: This function does NOT update `status.zones`. Zone reconciliation is handled
/// separately by `reconcile_instance_zones()` which is called:
/// 1. From the main reconcile loop after deployment changes
/// 2. From the `DNSZone` watcher when zone selections change
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance` - The instance to update
/// * `conditions` - Vector of status conditions to set
/// * `cluster_ref` - Optional cluster reference
///
/// # Errors
///
/// Returns an error if status patching fails.
pub(super) async fn update_status(
    client: &Client,
    instance: &Bind9Instance,
    conditions: Vec<Condition>,
    cluster_ref: Option<ClusterReference>,
) -> Result<()> {
    let api: Api<Bind9Instance> =
        Api::namespaced(client.clone(), &instance.namespace().unwrap_or_default());

    // Preserve existing zones - zone reconciliation is handled separately
    let zones = instance
        .status
        .as_ref()
        .map(|s| s.zones.clone())
        .unwrap_or_default();

    // Compute zones_count from zones length
    let zones_count = i32::try_from(zones.len()).ok();

    // Check if status has actually changed (now including zones)
    let current_status = &instance.status;
    let status_changed =
        if let Some(current) = current_status {
            // Check if cluster_ref or zones changed
            if current.cluster_ref != cluster_ref || current.zones != zones {
                true
            } else {
                // Check if any condition changed
                if current.conditions.len() == conditions.len() {
                    // Compare each condition
                    current.conditions.iter().zip(conditions.iter()).any(
                        |(current_cond, new_cond)| {
                            current_cond.r#type != new_cond.r#type
                                || current_cond.status != new_cond.status
                                || current_cond.message != new_cond.message
                                || current_cond.reason != new_cond.reason
                        },
                    )
                } else {
                    true
                }
            }
        } else {
            // No status exists, need to update
            true
        };

    // Only update if status has changed
    if !status_changed {
        debug!(
            "Status unchanged for Bind9Instance {}/{}, skipping patch",
            instance.namespace().unwrap_or_default(),
            instance.name_any()
        );
        return Ok(());
    }

    let new_status = Bind9InstanceStatus {
        conditions,
        observed_generation: instance.metadata.generation,
        service_address: None, // Will be populated when service is ready
        cluster_ref,
        zones,
        zones_count,
        rndc_key_rotation: None, // Will be populated by rotation reconciler
    };

    let patch = json!({ "status": new_status });
    api.patch_status(
        &instance.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
#[path = "status_helpers_tests.rs"]
mod status_helpers_tests;
