// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `status_helpers.rs`
//!
//! These tests validate status calculation logic for `Bind9Cluster` resources.
//! Some tests require Kubernetes API mocking, while pure functions are fully tested.

#[cfg(test)]
mod tests {
    use crate::bind9cluster::status_helpers::calculate_cluster_status;
    use crate::crd::{
        Bind9Instance, Bind9InstanceSpec, Bind9InstanceStatus, Condition, ServerRole,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    /// Helper to create a test `Bind9Instance`
    fn create_test_instance(name: &str, namespace: &str, is_ready: bool) -> Bind9Instance {
        let status = if is_ready {
            Some(Bind9InstanceStatus {
                conditions: vec![Condition {
                    r#type: "Ready".to_string(),
                    status: "True".to_string(),
                    reason: Some("DeploymentReady".to_string()),
                    message: Some("All pods are ready".to_string()),
                    last_transition_time: Some("2025-01-01T00:00:00Z".to_string()),
                }],
                ..Default::default()
            })
        } else {
            Some(Bind9InstanceStatus {
                conditions: vec![Condition {
                    r#type: "Ready".to_string(),
                    status: "False".to_string(),
                    reason: Some("DeploymentNotReady".to_string()),
                    message: Some("Pods are not ready".to_string()),
                    last_transition_time: Some("2025-01-01T00:00:00Z".to_string()),
                }],
                ..Default::default()
            })
        };

        #[allow(deprecated)]
        Bind9Instance {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                placement: None,
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: Some(1),
                version: None,
                image: None,
                config_map_refs: None,
                config: None,
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
                rndc_secret_ref: None,
                rndc_key: None,
                storage: None,
                bindcar_config: None,
            },
            status,
        }
    }

