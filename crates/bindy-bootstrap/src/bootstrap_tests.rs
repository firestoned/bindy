// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
mod tests {
    use crate::bootstrap::{
        build_all_crds, build_cluster_role_binding, build_deployment, build_kubeconfig_yaml,
        build_mc_kubeconfig_secret, build_mc_sa_token_secret, build_mc_service_account,
        build_mc_writer_role, build_mc_writer_role_binding, build_namespace,
        build_scout_cluster_role, build_scout_cluster_role_binding, build_scout_deployment,
        build_scout_secrets_reader_role, build_scout_secrets_reader_role_binding,
        build_scout_service_account, build_scout_source_registry, build_scout_writer_role,
        build_scout_writer_role_binding, build_secrets_writer_role,
        build_secrets_writer_role_binding, build_service_account,
        build_tokenreview_cluster_role_binding, parse_cluster_role, resolve_image,
        scout_source_field_manager, ScoutDeploymentOptions, BINDCAR_TOKENREVIEW_CLUSTER_ROLE_YAML,
        BINDCAR_TOKENREVIEW_NAME, BINDCAR_TOKEN_EXPIRATION_SECONDS, BINDCAR_TOKEN_FILENAME,
        BINDCAR_TOKEN_MOUNT_PATH, BINDCAR_TOKEN_VOLUME_NAME, BINDY_ADMIN_ROLE_YAML,
        BINDY_ROLE_YAML, CLUSTER_ROLE_BINDING_NAME, DEFAULT_IMAGE_TAG, DEFAULT_NAMESPACE,
        DEFAULT_SCOUT_CLUSTER_NAME, MC_DEFAULT_SERVICE_ACCOUNT_NAME, OPERATOR_CLUSTER_ROLE_YAMLS,
        OPERATOR_DEPLOYMENT_NAME, OPERATOR_IMAGE_BASE, OPERATOR_ROLE_NAME, POD_NAMESPACE_ENV,
        REMOTE_KUBECONFIG_SECRET_SUFFIX, REMOTE_KUBECONFIG_SECRET_TYPE, SA_TOKEN_SECRET_SUFFIX,
        SCOUT_CLUSTER_ROLE_BINDING_NAME, SCOUT_CLUSTER_ROLE_NAME, SCOUT_DEPLOYMENT_NAME,
        SCOUT_SECRETS_READER_ROLE_BINDING_NAME, SCOUT_SECRETS_READER_ROLE_NAME,
        SCOUT_SERVICE_ACCOUNT_NAME, SCOUT_SOURCES_CLUSTERS_SUFFIX, SCOUT_SOURCES_CONFIGMAP_NAME,
        SCOUT_SOURCES_ZONES_SUFFIX, SCOUT_TARGET_NAMESPACE_ENV, SCOUT_WRITER_ROLE_BINDING_NAME,
        SCOUT_WRITER_ROLE_NAME, SECRETS_WRITER_ROLE_BINDING_NAME, SECRETS_WRITER_ROLE_NAME,
        SERVICE_ACCOUNT_NAME,
    };

    /// Namespace-scoped split of `BINDY_ROLE_YAML`: the irreducible
    /// cluster-scoped half, used when the operator runs with
    /// `BINDY_WATCH_NAMESPACES` set. Contains only `clusterbind9providers`,
    /// the one bindy kind with `scope: Cluster`. Bootstrap does not apply it;
    /// the tests below pin the static manifest.
    const BINDY_NAMESPACED_CLUSTER_ROLE_YAML: &str =
        include_str!("../../../deploy/operator/rbac/namespaced/clusterrole.yaml");

    /// Namespace-scoped split of `BINDY_ROLE_YAML`: the per-namespace half,
    /// applied once per watched namespace. Bootstrap does not apply it; the
    /// tests below pin the static manifest.
    const BINDY_NAMESPACED_ROLE_YAML: &str =
        include_str!("../../../deploy/operator/rbac/namespaced/role.yaml");

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
            allowed_zones: None,
            allowed_ip_ranges: None,
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

    // --- B-5: Secrets RBAC hardening ---

    /// The operator ClusterRole must only grant READ-ONLY access to Secrets
    /// cluster-wide. The mutating verbs live in the namespaced secrets-writer Role.
    #[test]
    fn test_operator_cluster_role_secrets_is_read_only() {
        let role = parse_cluster_role(BINDY_ROLE_YAML).unwrap();
        let rules = role.rules.unwrap();
        let secrets_rule = rules
            .iter()
            .find(|r| {
                r.resources
                    .as_ref()
                    .is_some_and(|res| res.iter().any(|x| x == "secrets"))
            })
            .expect("operator ClusterRole must have a secrets rule");

        for verb in &secrets_rule.verbs {
            assert!(
                matches!(verb.as_str(), "get" | "list" | "watch"),
                "operator ClusterRole secrets rule must be read-only, found verb: {verb}"
            );
        }
        for forbidden in ["create", "update", "patch", "delete"] {
            assert!(
                !secrets_rule.verbs.iter().any(|v| v == forbidden),
                "operator ClusterRole must NOT grant cluster-wide secrets '{forbidden}'"
            );
        }
    }

    // --- ADR-0017: the zones-loaded readiness gate ---

    /// The verbs `rules` grant on exactly `resource`.
    fn verbs_on(rules: &[k8s_openapi::api::rbac::v1::PolicyRule], resource: &str) -> Vec<String> {
        rules
            .iter()
            .filter(|r| {
                r.resources
                    .as_ref()
                    .is_some_and(|res| res.iter().any(|x| x == resource))
            })
            .flat_map(|r| r.verbs.clone())
            .collect()
    }

    /// The operator sets the zones-loaded condition through `pods/status`
    /// (get + patch), in the cluster-wide role and in the per-namespace Role
    /// of namespace-restricted mode, and nothing more on that subresource.
    #[test]
    fn test_operator_roles_grant_pod_status_patch_for_zones_loaded_gate() {
        for (name, yaml) in [
            ("role.yaml", BINDY_ROLE_YAML),
            ("namespaced/role.yaml", BINDY_NAMESPACED_ROLE_YAML),
        ] {
            let mut verbs = verbs_on(&parse_role_rules(yaml), "pods/status");
            verbs.sort();
            assert_eq!(
                verbs,
                vec!["get".to_string(), "patch".to_string()],
                "{name} must grant exactly get+patch on pods/status"
            );
        }
    }

    /// Writing a pod's status must not come with writing the pod: the
    /// operator stays read-only on `pods` (it cannot change a pod's spec or
    /// labels, or delete it).
    #[test]
    fn test_operator_roles_keep_pods_read_only() {
        for (name, yaml) in [
            ("role.yaml", BINDY_ROLE_YAML),
            ("namespaced/role.yaml", BINDY_NAMESPACED_ROLE_YAML),
        ] {
            for verb in verbs_on(&parse_role_rules(yaml), "pods") {
                assert!(
                    matches!(verb.as_str(), "get" | "list" | "watch"),
                    "{name} must keep pods read-only, found verb: {verb}"
                );
            }
        }
    }

    // --- P1-2: the namespace-scoped RBAC split must stay faithful ---

    /// The only bindy kind with `scope: Cluster`. Everything else is Namespaced and
    /// therefore belongs in the per-namespace Role.
    const CLUSTER_SCOPED_KINDS: [&str; 2] =
        ["clusterbind9providers", "clusterbind9providers/status"];

    fn parse_role_rules(yaml: &str) -> Vec<k8s_openapi::api::rbac::v1::PolicyRule> {
        let doc: serde_json::Value = serde_saphyr::from_str(yaml).expect("valid RBAC YAML");
        serde_json::from_value(doc["rules"].clone()).expect("rules parse")
    }

    fn rule_resources(rules: &[k8s_openapi::api::rbac::v1::PolicyRule]) -> Vec<String> {
        rules
            .iter()
            .flat_map(|r| r.resources.clone().unwrap_or_default())
            .collect()
    }

    /// The namespaced Role must not grant any cluster-scoped kind — a Role cannot
    /// authorize one, so such a rule would be silently dead and misleading.
    #[test]
    fn test_namespaced_role_grants_no_cluster_scoped_kind() {
        let resources = rule_resources(&parse_role_rules(BINDY_NAMESPACED_ROLE_YAML));
        for kind in CLUSTER_SCOPED_KINDS {
            assert!(
                !resources.iter().any(|r| r == kind),
                "namespaced Role must not grant cluster-scoped {kind}"
            );
        }
    }

    /// The slim ClusterRole must grant ONLY cluster-scoped kinds. Anything else here
    /// re-introduces exactly the cluster-wide access this split exists to remove —
    /// a `secrets` or `deployments` rule would silently undo audit findings H3 and C2.
    #[test]
    fn test_namespaced_clusterrole_grants_only_cluster_scoped_kinds() {
        let resources = rule_resources(&parse_role_rules(BINDY_NAMESPACED_CLUSTER_ROLE_YAML));
        assert!(
            !resources.is_empty(),
            "the slim ClusterRole must not be empty"
        );
        for r in &resources {
            assert!(
                CLUSTER_SCOPED_KINDS.contains(&r.as_str()),
                "slim ClusterRole must not grant {r} — only cluster-scoped kinds belong here"
            );
        }
    }

    /// The two halves together must cover every resource the cluster-wide role grants.
    /// Without this, adding a rule to role.yaml and forgetting the split would quietly
    /// drop a permission in scoped mode — visible only as a 403 at runtime.
    #[test]
    fn test_namespaced_split_covers_every_cluster_wide_resource() {
        let full: std::collections::BTreeSet<String> =
            rule_resources(&parse_role_rules(BINDY_ROLE_YAML))
                .into_iter()
                .collect();
        let mut split: std::collections::BTreeSet<String> =
            rule_resources(&parse_role_rules(BINDY_NAMESPACED_ROLE_YAML))
                .into_iter()
                .collect();
        split.extend(rule_resources(&parse_role_rules(
            BINDY_NAMESPACED_CLUSTER_ROLE_YAML,
        )));

        let missing: Vec<_> = full.difference(&split).collect();
        assert!(
            missing.is_empty(),
            "these resources are granted cluster-wide but missing from the namespaced split: {missing:?}"
        );
    }

