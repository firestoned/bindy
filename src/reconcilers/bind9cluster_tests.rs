// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests {
    use crate::crd::{
        Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, Bind9ClusterStatus, Bind9Instance,
        Bind9InstanceSpec, Bind9InstanceStatus, Condition, PrimaryConfig, SecondaryConfig,
        ServerRole,
    };
    use crate::labels::FINALIZER_BIND9_CLUSTER;
    use crate::reconcilers::bind9cluster::calculate_cluster_status;
    use crate::status_reasons::{
        bind9_instance_condition_type, REASON_ALL_READY, REASON_NOT_READY, REASON_NO_CHILDREN,
        REASON_PARTIALLY_READY, REASON_READY,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    /// Helper function to create a test `Bind9Cluster`
    fn create_test_cluster(name: &str, namespace: &str) -> Bind9Cluster {
        Bind9Cluster {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                generation: Some(1),
                finalizers: None,
                deletion_timestamp: None,
                ..Default::default()
            },
            spec: Bind9ClusterSpec {
                common: Bind9ClusterCommonSpec {
                    version: Some("9.18".to_string()),
                    primary: Some(PrimaryConfig {
                        replicas: Some(2),
                        labels: None,
                        service: None,
                        allow_transfer: None,
                        rndc_secret_ref: None,
                    }),
                    secondary: Some(SecondaryConfig {
                        replicas: Some(2),
                        labels: None,
                        service: None,
                        allow_transfer: None,
                        rndc_secret_ref: None,
                    }),
                    image: None,
                    config_map_refs: None,
                    global: None,
                    rndc_secret_refs: None,
                    acls: None,
                    volumes: None,
                    volume_mounts: None,
                },
            },
            status: None,
        }
    }

    /// Helper function to create a test `Bind9Instance`
    fn create_test_instance(
        name: &str,
        namespace: &str,
        cluster_ref: &str,
        role: ServerRole,
        ready: bool,
    ) -> Bind9Instance {
        let status = if ready {
            Some(Bind9InstanceStatus {
                conditions: vec![Condition {
                    r#type: "Ready".to_string(),
                    status: "True".to_string(),
                    reason: Some("Reconciled".to_string()),
                    message: Some("Instance is ready".to_string()),
                    last_transition_time: Some("2025-11-30T00:00:00Z".to_string()),
                }],
                observed_generation: Some(1),
                service_address: Some("10.0.0.1".to_string()),
                cluster_ref: None,
                zones: Vec::new(),
                zones_count: None,
            })
        } else {
            Some(Bind9InstanceStatus {
                conditions: vec![Condition {
                    r#type: "Ready".to_string(),
                    status: "False".to_string(),
                    reason: Some("Progressing".to_string()),
                    message: Some("Instance is not ready".to_string()),
                    last_transition_time: Some("2025-11-30T00:00:00Z".to_string()),
                }],
                observed_generation: Some(1),
                service_address: None,
                cluster_ref: None,
                zones: Vec::new(),
                zones_count: None,
            })
        };

        Bind9Instance {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: cluster_ref.to_string(),
                role,
                replicas: Some(1),
                version: Some("9.18".to_string()),
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

    #[test]
    fn test_bind9cluster_status_default() {
        let status = Bind9ClusterStatus::default();
        assert_eq!(status.conditions.len(), 0);
        assert_eq!(status.instance_count, None);
        assert_eq!(status.ready_instances, None);
        assert_eq!(status.instances.len(), 0);
    }

    #[test]
    fn test_bind9cluster_status_with_values() {
        let status = Bind9ClusterStatus {
            conditions: vec![],
            observed_generation: Some(1),
            instance_count: Some(3),
            ready_instances: Some(2),
            instances: vec!["primary-dns".to_string(), "secondary-dns".to_string()],
        };

        assert_eq!(status.observed_generation, Some(1));
        assert_eq!(status.instance_count, Some(3));
        assert_eq!(status.ready_instances, Some(2));
        assert_eq!(status.instances.len(), 2);
        assert!(status.instances.contains(&"primary-dns".to_string()));
        assert!(status.instances.contains(&"secondary-dns".to_string()));
    }

    #[test]
    fn test_bind9cluster_spec_with_replicas() {
        use crate::crd::{Bind9ClusterCommonSpec, PrimaryConfig, SecondaryConfig};

        let spec = Bind9ClusterSpec {
            common: Bind9ClusterCommonSpec {
                version: Some("9.18".to_string()),
                primary: Some(PrimaryConfig {
                    replicas: Some(2),
                    labels: None,
                    service: None,
                    allow_transfer: None,
                    rndc_secret_ref: None,
                }),
                secondary: Some(SecondaryConfig {
                    replicas: Some(3),
                    labels: None,
                    service: None,
                    allow_transfer: None,
                    rndc_secret_ref: None,
                }),
                image: None,
                config_map_refs: None,
                global: None,
                rndc_secret_refs: None,
                acls: None,
                volumes: None,
                volume_mounts: None,
            },
        };

        assert_eq!(spec.common.version, Some("9.18".to_string()));
        assert_eq!(spec.common.primary.as_ref().unwrap().replicas, Some(2));
        assert_eq!(spec.common.secondary.as_ref().unwrap().replicas, Some(3));
    }

    #[test]
    fn test_bind9cluster_instances_empty() {
        let status = Bind9ClusterStatus {
            conditions: vec![],
            observed_generation: None,
            instance_count: Some(0),
            ready_instances: Some(0),
            instances: vec![],
        };

        assert_eq!(status.instances.len(), 0);
        assert_eq!(status.instance_count, Some(0));
        assert_eq!(status.ready_instances, Some(0));
    }

    // Tests for calculate_cluster_status function

    #[test]
    fn test_calculate_cluster_status_no_instances() {
        let instances: Vec<Bind9Instance> = vec![];
        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 0);
        assert_eq!(ready_instances, 0);
        assert_eq!(instance_names.len(), 0);
        assert_eq!(conditions.len(), 1); // Only encompassing condition

        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_NO_CHILDREN.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("No instances found for this cluster".to_string())
        );
    }

    #[test]
    fn test_calculate_cluster_status_all_ready() {
        let instances = vec![
            create_test_instance(
                "primary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                true,
            ),
            create_test_instance(
                "primary-2",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                true,
            ),
            create_test_instance(
                "secondary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Secondary,
                true,
            ),
        ];

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 3);
        assert_eq!(ready_instances, 3);
        assert_eq!(instance_names.len(), 3);
        assert!(instance_names.contains(&"primary-1".to_string()));
        assert!(instance_names.contains(&"primary-2".to_string()));
        assert!(instance_names.contains(&"secondary-1".to_string()));

        // Should have 1 encompassing + 3 instance conditions = 4 total
        assert_eq!(conditions.len(), 4);

        // Check encompassing condition
        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "True");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_ALL_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("All 3 instances are ready".to_string())
        );

        // Check instance-level conditions
        for (i, instance_condition) in conditions.iter().enumerate().skip(1).take(3) {
            assert_eq!(
                instance_condition.r#type,
                bind9_instance_condition_type(i - 1)
            );
            assert_eq!(instance_condition.status, "True");
            assert_eq!(instance_condition.reason, Some(REASON_READY.to_string()));
        }
    }

    #[test]
    fn test_calculate_cluster_status_some_ready() {
        let instances = vec![
            create_test_instance(
                "primary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                true,
            ),
            create_test_instance(
                "primary-2",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                false,
            ),
            create_test_instance(
                "secondary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Secondary,
                true,
            ),
        ];

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 3);
        assert_eq!(ready_instances, 2);
        assert_eq!(instance_names.len(), 3);

        // Should have 1 encompassing + 3 instance conditions = 4 total
        assert_eq!(conditions.len(), 4);

        // Check encompassing condition
        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_PARTIALLY_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("2/3 instances are ready".to_string())
        );

        // Check instance-level conditions
        // primary-1 is ready
        assert_eq!(conditions[1].r#type, bind9_instance_condition_type(0));
        assert_eq!(conditions[1].status, "True");
        assert_eq!(conditions[1].reason, Some(REASON_READY.to_string()));

        // primary-2 is not ready
        assert_eq!(conditions[2].r#type, bind9_instance_condition_type(1));
        assert_eq!(conditions[2].status, "False");
        assert_eq!(conditions[2].reason, Some(REASON_NOT_READY.to_string()));

        // secondary-1 is ready
        assert_eq!(conditions[3].r#type, bind9_instance_condition_type(2));
        assert_eq!(conditions[3].status, "True");
        assert_eq!(conditions[3].reason, Some(REASON_READY.to_string()));
    }

    #[test]
    fn test_calculate_cluster_status_none_ready() {
        let instances = vec![
            create_test_instance(
                "primary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                false,
            ),
            create_test_instance(
                "secondary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Secondary,
                false,
            ),
        ];

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 2);
        assert_eq!(ready_instances, 0);
        assert_eq!(instance_names.len(), 2);

        // Should have 1 encompassing + 2 instance conditions = 3 total
        assert_eq!(conditions.len(), 3);

        // Check encompassing condition
        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_NOT_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("No instances are ready".to_string())
        );

        // Check instance-level conditions - both not ready
        for (i, instance_condition) in conditions.iter().enumerate().skip(1).take(2) {
            assert_eq!(
                instance_condition.r#type,
                bind9_instance_condition_type(i - 1)
            );
            assert_eq!(instance_condition.status, "False");
            assert_eq!(
                instance_condition.reason,
                Some(REASON_NOT_READY.to_string())
            );
        }
    }

    #[test]
    fn test_calculate_cluster_status_single_ready_instance() {
        let instances = vec![create_test_instance(
            "primary-1",
            "test-ns",
            "test-cluster",
            ServerRole::Primary,
            true,
        )];

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 1);
        assert_eq!(ready_instances, 1);
        assert_eq!(instance_names.len(), 1);
        assert_eq!(instance_names[0], "primary-1");

        // Should have 1 encompassing + 1 instance condition = 2 total
        assert_eq!(conditions.len(), 2);

        // Check encompassing condition
        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "True");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_ALL_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("All 1 instances are ready".to_string())
        );

        // Check instance-level condition
        let instance_condition = &conditions[1];
        assert_eq!(instance_condition.r#type, bind9_instance_condition_type(0));
        assert_eq!(instance_condition.status, "True");
        assert_eq!(instance_condition.reason, Some(REASON_READY.to_string()));
    }

    #[test]
    fn test_calculate_cluster_status_single_not_ready_instance() {
        let instances = vec![create_test_instance(
            "primary-1",
            "test-ns",
            "test-cluster",
            ServerRole::Primary,
            false,
        )];

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 1);
        assert_eq!(ready_instances, 0);
        assert_eq!(instance_names.len(), 1);

        // Should have 1 encompassing + 1 instance condition = 2 total
        assert_eq!(conditions.len(), 2);

        // Check encompassing condition
        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_NOT_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("No instances are ready".to_string())
        );

        // Check instance-level condition
        let instance_condition = &conditions[1];
        assert_eq!(instance_condition.r#type, bind9_instance_condition_type(0));
        assert_eq!(instance_condition.status, "False");
        assert_eq!(
            instance_condition.reason,
            Some(REASON_NOT_READY.to_string())
        );
    }

    #[test]
    fn test_calculate_cluster_status_instance_without_status() {
        let instance = Bind9Instance {
            metadata: ObjectMeta {
                name: Some("primary-1".to_string()),
                namespace: Some("test-ns".to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: Some(1),
                version: Some("9.18".to_string()),
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
            status: None, // No status set
        };

        let instances = vec![instance];
        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 1);
        assert_eq!(ready_instances, 0); // Should not count as ready
        assert_eq!(instance_names.len(), 1);

        // Should have 1 encompassing + 1 instance condition = 2 total
        assert_eq!(conditions.len(), 2);

        // Check encompassing condition
        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_NOT_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("No instances are ready".to_string())
        );

        // Check instance-level condition - no status means not ready
        let instance_condition = &conditions[1];
        assert_eq!(instance_condition.r#type, bind9_instance_condition_type(0));
        assert_eq!(instance_condition.status, "False");
        assert_eq!(
            instance_condition.reason,
            Some(REASON_NOT_READY.to_string())
        );
    }

    #[test]
    fn test_calculate_cluster_status_instance_with_wrong_condition_type() {
        let instance = Bind9Instance {
            metadata: ObjectMeta {
                name: Some("primary-1".to_string()),
                namespace: Some("test-ns".to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: Some(1),
                version: Some("9.18".to_string()),
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
            status: Some(Bind9InstanceStatus {
                conditions: vec![Condition {
                    r#type: "NotReady".to_string(), // Wrong condition type
                    status: "True".to_string(),
                    reason: Some("Test".to_string()),
                    message: Some("Test message".to_string()),
                    last_transition_time: Some("2025-11-30T00:00:00Z".to_string()),
                }],
                observed_generation: Some(1),
                service_address: None,
                cluster_ref: None,
                zones: Vec::new(),
                zones_count: None,
            }),
        };

        let instances = vec![instance];
        let (instance_count, ready_instances, _instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 1);
        assert_eq!(ready_instances, 0); // Should not count as ready

        // Should have 1 encompassing + 1 instance condition = 2 total
        assert_eq!(conditions.len(), 2);

        // Check encompassing condition
        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_NOT_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("No instances are ready".to_string())
        );

        // Check instance-level condition - wrong condition type means not ready
        let instance_condition = &conditions[1];
        assert_eq!(instance_condition.r#type, bind9_instance_condition_type(0));
        assert_eq!(instance_condition.status, "False");
        assert_eq!(
            instance_condition.reason,
            Some(REASON_NOT_READY.to_string())
        );
    }

    #[test]
    fn test_calculate_cluster_status_large_cluster() {
        let mut instances = vec![];
        for i in 0..10 {
            let ready = i % 2 == 0; // Every other instance is ready
            instances.push(create_test_instance(
                &format!("instance-{i}"),
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                ready,
            ));
        }

        let (instance_count, ready_instances, instance_names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        assert_eq!(instance_count, 10);
        assert_eq!(ready_instances, 5);
        assert_eq!(instance_names.len(), 10);

        // Should have 1 encompassing + 10 instance conditions = 11 total
        assert_eq!(conditions.len(), 11);

        // Check encompassing condition
        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.r#type, "Ready");
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_PARTIALLY_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("5/10 instances are ready".to_string())
        );

        // Check instance-level conditions
        for i in 0..10 {
            let instance_condition = &conditions[i + 1];
            assert_eq!(instance_condition.r#type, bind9_instance_condition_type(i));

            if i % 2 == 0 {
                // Even indices are ready
                assert_eq!(instance_condition.status, "True");
                assert_eq!(instance_condition.reason, Some(REASON_READY.to_string()));
            } else {
                // Odd indices are not ready
                assert_eq!(instance_condition.status, "False");
                assert_eq!(
                    instance_condition.reason,
                    Some(REASON_NOT_READY.to_string())
                );
            }
        }
    }

    // Tests for cluster creation

    #[test]
    fn test_create_cluster_with_finalizer() {
        let mut cluster = create_test_cluster("test-cluster", "test-ns");
        cluster.metadata.finalizers = Some(vec![FINALIZER_BIND9_CLUSTER.to_string()]);

        assert!(cluster.metadata.finalizers.is_some());
        let finalizers = cluster.metadata.finalizers.as_ref().unwrap();
        assert_eq!(finalizers.len(), 1);
        assert!(finalizers.contains(&FINALIZER_BIND9_CLUSTER.to_string()));
    }

    #[test]
    fn test_create_cluster_with_deletion_timestamp() {
        use chrono::Utc;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;

        let mut cluster = create_test_cluster("test-cluster", "test-ns");
        cluster.metadata.deletion_timestamp = Some(Time(Utc::now()));

        assert!(cluster.metadata.deletion_timestamp.is_some());
    }

    #[test]
    fn test_cluster_spec_primary_replicas() {
        let cluster = create_test_cluster("test-cluster", "test-ns");
        let replicas = cluster
            .spec
            .common
            .primary
            .as_ref()
            .and_then(|p| p.replicas)
            .unwrap_or(0);

        assert_eq!(replicas, 2);
    }

    #[test]
    fn test_cluster_spec_secondary_replicas() {
        let cluster = create_test_cluster("test-cluster", "test-ns");
        let replicas = cluster
            .spec
            .common
            .secondary
            .as_ref()
            .and_then(|s| s.replicas)
            .unwrap_or(0);

        assert_eq!(replicas, 2);
    }

    // Tests for standard status reason constants

    #[test]
    fn test_status_reason_all_ready() {
        // Verify that REASON_ALL_READY constant has the correct value
        assert_eq!(REASON_ALL_READY, "AllReady");
    }

    #[test]
    fn test_status_reason_partially_ready() {
        // Verify that REASON_PARTIALLY_READY constant has the correct value
        assert_eq!(REASON_PARTIALLY_READY, "PartiallyReady");
    }

    #[test]
    fn test_status_reason_not_ready() {
        // Verify that REASON_NOT_READY constant has the correct value
        assert_eq!(REASON_NOT_READY, "NotReady");
    }

    #[test]
    fn test_status_reason_no_children() {
        // Verify that REASON_NO_CHILDREN constant has the correct value
        assert_eq!(REASON_NO_CHILDREN, "NoChildren");
    }

    // Tests for status message formats

    #[test]
    fn test_status_message_format_all_ready() {
        let instances = vec![
            create_test_instance(
                "primary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                true,
            ),
            create_test_instance(
                "primary-2",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                true,
            ),
        ];

        let (_count, _ready, _names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        // Should have 1 encompassing + 2 instance conditions = 3 total
        assert_eq!(conditions.len(), 3);

        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.status, "True");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_ALL_READY.to_string())
        );

        let message = encompassing_condition.message.as_ref().unwrap();
        assert!(message.starts_with("All "));
        assert!(message.ends_with(" instances are ready"));
        assert!(message.contains('2'));
    }

    #[test]
    fn test_status_message_format_partially_ready() {
        let instances = vec![
            create_test_instance(
                "primary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                true,
            ),
            create_test_instance(
                "primary-2",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                false,
            ),
            create_test_instance(
                "secondary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Secondary,
                false,
            ),
        ];

        let (_count, _ready, _names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        // Should have 1 encompassing + 3 instance conditions = 4 total
        assert_eq!(conditions.len(), 4);

        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_PARTIALLY_READY.to_string())
        );

        let message = encompassing_condition.message.as_ref().unwrap();
        assert!(message.contains('/'));
        assert!(message.ends_with(" instances are ready"));
        assert!(message.contains("1/3"));
    }

    #[test]
    fn test_status_message_format_not_ready() {
        let instances = vec![
            create_test_instance(
                "primary-1",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                false,
            ),
            create_test_instance(
                "primary-2",
                "test-ns",
                "test-cluster",
                ServerRole::Primary,
                false,
            ),
        ];

        let (_count, _ready, _names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        // Should have 1 encompassing + 2 instance conditions = 3 total
        assert_eq!(conditions.len(), 3);

        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_NOT_READY.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("No instances are ready".to_string())
        );
    }

    #[test]
    fn test_status_message_format_no_children() {
        let instances: Vec<Bind9Instance> = vec![];

        let (_count, _ready, _names, conditions) =
            calculate_cluster_status(&instances, "test-ns", "test-cluster");

        // Should have only 1 encompassing condition (no instances)
        assert_eq!(conditions.len(), 1);

        let encompassing_condition = &conditions[0];
        assert_eq!(encompassing_condition.status, "False");
        assert_eq!(
            encompassing_condition.reason,
            Some(REASON_NO_CHILDREN.to_string())
        );
        assert_eq!(
            encompassing_condition.message,
            Some("No instances found for this cluster".to_string())
        );
    }
}
