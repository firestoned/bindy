// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `selector.rs`

use crate::crd::{
    ARecord, ARecordSpec, DNSZone, DNSZoneSpec, LabelSelector, RecordSource, SOARecord,
};
use crate::selector::find_zones_for_record;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::collections::BTreeMap;

fn create_test_zone(
    name: &str,
    namespace: &str,
    match_labels: BTreeMap<String, String>,
) -> DNSZone {
    DNSZone {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: DNSZoneSpec {
            zone_name: format!("{}.com", name),
            cluster_ref: Some("test-cluster".to_string()),
            cluster_provider_ref: None,
            soa_record: SOARecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            ttl: Some(3600),
            name_server_ips: None,
            records_from: Some(vec![RecordSource {
                selector: LabelSelector {
                    match_labels: Some(match_labels),
                    match_expressions: None,
                },
            }]),
        },
        status: None,
    }
}

fn create_test_record(name: &str, namespace: &str, labels: BTreeMap<String, String>) -> ARecord {
    ARecord {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: ARecordSpec {
            name: "www".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: Some(300),
        },
        status: None,
    }
}

#[test]
fn test_find_zones_for_record_single_match() {
    // Create a store with one zone
    let zone = create_test_zone(
        "example",
        "dns-system",
        BTreeMap::from([("zone".to_string(), "example.com".to_string())]),
    );

    let (reader, mut writer) = kube::runtime::reflector::store::<DNSZone>();
    writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone.clone()));

    // Create a record that matches
    let record = create_test_record(
        "www-record",
        "dns-system",
        BTreeMap::from([("zone".to_string(), "example.com".to_string())]),
    );

    // Find matching zones
    let matches = find_zones_for_record(&reader, &record);

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "example");
    assert_eq!(matches[0].namespace.as_deref(), Some("dns-system"));
}

#[test]
fn test_find_zones_for_record_no_match() {
    // Create a store with one zone
    let zone = create_test_zone(
        "example",
        "dns-system",
        BTreeMap::from([("zone".to_string(), "example.com".to_string())]),
    );

    let (reader, mut writer) = kube::runtime::reflector::store::<DNSZone>();
    writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone.clone()));

    // Create a record that DOESN'T match
    let record = create_test_record(
        "www-record",
        "dns-system",
        BTreeMap::from([("zone".to_string(), "different.com".to_string())]),
    );

    // Find matching zones
    let matches = find_zones_for_record(&reader, &record);

    assert_eq!(matches.len(), 0);
}

#[test]
fn test_find_zones_for_record_multiple_matches() {
    // Create a store with multiple zones
    let zone1 = create_test_zone(
        "example",
        "dns-system",
        BTreeMap::from([("app".to_string(), "web".to_string())]),
    );
    let zone2 = create_test_zone(
        "internal",
        "dns-system",
        BTreeMap::from([("app".to_string(), "web".to_string())]),
    );

    let (reader, mut writer) = kube::runtime::reflector::store::<DNSZone>();
    writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone1.clone()));
    writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone2.clone()));

    // Create a record that matches both zones
    let record = create_test_record(
        "www-record",
        "dns-system",
        BTreeMap::from([("app".to_string(), "web".to_string())]),
    );

    // Find matching zones
    let mut matches = find_zones_for_record(&reader, &record);
    matches.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].name, "example");
    assert_eq!(matches[1].name, "internal");
}

#[test]
fn test_find_zones_for_record_different_namespace() {
    // Create a zone in namespace "dns-system"
    let zone = create_test_zone(
        "example",
        "dns-system",
        BTreeMap::from([("zone".to_string(), "example.com".to_string())]),
    );

    let (reader, mut writer) = kube::runtime::reflector::store::<DNSZone>();
    writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone.clone()));

    // Create a record in a DIFFERENT namespace
    let record = create_test_record(
        "www-record",
        "other-namespace",
        BTreeMap::from([("zone".to_string(), "example.com".to_string())]),
    );

    // Find matching zones - should be empty due to namespace mismatch
    let matches = find_zones_for_record(&reader, &record);

    assert_eq!(matches.len(), 0);
}

#[test]
fn test_find_zones_for_record_zone_without_selector() {
    // Create a zone WITHOUT recordsFrom selector
    let mut zone = create_test_zone(
        "example",
        "dns-system",
        BTreeMap::from([("zone".to_string(), "example.com".to_string())]),
    );
    zone.spec.records_from = None;

    let (reader, mut writer) = kube::runtime::reflector::store::<DNSZone>();
    writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(zone.clone()));

    // Create a record
    let record = create_test_record(
        "www-record",
        "dns-system",
        BTreeMap::from([("zone".to_string(), "example.com".to_string())]),
    );

    // Find matching zones - should be empty since zone has no selector
    let matches = find_zones_for_record(&reader, &record);

    assert_eq!(matches.len(), 0);
}
