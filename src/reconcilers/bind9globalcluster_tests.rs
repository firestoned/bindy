// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `bind9globalcluster.rs`

#[cfg(test)]
mod tests {
    use super::super::bind9globalcluster::calculate_cluster_status;
    use crate::crd::{
        Bind9Instance, Bind9InstanceSpec, Bind9InstanceStatus, Condition, ServerRole,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn test_calculate_cluster_status_no_instances() {
        let instances = vec![];
        let status = calculate_cluster_status(&instances, Some(1));

        assert_eq!(status.observed_generation, Some(1));
        assert_eq!(status.instances.len(), 0);
        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.conditions[0].r#type, "Ready");
        assert_eq!(status.conditions[0].status, "False");
        assert_eq!(status.conditions[0].reason, Some("NoInstances".to_string()));
    }

    #[test]
    fn test_calculate_cluster_status_all_ready() {
        let instances = vec![
            create_test_instance("primary-0", "test-ns", true),
            create_test_instance("secondary-0", "test-ns", true),
        ];

        let status = calculate_cluster_status(&instances, Some(2));

        assert_eq!(status.observed_generation, Some(2));
        assert_eq!(status.instances.len(), 2);
        assert!(status.instances.contains(&"test-ns/primary-0".to_string()));
        assert!(status
            .instances
            .contains(&"test-ns/secondary-0".to_string()));

        assert_eq!(status.conditions[0].r#type, "Ready");
        assert_eq!(status.conditions[0].status, "True");
        assert_eq!(status.conditions[0].reason, Some("AllReady".to_string()));
    }

    #[test]
    fn test_calculate_cluster_status_partially_ready() {
        let instances = vec![
            create_test_instance("primary-0", "test-ns", true),
            create_test_instance("secondary-0", "test-ns", false),
        ];

        let status = calculate_cluster_status(&instances, Some(3));

        assert_eq!(status.conditions[0].status, "False");
        assert_eq!(
            status.conditions[0].reason,
            Some("PartiallyReady".to_string())
        );
        assert!(status.conditions[0]
            .message
            .as_ref()
            .unwrap()
            .contains("1/2"));
    }

    #[test]
    fn test_calculate_cluster_status_none_ready() {
        let instances = vec![
            create_test_instance("primary-0", "test-ns", false),
            create_test_instance("secondary-0", "test-ns", false),
        ];

        let status = calculate_cluster_status(&instances, Some(4));

        assert_eq!(status.conditions[0].status, "False");
        assert_eq!(status.conditions[0].reason, Some("NotReady".to_string()));
    }

    #[test]
    fn test_calculate_cluster_status_cross_namespace() {
        // Test that instances from different namespaces are properly tracked
        let instances = vec![
            create_test_instance("primary-0", "namespace-a", true),
            create_test_instance("secondary-0", "namespace-b", true),
            create_test_instance("secondary-1", "namespace-c", true),
        ];

        let status = calculate_cluster_status(&instances, Some(5));

        assert_eq!(status.instances.len(), 3);
        assert!(status
            .instances
            .contains(&"namespace-a/primary-0".to_string()));
        assert!(status
            .instances
            .contains(&"namespace-b/secondary-0".to_string()));
        assert!(status
            .instances
            .contains(&"namespace-c/secondary-1".to_string()));

        assert_eq!(status.conditions[0].status, "True");
        assert_eq!(status.conditions[0].reason, Some("AllReady".to_string()));
    }

    fn create_test_instance(name: &str, namespace: &str, ready: bool) -> Bind9Instance {
        let status = if ready {
            Some(Bind9InstanceStatus {
                conditions: vec![Condition {
                    r#type: "Ready".to_string(),
                    status: "True".to_string(),
                    reason: Some("DeploymentReady".to_string()),
                    message: Some("Instance is ready".to_string()),
                    last_transition_time: None,
                }],
                observed_generation: Some(1),
                replicas: Some(1),
                ready_replicas: Some(1),
                service_address: Some("127.0.0.1".to_string()),
            })
        } else {
            Some(Bind9InstanceStatus {
                conditions: vec![Condition {
                    r#type: "Ready".to_string(),
                    status: "False".to_string(),
                    reason: Some("DeploymentNotReady".to_string()),
                    message: Some("Instance is not ready".to_string()),
                    last_transition_time: None,
                }],
                observed_generation: Some(1),
                replicas: Some(1),
                ready_replicas: Some(0),
                service_address: None,
            })
        };

        Bind9Instance {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-global-cluster".to_string(),
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
                storage: None,
                bindcar_config: None,
            },
            status,
        }
    }
}
