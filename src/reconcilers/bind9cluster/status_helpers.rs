// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Status calculation and update helpers for `Bind9Cluster` resources.
//!
//! This module handles computing cluster status from instance health and
//! patching the cluster status in Kubernetes.

#[allow(clippy::wildcard_imports)]
use super::types::*;

/// Calculate cluster status from instance health.
///
/// Analyzes the list of instances to determine cluster readiness.
/// Creates both an encompassing `Ready` condition and individual conditions
/// for each instance.
///
/// # Arguments
///
/// * `instances` - List of `Bind9Instance` resources for the cluster
/// * `namespace` - Cluster namespace (for logging)
/// * `name` - Cluster name (for logging)
///
/// # Returns
///
/// Tuple of:
/// - `instance_count` - Total number of instances
/// - `ready_instances` - Number of ready instances
/// - `instance_names` - Names of all instances
/// - `conditions` - Vector of status conditions (Ready + per-instance)
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn calculate_cluster_status(
    instances: &[Bind9Instance],
    namespace: &str,
    name: &str,
) -> (i32, i32, Vec<String>, Vec<Condition>) {
    // Count total instances and ready instances
    let instance_count = instances.len() as i32;
    let instance_names: Vec<String> = instances.iter().map(ResourceExt::name_any).collect();

    let ready_instances = instances
        .iter()
        .filter(|instance| {
            instance
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True")
        })
        .count() as i32;

    info!(
        "Bind9Cluster {}/{} has {} instances, {} ready",
        namespace, name, instance_count, ready_instances
    );

    // Create instance-level conditions
    let mut instance_conditions = Vec::new();
    for (index, instance) in instances.iter().enumerate() {
        let instance_name = instance.name_any();
        let is_instance_ready = instance
            .status
            .as_ref()
            .and_then(|status| status.conditions.first())
            .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

        let (status, reason, message) = if is_instance_ready {
            (
                "True",
                REASON_READY,
                format!("Instance {instance_name} is ready"),
            )
        } else {
            (
                "False",
                REASON_NOT_READY,
                format!("Instance {instance_name} is not ready"),
            )
        };

        instance_conditions.push(Condition {
            r#type: bind9_instance_condition_type(index),
            status: status.to_string(),
            reason: Some(reason.to_string()),
            message: Some(message),
            last_transition_time: Some(Utc::now().to_rfc3339()),
        });
    }

    // Create encompassing Ready condition
    let (encompassing_status, encompassing_reason, encompassing_message) = if instance_count == 0 {
        debug!("No instances found for cluster");
        (
            "False",
            REASON_NO_CHILDREN,
            "No instances found for this cluster".to_string(),
        )
    } else if ready_instances == instance_count {
        debug!("All instances ready");
        (
            "True",
            REASON_ALL_READY,
            format!("All {instance_count} instances are ready"),
        )
    } else if ready_instances > 0 {
        debug!(ready_instances, instance_count, "Cluster progressing");
        (
            "False",
            REASON_PARTIALLY_READY,
            format!("{ready_instances}/{instance_count} instances are ready"),
        )
    } else {
        debug!("Waiting for instances to become ready");
        (
            "False",
            REASON_NOT_READY,
            "No instances are ready".to_string(),
        )
    };

    let encompassing_condition = Condition {
        r#type: CONDITION_TYPE_READY.to_string(),
        status: encompassing_status.to_string(),
        reason: Some(encompassing_reason.to_string()),
        message: Some(encompassing_message.clone()),
        last_transition_time: Some(Utc::now().to_rfc3339()),
    };

    // Combine encompassing condition + instance-level conditions
    let mut all_conditions = vec![encompassing_condition];
    all_conditions.extend(instance_conditions);

    debug!(
        status = %encompassing_status,
        message = %encompassing_message,
        num_conditions = all_conditions.len(),
        "Determined cluster status"
    );

    (
        instance_count,
        ready_instances,
        instance_names,
        all_conditions,
    )
}