    // --- P3-2 / least privilege: no `create` on user-authored kinds ---

    /// The operator never creates a ClusterBind9Provider, a DNSZone, or any
    /// record CR — those are authored by users/GitOps, or (for records) by Scout
    /// under its own `bindy-scout` identity. Granting `create` on them would let
    /// a compromised operator token manufacture DNS records cluster-wide, so the
    /// verb is deliberately absent. If a reconciler ever legitimately needs to
    /// create one of these, this test is the place to make that decision explicit.
    #[test]
    fn test_operator_cluster_role_cannot_create_user_authored_kinds() {
        let role = parse_cluster_role(BINDY_ROLE_YAML).unwrap();
        let rules = role.rules.unwrap();

        for kind in [
            "clusterbind9providers",
            "dnszones",
            "arecords",
            "aaaarecords",
            "txtrecords",
            "cnamerecords",
            "mxrecords",
            "nsrecords",
            "srvrecords",
            "caarecords",
            "ptrrecords",
        ] {
            let rule = rules
                .iter()
                .find(|r| {
                    r.resources
                        .as_ref()
                        .is_some_and(|res| res.iter().any(|x| x == kind))
                })
                .unwrap_or_else(|| panic!("operator ClusterRole must have a rule for {kind}"));

            for forbidden in ["create", "delete", "deletecollection"] {
                assert!(
                    !rule.verbs.iter().any(|v| v == forbidden),
                    "operator ClusterRole must NOT grant '{forbidden}' on {kind}"
                );
            }
        }
    }

    /// The operator DOES create Bind9Instances (Bind9Cluster scaling) and
    /// Bind9Clusters (from a ClusterBind9Provider), so those keep `create`.
    /// This guards the reduction above from being over-applied.
    #[test]
    fn test_operator_cluster_role_retains_create_on_operator_authored_kinds() {
        let role = parse_cluster_role(BINDY_ROLE_YAML).unwrap();
        let rules = role.rules.unwrap();

        for kind in ["bind9instances", "bind9clusters"] {
            let rule = rules
                .iter()
                .find(|r| {
                    r.resources
                        .as_ref()
                        .is_some_and(|res| res.iter().any(|x| x == kind))
                })
                .unwrap_or_else(|| panic!("operator ClusterRole must have a rule for {kind}"));

            assert!(
                rule.verbs.iter().any(|v| v == "create"),
                "operator ClusterRole must retain 'create' on {kind} — the operator authors these"
            );
        }
    }

    #[test]
    fn test_build_secrets_writer_role_name_and_namespace() {
        let role = build_secrets_writer_role("bindy-system");
        assert_eq!(
            role.metadata.name.as_deref(),
            Some(SECRETS_WRITER_ROLE_NAME)
        );
        assert_eq!(role.metadata.namespace.as_deref(), Some("bindy-system"));
    }

    #[test]
    fn test_build_secrets_writer_role_has_only_mutating_verbs() {
        let role = build_secrets_writer_role("bindy-system");
        let rules = role.rules.unwrap();
        let rule = rules.first().unwrap();
        assert!(rule
            .resources
            .as_ref()
            .unwrap()
            .iter()
            .any(|r| r == "secrets"));
        let mut verbs = rule.verbs.clone();
        verbs.sort();
        assert_eq!(verbs, vec!["create", "delete", "patch", "update"]);
        // Read verbs stay on the cluster-wide ClusterRole, not here.
        for read in ["get", "list", "watch"] {
            assert!(!rule.verbs.iter().any(|v| v == read));
        }
    }

    #[test]
    fn test_build_secrets_writer_role_binding_references_role() {
        let rb = build_secrets_writer_role_binding("bindy-system");
        assert_eq!(
            rb.metadata.name.as_deref(),
            Some(SECRETS_WRITER_ROLE_BINDING_NAME)
        );
        assert_eq!(rb.metadata.namespace.as_deref(), Some("bindy-system"));
        assert_eq!(rb.role_ref.kind, "Role");
        assert_eq!(rb.role_ref.name, SECRETS_WRITER_ROLE_NAME);
    }

