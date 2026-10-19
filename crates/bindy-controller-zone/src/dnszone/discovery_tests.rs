// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `discovery.rs`
//!
//! These tests document expected behavior for DNS record discovery.
//! Full implementation requires Kubernetes API mocking infrastructure.

#[cfg(test)]
#[allow(deprecated)] // DNSZoneSpec::name_server_ips is set to None in test fixtures
mod tests {
    use crate::crd::LabelSelector;
    use crate::dnszone::discovery::scout_record_belongs_to_zone;
    use std::collections::BTreeMap;

    // =====================================================================
    // ADR-0025 decision 5: a Scout record joins only the zone in its label
    // =====================================================================

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    const MANAGED_BY: &str = "bindy.firestoned.io/managed-by";
    const ZONE: &str = "bindy.firestoned.io/zone";

    #[test]
    fn a_scout_record_belongs_to_the_zone_its_label_names() {
        let l = labels(&[(MANAGED_BY, "scout"), (ZONE, "foo.example.com")]);
        assert!(scout_record_belongs_to_zone(&l, "foo.example.com"));
    }

    #[test]
    fn scout_zone_matching_ignores_case_and_a_trailing_dot() {
        let l = labels(&[(MANAGED_BY, "scout"), (ZONE, "Foo.Example.com")]);
        assert!(scout_record_belongs_to_zone(&l, "foo.example.com."));
    }

    #[test]
    fn a_scout_record_labelled_for_another_zone_does_not_belong() {
        let l = labels(&[(MANAGED_BY, "scout"), (ZONE, "foo.example.com")]);
        assert!(!scout_record_belongs_to_zone(&l, "bar.example.com"));
        assert!(!scout_record_belongs_to_zone(&l, "example.com"));
    }

    #[test]
    fn a_scout_record_without_a_zone_label_belongs_nowhere() {
        let l = labels(&[(MANAGED_BY, "scout")]);
        assert!(!scout_record_belongs_to_zone(&l, "foo.example.com"));
    }

    #[test]
    fn a_record_not_managed_by_scout_is_left_to_the_selector() {
        assert!(scout_record_belongs_to_zone(
            &labels(&[(ZONE, "foo.example.com")]),
            "bar.example.com"
        ));
        assert!(scout_record_belongs_to_zone(
            &labels(&[(MANAGED_BY, "Bind9Instance")]),
            "bar.example.com"
        ));
        assert!(scout_record_belongs_to_zone(
            &labels(&[]),
            "bar.example.com"
        ));
    }

    #[test]
    fn test_label_selector_matches_exact() {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "nginx".to_string());
        labels.insert("env".to_string(), "prod".to_string());

        let selector = LabelSelector {
            match_labels: Some(labels.clone()),
            match_expressions: None,
        };

