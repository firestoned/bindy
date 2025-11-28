// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for bind9instance reconciler
//!
//! These tests focus on the logic and helper functions used by the reconciler.

#[cfg(test)]
mod tests {
    use crate::bind9_resources::{build_configmap, build_deployment, build_service};
    use crate::crd::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn create_test_instance(name: &str, replicas: i32, version: &str) -> Bind9Instance {
        Bind9Instance {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("test-ns".to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: Some(replicas),
                version: Some(version.to_string()),
                image: None,
                config_map_refs: None,
                config: Some(Bind9Config {
                    recursion: Some(false),
                    allow_query: Some(vec!["any".to_string()]),
                    allow_transfer: Some(vec!["10.0.0.0/8".to_string()]),
                    dnssec: Some(DNSSECConfig {
                        enabled: Some(true),
                        validation: Some(true),
                    }),
                    forwarders: Some(vec!["8.8.8.8".to_string()]),
                    listen_on: Some(vec!["any".to_string()]),
                    listen_on_v6: Some(vec!["any".to_string()]),
                }),
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
            },
            status: None,
        }
    }

    #[test]
    fn test_create_instance_with_default_replicas() {
        let instance = Bind9Instance {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: None, // Should default to 1
                version: Some("9.18".to_string()),
                image: None,
                config_map_refs: None,
                config: None,
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
            },
            status: None,
        };

