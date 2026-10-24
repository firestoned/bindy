// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for the per-name record's contributor map (ADR-0026).

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::scout::{
        LABEL_MANAGED_BY, LABEL_MANAGED_BY_SCOUT, LABEL_SOURCE_CLUSTER, LABEL_ZONE,
    };
    use std::fmt::Write as _;

    const CR_NAME: &str = "scout-prod-apex-example-com-0a1b2c3d";

    fn template(dns_name: &str) -> ARecord {
        new_name_record(&NameRecord {
            cr_name: CR_NAME,
            target_namespace: "bindy-system",
            cluster: "prod",
            zone: "example.com",
            dns_name,
        })
    }

    #[test]
    fn normalized_ips_sort_numerically_and_drop_repeats() {
        // ADR-0028 Decision 6: a reordered status is the same contribution.
        let ips = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(
            normalized_ips(ips(&["10.0.0.10", "10.0.0.9", "10.0.0.10"])),
            ips(&["10.0.0.9", "10.0.0.10"])
        );
        assert_eq!(
            normalized_ips(ips(&["10.0.0.9", "10.0.0.10"])),
            normalized_ips(ips(&["10.0.0.10", "10.0.0.9"]))
        );
        assert!(normalized_ips(Vec::new()).is_empty());
    }

    fn contribution(ips: &[&str], ttl: Option<i32>) -> Contribution {
        Contribution {
            ips: ips.iter().map(ToString::to_string).collect(),
            ttl,
        }
    }

    /// A record as the API server returns it: written once with `entries`,
    /// at `resource_version`.
    fn stored(entries: &[(&str, Contribution)], resource_version: &str) -> ARecord {
        let mut record = template("@");
        let map: Contributors = entries
            .iter()
            .map(|(k, c)| ((*k).to_string(), c.clone()))
            .collect();
        render(&mut record, &map).expect("render");
        record.metadata.resource_version = Some(resource_version.to_string());
        record
    }

    fn svc(name: &str) -> String {
        contributor_key("Service", "kube-system", name)
    }

    // ---------------------------------------------------------------------
    // Keys, labels, selectors
    // ---------------------------------------------------------------------

    #[test]
    fn a_contributor_key_is_kind_namespace_and_name() {
        assert_eq!(
            contributor_key("Service", "team-a", "svc-a"),
            "Service/team-a/svc-a"
        );
    }

    #[test]
    fn a_contributor_label_is_a_stable_hash_of_the_source_without_the_cluster() {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(b"Service\0team-a\0svc-a");
        let hex = digest.iter().take(8).fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        });
        assert_eq!(
            contributor_label("Service", "team-a", "svc-a"),
            format!("{CONTRIBUTOR_LABEL_PREFIX}{hex}")
        );
        assert_ne!(
            contributor_label("Service", "team-a", "web"),
            contributor_label("Ingress", "team-a", "web")
        );
        assert_ne!(
            contributor_label("Ingress", "team-a", "web"),
            contributor_label("Ingress", "team", "a-web")
        );
    }

    #[test]
    fn a_contributor_label_name_fits_the_label_limit() {
        let label = contributor_label("Service", &"n".repeat(63), &"s".repeat(253));
        let (prefix, name) = label.split_once('/').expect("prefixed label");
        assert!(name.len() <= 63, "{name}");
        assert!(prefix.len() <= 253);
    }

    #[test]
    fn the_own_record_selector_is_scoped_to_the_cluster_and_the_contributor() {
        let label = contributor_label("Service", "team-a", "svc-a");
        assert_eq!(
            name_record_selector("prod", &label),
            format!(
                "{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}=prod,{label}"
            )
        );
        assert_eq!(
            stale_name_record_selector("prod", "example.com", &label),
            format!(
                "{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}!=prod,\
                 {LABEL_ZONE}=example.com,{label}"
            )
        );
    }

    // ---------------------------------------------------------------------
    // The new record
    // ---------------------------------------------------------------------

    #[test]
    fn a_new_name_record_carries_the_base_labels_and_the_dns_name() {
        let record = template("@");
        assert_eq!(record.metadata.name.as_deref(), Some(CR_NAME));
        assert_eq!(record.metadata.namespace.as_deref(), Some("bindy-system"));
        let labels = record.metadata.labels.expect("labels");
        assert_eq!(labels.len(), 3, "{labels:?}");
        assert_eq!(labels[LABEL_MANAGED_BY], LABEL_MANAGED_BY_SCOUT);
        assert_eq!(labels[LABEL_SOURCE_CLUSTER], "prod");
        assert_eq!(labels[LABEL_ZONE], "example.com");
        assert_eq!(record.spec.name, "@");
        assert!(record.spec.ipv4_addresses.is_empty());
    }

    // ---------------------------------------------------------------------
    // Deriving the record from the map
    // ---------------------------------------------------------------------

    #[test]
    fn the_address_set_is_the_union_sorted_numerically_each_once() {
        let map: Contributors = [
            (svc("a"), contribution(&["10.0.0.246", "10.0.0.9"], None)),
            (svc("b"), contribution(&["10.0.0.241", "10.0.0.9"], None)),
            (svc("c"), contribution(&["10.0.0.240"], None)),
        ]
        .into();
        assert_eq!(
            derived_ips(&map),
            vec!["10.0.0.9", "10.0.0.240", "10.0.0.241", "10.0.0.246"]
        );
    }

    #[test]
    fn the_ttl_is_the_lowest_any_contributor_sets() {
        let map: Contributors = [
            (svc("a"), contribution(&["10.0.0.1"], Some(300))),
            (svc("b"), contribution(&["10.0.0.2"], None)),
            (svc("c"), contribution(&["10.0.0.3"], Some(60))),
        ]
        .into();
        assert_eq!(derived_ttl(&map), Some(60));
        assert!(ttls_disagree(&map));

        let none: Contributors = [(svc("a"), contribution(&["10.0.0.1"], None))].into();
        assert_eq!(derived_ttl(&none), None);
        assert!(!ttls_disagree(&none));

        let same: Contributors = [
            (svc("a"), contribution(&["10.0.0.1"], Some(60))),
            (svc("b"), contribution(&["10.0.0.2"], Some(60))),
        ]
        .into();
        assert!(!ttls_disagree(&same));
    }

    #[test]
    fn render_writes_the_annotation_the_contributor_labels_and_the_spec() {
        let mut record = template("@");
        let map: Contributors = [
            (svc("a"), contribution(&["10.0.0.2"], Some(300))),
            (
                contributor_key("Ingress", "web", "front"),
                contribution(&["10.0.0.1"], None),
            ),
        ]
        .into();
        render(&mut record, &map).expect("render");

        assert_eq!(contributors_of(&record).expect("parses back"), map);
        let labels = record.metadata.labels.clone().expect("labels");
        assert_eq!(
            labels.get(&contributor_label("Service", "kube-system", "a")),
            Some(&CONTRIBUTOR_LABEL_VALUE.to_string())
        );
        assert!(labels.contains_key(&contributor_label("Ingress", "web", "front")));
        assert_eq!(labels[LABEL_ZONE], "example.com");
        assert_eq!(record.spec.ipv4_addresses, vec!["10.0.0.1", "10.0.0.2"]);
        assert_eq!(record.spec.ttl, Some(300));
    }

    #[test]
    fn render_drops_the_label_of_a_contributor_that_left() {
        let mut record = stored(
            &[
                (&svc("a"), contribution(&["10.0.0.1"], None)),
                (&svc("b"), contribution(&["10.0.0.2"], None)),
            ],
            "4",
        );
        let only_a: Contributors = [(svc("a"), contribution(&["10.0.0.1"], None))].into();
        render(&mut record, &only_a).expect("render");
        let labels = record.metadata.labels.expect("labels");
        assert!(labels.contains_key(&contributor_label("Service", "kube-system", "a")));
        assert!(!labels.contains_key(&contributor_label("Service", "kube-system", "b")));
    }

    #[test]
    fn a_record_without_the_annotation_has_no_contributors() {
        assert!(contributors_of(&template("@")).expect("parses").is_empty());
    }

    #[test]
    fn a_malformed_annotation_is_an_error() {
        let mut record = template("@");
        record.metadata.annotations =
            Some([(ANNOTATION_CONTRIBUTORS.to_string(), "{not json".to_string())].into());
        let err = contributors_of(&record).expect_err("malformed");
        assert!(format!("{err:#}").contains(CR_NAME), "{err:#}");
    }

    // ---------------------------------------------------------------------
    // Planning a contribution
    // ---------------------------------------------------------------------

    #[test]
    fn the_first_contributor_creates_the_record() {
        let plan = plan_contribute(
            None,
            &template("@"),
            &svc("a"),
            &contribution(&["10.0.0.246"], None),
        )
        .expect("plan");
        let Plan::Create(record) = plan else {
            panic!("expected a create, got {plan:?}");
        };
        assert_eq!(record.spec.ipv4_addresses, vec!["10.0.0.246"]);
        assert_eq!(
            contributors_of(&record)
                .expect("parses")
                .keys()
                .collect::<Vec<_>>(),
            vec![&svc("a")]
        );
        assert!(record.metadata.resource_version.is_none());
    }

    #[test]
    fn a_second_contributor_replaces_the_record_with_the_union() {
        let current = stored(&[(&svc("a"), contribution(&["10.0.0.246"], None))], "4");
        let plan = plan_contribute(
            Some(&current),
            &template("@"),
            &svc("b"),
            &contribution(&["10.0.0.241"], None),
        )
        .expect("plan");
        let Plan::Replace(record) = plan else {
            panic!("expected a replace, got {plan:?}");
        };
        assert_eq!(record.spec.ipv4_addresses, vec!["10.0.0.241", "10.0.0.246"]);
        assert_eq!(
            record.metadata.resource_version.as_deref(),
            Some("4"),
            "the replace is conditional on the version read"
        );
        assert_eq!(contributors_of(&record).expect("parses").len(), 2);
    }

    #[test]
    fn a_changed_contribution_replaces_only_that_entry() {
        let current = stored(
            &[
                (&svc("a"), contribution(&["10.0.0.1"], None)),
                (&svc("b"), contribution(&["10.0.0.2"], None)),
            ],
            "4",
        );
        let plan = plan_contribute(
            Some(&current),
            &template("@"),
            &svc("a"),
            &contribution(&["10.0.0.3"], None),
        )
        .expect("plan");
        let Plan::Replace(record) = plan else {
            panic!("expected a replace, got {plan:?}");
        };
        assert_eq!(record.spec.ipv4_addresses, vec!["10.0.0.2", "10.0.0.3"]);
    }

    #[test]
    fn an_unchanged_contribution_writes_nothing() {
        let current = stored(
            &[
                (&svc("a"), contribution(&["10.0.0.1"], Some(60))),
                (&svc("b"), contribution(&["10.0.0.2"], None)),
            ],
            "4",
        );
        let plan = plan_contribute(
            Some(&current),
            &template("@"),
            &svc("a"),
            &contribution(&["10.0.0.1"], Some(60)),
        )
        .expect("plan");
        assert!(matches!(plan, Plan::Nothing), "{plan:?}");
    }

    #[test]
    fn a_hand_edited_spec_is_repaired_from_the_map() {
        let mut current = stored(&[(&svc("a"), contribution(&["10.0.0.1"], None))], "4");
        current.spec.ipv4_addresses = vec!["192.0.2.66".to_string()];
        let plan = plan_contribute(
            Some(&current),
            &template("@"),
            &svc("a"),
            &contribution(&["10.0.0.1"], None),
        )
        .expect("plan");
        let Plan::Replace(record) = plan else {
            panic!("expected a replace, got {plan:?}");
        };
        assert_eq!(record.spec.ipv4_addresses, vec!["10.0.0.1"]);
    }

    #[test]
    fn a_contribution_to_a_record_with_a_malformed_annotation_fails() {
        let mut current = stored(&[(&svc("a"), contribution(&["10.0.0.1"], None))], "4");
        current.metadata.annotations =
            Some([(ANNOTATION_CONTRIBUTORS.to_string(), "[]".to_string())].into());
        assert!(plan_contribute(
            Some(&current),
            &template("@"),
            &svc("b"),
            &contribution(&["10.0.0.2"], None)
        )
        .is_err());
    }

    // ---------------------------------------------------------------------
    // Planning a withdrawal
    // ---------------------------------------------------------------------

    #[test]
    fn withdrawing_from_a_missing_record_does_nothing() {
        assert!(matches!(
            plan_withdraw(None, &svc("a")).expect("plan"),
            Plan::Nothing
        ));
    }

    #[test]
    fn withdrawing_an_absent_entry_does_nothing() {
        let current = stored(&[(&svc("a"), contribution(&["10.0.0.1"], None))], "4");
        assert!(matches!(
            plan_withdraw(Some(&current), &svc("b")).expect("plan"),
            Plan::Nothing
        ));
    }

    #[test]
    fn withdrawing_one_of_several_keeps_the_others() {
        let current = stored(
            &[
                (&svc("a"), contribution(&["10.0.0.1"], Some(60))),
                (&svc("b"), contribution(&["10.0.0.2"], Some(300))),
            ],
            "4",
        );
        let plan = plan_withdraw(Some(&current), &svc("a")).expect("plan");
        let Plan::Replace(record) = plan else {
            panic!("expected a replace, got {plan:?}");
        };
        assert_eq!(record.spec.ipv4_addresses, vec!["10.0.0.2"]);
        assert_eq!(record.spec.ttl, Some(300));
        assert_eq!(record.metadata.resource_version.as_deref(), Some("4"));
        assert!(!record
            .metadata
            .labels
            .expect("labels")
            .contains_key(&contributor_label("Service", "kube-system", "a")));
    }

    #[test]
    fn withdrawing_the_last_contributor_deletes_the_record_conditionally() {
        let current = stored(&[(&svc("a"), contribution(&["10.0.0.1"], None))], "4");
        let plan = plan_withdraw(Some(&current), &svc("a")).expect("plan");
        let Plan::Delete { resource_version } = plan else {
            panic!("expected a delete, got {plan:?}");
        };
        assert_eq!(resource_version.as_deref(), Some("4"));
    }
}
