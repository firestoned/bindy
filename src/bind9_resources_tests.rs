//! Unit tests for bind9_resources

#[cfg(test)]
mod tests {
    use crate::bind9_resources::{build_configmap, build_deployment, build_labels, build_service};
    use crate::crd::{Bind9Config, Bind9Instance, Bind9InstanceSpec, DNSSECConfig};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn create_test_instance(name: &str) -> Bind9Instance {
        Bind9Instance {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some("test-ns".into()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                replicas: Some(2),
                version: Some("9.18".into()),
                config: Some(Bind9Config {
                    recursion: Some(false),
                    allow_query: Some(vec!["0.0.0.0/0".into()]),
                    allow_transfer: Some(vec!["10.0.0.0/8".into()]),
                    dnssec: Some(DNSSECConfig {
                        enabled: Some(true),
                        validation: Some(true),
                    }),
                    forwarders: None,
                    listen_on: None,
                    listen_on_v6: None,
                }),
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
        assert_eq!(labels.get("app.kubernetes.io/managed-by").unwrap(), "bindy");
    }

    #[test]
    fn test_build_configmap() {
        let instance = create_test_instance("test");
        let cm = build_configmap("test", "test-ns", &instance);

        assert_eq!(cm.metadata.name.as_deref(), Some("test-config"));
        assert_eq!(cm.metadata.namespace.as_deref(), Some("test-ns"));

        let data = cm.data.unwrap();
        assert!(data.contains_key("named.conf"));
        assert!(data.contains_key("named.conf.options"));

        let options = data.get("named.conf.options").unwrap();
        assert!(options.contains("recursion no"));
        assert!(options.contains("allow-query { 0.0.0.0/0; }"));
        assert!(options.contains("allow-transfer { 10.0.0.0/8; }"));
        assert!(options.contains("dnssec-enable yes"));
        assert!(options.contains("dnssec-validation yes"));
    }

    #[test]
    fn test_build_deployment() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance);

        assert_eq!(deployment.metadata.name.as_deref(), Some("test"));
        assert_eq!(deployment.metadata.namespace.as_deref(), Some("test-ns"));

        let spec = deployment.spec.unwrap();
        assert_eq!(spec.replicas, Some(2));

        let pod_spec = spec.template.spec.unwrap();
        assert_eq!(pod_spec.containers.len(), 1);

        let container = &pod_spec.containers[0];
        assert_eq!(container.name, "bind9");
        assert_eq!(
            container.image.as_deref(),
            Some("internetsystemsconsortium/bind9:9.18")
        );

        let ports = container.ports.as_ref().unwrap();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].container_port, 53);
        assert_eq!(ports[1].container_port, 53);
    }

    #[test]
    fn test_build_service() {
        let service = build_service("test", "test-ns");

        assert_eq!(service.metadata.name.as_deref(), Some("test"));
        assert_eq!(service.metadata.namespace.as_deref(), Some("test-ns"));

        let spec = service.spec.unwrap();
        assert_eq!(spec.type_.as_deref(), Some("ClusterIP"));

        let ports = spec.ports.unwrap();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].port, 53);
        assert_eq!(ports[1].port, 53);
    }

    #[test]
    fn test_build_pod_spec() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance);
        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();

        // Verify volumes
        let volumes = pod_spec.volumes.unwrap();
        assert_eq!(volumes.len(), 3);
        assert_eq!(volumes[0].name, "config");
        assert_eq!(volumes[1].name, "zones");
        assert_eq!(volumes[2].name, "cache");

        // Verify container configuration
        let container = &pod_spec.containers[0];
        assert_eq!(container.name, "bind9");

        // Verify volume mounts
        let mounts = container.volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 4);
        assert!(mounts.iter().any(|m| m.name == "config"));
        assert!(mounts.iter().any(|m| m.name == "zones"));
        assert!(mounts.iter().any(|m| m.name == "cache"));

        // Verify probes
        assert!(container.liveness_probe.is_some());
        assert!(container.readiness_probe.is_some());
    }

    #[test]
    fn test_build_deployment_replicas() {
        let mut instance = create_test_instance("test");
        instance.spec.replicas = Some(5);

        let deployment = build_deployment("test", "test-ns", &instance);
        let spec = deployment.spec.unwrap();

        assert_eq!(spec.replicas, Some(5));
    }

    #[test]
    fn test_build_deployment_version() {
        let mut instance = create_test_instance("test");
        instance.spec.version = Some("9.20".into());

        let deployment = build_deployment("test", "test-ns", &instance);
        let pod_spec = deployment.spec.unwrap().template.spec.unwrap();
        let container = &pod_spec.containers[0];

        assert_eq!(
            container.image.as_deref(),
            Some("internetsystemsconsortium/bind9:9.20")
        );
    }

    #[test]
    fn test_build_service_ports() {
        let service = build_service("test", "test-ns");
        let spec = service.spec.unwrap();
        let ports = spec.ports.unwrap();

        assert_eq!(ports.len(), 2);

        // Check TCP port
        let tcp_port = ports
            .iter()
            .find(|p| p.name.as_deref() == Some("dns-tcp"))
            .unwrap();
        assert_eq!(tcp_port.port, 53);
        assert_eq!(tcp_port.protocol.as_deref(), Some("TCP"));

        // Check UDP port
        let udp_port = ports
            .iter()
            .find(|p| p.name.as_deref() == Some("dns-udp"))
            .unwrap();
        assert_eq!(udp_port.port, 53);
        assert_eq!(udp_port.protocol.as_deref(), Some("UDP"));
    }

    #[test]
    fn test_configmap_contains_all_files() {
        let instance = create_test_instance("test");
        let cm = build_configmap("test", "test-ns", &instance);

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
        let cm = build_configmap("test-instance", "test-ns", &instance);

        assert_eq!(cm.metadata.name.as_deref(), Some("test-instance-config"));
        assert_eq!(cm.metadata.namespace.as_deref(), Some("test-ns"));
    }

    #[test]
    fn test_deployment_selector_matches_labels() {
        let instance = create_test_instance("test");
        let deployment = build_deployment("test", "test-ns", &instance);

        let spec = deployment.spec.unwrap();
        let selector_labels = spec.selector.match_labels.unwrap();
        let pod_labels = spec.template.metadata.unwrap().labels.unwrap();

        // Verify selector matches pod labels
        for (key, value) in selector_labels.iter() {
            assert_eq!(pod_labels.get(key), Some(value));
        }
    }

    #[test]
    fn test_service_selector_matches_deployment() {
        let service = build_service("test", "test-ns");
        let deployment_labels = build_labels("test");

        let svc_selector = service.spec.unwrap().selector.unwrap();

        // Verify service selector matches deployment labels
        for (key, value) in svc_selector.iter() {
            assert_eq!(deployment_labels.get(key), Some(value));
        }
    }
}