    #[test]
    fn test_calculate_cluster_status_no_instances() {
        let instances = vec![];
        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");

        assert_eq!(instance_count, 0);
        assert_eq!(ready_instances, 0);
        assert_eq!(instance_names.len(), 0);

        // Should have 1 encompassing Ready condition
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].r#type, "Ready");
        assert_eq!(conditions[0].status, "False");
        assert!(conditions[0]
            .message
            .as_ref()
            .unwrap()
            .contains("No instances found"));
    }

    #[test]
    fn test_calculate_cluster_status_all_ready() {
        let instances = vec![
            create_test_instance("instance-0", "default", true),
            create_test_instance("instance-1", "default", true),
            create_test_instance("instance-2", "default", true),
        ];

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");

        assert_eq!(instance_count, 3);
        assert_eq!(ready_instances, 3);
        assert_eq!(instance_names.len(), 3);

        // Should have 1 encompassing condition + 3 instance conditions = 4 total
        assert_eq!(conditions.len(), 4);

        // Check encompassing condition
        assert_eq!(conditions[0].r#type, "Ready");
        assert_eq!(conditions[0].status, "True");
        assert!(conditions[0]
            .message
            .as_ref()
            .unwrap()
            .contains("All 3 instances are ready"));
    }

    #[test]
    fn test_calculate_cluster_status_partially_ready() {
        let instances = vec![
            create_test_instance("instance-0", "default", true),
            create_test_instance("instance-1", "default", false),
            create_test_instance("instance-2", "default", true),
        ];

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");

        assert_eq!(instance_count, 3);
        assert_eq!(ready_instances, 2);
        assert_eq!(instance_names.len(), 3);

        // Check encompassing condition
        assert_eq!(conditions[0].r#type, "Ready");
        assert_eq!(conditions[0].status, "False");
        assert!(conditions[0]
            .message
            .as_ref()
            .unwrap()
            .contains("2/3 instances are ready"));

        // Check instance-level conditions
        assert_eq!(conditions[1].r#type, "Bind9Instance-0");
        assert_eq!(conditions[1].status, "True");

        assert_eq!(conditions[2].r#type, "Bind9Instance-1");
        assert_eq!(conditions[2].status, "False");

        assert_eq!(conditions[3].r#type, "Bind9Instance-2");
        assert_eq!(conditions[3].status, "True");
    }

    #[test]
    fn test_calculate_cluster_status_none_ready() {
        let instances = vec![
            create_test_instance("instance-0", "default", false),
            create_test_instance("instance-1", "default", false),
        ];

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");

        assert_eq!(instance_count, 2);
        assert_eq!(ready_instances, 0);
        assert_eq!(instance_names.len(), 2);

        // Check encompassing condition
        assert_eq!(conditions[0].r#type, "Ready");
        assert_eq!(conditions[0].status, "False");
        assert!(conditions[0]
            .message
            .as_ref()
            .unwrap()
            .contains("No instances are ready"));
    }

    // ========================================================================
    // Status patch decision (cluster_status_changed)
    // ========================================================================

    use crate::bind9cluster::status_helpers::cluster_status_changed;
    use crate::crd::Bind9ClusterStatus;

    fn build_current_status(generation: Option<i64>) -> Bind9ClusterStatus {
        let instances = vec![create_test_instance("instance-0", "default", true)];
        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");
        Bind9ClusterStatus {
            conditions,
            observed_generation: generation,
            instance_count: Some(instance_count),
            ready_instances: Some(ready_instances),
            instances: instance_names,
        }
    }

    #[test]
    fn test_cluster_status_changed_no_current_status() {
        let instances = vec![create_test_instance("instance-0", "default", true)];
        let (count, ready, names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");

        assert!(cluster_status_changed(
            None,
            &conditions,
            count,
            ready,
            &names,
            Some(1)
        ));
    }

    #[test]
    fn test_cluster_status_changed_generation_only_change_triggers_patch() {
        // THE BUG: a spec edit that does not change counts or conditions must
        // still trigger the patch so observedGeneration advances - otherwise
        // should_reconcile() returns true on every requeue forever.
        let current = build_current_status(Some(1));
        let instances = vec![create_test_instance("instance-0", "default", true)];
        let (count, ready, names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");

        assert!(
            cluster_status_changed(Some(&current), &conditions, count, ready, &names, Some(2)),
            "generation-only change must trigger a status patch"
        );
    }

    #[test]
    fn test_cluster_status_changed_unchanged_skips_patch() {
        let current = build_current_status(Some(2));
        let instances = vec![create_test_instance("instance-0", "default", true)];
        let (count, ready, names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");

        assert!(!cluster_status_changed(
            Some(&current),
            &conditions,
            count,
            ready,
            &names,
            Some(2)
        ));
    }

    #[test]
    fn test_cluster_status_changed_condition_change_triggers_patch() {
        let current = build_current_status(Some(2));
        let instances = vec![create_test_instance("instance-0", "default", false)];
        let (count, ready, names, conditions) =
            calculate_cluster_status(&instances, "default", "test-cluster");

        assert!(cluster_status_changed(
            Some(&current),
            &conditions,
            count,
            ready,
            &names,
            Some(2)
        ));
    }

    // ========================================================================
    // Per-instance readiness and order-insensitive comparison
    // ========================================================================

    fn condition(type_: &str, status: &str) -> Condition {
        Condition {
            r#type: type_.to_string(),
            status: status.to_string(),
            reason: Some("Reason".to_string()),
            message: Some(format!("{type_} is {status}")),
            last_transition_time: None,
        }
    }

    #[test]
    fn test_calculate_cluster_status_counts_ready_condition_not_first() {
        // THE BUG: an instance counted as ready only when its FIRST condition
        // was Ready=True. Readiness is looked up by type.
        let mut instance = create_test_instance("instance-0", "default", true);
        if let Some(status) = instance.status.as_mut() {
            status.conditions.insert(0, condition("Pod-0", "True"));
        }

        let (count, ready, _names, conditions) =
            calculate_cluster_status(&[instance], "default", "test-cluster");

        assert_eq!(count, 1);
        assert_eq!(ready, 1, "Ready=True in second place still counts");
        assert_eq!(conditions[0].status, "True");
        assert_eq!(conditions[1].status, "True", "per-instance condition");
    }

    #[test]
    fn test_cluster_status_changed_ignores_condition_order() {
        // Behaviour change: conditions are matched by type, so the same set
        // in another order is no change.
        let mut current = build_current_status(Some(2));
        current.conditions = vec![condition("Ready", "True"), condition("Degraded", "False")];
        let reordered = vec![condition("Degraded", "False"), condition("Ready", "True")];

        assert!(!cluster_status_changed(
            Some(&current),
            &reordered,
            current.instance_count.unwrap_or_default(),
            current.ready_instances.unwrap_or_default(),
            &current.instances.clone(),
            Some(2)
        ));
    }
}
