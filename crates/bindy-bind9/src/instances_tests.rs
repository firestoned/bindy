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

/// Per-reconcile resolution of instance RNDC keys and endpoints (ADR-0015).
///
/// These pin the API-call budget of the record and zone write paths: a
/// resolver answers every repeated lookup for an instance from memory, so the
/// number of Secret and Endpoints reads per reconcile is bounded by the number
/// of instances, not by records x instances.
#[cfg(test)]
mod resolver_tests {
    use crate::bind9::RndcKeyData;
    use crate::crd::{InstanceReference, RndcAlgorithm};
    use crate::instances::*;
    use k8s_openapi::api::core::v1::{EndpointAddress as K8sAddress, EndpointPort, EndpointSubset};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    const PORT_DNS_TCP: &str = "dns-tcp";
    const DNS_TCP_PORT: i32 = 53;
    const ENDPOINTS_PER_INSTANCE: usize = 2;

    /// Call counters shared between a test and its fake lookup.
    #[derive(Default)]
    struct Counters {
        key_lookups: AtomicUsize,
        endpoint_lookups: AtomicUsize,
        keys_forgotten: AtomicUsize,
    }

    /// A lookup that counts calls instead of talking to the API server.
    struct CountingLookup {
        counters: Arc<Counters>,
        fail_keys: bool,
        fail_endpoints: bool,
    }

    impl CountingLookup {
        fn new(counters: &Arc<Counters>) -> Self {
            Self {
                counters: Arc::clone(counters),
                fail_keys: false,
                fail_endpoints: false,
            }
        }
    }

    fn key_for(instance: &str) -> RndcKeyData {
        RndcKeyData {
            name: instance.to_string(),
            algorithm: RndcAlgorithm::HmacSha256,
            secret: format!("secret-of-{instance}"),
        }
    }

