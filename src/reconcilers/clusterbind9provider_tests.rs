// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `clusterbind9provider.rs`

#[cfg(test)]
mod tests {
    use super::super::clusterbind9provider::calculate_cluster_status;
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
        assert_eq!(status.conditions[0].reason, Some("NoChildren".to_string()));
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
                observed_parent_generation: None,
                service_address: Some("127.0.0.1".to_string()),
                cluster_ref: None,
                zones: Vec::new(),
                zones_count: None,
                rndc_key_rotation: None,
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
                observed_parent_generation: None,
                service_address: None,
                cluster_ref: None,
                zones: Vec::new(),
                zones_count: None,
                rndc_key_rotation: None,
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
                rndc_key: None,
                storage: None,
                bindcar_config: None,
            },
            status,
        }
    }

    // ========================================================================
    // Status patch decision (cluster_status_needs_update)
    // ========================================================================

    use super::super::clusterbind9provider::{
        cluster_status_needs_update, expected_cluster_namespaces,
    };

    #[test]
    fn test_cluster_status_needs_update_no_current_status() {
        let instances = vec![create_test_instance("primary-0", "test-ns", true)];
        let new_status = calculate_cluster_status(&instances, Some(1));

        assert!(cluster_status_needs_update(None, &new_status));
    }

    #[test]
    fn test_cluster_status_needs_update_generation_only_change_triggers_patch() {
        // THE BUG: a spec edit that does not change counts or conditions bumps
        // metadata.generation only. The patch must still be applied so
        // observedGeneration advances - otherwise should_reconcile() returns
        // true on every requeue forever.
        let instances = vec![create_test_instance("primary-0", "test-ns", true)];
        let current = calculate_cluster_status(&instances, Some(1));
        let new_status = calculate_cluster_status(&instances, Some(2));

        assert!(
            cluster_status_needs_update(Some(&current), &new_status),
            "generation-only change must trigger a status patch"
        );
    }

    #[test]
    fn test_cluster_status_needs_update_unchanged_skips_patch() {
        let instances = vec![create_test_instance("primary-0", "test-ns", true)];
        let current = calculate_cluster_status(&instances, Some(2));
        let new_status = calculate_cluster_status(&instances, Some(2));

        assert!(!cluster_status_needs_update(Some(&current), &new_status));
    }

    #[test]
    fn test_cluster_status_needs_update_count_change_triggers_patch() {
        let one = vec![create_test_instance("primary-0", "test-ns", true)];
        let two = vec![
            create_test_instance("primary-0", "test-ns", true),
            create_test_instance("primary-1", "test-ns", true),
        ];
        let current = calculate_cluster_status(&one, Some(2));
        let new_status = calculate_cluster_status(&two, Some(2));

        assert!(cluster_status_needs_update(Some(&current), &new_status));
    }

    // ========================================================================
    // Expected namespace set shared by reconcile + drift detection
    // ========================================================================

    #[test]
    fn test_expected_cluster_namespaces_falls_back_to_target_namespace() {
        // No instance references the provider: the managed cluster is expected
        // in the target namespace only
        let instances = vec![];
        let namespaces = expected_cluster_namespaces(&instances, "my-provider", "bindy-system");

        assert_eq!(namespaces.len(), 1);
        assert!(namespaces.contains("bindy-system"));
    }

    #[test]
    fn test_expected_cluster_namespaces_uses_instance_namespaces() {
        // THE BUG: drift detection previously only inspected the target
        // namespace. Instances in ns1/ns2 mean managed clusters are created in
        // ns1/ns2 - the expected set must be exactly those namespaces.
        let mut inst_a = create_test_instance("primary-0", "ns1", true);
        inst_a.spec.cluster_ref = "my-provider".to_string();
        let mut inst_b = create_test_instance("secondary-0", "ns2", true);
        inst_b.spec.cluster_ref = "my-provider".to_string();
        // An instance referencing a DIFFERENT provider must not contribute
        let mut other = create_test_instance("other-0", "ns3", true);
        other.spec.cluster_ref = "other-provider".to_string();

        let instances = vec![inst_a, inst_b, other];
        let namespaces = expected_cluster_namespaces(&instances, "my-provider", "bindy-system");

        assert_eq!(namespaces.len(), 2);
        assert!(namespaces.contains("ns1"));
        assert!(namespaces.contains("ns2"));
        assert!(!namespaces.contains("ns3"));
        assert!(!namespaces.contains("bindy-system"));
    }
}
