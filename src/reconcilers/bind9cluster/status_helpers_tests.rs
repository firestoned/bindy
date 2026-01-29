// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `status_helpers.rs`
//!
//! These tests validate status calculation logic for `Bind9Cluster` resources.
//! Some tests require Kubernetes API mocking, while pure functions are fully tested.

#[cfg(test)]
mod tests {
    use crate::crd::{
        Bind9Instance, Bind9InstanceSpec, Bind9InstanceStatus, Condition, ServerRole,
    };
    use crate::reconcilers::bind9cluster::status_helpers::calculate_cluster_status;
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

    #[tokio::test]
    async fn test_update_status_skips_when_unchanged() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A cluster with current status matching new status
        //        AND instance_count, ready_instances, instances list unchanged
        //        AND all conditions unchanged
        // When: update_status is called
        // Then: Should NOT patch the status
        //       AND log "Status unchanged, skipping update"
    }

    #[tokio::test]
    async fn test_update_status_patches_when_changed() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A cluster with current status ready_instances=2
        //        AND new status ready_instances=3
        // When: update_status is called
        // Then: Should patch the status with new values
        //       AND log "Updating Bind9Cluster status: 3 instances, 3 ready"
    }
}