        assert_eq!(instance.spec.replicas, None);
        assert_eq!(instance.spec.replicas.unwrap_or(1), 1);
    }

    #[test]
    fn test_create_instance_with_default_version() {
        let instance = Bind9Instance {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: Some(3),
                version: None, // Should default to "9.18"
                image: None,
                config_map_refs: None,
                config: None,
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
            },
            status: None,
        };

        assert_eq!(instance.spec.version, None);
        assert_eq!(instance.spec.version.as_deref().unwrap_or("9.18"), "9.18");
    }

    #[test]
    fn test_configmap_creation() {
        let instance = create_test_instance("test-instance", 2, "9.18");
        let cm = build_configmap("test-instance", "test-ns", &instance, None).unwrap();

        assert_eq!(cm.metadata.name.as_deref(), Some("test-instance-config"));
        assert_eq!(cm.metadata.namespace.as_deref(), Some("test-ns"));

        let data = cm.data.expect("ConfigMap should have data");
        assert!(data.contains_key("named.conf"));
        assert!(data.contains_key("named.conf.options"));

        let options = data.get("named.conf.options").unwrap();
        assert!(options.contains("recursion no"));
        assert!(options.contains("allow-query { any; }"));
        assert!(options.contains("dnssec-enable yes"));
    }

    #[test]
    fn test_deployment_creation() {
        let instance = create_test_instance("test-deploy", 3, "9.20");
        let deployment = build_deployment("test-deploy", "test-ns", &instance, None);

        assert_eq!(deployment.metadata.name.as_deref(), Some("test-deploy"));
        assert_eq!(deployment.metadata.namespace.as_deref(), Some("test-ns"));

        let spec = deployment.spec.expect("Deployment should have spec");
        assert_eq!(spec.replicas, Some(3));

        let pod_template = spec.template;
        let pod_spec = pod_template.spec.expect("Pod template should have spec");

        // Should have 1 container: bind9 only
        assert_eq!(pod_spec.containers.len(), 1);
        let container = &pod_spec.containers[0];
        assert_eq!(container.name, "bind9");
        assert_eq!(
            container.image.as_deref(),
            Some("internetsystemsconsortium/bind9:9.20")
        );
    }

    #[test]
    fn test_service_creation() {
        let service = build_service("test-svc", "test-ns");

        assert_eq!(service.metadata.name.as_deref(), Some("test-svc"));
        assert_eq!(service.metadata.namespace.as_deref(), Some("test-ns"));

        let spec = service.spec.expect("Service should have spec");
        assert_eq!(spec.type_.as_deref(), Some("ClusterIP"));

        let ports = spec.ports.expect("Service should have ports");
        assert_eq!(ports.len(), 2);

        // Verify TCP and UDP ports
        assert!(ports.iter().any(|p| p.name.as_deref() == Some("dns-tcp")));
        assert!(ports.iter().any(|p| p.name.as_deref() == Some("dns-udp")));
    }

    #[test]
    fn test_deployment_with_various_replica_counts() {
        for replicas in [1, 2, 3, 5, 10] {
            let instance = create_test_instance("test", replicas, "9.18");
            let deployment = build_deployment("test", "test-ns", &instance, None);

            let spec = deployment.spec.unwrap();
            assert_eq!(spec.replicas, Some(replicas));
        }
    }

    #[test]
    fn test_deployment_with_various_versions() {
        for version in ["9.16", "9.18", "9.20", "latest"] {
            let instance = create_test_instance("test", 1, version);
            let deployment = build_deployment("test", "test-ns", &instance, None);

            let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
            let container = &pod_spec.containers[0];
            let expected_image = format!("internetsystemsconsortium/bind9:{version}");
            assert_eq!(container.image.as_deref(), Some(expected_image.as_str()));
        }
    }

    #[test]
    fn test_configmap_with_recursion_enabled() {
        let instance = Bind9Instance {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("test-ns".to_string()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: Some(1),
                version: Some("9.18".to_string()),
                image: None,
                config_map_refs: None,
                config: Some(Bind9Config {
                    recursion: Some(true),
                    allow_query: None,
                    allow_transfer: None,
                    dnssec: None,
                    forwarders: None,
                    listen_on: None,
                    listen_on_v6: None,
                }),
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
            },
            status: None,
        };

        let cm = build_configmap("test", "test-ns", &instance, None);
        let data = cm.unwrap().data.unwrap();
        let options = data.get("named.conf.options").unwrap();

        assert!(options.contains("recursion yes"));
    }

    #[test]
    fn test_configmap_with_forwarders() {
        let instance = Bind9Instance {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("test-ns".to_string()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: Some(1),
                version: Some("9.18".to_string()),
                image: None,
                config_map_refs: None,
                config: Some(Bind9Config {
                    recursion: Some(false),
                    allow_query: None,
                    allow_transfer: None,
                    dnssec: None,
                    forwarders: Some(vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()]),
                    listen_on: None,
                    listen_on_v6: None,
                }),
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
            },
            status: None,
        };

        let cm = build_configmap("test", "test-ns", &instance, None);
        let data = cm.unwrap().data.unwrap();
        let named_conf = data.get("named.conf").unwrap();

        // Verify the named.conf contains the proper includes
        assert!(named_conf.contains("include \"/etc/bind/named.conf.options\""));
        assert!(named_conf.contains("include \"/etc/bind/zones/named.conf.zones\""));
    }

    #[test]
    fn test_instance_metadata() {
        let instance = create_test_instance("metadata-test", 2, "9.18");

        assert_eq!(instance.metadata.name.as_deref(), Some("metadata-test"));
        assert_eq!(instance.metadata.namespace.as_deref(), Some("test-ns"));
        assert_eq!(instance.metadata.generation, Some(1));
    }

    #[test]
    fn test_instance_status_initialization() {
        let instance = create_test_instance("status-test", 1, "9.18");

        // New instances should have None status
        assert!(instance.status.is_none());
    }

    #[test]
    fn test_instance_with_dnssec_disabled() {
        let instance = Bind9Instance {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("test-ns".to_string()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: ServerRole::Primary,
                replicas: Some(1),
                version: Some("9.18".to_string()),
                image: None,
                config_map_refs: None,
                config: Some(Bind9Config {
                    recursion: Some(false),
                    allow_query: None,
                    allow_transfer: None,
                    dnssec: Some(DNSSECConfig {
                        enabled: Some(false),
                        validation: Some(false),
                    }),
                    forwarders: None,
                    listen_on: None,
                    listen_on_v6: None,
                }),
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
            },
            status: None,
        };

        let cm = build_configmap("test", "test-ns", &instance, None);
        let data = cm.unwrap().data.unwrap();
        let options = data.get("named.conf.options").unwrap();

        // Should not contain DNSSEC settings when disabled
        assert!(!options.contains("dnssec-enable yes"));
        assert!(!options.contains("dnssec-validation yes"));
    }

    #[test]
    fn test_deployment_container_ports() {
        let instance = create_test_instance("port-test", 1, "9.18");
        let deployment = build_deployment("port-test", "test-ns", &instance, None);

        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let container = &pod_spec.containers[0];
        let ports = container.ports.as_ref().unwrap();

        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0].container_port, 53);
        assert_eq!(ports[0].protocol.as_deref(), Some("TCP"));
        assert_eq!(ports[1].container_port, 53);
        assert_eq!(ports[1].protocol.as_deref(), Some("UDP"));
        assert_eq!(ports[2].container_port, 953);
        assert_eq!(ports[2].protocol.as_deref(), Some("TCP"));
    }

    #[test]
    fn test_deployment_has_probes() {
        let instance = create_test_instance("probe-test", 1, "9.18");
        let deployment = build_deployment("probe-test", "test-ns", &instance, None);

        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let container = &pod_spec.containers[0];

        assert!(container.liveness_probe.is_some());
        assert!(container.readiness_probe.is_some());

        let liveness = container.liveness_probe.as_ref().unwrap();
        assert!(liveness.tcp_socket.is_some());
        assert_eq!(liveness.initial_delay_seconds, Some(30));

        let readiness = container.readiness_probe.as_ref().unwrap();
        assert!(readiness.tcp_socket.is_some());
        assert_eq!(readiness.initial_delay_seconds, Some(10));
    }

    #[test]
    fn test_deployment_volume_mounts() {
        let instance = create_test_instance("volume-test", 1, "9.18");
        let deployment = build_deployment("volume-test", "test-ns", &instance, None);

        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let container = &pod_spec.containers[0];
        let mounts = container.volume_mounts.as_ref().unwrap();

        assert_eq!(mounts.len(), 4);
        assert!(mounts.iter().any(|m| m.name == "config"));
        assert!(mounts.iter().any(|m| m.name == "zones"));
        assert!(mounts.iter().any(|m| m.name == "cache"));
    }

    #[test]
    fn test_service_selector_matches_deployment_labels() {
        let instance = create_test_instance("label-test", 1, "9.18");
        let deployment = build_deployment("label-test", "test-ns", &instance, None);
        let service = build_service("label-test", "test-ns");

        let deploy_labels = deployment.metadata.labels.unwrap();
        let svc_selector = service.spec.unwrap().selector.unwrap();

        // Service selector should match deployment labels
        for (key, value) in &svc_selector {
            assert_eq!(deploy_labels.get(key), Some(value));
        }
    }
}
