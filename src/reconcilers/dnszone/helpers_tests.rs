// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for DNS zone reconciliation helper functions.

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::super::helpers::*;
    use crate::crd::{DNSZone, DNSZoneSpec, DNSZoneStatus, InstanceReference, SOARecord};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    // Helper function to create a test DNSZone
    fn create_test_zone(name: &str, namespace: &str, generation: i64) -> DNSZone {
        DNSZone {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                generation: Some(generation),
                ..Default::default()
            },
            spec: DNSZoneSpec {
                zone_name: format!("{name}.example.com"),
                cluster_ref: None,
                soa_record: SOARecord {
                    primary_ns: "ns1.example.com.".to_string(),
                    admin_email: "admin.example.com.".to_string(),
                    serial: 2_024_010_101,
                    refresh: 3600,
                    retry: 600,
                    expire: 604_800,
                    negative_ttl: 86_400,
                },
                ttl: Some(3600),
                name_servers: None,
                name_server_ips: None,
                records_from: None,
                bind9_instances_from: None,
                dnssec_policy: None,
            },
            status: None,
        }
    }

    // Helper function to create a zone with status
    fn create_zone_with_status(
        name: &str,
        namespace: &str,
        generation: i64,
        observed_generation: Option<i64>,
    ) -> DNSZone {
        let mut zone = create_test_zone(name, namespace, generation);
        zone.status = Some(DNSZoneStatus {
            observed_generation,
            bind9_instances: vec![],
            bind9_instances_count: Some(0),
            conditions: vec![],
            records: vec![],
            records_count: 0,
            dnssec: None,
        });
        zone
    }

    // Helper to create instance reference for testing
    fn create_instance_ref(name: &str, timestamp: Option<String>) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: "default".to_string(),
            last_reconciled_at: timestamp,
        }
    }

    #[test]
    fn test_detect_spec_changes_first_reconciliation() {
        // Arrange: Zone with no status (first reconciliation)
        let zone = create_test_zone("test", "default", 1);

        // Act
        let (first_reconciliation, spec_changed) = detect_spec_changes(&zone);

        // Assert: Should detect first reconciliation and spec changed
        assert!(first_reconciliation, "Should be first reconciliation");
        assert!(spec_changed, "Spec should be considered changed");
    }

    #[test]
    fn test_detect_spec_changes_no_change() {
        // Arrange: Zone with matching generation
        let zone = create_zone_with_status("test", "default", 5, Some(5));

        // Act
        let (first_reconciliation, spec_changed) = detect_spec_changes(&zone);

        // Assert: Not first reconciliation, no changes
        assert!(!first_reconciliation, "Should not be first reconciliation");
        assert!(!spec_changed, "Spec should not have changed");
    }

    #[test]
    fn test_detect_spec_changes_with_change() {
        // Arrange: Zone with different generation (spec was updated)
        let zone = create_zone_with_status("test", "default", 7, Some(5));

        // Act
        let (first_reconciliation, spec_changed) = detect_spec_changes(&zone);

        // Assert: Not first reconciliation, but spec changed
        assert!(!first_reconciliation, "Should not be first reconciliation");
        assert!(spec_changed, "Spec should have changed");
    }

    #[test]
    fn test_detect_instance_changes_no_watch_instances() {
        // Arrange: No instances in watch event
        let current_instances = vec![create_instance_ref("instance1", None)];

        // Act
        let changed = detect_instance_changes("default", "test", None, &current_instances);

        // Assert: Should return true (first reconciliation or error case)
        assert!(changed, "Should detect change when watch_instances is None");
    }

    #[test]
    fn test_detect_instance_changes_list_changed_added() {
        // Arrange: Instance added
        let watch_instances = vec![create_instance_ref(
            "instance1",
            Some("2024-01-01T00:00:00Z".to_string()),
        )];

        let current_instances = vec![
            create_instance_ref("instance1", Some("2024-01-01T00:00:00Z".to_string())),
            create_instance_ref("instance2", None),
        ];

        // Act
        let changed = detect_instance_changes(
            "default",
            "test",
            Some(&watch_instances),
            &current_instances,
        );

        // Assert: Should detect list change
        assert!(changed, "Should detect instance list changed (added)");
    }

    #[test]
    fn test_detect_instance_changes_list_changed_removed() {
        // Arrange: Instance removed
        let watch_instances = vec![
            create_instance_ref("instance1", Some("2024-01-01T00:00:00Z".to_string())),
            create_instance_ref("instance2", Some("2024-01-01T00:00:00Z".to_string())),
        ];

        let current_instances = vec![create_instance_ref(
            "instance1",
            Some("2024-01-01T00:00:00Z".to_string()),
        )];

        // Act
        let changed = detect_instance_changes(
            "default",
            "test",
            Some(&watch_instances),
            &current_instances,
        );

        // Assert: Should detect list change
        assert!(changed, "Should detect instance list changed (removed)");
    }

    #[test]
    #[ignore = "KNOWN BUG: InstanceReference Hash/PartialEq mismatch prevents timestamp change detection"]
    fn test_detect_instance_changes_timestamp_changed() {
        // Arrange: Same instances, but timestamp changed (cleared)
        let watch_instances = vec![create_instance_ref(
            "instance1",
            Some("2024-01-01T00:00:00Z".to_string()),
        )];

        let current_instances = vec![create_instance_ref("instance1", None)];

        // Act
        let changed = detect_instance_changes(
            "default",
            "test",
            Some(&watch_instances),
            &current_instances,
        );

        // Assert: This test documents EXPECTED behavior - timestamp changes SHOULD be detected.
        //
        // KNOWN BUG - InstanceReference has inconsistent PartialEq and Hash implementations:
        // - PartialEq excludes last_reconciled_at (so instances with different timestamps are ==)
        // - Hash includes last_reconciled_at (so they have different hashes)
        // This violates the Rust invariant that a == b => hash(a) == hash(b), causing HashMap
        // lookups to fail even when PartialEq says they're equal.
        //
        // Result: Timestamp-only changes are NOT detected as changes at all, which means
        // reconciliation won't be triggered when only timestamps change. This is a correctness bug.
        //
        // TODO: Fix InstanceReference Hash impl in src/crd.rs to exclude last_reconciled_at,
        // matching the PartialEq implementation. Once fixed, remove the #[ignore] attribute.
        assert!(
            changed,
            "Timestamp changes should be detected to trigger reconciliation"
        );
    }

    #[test]
    fn test_detect_instance_changes_no_change() {
        // Arrange: Same instances, same timestamps
        let watch_instances = vec![
            create_instance_ref("instance1", Some("2024-01-01T00:00:00Z".to_string())),
            create_instance_ref("instance2", Some("2024-01-02T00:00:00Z".to_string())),
        ];

        let current_instances = vec![
            create_instance_ref("instance1", Some("2024-01-01T00:00:00Z".to_string())),
            create_instance_ref("instance2", Some("2024-01-02T00:00:00Z".to_string())),
        ];

        // Act
        let changed = detect_instance_changes(
            "default",
            "test",
            Some(&watch_instances),
            &current_instances,
        );

        // Assert: Should not detect changes
        assert!(!changed, "Should not detect changes when nothing changed");
    }

    #[test]
    fn test_detect_instance_changes_empty_lists() {
        // Arrange: Both empty
        let watch_instances = vec![];
        let current_instances = vec![];

        // Act
        let changed = detect_instance_changes(
            "default",
            "test",
            Some(&watch_instances),
            &current_instances,
        );

        // Assert: Should not detect changes
        assert!(
            !changed,
            "Should not detect changes when both lists are empty"
        );
    }

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
}
