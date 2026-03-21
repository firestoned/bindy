// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests {
    use crate::bootstrap::{
        build_all_crds, build_cluster_role_binding, build_deployment, build_namespace,
        build_service_account, parse_cluster_role, BINDY_ADMIN_ROLE_YAML, BINDY_ROLE_YAML,
        CLUSTER_ROLE_BINDING_NAME, DEFAULT_IMAGE_TAG, DEFAULT_NAMESPACE, OPERATOR_DEPLOYMENT_NAME,
        OPERATOR_IMAGE_BASE, OPERATOR_ROLE_NAME, SERVICE_ACCOUNT_NAME,
    };

    // --- Namespace ---

    #[test]
    fn test_build_namespace_sets_name() {
        let ns = build_namespace("test-system");
        assert_eq!(ns.metadata.name.as_deref(), Some("test-system"));
    }

    #[test]
    fn test_build_namespace_sets_metadata_label() {
        let ns = build_namespace("test-system");
        let labels = ns.metadata.labels.unwrap();
        assert_eq!(
            labels
                .get("kubernetes.io/metadata.name")
                .map(String::as_str),
            Some("test-system")
        );
    }

    #[test]
    fn test_build_namespace_default() {
        let ns = build_namespace(DEFAULT_NAMESPACE);
        assert_eq!(ns.metadata.name.as_deref(), Some("bindy-system"));
    }

    // --- ServiceAccount ---

    #[test]
    fn test_build_service_account_name() {
        let sa = build_service_account("any-ns");
        assert_eq!(sa.metadata.name.as_deref(), Some(SERVICE_ACCOUNT_NAME));
    }

    #[test]
    fn test_build_service_account_namespace() {
        let sa = build_service_account("custom-namespace");
        assert_eq!(sa.metadata.namespace.as_deref(), Some("custom-namespace"));
    }

    #[test]
    fn test_build_service_account_has_app_labels() {
        let sa = build_service_account("bindy-system");
        let labels = sa.metadata.labels.unwrap();
        assert_eq!(
            labels.get("app.kubernetes.io/name").map(String::as_str),
            Some("bindy")
        );
        assert_eq!(
            labels
                .get("app.kubernetes.io/component")
                .map(String::as_str),
            Some("rbac")
        );
    }

    // --- ClusterRoleBinding ---

    #[test]
    fn test_build_cluster_role_binding_name() {
        let crb = build_cluster_role_binding("bindy-system");
        assert_eq!(
            crb.metadata.name.as_deref(),
            Some(CLUSTER_ROLE_BINDING_NAME)
        );
    }

    #[test]
    fn test_build_cluster_role_binding_references_operator_role() {
        let crb = build_cluster_role_binding("bindy-system");
        assert_eq!(crb.role_ref.name, OPERATOR_ROLE_NAME);
        assert_eq!(crb.role_ref.kind, "ClusterRole");
        assert_eq!(crb.role_ref.api_group, "rbac.authorization.k8s.io");
    }

    #[test]
    fn test_build_cluster_role_binding_subject_namespace() {
        let crb = build_cluster_role_binding("my-namespace");
        let subjects = crb.subjects.unwrap();
        let subject = subjects.first().unwrap();
        assert_eq!(subject.namespace.as_deref(), Some("my-namespace"));
        assert_eq!(subject.name, SERVICE_ACCOUNT_NAME);
        assert_eq!(subject.kind, "ServiceAccount");
    }

    #[test]
    fn test_build_cluster_role_binding_custom_namespace_propagates() {
        let crb = build_cluster_role_binding("prod-dns");
        let subject = crb.subjects.unwrap().into_iter().next().unwrap();
        assert_eq!(subject.namespace.as_deref(), Some("prod-dns"));
    }

    // --- ClusterRole YAML parsing ---

    #[test]
    fn test_parse_operator_cluster_role_succeeds() {
        let role = parse_cluster_role(BINDY_ROLE_YAML).unwrap();
        assert_eq!(role.metadata.name.as_deref(), Some("bindy-role"));
    }

    #[test]
    fn test_parse_operator_cluster_role_has_rules() {
        let role = parse_cluster_role(BINDY_ROLE_YAML).unwrap();
        let rules = role.rules.unwrap();
        assert!(
            !rules.is_empty(),
            "Operator ClusterRole must have at least one rule"
        );
    }

    #[test]
    fn test_parse_admin_cluster_role_succeeds() {
        let role = parse_cluster_role(BINDY_ADMIN_ROLE_YAML).unwrap();
        assert_eq!(role.metadata.name.as_deref(), Some("bindy-admin-role"));
    }

    #[test]
    fn test_parse_invalid_yaml_returns_error() {
        let result = parse_cluster_role("this is: not: valid: yaml: !!!");
        assert!(result.is_err());
    }

    // --- Deployment ---

    #[test]
    fn test_build_deployment_name() {
        let d = build_deployment("bindy-system", "latest").unwrap();
        assert_eq!(d.metadata.name.as_deref(), Some(OPERATOR_DEPLOYMENT_NAME));
    }

    #[test]
    fn test_build_deployment_namespace() {
        let d = build_deployment("custom-ns", "latest").unwrap();
        assert_eq!(d.metadata.namespace.as_deref(), Some("custom-ns"));
    }

    #[test]
    fn test_build_deployment_image_tag() {
        let d = build_deployment("bindy-system", "v0.5.0").unwrap();
        let image = d
            .spec
            .unwrap()
            .template
            .spec
            .unwrap()
            .containers
            .into_iter()
            .next()
            .unwrap()
            .image
            .unwrap();
        assert_eq!(image, format!("{OPERATOR_IMAGE_BASE}:v0.5.0"));
    }

    #[test]
    fn test_build_deployment_latest_tag() {
        let d = build_deployment("bindy-system", "latest").unwrap();
        let image = d
            .spec
            .unwrap()
            .template
            .spec
            .unwrap()
            .containers
            .into_iter()
            .next()
            .unwrap()
            .image
            .unwrap();
        assert_eq!(image, format!("{OPERATOR_IMAGE_BASE}:latest"));
    }

    #[test]
    fn test_default_image_tag_is_nonempty() {
        assert!(!DEFAULT_IMAGE_TAG.is_empty());
    }

    #[test]
    fn test_default_image_tag_format() {
        // In debug builds it must be "latest"; in release builds it must start with "v"
        assert!(
            DEFAULT_IMAGE_TAG == "latest" || DEFAULT_IMAGE_TAG.starts_with('v'),
            "DEFAULT_IMAGE_TAG must be 'latest' or start with 'v', got: {DEFAULT_IMAGE_TAG}"
        );
    }

    // --- CRD generation ---

    #[test]
    fn test_build_all_crds_returns_twelve() {
        let crds = build_all_crds().unwrap();
        assert_eq!(crds.len(), 12, "Expected exactly 12 CRDs");
    }

    #[test]
    fn test_all_crds_have_names() {
        let crds = build_all_crds().unwrap();
        for crd in &crds {
            assert!(
                crd.metadata.name.is_some(),
                "Every CRD must have a name; got: {crd:?}"
            );
        }
    }

    #[test]
    fn test_all_crds_have_bindy_group() {
        let crds = build_all_crds().unwrap();
        for crd in &crds {
            let group = crd.spec.group.as_str();
            assert_eq!(
                group,
                "bindy.firestoned.io",
                "CRD {} has unexpected group",
                crd.metadata.name.as_deref().unwrap_or("unknown")
            );
        }
    }

    #[test]
    fn test_all_crds_have_storage_version() {
        let crds = build_all_crds().unwrap();
        for crd in &crds {
            let has_storage = crd.spec.versions.iter().any(|v| v.storage);
            assert!(
                has_storage,
                "CRD {} must have at least one storage version",
                crd.metadata.name.as_deref().unwrap_or("unknown")
            );
        }
    }

    #[test]
    fn test_crd_names_include_expected_resources() {
        let crds = build_all_crds().unwrap();
        let names: Vec<&str> = crds
            .iter()
            .filter_map(|c| c.metadata.name.as_deref())
            .collect();

        for expected in &[
            "arecords.bindy.firestoned.io",
            "dnszones.bindy.firestoned.io",
            "bind9instances.bindy.firestoned.io",
            "bind9clusters.bindy.firestoned.io",
            "clusterbind9providers.bindy.firestoned.io",
        ] {
            assert!(
                names.contains(expected),
                "Expected CRD {expected} not found in: {names:?}"
            );
        }
    }
}
