// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the generic DNS record operator.

#[cfg(test)]
mod tests {
    use crate::record_operator::ReconcileError;
    use hickory_proto::rr::RecordType;

    #[test]
    fn test_reconcile_error_display() {
        // Arrange
        let error = ReconcileError::from(anyhow::anyhow!("Test error"));

        // Act
        let error_msg = format!("{error}");

        // Assert
        assert!(error_msg.contains("Test error"));
    }

    #[test]
    fn test_reconcile_error_from_anyhow() {
        // Arrange
        let anyhow_error = anyhow::anyhow!("API call failed");

        // Act
        let reconcile_error = ReconcileError::from(anyhow_error);

        // Assert
        let error_msg = format!("{reconcile_error}");
        assert!(error_msg.contains("API call failed"));
    }

    #[test]
    fn test_hickory_record_types() {
        // Verify that hickory RecordType enum works as expected
        assert_eq!(RecordType::A.to_string(), "A");
        assert_eq!(RecordType::AAAA.to_string(), "AAAA");
        assert_eq!(RecordType::TXT.to_string(), "TXT");
        assert_eq!(RecordType::CNAME.to_string(), "CNAME");
        assert_eq!(RecordType::MX.to_string(), "MX");
        assert_eq!(RecordType::NS.to_string(), "NS");
        assert_eq!(RecordType::SRV.to_string(), "SRV");
        assert_eq!(RecordType::CAA.to_string(), "CAA");
    }

    // NOTE: The following functions require integration testing with real/mocked Kubernetes API:
    //
    // DnsRecordType trait implementations:
    //   - Test KIND, FINALIZER, RECORD_TYPE_STR constants for all record types
    //   - Test hickory_record_type() returns correct RecordType for each type
    //   - Test reconcile_record() calls the appropriate reconcile function
    //   - Test metadata() and status() accessors
    //
    // error_policy():
    //   - Tests that error_policy returns correct requeue action
    //   - Tests that it logs the error
    //   - Tests the requeue duration matches ERROR_REQUEUE_DURATION_SECS
    //
    // reconcile_wrapper():
    //   - Tests finalizer addition on Apply event
    //   - Tests reconcile_record is called on Apply
    //   - Tests status is checked after reconciliation
    //   - Tests requeue action based on readiness
    //   - Tests finalizer removal on Cleanup event
    //   - Tests delete_record is called on Cleanup
    //   - Tests metrics are recorded correctly
    //   - Tests error handling for Apply/Cleanup failures
    //   - Tests error handling for finalizer errors
    //
    // run_generic_record_controller():
    //   - Tests controller creation and configuration
    //   - Tests watcher configuration (any_semantic)
    //   - Tests DNSZone watching and event mapping
    //   - Tests reconciliation triggering for records with lastReconciledAt == None
    //   - Tests controller runs the reconcile_wrapper
    //   - Tests controller applies error_policy on errors
    //
    // These require:
    //   - Mock Kubernetes API client
    //   - Mock context with reflector stores
    //   - Mock Bind9Manager
    //   - Integration test infrastructure
    //
    // These tests should be added to the integration test suite in /tests/ directory.
}

/// The record controller's primary-stream trigger (ADR-0015): a record wakes
/// its own reconciler when its zone assignment changes, not when the
/// reconciler writes its own conditions.
#[cfg(test)]
mod zone_ref_trigger_tests {
    use crate::crd::ARecord;
    use crate::record_operator::zone_ref_hash;
    use serde_json::json;

    fn a_record(status: serde_json::Value) -> ARecord {
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "ARecord",
            "metadata": {"name": "www", "namespace": "dns", "generation": 1},
            "spec": {"name": "www", "ipv4Addresses": ["192.0.2.1"]},
            "status": status,
        }))
        .expect("valid ARecord fixture")
    }

    fn zone_ref(name: &str) -> serde_json::Value {
        json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "name": name,
            "namespace": "dns",
            "zoneName": format!("{name}.example"),
        })
    }

    fn ready(message: &str) -> serde_json::Value {
        json!([{"type": "Ready", "status": "True", "reason": "ReconcileSucceeded", "message": message}])
    }

    #[test]
    fn own_condition_write_does_not_change_the_trigger() {
        let before = a_record(json!({"zoneRef": zone_ref("zone-a")}));
        let after = a_record(json!({
            "zoneRef": zone_ref("zone-a"),
            "conditions": ready("A record added"),
            "lastUpdated": "2026-10-05T00:00:00Z",
        }));

        assert_eq!(zone_ref_hash(&before), zone_ref_hash(&after));
    }

    #[test]
    fn being_tagged_by_a_zone_changes_the_trigger() {
        let untagged = a_record(json!({}));
        let tagged = a_record(json!({"zoneRef": zone_ref("zone-a")}));

        assert_ne!(zone_ref_hash(&untagged), zone_ref_hash(&tagged));
    }

    #[test]
    fn moving_to_another_zone_changes_the_trigger() {
        let a = a_record(json!({"zoneRef": zone_ref("zone-a")}));
        let b = a_record(json!({"zoneRef": zone_ref("zone-b")}));

        assert_ne!(zone_ref_hash(&a), zone_ref_hash(&b));
    }

    #[test]
    fn the_trigger_always_has_a_value() {
        // kube passes every event for a predicate returning None, which would
        // silently restore the self-trigger.
        assert!(zone_ref_hash(&a_record(json!({}))).is_some());
    }
}