    #[test]
    fn test_build_secrets_writer_role_binding_subject() {
        let rb = build_secrets_writer_role_binding("custom-ns");
        let subject = rb.subjects.unwrap().into_iter().next().unwrap();
        assert_eq!(subject.kind, "ServiceAccount");
        assert_eq!(subject.name, SERVICE_ACCOUNT_NAME);
        assert_eq!(subject.namespace.as_deref(), Some("custom-ns"));
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

    // --- bindcar 0.7.0 TokenReview (Mode B) support in the bootstrap path ---

    /// Convenience helper: extract the operator PodSpec from a built Deployment.
    fn operator_pod_spec(namespace: &str) -> k8s_openapi::api::core::v1::PodSpec {
        build_deployment(namespace, "latest", None)
            .unwrap()
            .spec
            .unwrap()
            .template
            .spec
            .unwrap()
    }

    #[test]
    fn test_build_deployment_has_bindcar_token_projected_volume() {
        let pod_spec = operator_pod_spec("foo");
        let volumes = pod_spec.volumes.expect("pod must have volumes");
        let volume = volumes
            .iter()
            .find(|v| v.name == BINDCAR_TOKEN_VOLUME_NAME)
            .expect("bindcar-token projected volume must exist");
        let sources = volume
            .projected
            .as_ref()
            .expect("bindcar-token volume must be projected")
            .sources
            .as_ref()
            .expect("projected volume must have sources");
        let sa_token = sources
            .iter()
            .find_map(|s| s.service_account_token.as_ref())
            .expect("projected volume must have a serviceAccountToken source");
        assert_eq!(
            sa_token.audience.as_deref(),
            Some(crate::constants::BINDCAR_TOKEN_AUDIENCE),
            "token audience must be `bindcar` so bindcar 0.7.0 accepts it"
        );
        assert_eq!(
            sa_token.expiration_seconds,
            Some(BINDCAR_TOKEN_EXPIRATION_SECONDS),
            "token expiration must use the named constant"
        );
        assert_eq!(sa_token.path, BINDCAR_TOKEN_FILENAME);
    }

    #[test]
    fn test_build_deployment_mounts_bindcar_token_read_only() {
        let pod_spec = operator_pod_spec("foo");
        let container = pod_spec.containers.into_iter().next().unwrap();
        let mounts = container.volume_mounts.expect("container must have mounts");
        let mount = mounts
            .iter()
            .find(|m| m.name == BINDCAR_TOKEN_VOLUME_NAME)
            .expect("bindcar-token volumeMount must exist");
        assert_eq!(mount.mount_path, BINDCAR_TOKEN_MOUNT_PATH);
        assert_eq!(
            mount.read_only,
            Some(true),
            "bindcar token mount must be read-only"
        );
    }

    /// The mount path plus token file name must compose to the exact path the
    /// operator reads the bearer token from (`BINDCAR_TOKEN_PATH`).
    #[test]
    fn test_bindcar_token_mount_path_matches_operator_token_path() {
        assert_eq!(
            format!("{BINDCAR_TOKEN_MOUNT_PATH}/{BINDCAR_TOKEN_FILENAME}"),
            bindy_bind9::bind9::BINDCAR_TOKEN_PATH
        );
    }

    /// POD_NAMESPACE must come from the downward API so the operator stamps the
    /// correct `system:serviceaccount:<ns>:bindy` into operand pods'
    /// BIND_ALLOWED_SERVICE_ACCOUNTS regardless of the `--namespace` argument.
    #[test]
    fn test_build_deployment_has_pod_namespace_downward_api_env() {
        let pod_spec = operator_pod_spec("custom-ns");
        let container = pod_spec.containers.into_iter().next().unwrap();
        let env = container.env.expect("container must have env vars");
        let pod_namespace = env
            .iter()
            .find(|e| e.name == POD_NAMESPACE_ENV)
            .expect("POD_NAMESPACE env var must exist");
        let field_path = pod_namespace
            .value_from
            .as_ref()
            .and_then(|v| v.field_ref.as_ref())
            .map(|f| f.field_path.as_str());
        assert_eq!(
            field_path,
            Some("metadata.namespace"),
            "POD_NAMESPACE must use the downward API (fieldRef metadata.namespace)"
        );
    }

    #[test]
    fn test_tokenreview_cluster_role_is_applied_by_operator_bootstrap() {
        let names: Vec<String> = OPERATOR_CLUSTER_ROLE_YAMLS
            .iter()
            .map(|yaml| {
                parse_cluster_role(yaml)
                    .unwrap()
                    .metadata
                    .name
                    .expect("embedded ClusterRole must have a name")
            })
            .collect();
        assert!(
            names.iter().any(|n| n == BINDCAR_TOKENREVIEW_NAME),
            "operator bootstrap must apply the TokenReview ClusterRole, got: {names:?}"
        );
    }

    #[test]
    fn test_tokenreview_cluster_role_grants_only_create_tokenreviews() {
        let role = parse_cluster_role(BINDCAR_TOKENREVIEW_CLUSTER_ROLE_YAML).unwrap();
        assert_eq!(
            role.metadata.name.as_deref(),
            Some(BINDCAR_TOKENREVIEW_NAME)
        );
        let rules = role.rules.expect("TokenReview ClusterRole must have rules");
        assert_eq!(rules.len(), 1, "least-privilege: exactly one rule");
        let rule = &rules[0];
        assert_eq!(
            rule.api_groups.as_deref(),
            Some(&["authentication.k8s.io".to_string()][..])
        );
        assert_eq!(
            rule.resources.as_deref(),
            Some(&["tokenreviews".to_string()][..])
        );
        assert_eq!(rule.verbs, vec!["create".to_string()]);
    }

    #[test]
    fn test_build_tokenreview_cluster_role_binding_role_ref() {
        let crb = build_tokenreview_cluster_role_binding(DEFAULT_NAMESPACE).unwrap();
        assert_eq!(crb.metadata.name.as_deref(), Some(BINDCAR_TOKENREVIEW_NAME));
        assert_eq!(crb.role_ref.kind, "ClusterRole");
        assert_eq!(crb.role_ref.name, BINDCAR_TOKENREVIEW_NAME);
    }

    #[test]
    fn test_build_tokenreview_cluster_role_binding_subject_is_operand_bind9_sa() {
        let crb = build_tokenreview_cluster_role_binding(DEFAULT_NAMESPACE).unwrap();
        let subject = crb.subjects.unwrap().into_iter().next().unwrap();
        assert_eq!(subject.kind, "ServiceAccount");
        assert_eq!(subject.name, crate::constants::BIND9_SERVICE_ACCOUNT);
        assert_eq!(subject.namespace.as_deref(), Some(DEFAULT_NAMESPACE));
    }

    /// The binding subject namespace must follow the requested `--namespace`
    /// argument, not the hardcoded `bindy-system` from the embedded manifest.
    #[test]
    fn test_build_tokenreview_cluster_role_binding_follows_requested_namespace() {
        let crb = build_tokenreview_cluster_role_binding("custom-ns").unwrap();
        let subjects = crb.subjects.unwrap();
        assert!(!subjects.is_empty());
        for subject in subjects {
            assert_eq!(subject.namespace.as_deref(), Some("custom-ns"));
        }
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
    fn test_build_all_crds_returns_thirteen() {
        let crds = build_all_crds().unwrap();
        assert_eq!(crds.len(), 13, "Expected exactly 13 CRDs");
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

    #[test]
    fn test_build_scout_cluster_role_covers_gateway_api_routes() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();

        // Check that HTTPRoute rule exists
        let http_route_rule = rules
            .iter()
            .find(|r| {
                r.resources
                    .as_ref()
                    .is_some_and(|res| res.iter().any(|s| s == "httproutes"))
            })
            .expect("httproutes rule must exist for Gateway API");

        // Check that TLSRoute rule exists (may be in same rule as HTTPRoute)
        let has_tlsroutes = rules.iter().any(|r| {
            r.resources
                .as_ref()
                .is_some_and(|res| res.iter().any(|s| s == "tlsroutes"))
        });
        assert!(has_tlsroutes, "tlsroutes rule must exist for Gateway API");

        // Check that TCPRoute rule exists (may be in same rule as HTTPRoute/TLSRoute)
        let has_tcproutes = rules.iter().any(|r| {
            r.resources
                .as_ref()
                .is_some_and(|res| res.iter().any(|s| s == "tcproutes"))
        });
        assert!(has_tcproutes, "tcproutes rule must exist for Gateway API");

        // Check that GRPCRoute rule exists (may be in same rule as other Route)
        let has_grpcroutes = rules.iter().any(|r| {
            r.resources
                .as_ref()
                .is_some_and(|res| res.iter().any(|s| s == "grpcroutes"))
        });
        assert!(has_grpcroutes, "grpcroutes rule must exist for Gateway API");

        // Verify correct API group
        assert_eq!(
            http_route_rule.api_groups.as_ref().unwrap()[0],
            "gateway.networking.k8s.io",
            "HTTPRoute rule must use gateway.networking.k8s.io API group"
        );
    }

    #[test]
    fn test_build_scout_cluster_role_covers_gateways() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();

        // Gateways must be readable so Scout can follow a route's parentRefs back
        // to the serving gateway and discover its external IP.
        let has_gateways = rules.iter().any(|r| {
            r.api_groups
                .as_ref()
                .is_some_and(|ags| ags.iter().any(|ag| ag == "gateway.networking.k8s.io"))
                && r.resources
                    .as_ref()
                    .is_some_and(|res| res.iter().any(|s| s == "gateways"))
        });
        assert!(
            has_gateways,
            "ClusterRole must grant read access to gateways for parentRef chain-following"
        );
    }

    #[test]
    fn test_build_scout_cluster_role_gateway_routes_allow_finalizer_mutation() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();

        // Find the route rule (httproutes/tlsroutes/tcproutes — Scout adds/removes finalizers).
        let route_rule = rules
            .iter()
            .find(|r| {
                r.api_groups
                    .as_ref()
                    .is_some_and(|ags| ags.iter().any(|ag| ag == "gateway.networking.k8s.io"))
                    && r.resources
                        .as_ref()
                        .is_some_and(|res| res.iter().any(|s| s == "httproutes"))
            })
            .expect("httproutes rule must exist in gateway.networking.k8s.io");

        // Routes need full read + mutation verbs for finalizer management.
        for verb in ["get", "list", "watch", "patch", "update"] {
            assert!(
                route_rule.verbs.contains(&verb.to_string()),
                "gateway routes rule must include '{verb}' (required for finalizer management)"
            );
        }

        // Gateways are read-only — Scout only reads status.addresses.
        let gw_rule = rules
            .iter()
            .find(|r| {
                r.api_groups
                    .as_ref()
                    .is_some_and(|ags| ags.iter().any(|ag| ag == "gateway.networking.k8s.io"))
                    && r.resources
                        .as_ref()
                        .is_some_and(|res| res.iter().any(|s| s == "gateways"))
            })
            .expect("gateways rule must exist");

        assert!(
            !gw_rule.verbs.contains(&"patch".to_string()),
            "gateways rule must NOT include 'patch' (read-only)"
        );
        assert!(
            !gw_rule.verbs.contains(&"update".to_string()),
            "gateways rule must NOT include 'update' (read-only)"
        );
    }

    #[test]
    fn test_build_scout_cluster_role_gateway_route_finalizers_subresource() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();

