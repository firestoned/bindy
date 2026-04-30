// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `validation.rs`
//!
//! These tests document expected behavior for validation logic.
//! Full implementation requires Kubernetes API mocking infrastructure.

#[cfg(test)]
mod tests {
    use crate::crd::InstanceReference;
    use crate::reconcilers::dnszone::validation::filter_instances_needing_reconciliation;

    fn create_instance_ref(name: &str, namespace: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            last_reconciled_at: None,
        }
    }

    #[test]
    fn test_filter_instances_needing_reconciliation_all_need_reconciliation() {
        let instances = vec![
            create_instance_ref("instance-1", "default"),
            create_instance_ref("instance-2", "default"),
            create_instance_ref("instance-3", "default"),
        ];

        let result = filter_instances_needing_reconciliation(&instances);

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_filter_instances_needing_reconciliation_some_already_reconciled() {
        let mut instances = vec![
            create_instance_ref("instance-1", "default"),
            create_instance_ref("instance-2", "default"),
            create_instance_ref("instance-3", "default"),
        ];

        // Set timestamp on instance-2 (already reconciled)
        instances[1].last_reconciled_at = Some("2025-01-01T00:00:00Z".to_string());

        let result = filter_instances_needing_reconciliation(&instances);

        // Should only return instance-1 and instance-3
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "instance-1");
        assert_eq!(result[1].name, "instance-3");
    }

    #[test]
    fn test_filter_instances_needing_reconciliation_none_need_reconciliation() {
        let mut instances = vec![
            create_instance_ref("instance-1", "default"),
            create_instance_ref("instance-2", "default"),
        ];

        // Set timestamp on all instances (all already reconciled)
        instances[0].last_reconciled_at = Some("2025-01-01T00:00:00Z".to_string());
        instances[1].last_reconciled_at = Some("2025-01-01T00:00:01Z".to_string());

        let result = filter_instances_needing_reconciliation(&instances);

        assert_eq!(result.len(), 0);
    }

    // ========================================================================
    // T5: Zone-to-Instance Selection Tests (migrated from dnszone_tests.rs)
    // ========================================================================

    use crate::crd::{Bind9Instance, InstanceSource};
    use crate::reconcilers::dnszone::validation::get_instances_from_zone;

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
        let result = get_instances_from_zone(&zone, &store);
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
        let result = get_instances_from_zone(&zone, &store);
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

        let result = get_instances_from_zone(&zone, &store);
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

        let result = get_instances_from_zone(&zone, &store);
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

        let result = get_instances_from_zone(&zone, &store);
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

        let result = get_instances_from_zone(&zone, &store);
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

        let result = get_instances_from_zone(&zone, &store);
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

        let result = get_instances_from_zone(&zone, &store);
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

        let result = get_instances_from_zone(&zone, &store);
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

        let result = get_instances_from_zone(&zone, &instance_store);
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

        let result = get_instances_from_zone(&zone, &store);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no instances matching"));
    }

    // ========================================================================
    // T6: Duplicate Zone Detection Tests (migrated from dnszone_tests.rs)
    // ========================================================================

    use crate::crd::{InstanceReferenceWithStatus, InstanceStatus};
    use crate::reconcilers::dnszone::validation::check_for_duplicate_zones;

    /// Helper to create a zone with a specific zone name and status
    fn create_zone_with_status(
        name: &str,
        namespace: &str,
        zone_name: &str,
        bind9_instances: &[InstanceReferenceWithStatus],
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
                "zoneName": zone_name,
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

        if !bind9_instances.is_empty() {
            zone_json["status"] = json!({
                "bind9Instances": bind9_instances,
            });
        }

        serde_json::from_value(zone_json).expect("Failed to create test zone")
    }

    #[test]
    fn test_check_duplicate_zones_no_duplicates() {
        let (store, mut writer) = kube::runtime::reflector::store::<crate::crd::DNSZone>();

        let zone1 = create_zone_with_status(
            "zone1",
            "team-a",
            "example.com",
            &[InstanceReferenceWithStatus {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: "dns-1".to_string(),
                namespace: "default".to_string(),
                status: InstanceStatus::Configured,
                last_reconciled_at: None,
                message: None,
            }],
        );

        let zone2 = create_zone_with_status(
            "zone2",
            "team-b",
            "different.com",
            &[InstanceReferenceWithStatus {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: "dns-1".to_string(),
                namespace: "default".to_string(),
                status: InstanceStatus::Configured,
                last_reconciled_at: None,
                message: None,
            }],
        );

        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone1));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone2));

        let current_zone = create_zone_with_status("my-zone", "team-c", "third.com", &[]);
        let result = check_for_duplicate_zones(&current_zone, &store);
        assert!(result.is_none());
    }

    #[test]
    fn test_check_duplicate_zones_same_namespace() {
        let (store, mut writer) = kube::runtime::reflector::store::<crate::crd::DNSZone>();

        let existing_zone = create_zone_with_status(
            "existing-zone",
            "team-a",
            "example.com",
            &[InstanceReferenceWithStatus {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: "dns-1".to_string(),
                namespace: "default".to_string(),
                status: InstanceStatus::Configured,
                last_reconciled_at: None,
                message: None,
            }],
        );

        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(existing_zone));

        let new_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "new-zone",
                "namespace": "team-a",
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

        let new_zone: crate::crd::DNSZone =
            serde_json::from_value(new_zone_json).expect("Failed to create new zone");

        let result = check_for_duplicate_zones(&new_zone, &store);
        assert!(result.is_some());

        let duplicate_info = result.unwrap();
        assert_eq!(duplicate_info.zone_name, "example.com");
        assert_eq!(duplicate_info.conflicting_zones.len(), 1);
        assert_eq!(duplicate_info.conflicting_zones[0].name, "existing-zone");
        assert_eq!(duplicate_info.conflicting_zones[0].namespace, "team-a");
    }

    #[test]
    #[allow(clippy::similar_names)]
    fn test_check_duplicate_zones_different_namespace() {
        let (store, mut writer) = kube::runtime::reflector::store::<crate::crd::DNSZone>();

        let team_a_zone = create_zone_with_status(
            "team-a-zone",
            "team-a",
            "example.com",
            &[InstanceReferenceWithStatus {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: "dns-1".to_string(),
                namespace: "default".to_string(),
                status: InstanceStatus::Configured,
                last_reconciled_at: None,
                message: None,
            }],
        );

        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(team_a_zone));

        let team_b_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "team-b-zone",
                "namespace": "team-b",
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

        let team_b_zone: crate::crd::DNSZone =
            serde_json::from_value(team_b_zone_json).expect("Failed to create team B zone");

        let result = check_for_duplicate_zones(&team_b_zone, &store);
        assert!(result.is_some());

        let duplicate_info = result.unwrap();
        assert_eq!(duplicate_info.zone_name, "example.com");
        assert_eq!(duplicate_info.conflicting_zones.len(), 1);
        assert_eq!(duplicate_info.conflicting_zones[0].name, "team-a-zone");
        assert_eq!(duplicate_info.conflicting_zones[0].namespace, "team-a");
    }

    /// F-003: a *new* zone with the same `zoneName` is now flagged as a
    /// duplicate even if the existing zone has no instances configured yet.
    /// The pre-F-003 behaviour gated on `status.bind9_instances` being
    /// non-empty, leaving every race window open: a tenant could create a
    /// malicious zone first and claim the zoneName uncontested before the
    /// legitimate zone reached `Configured` state. The new check uses
    /// `spec.zoneName` and creation timestamp; the older CR wins.
    #[test]
    fn test_check_duplicate_zones_unconfigured_existing_still_conflicts() {
        let (store, mut writer) = kube::runtime::reflector::store::<crate::crd::DNSZone>();

        // existing zone has no status.bind9_instances yet — but it's older.
        let mut unconfigured_zone =
            create_zone_with_status("unconfigured-zone", "team-a", "example.com", &[]);
        unconfigured_zone.metadata.creation_timestamp =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                "2026-01-01T00:00:00Z".parse().unwrap(),
            ));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(unconfigured_zone));

        let new_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "new-zone",
                "namespace": "team-b",
                "creationTimestamp": "2026-04-01T00:00:00Z",
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
        let new_zone: crate::crd::DNSZone =
            serde_json::from_value(new_zone_json).expect("Failed to create new zone");

        let result = check_for_duplicate_zones(&new_zone, &store);
        assert!(
            result.is_some(),
            "F-003: the newer zone must lose to the older zone with the same zoneName"
        );
        let info = result.unwrap();
        assert_eq!(info.zone_name, "example.com");
        assert_eq!(info.conflicting_zones[0].name, "unconfigured-zone");
    }

    /// F-003: a *newer* zone whose existing same-zoneName neighbour is in
    /// `Failed` state is still flagged as a duplicate. The pre-F-003
    /// behaviour ignored failed zones, allowing a tenant to win the race
    /// any time the legitimate zone happened to be transiently failed.
    /// The new check is pure spec/creationTimestamp; the older CR wins
    /// regardless of status.
    #[test]
    fn test_check_duplicate_zones_failed_existing_still_conflicts() {
        let (store, mut writer) = kube::runtime::reflector::store::<crate::crd::DNSZone>();

        let mut failed_zone = create_zone_with_status(
            "failed-zone",
            "team-a",
            "example.com",
            &[InstanceReferenceWithStatus {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: "dns-1".to_string(),
                namespace: "default".to_string(),
                status: InstanceStatus::Failed,
                last_reconciled_at: None,
                message: None,
            }],
        );
        failed_zone.metadata.creation_timestamp =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                "2026-01-01T00:00:00Z".parse().unwrap(),
            ));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(failed_zone));

        let new_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "new-zone",
                "namespace": "team-b",
                "creationTimestamp": "2026-04-01T00:00:00Z",
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
        let new_zone: crate::crd::DNSZone =
            serde_json::from_value(new_zone_json).expect("Failed to create new zone");

        let result = check_for_duplicate_zones(&new_zone, &store);
        assert!(
            result.is_some(),
            "F-003: failed-state of the older zone must not unblock the newer claimant"
        );
    }

    /// F-003: when timestamps clearly favour the *current* zone (it is the
    /// older claimant), `check_for_duplicate_zones` returns None. The
    /// loser is the newer zone and its own reconciler will flag itself
    /// when it runs.
    #[test]
    fn test_check_duplicate_zones_current_is_older_returns_none() {
        let (store, mut writer) = kube::runtime::reflector::store::<crate::crd::DNSZone>();

        // A *newer* same-zoneName zone exists in the store.
        let newer_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "newer-zone",
                "namespace": "team-b",
                "creationTimestamp": "2026-04-01T00:00:00Z",
            },
            "spec": {
                "zoneName": "example.com",
                "soaRecord": {
                    "primaryNs": "ns1.example.com.",
                    "adminEmail": "admin.example.com.",
                    "serial": 1, "refresh": 3600, "retry": 1800,
                    "expire": 604_800, "negativeTtl": 86400
                },
                "ttl": 3600,
                "nameServerIPs": ["192.168.1.1"]
            }
        });
        let newer: crate::crd::DNSZone = serde_json::from_value(newer_zone_json).unwrap();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(newer));

        // The current zone is older.
        let older_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "older-zone",
                "namespace": "team-a",
                "creationTimestamp": "2026-01-01T00:00:00Z",
            },
            "spec": {
                "zoneName": "example.com",
                "soaRecord": {
                    "primaryNs": "ns1.example.com.",
                    "adminEmail": "admin.example.com.",
                    "serial": 1, "refresh": 3600, "retry": 1800,
                    "expire": 604_800, "negativeTtl": 86400
                },
                "ttl": 3600,
                "nameServerIPs": ["192.168.1.1"]
            }
        });
        let older: crate::crd::DNSZone = serde_json::from_value(older_zone_json).unwrap();

        let result = check_for_duplicate_zones(&older, &store);
        assert!(
            result.is_none(),
            "current zone is older → it wins; result must be None"
        );
    }

    #[test]
    fn test_check_duplicate_zones_same_zone_update() {
        let (store, mut writer) = kube::runtime::reflector::store::<crate::crd::DNSZone>();

        let existing_zone = create_zone_with_status(
            "my-zone",
            "team-a",
            "example.com",
            &[InstanceReferenceWithStatus {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: "dns-1".to_string(),
                namespace: "default".to_string(),
                status: InstanceStatus::Configured,
                last_reconciled_at: None,
                message: None,
            }],
        );

        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(existing_zone));

        let updated_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "my-zone",
                "namespace": "team-a",
            },
            "spec": {
                "zoneName": "example.com",
                "soaRecord": {
                    "primaryNs": "ns1.example.com.",
                    "adminEmail": "admin.example.com.",
                    "serial": 2,
                    "refresh": 3600,
                    "retry": 1800,
                    "expire": 604_800,
                    "negativeTtl": 86400
                },
                "ttl": 3600,
                "nameServerIPs": ["192.168.1.1"]
            }
        });

        let updated_zone: crate::crd::DNSZone =
            serde_json::from_value(updated_zone_json).expect("Failed to create updated zone");

        let result = check_for_duplicate_zones(&updated_zone, &store);
        assert!(result.is_none());
    }

    #[test]
    fn test_check_duplicate_zones_multiple_conflicts() {
        let (store, mut writer) = kube::runtime::reflector::store::<crate::crd::DNSZone>();

        let zone1 = create_zone_with_status(
            "zone1",
            "team-a",
            "example.com",
            &[InstanceReferenceWithStatus {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: "dns-1".to_string(),
                namespace: "default".to_string(),
                status: InstanceStatus::Configured,
                last_reconciled_at: None,
                message: None,
            }],
        );

        let zone2 = create_zone_with_status(
            "zone2",
            "team-b",
            "example.com",
            &[InstanceReferenceWithStatus {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: "dns-2".to_string(),
                namespace: "default".to_string(),
                status: InstanceStatus::Configured,
                last_reconciled_at: None,
                message: None,
            }],
        );

        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone1));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone2));

        let zone3_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "zone3",
                "namespace": "team-c",
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

        let zone3: crate::crd::DNSZone =
            serde_json::from_value(zone3_json).expect("Failed to create zone3");

        let result = check_for_duplicate_zones(&zone3, &store);
        assert!(result.is_some());

        let duplicate_info = result.unwrap();
        assert_eq!(duplicate_info.zone_name, "example.com");
        assert_eq!(duplicate_info.conflicting_zones.len(), 2);

        let names: Vec<String> = duplicate_info
            .conflicting_zones
            .iter()
            .map(|z| z.name.clone())
            .collect();
        assert!(names.contains(&"zone1".to_string()));
        assert!(names.contains(&"zone2".to_string()));
    }
}