        // Should match exact labels
        assert!(selector.matches(&labels));
    }

    /// Discovery maps a `status.records[].kind` to the RRset type it deletes
    /// through `DNSRecordKind::try_from(kind)?.to_hickory_record_type()`.
    fn hickory_type_of(kind: &str) -> anyhow::Result<hickory_proto::rr::RecordType> {
        Ok(crate::crd::DNSRecordKind::try_from(kind)?.to_hickory_record_type())
    }

    #[test]
    fn test_record_kind_maps_to_hickory_type_for_all_kinds() {
        use hickory_proto::rr::RecordType;

        let expected = [
            ("ARecord", RecordType::A),
            ("AAAARecord", RecordType::AAAA),
            ("TXTRecord", RecordType::TXT),
            ("CNAMERecord", RecordType::CNAME),
            ("MXRecord", RecordType::MX),
            ("NSRecord", RecordType::NS),
            ("SRVRecord", RecordType::SRV),
            ("CAARecord", RecordType::CAA),
            ("PTRRecord", RecordType::PTR),
        ];

        for (kind, record_type) in expected {
            assert_eq!(
                hickory_type_of(kind).expect("known kind must map"),
                record_type,
                "kind {kind} must map to {record_type}"
            );
        }
    }

    #[test]
    fn test_record_kind_to_hickory_type_rejects_unknown_kind() {
        assert!(hickory_type_of("UnknownRecord").is_err());
        assert!(hickory_type_of("").is_err());
    }

    // ========================================================================
    // claimed_by_live_record: a deleted or unselected record's DNS data must
    // survive when another live record in the zone declares the same name and
    // type (e.g. the same record renamed: delete old CR, create new CR).
    // ========================================================================

    fn claim_ref(
        kind: &str,
        name: &str,
        record_name: Option<&str>,
    ) -> crate::crd::RecordReferenceWithTimestamp {
        crate::crd::RecordReferenceWithTimestamp {
            api_version: crate::constants::API_GROUP_VERSION.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            namespace: "dns".to_string(),
            record_name: record_name.map(str::to_string),
            last_reconciled_at: None,
        }
    }

    #[test]
    fn test_claimed_by_live_record_finds_the_renamed_record() {
        use super::super::claimed_by_live_record;

        let gone = claim_ref("ARecord", "example-com-www", Some("www"));
        let live = vec![
            claim_ref("ARecord", "api.example.com", Some("api")),
            claim_ref("ARecord", "www.example.com", Some("www")),
        ];

        let claimant = claimed_by_live_record(&gone, &live).expect("www is still declared");
        assert_eq!(claimant.name, "www.example.com");
    }

    #[test]
    fn test_claimed_by_live_record_ignores_other_types_and_names() {
        use super::super::claimed_by_live_record;

        let gone = claim_ref("ARecord", "example-com-www", Some("www"));
        let live = vec![
            // Same name, different type: a different RRset.
            claim_ref("TXTRecord", "www-txt", Some("www")),
            // Same type, different name.
            claim_ref("ARecord", "api.example.com", Some("api")),
        ];

        assert!(claimed_by_live_record(&gone, &live).is_none());
    }

    #[test]
    fn test_claimed_by_live_record_is_case_insensitive() {
        use super::super::claimed_by_live_record;

        // DNS owner names compare case-insensitively.
        let gone = claim_ref("ARecord", "old", Some("WWW"));
        let live = vec![claim_ref("ARecord", "new", Some("www"))];

        assert!(claimed_by_live_record(&gone, &live).is_some());
    }

    #[test]
    fn test_claimed_by_live_record_never_counts_itself() {
        use super::super::claimed_by_live_record;

        // An unselected record can still be listed among the live records of
        // another selector pass; it must not keep its own data alive.
        let gone = claim_ref("ARecord", "example-com-www", Some("www"));
        let live = vec![claim_ref("ARecord", "example-com-www", Some("www"))];

        assert!(claimed_by_live_record(&gone, &live).is_none());
    }

    #[test]
    fn test_claimed_by_live_record_without_a_dns_name() {
        use super::super::claimed_by_live_record;

        let gone = claim_ref("ARecord", "old", None);
        let live = vec![claim_ref("ARecord", "new", None)];

        assert!(
            claimed_by_live_record(&gone, &live).is_none(),
            "no recorded DNS name: nothing to match on"
        );
    }

    #[test]
    fn test_unselected_previous_records_partitions_correctly() {
        use super::super::unselected_previous_records;
        use crate::crd::RecordReferenceWithTimestamp;
        use std::collections::HashSet;

        let make_ref = |kind: &str, name: &str| RecordReferenceWithTimestamp {
            api_version: crate::constants::API_GROUP_VERSION.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            namespace: "default".to_string(),
            record_name: Some("www".to_string()),
            last_reconciled_at: None,
        };

        let previous = vec![
            make_ref("ARecord", "still-selected"),
            make_ref("TXTRecord", "no-longer-selected"),
        ];

        let current: HashSet<String> = ["ARecord/still-selected".to_string()].into_iter().collect();

        let unselected = unselected_previous_records(&previous, &current);

        assert_eq!(unselected.len(), 1);
        assert_eq!(unselected[0].kind, "TXTRecord");
        assert_eq!(unselected[0].name, "no-longer-selected");
    }

    #[test]
    fn test_unselected_previous_records_empty_when_all_still_selected() {
        use super::super::unselected_previous_records;
        use crate::crd::RecordReferenceWithTimestamp;
        use std::collections::HashSet;

        let previous = vec![RecordReferenceWithTimestamp {
            api_version: crate::constants::API_GROUP_VERSION.to_string(),
            kind: "ARecord".to_string(),
            name: "web".to_string(),
            namespace: "default".to_string(),
            record_name: None,
            last_reconciled_at: None,
        }];

        let current: HashSet<String> = ["ARecord/web".to_string()].into_iter().collect();

        assert!(unselected_previous_records(&previous, &current).is_empty());
    }

    #[test]
    fn test_label_selector_no_match_different_value() {
        let mut selector_labels = BTreeMap::new();
        selector_labels.insert("app".to_string(), "nginx".to_string());

        let mut pod_labels = BTreeMap::new();
        pod_labels.insert("app".to_string(), "apache".to_string());

        let selector = LabelSelector {
            match_labels: Some(selector_labels),
            match_expressions: None,
        };

        // Should not match when value differs
        assert!(!selector.matches(&pod_labels));
    }

    // ========================================================================
    // zones_configured_on_instance - Endpoints watch mapper (issue #486)
    // ========================================================================

    fn zone_on_instances(
        zone_namespace: &str,
        zone_name: &str,
        instances: &[(&str, &str)],
    ) -> crate::crd::DNSZone {
        use crate::crd::{
            DNSZone, DNSZoneSpec, DNSZoneStatus, InstanceReferenceWithStatus, SOARecord,
        };
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        DNSZone {
            metadata: ObjectMeta {
                name: Some(zone_name.to_string()),
                namespace: Some(zone_namespace.to_string()),
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
                    retry: 1800,
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
            status: Some(DNSZoneStatus {
                bind9_instances: instances
                    .iter()
                    .map(|(namespace, name)| InstanceReferenceWithStatus {
                        api_version: crate::constants::API_GROUP_VERSION.to_string(),
                        kind: crate::constants::KIND_BIND9_INSTANCE.to_string(),
                        name: (*name).to_string(),
                        namespace: (*namespace).to_string(),
                        status: crate::crd::InstanceStatus::Configured,
                        last_reconciled_at: None,
                        message: None,
                    })
                    .collect(),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn test_zones_configured_on_instance_matches_serving_zone() {
        // A pod of `primary-dns` was replaced: every zone served by that
        // instance must reconcile so a wiped zone is recreated and replayed.
        let zones = vec![zone_on_instances(
            "bindy-system",
            "example-zone",
            &[("bindy-system", "primary-dns")],
        )];

        let matched =
            super::super::zones_configured_on_instance(&zones, "bindy-system", "primary-dns");

        assert_eq!(
            matched,
            vec![("bindy-system".to_string(), "example-zone".to_string())]
        );
    }

    #[test]
    fn test_zones_configured_on_instance_ignores_other_instances() {
        let zones = vec![zone_on_instances(
            "bindy-system",
            "example-zone",
            &[("bindy-system", "primary-dns")],
        )];

        assert!(
            super::super::zones_configured_on_instance(&zones, "bindy-system", "other-dns")
                .is_empty()
        );
    }

    #[test]
    fn test_zones_configured_on_instance_is_namespace_scoped() {
        // Same instance name in a different namespace must NOT match - two
        // tenants may each run a `primary-dns`.
        let zones = vec![zone_on_instances(
            "bindy-system",
            "example-zone",
            &[("bindy-system", "primary-dns")],
        )];

        assert!(
            super::super::zones_configured_on_instance(&zones, "tenant-a", "primary-dns")
                .is_empty()
        );
    }

    #[test]
    fn test_zones_configured_on_instance_skips_zones_without_status() {
        // A zone that has never reconciled has no configured instances yet.
        let mut zone = zone_on_instances("bindy-system", "example-zone", &[]);
        zone.status = None;

        assert!(
            super::super::zones_configured_on_instance(&[zone], "bindy-system", "primary-dns")
                .is_empty()
        );
    }

    #[test]
    fn test_zones_configured_on_instance_returns_all_zones_sharing_an_instance() {
        // One instance commonly serves many zones - a single pod restart must
        // fan out to every one of them.
        let zones = vec![
            zone_on_instances("bindy-system", "zone-a", &[("bindy-system", "primary-dns")]),
            zone_on_instances("bindy-system", "zone-b", &[("bindy-system", "primary-dns")]),
        ];

        let matched =
            super::super::zones_configured_on_instance(&zones, "bindy-system", "primary-dns");

        assert_eq!(matched.len(), 2);
    }

    // ========================================================================
    // Tagging only what is not tagged yet (ADR-0015): a zone reconcile used to
    // PATCH status.zoneRef on every selected record, every time.
    // ========================================================================

    fn zone_reference(name: &str, namespace: &str, zone_name: &str) -> crate::crd::ZoneReference {
        crate::crd::ZoneReference {
            api_version: crate::constants::API_GROUP_VERSION.to_string(),
            kind: crate::constants::KIND_DNS_ZONE.to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            zone_name: zone_name.to_string(),
            last_reconciled_at: None,
        }
    }

    #[test]
    fn test_is_tagged_with_zone_true_for_the_same_zone() {
        use super::super::is_tagged_with_zone;
        let zone_ref = zone_reference("example-com", "dns", "example.com");

        assert!(is_tagged_with_zone(
            Some(&zone_ref),
            "dns",
            "example-com",
            "example.com"
        ));
    }

    #[test]
    fn test_is_tagged_with_zone_false_when_untagged() {
        use super::super::is_tagged_with_zone;

        assert!(!is_tagged_with_zone(
            None,
            "dns",
            "example-com",
            "example.com"
        ));
    }

    #[test]
    fn test_is_tagged_with_zone_false_for_another_zone_or_fqdn() {
        use super::super::is_tagged_with_zone;
        let other_zone = zone_reference("other", "dns", "example.com");
        let other_ns = zone_reference("example-com", "elsewhere", "example.com");
        let other_fqdn = zone_reference("example-com", "dns", "example.org");

        for zone_ref in [other_zone, other_ns, other_fqdn] {
            assert!(!is_tagged_with_zone(
                Some(&zone_ref),
                "dns",
                "example-com",
                "example.com"
            ));
        }
    }

    // ========================================================================
    // Records retained by stale cleanup stay in status.records until their DNS
    // data is confirmed gone (the rc.2 "deleted but still served" orphans).
    // ========================================================================

    #[test]
    fn test_merge_retained_records_appends_missing_retained_refs() {
        use super::super::merge_retained_records;
        let mut current = vec![claim_ref("ARecord", "live", Some("live"))];
        let retained = vec![claim_ref("ARecord", "deleted", Some("gone"))];

        merge_retained_records(&mut current, &retained);

        let names: Vec<&str> = current.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["live", "deleted"]);
    }

    #[test]
    fn test_merge_retained_records_does_not_duplicate() {
        use super::super::merge_retained_records;
        let mut current = vec![claim_ref("ARecord", "www", Some("www"))];
        let retained = vec![claim_ref("ARecord", "www", Some("www"))];

        merge_retained_records(&mut current, &retained);

        assert_eq!(current.len(), 1);
    }
}
