// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for dnszone reconciler

#[cfg(test)]
mod tests {
    use crate::crd::*;
    use std::collections::BTreeMap;

    // ========================================================================
    // T5: Zone-to-Instance Selection (New Architecture)
    // ========================================================================

    use super::super::dnszone::validation::get_instances_from_zone;

    /// Helper to create a `Bind9Instance` with specific labels
    fn create_test_instance(
        name: &str,
        namespace: &str,
        labels: &BTreeMap<String, String>,
    ) -> Bind9Instance {
        use serde_json::json;

        // Create a minimal Bind9Instance using JSON deserialization
        // This avoids having to specify every field in the spec
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
    fn create_test_zone(
        name: &str,
        namespace: &str,
        bind9_instances_from: Option<Vec<InstanceSource>>,
    ) -> DNSZone {
        use serde_json::json;

        // Create a minimal DNSZone using JSON deserialization
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

        // Add bind9_instances_from if provided
        if let Some(sources) = bind9_instances_from {
            zone_json["spec"]["bind9InstancesFrom"] =
                serde_json::to_value(sources).expect("Failed to serialize bind9_instances_from");
        }

        serde_json::from_value(zone_json).expect("Failed to create test zone")
    }

    /// T5.1: `get_instances_from_zone` returns error when no `bind9_instances_from` configured
    #[test]
    fn test_get_instances_no_selectors() {
        // Create zone without bind9_instances_from
        let zone = create_test_zone("test-zone", "default", None);

        // Create empty store
        let (store, _writer) = kube::runtime::reflector::store::<Bind9Instance>();

        // Should return error
        let result = get_instances_from_zone(&zone, &store);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no bind9_instances_from selectors"));
    }

    /// T5.2: `get_instances_from_zone` returns error when `bind9_instances_from` is empty
    #[test]
    fn test_get_instances_empty_selectors() {
        // Create zone with empty bind9_instances_from
        let zone = create_test_zone("test-zone", "default", Some(vec![]));

        // Create empty store
        let (store, _writer) = kube::runtime::reflector::store::<Bind9Instance>();

        // Should return error
        let result = get_instances_from_zone(&zone, &store);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no bind9_instances_from selectors"));
    }

    /// T5.3: `get_instances_from_zone` selects instance with matching labels
    #[test]
    fn test_get_instances_match_labels() {
        // Create instance with labels
        let mut instance_labels = BTreeMap::new();
        instance_labels.insert("environment".to_string(), "production".to_string());
        instance_labels.insert("role".to_string(), "primary".to_string());
        let instance = create_test_instance("dns-primary", "default", &instance_labels);

        // Create store and add instance
        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance.clone()));

        // Create zone with matching selector
        let mut match_labels = BTreeMap::new();
        match_labels.insert("environment".to_string(), "production".to_string());
        let bind9_instances_from = vec![InstanceSource {
            selector: LabelSelector {
                match_labels: Some(match_labels),
                match_expressions: None,
            },
        }];
        let zone = create_test_zone("test-zone", "default", Some(bind9_instances_from));