    impl InstanceLookup for CountingLookup {
        fn rndc_key<'a>(
            &'a self,
            _namespace: &'a str,
            instance_name: &'a str,
        ) -> LookupFuture<'a, RndcKeyData> {
            self.counters.key_lookups.fetch_add(1, Ordering::SeqCst);
            let fail = self.fail_keys;
            Box::pin(async move {
                if fail {
                    return Err(anyhow::anyhow!("secret unavailable"));
                }
                Ok(key_for(instance_name))
            })
        }

        fn endpoints<'a>(
            &'a self,
            _namespace: &'a str,
            service_name: &'a str,
            _port_name: &'a str,
        ) -> LookupFuture<'a, Vec<EndpointAddress>> {
            self.counters
                .endpoint_lookups
                .fetch_add(1, Ordering::SeqCst);
            let fail = self.fail_endpoints;
            Box::pin(async move {
                if fail {
                    return Err(anyhow::anyhow!("no ready endpoints for {service_name}"));
                }
                Ok((0..ENDPOINTS_PER_INSTANCE)
                    .map(|i| EndpointAddress {
                        ip: format!("10.0.0.{i}"),
                        port: DNS_TCP_PORT,
                    })
                    .collect())
            })
        }

        fn forget_rndc_key(&self, _namespace: &str, _instance_name: &str) {
            self.counters.keys_forgotten.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn instance(name: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: "dns".to_string(),
            last_reconciled_at: None,
        }
    }

    // ------------------------------------------------------------------
    // RndcKeyCache (process-wide, cross-reconcile)
    // ------------------------------------------------------------------

    #[test]
    fn rndc_key_cache_hits_within_ttl() {
        let cache = RndcKeyCache::default();
        let loaded_at = Instant::now();
        cache.insert_at("dns", "primary-0", key_for("primary-0"), loaded_at);

        let hit = cache.get_at("dns", "primary-0", loaded_at + Duration::from_secs(1));

        assert_eq!(
            hit.map(|k| k.secret),
            Some("secret-of-primary-0".to_string())
        );
    }

    #[test]
    fn rndc_key_cache_expires_after_ttl() {
        let cache = RndcKeyCache::default();
        let loaded_at = Instant::now();
        cache.insert_at("dns", "primary-0", key_for("primary-0"), loaded_at);

        let miss = cache.get_at("dns", "primary-0", loaded_at + RNDC_KEY_CACHE_TTL);

        assert!(miss.is_none(), "an entry as old as the TTL must be re-read");
    }

    #[test]
    fn rndc_key_cache_is_keyed_by_namespace_and_instance() {
        let cache = RndcKeyCache::default();
        let now = Instant::now();
        cache.insert_at("dns", "primary-0", key_for("primary-0"), now);

        assert!(cache.get_at("other", "primary-0", now).is_none());
        assert!(cache.get_at("dns", "primary-1", now).is_none());
    }

    #[test]
    fn rndc_key_cache_invalidate_forgets_entry() {
        let cache = RndcKeyCache::default();
        let now = Instant::now();
        cache.insert_at("dns", "primary-0", key_for("primary-0"), now);

        cache.invalidate("dns", "primary-0");

        assert!(cache.get_at("dns", "primary-0", now).is_none());
    }

    // ------------------------------------------------------------------
    // Endpoints parsing and the Endpoints store
    // ------------------------------------------------------------------

    fn endpoints_object(
        name: &str,
        namespace: &str,
        ips: &[&str],
    ) -> k8s_openapi::api::core::v1::Endpoints {
        k8s_openapi::api::core::v1::Endpoints {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            subsets: Some(vec![EndpointSubset {
                addresses: Some(
                    ips.iter()
                        .map(|ip| K8sAddress {
                            ip: (*ip).to_string(),
                            ..Default::default()
                        })
                        .collect(),
                ),
                ports: Some(vec![
                    EndpointPort {
                        name: Some(PORT_DNS_TCP.to_string()),
                        port: DNS_TCP_PORT,
                        ..Default::default()
                    },
                    EndpointPort {
                        name: Some("http".to_string()),
                        port: 8080,
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }]),
        }
    }

    #[test]
    fn ready_endpoint_addresses_uses_the_named_port() {
        let endpoints = endpoints_object("primary-0", "dns", &["10.1.0.1", "10.1.0.2"]);

        let addresses = ready_endpoint_addresses(&endpoints, PORT_DNS_TCP);

        let rendered: Vec<String> = addresses
            .iter()
            .map(|a| format!("{}:{}", a.ip, a.port))
            .collect();
        assert_eq!(rendered, vec!["10.1.0.1:53", "10.1.0.2:53"]);
    }

    #[test]
    fn ready_endpoint_addresses_is_empty_when_port_is_missing() {
        let endpoints = endpoints_object("primary-0", "dns", &["10.1.0.1"]);

        assert!(ready_endpoint_addresses(&endpoints, "rndc-api").is_empty());
    }

    fn endpoints_store(
        objects: Vec<k8s_openapi::api::core::v1::Endpoints>,
    ) -> crate::context::MultiStore<k8s_openapi::api::core::v1::Endpoints> {
        use kube::runtime::{reflector, watcher};
        let (store, mut writer) = reflector::store();
        writer.apply_watcher_event(&watcher::Event::Init);
        for object in objects {
            writer.apply_watcher_event(&watcher::Event::InitApply(object));
        }
        writer.apply_watcher_event(&watcher::Event::InitDone);
        crate::context::MultiStore::new(vec![store])
    }

    #[test]
    fn cached_endpoints_reads_the_store_without_an_api_call() {
        let store = endpoints_store(vec![endpoints_object("primary-0", "dns", &["10.1.0.1"])]);

        let cached = cached_endpoints(&store, "dns", "primary-0", PORT_DNS_TCP);

        let addresses = cached.expect("Endpoints object is cached");
        assert_eq!(addresses.len(), 1);
        assert_eq!(addresses[0].ip, "10.1.0.1");
    }

    #[test]
    fn cached_endpoints_is_none_when_the_object_is_not_cached() {
        let store = endpoints_store(vec![endpoints_object("primary-0", "dns", &["10.1.0.1"])]);

        assert!(cached_endpoints(&store, "other", "primary-0", PORT_DNS_TCP).is_none());
        assert!(cached_endpoints(&store, "dns", "primary-1", PORT_DNS_TCP).is_none());
    }

    #[test]
    fn cached_endpoints_reports_a_cached_object_with_no_ready_pods_as_empty() {
        let store = endpoints_store(vec![endpoints_object("primary-0", "dns", &[])]);

        let cached = cached_endpoints(&store, "dns", "primary-0", PORT_DNS_TCP);

        assert_eq!(cached.map(|a| a.len()), Some(0));
    }

    // ------------------------------------------------------------------
    // InstanceResolver (per-reconcile memo)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn resolver_loads_each_instance_key_once_per_reconcile() {
        let counters = Arc::new(Counters::default());
        let resolver = InstanceResolver::new(CountingLookup::new(&counters));
        const LOOKUPS_PER_INSTANCE: usize = 50;

        for _ in 0..LOOKUPS_PER_INSTANCE {
            for name in ["primary-0", "primary-1", "primary-2"] {
                let key = resolver.rndc_key("dns", name).await.expect("key resolves");
                assert_eq!(key.name, name);
            }
        }

        assert_eq!(counters.key_lookups.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn resolver_resolves_endpoints_once_per_instance_and_port() {
        let counters = Arc::new(Counters::default());
        let resolver = InstanceResolver::new(CountingLookup::new(&counters));

        for _ in 0..10 {
            resolver
                .endpoints("dns", "primary-0", PORT_DNS_TCP)
                .await
                .expect("endpoints resolve");
            resolver
                .endpoints("dns", "primary-0", "http")
                .await
                .expect("endpoints resolve");
        }

        assert_eq!(counters.endpoint_lookups.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn resolver_does_not_memoize_a_failed_lookup() {
        let counters = Arc::new(Counters::default());
        let mut lookup = CountingLookup::new(&counters);
        lookup.fail_keys = true;
        let resolver = InstanceResolver::new(lookup);

        assert!(resolver.rndc_key("dns", "primary-0").await.is_err());
        assert!(resolver.rndc_key("dns", "primary-0").await.is_err());

        assert_eq!(counters.key_lookups.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn resolver_forget_rndc_key_forces_a_reload() {
        let counters = Arc::new(Counters::default());
        let resolver = InstanceResolver::new(CountingLookup::new(&counters));

        resolver.rndc_key("dns", "primary-0").await.expect("key");
        resolver.forget_rndc_key("dns", "primary-0");
        resolver.rndc_key("dns", "primary-0").await.expect("key");

        assert_eq!(counters.key_lookups.load(Ordering::SeqCst), 2);
        assert_eq!(counters.keys_forgotten.load(Ordering::SeqCst), 1);
    }

    // ------------------------------------------------------------------
    // for_each_instance_endpoint: lookups per reconcile do not scale with R
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn for_each_instance_endpoint_lookups_do_not_scale_with_records() {
        const RECORDS: usize = 300;
        let counters = Arc::new(Counters::default());
        let resolver = InstanceResolver::new(CountingLookup::new(&counters));
        let instances = vec![
            instance("primary-0"),
            instance("primary-1"),
            instance("primary-2"),
        ];
        let operations = Arc::new(AtomicUsize::new(0));

        // One reconcile writing RECORDS records to every primary.
        for _ in 0..RECORDS {
            let operations = Arc::clone(&operations);
            for_each_instance_endpoint(
                &resolver,
                &instances,
                true,
                PORT_DNS_TCP,
                move |_endpoint, _instance, key| {
                    let operations = Arc::clone(&operations);
                    async move {
                        assert!(key.is_some(), "the RNDC key is passed to every operation");
                        operations.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                },
            )
            .await
            .expect("every endpoint accepts the write");
        }

        // Before ADR-0015 this was RECORDS x instances for each of the two.
        assert_eq!(counters.key_lookups.load(Ordering::SeqCst), instances.len());
        assert_eq!(
            counters.endpoint_lookups.load(Ordering::SeqCst),
            instances.len()
        );
        assert_eq!(
            operations.load(Ordering::SeqCst),
            RECORDS * instances.len() * ENDPOINTS_PER_INSTANCE
        );
    }

    #[tokio::test]
    async fn for_each_instance_endpoint_forgets_the_key_of_an_instance_whose_write_failed() {
        let counters = Arc::new(Counters::default());
        let resolver = InstanceResolver::new(CountingLookup::new(&counters));
        let instances = vec![instance("primary-0"), instance("primary-1")];

        // primary-0 rejects every write (as it would after a key rotation).
        let result = for_each_instance_endpoint(
            &resolver,
            &instances,
            true,
            PORT_DNS_TCP,
            |_endpoint, instance_name, _key| async move {
                if instance_name == "primary-0" {
                    return Err(anyhow::anyhow!("TSIG BADSIG"));
                }
                Ok(())
            },
        )
        .await;
        assert!(result.is_ok(), "primary-1 still succeeded");

        assert_eq!(counters.keys_forgotten.load(Ordering::SeqCst), 1);

        // The next write re-reads primary-0's key; primary-1 stays memoized.
        for_each_instance_endpoint(&resolver, &instances, true, PORT_DNS_TCP, |_, _, _| async {
            Ok(())
        })
        .await
        .expect("writes succeed");
        assert_eq!(counters.key_lookups.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn for_each_instance_endpoint_without_key_never_reads_a_secret() {
        let counters = Arc::new(Counters::default());
        let resolver = InstanceResolver::new(CountingLookup::new(&counters));
        let instances = vec![instance("primary-0")];

        for_each_instance_endpoint(
            &resolver,
            &instances,
            false,
            PORT_DNS_TCP,
            |_, _, key| async move {
                assert!(key.is_none());
                Ok(())
            },
        )
        .await
        .expect("writes succeed");

        assert_eq!(counters.key_lookups.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn skip_unavailable_policy_skips_an_instance_without_endpoints() {
        let counters = Arc::new(Counters::default());
        let mut lookup = CountingLookup::new(&counters);
        lookup.fail_endpoints = true;
        let resolver = InstanceResolver::new(lookup);
        let instances = vec![instance("primary-0")];

        let (first, total) = for_each_instance_endpoint_with_policy(
            &resolver,
            &instances,
            true,
            PORT_DNS_TCP,
            EndpointFailurePolicy::SkipUnavailable,
            |_, _, _| async { Err(anyhow::anyhow!("no endpoint may be addressed")) },
        )
        .await
        .expect("an unavailable instance is skipped during deletion cleanup");

        assert!(first.is_none());
        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn strict_policy_propagates_an_endpoint_lookup_failure() {
        let counters = Arc::new(Counters::default());
        let mut lookup = CountingLookup::new(&counters);
        lookup.fail_endpoints = true;
        let resolver = InstanceResolver::new(lookup);
        let instances = vec![instance("primary-0")];

        let result = for_each_instance_endpoint(
            &resolver,
            &instances,
            true,
            PORT_DNS_TCP,
            |_, _, _| async { Ok(()) },
        )
        .await;

        assert!(result.is_err());
    }
}

/// Reaching pods the zones-loaded readiness gate still holds out of their
/// Service (ADR-0017).
///
/// A gated pod is listed under `notReadyAddresses`. The operator must still
/// write zones and records to it once its containers are ready, or the gate
/// never opens; it must not write to a pod whose containers are not ready.
#[cfg(test)]
mod gated_pod_tests {
    use crate::bind9::RndcKeyData;
    use crate::crd::RndcAlgorithm;
    use crate::instances::*;
    use k8s_openapi::api::core::v1::{
        EndpointAddress as K8sAddress, EndpointPort, EndpointSubset, Endpoints, ObjectReference,
        Pod, PodCondition, PodStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
    use std::sync::Arc;

    const PORT_HTTP: &str = "http";
    const HTTP_PORT: i32 = 8080;
    const NS: &str = "dns";
    const READY_IP: &str = "10.1.0.1";
    const GATED_IP: &str = "10.1.0.2";
    const STARTING_IP: &str = "10.1.0.3";

    fn pod(name: &str, ip: Option<&str>, containers_ready: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(NS.to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                pod_ip: ip.map(ToString::to_string),
                conditions: Some(vec![PodCondition {
                    type_: "ContainersReady".to_string(),
                    status: containers_ready.to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn address(ip: &str, pod_name: Option<&str>) -> K8sAddress {
        K8sAddress {
            ip: ip.to_string(),
            target_ref: pod_name.map(|name| ObjectReference {
                kind: Some("Pod".to_string()),
                name: Some(name.to_string()),
                namespace: Some(NS.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// `primary-0` with one Ready pod and two not-ready pods: one gated with
    /// its containers ready, one still starting.
    fn endpoints() -> Endpoints {
        Endpoints {
            metadata: ObjectMeta {
                name: Some("primary-0".to_string()),
                namespace: Some(NS.to_string()),
                ..Default::default()
            },
            subsets: Some(vec![EndpointSubset {
                addresses: Some(vec![address(READY_IP, Some("ready"))]),
                not_ready_addresses: Some(vec![
                    address(GATED_IP, Some("gated")),
                    address(STARTING_IP, Some("starting")),
                ]),
                ports: Some(vec![EndpointPort {
                    name: Some(PORT_HTTP.to_string()),
                    port: HTTP_PORT,
                    ..Default::default()
                }]),
            }]),
        }
    }

    fn pods() -> Vec<Arc<Pod>> {
        vec![
            Arc::new(pod("ready", Some(READY_IP), "True")),
            Arc::new(pod("gated", Some(GATED_IP), "True")),
            Arc::new(pod("starting", Some(STARTING_IP), "False")),
        ]
    }

    fn ips(addresses: &[EndpointAddress]) -> Vec<String> {
        addresses.iter().map(|a| a.ip.clone()).collect()
    }

    fn store_of<K>(objects: Vec<K>) -> crate::context::MultiStore<K>
    where
        K: kube::Resource<DynamicType = ()> + Clone + 'static,
    {
        use kube::runtime::{reflector, watcher};
        let (store, mut writer) = reflector::store();
        writer.apply_watcher_event(&watcher::Event::Init);
        for object in objects {
            writer.apply_watcher_event(&watcher::Event::InitApply(object));
        }
        writer.apply_watcher_event(&watcher::Event::InitDone);
        crate::context::MultiStore::new(vec![store])
    }

    // ------------------------------------------------------------------
    // pod_containers_ready
    // ------------------------------------------------------------------

    #[test]
    fn pod_with_ready_containers_and_an_ip_can_take_writes() {
        assert!(pod_containers_ready(&pod("p", Some(GATED_IP), "True")));
    }

    #[test]
    fn pod_whose_containers_are_not_ready_cannot_take_writes() {
        assert!(!pod_containers_ready(&pod("p", Some(GATED_IP), "False")));
    }

    #[test]
    fn pod_without_an_ip_cannot_take_writes() {
        assert!(!pod_containers_ready(&pod("p", None, "True")));
    }

    #[test]
    fn terminating_pod_cannot_take_writes() {
        let mut terminating = pod("p", Some(GATED_IP), "True");
        terminating.metadata.deletion_timestamp = Some(Time(k8s_openapi::jiff::Timestamp::now()));
        assert!(!pod_containers_ready(&terminating));
    }

    #[test]
    fn pod_without_conditions_cannot_take_writes() {
        let mut bare = pod("p", Some(GATED_IP), "True");
        if let Some(status) = bare.status.as_mut() {
            status.conditions = None;
        }
        assert!(!pod_containers_ready(&bare));
    }

    // ------------------------------------------------------------------
    // writable_endpoint_addresses
    // ------------------------------------------------------------------

    #[test]
    fn writable_addresses_include_ready_and_gated_container_ready_pods() {
        let addresses = writable_endpoint_addresses(&endpoints(), PORT_HTTP, &pods());

        assert_eq!(ips(&addresses), vec![READY_IP, GATED_IP]);
        assert!(addresses.iter().all(|a| a.port == HTTP_PORT));
    }

    #[test]
    fn writable_addresses_skip_a_not_ready_address_whose_pod_is_unknown() {
        // No pod in the cache for the not-ready addresses: only Ready ones.
        let only_ready = vec![Arc::new(pod("ready", Some(READY_IP), "True"))];
        let addresses = writable_endpoint_addresses(&endpoints(), PORT_HTTP, &only_ready);

        assert_eq!(ips(&addresses), vec![READY_IP]);
    }

    #[test]
    fn writable_addresses_skip_a_not_ready_address_without_a_target_ref() {
        let mut eps = endpoints();
        if let Some(subset) = eps.subsets.as_mut().and_then(|s| s.first_mut()) {
            subset.not_ready_addresses = Some(vec![address(GATED_IP, None)]);
        }
        let addresses = writable_endpoint_addresses(&eps, PORT_HTTP, &pods());

        assert_eq!(ips(&addresses), vec![READY_IP]);
    }

    #[test]
    fn writable_addresses_are_empty_for_a_missing_port() {
        assert!(writable_endpoint_addresses(&endpoints(), "rndc-api", &pods()).is_empty());
    }

    #[test]
    fn cached_writable_endpoints_reads_both_stores() {
        let endpoints_store = store_of(vec![endpoints()]);
        let pod_store = store_of(pods().into_iter().map(|p| (*p).clone()).collect());

        let cached = cached_writable_endpoints(
            &endpoints_store,
            Some(&pod_store),
            NS,
            "primary-0",
            PORT_HTTP,
        )
        .expect("Endpoints object is cached");
        assert_eq!(ips(&cached), vec![READY_IP, GATED_IP]);

        let without_pods =
            cached_writable_endpoints(&endpoints_store, None, NS, "primary-0", PORT_HTTP)
                .expect("Endpoints object is cached");
        assert_eq!(
            ips(&without_pods),
            vec![READY_IP],
            "without the Pod store only Ready addresses are writable"
        );

        assert!(cached_writable_endpoints(
            &endpoints_store,
            Some(&pod_store),
            NS,
            "other",
            PORT_HTTP
        )
        .is_none());
    }

    // ------------------------------------------------------------------
    // SinglePodLookup
    // ------------------------------------------------------------------

    /// Every instance has the same two endpoints.
    struct FixedLookup;

    impl InstanceLookup for FixedLookup {
        fn rndc_key<'a>(
            &'a self,
            _namespace: &'a str,
            instance_name: &'a str,
        ) -> LookupFuture<'a, RndcKeyData> {
            Box::pin(async move {
                Ok(RndcKeyData {
                    name: instance_name.to_string(),
                    algorithm: RndcAlgorithm::HmacSha256,
                    secret: "s".to_string(),
                })
            })
        }

        fn endpoints<'a>(
            &'a self,
            _namespace: &'a str,
            _service_name: &'a str,
            _port_name: &'a str,
        ) -> LookupFuture<'a, Vec<EndpointAddress>> {
            Box::pin(async move {
                Ok(vec![
                    EndpointAddress {
                        ip: READY_IP.to_string(),
                        port: HTTP_PORT,
                    },
                    EndpointAddress {
                        ip: GATED_IP.to_string(),
                        port: HTTP_PORT,
                    },
                ])
            })
        }
    }

    #[tokio::test]
    async fn single_pod_lookup_returns_only_the_target_pod_of_its_instance() {
        let lookup = SinglePodLookup::new(FixedLookup, NS, "primary-0", GATED_IP);

        let addresses = lookup
            .endpoints(NS, "primary-0", PORT_HTTP)
            .await
            .expect("the pod is an endpoint");

        assert_eq!(ips(&addresses), vec![GATED_IP]);
    }

    #[tokio::test]
    async fn single_pod_lookup_returns_nothing_for_another_instance() {
        let lookup = SinglePodLookup::new(FixedLookup, NS, "primary-0", GATED_IP);

        let other = lookup
            .endpoints(NS, "primary-1", PORT_HTTP)
            .await
            .expect("another instance is not an error");
        assert!(other.is_empty());

        let other_ns = lookup
            .endpoints("elsewhere", "primary-0", PORT_HTTP)
            .await
            .expect("an instance in another namespace is not an error");
        assert!(other_ns.is_empty());
    }

    #[tokio::test]
    async fn single_pod_lookup_fails_when_the_pod_is_not_an_endpoint() {
        let lookup = SinglePodLookup::new(FixedLookup, NS, "primary-0", STARTING_IP);

        let result = lookup.endpoints(NS, "primary-0", PORT_HTTP).await;

        assert!(
            result.is_err(),
            "a pod that cannot take writes must not count as loaded"
        );
    }

    #[tokio::test]
    async fn single_pod_lookup_delegates_rndc_keys() {
        let lookup = SinglePodLookup::new(FixedLookup, NS, "primary-0", GATED_IP);

        let key = lookup.rndc_key(NS, "primary-1").await.expect("key");

        assert_eq!(key.name, "primary-1");
    }
}
