// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `transfer_peers.rs` (ADR-0019).

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::crd::{Condition, DNSZone, DNSZoneSpec, SOARecord};
    use crate::dnszone::status_helpers::set_final_zone_conditions;
    use crate::dnszone::types::ZoneConfigOutcome;
    use bindy_controller_sdk::status::DNSZoneStatusUpdater;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[allow(deprecated)]
    fn zone() -> DNSZone {
        DNSZone {
            metadata: ObjectMeta {
                name: Some("test-zone".to_string()),
                namespace: Some("default".to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: DNSZoneSpec {
                zone_name: "example.com".to_string(),
                cluster_ref: None,
                soa_record: SOARecord {
                    primary_ns: "ns1.example.com.".to_string(),
                    admin_email: "admin.example.com.".to_string(),
                    serial: 1,
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

    fn condition<'a>(conditions: &'a [Condition], kind: &str) -> &'a Condition {
        conditions
            .iter()
            .find(|c| c.r#type == kind)
            .unwrap_or_else(|| panic!("condition {kind} not found"))
    }

    fn finalize(updater: &mut DNSZoneStatusUpdater) {
        // Every instance counted as configured: only the peer/load state can
        // make the zone not Ready.
        set_final_zone_conditions(
            updater,
            "example.com",
            "default",
            "test-zone",
            ZoneConfigOutcome {
                instances_configured: 2,
                endpoints_configured: 2,
                ..Default::default()
            },
            ZoneConfigOutcome {
                instances_configured: 1,
                endpoints_configured: 1,
                ..Default::default()
            },
            2,
            1,
            0,
            Some(1),
        );
    }

    #[test]
    fn secondary_not_loaded_makes_the_zone_degraded_naming_the_instance() {
        // rc.7: the zone said Ready=True "configured on 2 primary and 1
        // secondary instance(s)" while the secondary served nothing.
        let zone = zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);

        mark_secondaries_not_loaded(
            &mut updater,
            "example.com",
            &["dns/secondary-0 (10.0.0.9:8080)".to_string()],
        );
        finalize(&mut updater);

        let conditions = updater.conditions();
        let degraded = condition(conditions, "Degraded");
        assert_eq!(degraded.status, "True");
        assert_eq!(
            degraded.reason.as_deref(),
            Some(REASON_SECONDARY_NOT_LOADED)
        );
        assert!(
            degraded
                .message
                .as_deref()
                .is_some_and(|m| m.contains("dns/secondary-0")),
            "the message must name the instance: {degraded:?}"
        );
        assert_eq!(condition(conditions, "Ready").status, "False");
    }

    #[test]
    fn nothing_not_loaded_sets_no_condition() {
        let zone = zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);

        mark_secondaries_not_loaded(&mut updater, "example.com", &[]);
        finalize(&mut updater);

        assert_eq!(condition(updater.conditions(), "Ready").status, "True");
    }

    #[test]
    fn failed_peer_refresh_makes_the_zone_degraded() {
        let zone = zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);

        mark_peer_refresh_failed(
            &mut updater,
            "example.com",
            &["primary 10.0.0.1:8080: HTTP 500".to_string()],
        );
        finalize(&mut updater);

        let degraded = condition(updater.conditions(), "Degraded");
        assert_eq!(degraded.status, "True");
        assert_eq!(degraded.reason.as_deref(), Some(REASON_PEERS_NOT_UPDATED));
        assert_eq!(condition(updater.conditions(), "Ready").status, "False");
    }

    #[test]
    fn no_transfer_source_makes_the_zone_degraded() {
        let zone = zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);

        mark_no_transfer_source(&mut updater, "example.com");
        finalize(&mut updater);

        let degraded = condition(updater.conditions(), "Degraded");
        assert_eq!(degraded.reason.as_deref(), Some(REASON_NO_TRANSFER_SOURCE));
    }

    #[test]
    fn peers_are_recorded_only_when_every_push_succeeded() {
        let all_good = PeerPushResult::default();
        assert!(all_good.complete());

        let primary_failed = PeerPushResult {
            primary_failures: vec!["x".to_string()],
            ..Default::default()
        };
        assert!(!primary_failed.complete());

        let secondary_failed = PeerPushResult {
            secondary_failures: 1,
            ..Default::default()
        };
        assert!(!secondary_failed.complete());

        let skipped = PeerPushResult {
            secondaries_skipped: true,
            ..Default::default()
        };
        assert!(
            !skipped.complete(),
            "secondaries left untouched for want of a transfer source were not pushed"
        );
    }

    #[test]
    fn a_pending_record_deletion_makes_the_zone_degraded() {
        // Chaos suite: a deleted record still served by a primary while the
        // zone said Ready=True.
        let zone = zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);

        crate::dnszone::status_helpers::mark_record_deletions_pending(
            &mut updater,
            "example.com",
            &["ARecord/churn-0".to_string()],
            false,
        );
        finalize(&mut updater);

        let degraded = condition(updater.conditions(), "Degraded");
        assert_eq!(degraded.status, "True");
        assert_eq!(
            degraded.reason.as_deref(),
            Some(crate::dnszone::status_helpers::REASON_RECORD_DELETION_PENDING)
        );
        assert!(degraded
            .message
            .as_deref()
            .is_some_and(|m| m.contains("ARecord/churn-0")));
        assert_eq!(condition(updater.conditions(), "Ready").status, "False");
    }

    #[test]
    fn an_unselected_record_cleanup_pending_makes_the_zone_degraded() {
        let zone = zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);

        crate::dnszone::status_helpers::mark_record_deletions_pending(
            &mut updater,
            "example.com",
            &[],
            true,
        );
        finalize(&mut updater);

        assert_eq!(condition(updater.conditions(), "Ready").status, "False");
    }

    #[test]
    fn no_pending_deletion_sets_no_condition() {
        let zone = zone();
        let mut updater = DNSZoneStatusUpdater::new(&zone);

        crate::dnszone::status_helpers::mark_record_deletions_pending(
            &mut updater,
            "example.com",
            &[],
            false,
        );
        finalize(&mut updater);

        assert_eq!(condition(updater.conditions(), "Ready").status, "True");
    }
}
