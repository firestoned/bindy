// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the pure helpers of managed-instance reconciliation.

#[cfg(test)]
mod tests {
    use crate::bind9cluster::instances::{
        desired_managed_spec, desired_replicas, instance_index, managed_bindcar_config,
        managed_fields_differ, managed_instance_name, missing_indices, new_managed_instance_fields,
        scale_down_victims,
    };
    use crate::crd::{Bind9ClusterCommonSpec, Bind9Instance, ServerRole};
    use crate::labels::BINDY_INSTANCE_INDEX_ANNOTATION;
    use serde_json::json;

    const CLUSTER: &str = "prod-dns";

    fn common(value: serde_json::Value) -> Bind9ClusterCommonSpec {
        serde_json::from_value(value).expect("valid Bind9ClusterCommonSpec")
    }

    /// A managed instance named `name`, with `index` in its index annotation
    /// when given.
    fn instance(name: &str, role: &str, index: Option<&str>) -> Bind9Instance {
        let annotations = index.map_or_else(
            || json!({}),
            |i| json!({ BINDY_INSTANCE_INDEX_ANNOTATION: i }),
        );
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": { "name": name, "namespace": "dns", "annotations": annotations },
            "spec": { "clusterRef": CLUSTER, "role": role },
        }))
        .expect("valid Bind9Instance")
    }

    fn refs(instances: &[Bind9Instance]) -> Vec<&Bind9Instance> {
        instances.iter().collect()
    }

    // ------------------------------------------------------------------
    // desired_replicas / managed_instance_name / instance_index
    // ------------------------------------------------------------------

    #[test]
    fn test_desired_replicas_reads_each_role_block() {
        let spec = common(json!({
            "primary": { "replicas": 2 },
            "secondary": { "replicas": 3 },
        }));
        assert_eq!(desired_replicas(&spec, ServerRole::Primary), 2);
        assert_eq!(desired_replicas(&spec, ServerRole::Secondary), 3);
    }

    #[test]
    fn test_desired_replicas_zero_when_role_block_or_field_absent() {
        let spec = common(json!({ "primary": {} }));
        assert_eq!(desired_replicas(&spec, ServerRole::Primary), 0);
        assert_eq!(desired_replicas(&spec, ServerRole::Secondary), 0);
    }

    #[test]
    fn test_managed_instance_name_uses_role_label() {
        assert_eq!(
            managed_instance_name(CLUSTER, ServerRole::Primary, 0),
            "prod-dns-primary-0"
        );
        assert_eq!(
            managed_instance_name(CLUSTER, ServerRole::Secondary, 4),
            "prod-dns-secondary-4"
        );
    }

    #[test]
    fn test_instance_index_reads_annotation_and_defaults_to_zero() {
        assert_eq!(instance_index(&instance("a", "primary", Some("7"))), 7);
        assert_eq!(instance_index(&instance("b", "primary", None)), 0);
        assert_eq!(instance_index(&instance("c", "primary", Some("x"))), 0);
    }

    // ------------------------------------------------------------------
    // Scale-up
    // ------------------------------------------------------------------

    #[test]
    fn test_missing_indices_all_when_none_exist() {
        assert_eq!(
            missing_indices(CLUSTER, ServerRole::Primary, 3, &[]),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn test_missing_indices_skips_existing_names_only() {
        let existing = [
            instance("prod-dns-secondary-1", "secondary", Some("1")),
            // A name outside the desired range does not fill a gap
            instance("prod-dns-secondary-9", "secondary", Some("9")),
        ];
        assert_eq!(
            missing_indices(CLUSTER, ServerRole::Secondary, 3, &refs(&existing)),
            vec![0, 2]
        );
    }

    #[test]
    fn test_missing_indices_role_specific_names() {
        // A primary named for index 0 does not satisfy secondary index 0
        let existing = [instance("prod-dns-primary-0", "primary", Some("0"))];
        assert_eq!(
            missing_indices(CLUSTER, ServerRole::Secondary, 1, &refs(&existing)),
            vec![0]
        );
    }

    #[test]
    fn test_missing_indices_empty_when_desired_zero() {
        assert!(missing_indices(CLUSTER, ServerRole::Primary, 0, &[]).is_empty());
    }

    // ------------------------------------------------------------------
    // Scale-down
    // ------------------------------------------------------------------

    #[test]
    fn test_scale_down_victims_highest_index_first() {
        let existing = [
            instance("p-1", "primary", Some("1")),
            instance("p-3", "primary", Some("3")),
            instance("p-0", "primary", Some("0")),
            instance("p-2", "primary", Some("2")),
        ];
        assert_eq!(
            scale_down_victims(&refs(&existing), 2),
            vec!["p-3".to_string(), "p-2".to_string()]
        );
    }

    #[test]
    fn test_scale_down_victims_equal_indices_last_listed_first() {
        // Without an index annotation every instance sorts as index 0; the
        // previous ascending-sort-then-reverse took the last-listed first.
        let existing = [
            instance("a", "primary", None),
            instance("b", "primary", None),
            instance("c", "primary", None),
        ];
        assert_eq!(
            scale_down_victims(&refs(&existing), 1),
            vec!["c".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn test_scale_down_victims_none_when_at_or_below_desired() {
        let existing = [instance("p-0", "primary", Some("0"))];
        assert!(scale_down_victims(&refs(&existing), 1).is_empty());
        assert!(scale_down_victims(&refs(&existing), 3).is_empty());
    }

    // ------------------------------------------------------------------
    // Managed spec
    // ------------------------------------------------------------------

    fn cluster_common() -> Bind9ClusterCommonSpec {
        common(json!({
            "version": "9.20",
            "image": { "image": "bind9:9.20", "imagePullPolicy": "IfNotPresent" },
            "configMapRefs": { "namedConf": "my-named-conf" },
            "volumes": [{ "name": "extra", "emptyDir": {} }],
            "volumeMounts": [{ "name": "extra", "mountPath": "/extra" }],
            "global": { "bindcarConfig": { "image": "bindcar:v1" } },
        }))
    }

    #[test]
    fn test_new_managed_instance_spec_copies_cluster_fields() {
        let common = cluster_common();
        let spec = desired_managed_spec(
            &common,
            managed_bindcar_config(&common),
            &new_managed_instance_fields(CLUSTER, ServerRole::Secondary),
        );

        assert_eq!(spec.cluster_ref, CLUSTER);
        assert_eq!(spec.role, ServerRole::Secondary);
        assert_eq!(spec.replicas, Some(1));
        assert_eq!(spec.version, common.version);
        assert_eq!(spec.image, common.image);
        assert_eq!(spec.config_map_refs, common.config_map_refs);
        assert_eq!(spec.volumes, common.volumes);
        assert_eq!(spec.volume_mounts, common.volume_mounts);
        assert_eq!(
            spec.bindcar_config.as_ref(),
            managed_bindcar_config(&common)
        );
        assert!(spec.bindcar_config.is_some());
        assert!(spec.config.is_none());
        assert!(spec.primary_servers.is_none());
        assert!(spec.rndc_key.is_none());
        assert!(spec.storage.is_none());
        assert!(spec.placement.is_none());
        #[allow(deprecated)]
        let deprecated_ref = spec.rndc_secret_ref.is_none();
        assert!(deprecated_ref);
    }

    #[test]
    fn test_updated_managed_spec_preserves_instance_fields() {
        let common = cluster_common();
        let existing: Bind9Instance = serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": { "name": "prod-dns-primary-0", "namespace": "dns" },
            "spec": {
                "clusterRef": CLUSTER,
                "role": "primary",
                "replicas": 1,
                "version": "9.18",
                "config": { "recursion": false },
                "primaryServers": ["10.0.0.1"],
                "rndcKey": { "autoRotate": true, "rotateAfter": "30d" },
                "storage": {},
                "placement": { "spread": [] },
            },
        }))
        .expect("valid Bind9Instance");

        let spec = desired_managed_spec(&common, managed_bindcar_config(&common), &existing.spec);

        // Cluster-inherited fields follow the cluster, config is cleared
        assert_eq!(spec.version, common.version);
        assert_eq!(spec.image, common.image);
        assert_eq!(spec.config_map_refs, common.config_map_refs);
        assert_eq!(spec.volumes, common.volumes);
        assert_eq!(spec.volume_mounts, common.volume_mounts);
        assert_eq!(
            spec.bindcar_config.as_ref(),
            managed_bindcar_config(&common)
        );
        assert!(spec.config.is_none());
        // Instance-specific fields are kept
        assert_eq!(spec.cluster_ref, existing.spec.cluster_ref);
        assert_eq!(spec.role, existing.spec.role);
        assert_eq!(spec.replicas, existing.spec.replicas);
        assert_eq!(spec.primary_servers, existing.spec.primary_servers);
        assert_eq!(spec.rndc_key, existing.spec.rndc_key);
        assert!(spec.storage.is_some() && existing.spec.storage.is_some());
        assert_eq!(spec.placement, existing.spec.placement);
    }

    #[test]
    fn test_managed_fields_differ_false_once_converged() {
        let common = cluster_common();
        let bindcar = managed_bindcar_config(&common);
        let spec = desired_managed_spec(
            &common,
            bindcar,
            &new_managed_instance_fields(CLUSTER, ServerRole::Primary),
        );
        assert!(!managed_fields_differ(&spec, &common, bindcar));
    }

    #[test]
    fn test_managed_fields_differ_on_each_managed_field() {
        let common = cluster_common();
        let bindcar = managed_bindcar_config(&common);
        let converged = desired_managed_spec(
            &common,
            bindcar,
            &new_managed_instance_fields(CLUSTER, ServerRole::Primary),
        );

        let mut drifted = vec![];
        let mut s = converged.clone();
        s.version = Some("9.16".to_string());
        drifted.push(s);
        let mut s = converged.clone();
        s.image = None;
        drifted.push(s);
        let mut s = converged.clone();
        s.config_map_refs = None;
        drifted.push(s);
        let mut s = converged.clone();
        s.volumes = None;
        drifted.push(s);
        let mut s = converged.clone();
        s.volume_mounts = None;
        drifted.push(s);
        let mut s = converged.clone();
        s.bindcar_config = None;
        drifted.push(s);

        for spec in &drifted {
            assert!(managed_fields_differ(spec, &common, bindcar), "{spec:?}");
        }
    }

    #[test]
    fn test_managed_fields_differ_ignores_config_and_instance_fields() {
        // `config` is cleared on update but does not by itself trigger one
        let common = cluster_common();
        let bindcar = managed_bindcar_config(&common);
        let mut spec = desired_managed_spec(
            &common,
            bindcar,
            &new_managed_instance_fields(CLUSTER, ServerRole::Primary),
        );
        spec.config = serde_json::from_value(json!({ "recursion": true })).ok();
        spec.primary_servers = Some(vec!["10.0.0.9".to_string()]);
        spec.replicas = Some(1);
        assert!(spec.config.is_some());
        assert!(!managed_fields_differ(&spec, &common, bindcar));
    }
}
