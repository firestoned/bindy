// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests {
    use crate::crd::{Bind9ClusterSpec, Bind9ClusterStatus};

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
        use crate::crd::{PrimaryConfig, SecondaryConfig};

        let spec = Bind9ClusterSpec {
            version: Some("9.18".to_string()),
            primary: Some(PrimaryConfig {
                replicas: Some(2),
                service: None,
                allow_transfer: None,
            }),
            secondary: Some(SecondaryConfig {
                replicas: Some(3),
                service: None,
                allow_transfer: None,
            }),
            image: None,
            config_map_refs: None,
            global: None,
            rndc_secret_refs: None,
            acls: None,
            volumes: None,
            volume_mounts: None,
        };

        assert_eq!(spec.version, Some("9.18".to_string()));
        assert_eq!(spec.primary.as_ref().unwrap().replicas, Some(2));
        assert_eq!(spec.secondary.as_ref().unwrap().replicas, Some(3));
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
}
