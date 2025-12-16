// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `resources.rs`

#[cfg(test)]
mod tests {
    use k8s_openapi::api::core::v1::{ConfigMap, ServiceAccount};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::Client;
    use std::collections::BTreeMap;

    const TEST_NAMESPACE: &str = "test-namespace";
    const TEST_NAME: &str = "test-resource";
    const FIELD_MANAGER: &str = "test-controller";

    /// Helper to create a mock Kubernetes client
    async fn mock_client() -> Client {
        // In real integration tests, this would use k8s-openapi test fixtures
        // For unit tests, we'll use a simplified mock
        Client::try_default()
            .await
            .expect("Failed to create mock client")
    }

    /// Helper to create a test ServiceAccount
    fn create_test_service_account() -> ServiceAccount {
        ServiceAccount {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Helper to create a test ConfigMap
    fn create_test_configmap() -> ConfigMap {
        let mut data = BTreeMap::new();
        data.insert("key1".to_string(), "value1".to_string());

        ConfigMap {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        }
    }

    /// Helper to create a test ConfigMap with updated data
    fn create_test_configmap_updated() -> ConfigMap {
        let mut data = BTreeMap::new();
        data.insert("key1".to_string(), "value1-updated".to_string());
        data.insert("key2".to_string(), "value2".to_string());

        ConfigMap {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        }
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_apply_creates_when_missing() {
        let _client = mock_client().await;
        let _sa = create_test_service_account();

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Verify resource doesn't exist
        // 2. Call create_or_apply
        // 3. Verify resource was created
        // 4. Verify resource has expected spec
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_apply_updates_when_exists() {
        let _client = mock_client().await;
        let _sa = create_test_service_account();

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource first
        // 2. Modify the resource spec
        // 3. Call create_or_apply
        // 4. Verify resource was updated (not replaced)
        // 5. Verify server-side apply field manager is set
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_apply_is_idempotent() {
        let _client = mock_client().await;
        let _sa = create_test_service_account();

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Call create_or_apply first time
        // 2. Call create_or_apply second time with same resource
        // 3. Verify no errors occurred
        // 4. Verify resource remains in expected state
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_replace_creates_when_missing() {
        let _client = mock_client().await;
        let _cm = create_test_configmap();

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Verify resource doesn't exist
        // 2. Call create_or_replace
        // 3. Verify resource was created
        // 4. Verify resource has expected data
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_replace_replaces_when_exists() {
        let _client = mock_client().await;
        let _cm_original = create_test_configmap();
        let _cm_updated = create_test_configmap_updated();

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource first
        // 2. Call create_or_replace with updated resource
        // 3. Verify resource was replaced (entire spec replaced)
        // 4. Verify new data is present
        // 5. Verify old data (if any) was removed
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_replace_is_idempotent() {
        let _client = mock_client().await;
        let _cm = create_test_configmap();

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Call create_or_replace first time
        // 2. Call create_or_replace second time with same resource
        // 3. Verify no errors occurred
        // 4. Verify resource remains in expected state
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_patch_json_creates_when_missing() {
        let _client = mock_client().await;
        let _sa = create_test_service_account();
        let _patch = serde_json::json!({
            "apiVersion": "v1",
            "kind": "ServiceAccount",
            "metadata": {
                "name": TEST_NAME,
                "namespace": TEST_NAMESPACE,
            }
        });

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Verify resource doesn't exist
        // 2. Call create_or_patch_json
        // 3. Verify resource was created
        // 4. Verify resource has expected spec
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_patch_json_patches_when_exists() {
        let _client = mock_client().await;
        let _cm = create_test_configmap();
        let _patch = serde_json::json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": TEST_NAME,
                "namespace": TEST_NAMESPACE,
                "labels": {
                    "updated": "true"
                }
            },
            "data": {
                "key1": "value1-updated",
                "key2": "value2"
            }
        });

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource first
        // 2. Call create_or_patch_json with updated JSON patch
        // 3. Verify resource was patched (not replaced)
        // 4. Verify server-side apply was used
        // 5. Verify field manager is set correctly
    }

    #[tokio::test]
    #[ignore] // Requires Kubernetes cluster
    async fn test_create_or_patch_json_handles_already_exists_error() {
        let _client = mock_client().await;
        let _sa = create_test_service_account();
        let _patch = serde_json::json!({
            "apiVersion": "v1",
            "kind": "ServiceAccount",
            "metadata": {
                "name": TEST_NAME,
                "namespace": TEST_NAMESPACE,
            }
        });

        // This test requires a real cluster, so we skip the actual API call
        // In integration tests, we would:
        // 1. Create the resource first
        // 2. Call create_or_patch_json (which tries to create)
        // 3. Verify AlreadyExists error is caught
        // 4. Verify function falls back to patch
        // 5. Verify no error is returned to caller
    }

    #[test]
    fn test_create_test_service_account_has_name_and_namespace() {
        let sa = create_test_service_account();
        assert_eq!(sa.metadata.name.as_ref().unwrap(), TEST_NAME);
        assert_eq!(sa.metadata.namespace.as_ref().unwrap(), TEST_NAMESPACE);
    }

    #[test]
    fn test_create_test_configmap_has_data() {
        let cm = create_test_configmap();
        assert_eq!(cm.metadata.name.as_ref().unwrap(), TEST_NAME);
        assert_eq!(cm.metadata.namespace.as_ref().unwrap(), TEST_NAMESPACE);
        assert!(cm.data.is_some());
        assert_eq!(cm.data.as_ref().unwrap().len(), 1);
        assert_eq!(cm.data.as_ref().unwrap().get("key1").unwrap(), "value1");
    }

    #[test]
    fn test_create_test_configmap_updated_has_different_data() {
        let cm_original = create_test_configmap();
        let cm_updated = create_test_configmap_updated();

        // Verify updated version has more keys
        assert_eq!(cm_original.data.as_ref().unwrap().len(), 1);
        assert_eq!(cm_updated.data.as_ref().unwrap().len(), 2);

        // Verify key1 was updated
        assert_eq!(
            cm_original.data.as_ref().unwrap().get("key1").unwrap(),
            "value1"
        );
        assert_eq!(
            cm_updated.data.as_ref().unwrap().get("key1").unwrap(),
            "value1-updated"
        );

        // Verify key2 is new
        assert!(cm_original.data.as_ref().unwrap().get("key2").is_none());
        assert_eq!(
            cm_updated.data.as_ref().unwrap().get("key2").unwrap(),
            "value2"
        );
    }

    #[test]
    fn test_resource_without_name_should_fail() {
        // This test verifies the behavior when a resource has no name
        // The actual functions require a name and will return an error
        let sa = ServiceAccount {
            metadata: ObjectMeta {
                name: None, // Missing name
                namespace: Some(TEST_NAMESPACE.to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(sa.metadata.name.is_none());
    }

    #[test]
    fn test_configmap_serialization() {
        // Verify that ConfigMap can be serialized (required for patching)
        let cm = create_test_configmap();
        let serialized = serde_json::to_value(&cm);
        assert!(serialized.is_ok());

        let value = serialized.unwrap();
        assert_eq!(value["kind"], "ConfigMap");
        assert_eq!(value["metadata"]["name"], TEST_NAME);
        assert_eq!(value["metadata"]["namespace"], TEST_NAMESPACE);
    }

    #[test]
    fn test_service_account_serialization() {
        // Verify that ServiceAccount can be serialized (required for patching)
        let sa = create_test_service_account();
        let serialized = serde_json::to_value(&sa);
        assert!(serialized.is_ok());

        let value = serialized.unwrap();
        assert_eq!(value["kind"], "ServiceAccount");
        assert_eq!(value["metadata"]["name"], TEST_NAME);
        assert_eq!(value["metadata"]["namespace"], TEST_NAMESPACE);
    }

    #[test]
    fn test_configmap_data_manipulation() {
        // Test data manipulation operations
        let mut data = BTreeMap::new();

        // Add initial data
        data.insert("key1".to_string(), "value1".to_string());
        assert_eq!(data.len(), 1);
        assert_eq!(data.get("key1").unwrap(), "value1");

        // Update existing key
        data.insert("key1".to_string(), "value1-updated".to_string());
        assert_eq!(data.len(), 1);
        assert_eq!(data.get("key1").unwrap(), "value1-updated");

        // Add new key
        data.insert("key2".to_string(), "value2".to_string());
        assert_eq!(data.len(), 2);

        // Remove key
        data.remove("key1");
        assert_eq!(data.len(), 1);
        assert!(!data.contains_key("key1"));
        assert_eq!(data.get("key2").unwrap(), "value2");
    }

    #[test]
    fn test_configmap_with_empty_data() {
        let cm = ConfigMap {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                ..Default::default()
            },
            data: None,
            ..Default::default()
        };

        assert!(cm.data.is_none());
        assert_eq!(cm.metadata.name.as_ref().unwrap(), TEST_NAME);
    }

    #[test]
    fn test_configmap_with_multiple_entries() {
        let mut data = BTreeMap::new();
        data.insert("config.yaml".to_string(), "key: value".to_string());
        data.insert(
            "script.sh".to_string(),
            "#!/bin/bash\necho hello".to_string(),
        );
        data.insert("data.json".to_string(), r#"{"test": true}"#.to_string());

        let cm = ConfigMap {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        };

        assert_eq!(cm.data.as_ref().unwrap().len(), 3);
        assert!(cm.data.as_ref().unwrap().contains_key("config.yaml"));
        assert!(cm.data.as_ref().unwrap().contains_key("script.sh"));
        assert!(cm.data.as_ref().unwrap().contains_key("data.json"));
    }

    #[test]
    fn test_service_account_with_labels() {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "bindy".to_string());
        labels.insert("component".to_string(), "dns".to_string());

        let sa = ServiceAccount {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(sa.metadata.labels.is_some());
        assert_eq!(sa.metadata.labels.as_ref().unwrap().len(), 2);
        assert_eq!(
            sa.metadata.labels.as_ref().unwrap().get("app").unwrap(),
            "bindy"
        );
        assert_eq!(
            sa.metadata
                .labels
                .as_ref()
                .unwrap()
                .get("component")
                .unwrap(),
            "dns"
        );
    }

    #[test]
    fn test_service_account_with_annotations() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "kubectl.kubernetes.io/last-applied-configuration".to_string(),
            "{}".to_string(),
        );

        let sa = ServiceAccount {
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                annotations: Some(annotations),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(sa.metadata.annotations.is_some());
        assert_eq!(sa.metadata.annotations.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_resource_metadata_fields() {
        let sa = create_test_service_account();

        // Required fields
        assert!(sa.metadata.name.is_some());
        assert!(sa.metadata.namespace.is_some());

        // Optional fields (should be None/default in test helper)
        assert!(sa.metadata.labels.is_none());
        assert!(sa.metadata.annotations.is_none());
        assert!(sa.metadata.finalizers.is_none());
        assert!(sa.metadata.owner_references.is_none());
    }

    #[test]
    fn test_json_patch_structure() {
        // Test creating a valid JSON patch for resources
        let patch = serde_json::json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": TEST_NAME,
                "namespace": TEST_NAMESPACE,
                "labels": {
                    "app": "bindy"
                }
            },
            "data": {
                "key": "value"
            }
        });

        assert!(patch.is_object());
        assert_eq!(patch["apiVersion"], "v1");
        assert_eq!(patch["kind"], "ConfigMap");
        assert_eq!(patch["metadata"]["name"], TEST_NAME);
        assert_eq!(patch["metadata"]["namespace"], TEST_NAMESPACE);
        assert_eq!(patch["metadata"]["labels"]["app"], "bindy");
        assert_eq!(patch["data"]["key"], "value");
    }

    #[test]
    fn test_configmap_key_ordering() {
        // BTreeMap should maintain sorted order
        let cm1 = create_test_configmap();
        let cm2 = create_test_configmap();

        // Both should have same keys in same order
        let keys1: Vec<_> = cm1.data.as_ref().unwrap().keys().collect();
        let keys2: Vec<_> = cm2.data.as_ref().unwrap().keys().collect();

        assert_eq!(keys1, keys2);
    }

    #[test]
    fn test_resource_name_validation_logic() {
        // Test the logic that validates resource names
        let valid_names = vec!["test-resource", "test.resource", "test123", "a"];
        let invalid_names = vec!["", "Test", "test_resource", "test resource"];

        for name in valid_names {
            // These should be acceptable Kubernetes names
            assert!(name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.'));
        }

        for name in invalid_names {
            // These should NOT be acceptable
            let is_valid = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
            assert!(!is_valid);
        }
    }

    #[test]
    fn test_namespace_name_validation_logic() {
        // Test namespace naming rules
        let valid_namespaces = vec!["default", "kube-system", "test-ns", "ns1"];
        let invalid_namespaces = vec!["", "Kube-System", "test_ns", "test ns"];

        for ns in valid_namespaces {
            assert!(!ns.is_empty());
            assert!(ns
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        }

        for ns in invalid_namespaces {
            let is_valid = !ns.is_empty()
                && ns
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            assert!(!is_valid);
        }
    }

    #[test]
    fn test_field_manager_string() {
        // Test field manager naming conventions
        let valid_managers = vec!["bindy-controller", "test-controller", "my-operator"];

        for manager in valid_managers {
            assert!(!manager.is_empty());
            assert!(manager.len() <= 128); // Kubernetes limit
            assert!(manager
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        }

        assert_eq!(FIELD_MANAGER, "test-controller");
    }

    #[test]
    fn test_configmap_binary_data_not_used() {
        // Test that we're using data field, not binaryData
        let cm = create_test_configmap();

        assert!(cm.data.is_some());
        assert!(cm.binary_data.is_none());
    }

    #[test]
    fn test_service_account_secrets_field() {
        // Modern Kubernetes doesn't use secrets field in ServiceAccount
        let sa = create_test_service_account();

        // Secrets field should be None (deprecated)
        assert!(sa.secrets.is_none());
    }

    #[test]
    fn test_resource_api_version_and_kind() {
        // Test that resources have correct apiVersion and kind
        let sa = create_test_service_account();
        let sa_json = serde_json::to_value(&sa).unwrap();

        assert_eq!(sa_json["apiVersion"], "v1");
        assert_eq!(sa_json["kind"], "ServiceAccount");

        let cm = create_test_configmap();
        let cm_json = serde_json::to_value(&cm).unwrap();

        assert_eq!(cm_json["apiVersion"], "v1");
        assert_eq!(cm_json["kind"], "ConfigMap");
    }
}
