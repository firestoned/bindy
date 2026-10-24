// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for Scout name conflicts (ADR-0027).

use super::*;
use bindy_api::crd::ARecord;
use std::collections::BTreeMap;

const ZONE_NS: &str = "bindy-system";
const ZONE_CR: &str = "shared-test";
const RECORDS_NS: &str = "bindy-system";

/// Builder for an `ARecord` as the API server holds it.
struct Rec {
    name: &'static str,
    dns_name: &'static str,
    cluster: Option<&'static str>,
    zone: Option<&'static str>,
    published: bool,
    created: &'static str,
    deleting: bool,
}

fn rec(name: &'static str, cluster: &'static str, created: &'static str) -> Rec {
    Rec {
        name,
        dns_name: "api",
        cluster: Some(cluster),
        zone: Some(ZONE_CR),
        published: false,
        created,
        deleting: false,
    }
}

impl Rec {
    fn published(mut self) -> Self {
        self.published = true;
        self
    }
    fn hand_written(mut self) -> Self {
        self.cluster = None;
        self
    }
    fn deleting(mut self) -> Self {
        self.deleting = true;
        self
    }
    fn unbound(mut self) -> Self {
        self.zone = None;
        self
    }
    fn named(mut self, dns_name: &'static str) -> Self {
        self.dns_name = dns_name;
        self
    }
    fn build(&self) -> ARecord {
        let mut labels = serde_json::Map::new();
        if let Some(cluster) = self.cluster {
            labels.insert(
                bindy_api::labels::BINDY_MANAGED_BY_LABEL.to_string(),
                serde_json::json!(bindy_api::labels::MANAGED_BY_SCOUT),
            );
            labels.insert(
                bindy_api::labels::BINDY_SOURCE_CLUSTER_LABEL.to_string(),
                serde_json::json!(cluster),
            );
        }
        let mut status = serde_json::json!({"conditions": []});
        if let Some(zone) = self.zone {
            status["zoneRef"] = serde_json::json!({
                "apiVersion": "bindy.firestoned.io/v1beta1", "kind": "DNSZone",
                "name": zone, "namespace": ZONE_NS, "zoneName": "shared.test"
            });
        }
        if self.published {
            status["publishedName"] = serde_json::json!(self.dns_name);
            status["lastUpdated"] = serde_json::json!("2026-10-23T10:00:00Z");
        }
        let mut meta = serde_json::json!({
            "name": self.name, "namespace": RECORDS_NS, "labels": labels,
            "creationTimestamp": self.created,
        });
        if self.deleting {
            meta["deletionTimestamp"] = serde_json::json!("2026-10-23T12:00:00Z");
        }
        serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1", "kind": "ARecord",
            "metadata": meta,
            "spec": {"name": self.dns_name, "ipv4Addresses": ["192.0.2.1"]},
            "status": status,
        }))
        .expect("valid ARecord")
    }
}

fn registry(entries: &[(&str, &str)]) -> ScoutRegistry {
    let data: BTreeMap<String, String> = entries
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    ScoutRegistry::from_data(&data)
}