        // The /finalizers subresource rules must exist for each route type.
        for subresource in [
            "httproutes/finalizers",
            "grpcroutes/finalizers",
            "tlsroutes/finalizers",
            "tcproutes/finalizers",
        ] {
            let has_rule = rules.iter().any(|r| {
                r.api_groups
                    .as_ref()
                    .is_some_and(|ags| ags.iter().any(|ag| ag == "gateway.networking.k8s.io"))
                    && r.resources
                        .as_ref()
                        .is_some_and(|res| res.iter().any(|s| s == subresource))
                    && r.verbs.contains(&"update".to_string())
            });
            assert!(
                has_rule,
                "ClusterRole must have an 'update' rule for {subresource}"
            );
        }
    }

    #[test]
    fn test_build_scout_cluster_role_covers_namespaces_read_only() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();

        // Namespace get/list is required for --namespace-selector /
        // BINDY_SCOUT_NAMESPACE_SELECTOR — scout checks a source object's namespace
        // labels against the configured selector before acting.
        let ns_rule = rules
            .iter()
            .find(|r| {
                r.api_groups
                    .as_ref()
                    .is_some_and(|ags| ags.iter().any(|ag| ag.is_empty()))
                    && r.resources
                        .as_ref()
                        .is_some_and(|res| res.iter().any(|s| s == "namespaces"))
            })
            .expect("namespaces rule must exist for --namespace-selector support");

        for verb in ["get", "list"] {
            assert!(
                ns_rule.verbs.contains(&verb.to_string()),
                "namespaces rule must include '{verb}'"
            );
        }
        assert!(
            !ns_rule.verbs.contains(&"watch".to_string())
                && !ns_rule.verbs.contains(&"patch".to_string())
                && !ns_rule.verbs.contains(&"update".to_string())
                && !ns_rule.verbs.contains(&"delete".to_string())
                && !ns_rule.verbs.contains(&"create".to_string()),
            "namespaces rule must be strictly read-only (get/list only) — Scout never mutates Namespaces"
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

    // --- M-25: Scout cluster-wide Secret read closed ---

    /// The Scout ClusterRole must NOT grant any cluster-wide access to Secrets.
    /// Reading the remote-cluster kubeconfig Secret (Phase 2 only) is granted by a
    /// separate namespaced, resourceNames-restricted Role instead (see the
    /// `test_build_scout_secrets_reader_role_*` tests below).
    #[test]
    fn test_build_scout_cluster_role_has_no_secrets_rule() {
        let role = build_scout_cluster_role();
        let rules = role.rules.unwrap();
        let has_secrets_rule = rules.iter().any(|r| {
            r.resources
                .as_ref()
                .is_some_and(|res| res.iter().any(|s| s == "secrets"))
        });
        assert!(
            !has_secrets_rule,
            "Scout ClusterRole must NOT grant cluster-wide Secret access — \
             see build_scout_secrets_reader_role for the namespaced replacement"
        );
    }

    // --- Scout secrets-reader Role (namespaced, resourceNames-restricted) ---

    #[test]
    fn test_build_scout_secrets_reader_role_name() {
        let role = build_scout_secrets_reader_role("foo", "my-kubeconfig");
        assert_eq!(
            role.metadata.name.as_deref(),
            Some(SCOUT_SECRETS_READER_ROLE_NAME)
        );
    }

    #[test]
    fn test_build_scout_secrets_reader_role_namespace() {
        let role = build_scout_secrets_reader_role("bar", "my-kubeconfig");
        assert_eq!(role.metadata.namespace.as_deref(), Some("bar"));
    }

    #[test]
    fn test_build_scout_secrets_reader_role_grants_get_on_secrets_only() {
        let role = build_scout_secrets_reader_role("foo", "my-kubeconfig");
        let rules = role.rules.unwrap();
        assert_eq!(rules.len(), 1, "must grant exactly one rule");
        let rule = &rules[0];
        assert_eq!(
            rule.resources.as_ref().unwrap(),
            &vec!["secrets".to_string()]
        );
        assert_eq!(rule.verbs, vec!["get".to_string()], "must be get-only");
    }

    #[test]
    fn test_build_scout_secrets_reader_role_restricts_resource_names() {
        let role = build_scout_secrets_reader_role("foo", "my-remote-kubeconfig");
        let rules = role.rules.unwrap();
        let resource_names = rules[0]
            .resource_names
            .as_ref()
            .expect("rule must set resourceNames to scope to the single Secret");
        assert_eq!(
            resource_names,
            &vec!["my-remote-kubeconfig".to_string()],
            "resourceNames must restrict the rule to exactly the configured Secret name"
        );
    }

    #[test]
    fn test_build_scout_secrets_reader_role_binding_name() {
        let rb = build_scout_secrets_reader_role_binding("foo");
        assert_eq!(
            rb.metadata.name.as_deref(),
            Some(SCOUT_SECRETS_READER_ROLE_BINDING_NAME)
        );
    }

    #[test]
    fn test_build_scout_secrets_reader_role_binding_namespace() {
        let rb = build_scout_secrets_reader_role_binding("bar");
        assert_eq!(rb.metadata.namespace.as_deref(), Some("bar"));
    }

    #[test]
    fn test_build_scout_secrets_reader_role_binding_references_role() {
        let rb = build_scout_secrets_reader_role_binding("foo");
        assert_eq!(rb.role_ref.name, SCOUT_SECRETS_READER_ROLE_NAME);
        assert_eq!(rb.role_ref.kind, "Role");
    }

    #[test]
    fn test_build_scout_secrets_reader_role_binding_subject_is_scout_sa() {
        let rb = build_scout_secrets_reader_role_binding("baz");
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

    /// Returns the plain `value` of env var `name` on the scout container, if set.
    fn scout_container_env_value(
        d: k8s_openapi::api::apps::v1::Deployment,
        name: &str,
    ) -> Option<String> {
        d.spec?
            .template
            .spec?
            .containers
            .into_iter()
            .next()?
            .env?
            .into_iter()
            .find(|e| e.name == name)?
            .value
    }

    /// B12: `bindy bootstrap scout --namespace foo` creates the writer Role in
    /// `foo`, so the scout it deploys must target `foo` too. Without the env
    /// var the scout falls back to `bindy-system`, where it has no RBAC.
    #[test]
    fn test_build_scout_deployment_sets_target_namespace_env_to_bootstrap_namespace() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("foo", &opts).unwrap();
        assert_eq!(
            scout_container_env_value(d, SCOUT_TARGET_NAMESPACE_ENV).as_deref(),
            Some("foo")
        );
    }

    #[test]
    fn test_build_scout_deployment_target_namespace_env_follows_namespace() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("bar", &opts).unwrap();
        assert_eq!(
            scout_container_env_value(d, SCOUT_TARGET_NAMESPACE_ENV).as_deref(),
            Some("bar")
        );
    }

    #[test]
    fn test_scout_target_namespace_env_name_matches_scout_reader() {
        // The scout reads exactly this variable (crates/bindy-scout/src/scout.rs).
        assert_eq!(SCOUT_TARGET_NAMESPACE_ENV, "BINDY_SCOUT_NAMESPACE");
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
    fn test_build_scout_deployment_passes_the_allowlists() {
        let mut opts = scout_opts("latest", None, "edge-foo", &[], None, None);
        opts.allowed_zones = Some("foo.example.com");
        opts.allowed_ip_ranges = Some("10.0.0.0/24");
        let d = build_scout_deployment("foo", &opts).unwrap();
        assert_eq!(
            scout_container_args(d),
            vec![
                "scout",
                "--cluster-name",
                "edge-foo",
                "--allowed-zones",
                "foo.example.com",
                "--allowed-ip-ranges",
                "10.0.0.0/24"
            ]
        );
    }

    #[test]
    fn test_build_scout_deployment_no_allowlists_omits_flags() {
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let args = scout_container_args(build_scout_deployment("foo", &opts).unwrap());
        assert!(!args.contains(&"--allowed-zones".to_string()));
        assert!(!args.contains(&"--allowed-ip-ranges".to_string()));
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
    fn test_build_scout_deployment_never_runs_two_scouts_at_once() {
        // ADR-0002 addendum: an upgrade must not run the old and the new
        // Scout side by side, so one replica and the Recreate strategy.
        let opts = scout_opts("latest", None, DEFAULT_SCOUT_CLUSTER_NAME, &[], None, None);
        let d = build_scout_deployment("bindy-system", &opts).unwrap();
        let spec = d.spec.expect("deployment spec");
        assert_eq!(spec.replicas, Some(1));
        let strategy = spec.strategy.expect("an explicit strategy");
        assert_eq!(strategy.type_.as_deref(), Some("Recreate"));
        assert!(strategy.rolling_update.is_none());
    }

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

    // --- Multi-cluster (MC) Scout source registry (ADR-0025) ---

    #[test]
    fn test_scout_source_registry_names_the_configmap_the_policy_reads() {
        let cm = build_scout_source_registry(
            "bindy-system",
            "edge-foo",
            &["foo".into()],
            &["foo.example.com".into()],
        );
        assert_eq!(SCOUT_SOURCES_CONFIGMAP_NAME, "bindy-scout-sources");
        assert_eq!(
            cm.metadata.name.as_deref(),
            Some(SCOUT_SOURCES_CONFIGMAP_NAME)
        );
        assert_eq!(cm.metadata.namespace.as_deref(), Some("bindy-system"));
    }

    #[test]
    fn test_scout_source_registry_writes_only_this_accounts_keys() {
        let cm = build_scout_source_registry(
            "bindy-system",
            "edge-foo",
            &["foo".into(), "foo-old".into()],
            &["foo.example.com".into(), "foo.example.org".into()],
        );
        let data = cm.data.expect("data");
        assert_eq!(SCOUT_SOURCES_CLUSTERS_SUFFIX, ".clusters");
        assert_eq!(SCOUT_SOURCES_ZONES_SUFFIX, ".zones");
        assert_eq!(data.len(), 2, "{data:?}");
        assert_eq!(data["edge-foo.clusters"], "foo,foo-old");
        assert_eq!(data["edge-foo.zones"], "foo.example.com,foo.example.org");
    }

    #[test]
    fn test_scout_source_registry_normalizes_entries() {
        let cm = build_scout_source_registry(
            "bindy-system",
            "edge-foo",
            &[" foo ".into(), String::new()],
            &[" Foo.Example.com. ".into()],
        );
        let data = cm.data.expect("data");
        assert_eq!(data["edge-foo.clusters"], "foo");
        assert_eq!(data["edge-foo.zones"], "foo.example.com");
    }

    #[test]
    fn test_scout_source_registry_without_entries_writes_no_keys() {
        let cm = build_scout_source_registry("bindy-system", "edge-foo", &[], &[]);
        assert!(cm.data.unwrap_or_default().is_empty());
    }

    #[test]
    fn test_scout_source_field_manager_is_per_service_account() {
        assert_eq!(
            scout_source_field_manager("edge-foo"),
            "bindy-bootstrap-mc.edge-foo"
        );
        assert_ne!(
            scout_source_field_manager("edge-foo"),
            scout_source_field_manager("edge-bar")
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            false,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            false,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
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
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["contexts"][0]["context"]["cluster"].as_str(),
            Some("my-cluster")
        );
        assert_eq!(
            parsed["contexts"][0]["context"]["user"].as_str(),
            Some("scout")
        );
    }

    #[test]
    fn test_build_kubeconfig_yaml_refuses_no_ca_without_opt_in() {
        // H2 regression: with no CA data and `allow_insecure = false` the builder
        // must refuse rather than silently distribute a kubeconfig that disables
        // TLS verification.
        let result = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
            false,
        );
        assert!(
            result.is_err(),
            "expected refusal when CA missing and not opted in"
        );
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("insecure-skip-tls-verify"),
            "error must reference the opt-out flag, got: {msg}"
        );
    }

    #[test]
    fn test_build_kubeconfig_yaml_insecure_opt_in_sets_flag() {
        // H2: explicit opt-in still produces insecure-skip-tls-verify=true so
        // kind-to-kind local development flows keep working.
        let yaml = build_kubeconfig_yaml(
            "my-cluster",
            "https://example.com:6443",
            None,
            "scout",
            "my-token",
            true,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["clusters"][0]["cluster"]["insecure-skip-tls-verify"].as_bool(),
            Some(true)
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
        assert_eq!(kubeconfig_secret, "bindy-scout-remote-kubeconfig");
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
        assert_eq!(kubeconfig_secret, "bindy-scout-prod-us-east-kubeconfig");
    }
}

