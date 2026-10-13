// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `finalizers.rs`

#[cfg(test)]
mod tests {
    use crate::finalizers::{ensure_finalizer, handle_deletion, has_finalizer, remove_finalizer};
    use crate::test_api::{requests, scripted_client, status_body, HTTP_OK};
    use bindy_api::crd::{
        Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, ClusterBind9Provider,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
    use kube::Api;

    const TEST_FINALIZER: &str = "test.firestoned.io/finalizer";
    const TEST_NAMESPACE: &str = "test-namespace";
    const TEST_NAME: &str = "test-resource";

    /// Helper to create a test `Bind9Cluster`
    fn create_test_cluster() -> Bind9Cluster {
        Bind9Cluster {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                finalizers: None,
                deletion_timestamp: None,
                generation: Some(1),
                ..Default::default()
            },
            spec: Bind9ClusterSpec {
                common: Bind9ClusterCommonSpec {
                    version: None,
                    primary: None,
                    secondary: None,
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

    /// Helper to create a test `Bind9Cluster` with finalizers
    fn create_test_cluster_with_finalizers(finalizers: Vec<String>) -> Bind9Cluster {
        Bind9Cluster {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                finalizers: Some(finalizers),
                deletion_timestamp: None,
                generation: Some(1),
                ..Default::default()
            },
            spec: Bind9ClusterSpec {
                common: Bind9ClusterCommonSpec {
                    version: None,
                    primary: None,
                    secondary: None,
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

    /// Helper to create a test `Bind9Cluster` with deletion timestamp
    fn create_test_cluster_being_deleted(finalizers: Vec<String>) -> Bind9Cluster {
        Bind9Cluster {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                finalizers: Some(finalizers),
                deletion_timestamp: Some(Time(k8s_openapi::jiff::Timestamp::now())),
                generation: Some(1),
                ..Default::default()
            },
            spec: Bind9ClusterSpec {
                common: Bind9ClusterCommonSpec {
                    version: None,
                    primary: None,
                    secondary: None,
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

    /// Helper to create a test `ClusterBind9Provider` (cluster-scoped)
    fn create_test_cluster_provider() -> ClusterBind9Provider {
        use bindy_api::crd::ClusterBind9ProviderSpec;
        ClusterBind9Provider {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: None, // Cluster-scoped
                finalizers: None,
                deletion_timestamp: None,
                generation: Some(1),
                ..Default::default()
            },
            spec: ClusterBind9ProviderSpec {
                namespace: None,
                common: Bind9ClusterCommonSpec {
                    version: None,
                    primary: None,
                    secondary: None,
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

    /// Helper to create a test `ClusterBind9Provider` with finalizers
    fn create_test_cluster_provider_with_finalizers(
        finalizers: Vec<String>,
    ) -> ClusterBind9Provider {
        use bindy_api::crd::ClusterBind9ProviderSpec;
        ClusterBind9Provider {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: None, // Cluster-scoped
                finalizers: Some(finalizers),
                deletion_timestamp: None,
                generation: Some(1),
                ..Default::default()
            },
            spec: ClusterBind9ProviderSpec {
                namespace: None,
                common: Bind9ClusterCommonSpec {
                    version: None,
                    primary: None,
                    secondary: None,
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

    /// Helper to create a test `ClusterBind9Provider` with deletion timestamp
    fn create_test_cluster_provider_being_deleted(finalizers: Vec<String>) -> ClusterBind9Provider {
        use bindy_api::crd::ClusterBind9ProviderSpec;
        ClusterBind9Provider {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: None, // Cluster-scoped
                finalizers: Some(finalizers),
                deletion_timestamp: Some(Time(k8s_openapi::jiff::Timestamp::now())),
                generation: Some(1),
                ..Default::default()
            },
            spec: ClusterBind9ProviderSpec {
                namespace: None,
                common: Bind9ClusterCommonSpec {
                    version: None,
                    primary: None,
                    secondary: None,
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

    /// The PATCH content type kube sends for a JSON Patch.
    const JSON_PATCH_CONTENT_TYPE: &str = "application/json-patch+json";
    const HTTP_CONFLICT: u16 = 409;
    const RESOURCE_VERSION: &str = "42";
    const OTHER_FINALIZER: &str = "other.firestoned.io/finalizer";

    fn with_resource_version(mut cluster: Bind9Cluster) -> Bind9Cluster {
        cluster.metadata.resource_version = Some(RESOURCE_VERSION.to_string());
        cluster
    }

    fn json<T: serde::Serialize>(obj: &T) -> serde_json::Value {
        serde_json::to_value(obj).expect("serializes")
    }

    // ========================================================================
    // has_finalizer: the decision every helper starts from
    // ========================================================================

    #[test]
    fn has_finalizer_is_false_without_finalizers() {
        assert!(!has_finalizer(&create_test_cluster(), TEST_FINALIZER));
        assert!(!has_finalizer(
            &create_test_cluster_with_finalizers(vec![]),
            TEST_FINALIZER
        ));
    }

    #[test]
    fn has_finalizer_matches_the_exact_string_only() {
        let cluster = create_test_cluster_with_finalizers(vec![OTHER_FINALIZER.to_string()]);
        assert!(!has_finalizer(&cluster, TEST_FINALIZER));
        assert!(has_finalizer(&cluster, OTHER_FINALIZER));
    }

    // ========================================================================
    // ensure_finalizer / remove_finalizer against the mock API server,
    // namespaced and cluster-scoped
    // ========================================================================

    #[tokio::test]
    async fn ensure_finalizer_patches_a_namespaced_resource_when_missing() {
        let cluster = with_resource_version(create_test_cluster());
        let (client, seen) = scripted_client(vec![(HTTP_OK, json(&cluster))]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);

        ensure_finalizer(&api, &cluster, TEST_FINALIZER)
            .await
            .expect("finalizer added");

        let seen = requests(&seen);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "PATCH");
        assert_eq!(seen[0].content_type, JSON_PATCH_CONTENT_TYPE);
        assert!(
            seen[0]
                .path
                .ends_with("/namespaces/test-namespace/bind9clusters/test-resource"),
            "{}",
            seen[0].path
        );
        assert_eq!(
            seen[0].body,
            serde_json::json!([
                {"op": "test", "path": "/metadata/finalizers", "value": null},
                {"op": "add", "path": "/metadata/finalizers", "value": [TEST_FINALIZER]},
            ])
        );
    }

    #[tokio::test]
    async fn ensure_finalizer_appends_with_a_resource_version_guard() {
        let cluster = with_resource_version(create_test_cluster_with_finalizers(vec![
            OTHER_FINALIZER.to_string(),
        ]));
        let (client, seen) = scripted_client(vec![(HTTP_OK, json(&cluster))]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);

        ensure_finalizer(&api, &cluster, TEST_FINALIZER)
            .await
            .expect("finalizer added");

        assert_eq!(
            requests(&seen)[0].body,
            serde_json::json!([
                {"op": "test", "path": "/metadata/resourceVersion", "value": RESOURCE_VERSION},
                {"op": "add", "path": "/metadata/finalizers/-", "value": TEST_FINALIZER},
            ])
        );
    }

    #[tokio::test]
    async fn ensure_finalizer_is_a_no_op_when_present() {
        let cluster = create_test_cluster_with_finalizers(vec![TEST_FINALIZER.to_string()]);
        let (client, seen) = scripted_client(vec![]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);

        ensure_finalizer(&api, &cluster, TEST_FINALIZER)
            .await
            .expect("nothing to do");

        assert!(requests(&seen).is_empty(), "no API call");
    }

    #[tokio::test]
    async fn ensure_finalizer_patches_a_cluster_scoped_resource() {
        let provider = create_test_cluster_provider();
        let (client, seen) = scripted_client(vec![(HTTP_OK, json(&provider))]);
        let api: Api<ClusterBind9Provider> = Api::all(client);

        ensure_finalizer(&api, &provider, TEST_FINALIZER)
            .await
            .expect("finalizer added");

        let seen = requests(&seen);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "PATCH");
        assert!(
            seen[0]
                .path
                .ends_with("/clusterbind9providers/test-resource")
                && !seen[0].path.contains("/namespaces/"),
            "{}",
            seen[0].path
        );
    }

    #[tokio::test]
    async fn ensure_finalizer_fails_when_the_guarded_patch_is_rejected() {
        let cluster = with_resource_version(create_test_cluster());
        let (client, _seen) = scripted_client(vec![(
            HTTP_CONFLICT,
            status_body(HTTP_CONFLICT, "Conflict"),
        )]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);

        let err = ensure_finalizer(&api, &cluster, TEST_FINALIZER)
            .await
            .expect_err("a rejected patch fails");

        let text = format!("{err:#}");
        assert!(text.contains("test-namespace/test-resource"), "{text}");
        assert!(crate::kube_errors::is_conflict_anyhow(&err));
    }

    #[tokio::test]
    async fn ensure_finalizer_requires_a_resource_version_to_append() {
        // Existing finalizers and no resourceVersion: no safe patch exists.
        let cluster = create_test_cluster_with_finalizers(vec![OTHER_FINALIZER.to_string()]);
        let (client, seen) = scripted_client(vec![]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);

        assert!(ensure_finalizer(&api, &cluster, TEST_FINALIZER)
            .await
            .is_err());
        assert!(requests(&seen).is_empty());
    }

    #[tokio::test]
    async fn remove_finalizer_patches_the_exact_index() {
        let cluster = create_test_cluster_with_finalizers(vec![
            OTHER_FINALIZER.to_string(),
            TEST_FINALIZER.to_string(),
        ]);
        let (client, seen) = scripted_client(vec![(HTTP_OK, json(&cluster))]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);

        remove_finalizer(&api, &cluster, TEST_FINALIZER)
            .await
            .expect("finalizer removed");

        let seen = requests(&seen);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "PATCH");
        assert_eq!(
            seen[0].body,
            serde_json::json!([
                {"op": "test", "path": "/metadata/finalizers/1", "value": TEST_FINALIZER},
                {"op": "remove", "path": "/metadata/finalizers/1"},
            ])
        );
    }

    #[tokio::test]
    async fn remove_finalizer_is_a_no_op_when_absent() {
        let cluster = create_test_cluster_with_finalizers(vec![OTHER_FINALIZER.to_string()]);
        let (client, seen) = scripted_client(vec![]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);

        remove_finalizer(&api, &cluster, TEST_FINALIZER)
            .await
            .expect("nothing to do");

        assert!(requests(&seen).is_empty());
    }

    #[tokio::test]
    async fn remove_finalizer_patches_a_cluster_scoped_resource() {
        let provider =
            create_test_cluster_provider_with_finalizers(vec![TEST_FINALIZER.to_string()]);
        let (client, seen) = scripted_client(vec![(HTTP_OK, json(&provider))]);
        let api: Api<ClusterBind9Provider> = Api::all(client);

        remove_finalizer(&api, &provider, TEST_FINALIZER)
            .await
            .expect("finalizer removed");

        let seen = requests(&seen);
        assert_eq!(seen.len(), 1);
        assert!(
            seen[0]
                .path
                .ends_with("/clusterbind9providers/test-resource")
                && !seen[0].path.contains("/namespaces/"),
            "{}",
            seen[0].path
        );
    }

    #[tokio::test]
    async fn remove_finalizer_failure_names_the_cluster_scoped_object() {
        let provider =
            create_test_cluster_provider_with_finalizers(vec![TEST_FINALIZER.to_string()]);
        let (client, _seen) = scripted_client(vec![(
            HTTP_CONFLICT,
            status_body(HTTP_CONFLICT, "Conflict"),
        )]);
        let api: Api<ClusterBind9Provider> = Api::all(client);

        let err = remove_finalizer(&api, &provider, TEST_FINALIZER)
            .await
            .expect_err("a rejected patch fails");

        let text = format!("{err:#}");
        assert!(text.contains("from test-resource"), "{text}");
    }

    #[test]
    fn test_create_test_cluster_has_no_finalizers() {
        let cluster = create_test_cluster();
        assert!(cluster.metadata.finalizers.is_none());
        assert_eq!(cluster.metadata.name.as_ref().unwrap(), TEST_NAME);
        assert_eq!(cluster.metadata.namespace.as_ref().unwrap(), TEST_NAMESPACE);
    }

    #[test]
    fn test_create_test_cluster_with_finalizers_has_finalizers() {
        let cluster = create_test_cluster_with_finalizers(vec![TEST_FINALIZER.to_string()]);
        assert!(cluster.metadata.finalizers.is_some());
        assert_eq!(cluster.metadata.finalizers.as_ref().unwrap().len(), 1);
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&TEST_FINALIZER.to_string()));
    }

    #[test]
    fn test_create_test_cluster_being_deleted_has_deletion_timestamp() {
        let cluster = create_test_cluster_being_deleted(vec![TEST_FINALIZER.to_string()]);
        assert!(cluster.metadata.deletion_timestamp.is_some());
        assert!(cluster.metadata.finalizers.is_some());
    }

    #[test]
    fn test_create_test_cluster_provider_has_no_namespace() {
        let cluster = create_test_cluster_provider();
        assert!(cluster.metadata.namespace.is_none());
        assert_eq!(cluster.metadata.name.as_ref().unwrap(), TEST_NAME);
    }

    #[test]
    fn test_create_test_cluster_provider_with_finalizers_has_finalizers() {
        let cluster =
            create_test_cluster_provider_with_finalizers(vec![TEST_FINALIZER.to_string()]);
        assert!(cluster.metadata.finalizers.is_some());
        assert_eq!(cluster.metadata.finalizers.as_ref().unwrap().len(), 1);
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&TEST_FINALIZER.to_string()));
    }

    #[test]
    fn test_create_test_cluster_provider_being_deleted_has_deletion_timestamp() {
        let cluster = create_test_cluster_provider_being_deleted(vec![TEST_FINALIZER.to_string()]);
        assert!(cluster.metadata.deletion_timestamp.is_some());
        assert!(cluster.metadata.finalizers.is_some());
        assert!(cluster.metadata.namespace.is_none());
    }

    #[test]
    fn test_bind9cluster_kind() {
        use kube::Resource;
        let _cluster = create_test_cluster();
        assert_eq!(Bind9Cluster::kind(&()), "Bind9Cluster");
    }

    #[test]
    fn test_bind9globalcluster_kind() {
        use kube::Resource;
        let _cluster = create_test_cluster_provider();
        assert_eq!(ClusterBind9Provider::kind(&()), "ClusterBind9Provider");
    }

    #[test]
    fn test_cluster_has_finalizer_check() {
        let cluster_without = create_test_cluster();
        let cluster_with = create_test_cluster_with_finalizers(vec![TEST_FINALIZER.to_string()]);

        // Test the logic for checking if finalizer is present
        assert!(cluster_without.metadata.finalizers.is_none());
        assert!(!cluster_without
            .metadata
            .finalizers
            .as_ref()
            .is_some_and(|f| f.contains(&TEST_FINALIZER.to_string())));

        assert!(cluster_with.metadata.finalizers.is_some());
        assert!(cluster_with
            .metadata
            .finalizers
            .as_ref()
            .is_some_and(|f| f.contains(&TEST_FINALIZER.to_string())));
    }

    #[test]
    fn test_cluster_has_deletion_timestamp_check() {
        let cluster_normal = create_test_cluster();
        let cluster_deleting = create_test_cluster_being_deleted(vec![TEST_FINALIZER.to_string()]);

        // Test the logic for checking if resource is being deleted
        assert!(cluster_normal.metadata.deletion_timestamp.is_none());
        assert!(cluster_deleting.metadata.deletion_timestamp.is_some());
    }

    #[test]
    fn test_cluster_provider_has_finalizer_check() {
        let cluster_without = create_test_cluster_provider();
        let cluster_with =
            create_test_cluster_provider_with_finalizers(vec![TEST_FINALIZER.to_string()]);

        // Test the logic for checking if finalizer is present on cluster-scoped resources
        assert!(cluster_without.metadata.finalizers.is_none());
        assert!(!cluster_without
            .metadata
            .finalizers
            .as_ref()
            .is_some_and(|f| f.contains(&TEST_FINALIZER.to_string())));

        assert!(cluster_with.metadata.finalizers.is_some());
        assert!(cluster_with
            .metadata
            .finalizers
            .as_ref()
            .is_some_and(|f| f.contains(&TEST_FINALIZER.to_string())));
    }

    #[test]
    fn test_finalizer_list_manipulation() {
        // Test finalizer list operations that would happen in ensure_finalizer
        let mut finalizers: Vec<String> = vec![];

        // Adding first finalizer
        finalizers.push(TEST_FINALIZER.to_string());
        assert_eq!(finalizers.len(), 1);
        assert!(finalizers.contains(&TEST_FINALIZER.to_string()));

        // Idempotency check - don't add if already present
        if !finalizers.contains(&TEST_FINALIZER.to_string()) {
            finalizers.push(TEST_FINALIZER.to_string());
        }
        assert_eq!(finalizers.len(), 1); // Should still be 1

        // Adding different finalizer
        let other_finalizer = "other.firestoned.io/finalizer";
        finalizers.push(other_finalizer.to_string());
        assert_eq!(finalizers.len(), 2);

        // Removing specific finalizer
        finalizers.retain(|f| f != TEST_FINALIZER);
        assert_eq!(finalizers.len(), 1);
        assert!(!finalizers.contains(&TEST_FINALIZER.to_string()));
        assert!(finalizers.contains(&other_finalizer.to_string()));

        // Removing last finalizer
        finalizers.retain(|f| f != other_finalizer);
        assert_eq!(finalizers.len(), 0);
    }

    #[test]
    fn test_multiple_finalizers_handling() {
        let finalizer1 = "finalizer1.firestoned.io/cleanup";
        let finalizer2 = "finalizer2.firestoned.io/cleanup";
        let finalizer3 = "finalizer3.firestoned.io/cleanup";

        let cluster = create_test_cluster_with_finalizers(vec![
            finalizer1.to_string(),
            finalizer2.to_string(),
            finalizer3.to_string(),
        ]);

        assert_eq!(cluster.metadata.finalizers.as_ref().unwrap().len(), 3);
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&finalizer1.to_string()));
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&finalizer2.to_string()));
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&finalizer3.to_string()));
    }

    #[test]
    fn test_namespace_scoped_vs_cluster_scoped() {
        let ns_scoped = create_test_cluster();
        let cluster_scoped = create_test_cluster_provider();

        // Namespace-scoped resources have a namespace
        assert!(ns_scoped.metadata.namespace.is_some());
        assert_eq!(
            ns_scoped.metadata.namespace.as_ref().unwrap(),
            TEST_NAMESPACE
        );

        // Cluster-scoped resources do NOT have a namespace
        assert!(cluster_scoped.metadata.namespace.is_none());
    }

    #[test]
    fn test_resource_names_are_set() {
        let cluster = create_test_cluster();
        let cluster_provider = create_test_cluster_provider();

        assert!(cluster.metadata.name.is_some());
        assert_eq!(cluster.metadata.name.as_ref().unwrap(), TEST_NAME);

        assert!(cluster_provider.metadata.name.is_some());
        assert_eq!(cluster_provider.metadata.name.as_ref().unwrap(), TEST_NAME);
    }

    #[test]
    fn test_deletion_timestamp_and_finalizer_combination() {
        // Case 1: Being deleted WITH finalizer (needs cleanup)
        let case1 = create_test_cluster_being_deleted(vec![TEST_FINALIZER.to_string()]);
        assert!(case1.metadata.deletion_timestamp.is_some());
        assert!(case1
            .metadata
            .finalizers
            .as_ref()
            .is_some_and(|f| f.contains(&TEST_FINALIZER.to_string())));

        // Case 2: Being deleted WITHOUT finalizer (no cleanup needed)
        let case2 = create_test_cluster_being_deleted(vec![]);
        assert!(case2.metadata.deletion_timestamp.is_some());
        assert!(
            case2.metadata.finalizers.is_none()
                || case2.metadata.finalizers.as_ref().unwrap().is_empty()
        );

        // Case 3: NOT being deleted WITH finalizer (normal operation)
        let case3 = create_test_cluster_with_finalizers(vec![TEST_FINALIZER.to_string()]);
        assert!(case3.metadata.deletion_timestamp.is_none());
        assert!(case3
            .metadata
            .finalizers
            .as_ref()
            .is_some_and(|f| f.contains(&TEST_FINALIZER.to_string())));

        // Case 4: NOT being deleted WITHOUT finalizer (initial state)
        let case4 = create_test_cluster();
        assert!(case4.metadata.deletion_timestamp.is_none());
        assert!(case4.metadata.finalizers.is_none());
    }

    #[test]
    fn test_resource_generation_tracking() {
        let cluster = create_test_cluster();
        assert_eq!(cluster.metadata.generation, Some(1));

        let deleting_cluster = create_test_cluster_being_deleted(vec![TEST_FINALIZER.to_string()]);
        assert_eq!(deleting_cluster.metadata.generation, Some(1));
    }

    #[test]
    fn test_empty_finalizer_list_vs_none() {
        // None vs empty list distinction
        let cluster_none = create_test_cluster();
        let cluster_empty = create_test_cluster_with_finalizers(vec![]);

        // Both should be treated as "no finalizers"
        assert!(cluster_none.metadata.finalizers.is_none());
        assert!(cluster_empty.metadata.finalizers.is_some());
        assert!(cluster_empty
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .is_empty());

        // Logic check: both should not have our finalizer
        assert!(!cluster_none
            .metadata
            .finalizers
            .as_ref()
            .is_some_and(|f| f.contains(&TEST_FINALIZER.to_string())));
        assert!(!cluster_empty
            .metadata
            .finalizers
            .as_ref()
            .is_some_and(|f| f.contains(&TEST_FINALIZER.to_string())));
    }

    // ========================================================================
    // Guarded JSON Patch builders (concurrency-safe finalizer add/remove)
    // ========================================================================

    use crate::finalizers::{build_add_finalizer_patch, build_remove_finalizer_patch};

    #[test]
    fn test_build_add_finalizer_patch_empty_finalizers_tests_for_absence() {
        // With no existing finalizers the patch must test that the array is
        // absent before creating it, so a racing writer's fresh finalizer is
        // never clobbered
        let patch = build_add_finalizer_patch(&[], Some("42"), TEST_FINALIZER).unwrap();
        let value = serde_json::to_value(&patch).unwrap();

        assert_eq!(
            value,
            serde_json::json!([
                {"op": "test", "path": "/metadata/finalizers", "value": null},
                {"op": "add", "path": "/metadata/finalizers", "value": [TEST_FINALIZER]},
            ])
        );
    }

    #[test]
    fn test_build_add_finalizer_patch_existing_finalizers_appends_with_rv_guard() {
        // With existing finalizers the patch must guard on resourceVersion and
        // APPEND (path "-") rather than rewriting the whole array
        let existing = vec!["other.io/finalizer".to_string()];
        let patch = build_add_finalizer_patch(&existing, Some("42"), TEST_FINALIZER).unwrap();
        let value = serde_json::to_value(&patch).unwrap();

        assert_eq!(
            value,
            serde_json::json!([
                {"op": "test", "path": "/metadata/resourceVersion", "value": "42"},
                {"op": "add", "path": "/metadata/finalizers/-", "value": TEST_FINALIZER},
            ])
        );
    }

    #[test]
    fn test_build_add_finalizer_patch_existing_finalizers_requires_resource_version() {
        let existing = vec!["other.io/finalizer".to_string()];
        assert!(build_add_finalizer_patch(&existing, None, TEST_FINALIZER).is_err());
    }

    #[test]
    fn test_build_remove_finalizer_patch_tests_exact_index_content() {
        // The patch must test the exact index content before removing it, so a
        // concurrent add/remove (shifting indices) fails the test server-side
        // instead of removing a FOREIGN finalizer
        let existing = vec![
            "other.io/finalizer".to_string(),
            TEST_FINALIZER.to_string(),
            "third.io/finalizer".to_string(),
        ];
        let patch = build_remove_finalizer_patch(&existing, TEST_FINALIZER)
            .unwrap()
            .expect("finalizer is present, patch expected");
        let value = serde_json::to_value(&patch).unwrap();

        assert_eq!(
            value,
            serde_json::json!([
                {"op": "test", "path": "/metadata/finalizers/1", "value": TEST_FINALIZER},
                {"op": "remove", "path": "/metadata/finalizers/1"},
            ])
        );
    }

    #[test]
    fn test_build_remove_finalizer_patch_absent_finalizer_returns_none() {
        let existing = vec!["other.io/finalizer".to_string()];
        let patch = build_remove_finalizer_patch(&existing, TEST_FINALIZER).unwrap();
        assert!(patch.is_none());
    }

    #[test]
    fn test_build_remove_finalizer_patch_empty_list_returns_none() {
        let patch = build_remove_finalizer_patch(&[], TEST_FINALIZER).unwrap();
        assert!(patch.is_none());
    }

    // =====================================================
    // handle_deletion against the mock API server, both scopes
    // =====================================================

    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn handle_deletion_runs_cleanup_then_removes_the_finalizer() {
        let cluster = create_test_cluster_being_deleted(vec![TEST_FINALIZER.to_string()]);
        let (client, seen) = scripted_client(vec![(HTTP_OK, json(&cluster))]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);
        let cleaned = AtomicBool::new(false);

        handle_deletion(&api, &cluster, TEST_FINALIZER, || async {
            cleaned.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await
        .expect("deletion succeeds");

        assert!(cleaned.load(Ordering::SeqCst), "cleanup must run");
        let seen = requests(&seen);
        assert_eq!(seen.len(), 1, "exactly one API call: the finalizer patch");
        assert_eq!(seen[0].method, "PATCH");
        assert!(
            seen[0]
                .path
                .ends_with("/namespaces/test-namespace/bind9clusters/test-resource"),
            "patched the resource itself: {}",
            seen[0].path
        );
    }

    #[tokio::test]
    async fn handle_deletion_skips_cleanup_when_the_finalizer_is_absent() {
        let cluster = create_test_cluster_being_deleted(vec![]);
        let (client, seen) = scripted_client(vec![]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);
        let cleaned = AtomicBool::new(false);

        handle_deletion(&api, &cluster, TEST_FINALIZER, || async {
            cleaned.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await
        .expect("nothing to do is not an error");

        assert!(!cleaned.load(Ordering::SeqCst), "no finalizer, no cleanup");
        assert!(requests(&seen).is_empty(), "no API call");
    }

    #[tokio::test]
    async fn handle_deletion_keeps_the_finalizer_when_cleanup_fails() {
        let cluster = create_test_cluster_being_deleted(vec![TEST_FINALIZER.to_string()]);
        let (client, seen) = scripted_client(vec![]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, TEST_NAMESPACE);

        let result = handle_deletion(&api, &cluster, TEST_FINALIZER, || async {
            Err(anyhow::anyhow!("child resources still present"))
        })
        .await;

        assert!(result.is_err(), "a failed cleanup fails the deletion");
        assert!(
            requests(&seen).is_empty(),
            "the finalizer must stay until cleanup succeeds"
        );
    }

    #[tokio::test]
    async fn handle_deletion_of_a_cluster_scoped_resource_removes_the_finalizer() {
        let provider = create_test_cluster_provider_being_deleted(vec![TEST_FINALIZER.to_string()]);
        let (client, seen) = scripted_client(vec![(HTTP_OK, json(&provider))]);
        let api: Api<ClusterBind9Provider> = Api::all(client);
        let cleaned = AtomicBool::new(false);

        handle_deletion(&api, &provider, TEST_FINALIZER, || async {
            cleaned.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await
        .expect("deletion succeeds");

        assert!(cleaned.load(Ordering::SeqCst), "cleanup must run");
        let seen = requests(&seen);
        assert_eq!(seen.len(), 1, "exactly one API call: the finalizer patch");
        assert_eq!(seen[0].method, "PATCH");
        assert!(
            seen[0]
                .path
                .ends_with("/clusterbind9providers/test-resource")
                && !seen[0].path.contains("/namespaces/"),
            "patched the cluster-scoped resource: {}",
            seen[0].path
        );
    }

    #[tokio::test]
    async fn handle_deletion_of_a_cluster_scoped_resource_skips_cleanup_without_finalizer() {
        let provider = create_test_cluster_provider_being_deleted(vec![]);
        let (client, seen) = scripted_client(vec![]);
        let api: Api<ClusterBind9Provider> = Api::all(client);
        let cleaned = AtomicBool::new(false);

        handle_deletion(&api, &provider, TEST_FINALIZER, || async {
            cleaned.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await
        .expect("nothing to do is not an error");

        assert!(!cleaned.load(Ordering::SeqCst));
        assert!(requests(&seen).is_empty());
    }

    #[tokio::test]
    async fn handle_deletion_of_a_cluster_scoped_resource_keeps_finalizer_on_failure() {
        let provider = create_test_cluster_provider_being_deleted(vec![TEST_FINALIZER.to_string()]);
        let (client, seen) = scripted_client(vec![]);
        let api: Api<ClusterBind9Provider> = Api::all(client);

        let result = handle_deletion(&api, &provider, TEST_FINALIZER, || async {
            Err(anyhow::anyhow!("instances still present"))
        })
        .await;

        assert!(result.is_err());
        assert!(requests(&seen).is_empty());
    }
}
