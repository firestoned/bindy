// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests {
    use crate::bootstrap::{
        build_all_crds, build_cluster_role_binding, build_deployment, build_kubeconfig_yaml,
        build_mc_kubeconfig_secret, build_mc_sa_token_secret, build_mc_service_account,
        build_mc_writer_role, build_mc_writer_role_binding, build_namespace,
        build_scout_cluster_role, build_scout_cluster_role_binding, build_scout_deployment,
        build_scout_service_account, build_scout_writer_role, build_scout_writer_role_binding,
        build_service_account, parse_cluster_role, resolve_image, ScoutDeploymentOptions,
        BINDY_ADMIN_ROLE_YAML, BINDY_ROLE_YAML, CLUSTER_ROLE_BINDING_NAME, DEFAULT_IMAGE_TAG,
        DEFAULT_NAMESPACE, DEFAULT_SCOUT_CLUSTER_NAME, MC_DEFAULT_SERVICE_ACCOUNT_NAME,
        OPERATOR_DEPLOYMENT_NAME, OPERATOR_IMAGE_BASE, OPERATOR_ROLE_NAME,
        REMOTE_KUBECONFIG_SECRET_SUFFIX, REMOTE_KUBECONFIG_SECRET_TYPE, SA_TOKEN_SECRET_SUFFIX,
        SCOUT_CLUSTER_ROLE_BINDING_NAME, SCOUT_CLUSTER_ROLE_NAME, SCOUT_DEPLOYMENT_NAME,
        SCOUT_SERVICE_ACCOUNT_NAME, SCOUT_WRITER_ROLE_BINDING_NAME, SCOUT_WRITER_ROLE_NAME,
        SERVICE_ACCOUNT_NAME,
    };

    /// Convenience helper: build a minimal `ScoutDeploymentOptions` for tests.
    fn scout_opts<'a>(
        image_tag: &'a str,
        registry: Option<&'a str>,
        cluster_name: &'a str,
        default_ips: &'a [String],
        default_zone: Option<&'a str>,
        remote_secret: Option<&'a str>,
    ) -> ScoutDeploymentOptions<'a> {
        ScoutDeploymentOptions {
            image_tag,
            registry,
            cluster_name,
            default_ips,
            default_zone,
            remote_secret,
        }
    }

    // --- Namespace ---

    #[test]
    fn test_build_namespace_sets_name() {
        let ns = build_namespace("foo");
        assert_eq!(ns.metadata.name.as_deref(), Some("foo"));
    }

    #[test]
    fn test_build_namespace_sets_metadata_label() {
        let ns = build_namespace("foo");
        let labels = ns.metadata.labels.unwrap();
        assert_eq!(
            labels
                .get("kubernetes.io/metadata.name")
                .map(String::as_str),
            Some("foo")
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
        let sa = build_service_account("foo");
        assert_eq!(sa.metadata.name.as_deref(), Some(SERVICE_ACCOUNT_NAME));
    }

    #[test]
    fn test_build_service_account_namespace() {
        let sa = build_service_account("bar");
        assert_eq!(sa.metadata.namespace.as_deref(), Some("bar"));
    }

    #[test]
    fn test_build_service_account_has_app_labels() {
        let sa = build_service_account("foobar");
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
        let crb = build_cluster_role_binding("foo");
        assert_eq!(
            crb.metadata.name.as_deref(),
            Some(CLUSTER_ROLE_BINDING_NAME)
        );
    }

    #[test]
    fn test_build_cluster_role_binding_references_operator_role() {
        let crb = build_cluster_role_binding("foo");
        assert_eq!(crb.role_ref.name, OPERATOR_ROLE_NAME);
        assert_eq!(crb.role_ref.kind, "ClusterRole");
        assert_eq!(crb.role_ref.api_group, "rbac.authorization.k8s.io");
    }

    #[test]
    fn test_build_cluster_role_binding_subject_namespace() {
        let crb = build_cluster_role_binding("baz");
        let subjects = crb.subjects.unwrap();
        let subject = subjects.first().unwrap();
        assert_eq!(subject.namespace.as_deref(), Some("baz"));
        assert_eq!(subject.name, SERVICE_ACCOUNT_NAME);
        assert_eq!(subject.kind, "ServiceAccount");
    }

    #[test]
    fn test_build_cluster_role_binding_custom_namespace_propagates() {
        let crb = build_cluster_role_binding("bar");
        let subject = crb.subjects.unwrap().into_iter().next().unwrap();
        assert_eq!(subject.namespace.as_deref(), Some("bar"));
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
        let d = build_deployment("foo", "latest", None).unwrap();
        assert_eq!(d.metadata.name.as_deref(), Some(OPERATOR_DEPLOYMENT_NAME));
    }

    #[test]
    fn test_build_deployment_namespace() {
        let d = build_deployment("bar", "latest", None).unwrap();
        assert_eq!(d.metadata.namespace.as_deref(), Some("bar"));
    }

    #[test]
    fn test_build_deployment_image_tag() {
        let d = build_deployment("foo", "v0.5.0", None).unwrap();
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
        let d = build_deployment("foo", "latest", None).unwrap();
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

    // --- Scout ServiceAccount ---

    #[test]
    fn test_build_scout_service_account_name() {
        let sa = build_scout_service_account("foo");
        assert_eq!(
            sa.metadata.name.as_deref(),
            Some(SCOUT_SERVICE_ACCOUNT_NAME)
        );
    }

    #[test]
    fn test_build_scout_service_account_namespace() {
        let sa = build_scout_service_account("bar");
        assert_eq!(sa.metadata.namespace.as_deref(), Some("bar"));
    }

    #[test]
    fn test_build_scout_service_account_has_app_labels() {
        let sa = build_scout_service_account("foobar");
        let labels = sa.metadata.labels.unwrap();
        assert_eq!(
            labels.get("app.kubernetes.io/name").map(String::as_str),
            Some("bindy")
        );
        assert_eq!(
            labels
                .get("app.kubernetes.io/component")
                .map(String::as_str),
            Some("scout")
        );
    }

    // --- Scout ClusterRole ---

    #[test]
    fn test_build_scout_cluster_role_name() {
        let role = build_scout_cluster_role();
        assert_eq!(role.metadata.name.as_deref(), Some(SCOUT_CLUSTER_ROLE_NAME));
    }

    #[test]
    fn test_build_scout_cluster_role_has_rules() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();
        assert!(
            !rules.is_empty(),
            "Scout ClusterRole must have at least one rule"
        );
    }

    #[test]
    fn test_build_scout_cluster_role_covers_ingresses() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();
        let has_ingress_rule = rules.iter().any(|r| {
            r.resources
                .as_ref()
                .is_some_and(|res| res.iter().any(|s| s == "ingresses"))
        });
        assert!(
            has_ingress_rule,
            "Scout ClusterRole must include an ingresses rule"
        );
    }

    #[test]
    fn test_build_scout_cluster_role_ingresses_allows_patch_and_update() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();
        let ingress_rule = rules
            .iter()
            .find(|r| {
                r.resources
                    .as_ref()
                    .is_some_and(|res| res.iter().any(|s| s == "ingresses"))
            })
            .expect("ingresses rule must exist");
        assert!(
            ingress_rule.verbs.contains(&"patch".to_string()),
            "ingresses rule must include 'patch' (required by kube-rs finalizer)"
        );
        assert!(
            ingress_rule.verbs.contains(&"update".to_string()),
            "ingresses rule must include 'update'"
        );
    }

    #[test]
    fn test_build_scout_cluster_role_covers_services() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();
        let has_services_rule = rules.iter().any(|r| {
            r.api_groups
                .as_ref()
                .is_some_and(|g| g.iter().any(|s| s.is_empty()))
                && r.resources
                    .as_ref()
                    .is_some_and(|res| res.iter().any(|s| s == "services"))
        });
        assert!(
            has_services_rule,
            "ClusterRole must include a services rule"
        );
    }

    #[test]
    fn test_build_scout_cluster_role_services_allows_patch_and_update() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();
        let svc_rule = rules
            .iter()
            .find(|r| {
                r.resources
                    .as_ref()
                    .is_some_and(|res| res.iter().any(|s| s == "services"))
            })
            .expect("services rule must exist");
        assert!(
            svc_rule.verbs.contains(&"patch".to_string()),
            "services rule must include 'patch' (required for finalizer management)"
        );
        assert!(
            svc_rule.verbs.contains(&"update".to_string()),
            "services rule must include 'update'"
        );
    }

    // --- Scout ClusterRoleBinding ---

    #[test]
    fn test_build_scout_cluster_role_binding_name() {
        let crb = build_scout_cluster_role_binding("foo");
        assert_eq!(
            crb.metadata.name.as_deref(),
            Some(SCOUT_CLUSTER_ROLE_BINDING_NAME)
        );
    }

    #[test]
    fn test_build_scout_cluster_role_binding_references_scout_role() {
        let crb = build_scout_cluster_role_binding("foo");
        assert_eq!(crb.role_ref.name, SCOUT_CLUSTER_ROLE_NAME);
        assert_eq!(crb.role_ref.kind, "ClusterRole");
    }

    #[test]
    fn test_build_scout_cluster_role_binding_subject_namespace() {
        let crb = build_scout_cluster_role_binding("baz");
        let subjects = crb.subjects.unwrap();
        let subject = subjects.first().unwrap();
        assert_eq!(subject.namespace.as_deref(), Some("baz"));
        assert_eq!(subject.name, SCOUT_SERVICE_ACCOUNT_NAME);
        assert_eq!(subject.kind, "ServiceAccount");
    }

    // --- Scout writer Role ---

    #[test]
    fn test_build_scout_writer_role_name() {
        let role = build_scout_writer_role("foo");
        assert_eq!(role.metadata.name.as_deref(), Some(SCOUT_WRITER_ROLE_NAME));
    }

    #[test]
    fn test_build_scout_writer_role_namespace() {
        let role = build_scout_writer_role("bar");
        assert_eq!(role.metadata.namespace.as_deref(), Some("bar"));
    }

    #[test]
    fn test_build_scout_writer_role_grants_arecords() {
        let role = build_scout_writer_role("foo");
        let rules = role.rules.unwrap();
        let has_arecord_rule = rules.iter().any(|r| {
            r.resources
                .as_ref()
                .is_some_and(|res| res.iter().any(|s| s == "arecords"))
        });
        assert!(
            has_arecord_rule,
            "Scout writer Role must include an arecords rule"
        );
    }

    // --- Scout writer RoleBinding ---

    #[test]
    fn test_build_scout_writer_role_binding_name() {
        let rb = build_scout_writer_role_binding("foo");
        assert_eq!(
            rb.metadata.name.as_deref(),
            Some(SCOUT_WRITER_ROLE_BINDING_NAME)
        );
    }

    #[test]
    fn test_build_scout_writer_role_binding_namespace() {
        let rb = build_scout_writer_role_binding("bar");
        assert_eq!(rb.metadata.namespace.as_deref(), Some("bar"));
    }

    #[test]
    fn test_build_scout_writer_role_binding_references_writer_role() {
        let rb = build_scout_writer_role_binding("foo");
        assert_eq!(rb.role_ref.name, SCOUT_WRITER_ROLE_NAME);
        assert_eq!(rb.role_ref.kind, "Role");
    }

    #[test]
    fn test_build_scout_writer_role_binding_subject_is_scout_sa() {
        let rb = build_scout_writer_role_binding("baz");
        let subjects = rb.subjects.unwrap();
        let subject = subjects.first().unwrap();
        assert_eq!(subject.name, SCOUT_SERVICE_ACCOUNT_NAME);
        assert_eq!(subject.namespace.as_deref(), Some("baz"));
    }

    // --- Scout Deployment ---

    fn scout_container_args(d: k8s_openapi::api::apps::v1::Deployment) -> Vec<String> {
        d.spec
            .unwrap()
            .template
            .spec
            .unwrap()
            .containers
            .into_iter()
            .next()
            .unwrap()
            .args
            .unwrap()
    }

    #[test]
    fn test_build_scout_deployment_name() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
        assert_eq!(d.metadata.name.as_deref(), Some(SCOUT_DEPLOYMENT_NAME));
    }

    #[test]
    fn test_build_scout_deployment_namespace() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("bar", &opts).unwrap();
        assert_eq!(d.metadata.namespace.as_deref(), Some("bar"));
    }

    #[test]
    fn test_build_scout_deployment_image_tag() {
        let opts = scout_opts("v0.5.0", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
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
    fn test_build_scout_deployment_uses_scout_service_account() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
        let sa_name = d
            .spec
            .unwrap()
            .template
            .spec
            .unwrap()
            .service_account_name
            .unwrap();
        assert_eq!(sa_name, SCOUT_SERVICE_ACCOUNT_NAME);
    }

    #[test]
    fn test_build_scout_deployment_args_default_cluster_name() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
        let args = scout_container_args(d);
        assert_eq!(
            args,
            vec!["scout", "--cluster-name", DEFAULT_SCOUT_CLUSTER_NAME]
        );
    }

    #[test]
    fn test_build_scout_deployment_args_custom_cluster_name() {
        let opts = scout_opts("latest", None, "prod-cluster", &[], None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
        let args = scout_container_args(d);
        assert_eq!(args, vec!["scout", "--cluster-name", "prod-cluster"]);
    }

    #[test]
    fn test_build_scout_deployment_args_with_default_ips() {
        let ips = vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()];
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &ips, None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
        let args = scout_container_args(d);
        assert_eq!(
            args,
            vec![
                "scout",
                "--cluster-name",
                DEFAULT_SCOUT_CLUSTER_NAME,
                "--default-ips",
                "10.0.0.1,10.0.0.2"
            ]
        );
    }

    #[test]
    fn test_build_scout_deployment_args_with_default_zone() {
        let opts = scout_opts(
            "latest",
            None,
            DEFAULT_SCOUT_CLUSTER_NAME,
            &[],
            Some("example.com"),
            None,
        );
        let d = build_scout_deployment("foo", &opts).unwrap();
        let args = scout_container_args(d);
        assert_eq!(
            args,
            vec![
                "scout",
                "--cluster-name",
                DEFAULT_SCOUT_CLUSTER_NAME,
                "--default-zone",
                "example.com"
            ]
        );
    }

    #[test]
    fn test_build_scout_deployment_args_all_options() {
        let ips = vec!["192.168.1.1".to_string()];
        let opts = scout_opts(
            "latest",
            None,
            "staging",
            &ips,
            Some("internal.example.com"),
            None,
        );
        let d = build_scout_deployment("foo", &opts).unwrap();
        let args = scout_container_args(d);
        assert_eq!(
            args,
            vec![
                "scout",
                "--cluster-name",
                "staging",
                "--default-ips",
                "192.168.1.1",
                "--default-zone",
                "internal.example.com"
            ]
        );
    }

    #[test]
    fn test_build_scout_deployment_no_default_ips_omits_flag() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
        let args = scout_container_args(d);
        assert!(!args.contains(&"--default-ips".to_string()));
    }

    #[test]
    fn test_build_scout_deployment_no_default_zone_omits_flag() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
        let args = scout_container_args(d);
        assert!(!args.contains(&"--default-zone".to_string()));
    }

    // --- resolve_image ---

    #[test]
    fn test_resolve_image_no_registry_uses_default() {
        assert_eq!(
            resolve_image(None, "v0.5.0"),
            format!("{OPERATOR_IMAGE_BASE}:v0.5.0")
        );
    }

    #[test]
    fn test_resolve_image_no_registry_latest() {
        assert_eq!(
            resolve_image(None, "latest"),
            format!("{OPERATOR_IMAGE_BASE}:latest")
        );
    }

    #[test]
    fn test_resolve_image_custom_registry() {
        assert_eq!(
            resolve_image(Some("my.registry.io/org"), "v0.5.0"),
            "my.registry.io/org/bindy:v0.5.0"
        );
    }

    #[test]
    fn test_resolve_image_custom_registry_trailing_slash_stripped() {
        assert_eq!(
            resolve_image(Some("my.registry.io/org/"), "v0.5.0"),
            "my.registry.io/org/bindy:v0.5.0"
        );
    }

    #[test]
    fn test_resolve_image_simple_registry() {
        assert_eq!(
            resolve_image(Some("registry.example.com"), "latest"),
            "registry.example.com/bindy:latest"
        );
    }

    // --- Operator deployment with registry override ---

    #[test]
    fn test_build_deployment_custom_registry() {
        let d = build_deployment("foo", "v0.5.0", Some("my.reg.io/mirror")).unwrap();
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
        assert_eq!(image, "my.reg.io/mirror/bindy:v0.5.0");
    }

    // --- Scout deployment with registry override ---

    #[test]
    fn test_build_scout_deployment_custom_registry() {
        let opts = scout_opts(
            "v0.5.0",
            Some("my.reg.io/mirror"),
            DEFAULT_SCOUT_CLUSTER_NAME,
            &[],
            None,
            None,
        );
        let d = build_scout_deployment("foo", &opts).unwrap();
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
        assert_eq!(image, "my.reg.io/mirror/bindy:v0.5.0");
    }

    // --- Multi-cluster (MC) ServiceAccount ---

    #[test]
    fn test_build_mc_service_account_name() {
        let sa = build_mc_service_account("bindy-system", "scout");
        assert_eq!(sa.metadata.name.as_deref(), Some("scout"));
    }

    #[test]
    fn test_build_mc_service_account_custom_name() {
        let sa = build_mc_service_account("bindy-system", "remote-writer");
        assert_eq!(sa.metadata.name.as_deref(), Some("remote-writer"));
    }

    #[test]
    fn test_build_mc_service_account_namespace() {
        let sa = build_mc_service_account("bindy-system", "scout");
        assert_eq!(sa.metadata.namespace.as_deref(), Some("bindy-system"));
    }

    #[test]
    fn test_build_mc_service_account_custom_namespace() {
        let sa = build_mc_service_account("my-ns", "scout");
        assert_eq!(sa.metadata.namespace.as_deref(), Some("my-ns"));
    }

    #[test]
    fn test_build_mc_service_account_has_app_name_label() {
        let sa = build_mc_service_account("bindy-system", "scout");
        let labels = sa.metadata.labels.unwrap();
        assert_eq!(
            labels.get("app.kubernetes.io/name").map(String::as_str),
            Some("bindy")
        );
    }

    #[test]
    fn test_build_mc_service_account_has_component_label() {
        let sa = build_mc_service_account("bindy-system", "scout");
        let labels = sa.metadata.labels.unwrap();
        assert_eq!(
            labels
                .get("app.kubernetes.io/component")
                .map(String::as_str),
            Some("scout-remote")
        );
    }

    // --- Multi-cluster (MC) writer Role ---

    #[test]
    fn test_build_mc_writer_role_name_matches_sa() {
        // Role name == SA name (mirrors remote-cluster-rbac.yaml convention)
        let role = build_mc_writer_role("bindy-system", "scout");
        assert_eq!(role.metadata.name.as_deref(), Some("scout"));
    }

    #[test]
    fn test_build_mc_writer_role_custom_sa_name() {
        let role = build_mc_writer_role("bindy-system", "bindy-scout-remote");
        assert_eq!(role.metadata.name.as_deref(), Some("bindy-scout-remote"));
    }

    #[test]
    fn test_build_mc_writer_role_namespace() {
        let role = build_mc_writer_role("bindy-system", "scout");
        assert_eq!(role.metadata.namespace.as_deref(), Some("bindy-system"));
    }

    #[test]
    fn test_build_mc_writer_role_grants_arecords() {
        let role = build_mc_writer_role("bindy-system", "scout");
        let rules = role.rules.unwrap();
        let has_arecord_rule = rules.iter().any(|r| {
            r.resources
                .as_ref()
                .is_some_and(|res| res.iter().any(|s| s == "arecords"))
        });
        assert!(
            has_arecord_rule,
            "MC writer Role must include an arecords rule"
        );
    }

    #[test]
    fn test_build_mc_writer_role_grants_write_verbs() {
        let role = build_mc_writer_role("bindy-system", "scout");
        let rules = role.rules.unwrap();
        let rule = rules.first().unwrap();
        let verbs = &rule.verbs;
        assert!(verbs.iter().any(|v| v == "create"), "must grant create");
        assert!(verbs.iter().any(|v| v == "update"), "must grant update");
        assert!(verbs.iter().any(|v| v == "patch"), "must grant patch");
        assert!(verbs.iter().any(|v| v == "delete"), "must grant delete");
        assert!(verbs.iter().any(|v| v == "get"), "must grant get");
    }

    #[test]
    fn test_build_mc_writer_role_uses_bindy_api_group() {
        let role = build_mc_writer_role("bindy-system", "scout");
        let rules = role.rules.unwrap();
        let rule = rules.first().unwrap();
        let groups = rule.api_groups.as_ref().unwrap();
        assert!(
            groups.iter().any(|g| g == "bindy.firestoned.io"),
            "MC writer Role must use bindy.firestoned.io API group"
        );
    }

    #[test]
    fn test_build_mc_writer_role_grants_dnszones_read() {
        // Scout watches DNSZones via Api::namespaced (not Api::all) so the namespaced
        // Role is sufficient — no ClusterRole required.
        let role = build_mc_writer_role("bindy-system", "scout");
        let rules = role.rules.unwrap();
        let has_dnszone_rule = rules.iter().any(|r| {
            r.resources
                .as_ref()
                .is_some_and(|res| res.iter().any(|s| s == "dnszones"))
        });
        assert!(
            has_dnszone_rule,
            "MC writer Role must include a dnszones rule for zone validation"
        );
    }

    #[test]
    fn test_build_mc_writer_role_dnszones_read_only() {
        let role = build_mc_writer_role("bindy-system", "scout");
        let rules = role.rules.unwrap();
        let dnszone_rule = rules
            .iter()
            .find(|r| {
                r.resources
                    .as_ref()
                    .is_some_and(|res| res.iter().any(|s| s == "dnszones"))
            })
            .expect("dnszones rule must exist");
        let verbs = &dnszone_rule.verbs;
        assert!(verbs.iter().any(|v| v == "get"), "dnszones: must allow get");
        assert!(
            verbs.iter().any(|v| v == "list"),
            "dnszones: must allow list"
        );
        assert!(
            verbs.iter().any(|v| v == "watch"),
            "dnszones: must allow watch"
        );
        assert!(
            !verbs.iter().any(|v| v == "create"),
            "dnszones: must NOT allow create"
        );
        assert!(
            !verbs.iter().any(|v| v == "delete"),
            "dnszones: must NOT allow delete"
        );
    }

    // --- Multi-cluster (MC) writer RoleBinding ---

    #[test]
    fn test_build_mc_writer_role_binding_name_matches_sa() {
        // RoleBinding name == SA name (mirrors remote-cluster-rbac.yaml convention)
        let rb = build_mc_writer_role_binding("bindy-system", "scout");
        assert_eq!(rb.metadata.name.as_deref(), Some("scout"));
    }

    #[test]
    fn test_build_mc_writer_role_binding_namespace() {
        let rb = build_mc_writer_role_binding("bindy-system", "scout");
        assert_eq!(rb.metadata.namespace.as_deref(), Some("bindy-system"));
    }

    #[test]
    fn test_build_mc_writer_role_binding_references_role_with_sa_name() {
        let rb = build_mc_writer_role_binding("bindy-system", "scout");
        assert_eq!(rb.role_ref.name, "scout");
        assert_eq!(rb.role_ref.kind, "Role");
        assert_eq!(rb.role_ref.api_group, "rbac.authorization.k8s.io");
    }

    #[test]
    fn test_build_mc_writer_role_binding_subject_name() {
        let rb = build_mc_writer_role_binding("bindy-system", "scout");
        let subjects = rb.subjects.unwrap();
        let subject = subjects.first().unwrap();
        assert_eq!(subject.name, "scout");
    }

    #[test]
    fn test_build_mc_writer_role_binding_subject_kind() {
        let rb = build_mc_writer_role_binding("bindy-system", "scout");
        let subjects = rb.subjects.unwrap();
        let subject = subjects.first().unwrap();
        assert_eq!(subject.kind, "ServiceAccount");
    }

    #[test]
    fn test_build_mc_writer_role_binding_subject_namespace() {
        let rb = build_mc_writer_role_binding("bindy-system", "scout");
        let subjects = rb.subjects.unwrap();
        let subject = subjects.first().unwrap();
        assert_eq!(subject.namespace.as_deref(), Some("bindy-system"));
    }

    // --- Multi-cluster SA token Secret ---

    #[test]
    fn test_build_mc_sa_token_secret_name() {
        let s = build_mc_sa_token_secret("bindy-system", "scout");
        let expected = format!("scout{SA_TOKEN_SECRET_SUFFIX}");
        assert_eq!(s.metadata.name.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn test_build_mc_sa_token_secret_custom_sa_name() {
        let s = build_mc_sa_token_secret("bindy-system", "remote-writer");
        let expected = format!("remote-writer{SA_TOKEN_SECRET_SUFFIX}");
        assert_eq!(s.metadata.name.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn test_build_mc_sa_token_secret_namespace() {
        let s = build_mc_sa_token_secret("bindy-system", "scout");
        assert_eq!(s.metadata.namespace.as_deref(), Some("bindy-system"));
    }

    #[test]
    fn test_build_mc_sa_token_secret_type() {
        let s = build_mc_sa_token_secret("bindy-system", "scout");
        assert_eq!(
            s.type_.as_deref(),
            Some("kubernetes.io/service-account-token")
        );
    }

    #[test]
    fn test_build_mc_sa_token_secret_annotation_references_sa() {
        let s = build_mc_sa_token_secret("bindy-system", "scout");
        let annotations = s.metadata.annotations.unwrap();
        assert_eq!(
            annotations
                .get("kubernetes.io/service-account.name")
                .map(String::as_str),
            Some("scout")
        );
    }

    #[test]
    fn test_build_mc_sa_token_secret_has_app_labels() {
        let s = build_mc_sa_token_secret("bindy-system", "scout");
        let labels = s.metadata.labels.unwrap();
        assert_eq!(
            labels.get("app.kubernetes.io/name").map(String::as_str),
            Some("bindy")
        );
        assert_eq!(
            labels
                .get("app.kubernetes.io/component")
                .map(String::as_str),
            Some("scout-remote")
        );
    }

    // --- Multi-cluster remote kubeconfig Secret ---

    #[test]
    fn test_build_mc_kubeconfig_secret_name() {
        let s = build_mc_kubeconfig_secret("bindy-system", "scout", "kubeconfig: content");
        let expected = format!("scout{REMOTE_KUBECONFIG_SECRET_SUFFIX}");
        assert_eq!(s.metadata.name.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn test_build_mc_kubeconfig_secret_namespace() {
        let s = build_mc_kubeconfig_secret("bindy-system", "scout", "kubeconfig: content");
        assert_eq!(s.metadata.namespace.as_deref(), Some("bindy-system"));
    }

    #[test]
    fn test_build_mc_kubeconfig_secret_type() {
        let s = build_mc_kubeconfig_secret("bindy-system", "scout", "kubeconfig: content");
        assert_eq!(s.type_.as_deref(), Some(REMOTE_KUBECONFIG_SECRET_TYPE));
    }

    #[test]
    fn test_build_mc_kubeconfig_secret_has_kubeconfig_key() {
        let s = build_mc_kubeconfig_secret("bindy-system", "scout", "kubeconfig: content");
        let data = s.data.unwrap();
        assert!(
            data.contains_key("kubeconfig"),
            "Secret must have 'kubeconfig' data key"
        );
    }

    #[test]
    fn test_build_mc_kubeconfig_secret_data_matches_input() {
        let yaml = "kubeconfig: content\n";
        let s = build_mc_kubeconfig_secret("bindy-system", "scout", yaml);
        let data = s.data.unwrap();
        let bytes = &data.get("kubeconfig").unwrap().0;
        assert_eq!(bytes, yaml.as_bytes());
    }

    #[test]
    fn test_build_mc_kubeconfig_secret_has_sa_label() {
        let s = build_mc_kubeconfig_secret("bindy-system", "scout", "kubeconfig: content");
        let labels = s.metadata.labels.unwrap();
        assert_eq!(
            labels
                .get("bindy.firestoned.io/service-account")
                .map(String::as_str),
            Some("scout")
        );
    }

    // --- build_kubeconfig_yaml ---

    #[test]
    fn test_build_kubeconfig_yaml_api_version() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed["apiVersion"].as_str(), Some("v1"));
    }

    #[test]
    fn test_build_kubeconfig_yaml_kind() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed["kind"].as_str(), Some("Config"));
    }

    #[test]
    fn test_build_kubeconfig_yaml_server() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["clusters"][0]["cluster"]["server"].as_str(),
            Some("https://example.com:6443")
        );
    }

    #[test]
    fn test_build_kubeconfig_yaml_token() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["users"][0]["user"]["token"].as_str(),
            Some("my-token")
        );
    }

    #[test]
    fn test_build_kubeconfig_yaml_cluster_name() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed["clusters"][0]["name"].as_str(), Some("my-cluster"));
    }

    #[test]
    fn test_build_kubeconfig_yaml_no_ca_sets_insecure_skip() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["clusters"][0]["cluster"]["insecure-skip-tls-verify"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn test_build_kubeconfig_yaml_with_ca_sets_ca_data() {
        let ca = "dGVzdGNh"; // base64 encoded "testca"
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            Some(ca),
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["clusters"][0]["cluster"]["certificate-authority-data"].as_str(),
            Some(ca)
        );
    }

    #[test]
    fn test_build_kubeconfig_yaml_with_ca_no_insecure_skip() {
        let ca = "dGVzdGNh";
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            Some(ca),
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert!(
            parsed["clusters"][0]["cluster"]["insecure-skip-tls-verify"].is_null(),
            "insecure-skip-tls-verify must not be set when CA is provided"
        );
    }

    #[test]
    fn test_build_kubeconfig_yaml_current_context_is_default() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed["current-context"].as_str(), Some("default"));
    }

    #[test]
    fn test_build_kubeconfig_yaml_user_name() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed["users"][0]["name"].as_str(), Some("scout"));
    }

    #[test]
    fn test_build_kubeconfig_yaml_context_references_cluster_and_user() {
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
        )
        .unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["contexts"][0]["context"]["cluster"].as_str(),
            Some("my-cluster")
        );
        assert_eq!(
            parsed["contexts"][0]["context"]["user"].as_str(),
            Some("scout")
        );
    }

    // -------------------------------------------------------------------------
    // run_revoke_multi_cluster — resource name derivation
    // -------------------------------------------------------------------------

    /// Verify the exact resource names that `run_revoke_multi_cluster` would delete
    /// for the default service account name.  If the suffix constants change, this
    /// test catches the drift before it reaches a cluster.
    #[test]
    fn test_revoke_mc_resource_names_default_sa() {
        // Arrange
        let sa = MC_DEFAULT_SERVICE_ACCOUNT_NAME;

        // Act — derive the same names the revoke function uses
        let token_secret = format!("{sa}{SA_TOKEN_SECRET_SUFFIX}");
        let kubeconfig_secret = format!("{sa}{REMOTE_KUBECONFIG_SECRET_SUFFIX}");

        // Assert
        assert_eq!(sa, "bindy-scout-remote");
        assert_eq!(token_secret, "bindy-scout-remote-token");
        assert_eq!(kubeconfig_secret, "bindy-scout-remote-remote-kubeconfig");
    }

    /// Verify resource names for a custom per-cluster service account name.
    #[test]
    fn test_revoke_mc_resource_names_custom_sa() {
        // Arrange
        let sa = "bindy-scout-prod-us-east";

        // Act
        let token_secret = format!("{sa}{SA_TOKEN_SECRET_SUFFIX}");
        let kubeconfig_secret = format!("{sa}{REMOTE_KUBECONFIG_SECRET_SUFFIX}");

        // Assert
        assert_eq!(token_secret, "bindy-scout-prod-us-east-token");
        assert_eq!(
            kubeconfig_secret,
            "bindy-scout-prod-us-east-remote-kubeconfig"
        );
    }
}