/// The Scout RBAC built by `bindy bootstrap scout` and the static manifests
/// that mirror it (`deploy/scout/*.yaml`, `docs/src/guide/scout.md`) are three
/// representations of one policy. These tests fail when any of them drifts,
/// so the sync rule in `CLAUDE.md` is enforced rather than remembered
/// (roadmap 01 Phase E).
#[cfg(test)]
mod rbac_drift {
    use crate::bootstrap::{
        build_scout_cluster_role, build_scout_cluster_role_binding,
        build_scout_secrets_reader_role, build_scout_secrets_reader_role_binding,
        build_scout_writer_role, build_scout_writer_role_binding,
    };
    use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding, Role, RoleBinding};
    use serde::de::DeserializeOwned;

    /// The namespace the static manifests are written for.
    const MANIFEST_NAMESPACE: &str = "bindy-system";
    /// The Secret the static secrets-reader Role is restricted to.
    const MANIFEST_REMOTE_SECRET: &str = "bindy-scout-remote-kubeconfig";

    /// `bindy bootstrap mc` with its defaults must produce exactly the Secret
    /// the static secrets-reader Role lets the scout read; otherwise a scout
    /// set up by the documented defaults is forbidden its own kubeconfig.
    #[test]
    fn static_scout_deployment_never_runs_two_scouts_at_once() {
        let docs = yaml_docs(SCOUT_DEPLOYMENT_YAML);
        let deployment = docs
            .iter()
            .find(|d| d["kind"] == "Deployment")
            .expect("deploy/scout/deployment.yaml has a Deployment");
        assert_eq!(deployment["spec"]["replicas"], 1);
        assert_eq!(
            deployment["spec"]["strategy"]["type"], "Recreate",
            "deploy/scout/deployment.yaml must match build_scout_deployment (ADR-0002 addendum)"
        );
    }

    #[test]
    fn default_mc_secret_is_the_one_the_static_rbac_allows() {
        let secret = crate::bootstrap::build_mc_kubeconfig_secret(
            MANIFEST_NAMESPACE,
            crate::bootstrap::MC_DEFAULT_SERVICE_ACCOUNT_NAME,
            "kubeconfig: content",
        );

        assert_eq!(
            secret.metadata.name.as_deref(),
            Some(MANIFEST_REMOTE_SECRET)
        );
    }

    const CLUSTERROLE_YAML: &str = include_str!("../../../deploy/scout/clusterrole.yaml");
    const CLUSTERROLEBINDING_YAML: &str =
        include_str!("../../../deploy/scout/clusterrolebinding.yaml");
    const ROLE_YAML: &str = include_str!("../../../deploy/scout/role.yaml");
    const ROLEBINDING_YAML: &str = include_str!("../../../deploy/scout/rolebinding.yaml");
    const SECRETS_READER_YAML: &str =
        include_str!("../../../deploy/scout/secrets-reader-rbac.yaml");
    const SCOUT_GUIDE_MD: &str = include_str!("../../../docs/src/guide/scout.md");
    const SCOUT_DEPLOYMENT_YAML: &str = include_str!("../../../deploy/scout/deployment.yaml");

    /// Every YAML document in `text`.
    fn yaml_docs(text: &str) -> Vec<serde_json::Value> {
        split_yaml_documents(text)
            .into_iter()
            .map_while(|doc| serde_saphyr::from_str::<serde_json::Value>(&doc).ok())
            .filter(|value| !value.is_null())
            .collect()
    }

    /// Split a multi-document YAML stream on its `---` separator lines.
    fn split_yaml_documents(text: &str) -> Vec<String> {
        let mut docs = vec![String::new()];
        for line in text.lines() {
            if line.trim_end() == "---" {
                docs.push(String::new());
                continue;
            }
            if let Some(current) = docs.last_mut() {
                current.push_str(line);
                current.push('\n');
            }
        }
        docs.retain(|doc| !doc.trim().is_empty());
        docs
    }

    /// Every YAML document inside the ```yaml blocks of a Markdown page.
    fn markdown_yaml_docs(markdown: &str) -> Vec<serde_json::Value> {
        let mut docs = Vec::new();
        let mut block: Option<String> = None;
        for line in markdown.lines() {
            match (&mut block, line.trim_start()) {
                (None, fence) if fence.starts_with("```yaml") => block = Some(String::new()),
                (Some(text), "```") => {
                    docs.extend(yaml_docs(text));
                    block = None;
                }
                (Some(text), _) => {
                    text.push_str(line);
                    text.push('\n');
                }
                (None, _) => {}
            }
        }
        docs
    }

    /// The documents of `kind` named `name`, deserialized.
    fn find<T: DeserializeOwned>(docs: &[serde_json::Value], kind: &str, name: &str) -> Vec<T> {
        docs.iter()
            .filter(|doc| {
                doc.get("kind").and_then(serde_json::Value::as_str) == Some(kind)
                    && doc
                        .get("metadata")
                        .and_then(|m| m.get("name"))
                        .and_then(serde_json::Value::as_str)
                        == Some(name)
            })
            .map(|doc| serde_json::from_value(doc.clone()).expect("a valid RBAC object"))
            .collect()
    }

    use k8s_openapi::api::rbac::v1::Subject;

    /// Subjects with the API server's defaulting applied: a `ServiceAccount`
    /// subject's empty `apiGroup` and an absent one are the same thing.
    fn normalized(subjects: Option<Vec<Subject>>) -> Option<Vec<Subject>> {
        subjects.map(|subjects| {
            subjects
                .into_iter()
                .map(|mut subject| {
                    if subject.api_group.as_deref() == Some("") {
                        subject.api_group = None;
                    }
                    subject
                })
                .collect()
        })
    }

    /// For a guide page: one ClusterRole block is the whole role and matches
    /// exactly; any other block is an excerpt ("... existing rules ...") and
    /// may only show rules the role really has.
    fn assert_cluster_role_matches_or_excerpts(docs: &[serde_json::Value], source: &str) {
        let built = build_scout_cluster_role();
        let built_rules = built.rules.clone().unwrap_or_default();
        let name = built.metadata.name.clone().unwrap();
        let found: Vec<ClusterRole> = find(docs, "ClusterRole", &name);
        assert!(
            found.iter().any(|role| role.rules == built.rules),
            "{source}: no complete ClusterRole {name} matching bootstrap"
        );
        for role in found {
            for rule in role.rules.unwrap_or_default() {
                assert!(
                    built_rules.contains(&rule),
                    "{source}: ClusterRole {name} shows a rule bootstrap does not grant: {rule:?}"
                );
            }
        }
    }

    fn assert_cluster_role_matches(docs: &[serde_json::Value], source: &str) {
        let built = build_scout_cluster_role();
        let name = built.metadata.name.clone().unwrap();
        let found: Vec<ClusterRole> = find(docs, "ClusterRole", &name);
        assert!(!found.is_empty(), "{source}: no ClusterRole {name}");
        for role in found {
            assert_eq!(
                role.rules, built.rules,
                "{source}: ClusterRole {name} rules drifted"
            );
        }
    }

    fn assert_role_matches(docs: &[serde_json::Value], built: &Role, source: &str) {
        let name = built.metadata.name.clone().unwrap();
        let found: Vec<Role> = find(docs, "Role", &name);
        assert!(!found.is_empty(), "{source}: no Role {name}");
        for role in found {
            assert_eq!(
                role.rules, built.rules,
                "{source}: Role {name} rules drifted"
            );
        }
    }

    fn assert_cluster_role_binding_matches(docs: &[serde_json::Value], source: &str) {
        let built = build_scout_cluster_role_binding(MANIFEST_NAMESPACE);
        let name = built.metadata.name.clone().unwrap();
        let found: Vec<ClusterRoleBinding> = find(docs, "ClusterRoleBinding", &name);
        assert!(!found.is_empty(), "{source}: no ClusterRoleBinding {name}");
        for binding in found {
            assert_eq!(
                binding.role_ref, built.role_ref,
                "{source}: {name} roleRef drifted"
            );
            assert_eq!(
                normalized(binding.subjects),
                normalized(built.subjects.clone()),
                "{source}: {name} subjects drifted"
            );
        }
    }

    fn assert_role_binding_matches(docs: &[serde_json::Value], built: &RoleBinding, source: &str) {
        let name = built.metadata.name.clone().unwrap();
        let found: Vec<RoleBinding> = find(docs, "RoleBinding", &name);
        assert!(!found.is_empty(), "{source}: no RoleBinding {name}");
        for binding in found {
            assert_eq!(
                binding.role_ref, built.role_ref,
                "{source}: {name} roleRef drifted"
            );
            assert_eq!(
                normalized(binding.subjects),
                normalized(built.subjects.clone()),
                "{source}: {name} subjects drifted"
            );
        }
    }

    #[test]
    fn deploy_scout_clusterrole_matches_bootstrap() {
        assert_cluster_role_matches(
            &yaml_docs(CLUSTERROLE_YAML),
            "deploy/scout/clusterrole.yaml",
        );
    }

    #[test]
    fn deploy_scout_clusterrolebinding_matches_bootstrap() {
        assert_cluster_role_binding_matches(
            &yaml_docs(CLUSTERROLEBINDING_YAML),
            "deploy/scout/clusterrolebinding.yaml",
        );
    }

    #[test]
    fn deploy_scout_writer_role_and_binding_match_bootstrap() {
        assert_role_matches(
            &yaml_docs(ROLE_YAML),
            &build_scout_writer_role(MANIFEST_NAMESPACE),
            "deploy/scout/role.yaml",
        );
        assert_role_binding_matches(
            &yaml_docs(ROLEBINDING_YAML),
            &build_scout_writer_role_binding(MANIFEST_NAMESPACE),
            "deploy/scout/rolebinding.yaml",
        );
    }

    #[test]
    fn deploy_scout_secrets_reader_matches_bootstrap() {
        let docs = yaml_docs(SECRETS_READER_YAML);
        assert_role_matches(
            &docs,
            &build_scout_secrets_reader_role(MANIFEST_NAMESPACE, MANIFEST_REMOTE_SECRET),
            "deploy/scout/secrets-reader-rbac.yaml",
        );
        assert_role_binding_matches(
            &docs,
            &build_scout_secrets_reader_role_binding(MANIFEST_NAMESPACE),
            "deploy/scout/secrets-reader-rbac.yaml",
        );
    }

    #[test]
    fn scout_guide_examples_match_bootstrap() {
        let docs = markdown_yaml_docs(SCOUT_GUIDE_MD);
        let source = "docs/src/guide/scout.md";
        assert_cluster_role_matches_or_excerpts(&docs, source);
        assert_cluster_role_binding_matches(&docs, source);
        assert_role_matches(&docs, &build_scout_writer_role(MANIFEST_NAMESPACE), source);
        assert_role_binding_matches(
            &docs,
            &build_scout_writer_role_binding(MANIFEST_NAMESPACE),
            source,
        );
        assert_role_matches(
            &docs,
            &build_scout_secrets_reader_role(MANIFEST_NAMESPACE, MANIFEST_REMOTE_SECRET),
            source,
        );
        assert_role_binding_matches(
            &docs,
            &build_scout_secrets_reader_role_binding(MANIFEST_NAMESPACE),
            source,
        );
    }
}

