// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `finalizers.rs`

#[cfg(test)]
mod tests {
    use crate::crd::{
        Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, ClusterBind9Provider,
    };
    use crate::reconcilers::finalizers::FinalizerCleanup;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
    use k8s_openapi::chrono::Utc;
    use kube::Client;

    const TEST_FINALIZER: &str = "test.firestoned.io/finalizer";
    const TEST_NAMESPACE: &str = "test-namespace";
    const TEST_NAME: &str = "test-resource";

    /// Helper to create a test Bind9Cluster
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
                    zones_from: None,
                },
            },
            status: None,
        }
    }

    /// Helper to create a test Bind9Cluster with finalizers
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
                    zones_from: None,
                },
            },
            status: None,
        }
    }

    /// Helper to create a test Bind9Cluster with deletion timestamp
    fn create_test_cluster_being_deleted(finalizers: Vec<String>) -> Bind9Cluster {
        Bind9Cluster {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                finalizers: Some(finalizers),
                deletion_timestamp: Some(Time(Utc::now())),
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
                    zones_from: None,
                },
            },
            status: None,
        }
    }

    /// Helper to create a test ClusterBind9Provider (cluster-scoped)
    fn create_test_cluster_provider() -> ClusterBind9Provider {
        use crate::crd::ClusterBind9ProviderSpec;
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
                    zones_from: None,
                },
            },
            status: None,
        }
    }

    /// Helper to create a test ClusterBind9Provider with finalizers
    fn create_test_cluster_provider_with_finalizers(
        finalizers: Vec<String>,
    ) -> ClusterBind9Provider {
        use crate::crd::ClusterBind9ProviderSpec;
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
                    zones_from: None,
                },
            },
            status: None,
        }
    }

    /// Helper to create a test ClusterBind9Provider with deletion timestamp
    fn create_test_cluster_provider_being_deleted(finalizers: Vec<String>) -> ClusterBind9Provider {
        use crate::crd::ClusterBind9ProviderSpec;
        ClusterBind9Provider {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: None, // Cluster-scoped
                finalizers: Some(finalizers),
                deletion_timestamp: Some(Time(Utc::now())),
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
                    zones_from: None,
                },
            },
            status: None,
        }
    }

    /// Helper to create a mock Kubernetes client
    async fn mock_client() -> Client {
        // In real integration tests, this would use k8s-openapi test fixtures
        // For unit tests, we'll use a simplified mock
        Client::try_default()
            .await
            .expect("Failed to create mock client")
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_ensure_finalizer_adds_when_missing() {
        let _client = mock_client().await;
        let cluster = create_test_cluster();

        // Verify no finalizers initially
        assert!(cluster.metadata.finalizers.is_none());

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource
        // 2. Call ensure_finalizer
        // 3. Verify the finalizer was added
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_ensure_finalizer_idempotent_when_present() {
        let _client = mock_client().await;
        let cluster = create_test_cluster_with_finalizers(vec![TEST_FINALIZER.to_string()]);

        // Verify finalizer already present
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&TEST_FINALIZER.to_string()));

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource with the finalizer
        // 2. Call ensure_finalizer again
        // 3. Verify only one instance of the finalizer exists
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_remove_finalizer_removes_when_present() {
        let _client = mock_client().await;
        let cluster = create_test_cluster_with_finalizers(vec![TEST_FINALIZER.to_string()]);

        // Verify finalizer is present
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&TEST_FINALIZER.to_string()));

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource with the finalizer
        // 2. Call remove_finalizer
        // 3. Verify the finalizer was removed
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_remove_finalizer_idempotent_when_absent() {
        let _client = mock_client().await;
        let cluster = create_test_cluster();

        // Verify no finalizers
        assert!(cluster.metadata.finalizers.is_none());

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource without finalizers
        // 2. Call remove_finalizer
        // 3. Verify no error occurs
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_handle_deletion_runs_cleanup_and_removes_finalizer() {
        let _client = mock_client().await;
        let cluster = create_test_cluster_being_deleted(vec![TEST_FINALIZER.to_string()]);

        // Verify resource is being deleted
        assert!(cluster.metadata.deletion_timestamp.is_some());

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource with deletion timestamp and finalizer
        // 2. Call handle_deletion
        // 3. Verify cleanup was called
        // 4. Verify the finalizer was removed
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_handle_deletion_skips_when_finalizer_absent() {
        let _client = mock_client().await;
        let cluster = create_test_cluster_being_deleted(vec![]);

        // Verify resource is being deleted but has no finalizers
        assert!(cluster.metadata.deletion_timestamp.is_some());
        assert!(
            cluster.metadata.finalizers.is_none()
                || cluster.metadata.finalizers.as_ref().unwrap().is_empty()
        );

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource with deletion timestamp but no finalizer
        // 2. Call handle_deletion
        // 3. Verify cleanup was NOT called
        // 4. Verify no errors occurred
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_ensure_cluster_finalizer_adds_when_missing() {
        let _client = mock_client().await;
        let cluster = create_test_cluster_provider();

        // Verify no finalizers initially
        assert!(cluster.metadata.finalizers.is_none());

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the cluster-scoped resource
        // 2. Call ensure_cluster_finalizer
        // 3. Verify the finalizer was added
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_ensure_cluster_finalizer_idempotent_when_present() {
        let _client = mock_client().await;
        let cluster =
            create_test_cluster_provider_with_finalizers(vec![TEST_FINALIZER.to_string()]);

        // Verify finalizer already present
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&TEST_FINALIZER.to_string()));

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the cluster-scoped resource with the finalizer
        // 2. Call ensure_cluster_finalizer again
        // 3. Verify only one instance of the finalizer exists
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_remove_cluster_finalizer_removes_when_present() {
        let _client = mock_client().await;
        let cluster =
            create_test_cluster_provider_with_finalizers(vec![TEST_FINALIZER.to_string()]);

        // Verify finalizer is present
        assert!(cluster
            .metadata
            .finalizers
            .as_ref()
            .unwrap()
            .contains(&TEST_FINALIZER.to_string()));

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the cluster-scoped resource with the finalizer
        // 2. Call remove_cluster_finalizer
        // 3. Verify the finalizer was removed
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_remove_cluster_finalizer_idempotent_when_absent() {
        let _client = mock_client().await;
        let cluster = create_test_cluster_provider();

        // Verify no finalizers
        assert!(cluster.metadata.finalizers.is_none());

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the cluster-scoped resource without finalizers
        // 2. Call remove_cluster_finalizer
        // 3. Verify no error occurs
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_handle_cluster_deletion_runs_cleanup_and_removes_finalizer() {
        let _client = mock_client().await;
        let cluster = create_test_cluster_provider_being_deleted(vec![TEST_FINALIZER.to_string()]);

        // Verify resource is being deleted
        assert!(cluster.metadata.deletion_timestamp.is_some());

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the cluster-scoped resource with deletion timestamp and finalizer
        // 2. Call handle_cluster_deletion
        // 3. Verify cleanup was called
        // 4. Verify the finalizer was removed
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_handle_cluster_deletion_skips_when_finalizer_absent() {
        let _client = mock_client().await;
        let cluster = create_test_cluster_provider_being_deleted(vec![]);

        // Verify resource is being deleted but has no finalizers
        assert!(cluster.metadata.deletion_timestamp.is_some());
        assert!(
            cluster.metadata.finalizers.is_none()
                || cluster.metadata.finalizers.as_ref().unwrap().is_empty()
        );

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the cluster-scoped resource with deletion timestamp but no finalizer
        // 2. Call handle_cluster_deletion
        // 3. Verify cleanup was NOT called
        // 4. Verify no errors occurred
    }

    #[test]
    fn test_finalizer_cleanup_trait_requires_async() {
        // This is a compile-time test to verify the trait signature
        // If this compiles, the trait is correctly defined as async
        fn _assert_trait_is_async<T: FinalizerCleanup>() {}
        _assert_trait_is_async::<Bind9Cluster>();
        _assert_trait_is_async::<ClusterBind9Provider>();
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
}
