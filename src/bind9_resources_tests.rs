// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `bind9_resources`

#[cfg(test)]
mod tests {
    use crate::bind9_resources::{
        build_configmap, build_deployment, build_labels, build_labels_from_instance, build_service,
    };
    use crate::constants::KIND_BIND9_CLUSTER;
    use crate::crd::{Bind9Config, Bind9Instance, Bind9InstanceSpec, DNSSECConfig};
    use crate::labels::BINDY_MANAGED_BY_LABEL;
    use k8s_openapi::api::core::v1::ServiceSpec;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    fn create_test_instance(name: &str) -> Bind9Instance {
        Bind9Instance {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some("test-ns".into()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: "test-cluster".to_string(),
                role: crate::crd::ServerRole::Primary,
                replicas: Some(2),
                version: Some("9.18".into()),
                image: None,
                config_map_refs: None,
                config: Some(Bind9Config {
                    recursion: Some(false),
                    allow_query: Some(vec!["0.0.0.0/0".into()]),
                    allow_transfer: Some(vec!["10.0.0.0/8".into()]),
                    dnssec: Some(DNSSECConfig {
                        validation: Some(true),
                    }),
                    forwarders: None,
                    listen_on: None,
                    listen_on_v6: None,
                    rndc_secret_ref: None,
                    bindcar_config: None,
                }),
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
                rndc_secret_ref: None,
                storage: None,
                bindcar_config: None,
                zones_from: None,
            },
            status: None,
        }
    }

    #[test]
    fn test_build_labels() {
        let labels = build_labels("test-instance");
        assert_eq!(labels.get("app").unwrap(), "bind9");
        assert_eq!(labels.get("instance").unwrap(), "test-instance");
        assert_eq!(labels.get("app.kubernetes.io/name").unwrap(), "bind9");
        assert_eq!(
            labels.get("app.kubernetes.io/managed-by").unwrap(),
            "Bind9Instance"
        );
        assert_eq!(labels.get("app.kubernetes.io/part-of").unwrap(), "bindy");
    }

    #[test]
    fn test_build_labels_from_instance_standalone() {
        // Test standalone instance (no managed-by label)
        let instance = create_test_instance("test-instance");
        let labels = build_labels_from_instance("test-instance", &instance);

        assert_eq!(labels.get("app").unwrap(), "bind9");
        assert_eq!(labels.get("instance").unwrap(), "test-instance");
        assert_eq!(labels.get("app.kubernetes.io/name").unwrap(), "bind9");
        assert_eq!(
            labels.get("app.kubernetes.io/managed-by").unwrap(),
            "Bind9Instance"
        );
        assert_eq!(labels.get("app.kubernetes.io/part-of").unwrap(), "bindy");
    }

    #[test]
    fn test_build_labels_from_instance_cluster_managed() {
        // Test instance managed by a cluster
        let mut instance = create_test_instance("test-instance");
        let mut instance_labels = BTreeMap::new();
        instance_labels.insert(
            BINDY_MANAGED_BY_LABEL.to_string(),
            KIND_BIND9_CLUSTER.to_string(),
        );
        instance.metadata.labels = Some(instance_labels);

        let labels = build_labels_from_instance("test-instance", &instance);

        assert_eq!(labels.get("app").unwrap(), "bind9");
        assert_eq!(labels.get("instance").unwrap(), "test-instance");
        assert_eq!(labels.get("app.kubernetes.io/name").unwrap(), "bind9");
        // IMPORTANT: Should propagate Bind9Cluster from instance labels
        assert_eq!(
            labels.get("app.kubernetes.io/managed-by").unwrap(),
            KIND_BIND9_CLUSTER
        );
        assert_eq!(labels.get("app.kubernetes.io/part-of").unwrap(), "bindy");
    }