/// `bindy bootstrap mc` writes the kubeconfig Secret (it carries the
/// ServiceAccount token) to a private file, never to stdout unless asked.
#[cfg(test)]
mod mc_output_tests {
    use crate::bootstrap::{mc_output_path, write_private_file};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// Owner read/write only.
    #[cfg(unix)]
    const PRIVATE_MODE: u32 = 0o600;
    /// Group and world readable, as a default umask creates files.
    #[cfg(unix)]
    const WORLD_READABLE_MODE: u32 = 0o644;
    /// The permission bits of a file mode.
    #[cfg(unix)]
    const PERMISSION_BITS: u32 = 0o777;

    #[cfg(unix)]
    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path)
            .expect("file exists")
            .permissions()
            .mode()
            & PERMISSION_BITS
    }

    #[test]
    fn defaults_to_a_file_named_after_the_service_account() {
        assert_eq!(
            mc_output_path(None, "bindy-scout-remote").expect("a path"),
            PathBuf::from("bindy-scout-remote-kubeconfig.yaml")
        );
    }

    #[test]
    fn stdout_is_refused_because_the_secret_carries_a_token() {
        let err = mc_output_path(Some("-"), "sa").expect_err("stdout is refused");
        assert!(err.to_string().contains("token"), "unexpected error: {err}");
    }

    #[test]
    fn an_explicit_path_is_used_as_is() {
        assert_eq!(
            mc_output_path(Some("/tmp/child-a.yaml"), "sa").expect("a path"),
            PathBuf::from("/tmp/child-a.yaml")
        );
    }

    #[test]
    fn an_existing_longer_file_is_fully_replaced() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("secret.yaml");
        std::fs::write(&path, "old token that is longer than the new one\n").expect("seeded");

        write_private_file(&path, "token: new\n").expect("written");

        assert_eq!(
            std::fs::read_to_string(&path).expect("readable"),
            "token: new\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_new_file_is_private() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("secret.yaml");

        write_private_file(&path, "token: abc\n").expect("written");

        assert_eq!(mode(&path), PRIVATE_MODE);
        assert_eq!(
            std::fs::read_to_string(&path).expect("readable"),
            "token: abc\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn an_existing_readable_file_is_made_private_and_replaced() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("secret.yaml");
        std::fs::write(&path, "old token that is longer than the new one\n").expect("seeded");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(WORLD_READABLE_MODE))
            .expect("chmod");

        write_private_file(&path, "token: new\n").expect("written");

        assert_eq!(mode(&path), PRIVATE_MODE);
        assert_eq!(
            std::fs::read_to_string(&path).expect("readable"),
            "token: new\n"
        );
    }
}

/// `bindy bootstrap revoke-mc` deletes each object it created, skipping one
/// that is already gone and failing on any other error.
#[cfg(test)]
mod mc_revoke_tests {
    use crate::bootstrap::delete_mc_resource;
    use k8s_openapi::api::rbac::v1::Role;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const NAMESPACE: &str = "bindy-system";
    const NAME: &str = "bindy-scout-remote";
    const ROLE_PATH: &str =
        "/apis/rbac.authorization.k8s.io/v1/namespaces/bindy-system/roles/bindy-scout-remote";
    const HTTP_OK: u16 = 200;
    const HTTP_INTERNAL_SERVER_ERROR: u16 = 500;

    fn client_for(server: &MockServer) -> kube::Client {
        let config = kube::Config::new(server.uri().parse().expect("mock server uri"));
        kube::Client::try_from(config).expect("client from mock config")
    }

    fn status_json(code: u16, reason: &str) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "v1",
            "kind": "Status",
            "status": "Failure",
            "message": "test",
            "reason": reason,
            "code": code
        })
    }

    async fn server_answering_delete(response: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(ROLE_PATH))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn deletes_a_present_object() {
        let server = server_answering_delete(ResponseTemplate::new(HTTP_OK).set_body_json(
            serde_json::json!({
                "apiVersion": "rbac.authorization.k8s.io/v1",
                "kind": "Role",
                "metadata": {"name": NAME, "namespace": NAMESPACE}
            }),
        ))
        .await;

        delete_mc_resource::<Role>(&client_for(&server), NAMESPACE, NAME)
            .await
            .expect("a present object is deleted");
    }

    #[tokio::test]
    async fn skips_an_absent_object() {
        let server = server_answering_delete(
            ResponseTemplate::new(crate::constants::HTTP_NOT_FOUND)
                .set_body_json(status_json(crate::constants::HTTP_NOT_FOUND, "NotFound")),
        )
        .await;

        delete_mc_resource::<Role>(&client_for(&server), NAMESPACE, NAME)
            .await
            .expect("an absent object is skipped, not an error");
    }

    #[tokio::test]
    async fn fails_on_a_server_error_naming_the_object() {
        let server = server_answering_delete(
            ResponseTemplate::new(HTTP_INTERNAL_SERVER_ERROR)
                .set_body_json(status_json(HTTP_INTERNAL_SERVER_ERROR, "InternalError")),
        )
        .await;

        let err = delete_mc_resource::<Role>(&client_for(&server), NAMESPACE, NAME)
            .await
            .expect_err("a server error propagates");

        assert_eq!(err.to_string(), format!("Failed to delete Role/{NAME}"));
    }
}

#[cfg(test)]
mod apply_tests {
    use crate::bootstrap::{apply, build_service_account};
    use k8s_openapi::api::core::v1::ServiceAccount;
    use kube::Api;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const NAMESPACE: &str = "bindy-system";
    const SA_PATH: &str = "/api/v1/namespaces/bindy-system/serviceaccounts/bindy";
    const FIELD_MANAGER: &str = "bindy-bootstrap";
    const APPLY_PATCH_CONTENT_TYPE: &str = "application/apply-patch+yaml";
    const HTTP_OK: u16 = 200;
    const HTTP_INTERNAL_SERVER_ERROR: u16 = 500;

    fn api_for(server: &MockServer) -> Api<ServiceAccount> {
        let config = kube::Config::new(server.uri().parse().expect("mock server uri"));
        let client = kube::Client::try_from(config).expect("client from mock config");
        Api::namespaced(client, NAMESPACE)
    }

    fn applied_body() -> serde_json::Value {
        serde_json::to_value(build_service_account(NAMESPACE)).expect("service account json")
    }

    #[tokio::test]
    async fn server_side_applies_the_object_by_its_name_with_force() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(SA_PATH))
            .and(query_param("fieldManager", FIELD_MANAGER))
            .and(query_param("force", "true"))
            .and(header("content-type", APPLY_PATCH_CONTENT_TYPE))
            .and(body_json(applied_body()))
            .respond_with(ResponseTemplate::new(HTTP_OK).set_body_json(applied_body()))
            .expect(1)
            .mount(&server)
            .await;

        apply(
            &api_for(&server),
            &build_service_account(NAMESPACE),
            FIELD_MANAGER,
        )
        .await
        .expect("the object is applied");
    }

    #[tokio::test]
    async fn a_missing_name_is_an_error_and_sends_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(HTTP_OK))
            .expect(0)
            .mount(&server)
            .await;
        let mut sa = build_service_account(NAMESPACE);
        sa.metadata.name = None;

        let err = apply(&api_for(&server), &sa, FIELD_MANAGER)
            .await
            .expect_err("an object without a name cannot be applied");

        assert_eq!(
            err.to_string(),
            "Cannot apply ServiceAccount: metadata.name is not set"
        );
    }

    #[tokio::test]
    async fn a_server_error_names_the_object() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(SA_PATH))
            .respond_with(
                ResponseTemplate::new(HTTP_INTERNAL_SERVER_ERROR).set_body_json(
                    serde_json::json!({
                        "apiVersion": "v1",
                        "kind": "Status",
                        "status": "Failure",
                        "message": "test",
                        "reason": "InternalError",
                        "code": HTTP_INTERNAL_SERVER_ERROR
                    }),
                ),
            )
            .mount(&server)
            .await;

        let err = apply(
            &api_for(&server),
            &build_service_account(NAMESPACE),
            FIELD_MANAGER,
        )
        .await
        .expect_err("a server error propagates");

        assert_eq!(err.to_string(), "Failed to apply ServiceAccount/bindy");
    }
}

#[cfg(test)]
mod plan_tests {
    use crate::bootstrap::{
        build_all_crds, build_cluster_role_binding, build_deployment, build_namespace,
        build_scout_cluster_role, build_scout_cluster_role_binding, build_scout_deployment,
        build_scout_secrets_reader_role, build_scout_secrets_reader_role_binding,
        build_scout_service_account, build_scout_writer_role, build_scout_writer_role_binding,
        build_secrets_writer_role, build_secrets_writer_role_binding, build_service_account,
        build_tokenreview_cluster_role_binding, operator_steps, parse_cluster_role, render_dry_run,
        scout_steps, ScoutDeploymentOptions, Step, OPERATOR_CLUSTER_ROLE_YAMLS,
        OPERATOR_DRY_RUN_BANNER, SCOUT_DRY_RUN_BANNER,
    };