        // Should find the instance
        let result = get_instances_from_zone(&zone, &store);
        assert!(result.is_ok());
        let instances = result.unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].name, "dns-primary");
        assert_eq!(instances[0].namespace, "default");
        assert_eq!(instances[0].api_version, "bindy.firestoned.io/v1beta1");
        assert_eq!(instances[0].kind, "Bind9Instance");
    }

    /// T5.4: `get_instances_from_zone` returns empty when no instances match
    #[test]
    fn test_get_instances_no_match() {
        // Create instance with labels
        let mut instance_labels = BTreeMap::new();
        instance_labels.insert("environment".to_string(), "development".to_string());
        let instance = create_test_instance("dns-dev", "default", &instance_labels);

        // Create store and add instance
        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance));

        // Create zone with non-matching selector
        let mut match_labels = BTreeMap::new();
        match_labels.insert("environment".to_string(), "production".to_string());
        let bind9_instances_from = vec![InstanceSource {
            selector: LabelSelector {
                match_labels: Some(match_labels),
                match_expressions: None,
            },
        }];
        let zone = create_test_zone("test-zone", "default", Some(bind9_instances_from));

        // Should return error (no instances match)
        let result = get_instances_from_zone(&zone, &store);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no instances matching"));
    }

    /// T5.5: `get_instances_from_zone` supports OR logic across multiple selectors
    #[test]
    fn test_get_instances_or_logic() {
        // Create instances with different labels
        let mut prod_labels = BTreeMap::new();
        prod_labels.insert("environment".to_string(), "production".to_string());
        let prod_instance = create_test_instance("dns-prod", "default", &prod_labels);

        let mut staging_labels = BTreeMap::new();
        staging_labels.insert("environment".to_string(), "staging".to_string());
        let staging_instance = create_test_instance("dns-staging", "default", &staging_labels);

        let mut dev_labels = BTreeMap::new();
        dev_labels.insert("environment".to_string(), "development".to_string());
        let dev_instance = create_test_instance("dns-dev", "default", &dev_labels);

        // Create store and add all instances
        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(prod_instance));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(staging_instance));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(dev_instance));

        // Create zone with multiple selectors (OR logic)
        let mut prod_match = BTreeMap::new();
        prod_match.insert("environment".to_string(), "production".to_string());
        let mut staging_match = BTreeMap::new();
        staging_match.insert("environment".to_string(), "staging".to_string());

        let bind9_instances_from = vec![
            InstanceSource {
                selector: LabelSelector {
                    match_labels: Some(prod_match),
                    match_expressions: None,
                },
            },
            InstanceSource {
                selector: LabelSelector {
                    match_labels: Some(staging_match),
                    match_expressions: None,
                },
            },
        ];
        let zone = create_test_zone("test-zone", "default", Some(bind9_instances_from));

        // Should find both prod and staging instances (OR logic)
        let result = get_instances_from_zone(&zone, &store);
        assert!(result.is_ok());
        let instances = result.unwrap();
        assert_eq!(instances.len(), 2);
        let names: Vec<String> = instances.iter().map(|i| i.name.clone()).collect();
        assert!(names.contains(&"dns-prod".to_string()));
        assert!(names.contains(&"dns-staging".to_string()));
        assert!(!names.contains(&"dns-dev".to_string()));
    }

    /// T5.6: `get_instances_from_zone` selects instances across namespaces
    #[test]
    fn test_get_instances_cross_namespace() {
        // Create instances in different namespaces
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "bind9".to_string());

        let instance_a = create_test_instance("dns-1", "namespace-a", &labels);
        let instance_b = create_test_instance("dns-2", "namespace-b", &labels);

        // Create store and add instances
        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance_a));
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance_b));

        // Create zone with selector matching both
        let bind9_instances_from = vec![InstanceSource {
            selector: LabelSelector {
                match_labels: Some(labels),
                match_expressions: None,
            },
        }];
        let zone = create_test_zone("test-zone", "default", Some(bind9_instances_from));

        // Should find both instances
        let result = get_instances_from_zone(&zone, &store);
        assert!(result.is_ok());
        let found_instances = result.unwrap();
        assert_eq!(found_instances.len(), 2);

        // Verify namespaces
        let namespaces: Vec<String> = found_instances
            .iter()
            .map(|i| i.namespace.clone())
            .collect();
        assert!(namespaces.contains(&"namespace-a".to_string()));
        assert!(namespaces.contains(&"namespace-b".to_string()));
    }

    /// T5.7: `get_instances_from_zone` requires ALL `matchLabels` to match (AND logic)
    #[test]
    fn test_get_instances_match_labels_and_logic() {
        // Create instance with partial labels
        let mut instance_labels = BTreeMap::new();
        instance_labels.insert("environment".to_string(), "production".to_string());
        // Missing "role" label
        let instance = create_test_instance("dns-primary", "default", &instance_labels);

        // Create store and add instance
        let (store, mut writer) = kube::runtime::reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(instance));

        // Create zone requiring BOTH environment AND role (AND logic)
        let mut match_labels = BTreeMap::new();
        match_labels.insert("environment".to_string(), "production".to_string());
        match_labels.insert("role".to_string(), "primary".to_string());
        let bind9_instances_from = vec![InstanceSource {
            selector: LabelSelector {
                match_labels: Some(match_labels),
                match_expressions: None,
            },
        }];
        let zone = create_test_zone("test-zone", "default", Some(bind9_instances_from));

        // Should NOT match (instance missing "role" label)
        let result = get_instances_from_zone(&zone, &store);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no instances matching"));
    }

    // ========================================================================
    // T6: Duplicate Zone Detection
    // ========================================================================

    use super::super::dnszone::validation::check_for_duplicate_zones;

    /// Helper to create a zone with a specific zone name and status
    fn create_zone_with_status(
        name: &str,
        namespace: &str,
        zone_name: &str,
        bind9_instances: &[InstanceReferenceWithStatus],
    ) -> DNSZone {
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

        // Add status if instances provided
        if !bind9_instances.is_empty() {
            zone_json["status"] = json!({
                "bind9Instances": bind9_instances,
            });
        }

        serde_json::from_value(zone_json).expect("Failed to create test zone")
    }

    /// T6.1: `check_for_duplicate_zones` returns None when no duplicates exist
    #[test]
    fn test_check_duplicate_zones_no_duplicates() {
        // Create store with zones having different zone names
        let (store, mut writer) = kube::runtime::reflector::store::<DNSZone>();

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

        // Create current zone with unique zone name (third.com)
        let current_zone = create_zone_with_status("my-zone", "team-c", "third.com", &[]);

        // Should return None (no duplicates - different zone name)
        let result = check_for_duplicate_zones(&current_zone, &store);
        assert!(result.is_none());
    }

    /// T6.2: `check_for_duplicate_zones` detects duplicate zone in same namespace
    #[test]
    fn test_check_duplicate_zones_same_namespace() {
        let (store, mut writer) = kube::runtime::reflector::store::<DNSZone>();

        // Existing zone with example.com
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

        // New zone trying to claim example.com
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

        let new_zone: DNSZone =
            serde_json::from_value(new_zone_json).expect("Failed to create new zone");

        // Should detect duplicate
        let result = check_for_duplicate_zones(&new_zone, &store);
        assert!(result.is_some());

        let duplicate_info = result.unwrap();
        assert_eq!(duplicate_info.zone_name, "example.com");
        assert_eq!(duplicate_info.conflicting_zones.len(), 1);
        assert_eq!(duplicate_info.conflicting_zones[0].name, "existing-zone");
        assert_eq!(duplicate_info.conflicting_zones[0].namespace, "team-a");
    }

    /// T6.3: `check_for_duplicate_zones` detects duplicate zone in different namespace
    #[test]
    #[allow(clippy::similar_names)]
    fn test_check_duplicate_zones_different_namespace() {
        let (store, mut writer) = kube::runtime::reflector::store::<DNSZone>();

        // Team A claims example.com
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

        // Team B tries to claim example.com
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

        let team_b_zone: DNSZone =
            serde_json::from_value(team_b_zone_json).expect("Failed to create team B zone");

        // Should detect duplicate
        let result = check_for_duplicate_zones(&team_b_zone, &store);
        assert!(result.is_some());

        let duplicate_info = result.unwrap();
        assert_eq!(duplicate_info.zone_name, "example.com");
        assert_eq!(duplicate_info.conflicting_zones.len(), 1);
        assert_eq!(duplicate_info.conflicting_zones[0].name, "team-a-zone");
        assert_eq!(duplicate_info.conflicting_zones[0].namespace, "team-a");
    }

    /// T6.4: `check_for_duplicate_zones` allows same zone name if existing zone has no instances
    #[test]
    fn test_check_duplicate_zones_no_instances_no_conflict() {
        let (store, mut writer) = kube::runtime::reflector::store::<DNSZone>();

        // Existing zone with no instances (not configured anywhere)
        let unconfigured_zone =
            create_zone_with_status("unconfigured-zone", "team-a", "example.com", &[]);

        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(unconfigured_zone));

        // New zone trying to claim example.com
        let new_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "new-zone",
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

        let new_zone: DNSZone =
            serde_json::from_value(new_zone_json).expect("Failed to create new zone");

        // Should NOT detect duplicate (existing zone has no instances)
        let result = check_for_duplicate_zones(&new_zone, &store);
        assert!(result.is_none());
    }

    /// T6.5: `check_for_duplicate_zones` ignores zones with status=Failed
    #[test]
    fn test_check_duplicate_zones_ignores_failed() {
        let (store, mut writer) = kube::runtime::reflector::store::<DNSZone>();

        // Existing zone with Failed status
        let failed_zone = create_zone_with_status(
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

        writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(failed_zone));

        // New zone trying to claim example.com
        let new_zone_json = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {
                "name": "new-zone",
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

        let new_zone: DNSZone =
            serde_json::from_value(new_zone_json).expect("Failed to create new zone");

        // Should NOT detect duplicate (failed zones don't count)
        let result = check_for_duplicate_zones(&new_zone, &store);
        assert!(result.is_none());
    }

    /// T6.6: `check_for_duplicate_zones` allows updating the same zone
    #[test]
    fn test_check_duplicate_zones_same_zone_update() {
        let (store, mut writer) = kube::runtime::reflector::store::<DNSZone>();

        // Existing zone
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

        // Same zone being updated
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

        let updated_zone: DNSZone =
            serde_json::from_value(updated_zone_json).expect("Failed to create updated zone");

        // Should NOT detect duplicate (same zone being updated)
        let result = check_for_duplicate_zones(&updated_zone, &store);
        assert!(result.is_none());
    }

    /// T6.7: `check_for_duplicate_zones` detects multiple conflicting zones
    #[test]
    fn test_check_duplicate_zones_multiple_conflicts() {
        let (store, mut writer) = kube::runtime::reflector::store::<DNSZone>();

        // First zone claims example.com
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

        // Second zone also claims example.com (somehow slipped through)
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

        // Third zone tries to claim example.com
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

        let zone3: DNSZone = serde_json::from_value(zone3_json).expect("Failed to create zone3");

        // Should detect both conflicting zones
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
