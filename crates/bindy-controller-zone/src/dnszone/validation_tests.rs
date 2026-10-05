// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `validation.rs`
//!
//! These tests document expected behavior for validation logic.
//! Full implementation requires Kubernetes API mocking infrastructure.

#[cfg(test)]
mod tests {
    use crate::crd::InstanceReference;
    use crate::dnszone::validation::filter_instances_needing_reconciliation;

    fn create_instance_ref(name: &str, namespace: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            last_reconciled_at: None,
        }
    }

    /// Wrap a plain reflector `Store` as a single-shard `MultiStore`.
    ///
    /// The production code takes a `MultiStore` so it can span several namespace
    /// watches; a unit test only ever needs one shard, and a single shard is exactly
    /// the cluster-wide shape.
    fn single_shard<K>(store: kube::runtime::reflector::Store<K>) -> crate::context::MultiStore<K>
    where
        K: kube::Resource + Clone + 'static,
        K::DynamicType: std::hash::Hash + Eq + Clone + std::fmt::Debug + Default,
    {
        crate::context::MultiStore::new(vec![store])
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

    // ========================================================================
    // T6: Duplicate Zone Detection Tests (migrated from dnszone_tests.rs)
    // ========================================================================

    use crate::crd::{InstanceReferenceWithStatus, InstanceStatus};
    use crate::dnszone::validation::check_for_duplicate_zones;

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
        let result = check_for_duplicate_zones(&current_zone, &single_shard(store.clone()));
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

        let result = check_for_duplicate_zones(&new_zone, &single_shard(store.clone()));
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

        let result = check_for_duplicate_zones(&team_b_zone, &single_shard(store.clone()));
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

        let result = check_for_duplicate_zones(&new_zone, &single_shard(store.clone()));
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

        let result = check_for_duplicate_zones(&new_zone, &single_shard(store.clone()));
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

        let result = check_for_duplicate_zones(&older, &single_shard(store.clone()));
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

        let result = check_for_duplicate_zones(&updated_zone, &single_shard(store.clone()));
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

        let result = check_for_duplicate_zones(&zone3, &single_shard(store.clone()));
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
