// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `records/status_helpers.rs`.
//!
//! The status decision is made from the object the reconcile already holds
//! (the watch cache), with no GET, and the patch never carries the fields the
//! `DNSZone` controller owns (ADR-0016 decision 5).

#[cfg(test)]
mod tests {
    use super::super::{cached_record_status, record_status_patch, RecordStatusUpdate};
    use crate::crd::{Condition, RecordStatus, ZoneReference};

    const NOW: &str = "2026-10-06T12:00:00+00:00";
    const EARLIER: &str = "2026-10-01T08:00:00+00:00";
    const GENERATION: i64 = 3;

    fn ready_condition(status: &str, reason: &str, message: &str) -> Condition {
        Condition {
            r#type: "Ready".to_string(),
            status: status.to_string(),
            reason: Some(reason.to_string()),
            message: Some(message.to_string()),
            last_transition_time: Some(EARLIER.to_string()),
        }
    }

    fn zone_ref() -> ZoneReference {
        serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "name": "example-com",
            "namespace": "dns",
            "zoneName": "example.com"
        }))
        .expect("valid ZoneReference fixture")
    }

    fn status(observed: Option<i64>, conditions: Vec<Condition>) -> RecordStatus {
        RecordStatus {
            conditions,
            observed_generation: observed,
            #[allow(deprecated)]
            zone: Some("example.com".to_string()),
            zone_ref: Some(zone_ref()),
            record_hash: Some("old-hash".to_string()),
            last_updated: Some(EARLIER.to_string()),
            addresses: Some("192.0.2.1".to_string()),
            published_name: Some("www".to_string()),
        }
    }

    fn published(message: &str) -> RecordStatusUpdate<'_> {
        RecordStatusUpdate {
            status: "True",
            reason: "ReconcileSucceeded",
            message,
            observed_generation: Some(GENERATION),
            record_hash: Some("new-hash".to_string()),
            last_updated: Some(NOW.to_string()),
            addresses: Some("192.0.2.9".to_string()),
            published_name: Some("www".to_string()),
        }
    }

    #[test]
    fn the_patch_never_carries_zone_or_zone_ref() {
        let current = status(Some(GENERATION), vec![]);

        let patch = record_status_patch(
            Some(&current),
            Some(GENERATION),
            &published("A record added"),
            NOW,
        )
        .expect("a missing condition is written");

        let status = &patch["status"];
        assert!(status.get("zone").is_none(), "{patch}");
        assert!(status.get("zoneRef").is_none(), "{patch}");
    }

    #[test]
    fn the_patch_skips_when_the_cached_status_already_matches() {
        let current = status(
            Some(GENERATION),
            vec![ready_condition(
                "True",
                "ReconcileSucceeded",
                "A record added",
            )],
        );

        assert!(
            record_status_patch(
                Some(&current),
                Some(GENERATION),
                &published("A record added"),
                NOW
            )
            .is_none(),
            "an unchanged condition at the observed generation is not re-written"
        );
    }

    #[test]
    fn a_new_generation_is_written_even_with_the_same_condition() {
        let current = status(
            Some(GENERATION - 1),
            vec![ready_condition(
                "True",
                "ReconcileSucceeded",
                "A record added",
            )],
        );

        let patch = record_status_patch(
            Some(&current),
            Some(GENERATION),
            &published("A record added"),
            NOW,
        )
        .expect("a new generation is written");

        assert_eq!(patch["status"]["observedGeneration"], GENERATION);
    }

    #[test]
    fn a_changed_message_is_written() {
        let current = status(
            Some(GENERATION),
            vec![ready_condition("False", "ReconcileFailed", "Refused")],
        );
        let update =
            RecordStatusUpdate::not_ready("ReconcileFailed", "Timed out", Some(GENERATION));

        let patch = record_status_patch(Some(&current), Some(GENERATION), &update, NOW)
            .expect("a changed message is written");

        assert_eq!(patch["status"]["conditions"][0]["message"], "Timed out");
    }

    #[test]
    fn a_not_ready_patch_leaves_addresses_published_name_hash_and_timestamp_alone() {
        let current = status(Some(GENERATION), vec![]);
        let update = RecordStatusUpdate::not_ready(
            "NoPrimaryInstances",
            "DNSZone has no primary instances configured",
            Some(GENERATION),
        );

        let patch = record_status_patch(Some(&current), Some(GENERATION), &update, NOW)
            .expect("a missing condition is written");

        let status = &patch["status"];
        for preserved in ["addresses", "publishedName", "recordHash", "lastUpdated"] {
            assert!(
                status.get(preserved).is_none(),
                "{preserved} must be omitted so the merge patch keeps it: {patch}"
            );
        }
    }

    #[test]
    fn a_published_patch_sets_addresses_and_published_name() {
        let patch = record_status_patch(None, Some(GENERATION), &published("added"), NOW)
            .expect("a record without status is written");

        let status = &patch["status"];
        assert_eq!(status["addresses"], "192.0.2.9");
        assert_eq!(status["publishedName"], "www");
        assert_eq!(status["recordHash"], "new-hash");
        assert_eq!(status["lastUpdated"], NOW);
    }

    #[test]
    fn the_transition_time_is_kept_while_the_status_value_holds() {
        let current = status(
            Some(GENERATION),
            vec![ready_condition("False", "ZoneNotFound", "old")],
        );
        let update = RecordStatusUpdate::not_ready("NotSelected", "new", Some(GENERATION));

        let patch = record_status_patch(Some(&current), Some(GENERATION), &update, NOW)
            .expect("a changed reason is written");

        assert_eq!(
            patch["status"]["conditions"][0]["lastTransitionTime"],
            EARLIER
        );
    }

    #[test]
    fn the_transition_time_moves_when_the_status_value_flips() {
        let current = status(
            Some(GENERATION),
            vec![ready_condition("False", "ReconcileFailed", "Refused")],
        );

        let patch = record_status_patch(Some(&current), Some(GENERATION), &published("ok"), NOW)
            .expect("a flip to Ready is written");

        assert_eq!(patch["status"]["conditions"][0]["lastTransitionTime"], NOW);
    }

    #[test]
    fn the_cached_status_is_read_from_the_object_itself() {
        let record: crate::crd::ARecord = serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "ARecord",
            "metadata": {"name": "www", "namespace": "dns", "generation": GENERATION},
            "spec": {"name": "www", "ipv4Addresses": ["192.0.2.1"]},
            "status": {"observedGeneration": GENERATION, "conditions": [], "publishedName": "www"}
        }))
        .expect("valid ARecord fixture");

        let cached = cached_record_status(&record).expect("status present");

        assert_eq!(cached.observed_generation, Some(GENERATION));
        assert_eq!(cached.published_name.as_deref(), Some("www"));
    }
}
