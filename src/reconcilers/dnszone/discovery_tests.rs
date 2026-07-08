// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `discovery.rs`
//!
//! These tests document expected behavior for DNS record discovery.
//! Full implementation requires Kubernetes API mocking infrastructure.

#[cfg(test)]
mod tests {
    use crate::crd::LabelSelector;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn test_reconcile_zone_records_no_selectors() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A zone with no recordsFrom selectors
        // When: reconcile_zone_records is called
        // Then: Should return Ok(vec![]) immediately
        //       AND log "No label selectors defined - skipping record discovery"
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

    #[test]
    fn test_hickory_record_type_for_kind_maps_all_kinds() {
        use super::super::hickory_record_type_for_kind;
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
        ];

        for (kind, record_type) in expected {
            assert_eq!(
                hickory_record_type_for_kind(kind).expect("known kind must map"),
                record_type,
                "kind {kind} must map to {record_type}"
            );
        }
    }

    #[test]
    fn test_hickory_record_type_for_kind_rejects_unknown_kind() {
        use super::super::hickory_record_type_for_kind;

        assert!(hickory_record_type_for_kind("PTRRecord").is_err());
        assert!(hickory_record_type_for_kind("").is_err());
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
}