fn verdict_name(me: &ARecord, all: &[ARecord], reg: &ScoutRegistry) -> Option<String> {
    let me = claim_facts(me).expect("me is bound");
    let facts: Vec<ClaimFacts<'_>> = all.iter().filter_map(claim_facts).collect();
    match scout_conflict(&me, &facts, reg) {
        ConflictVerdict::Clear => None,
        ConflictVerdict::HeldBack(holder) => Some(holder.name.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Facts
// ---------------------------------------------------------------------------

#[test]
fn an_unbound_record_claims_nothing() {
    assert!(claim_facts(&rec("a", "foo", "2026-10-01T00:00:00Z").unbound().build()).is_none());
}

#[test]
fn published_needs_the_published_name_and_a_successful_write() {
    let r = rec("a", "foo", "2026-10-01T00:00:00Z").published().build();
    assert!(claim_facts(&r).expect("bound").published);
    let mut renamed = r.clone();
    renamed.spec.name = "www".to_string();
    assert!(
        !claim_facts(&renamed).expect("bound").published,
        "published under its old name"
    );
    let mut never_written = r;
    never_written.status.as_mut().expect("status").last_updated = None;
    assert!(!claim_facts(&never_written).expect("bound").published);
}

#[test]
fn only_scout_records_carry_a_cluster() {
    let scout = rec("a", "foo", "2026-10-01T00:00:00Z").build();
    let hand = rec("b", "foo", "2026-10-01T00:00:00Z")
        .hand_written()
        .build();
    assert_eq!(
        claim_facts(&scout).expect("bound").source_cluster,
        Some("foo")
    );
    assert_eq!(claim_facts(&hand).expect("bound").source_cluster, None);
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

#[test]
fn two_names_on_one_account_are_one_cluster() {
    let reg = registry(&[
        ("scout-foo.clusters", "foo, foo-old"),
        ("scout-foo.zones", "foo.test"),
        ("scout-bar.clusters", "bar"),
    ]);
    assert!(reg.same_account("foo", "foo-old"));
    assert!(reg.same_account("foo-old", "foo"));
    assert!(!reg.same_account("foo", "bar"));
    assert!(!reg.same_account("foo", "unknown"));
}

#[test]
fn an_empty_registry_registers_nothing_together() {
    assert!(!ScoutRegistry::default().same_account("foo", "foo-old"));
}

// ---------------------------------------------------------------------------
// Holder (Decision 2)
// ---------------------------------------------------------------------------

#[test]
fn the_record_that_published_keeps_the_name() {
    let older = rec("bar-api", "bar", "2026-10-01T00:00:00Z").build();
    let published = rec("foo-api", "foo", "2026-10-05T00:00:00Z")
        .published()
        .build();
    let all = [older.clone(), published.clone()];
    assert_eq!(
        verdict_name(&older, &all, &ScoutRegistry::default()).as_deref(),
        Some("foo-api"),
        "an older record that never published does not take a published name"
    );
    assert_eq!(
        verdict_name(&published, &all, &ScoutRegistry::default()),
        None
    );
}

#[test]
fn among_several_published_the_oldest_keeps_the_name() {
    let a = rec("bar-api", "bar", "2026-10-01T00:00:00Z")
        .published()
        .build();
    let b = rec("foo-api", "foo", "2026-10-05T00:00:00Z")
        .published()
        .build();
    let all = [a.clone(), b.clone()];
    assert_eq!(verdict_name(&a, &all, &ScoutRegistry::default()), None);
    assert_eq!(
        verdict_name(&b, &all, &ScoutRegistry::default()).as_deref(),
        Some("bar-api")
    );
}

#[test]
fn ties_are_broken_by_name() {
    let a = rec("a-api", "bar", "2026-10-01T00:00:00Z")
        .published()
        .build();
    let b = rec("b-api", "foo", "2026-10-01T00:00:00Z")
        .published()
        .build();
    let all = [b.clone(), a.clone()];
    assert_eq!(verdict_name(&a, &all, &ScoutRegistry::default()), None);
    assert_eq!(
        verdict_name(&b, &all, &ScoutRegistry::default()).as_deref(),
        Some("a-api")
    );
}

#[test]
fn when_nobody_has_published_nobody_is_held_back() {
    let a = rec("bar-api", "bar", "2026-10-01T00:00:00Z").build();
    let b = rec("foo-api", "foo", "2026-10-05T00:00:00Z").build();
    let all = [a.clone(), b.clone()];
    assert_eq!(verdict_name(&a, &all, &ScoutRegistry::default()), None);
    assert_eq!(verdict_name(&b, &all, &ScoutRegistry::default()), None);
}

#[test]
fn a_holder_that_is_being_deleted_hands_the_name_on() {
    let leaving = rec("foo-api", "foo", "2026-10-01T00:00:00Z")
        .published()
        .deleting()
        .build();
    let next = rec("bar-api", "bar", "2026-10-05T00:00:00Z").build();
    let all = [leaving, next.clone()];
    assert_eq!(verdict_name(&next, &all, &ScoutRegistry::default()), None);
}

// ---------------------------------------------------------------------------
// What is a conflict (Decision 1)
// ---------------------------------------------------------------------------

#[test]
fn the_same_cluster_is_never_a_conflict() {
    // A handover inside one cluster: per-source record next to per-name.
    let old = rec("per-source", "foo", "2026-10-01T00:00:00Z")
        .published()
        .build();
    let new = rec("per-name", "foo", "2026-10-05T00:00:00Z").build();
    let all = [old, new.clone()];
    assert_eq!(verdict_name(&new, &all, &ScoutRegistry::default()), None);
}

#[test]
fn a_registered_rename_is_never_a_conflict() {
    let old = rec("foo-old-api", "foo-old", "2026-10-01T00:00:00Z")
        .published()
        .build();
    let new = rec("foo-api", "foo", "2026-10-05T00:00:00Z").build();
    let all = [old.clone(), new.clone()];
    let reg = registry(&[("scout-foo.clusters", "foo,foo-old")]);
    assert_eq!(verdict_name(&new, &all, &reg), None);
    assert_eq!(
        verdict_name(&new, &all, &ScoutRegistry::default()).as_deref(),
        Some("foo-old-api"),
        "unregistered, the rename shows as a conflict and the old record keeps serving"
    );
}

#[test]
fn hand_written_records_are_never_in_conflict() {
    let hand = rec("hand", "x", "2026-10-01T00:00:00Z")
        .hand_written()
        .published()
        .build();
    let scout = rec("foo-api", "foo", "2026-10-05T00:00:00Z").build();
    let all = [hand.clone(), scout.clone()];
    assert_eq!(verdict_name(&scout, &all, &ScoutRegistry::default()), None);
    assert_eq!(verdict_name(&hand, &all, &ScoutRegistry::default()), None);
}

#[test]
fn other_names_and_zones_are_not_rivals() {
    let api = rec("bar-api", "bar", "2026-10-01T00:00:00Z")
        .published()
        .build();
    let www = rec("foo-www", "foo", "2026-10-05T00:00:00Z")
        .named("www")
        .build();
    assert_eq!(
        verdict_name(&www, &[api.clone(), www.clone()], &ScoutRegistry::default()),
        None
    );
    let mut other_zone = rec("foo-api", "foo", "2026-10-05T00:00:00Z").build();
    other_zone
        .status
        .as_mut()
        .and_then(|s| s.zone_ref.as_mut())
        .expect("zone ref")
        .name = "other-zone".to_string();
    assert_eq!(
        verdict_name(
            &other_zone,
            &[api, other_zone.clone()],
            &ScoutRegistry::default()
        ),
        None
    );
}

#[test]
fn names_are_compared_ignoring_case() {
    let held = rec("foo-api", "foo", "2026-10-05T00:00:00Z")
        .named("API")
        .build();
    let holder = rec("bar-api", "bar", "2026-10-01T00:00:00Z")
        .published()
        .build();
    assert_eq!(
        verdict_name(&held, &[holder, held.clone()], &ScoutRegistry::default()).as_deref(),
        Some("bar-api")
    );
}

#[test]
fn a_stale_copy_of_itself_in_the_store_is_not_a_rival() {
    let me = rec("foo-api", "foo", "2026-10-05T00:00:00Z").build();
    let mut store_copy = me.clone();
    store_copy.metadata.labels.as_mut().expect("labels").insert(
        bindy_api::labels::BINDY_SOURCE_CLUSTER_LABEL.to_string(),
        "someone-else".to_string(),
    );
    assert_eq!(
        verdict_name(&me, &[store_copy], &ScoutRegistry::default()),
        None
    );
}

#[test]
fn a_record_being_deleted_is_never_held_back() {
    let me = rec("foo-api", "foo", "2026-10-05T00:00:00Z")
        .deleting()
        .build();
    let holder = rec("bar-api", "bar", "2026-10-01T00:00:00Z")
        .published()
        .build();
    assert_eq!(
        verdict_name(&me, &[holder], &ScoutRegistry::default()),
        None
    );
}

#[test]
fn has_cross_cluster_rivals_skips_the_registry_lookup_when_alone() {
    let me = rec("foo-api", "foo", "2026-10-05T00:00:00Z").build();
    let same_cluster = [rec("foo-api-2", "foo", "2026-10-01T00:00:00Z")
        .published()
        .build()];
    let facts: Vec<_> = same_cluster.iter().filter_map(claim_facts).collect();
    let me_facts = claim_facts(&me).expect("bound");
    assert!(!has_cross_cluster_rivals(&me_facts, &facts));
    let other = [rec("bar-api", "bar", "2026-10-01T00:00:00Z").build()];
    let facts2: Vec<_> = other.iter().filter_map(claim_facts).collect();
    assert!(has_cross_cluster_rivals(&me_facts, &facts2));
}

// ---------------------------------------------------------------------------
// The finalizer re-push picks the holder (Decision 4)
// ---------------------------------------------------------------------------

#[test]
fn the_holder_among_claimants_is_the_published_oldest() {
    let unpublished_old = rec("a", "bar", "2026-10-01T00:00:00Z").build();
    let published_new = rec("b", "baz", "2026-10-09T00:00:00Z").published().build();
    let published_old = rec("c", "qux", "2026-10-03T00:00:00Z").published().build();
    let all = [unpublished_old, published_new, published_old];
    let facts: Vec<_> = all.iter().filter_map(claim_facts).collect();
    assert_eq!(rrset_holder(facts.iter()).map(|h| h.name), Some("c"));
    assert_eq!(rrset_holder(facts[..1].iter()).map(|h| h.name), None);
}

#[test]
fn the_conflict_message_names_the_holder_its_cluster_and_the_name() {
    let holder = rec("scout-foo-api", "foo", "2026-10-01T00:00:00Z")
        .published()
        .build();
    assert_eq!(
        conflict_message(&holder, "shared.test"),
        "ARecord bindy-system/scout-foo-api of cluster 'foo' has published 'api' in shared.test; \
         this record writes nothing until it leaves (ADR-0027)"
    );
}