/// Determines whether the `Bind9Cluster` status patch is needed.
///
/// Compares the current status against the values about to be written. The
/// patch is needed if any of the following changed:
/// - `instance_count`, `ready_instances`, or the instance name list
/// - `observed_generation` (so spec edits that don't change counts/conditions
///   still advance `observedGeneration` and stop perpetual re-reconciliation)
/// - Any condition's type, status, reason, or message
///
/// # Arguments
///
/// * `current` - The status currently stored on the resource (if any)
/// * `conditions` - New conditions about to be written
/// * `instance_count` - New total instance count
/// * `ready_instances` - New ready instance count
/// * `instances` - New instance name list
/// * `generation` - The `metadata.generation` about to be written as `observed_generation`
///
/// # Returns
///
/// `true` if the status patch should be applied, `false` if it can be skipped.
#[must_use]
pub fn cluster_status_changed(
    current: Option<&Bind9ClusterStatus>,
    conditions: &[Condition],
    instance_count: i32,
    ready_instances: i32,
    instances: &[String],
    generation: Option<i64>,
) -> bool {
    let Some(current) = current else {
        // No status exists, need to update
        return true;
    };

    // Check if counts changed
    if current.instance_count != Some(instance_count)
        || current.ready_instances != Some(ready_instances)
        || current.instances != instances
    {
        return true;
    }

    // Check if the observed generation is behind the generation about to be
    // written. Without this, a spec edit that does not change counts or
    // conditions never advances observedGeneration, causing should_reconcile()
    // to return true on every requeue forever.
    if current.observed_generation != generation {
        return true;
    }

    // Check if any condition changed
    if current.conditions.len() != conditions.len() {
        return true;
    }

    current
        .conditions
        .iter()
        .zip(conditions.iter())
        .any(|(current_cond, new_cond)| {
            current_cond.r#type != new_cond.r#type
                || current_cond.status != new_cond.status
                || current_cond.message != new_cond.message
                || current_cond.reason != new_cond.reason
        })
}

/// Update the status of a `Bind9Cluster` with multiple conditions.
///
/// Patches the cluster status in Kubernetes if it has changed.
/// Performs a comparison to avoid unnecessary API calls when status is unchanged.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The `Bind9Cluster` to update
/// * `conditions` - Vector of status conditions to set
/// * `instance_count` - Total number of instances
/// * `ready_instances` - Number of ready instances
/// * `instances` - Names of all instances
///
/// # Errors
///
/// Returns an error if status patching fails.
pub(super) async fn update_status(
    client: &Client,
    cluster: &Bind9Cluster,
    conditions: Vec<Condition>,
    instance_count: i32,
    ready_instances: i32,
    instances: Vec<String>,
) -> Result<()> {
    let api: Api<Bind9Cluster> =
        Api::namespaced(client.clone(), &cluster.namespace().unwrap_or_default());

    // Check if status has actually changed (including observed_generation)
    let status_changed = cluster_status_changed(
        cluster.status.as_ref(),
        &conditions,
        instance_count,
        ready_instances,
        &instances,
        cluster.metadata.generation,
    );

    // Only update if status has changed
    if !status_changed {
        debug!(
            namespace = %cluster.namespace().unwrap_or_default(),
            name = %cluster.name_any(),
            "Status unchanged, skipping update"
        );
        info!(
            "Bind9Cluster {}/{} status unchanged, skipping update",
            cluster.namespace().unwrap_or_default(),
            cluster.name_any()
        );
        return Ok(());
    }

    debug!(
        instance_count,
        ready_instances,
        instances_count = instances.len(),
        num_conditions = conditions.len(),
        "Preparing status update"
    );

    let new_status = Bind9ClusterStatus {
        conditions,
        observed_generation: cluster.metadata.generation,
        instance_count: Some(instance_count),
        ready_instances: Some(ready_instances),
        instances,
    };

    info!(
        "Updating Bind9Cluster {}/{} status: {} instances, {} ready",
        cluster.namespace().unwrap_or_default(),
        cluster.name_any(),
        instance_count,
        ready_instances
    );

    let patch = json!({ "status": new_status });
    api.patch_status(
        &cluster.name_any(),
        &PatchParams::apply("bindy-controller"),
        &Patch::Merge(&patch),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
#[path = "status_helpers_tests.rs"]
mod status_helpers_tests;
