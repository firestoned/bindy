// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `status_helpers.rs`

#[cfg(test)]
mod tests {
    use crate::crd::InstanceReference;

    /// Helper to create test instance references
    fn create_instance_ref(name: &str, namespace: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            last_reconciled_at: None,
        }
    }

    #[tokio::test]
    async fn test_calculate_expected_instance_counts() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: 5 instance references
        //        AND 2 are PRIMARY
        //        AND 3 are SECONDARY
        // When: calculate_expected_instance_counts is called
        // Then: Should return (2, 3)
    }

    #[tokio::test]
    async fn test_calculate_expected_instance_counts_no_secondaries() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: 3 instance references
        //        AND all 3 are PRIMARY
        //        AND 0 are SECONDARY
        // When: calculate_expected_instance_counts is called
        // Then: Should return (3, 0)
    }

    #[tokio::test]
    async fn test_finalize_zone_status_all_ready() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: No degraded conditions
        //        AND primary_count == expected_primary_count
        //        AND secondary_count == expected_secondary_count
        // When: finalize_zone_status is called
        // Then: Should set Ready=True condition
        //       AND clear any stale Degraded condition
        //       AND apply status to API
    }

    #[tokio::test]
    async fn test_finalize_zone_status_degraded_condition_exists() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: status_updater has a Degraded condition set
        //        AND primary_count == expected_primary_count
        // When: finalize_zone_status is called
        // Then: Should keep the Degraded condition
        //       AND NOT overwrite with Ready condition
        //       AND log "reconciliation completed with degraded state"
    }

    #[tokio::test]
    async fn test_finalize_zone_status_partial_reconciliation() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: No degraded conditions
        //        AND primary_count = 1, expected_primary_count = 2
        //        AND secondary_count = 2, expected_secondary_count = 3
        // When: finalize_zone_status is called
        // Then: Should set Degraded=True condition
        //       AND reason should be "PartialReconciliation"
        //       AND message should include "1 instance(s) pending"
        //       AND log "partially configured: 1/2 primaries, 2/3 secondaries"
    }

    #[tokio::test]
    async fn test_finalize_zone_status_partial_primary_only() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: No degraded conditions
        //        AND primary_count = 1, expected_primary_count = 3
        //        AND secondary_count = 0, expected_secondary_count = 0
        // When: finalize_zone_status is called
        // Then: Should set Degraded=True condition
        //       AND reason should be "PartialReconciliation"
        //       AND message should include "2 instance(s) pending"
    }

    #[tokio::test]
    async fn test_finalize_zone_status_sets_observed_generation() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: generation = Some(5)
        // When: finalize_zone_status is called
        // Then: Should call status_updater.set_observed_generation(Some(5))
        //       AND observedGeneration should be set in status
    }

    #[test]
    fn test_instance_reference_counts() {
        // Test that we can count instance references
        let refs = [
            create_instance_ref("instance-1", "default"),
            create_instance_ref("instance-2", "default"),
            create_instance_ref("instance-3", "other"),
        ];

        assert_eq!(refs.len(), 3);
    }

    // ========================================================================
    // set_final_zone_conditions - final condition triple (bugs: endpoint-vs-
    // instance unit mismatch, Ready never cleared, Progressing never resolved)
    // ========================================================================

    use crate::crd::{Condition, DNSZone, DNSZoneSpec, SOARecord};
    use crate::reconcilers::dnszone::status_helpers::set_final_zone_conditions;
    use crate::reconcilers::dnszone::types::ZoneConfigOutcome;
    use crate::reconcilers::status::DNSZoneStatusUpdater;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[allow(deprecated)]
    fn create_test_zone() -> DNSZone {
        DNSZone {
            metadata: ObjectMeta {
                name: Some("test-zone".to_string()),
                namespace: Some("default".to_string()),
                generation: Some(3),
                ..Default::default()
            },
            spec: DNSZoneSpec {
                zone_name: "example.com".to_string(),
                cluster_ref: None,
                soa_record: SOARecord {
                    primary_ns: "ns1.example.com.".to_string(),
                    admin_email: "admin.example.com.".to_string(),
                    serial: 2_024_010_101,
                    refresh: 3600,
                    retry: 600,
                    expire: 604_800,
                    negative_ttl: 86400,
                },
                ttl: None,
                name_servers: None,
                name_server_ips: None,
                records_from: None,
                bind9_instances_from: None,
                dnssec_policy: None,
            },
            status: None,
        }
    }

    fn find_condition<'a>(conditions: &'a [Condition], r#type: &str) -> &'a Condition {
        conditions
            .iter()
            .find(|c| c.r#type == r#type)
            .unwrap_or_else(|| panic!("condition {type} not found", type = r#type))
    }

    #[test]
    fn test_set_final_zone_conditions_success_converges_triple() {
        let zone = create_test_zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);
        // Simulate the Progressing=True set at the start of BIND9 configuration
        updater.set_condition("Progressing", "True", "PrimaryReconciling", "working");

        set_final_zone_conditions(
            &mut updater,
            "example.com",
            "default",
            "test-zone",
            ZoneConfigOutcome {
                instances_configured: 2,
                endpoints_configured: 4,
            },
            ZoneConfigOutcome {
                instances_configured: 1,
                endpoints_configured: 2,
            },
            2, // expected primaries
            1, // expected secondaries
            5, // records
            Some(3),
        );

        let conditions = updater.conditions();
        assert_eq!(find_condition(conditions, "Ready").status, "True");
        assert_eq!(find_condition(conditions, "Degraded").status, "False");
        // Progressing must be resolved on success - it previously stayed True forever
        assert_eq!(find_condition(conditions, "Progressing").status, "False");
    }

    #[test]
    fn test_set_final_zone_conditions_partial_instance_failure_not_masked_by_endpoints() {
        // THE BUG: instance A has 2 pod endpoints configured, instance B got
        // nothing. Endpoint count (2) >= expected instance count (2) previously
        // masked the failure and reported Ready=True. Comparing INSTANCE units
        // must report Degraded instead.
        let zone = create_test_zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);

        set_final_zone_conditions(
            &mut updater,
            "example.com",
            "default",
            "test-zone",
            ZoneConfigOutcome {
                instances_configured: 1, // only instance A fully configured
                endpoints_configured: 2, // ... but it has 2 endpoints
            },
            ZoneConfigOutcome::default(),
            2, // expected primaries: A and B
            0,
            0,
            Some(3),
        );

        let conditions = updater.conditions();
        let degraded = find_condition(conditions, "Degraded");
        assert_eq!(degraded.status, "True");
        assert_eq!(degraded.reason.as_deref(), Some("PartialReconciliation"));
        assert!(degraded.message.as_deref().unwrap().contains("1/2 primary"));
        // Ready must be False on partial configuration
        assert_eq!(find_condition(conditions, "Ready").status, "False");
        assert_eq!(find_condition(conditions, "Progressing").status, "False");
    }

    #[test]
    fn test_set_final_zone_conditions_degraded_clears_stale_ready() {
        // A zone that was Ready=True from a previous reconciliation must have
        // Ready set to False when this reconciliation set a Degraded condition
        let mut zone = create_test_zone();
        zone.status = Some(crate::crd::DNSZoneStatus {
            conditions: vec![Condition {
                r#type: "Ready".to_string(),
                status: "True".to_string(),
                reason: Some("ReconcileSucceeded".to_string()),
                message: Some("previously fine".to_string()),
                last_transition_time: None,
            }],
            ..Default::default()
        });
        let mut updater = DNSZoneStatusUpdater::new(&zone);
        updater.set_condition("Degraded", "True", "SecondaryFailed", "secondary broke");

        set_final_zone_conditions(
            &mut updater,
            "example.com",
            "default",
            "test-zone",
            ZoneConfigOutcome {
                instances_configured: 1,
                endpoints_configured: 1,
            },
            ZoneConfigOutcome::default(),
            1,
            0,
            0,
            Some(3),
        );

        let conditions = updater.conditions();
        assert_eq!(find_condition(conditions, "Degraded").status, "True");
        // The stale Ready=True must not survive a degraded reconciliation
        assert_eq!(find_condition(conditions, "Ready").status, "False");
        assert_eq!(find_condition(conditions, "Progressing").status, "False");
    }

    #[test]
    fn test_set_failure_conditions_sets_consistent_triple() {
        let zone = create_test_zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);
        updater.set_condition("Progressing", "True", "PrimaryReconciling", "working");

        crate::reconcilers::dnszone::bind9_config::set_failure_conditions(
            &mut updater,
            "PrimaryFailed",
            "could not configure primaries",
        );

        let conditions = updater.conditions();
        let ready = find_condition(conditions, "Ready");
        assert_eq!(ready.status, "False");
        assert_eq!(ready.reason.as_deref(), Some("PrimaryFailed"));
        assert_eq!(find_condition(conditions, "Degraded").status, "True");
        assert_eq!(find_condition(conditions, "Progressing").status, "False");
    }
}
