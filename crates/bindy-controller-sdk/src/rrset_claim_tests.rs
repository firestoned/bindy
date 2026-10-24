// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `rrset_claim.rs`: an RRset another live record still
//! declares survives a deletion (bug-211, ADR-0022 Decision 5).

#[cfg(test)]
mod tests {
    use super::super::{claim_of, claims_rrset, live_claimant, RrsetClaim};
    use bindy_api::crd::{ARecord, TXTRecord, ZoneReference};
    use serde_json::json;
    use std::sync::Arc;

    const ZONE_NS: &str = "dns";
    const ZONE: &str = "example-com";

    /// A claim in the default zone, by an object in namespace `team`.
    fn claim<'a>(kind: &'a str, name: &'a str, dns_name: &'a str) -> RrsetClaim<'a> {
        RrsetClaim {
            kind,
            namespace: "team",
            name,
            zone_namespace: ZONE_NS,
            zone_name: ZONE,
            dns_name,
            deleting: false,
        }
    }

    #[test]
    fn another_record_with_the_same_name_and_kind_claims_the_rrset() {
        let gone = claim("ARecord", "old", "www");
        let other = claim("ARecord", "new", "www");

        assert!(claims_rrset(&other, &gone));
    }

    #[test]
    fn a_record_with_another_name_does_not_claim_it() {
        let gone = claim("ARecord", "old", "www");
        let other = claim("ARecord", "api", "api");

        assert!(!claims_rrset(&other, &gone));
    }

    #[test]
    fn a_record_never_claims_its_own_rrset() {
        let gone = claim("ARecord", "web", "www");
        let same_object = claim("ARecord", "web", "www");

        assert!(!claims_rrset(&same_object, &gone));
    }

    #[test]
    fn the_same_name_in_another_namespace_is_another_object() {
        let gone = claim("ARecord", "web", "www");
        let other = RrsetClaim {
            namespace: "other-team",
            ..claim("ARecord", "web", "www")
        };

        assert!(claims_rrset(&other, &gone));
    }

    #[test]
    fn another_kind_is_another_rrset() {
        let gone = claim("ARecord", "old", "www");
        let other = claim("TXTRecord", "new", "www");

        assert!(!claims_rrset(&other, &gone));
    }

    #[test]
    fn a_record_of_another_zone_does_not_claim_it() {
        let gone = claim("ARecord", "old", "www");
        let other_zone = RrsetClaim {
            zone_name: "example-org",
            ..claim("ARecord", "new", "www")
        };
        let other_zone_namespace = RrsetClaim {
            zone_namespace: "elsewhere",
            ..claim("ARecord", "new", "www")
        };

        assert!(!claims_rrset(&other_zone, &gone));
        assert!(!claims_rrset(&other_zone_namespace, &gone));
    }

    #[test]
    fn a_record_being_deleted_does_not_claim_it() {
        let gone = claim("ARecord", "old", "www");
        let deleting = RrsetClaim {
            deleting: true,
            ..claim("ARecord", "new", "www")
        };

        assert!(!claims_rrset(&deleting, &gone));
    }

    #[test]
    fn dns_names_compare_case_insensitively() {
        let gone = claim("ARecord", "old", "WWW");
        let other = claim("ARecord", "new", "www");

        assert!(claims_rrset(&other, &gone));
    }

    // ========================================================================
    // claim_of / live_claimant: the claims typed records make
    // ========================================================================

    fn zone_ref(name: &str) -> ZoneReference {
        ZoneReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "DNSZone".to_string(),
            name: name.to_string(),
            namespace: ZONE_NS.to_string(),
            zone_name: format!("{name}.example"),
            last_reconciled_at: None,
        }
    }

    /// An `ARecord` `team/<name>` declaring `dns_name`, tagged with `zone`
    /// when given, and being deleted when `deleting`.
    fn a_record(name: &str, dns_name: &str, zone: Option<&str>, deleting: bool) -> Arc<ARecord> {
        let mut value = json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "ARecord",
            "metadata": {"name": name, "namespace": "team"},
            "spec": {"name": dns_name, "ipv4Addresses": ["192.0.2.1"]},
        });
        if let Some(zone) = zone {
            value["status"] = json!({"zoneRef": zone_ref(zone)});
        }
        if deleting {
            value["metadata"]["deletionTimestamp"] = json!("2026-10-13T00:00:00Z");
        }
        Arc::new(serde_json::from_value(value).expect("valid fixture"))
    }

    #[test]
    fn claim_of_reads_kind_object_zone_and_deletion() {
        let record = a_record("web", "www", Some(ZONE), true);
        let zone = zone_ref(ZONE);

        let got = claim_of(record.as_ref(), &zone, "www");

        assert_eq!(
            got,
            RrsetClaim {
                kind: "ARecord",
                namespace: "team",
                name: "web",
                zone_namespace: ZONE_NS,
                zone_name: ZONE,
                dns_name: "www",
                deleting: true,
            }
        );
    }

    #[test]
    fn a_live_record_of_the_zone_with_the_same_name_is_the_claimant() {
        let gone_record = a_record("old", "www", Some(ZONE), true);
        let zone = zone_ref(ZONE);
        let gone = claim_of(gone_record.as_ref(), &zone, "www");
        let live = vec![
            a_record("api", "api", Some(ZONE), false),
            a_record("new", "WWW", Some(ZONE), false),
        ];

        let claimant = live_claimant(&gone, &live).expect("www is still declared");

        assert_eq!(claimant.metadata.name.as_deref(), Some("new"));
    }

    #[test]
    fn the_record_itself_untagged_deleting_or_other_zone_records_claim_nothing() {
        let gone_record = a_record("old", "www", Some(ZONE), true);
        let zone = zone_ref(ZONE);
        let gone = claim_of(gone_record.as_ref(), &zone, "www");
        let live = vec![
            // The record being deleted, as the store still holds it.
            Arc::clone(&gone_record),
            // Not tagged by any zone yet.
            a_record("untagged", "www", None, false),
            // Being deleted too.
            a_record("leaving", "www", Some(ZONE), true),
            // Another zone.
            a_record("elsewhere", "www", Some("example-org"), false),
        ];

        assert!(live_claimant(&gone, &live).is_none());
    }

    #[test]
    fn a_record_of_another_kind_claims_nothing() {
        let zone = zone_ref(ZONE);
        let txt: TXTRecord = serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "TXTRecord",
            "metadata": {"name": "new", "namespace": "team"},
            "spec": {"name": "www", "text": ["v=spf1 -all"]},
            "status": {"zoneRef": zone_ref(ZONE)},
        }))
        .expect("valid fixture");
        let gone = claim("ARecord", "old", "www");

        assert!(!claims_rrset(&claim_of(&txt, &zone, "www"), &gone));
    }

    /// A claimant that has published `dns_name`, created at `created`.
    fn published(name: &str, dns_name: &str, created: &str) -> Arc<ARecord> {
        let mut record = (*a_record(name, dns_name, Some(ZONE), false)).clone();
        record.metadata.creation_timestamp =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                created.parse().expect("timestamp"),
            ));
        let status = record.status.as_mut().expect("status");
        status.published_name = Some(dns_name.to_string());
        status.last_updated = Some("2026-10-23T10:00:00Z".to_string());
        Arc::new(record)
    }

    #[test]
    fn the_claimant_re_pushed_is_the_holder_not_the_first_in_the_store() {
        // ADR-0027 Decision 4: with three claimants left, the finalizer
        // re-pushes the one the holder rule picks (published, oldest).
        let gone_record = a_record("old", "api", Some(ZONE), true);
        let zone = zone_ref(ZONE);
        let gone = claim_of(gone_record.as_ref(), &zone, "api");
        let live = vec![
            a_record("never-published", "api", Some(ZONE), false),
            published("newer", "api", "2026-10-09T00:00:00Z"),
            published("older", "api", "2026-10-03T00:00:00Z"),
        ];

        let claimant = live_claimant(&gone, &live).expect("api is still declared");

        assert_eq!(claimant.metadata.name.as_deref(), Some("older"));
    }

    #[test]
    fn without_a_published_claimant_the_first_one_is_re_pushed() {
        let gone_record = a_record("old", "api", Some(ZONE), true);
        let zone = zone_ref(ZONE);
        let gone = claim_of(gone_record.as_ref(), &zone, "api");
        let live = vec![
            a_record("first", "api", Some(ZONE), false),
            a_record("second", "api", Some(ZONE), false),
        ];

        let claimant = live_claimant(&gone, &live).expect("api is still declared");

        assert_eq!(claimant.metadata.name.as_deref(), Some("first"));
    }
}