    #[test]
    fn test_build_deployment_propagates_managed_by_label() {
        // Test that deployment propagates managed-by label from cluster-managed instance
        let mut instance = create_test_instance("test-instance");
        let mut instance_labels = BTreeMap::new();
        instance_labels.insert(
            BINDY_MANAGED_BY_LABEL.to_string(),
            KIND_BIND9_CLUSTER.to_string(),
        );
        instance.metadata.labels = Some(instance_labels);

        let deployment = build_deployment("test-instance", "test-ns", &instance, None, None);

        let labels = deployment.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get("app.kubernetes.io/managed-by").unwrap(),
            KIND_BIND9_CLUSTER,
            "Deployment should propagate managed-by label from instance"
        );
    }

    #[test]
    fn test_build_service_propagates_managed_by_label() {
        // Test that service propagates managed-by label from cluster-managed instance
        let mut instance = create_test_instance("test-instance");
        let mut instance_labels = BTreeMap::new();
        instance_labels.insert(
            BINDY_MANAGED_BY_LABEL.to_string(),
            KIND_BIND9_CLUSTER.to_string(),
        );
        instance.metadata.labels = Some(instance_labels);

        let service = build_service("test-instance", "test-ns", &instance, None);

        let labels = service.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get("app.kubernetes.io/managed-by").unwrap(),
            KIND_BIND9_CLUSTER,
            "Service should propagate managed-by label from instance"
        );
    }

    #[test]
    fn test_build_configmap() {
        let instance = create_test_instance("test");
        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();

        assert_eq!(cm.metadata.name.as_deref(), Some("test-config"));
        assert_eq!(cm.metadata.namespace.as_deref(), Some("test-ns"));

        let data = cm.data.unwrap();
        assert!(data.contains_key("named.conf"));
        assert!(data.contains_key("named.conf.options"));

        let options = data.get("named.conf.options").unwrap();
        assert!(options.contains("recursion no"));
        assert!(options.contains("allow-query { 0.0.0.0/0; }"));
        assert!(options.contains("allow-transfer { 10.0.0.0/8; }"));
        assert!(options.contains("dnssec-validation yes"));
    }

    #[test]
    fn test_build_deployment() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance, None, None);

        assert_eq!(deployment.metadata.name.as_deref(), Some("test"));
        assert_eq!(deployment.metadata.namespace.as_deref(), Some("test-ns"));

        let spec = deployment.spec.unwrap();
        assert_eq!(spec.replicas, Some(2));

        let pod_spec = spec.template.spec.unwrap();
        // Should have 2 containers: bind9 + api sidecar
        assert_eq!(pod_spec.containers.len(), 2);

        let container = &pod_spec.containers[0];
        assert_eq!(container.name, "bind9");
        assert_eq!(
            container.image.as_deref(),
            Some("internetsystemsconsortium/bind9:9.18")
        );

        let ports = container.ports.as_ref().unwrap();
        // Should have 3 ports: DNS TCP, DNS UDP, and RNDC
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0].container_port, 53); // DNS TCP
        assert_eq!(ports[1].container_port, 53); // DNS UDP
        assert_eq!(ports[2].container_port, 953); // RNDC
    }

    #[test]
    fn test_build_service() {
        let instance = create_test_instance("test");
        let service = build_service("test", "test-ns", &instance, None);

        assert_eq!(service.metadata.name.as_deref(), Some("test"));
        assert_eq!(service.metadata.namespace.as_deref(), Some("test-ns"));

        let spec = service.spec.unwrap();
        assert_eq!(spec.type_.as_deref(), Some("ClusterIP"));

        let ports = spec.ports.unwrap();
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0].port, 53); // dns-tcp
        assert_eq!(ports[1].port, 53); // dns-udp
        assert_eq!(ports[2].port, 80); // http api
    }

    #[test]
    fn test_build_pod_spec() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();

        // Verify volumes
        let volumes = pod_spec.volumes.unwrap();
        assert_eq!(volumes.len(), 4);
        // Order: zones, cache, rndc-key, config (from build_volumes function)
        assert_eq!(volumes[0].name, "zones");
        assert_eq!(volumes[1].name, "cache");
        assert_eq!(volumes[2].name, "rndc-key");
        assert_eq!(volumes[3].name, "config");

        // Verify container configuration
        let container = &pod_spec.containers[0];
        assert_eq!(container.name, "bind9");

        // Verify volume mounts
        let mounts = container.volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 6); // zones, cache, rndc-key, named.conf, named.conf.options, rndc.conf
        assert!(mounts.iter().any(|m| m.name == "config"));
        assert!(mounts.iter().any(|m| m.name == "zones"));
        assert!(mounts.iter().any(|m| m.name == "cache"));
        assert!(mounts.iter().any(|m| m.name == "rndc-key"));

        // Verify probes
        assert!(container.liveness_probe.is_some());
        assert!(container.readiness_probe.is_some());
    }

    #[test]
    fn test_build_deployment_replicas() {
        let mut instance = create_test_instance("test");
        instance.spec.replicas = Some(5);

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let spec = deployment.spec.unwrap();

        assert_eq!(spec.replicas, Some(5));
    }

    #[test]
    fn test_build_deployment_version() {
        let mut instance = create_test_instance("test");
        instance.spec.version = Some("9.20".into());

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let container = &pod_spec.containers[0];

        assert_eq!(
            container.image.as_deref(),
            Some("internetsystemsconsortium/bind9:9.20")
        );
    }

    #[test]
    fn test_build_service_ports() {
        let instance = create_test_instance("test");

        let service = build_service("test", "test-ns", &instance, None);
        let spec = service.spec.unwrap();
        let ports = spec.ports.unwrap();

        // Service should expose DNS (TCP/UDP) and HTTP API (not RNDC)
        assert_eq!(ports.len(), 3);

        // Check DNS TCP port
        let tcp_port = ports
            .iter()
            .find(|p| p.name.as_deref() == Some("dns-tcp"))
            .unwrap();
        assert_eq!(tcp_port.port, 53);
        assert_eq!(tcp_port.protocol.as_deref(), Some("TCP"));

        // Check DNS UDP port
        let udp_port = ports
            .iter()
            .find(|p| p.name.as_deref() == Some("dns-udp"))
            .unwrap();
        assert_eq!(udp_port.port, 53);
        assert_eq!(udp_port.protocol.as_deref(), Some("UDP"));

        // Check HTTP API port (not RNDC)
        let http_port = ports
            .iter()
            .find(|p| p.name.as_deref() == Some("http"))
            .unwrap();
        assert_eq!(http_port.port, 80, "HTTP API should be exposed on port 80");
        assert_eq!(http_port.protocol.as_deref(), Some("TCP"));

        // Verify RNDC port is NOT exposed
        assert!(
            ports.iter().all(|p| p.name.as_deref() != Some("rndc")),
            "RNDC port should not be exposed in Service (localhost only)"
        );
    }

    #[test]
    fn test_configmap_contains_all_files() {
        let instance = create_test_instance("test");
        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();

        let data = cm.data.unwrap();
        assert!(data.contains_key("named.conf"));
        assert!(data.contains_key("named.conf.options"));
    }

    #[test]
    fn test_labels_consistency() {
        let labels = build_labels("my-instance");

        // Verify all required labels are present
        assert!(labels.contains_key("app"));
        assert!(labels.contains_key("instance"));
        assert!(labels.contains_key("app.kubernetes.io/name"));
        assert!(labels.contains_key("app.kubernetes.io/instance"));
        assert!(labels.contains_key("app.kubernetes.io/component"));
        assert!(labels.contains_key("app.kubernetes.io/managed-by"));

        // Verify label values
        assert_eq!(labels.get("instance").unwrap(), "my-instance");
    }

    #[test]
    fn test_configmap_naming() {
        let instance = create_test_instance("test-instance");
        let cm = build_configmap("test-instance", "test-ns", &instance, None, None).unwrap();

        assert_eq!(cm.metadata.name.as_deref(), Some("test-instance-config"));
        assert_eq!(cm.metadata.namespace.as_deref(), Some("test-ns"));
    }

    #[test]
    fn test_deployment_selector_matches_labels() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance, None, None);

        let spec = deployment.spec.unwrap();
        let selector_labels = spec.selector.match_labels.unwrap();
        let pod_labels = spec.template.metadata.unwrap().labels.unwrap();

        // Verify selector matches pod labels
        for (key, value) in &selector_labels {
            assert_eq!(pod_labels.get(key), Some(value));
        }
    }

    #[test]
    fn test_service_selector_matches_deployment() {
        let instance = create_test_instance("test");

        let service = build_service("test", "test-ns", &instance, None);
        let deployment_labels = build_labels("test");

        let svc_selector = service.spec.unwrap().selector.unwrap();

        // Verify service selector matches deployment labels
        for (key, value) in &svc_selector {
            assert_eq!(deployment_labels.get(key), Some(value));
        }
    }

    // =====================================================
    // Comprehensive Edge-Case Tests for Missing Coverage
    // =====================================================

    #[test]
    fn test_configmap_with_custom_refs_returns_none() {
        use crate::crd::ConfigMapRefs;

        let mut instance = create_test_instance("test");
        instance.spec.config_map_refs = Some(ConfigMapRefs {
            named_conf_zones: None,
            named_conf: Some("custom-named-conf".to_string()),
            named_conf_options: None,
        });

        let cm = build_configmap("test", "test-ns", &instance, None, None);

        // Should return None when custom ConfigMaps are referenced
        assert!(cm.is_none());
    }

    #[test]
    fn test_configmap_with_custom_options_ref_returns_none() {
        use crate::crd::ConfigMapRefs;

        let mut instance = create_test_instance("test");
        instance.spec.config_map_refs = Some(ConfigMapRefs {
            named_conf_zones: None,
            named_conf: None,
            named_conf_options: Some("custom-options".to_string()),
        });

        let cm = build_configmap("test", "test-ns", &instance, None, None);

        // Should return None when custom ConfigMaps are referenced
        assert!(cm.is_none());
    }

    #[test]
    fn test_configmap_with_both_custom_refs_returns_none() {
        use crate::crd::ConfigMapRefs;

        let mut instance = create_test_instance("test");
        instance.spec.config_map_refs = Some(ConfigMapRefs {
            named_conf_zones: None,
            named_conf: Some("custom-named-conf".to_string()),
            named_conf_options: Some("custom-options".to_string()),
        });

        let cm = build_configmap("test", "test-ns", &instance, None, None);

        assert!(cm.is_none());
    }

    #[test]
    fn test_configmap_with_empty_custom_refs_generates_config() {
        use crate::crd::ConfigMapRefs;

        let mut instance = create_test_instance("test");
        instance.spec.config_map_refs = Some(ConfigMapRefs {
            named_conf_zones: None,
            named_conf: None,
            named_conf_options: None,
        });

        let cm = build_configmap("test", "test-ns", &instance, None, None);

        // Should still generate ConfigMap if refs exist but are empty
        assert!(cm.is_some());
    }

    #[test]
    fn test_configmap_with_empty_allow_query_list() {
        let mut instance = create_test_instance("test");
        instance.spec.config.as_mut().unwrap().allow_query = Some(vec![]);

        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();
        let options = cm.data.unwrap().get("named.conf.options").unwrap().clone();

        // Empty allow_query list should result in no allow-query directive
        assert!(!options.contains("allow-query"));
    }

    #[test]
    fn test_configmap_with_empty_allow_transfer_list() {
        let mut instance = create_test_instance("test");
        instance.spec.config.as_mut().unwrap().allow_transfer = Some(vec![]);

        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();
        let options = cm.data.unwrap().get("named.conf.options").unwrap().clone();

        // Empty allow_transfer list should result in "none"
        assert!(options.contains("allow-transfer { none; }"));
    }

    #[test]
    fn test_configmap_with_no_config_section() {
        let mut instance = create_test_instance("test");
        instance.spec.config = None;

        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();
        let options = cm.data.unwrap().get("named.conf.options").unwrap().clone();

        // Defaults should be applied - recursion no, but NO allow-transfer directive
        assert!(options.contains("recursion no"));
        // With no config, no allow-transfer directive should be present (BIND9's default: none)
        assert!(!options.contains("allow-transfer"));
    }

    #[test]
    fn test_configmap_with_dnssec_validation_disabled() {
        let mut instance = create_test_instance("test");
        instance.spec.config.as_mut().unwrap().dnssec = Some(DNSSECConfig {
            validation: Some(false),
        });

        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();
        let options = cm.data.unwrap().get("named.conf.options").unwrap().clone();

        // Should contain dnssec-validation no when disabled
        assert!(options.contains("dnssec-validation no"));
        assert!(!options.contains("dnssec-validation yes"));
    }

    #[test]
    fn test_configmap_with_dnssec_validation_enabled() {
        let mut instance = create_test_instance("test");
        instance.spec.config.as_mut().unwrap().dnssec = Some(DNSSECConfig {
            validation: Some(true),
        });

        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();
        let options = cm.data.unwrap().get("named.conf.options").unwrap().clone();

        // DNSSEC is always enabled in BIND 9.15+, only validation can be configured
        assert!(options.contains("dnssec-validation yes"));
    }

    #[test]
    fn test_configmap_without_dnssec_config() {
        let mut instance = create_test_instance("test");
        instance.spec.config.as_mut().unwrap().dnssec = None;

        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();
        let options = cm.data.unwrap().get("named.conf.options").unwrap().clone();

        // Default behavior when no DNSSEC config is provided
        assert!(!options.contains("dnssec-validation yes"));
    }

    #[test]
    fn test_deployment_with_custom_image() {
        use crate::crd::ImageConfig;

        let mut instance = create_test_instance("test");
        instance.spec.image = Some(ImageConfig {
            image: Some("custom-registry/bind9:custom-tag".to_string()),
            image_pull_policy: Some("Always".to_string()),
            image_pull_secrets: None,
        });

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        assert_eq!(
            container.image.as_deref(),
            Some("custom-registry/bind9:custom-tag")
        );
        assert_eq!(container.image_pull_policy.as_deref(), Some("Always"));
    }

    #[test]
    fn test_deployment_with_image_pull_secrets() {
        use crate::crd::ImageConfig;

        let mut instance = create_test_instance("test");
        instance.spec.image = Some(ImageConfig {
            image: None,
            image_pull_policy: None,
            image_pull_secrets: Some(vec!["secret1".to_string(), "secret2".to_string()]),
        });

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();

        let secrets = pod_spec.image_pull_secrets.unwrap();
        assert_eq!(secrets.len(), 2);
        assert_eq!(secrets[0].name.as_str(), "secret1");
        assert_eq!(secrets[1].name.as_str(), "secret2");
    }

    #[test]
    fn test_deployment_with_default_replicas() {
        let mut instance = create_test_instance("test");
        instance.spec.replicas = None;

        let deployment = build_deployment("test", "test-ns", &instance, None, None);

        // Should default to 1 replica
        assert_eq!(deployment.spec.unwrap().replicas, Some(1));
    }

    #[test]
    fn test_deployment_with_zero_replicas() {
        let mut instance = create_test_instance("test");
        instance.spec.replicas = Some(0);

        let deployment = build_deployment("test", "test-ns", &instance, None, None);

        assert_eq!(deployment.spec.unwrap().replicas, Some(0));
    }

    #[test]
    fn test_deployment_with_custom_volumes() {
        use k8s_openapi::api::core::v1::{PersistentVolumeClaimVolumeSource, Volume};

        let mut instance = create_test_instance("test");
        instance.spec.volumes = Some(vec![Volume {
            name: "data-volume".to_string(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: "my-pvc".to_string(),
                read_only: Some(false),
            }),
            ..Default::default()
        }]);

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let volumes = deployment
            .spec
            .unwrap()
            .template
            .spec
            .unwrap()
            .volumes
            .unwrap();

        // Should have default volumes (zones, cache, config) plus custom volume
        assert!(volumes.iter().any(|v| v.name == "data-volume"));
        assert!(volumes.iter().any(|v| v.name == "zones"));
        assert!(volumes.iter().any(|v| v.name == "cache"));
        assert!(volumes.iter().any(|v| v.name == "config"));
    }

    #[test]
    fn test_deployment_with_custom_volume_mounts() {
        use k8s_openapi::api::core::v1::VolumeMount;

        let mut instance = create_test_instance("test");
        instance.spec.volume_mounts = Some(vec![VolumeMount {
            name: "data-volume".to_string(),
            mount_path: "/data".to_string(),
            read_only: Some(false),
            ..Default::default()
        }]);

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let mounts = pod_spec.containers[0].volume_mounts.as_ref().unwrap();

        // Should have custom mount
        assert!(mounts.iter().any(|m| m.name == "data-volume"));
        assert!(mounts.iter().any(|m| m.mount_path == "/data"));
    }

    #[test]
    fn test_deployment_container_has_correct_command() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        // Verify named command with correct flags
        assert_eq!(container.command.as_ref().unwrap()[0], "named");
        let args = container.args.as_ref().unwrap();
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"/etc/bind/named.conf".to_string()));
        assert!(args.contains(&"-g".to_string())); // Foreground mode
    }

    #[test]
    fn test_deployment_has_tz_environment_variable() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        let env = container.env.as_ref().unwrap();
        let tz_var = env.iter().find(|e| e.name == "TZ").unwrap();
        assert_eq!(tz_var.value.as_deref(), Some("UTC"));
    }

    #[test]
    fn test_deployment_has_malloc_conf_environment_variable() {
        use crate::constants::BIND9_MALLOC_CONF;

        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        let env = container.env.as_ref().unwrap();
        let malloc_conf_var = env.iter().find(|e| e.name == "MALLOC_CONF").unwrap();
        assert_eq!(malloc_conf_var.value.as_deref(), Some(BIND9_MALLOC_CONF));
    }

    #[test]
    fn test_deployment_probe_configuration() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        // Liveness probe
        let liveness = container.liveness_probe.as_ref().unwrap();
        assert_eq!(liveness.initial_delay_seconds, Some(30));
        assert_eq!(liveness.period_seconds, Some(10));
        assert_eq!(liveness.timeout_seconds, Some(5));
        assert_eq!(liveness.failure_threshold, Some(3));

        // Readiness probe
        let readiness = container.readiness_probe.as_ref().unwrap();
        assert_eq!(readiness.initial_delay_seconds, Some(10));
        assert_eq!(readiness.period_seconds, Some(5));
        assert_eq!(readiness.timeout_seconds, Some(3));
        assert_eq!(readiness.failure_threshold, Some(3));
    }

    #[test]
    fn test_service_has_correct_type() {
        let instance = create_test_instance("test");

        let service = build_service("test", "test-ns", &instance, None);

        assert_eq!(service.spec.unwrap().type_.as_deref(), Some("ClusterIP"));
    }

    #[test]
    fn test_service_port_target_ports() {
        let instance = create_test_instance("test");

        let service = build_service("test", "test-ns", &instance, None);
        let ports = service.spec.unwrap().ports.unwrap();

        for port in &ports {
            let expected_port = if port.name.as_deref() == Some("http") {
                8080
            } else {
                53
            };
            assert_eq!(
                port.target_port,
                Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(expected_port))
            );
        }
    }

    #[test]
    fn test_build_labels_with_special_characters() {
        let labels = build_labels("my-instance_v2.0");

        assert_eq!(labels.get("instance").unwrap(), "my-instance_v2.0");
    }

    #[test]
    fn test_build_labels_contains_all_recommended_labels() {
        let labels = build_labels("test");

        // Check all Kubernetes recommended labels
        assert!(labels.contains_key("app.kubernetes.io/name"));
        assert!(labels.contains_key("app.kubernetes.io/instance"));
        assert!(labels.contains_key("app.kubernetes.io/component"));
        assert!(labels.contains_key("app.kubernetes.io/managed-by"));

        assert_eq!(
            labels.get("app.kubernetes.io/component").unwrap(),
            "dns-server"
        );
    }

    #[test]
    fn test_configmap_with_multiple_acls() {
        let mut instance = create_test_instance("test");
        instance.spec.config.as_mut().unwrap().allow_query = Some(vec![
            "10.0.0.0/8".to_string(),
            "192.168.0.0/16".to_string(),
            "172.16.0.0/12".to_string(),
        ]);

        let cm = build_configmap("test", "test-ns", &instance, None, None).unwrap();
        let options = cm.data.unwrap().get("named.conf.options").unwrap().clone();

        assert!(options.contains("allow-query { 10.0.0.0/8; 192.168.0.0/16; 172.16.0.0/12; }"));
    }

    #[test]
    fn test_deployment_with_custom_namespace() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "custom-namespace", &instance, None, None);

        assert_eq!(
            deployment.metadata.namespace.as_deref(),
            Some("custom-namespace")
        );
    }

    #[test]
    fn test_service_with_custom_namespace() {
        let instance = create_test_instance("test");
        let service = build_service("test", "custom-namespace", &instance, None);

        assert_eq!(
            service.metadata.namespace.as_deref(),
            Some("custom-namespace")
        );
    }

    #[test]
    fn test_configmap_with_custom_namespace() {
        let instance = create_test_instance("test");
        let cm = build_configmap("test", "custom-namespace", &instance, None, None).unwrap();

        assert_eq!(cm.metadata.namespace.as_deref(), Some("custom-namespace"));
    }

    #[test]
    fn test_deployment_default_version() {
        let mut instance = create_test_instance("test");
        instance.spec.version = None;

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        // Should default to 9.18
        assert_eq!(
            container.image.as_deref(),
            Some("internetsystemsconsortium/bind9:9.18")
        );
    }

    #[test]
    fn test_deployment_with_custom_image_overrides_version() {
        use crate::crd::ImageConfig;

        let mut instance = create_test_instance("test");
        instance.spec.version = Some("9.20".to_string());
        instance.spec.image = Some(ImageConfig {
            image: Some("custom/image:tag".to_string()),
            image_pull_policy: None,
            image_pull_secrets: None,
        });

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        // Custom image should override version
        assert_eq!(container.image.as_deref(), Some("custom/image:tag"));
    }

    #[test]
    fn test_deployment_with_image_config_but_no_custom_image() {
        use crate::crd::ImageConfig;

        let mut instance = create_test_instance("test");
        instance.spec.version = Some("9.20".to_string());
        instance.spec.image = Some(ImageConfig {
            image: None,
            image_pull_policy: Some("Always".to_string()),
            image_pull_secrets: None,
        });

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        // Should use version from spec
        assert_eq!(
            container.image.as_deref(),
            Some("internetsystemsconsortium/bind9:9.20")
        );
        assert_eq!(container.image_pull_policy.as_deref(), Some("Always"));
    }

    #[test]
    fn test_deployment_default_image_pull_policy() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let container = &deployment.spec.unwrap().template.spec.unwrap().containers[0];

        assert_eq!(container.image_pull_policy.as_deref(), Some("IfNotPresent"));
    }

    #[test]
    fn test_build_service_with_nodeport_type() {
        let instance = create_test_instance("test");
        let custom_config = crate::crd::ServiceConfig {
            spec: Some(ServiceSpec {
                type_: Some("NodePort".into()),
                ..Default::default()
            }),
            annotations: None,
        };
        let service = build_service("test", "test-ns", &instance, Some(&custom_config));

        assert_eq!(service.metadata.name.as_deref(), Some("test"));
        assert_eq!(service.spec.unwrap().type_.as_deref(), Some("NodePort"));
    }

    #[test]
    fn test_build_service_with_loadbalancer_type() {
        let instance = create_test_instance("test");
        let custom_config = crate::crd::ServiceConfig {
            spec: Some(ServiceSpec {
                type_: Some("LoadBalancer".into()),
                load_balancer_ip: Some("192.168.1.100".into()),
                ..Default::default()
            }),
            annotations: None,
        };
        let service = build_service("test", "test-ns", &instance, Some(&custom_config));

        let spec = service.spec.unwrap();
        assert_eq!(spec.type_.as_deref(), Some("LoadBalancer"));
        assert_eq!(spec.load_balancer_ip.as_deref(), Some("192.168.1.100"));
    }

    #[test]
    fn test_build_service_with_session_affinity() {
        let instance = create_test_instance("test");
        let custom_config = crate::crd::ServiceConfig {
            spec: Some(ServiceSpec {
                session_affinity: Some("ClientIP".into()),
                ..Default::default()
            }),
            annotations: None,
        };
        let service = build_service("test", "test-ns", &instance, Some(&custom_config));

        let spec = service.spec.unwrap();
        assert_eq!(spec.session_affinity.as_deref(), Some("ClientIP"));
        // Should still have default type
        assert_eq!(spec.type_.as_deref(), Some("ClusterIP"));
    }

    #[test]
    fn test_build_service_defaults_to_clusterip() {
        let instance = create_test_instance("test");
        let service_none = build_service("test", "test-ns", &instance, None);
        let custom_config = crate::crd::ServiceConfig {
            spec: Some(ServiceSpec {
                type_: Some("ClusterIP".into()),
                ..Default::default()
            }),
            annotations: None,
        };
        let service_clusterip = build_service("test", "test-ns", &instance, Some(&custom_config));

        assert_eq!(
            service_none.spec.as_ref().unwrap().type_,
            service_clusterip.spec.as_ref().unwrap().type_
        );
    }

    #[test]
    fn test_build_service_partial_spec_merge() {
        let instance = create_test_instance("test");
        let custom_config = crate::crd::ServiceConfig {
            spec: Some(ServiceSpec {
                type_: Some("NodePort".into()),
                external_traffic_policy: Some("Local".into()),
                ..Default::default()
            }),
            annotations: None,
        };
        let service = build_service("test", "test-ns", &instance, Some(&custom_config));

        let spec = service.spec.unwrap();
        assert_eq!(spec.type_.as_deref(), Some("NodePort"));
        assert_eq!(spec.external_traffic_policy.as_deref(), Some("Local"));
        // Ports should still be default (not affected by custom spec)
        assert_eq!(spec.ports.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_build_service_with_annotations() {
        use std::collections::BTreeMap;

        let instance = create_test_instance("test");
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "metallb.universe.tf/address-pool".to_string(),
            "my-pool".to_string(),
        );
        annotations.insert(
            "external-dns.alpha.kubernetes.io/hostname".to_string(),
            "ns1.example.com".to_string(),
        );

        let custom_config = crate::crd::ServiceConfig {
            spec: Some(ServiceSpec {
                type_: Some("LoadBalancer".into()),
                ..Default::default()
            }),
            annotations: Some(annotations.clone()),
        };
        let service = build_service("test", "test-ns", &instance, Some(&custom_config));

        // Verify annotations are applied
        let svc_annotations = service.metadata.annotations.as_ref().unwrap();
        assert_eq!(
            svc_annotations.get("metallb.universe.tf/address-pool"),
            Some(&"my-pool".to_string())
        );
        assert_eq!(
            svc_annotations.get("external-dns.alpha.kubernetes.io/hostname"),
            Some(&"ns1.example.com".to_string())
        );

        // Verify spec is also applied
        assert_eq!(service.spec.unwrap().type_.as_deref(), Some("LoadBalancer"));
    }

    #[test]
    fn test_deployment_rndc_conf_volume_mount() {
        let instance = create_test_instance("rndc-test");
        let deployment = build_deployment("rndc-test", "test-ns", &instance, None, None);

        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let container = &pod_spec.containers[0];
        let mounts = container.volume_mounts.as_ref().unwrap();

        // Verify rndc.conf is mounted from the config ConfigMap
        let rndc_conf_mount = mounts
            .iter()
            .find(|m| m.mount_path == "/etc/bind/rndc.conf");
        assert!(rndc_conf_mount.is_some(), "rndc.conf mount not found");

        let mount = rndc_conf_mount.unwrap();
        assert_eq!(mount.name, "config");
        assert_eq!(mount.sub_path.as_deref(), Some("rndc.conf"));
        assert_eq!(mount.mount_path, "/etc/bind/rndc.conf");
    }

    #[test]
    fn test_configmap_contains_rndc_conf() {
        let instance = create_test_instance("rndc-test");
        let configmap = build_configmap("rndc-test", "test-ns", &instance, None, None);

        assert!(configmap.is_some());
        let cm = configmap.unwrap();
        let data = cm.data.unwrap();

        // Verify rndc.conf is present in ConfigMap
        assert!(
            data.contains_key("rndc.conf"),
            "rndc.conf not found in ConfigMap"
        );

        // Verify rndc.conf content includes key file include
        let rndc_conf = data.get("rndc.conf").unwrap();
        assert!(
            rndc_conf.contains("include \"/etc/bind/keys/rndc.key\""),
            "rndc.conf should include the rndc.key file"
        );
    }

    #[test]
    fn test_rndc_key_volume_mount() {
        let instance = create_test_instance("rndc-test");
        let deployment = build_deployment("rndc-test", "test-ns", &instance, None, None);

        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let container = &pod_spec.containers[0];
        let mounts = container.volume_mounts.as_ref().unwrap();

        // Verify rndc key directory is mounted from Secret
        let rndc_key_mount = mounts.iter().find(|m| m.mount_path == "/etc/bind/keys");
        assert!(rndc_key_mount.is_some(), "rndc key mount not found");

        let mount = rndc_key_mount.unwrap();
        assert_eq!(mount.name, "rndc-key");
        assert_eq!(mount.mount_path, "/etc/bind/keys");
        assert_eq!(mount.read_only, Some(true));
    }

    #[test]
    fn test_rndc_key_volume_source() {
        let instance = create_test_instance("rndc-test");
        let deployment = build_deployment("rndc-test", "test-ns", &instance, None, None);

        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let volumes = pod_spec.volumes.unwrap();

        // Verify rndc-key volume is backed by a Secret
        let rndc_key_volume = volumes.iter().find(|v| v.name == "rndc-key");
        assert!(rndc_key_volume.is_some(), "rndc-key volume not found");

        let volume = rndc_key_volume.unwrap();
        assert!(
            volume.secret.is_some(),
            "rndc-key volume should be a Secret"
        );

        let secret_source = volume.secret.as_ref().unwrap();
        assert_eq!(
            secret_source.secret_name.as_deref(),
            Some("rndc-test-rndc-key")
        );
    }

    #[test]
    fn test_service_api_port_default() {
        // Test default API port (8080) when no bindcar_config is specified
        let instance = create_test_instance("test");

        let service = build_service("test", "test-ns", &instance, None);
        let ports = service.spec.unwrap().ports.unwrap();

        let http_port = ports
            .iter()
            .find(|p| p.name.as_deref() == Some("http"))
            .unwrap();

        assert_eq!(http_port.port, 80, "External port should be 80");
        // Target port should be 8080 (default)
        assert_eq!(
            http_port.target_port.as_ref().map(|p| match p {
                k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(i) => i,
                _ => &0,
            }),
            Some(&8080),
            "Default target port should be 8080"
        );
    }

    #[test]
    fn test_service_api_port_custom() {
        // Test custom API port via bindcar_config
        let mut instance = create_test_instance("test");
        instance.spec.bindcar_config = Some(crate::crd::BindcarConfig {
            image: None,
            image_pull_policy: None,
            resources: None,
            port: Some(9090),
            env_vars: None,
            volumes: None,
            volume_mounts: None,
            log_level: None,
        });

        let service = build_service("test", "test-ns", &instance, None);
        let ports = service.spec.unwrap().ports.unwrap();

        let http_port = ports
            .iter()
            .find(|p| p.name.as_deref() == Some("http"))
            .unwrap();

        assert_eq!(http_port.port, 80, "External port should always be 80");
        // Target port should be custom value
        assert_eq!(
            http_port.target_port.as_ref().map(|p| match p {
                k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(i) => i,
                _ => &0,
            }),
            Some(&9090),
            "Custom target port should be 9090"
        );
    }

    #[test]
    fn test_deployment_includes_api_sidecar() {
        // Test that deployment includes the API sidecar container
        let instance = create_test_instance("test");

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let containers = pod_spec.containers;

        assert_eq!(
            containers.len(),
            2,
            "Should have 2 containers: BIND9 and API sidecar"
        );

        // Check BIND9 container
        let bind9_container = containers.iter().find(|c| c.name == "bind9");
        assert!(bind9_container.is_some(), "BIND9 container should exist");

        // Check API sidecar container
        let api_container = containers.iter().find(|c| c.name == "api");
        assert!(
            api_container.is_some(),
            "API sidecar container should exist"
        );

        let api = api_container.unwrap();
        assert!(
            api.image.as_ref().unwrap().contains("bindcar"),
            "API container should use bindcar image"
        );

        // Verify API container has correct port
        let api_ports = api.ports.as_ref().unwrap();
        assert_eq!(api_ports.len(), 1);
        assert_eq!(api_ports[0].name.as_deref(), Some("http"));
        assert_eq!(
            api_ports[0].container_port, 8080,
            "API container port should be 8080"
        );
    }

    #[test]
    fn test_api_sidecar_shares_volumes() {
        // Test that API sidecar mounts the same volumes as BIND9
        let instance = create_test_instance("test");

        let deployment = build_deployment("test", "test-ns", &instance, None, None);
        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let containers = pod_spec.containers;

        let api_container = containers.iter().find(|c| c.name == "api").unwrap();
        let volume_mounts = api_container.volume_mounts.as_ref().unwrap();

        // Verify cache volume is mounted
        let cache_mount = volume_mounts.iter().find(|m| m.name == "cache");
        assert!(cache_mount.is_some(), "API should mount cache volume");
        assert_eq!(
            cache_mount.unwrap().mount_path,
            "/var/cache/bind",
            "Cache should be mounted at /var/cache/bind"
        );

        // Verify rndc-key volume is mounted (read-only)
        let key_mount = volume_mounts.iter().find(|m| m.name == "rndc-key");
        assert!(key_mount.is_some(), "API should mount rndc-key volume");
        assert_eq!(
            key_mount.unwrap().mount_path,
            "/etc/bind/keys",
            "RNDC keys should be mounted at /etc/bind/keys"
        );
        assert_eq!(
            key_mount.unwrap().read_only,
            Some(true),
            "RNDC key volume should be read-only for API"
        );
    }
}