    const NAMESPACE: &str = "team-dns";
    const TAG: &str = "v9.9.9";
    const REGISTRY: &str = "my.reg.io/org";
    const SECRET: &str = "bindy-scout-remote-kubeconfig";
    const ZONE: &str = "example.com";

    fn ips() -> Vec<String> {
        vec!["192.0.2.10".to_string()]
    }

    fn opts<'a>(ips: &'a [String], remote_secret: Option<&'a str>) -> ScoutDeploymentOptions<'a> {
        ScoutDeploymentOptions {
            image_tag: TAG,
            registry: Some(REGISTRY),
            cluster_name: "child-1",
            default_ips: ips,
            default_zone: Some(ZONE),
            remote_secret,
            allowed_zones: None,
            allowed_ip_ranges: None,
        }
    }

    /// The dry-run output exactly as the pre-refactor `run_*_dry_run`
    /// functions printed it: banner, blank line, one `---` / `# label` /
    /// YAML block per object, footer.
    fn expected_dry_run(banner: &str, blocks: &[(String, String)]) -> String {
        let mut out = format!("{banner}\n\n");
        for (label, yaml) in blocks {
            out.push_str(&format!("---\n# {label}\n{yaml}"));
        }
        out.push_str("# Dry-run complete \u{2014} no resources were applied\n");
        out
    }

    fn yaml<T: serde::Serialize>(value: &T) -> String {
        bindy_api::yaml::to_string(value).expect("yaml")
    }

    fn crd_blocks() -> Vec<(String, String)> {
        build_all_crds()
            .expect("crds")
            .iter()
            .map(|crd| {
                let name = crd.metadata.name.clone().expect("crd name");
                (format!("CustomResourceDefinition/{name}"), yaml(crd))
            })
            .collect()
    }

    fn applied_lines(steps: &[Step]) -> Vec<String> {
        steps.iter().map(|s| s.applied.clone()).collect()
    }

    fn crd_applied_lines() -> Vec<String> {
        build_all_crds()
            .expect("crds")
            .into_iter()
            .map(|crd| format!("✓ CRD: {}", crd.metadata.name.expect("name")))
            .collect()
    }

    #[test]
    fn operator_dry_run_output_is_pinned() {
        let mut blocks = vec![("Namespace".to_string(), yaml(&build_namespace(NAMESPACE)))];
        blocks.extend(crd_blocks());
        blocks.push((
            "ServiceAccount".to_string(),
            yaml(&build_service_account(NAMESPACE)),
        ));
        for role_yaml in OPERATOR_CLUSTER_ROLE_YAMLS {
            let role = parse_cluster_role(role_yaml).expect("role");
            let name = role.metadata.name.clone().expect("role name");
            blocks.push((format!("ClusterRole ({name})"), yaml(&role)));
        }
        blocks.push((
            "ClusterRoleBinding".to_string(),
            yaml(&build_cluster_role_binding(NAMESPACE)),
        ));
        blocks.push((
            "ClusterRoleBinding (bindcar-tokenreview)".to_string(),
            yaml(&build_tokenreview_cluster_role_binding(NAMESPACE).expect("crb")),
        ));
        blocks.push((
            "Role (secrets-writer)".to_string(),
            yaml(&build_secrets_writer_role(NAMESPACE)),
        ));
        blocks.push((
            "RoleBinding (secrets-writer)".to_string(),
            yaml(&build_secrets_writer_role_binding(NAMESPACE)),
        ));
        blocks.push((
            "Deployment".to_string(),
            yaml(&build_deployment(NAMESPACE, TAG, Some(REGISTRY)).expect("deployment")),
        ));

        let steps = operator_steps(NAMESPACE, TAG, Some(REGISTRY)).expect("steps");
        let rendered = render_dry_run(OPERATOR_DRY_RUN_BANNER, &steps).expect("render");

        assert_eq!(
            OPERATOR_DRY_RUN_BANNER,
            "# Dry-run mode \u{2014} no resources will be applied"
        );
        assert_eq!(rendered, expected_dry_run(OPERATOR_DRY_RUN_BANNER, &blocks));
    }

    fn scout_blocks(with_secret: bool) -> Vec<(String, String)> {
        let ips = ips();
        let opts = opts(&ips, with_secret.then_some(SECRET));
        let mut blocks = vec![("Namespace".to_string(), yaml(&build_namespace(NAMESPACE)))];
        blocks.extend(crd_blocks());
        blocks.push((
            "ServiceAccount (scout)".to_string(),
            yaml(&build_scout_service_account(NAMESPACE)),
        ));
        blocks.push((
            "ClusterRole (scout)".to_string(),
            yaml(&build_scout_cluster_role()),
        ));
        blocks.push((
            "ClusterRoleBinding (scout)".to_string(),
            yaml(&build_scout_cluster_role_binding(NAMESPACE)),
        ));
        blocks.push((
            "Role (scout-writer)".to_string(),
            yaml(&build_scout_writer_role(NAMESPACE)),
        ));
        blocks.push((
            "RoleBinding (scout-writer)".to_string(),
            yaml(&build_scout_writer_role_binding(NAMESPACE)),
        ));
        if with_secret {
            blocks.push((
                "Role (scout-secrets-reader)".to_string(),
                yaml(&build_scout_secrets_reader_role(NAMESPACE, SECRET)),
            ));
            blocks.push((
                "RoleBinding (scout-secrets-reader)".to_string(),
                yaml(&build_scout_secrets_reader_role_binding(NAMESPACE)),
            ));
        }
        blocks.push((
            "Deployment (scout)".to_string(),
            yaml(&build_scout_deployment(NAMESPACE, &opts).expect("deployment")),
        ));
        blocks
    }

    #[test]
    fn scout_dry_run_output_is_pinned() {
        let ips = ips();
        let steps = scout_steps(NAMESPACE, &opts(&ips, None)).expect("steps");
        let rendered = render_dry_run(SCOUT_DRY_RUN_BANNER, &steps).expect("render");

        assert_eq!(
            SCOUT_DRY_RUN_BANNER,
            "# Dry-run mode (scout) \u{2014} no resources will be applied"
        );
        assert_eq!(
            rendered,
            expected_dry_run(SCOUT_DRY_RUN_BANNER, &scout_blocks(false))
        );
    }

    #[test]
    fn scout_dry_run_with_remote_secret_adds_the_secrets_reader() {
        let ips = ips();
        let steps = scout_steps(NAMESPACE, &opts(&ips, Some(SECRET))).expect("steps");
        let rendered = render_dry_run(SCOUT_DRY_RUN_BANNER, &steps).expect("render");

        assert_eq!(
            rendered,
            expected_dry_run(SCOUT_DRY_RUN_BANNER, &scout_blocks(true))
        );
    }

    #[test]
    fn operator_applied_lines_are_pinned() {
        let steps = operator_steps(NAMESPACE, TAG, Some(REGISTRY)).expect("steps");
        let mut expected = vec![format!("✓ Namespace: {NAMESPACE}")];
        expected.extend(crd_applied_lines());
        expected.extend([
            format!("✓ ServiceAccount: bindy (namespace: {NAMESPACE})"),
            "✓ ClusterRole: bindy-role".to_string(),
            "✓ ClusterRole: bindy-admin-role".to_string(),
            "✓ ClusterRole: bindcar-tokenreview".to_string(),
            "✓ ClusterRoleBinding: bindy-rolebinding".to_string(),
            format!("✓ ClusterRoleBinding: bindcar-tokenreview (subject namespace: {NAMESPACE})"),
            format!("✓ Role: bindy-secrets-writer (namespace: {NAMESPACE})"),
            format!("✓ RoleBinding: bindy-secrets-writer (namespace: {NAMESPACE})"),
            format!("✓ Deployment: bindy (image: {REGISTRY}/bindy:{TAG})"),
        ]);

        assert_eq!(applied_lines(&steps), expected);
    }

    #[test]
    fn scout_applied_lines_are_pinned() {
        let ips = ips();
        let steps = scout_steps(NAMESPACE, &opts(&ips, Some(SECRET))).expect("steps");
        let mut expected = vec![format!("✓ Namespace: {NAMESPACE}")];
        expected.extend(crd_applied_lines());
        expected.extend([
            format!("✓ ServiceAccount: bindy-scout (namespace: {NAMESPACE})"),
            "✓ ClusterRole: bindy-scout".to_string(),
            "✓ ClusterRoleBinding: bindy-scout".to_string(),
            format!("✓ Role: bindy-scout-writer (namespace: {NAMESPACE})"),
            format!("✓ RoleBinding: bindy-scout-writer (namespace: {NAMESPACE})"),
            format!(
                "✓ Role: bindy-scout-secrets-reader (namespace: {NAMESPACE}, secret: {SECRET})"
            ),
            format!("✓ RoleBinding: bindy-scout-secrets-reader (namespace: {NAMESPACE})"),
            format!("✓ Deployment: bindy-scout (image: {REGISTRY}/bindy:{TAG})"),
        ]);

        assert_eq!(applied_lines(&steps), expected);
    }
}

#[cfg(test)]
mod builder_helper_tests {
    use crate::bootstrap::{labels, mark_storage_version, role_ref, rule, sa_subject};
    use k8s_openapi::api::rbac::v1::{ClusterRole, Role};
    use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::{
        CustomResourceDefinition, CustomResourceDefinitionVersion,
    };

    #[test]
    fn labels_carry_the_app_name_and_component() {
        let labels = labels("scout");
        assert_eq!(labels.len(), 2);
        assert_eq!(labels["app.kubernetes.io/name"], "bindy");
        assert_eq!(labels["app.kubernetes.io/component"], "scout");
    }

