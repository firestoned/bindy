// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `instances.rs`

#[cfg(test)]
mod tests {
    use crate::crd::{Bind9Instance, InstanceSource};
    use crate::instances::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn single_shard<K>(store: kube::runtime::reflector::Store<K>) -> crate::context::MultiStore<K>
    where
        K: kube::Resource + Clone + 'static,
        K::DynamicType: std::hash::Hash + Eq + Clone + std::fmt::Debug + Default,
    {
        crate::context::MultiStore::new(vec![store])
    }

    // ========================================================================
    // Zone-to-instance selection (moved from the zone controller's validation tests)
    // ========================================================================

    /// Helper to create a `Bind9Instance` with specific labels
    fn create_test_instance_with_labels(
        name: &str,
        namespace: &str,
        labels: &std::collections::BTreeMap<String, String>,
    ) -> Bind9Instance {
        use serde_json::json;

        let instance_json = json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": {
                "name": name,
                "namespace": namespace,
                "labels": labels,
                "uid": format!("uid-{}", name),
            },
            "spec": {
                "clusterRef": "test-cluster",
                "role": "primary",
            }
        });

        serde_json::from_value(instance_json).expect("Failed to create test instance")
    }

    /// Helper to create a `DNSZone` with `bind9_instances_from` selectors
    fn create_test_zone_with_selectors(
        name: &str,
        namespace: &str,
        bind9_instances_from: Option<Vec<InstanceSource>>,
    ) -> crate::crd::DNSZone {
        use serde_json::json;

        let mut zone_json = json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": name,
                "namespace": namespace,
            },
            "spec": {
                "zoneName": "example.com",
                "soaRecord": {
                    "primaryNs": "ns1.example.com.",
                    "adminEmail": "admin.example.com.",
                    "serial": 1,
                    "refresh": 3600,
                    "retry": 1800,
                    "expire": 604_800,
                    "negativeTtl": 86400
                },
                "ttl": 3600,
                "nameServerIPs": ["192.168.1.1"]
            }
        });

        if let Some(sources) = bind9_instances_from {
            zone_json["spec"]["bind9InstancesFrom"] =
                serde_json::to_value(sources).expect("Failed to serialize bind9_instances_from");
        }

        serde_json::from_value(zone_json).expect("Failed to create test zone")
    }

    #[test]
    fn test_get_instances_no_selectors() {
        let zone = create_test_zone_with_selectors("test-zone", "default", None);
        let (store, _writer) = kube::runtime::reflector::store::<Bind9Instance>();
        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no bind9_instances_from selectors"));
    }

    #[test]
    fn test_get_instances_empty_selectors() {
        let zone = create_test_zone_with_selectors("test-zone", "default", Some(vec![]));
        let (store, _writer) = kube::runtime::reflector::store::<Bind9Instance>();
        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no bind9_instances_from selectors"));
    }

    #[test]
    fn test_get_instances_match_labels() {
        let mut instance_labels = std::collections::BTreeMap::new();
        instance_labels.insert("environment".to_string(), "production".to_string());
        instance_labels.insert("role".to_string(), "primary".to_string());
        let instance = create_test_instance_with_labels("dns-primary", "default", &instance_labels);

        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance.clone()));

        let mut match_labels = std::collections::BTreeMap::new();
        match_labels.insert("environment".to_string(), "production".to_string());
        let bind9_instances_from = vec![InstanceSource {
            selector: crate::crd::LabelSelector {
                match_labels: Some(match_labels),
                match_expressions: None,
            },
        }];
        let zone =
            create_test_zone_with_selectors("test-zone", "default", Some(bind9_instances_from));

        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(result.is_ok());
        let instances = result.unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].name, "dns-primary");
        assert_eq!(instances[0].namespace, "default");
    }

    #[test]
    fn test_get_instances_no_match() {
        let mut instance_labels = std::collections::BTreeMap::new();
        instance_labels.insert("environment".to_string(), "development".to_string());
        let instance = create_test_instance_with_labels("dns-dev", "default", &instance_labels);

        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance));

        let mut match_labels = std::collections::BTreeMap::new();
        match_labels.insert("environment".to_string(), "production".to_string());
        let bind9_instances_from = vec![InstanceSource {
            selector: crate::crd::LabelSelector {
                match_labels: Some(match_labels),
                match_expressions: None,
            },
        }];
        let zone =
            create_test_zone_with_selectors("test-zone", "default", Some(bind9_instances_from));

        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no instances matching"));
    }

    #[test]
    fn test_get_instances_or_logic() {
        let mut prod_labels = std::collections::BTreeMap::new();
        prod_labels.insert("environment".to_string(), "production".to_string());
        let prod_instance = create_test_instance_with_labels("dns-prod", "default", &prod_labels);

        let mut staging_labels = std::collections::BTreeMap::new();
        staging_labels.insert("environment".to_string(), "staging".to_string());
        let staging_instance =
            create_test_instance_with_labels("dns-staging", "default", &staging_labels);

        let mut dev_labels = std::collections::BTreeMap::new();
        dev_labels.insert("environment".to_string(), "development".to_string());
        let dev_instance = create_test_instance_with_labels("dns-dev", "default", &dev_labels);

        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(prod_instance));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(staging_instance));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(dev_instance));

        let mut prod_match = std::collections::BTreeMap::new();
        prod_match.insert("environment".to_string(), "production".to_string());
        let mut staging_match = std::collections::BTreeMap::new();
        staging_match.insert("environment".to_string(), "staging".to_string());

        let bind9_instances_from = vec![
            InstanceSource {
                selector: crate::crd::LabelSelector {
                    match_labels: Some(prod_match),
                    match_expressions: None,
                },
            },
            InstanceSource {
                selector: crate::crd::LabelSelector {
                    match_labels: Some(staging_match),
                    match_expressions: None,
                },
            },
        ];
        let zone =
            create_test_zone_with_selectors("test-zone", "default", Some(bind9_instances_from));

        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(result.is_ok());
        let instances = result.unwrap();
        assert_eq!(instances.len(), 2);
        let names: Vec<String> = instances.iter().map(|i| i.name.clone()).collect();
        assert!(names.contains(&"dns-prod".to_string()));
        assert!(names.contains(&"dns-staging".to_string()));
        assert!(!names.contains(&"dns-dev".to_string()));
    }

    /// Helper: like `create_test_instance_with_labels`, but also stamps the
    /// platform-admin annotation that grants cross-namespace zone access.
    fn create_test_instance_with_labels_and_annotation(
        name: &str,
        namespace: &str,
        labels: &std::collections::BTreeMap<String, String>,
        allow_zone_namespaces: &str,
    ) -> Bind9Instance {
        let mut inst = create_test_instance_with_labels(name, namespace, labels);
        let mut annotations = std::collections::BTreeMap::new();
        annotations.insert(
            crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES.to_string(),
            allow_zone_namespaces.to_string(),
        );
        inst.metadata.annotations = Some(annotations);
        inst
    }

    /// F-003: a `DNSZone` whose label selector matches a `Bind9Instance`
    /// in *another namespace* is rejected by default. The platform admin
    /// must opt that namespace in via the
    /// `bindy.firestoned.io/allow-zone-namespaces` annotation on the
    /// `Bind9Instance`.
    #[test]
    fn test_get_instances_cross_namespace_denied_by_default() {
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("app".to_string(), "bind9".to_string());

        // No allow annotation on either instance.
        let instance_a = create_test_instance_with_labels("dns-1", "namespace-a", &labels);
        let instance_b = create_test_instance_with_labels("dns-2", "namespace-b", &labels);

        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance_a));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance_b));

        let bind9_instances_from = vec![InstanceSource {
            selector: crate::crd::LabelSelector {
                match_labels: Some(labels),
                match_expressions: None,
            },
        }];
        let zone =
            create_test_zone_with_selectors("test-zone", "default", Some(bind9_instances_from));

        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(
            result.is_err(),
            "F-003: cross-namespace match without instance annotation must be rejected"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("F-003 namespace gate"),
            "rejection message must reference the namespace gate, got: {msg}"
        );
    }

    /// F-003: cross-namespace targeting works when the platform admin
    /// stamps the `bindy.firestoned.io/allow-zone-namespaces` annotation
    /// on the target `Bind9Instance` and lists the zone's namespace.
    #[test]
    fn test_get_instances_cross_namespace_allowed_by_annotation() {
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("app".to_string(), "bind9".to_string());
        labels.insert("environment".to_string(), "production".to_string());

        // Platform admin annotates two production instances to allow
        // tenant-a.
        let inst1 = create_test_instance_with_labels_and_annotation(
            "primary-1",
            "bindy-system",
            &labels,
            "tenant-a",
        );
        let inst2 = create_test_instance_with_labels_and_annotation(
            "primary-2",
            "bindy-system",
            &labels,
            "tenant-a,tenant-b",
        );

        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(inst1));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(inst2));

        let bind9_instances_from = vec![InstanceSource {
            selector: crate::crd::LabelSelector {
                match_labels: Some(labels),
                match_expressions: None,
            },
        }];
        let zone =
            create_test_zone_with_selectors("tenant-zone", "tenant-a", Some(bind9_instances_from));

        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(result.is_ok(), "annotation opt-in must succeed: {result:?}");
        let found = result.unwrap();
        assert_eq!(found.len(), 2);
        for inst in &found {
            assert_eq!(inst.namespace, "bindy-system");
        }
    }

    /// F-003: the wildcard value `*` re-enables cluster-wide cross-namespace
    /// matching. This is the explicit-opt-in escape hatch for platform
    /// admins who really want the pre-F-003 behaviour.
    #[test]
    fn test_get_instances_cross_namespace_wildcard_annotation() {
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("app".to_string(), "bind9".to_string());

        let inst = create_test_instance_with_labels_and_annotation(
            "wide-open",
            "bindy-system",
            &labels,
            "*",
        );
        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(inst));

        let bind9_instances_from = vec![InstanceSource {
            selector: crate::crd::LabelSelector {
                match_labels: Some(labels),
                match_expressions: None,
            },
        }];
        let zone = create_test_zone_with_selectors(
            "any-tenant-zone",
            "some-random-tenant",
            Some(bind9_instances_from),
        );

        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(result.is_ok(), "wildcard '*' must allow any namespace");
        assert_eq!(result.unwrap().len(), 1);
    }

    /// F-003: an annotation that *omits* the requesting zone's namespace
    /// still rejects that namespace, even if other namespaces are listed.
    #[test]
    fn test_get_instances_cross_namespace_annotation_excludes_other_namespaces() {
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("app".to_string(), "bind9".to_string());

        // Annotation lists tenant-a only.
        let inst = create_test_instance_with_labels_and_annotation(
            "primary-1",
            "bindy-system",
            &labels,
            "tenant-a",
        );
        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(inst));

        let bind9_instances_from = vec![InstanceSource {
            selector: crate::crd::LabelSelector {
                match_labels: Some(labels),
                match_expressions: None,
            },
        }];
        // Zone in tenant-b — not in the allow-list.
        let zone = create_test_zone_with_selectors(
            "tenant-b-zone",
            "tenant-b",
            Some(bind9_instances_from),
        );

        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(
            result.is_err(),
            "annotation that does not list the zone's namespace must reject"
        );
    }

    /// F-003 happy path: same-namespace targeting always works without
    /// any annotation on the target instance.
    #[test]
    fn test_get_instances_same_namespace_always_allowed() {
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("app".to_string(), "bind9".to_string());
        let inst = create_test_instance_with_labels("local-1", "tenant-a", &labels);

        let (instance_store, mut iwriter) = kube::runtime::reflector::store::<Bind9Instance>();
        iwriter.apply_watcher_event(&kube::runtime::watcher::Event::Apply(inst));

        let bind9_instances_from = vec![InstanceSource {
            selector: crate::crd::LabelSelector {
                match_labels: Some(labels),
                match_expressions: None,
            },
        }];
        let zone =
            create_test_zone_with_selectors("tenant-zone", "tenant-a", Some(bind9_instances_from));

        let result = get_instances_from_zone(&zone, &single_shard(instance_store.clone()));
        assert!(
            result.is_ok(),
            "same-namespace match must always succeed: {result:?}"
        );
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_get_instances_match_labels_and_logic() {
        let mut instance_labels = std::collections::BTreeMap::new();
        instance_labels.insert("environment".to_string(), "production".to_string());
        let instance = create_test_instance_with_labels("dns-primary", "default", &instance_labels);

        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance));

        let mut match_labels = std::collections::BTreeMap::new();
        match_labels.insert("environment".to_string(), "production".to_string());
        match_labels.insert("role".to_string(), "primary".to_string());
        let bind9_instances_from = vec![InstanceSource {
            selector: crate::crd::LabelSelector {
                match_labels: Some(match_labels),
                match_expressions: None,
            },
        }];
        let zone =
            create_test_zone_with_selectors("test-zone", "default", Some(bind9_instances_from));

        let result = get_instances_from_zone(&zone, &single_shard(store.clone()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no instances matching"));
    }

    // ========================================================================
    // Endpoint helpers (moved from the zone controller's helpers tests)
    // ========================================================================

    // NOTE: refetch_zone() and handle_duplicate_zone() require a real or mocked Kubernetes client.
    // These functions make actual API calls, so they need integration tests or mock setup.
    // For now, we document what tests would be needed:
    //
    // test_refetch_zone_success:
    //   - Mock k8s API to return a zone
    //   - Verify the returned zone matches expected data
    //
    // test_refetch_zone_not_found:
    //   - Mock k8s API to return NotFound error
    //   - Verify error is propagated correctly
    //
    // test_handle_duplicate_zone_success:
    //   - Mock k8s API for status update
    //   - Create duplicate info with conflicting zones
    //   - Verify status updater sets DuplicateZone condition
    //   - Verify status is applied to API server
    //
    // test_handle_duplicate_zone_api_error:
    //   - Mock k8s API to return error on status update
    //   - Verify error is propagated
    //
    // These would require kube::Client mocking infrastructure, which is typically
    // done in integration tests with test fixtures or mock servers.

    // ========================================================================
    // Deletion-cleanup failure classification (is_unavailable_for_deletion)
    // ========================================================================

    fn kube_api_error(code: u16) -> anyhow::Error {
        anyhow::Error::from(kube::Error::Api(
            kube::core::Status::failure("test error", "TestReason")
                .with_code(code)
                .boxed(),
        ))
    }

    #[test]
    fn test_is_unavailable_for_deletion_kube_404_is_unavailable() {
        // A missing Secret/Endpoints object (404) means the target is gone:
        // safe to skip during deletion cleanup
        let err = kube_api_error(HTTP_STATUS_NOT_FOUND);
        assert!(is_unavailable_for_deletion(&err));
    }

    #[test]
    fn test_is_unavailable_for_deletion_kube_transient_errors_are_not() {
        // Timeouts / rate limits / server errors are potentially transient and
        // must be retried, never skipped
        const HTTP_TOO_MANY_REQUESTS: u16 = 429;
        const HTTP_INTERNAL_SERVER_ERROR: u16 = 500;
        const HTTP_SERVICE_UNAVAILABLE: u16 = 503;
        for code in [
            HTTP_TOO_MANY_REQUESTS,
            HTTP_INTERNAL_SERVER_ERROR,
            HTTP_SERVICE_UNAVAILABLE,
        ] {
            let err = kube_api_error(code);
            assert!(
                !is_unavailable_for_deletion(&err),
                "HTTP {code} must be treated as transient"
            );
        }
    }

    #[test]
    fn test_is_unavailable_for_deletion_context_wrapped_kube_404() {
        // load_rndc_key/get_endpoint wrap kube errors with .context(); the
        // classification must see through the anyhow context chain
        use anyhow::Context;
        let err: anyhow::Error = Err::<(), _>(kube::Error::Api(
            kube::core::Status::failure("secret not found", "NotFound")
                .with_code(HTTP_STATUS_NOT_FOUND)
                .boxed(),
        ))
        .context("Failed to get RNDC secret")
        .unwrap_err();
        assert!(is_unavailable_for_deletion(&err));
    }

    #[test]
    fn test_is_unavailable_for_deletion_non_kube_error_is_unavailable() {
        // "No ready endpoints found" style errors are not fixed by retrying a
        // deletion: skip with a warning
        let err = anyhow::anyhow!("No ready endpoints found for service foo with port 'http'");
        assert!(is_unavailable_for_deletion(&err));
    }

    // ========================================================================
    // Pod listing helpers (running_pod_name_and_ip)
    // ========================================================================

    fn make_pod(
        name: &str,
        phase: Option<&str>,
        ip: Option<&str>,
    ) -> k8s_openapi::api::core::v1::Pod {
        k8s_openapi::api::core::v1::Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            status: Some(k8s_openapi::api::core::v1::PodStatus {
                phase: phase.map(ToString::to_string),
                pod_ip: ip.map(ToString::to_string),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_running_pod_name_and_ip_running_with_ip() {
        let pod = make_pod("pod-1", Some("Running"), Some("10.0.0.1"));
        assert_eq!(
            running_pod_name_and_ip(&pod),
            Some(("pod-1".to_string(), "10.0.0.1".to_string()))
        );
    }

    #[test]
    fn test_running_pod_name_and_ip_pending_pod_is_skipped() {
        let pod = make_pod("pod-pending", Some("Pending"), None);
        assert_eq!(running_pod_name_and_ip(&pod), None);
    }

    #[test]
    fn test_running_pod_name_and_ip_running_without_ip_is_skipped() {
        // A pod can briefly report Running before its IP is populated in the
        // cache; it must be skipped, not fail the whole pod listing
        let pod = make_pod("pod-no-ip", Some("Running"), None);
        assert_eq!(running_pod_name_and_ip(&pod), None);
    }

    #[test]
    fn test_running_pod_name_and_ip_no_status() {
        let pod = k8s_openapi::api::core::v1::Pod {
            metadata: ObjectMeta {
                name: Some("pod-nostatus".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(running_pod_name_and_ip(&pod), None);
    }
}