    #[test]
    fn sa_subject_is_a_core_group_service_account() {
        let subject = sa_subject("bindy", "ns");
        assert_eq!(subject.kind, "ServiceAccount");
        assert_eq!(subject.name, "bindy");
        assert_eq!(subject.namespace.as_deref(), Some("ns"));
        assert_eq!(subject.api_group.as_deref(), Some(""));
    }

    #[test]
    fn role_ref_names_the_rbac_group_and_kind() {
        let cluster = role_ref::<ClusterRole>("bindy-role");
        assert_eq!(cluster.api_group, "rbac.authorization.k8s.io");
        assert_eq!(cluster.kind, "ClusterRole");
        assert_eq!(cluster.name, "bindy-role");
        assert_eq!(role_ref::<Role>("r").kind, "Role");
    }

    #[test]
    fn rule_copies_groups_resources_and_verbs_in_order() {
        let r = rule(&[""], &["secrets", "configmaps"], &["get", "list"]);
        assert_eq!(r.api_groups, Some(vec![String::new()]));
        assert_eq!(
            r.resources,
            Some(vec!["secrets".to_string(), "configmaps".to_string()])
        );
        assert_eq!(r.verbs, vec!["get".to_string(), "list".to_string()]);
        assert_eq!(r.resource_names, None);
        assert_eq!(r.non_resource_urls, None);
    }

    #[test]
    fn mark_storage_version_serves_and_stores_the_first_version() {
        let mut crd = CustomResourceDefinition::default();
        crd.spec.versions = vec![
            CustomResourceDefinitionVersion {
                name: "v1".to_string(),
                ..Default::default()
            },
            CustomResourceDefinitionVersion {
                name: "v2".to_string(),
                ..Default::default()
            },
        ];

        let crd = mark_storage_version(crd).expect("versions present");

        assert!(crd.spec.versions[0].storage);
        assert!(crd.spec.versions[0].served);
        assert!(!crd.spec.versions[1].storage);
        assert!(!crd.spec.versions[1].served);
    }

    #[test]
    fn mark_storage_version_without_versions_is_an_error() {
        let mut crd = CustomResourceDefinition::default();
        crd.metadata.name = Some("things.example.com".to_string());

        let err = mark_storage_version(crd).expect_err("no version to store");

        assert_eq!(
            err.to_string(),
            "CRD things.example.com has no spec.versions to mark as served and stored"
        );
    }
}

#[cfg(test)]
mod cluster_info_tests {
    use crate::bootstrap::{cluster_info, resolve_server};
    use kube::config::Kubeconfig;

    const OVERRIDE: &str = "https://172.18.0.3:6443";

    fn kubeconfig(cluster_body: &str) -> Kubeconfig {
        Kubeconfig::from_yaml(&format!(
            "apiVersion: v1
kind: Config
current-context: ctx
contexts:
- name: ctx
  context:
    cluster: queen
    user: admin
clusters:
- name: queen
  cluster:
{cluster_body}
users:
- name: admin
  user:
    token: t
"
        ))
        .expect("kubeconfig yaml")
    }

    #[test]
    fn reads_the_server_and_inline_ca_of_the_current_context() {
        let info = cluster_info(&kubeconfig(
            "    server: https://queen.example:6443\n    certificate-authority-data: Q0E=",
        ))
        .expect("cluster info");

        assert_eq!(info.name, "queen");
        assert_eq!(info.server.as_deref(), Some("https://queen.example:6443"));
        assert_eq!(info.ca_data_b64.as_deref(), Some("Q0E="));
        assert_eq!(
            resolve_server(None, &info).expect("server"),
            "https://queen.example:6443"
        );
    }

    #[test]
    fn a_missing_server_is_an_error_without_an_override() {
        let info =
            cluster_info(&kubeconfig("    insecure-skip-tls-verify: true")).expect("cluster info");

        assert_eq!(info.server, None);
        let err = resolve_server(None, &info).expect_err("no server to write");
        assert_eq!(
            err.to_string(),
            "KUBECONFIG cluster 'queen' has no server URL; pass --server <url> with an API \
             server address reachable from the child cluster"
        );
    }

    #[test]
    fn the_override_wins_even_when_the_kubeconfig_has_no_server() {
        let info =
            cluster_info(&kubeconfig("    insecure-skip-tls-verify: true")).expect("cluster info");

        assert_eq!(
            resolve_server(Some(OVERRIDE), &info).expect("server"),
            OVERRIDE
        );
    }

    #[test]
    fn the_override_wins_over_the_kubeconfig_server() {
        let info = cluster_info(&kubeconfig("    server: https://queen.example:6443"))
            .expect("cluster info");

        assert_eq!(
            resolve_server(Some(OVERRIDE), &info).expect("server"),
            OVERRIDE
        );
    }
}

/// ADR-0002 addendum: an existing Scout Deployment on `RollingUpdate` is moved
/// to `Recreate` before the server-side apply, because the API server refuses
/// an apply that sets `Recreate` while its defaulted `rollingUpdate` remains
/// (verified on kind: "spec.strategy.rollingUpdate: Forbidden").
mod recreate_migration_tests {
    use crate::bootstrap::{
        build_scout_deployment, needs_recreate_migration, prepare_deployment_strategy,
        recreate_strategy_patch, ScoutDeploymentOptions, DEFAULT_SCOUT_CLUSTER_NAME,
    };
    use k8s_openapi::api::apps::v1::Deployment;
    use kube::Api;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const NAMESPACE: &str = "bindy-system";
    const DEPLOYMENT_PATH: &str = "/apis/apps/v1/namespaces/bindy-system/deployments/bindy-scout";
    const MERGE_PATCH_CONTENT_TYPE: &str = "application/merge-patch+json";
    const HTTP_OK: u16 = 200;
    const HTTP_NOT_FOUND: u16 = 404;

    fn desired() -> Deployment {
        let opts = ScoutDeploymentOptions {
            image_tag: "latest",
            registry: None,
            cluster_name: DEFAULT_SCOUT_CLUSTER_NAME,
            default_ips: &[],
            default_zone: None,
            remote_secret: None,
            allowed_zones: None,
            allowed_ip_ranges: None,
        };
        build_scout_deployment(NAMESPACE, &opts).expect("scout deployment")
    }

    /// The Deployment as the API server holds it with strategy `strategy`.
    fn live(strategy: serde_json::Value) -> serde_json::Value {
        let mut d = serde_json::to_value(desired()).expect("deployment json");
        d["spec"]["strategy"] = strategy;
        d
    }

    fn rolling_update() -> serde_json::Value {
        serde_json::json!({"type": "RollingUpdate", "rollingUpdate": {"maxSurge": "25%", "maxUnavailable": "25%"}})
    }

    fn api_for(server: &MockServer) -> Api<Deployment> {
        let config = kube::Config::new(server.uri().parse().expect("mock server uri"));
        let client = kube::Client::try_from(config).expect("client from mock config");
        Api::namespaced(client, NAMESPACE)
    }

    fn not_found() -> ResponseTemplate {
        ResponseTemplate::new(HTTP_NOT_FOUND).set_body_json(serde_json::json!({
            "apiVersion": "v1", "kind": "Status", "status": "Failure",
            "message": "not found", "reason": "NotFound", "code": HTTP_NOT_FOUND
        }))
    }

    #[test]
    fn a_rolling_update_deployment_needs_migrating_to_recreate() {
        let existing: Deployment =
            serde_json::from_value(live(rolling_update())).expect("deployment");
        assert!(needs_recreate_migration(&existing, &desired()));
    }

    #[test]
    fn a_recreate_deployment_needs_nothing() {
        let existing: Deployment =
            serde_json::from_value(live(serde_json::json!({"type": "Recreate"})))
                .expect("deployment");
        assert!(!needs_recreate_migration(&existing, &desired()));
    }

    #[test]
    fn a_desired_rolling_update_is_never_migrated() {
        let existing: Deployment =
            serde_json::from_value(live(rolling_update())).expect("deployment");
        let mut wanted = desired();
        wanted.spec.as_mut().expect("spec").strategy = None;
        assert!(!needs_recreate_migration(&existing, &wanted));
    }

    #[test]
    fn the_patch_sets_recreate_and_deletes_rolling_update() {
        assert_eq!(
            recreate_strategy_patch(),
            serde_json::json!({"spec": {"strategy": {"type": "Recreate", "rollingUpdate": null}}})
        );
    }

    #[tokio::test]
    async fn an_existing_rolling_update_deployment_is_merge_patched_first() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(DEPLOYMENT_PATH))
            .respond_with(ResponseTemplate::new(HTTP_OK).set_body_json(live(rolling_update())))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path(DEPLOYMENT_PATH))
            .and(header("content-type", MERGE_PATCH_CONTENT_TYPE))
            .and(body_json(recreate_strategy_patch()))
            .respond_with(
                ResponseTemplate::new(HTTP_OK)
                    .set_body_json(live(serde_json::json!({"type": "Recreate"}))),
            )
            .expect(1)
            .mount(&server)
            .await;

        prepare_deployment_strategy(&api_for(&server), &desired())
            .await
            .expect("the strategy is migrated");
    }

    #[tokio::test]
    async fn a_missing_deployment_is_left_to_the_apply() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(DEPLOYMENT_PATH))
            .respond_with(not_found())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(HTTP_OK))
            .expect(0)
            .mount(&server)
            .await;

        prepare_deployment_strategy(&api_for(&server), &desired())
            .await
            .expect("nothing to migrate");
    }

    #[tokio::test]
    async fn an_already_recreate_deployment_is_not_patched() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(DEPLOYMENT_PATH))
            .respond_with(
                ResponseTemplate::new(HTTP_OK)
                    .set_body_json(live(serde_json::json!({"type": "Recreate"}))),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(HTTP_OK))
            .expect(0)
            .mount(&server)
            .await;

        prepare_deployment_strategy(&api_for(&server), &desired())
            .await
            .expect("nothing to migrate");
    }
}
