// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Scout name conflicts (ADR-0027).
//!
//! Scout records of two edge clusters that claim one DNS name in one zone
//! would replace each other's RRset on every write (ADR-0021). Instead, the
//! record that has already **published** the name keeps it and the others are
//! held back. Every path that writes a record to BIND9 asks [`scout_conflict`]
//! first: the record controller, zone replay, and the finalizer re-push
//! ([`rrset_holder`], through `rrset_claim::live_claimant`).
//!
//! Two records are rivals when both are Scout-managed, bound to the same zone,
//! of the same kind, declare the same name (ignoring case), are not being
//! deleted, and come from different `source-cluster` names that are not
//! registered to one account in the `bindy-scout-sources` ConfigMap (two names
//! on one account are a cluster rename, ADR-0025).

use crate::context::RecordKind;
use anyhow::{Context as _, Result};
use bindy_api::labels::{
    BINDY_MANAGED_BY_LABEL, BINDY_SOURCE_CLUSTER_LABEL, MANAGED_BY_SCOUT,
    SCOUT_SOURCES_CLUSTERS_SUFFIX, SCOUT_SOURCES_CONFIGMAP_NAME,
};
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::jiff::Timestamp;
use kube::{Api, Client};
use std::collections::{BTreeMap, BTreeSet};

/// What the conflict rule needs to know about one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimFacts<'a> {
    /// The record kind, e.g. `ARecord`
    pub kind: &'a str,
    /// Namespace of the record resource
    pub namespace: &'a str,
    /// Name of the record resource
    pub name: &'a str,
    /// Namespace of the `DNSZone` the record is bound to
    pub zone_namespace: &'a str,
    /// Name of the `DNSZone` the record is bound to
    pub zone_name: &'a str,
    /// The DNS owner name the record declares (`spec.name`)
    pub dns_name: &'a str,
    /// The cluster that wrote it, for a Scout-managed record; `None` otherwise
    pub source_cluster: Option<&'a str>,
    /// Whether the record resource is being deleted
    pub deleting: bool,
    /// Whether the record has published this name to BIND9:
    /// `status.publishedName` equals `spec.name` (ignoring case) and
    /// `status.lastUpdated` is set, both written only after a successful write
    pub published: bool,
    /// When the record resource was created
    pub created: Option<Timestamp>,
}

impl ClaimFacts<'_> {
    /// Whether `other` is the same record resource.
    fn same_object(&self, other: &ClaimFacts<'_>) -> bool {
        self.kind == other.kind && self.namespace == other.namespace && self.name == other.name
    }

    /// Whether `other` claims the same RRset: same kind, zone and name.
    fn same_rrset(&self, other: &ClaimFacts<'_>) -> bool {
        self.kind == other.kind
            && self.zone_namespace == other.zone_namespace
            && self.zone_name == other.zone_name
            && self.dns_name.eq_ignore_ascii_case(other.dns_name)
    }

    /// The order that picks a holder: oldest first, then namespace and name.
    fn holder_key(&self) -> (Option<Timestamp>, &str, &str) {
        (self.created, self.namespace, self.name)
    }
}

/// The facts of `record`, or `None` when no zone has bound it
/// (`status.zoneRef` unset): an unbound record claims nothing.
#[must_use]
pub fn claim_facts<K: RecordKind>(record: &K) -> Option<ClaimFacts<'_>> {
    let status = record.record_status()?;
    let zone_ref = status.zone_ref.as_ref()?;
    let meta = record.meta();
    let labels = meta.labels.as_ref();
    let scout = labels
        .and_then(|l| l.get(BINDY_MANAGED_BY_LABEL))
        .is_some_and(|v| v == MANAGED_BY_SCOUT);
    let source_cluster = scout
        .then(|| labels.and_then(|l| l.get(BINDY_SOURCE_CLUSTER_LABEL)))
        .flatten()
        .map(String::as_str);
    let dns_name = record.spec_name();
    let published = status.last_updated.is_some()
        && status
            .published_name
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case(dns_name));
    Some(ClaimFacts {
        kind: K::KIND,
        namespace: meta.namespace.as_deref().unwrap_or_default(),
        name: meta.name.as_deref().unwrap_or_default(),
        zone_namespace: &zone_ref.namespace,
        zone_name: &zone_ref.name,
        dns_name,
        source_cluster,
        deleting: meta.deletion_timestamp.is_some(),
        published,
        created: meta.creation_timestamp.as_ref().map(|t| t.0),
    })
}

/// The holder among `claimants` (ADR-0027 Decision 2): of those that have
/// published and are not being deleted, the oldest, ties broken by namespace
/// and name. `None` when none has published: nobody holds the name yet.
#[must_use]
pub fn rrset_holder<'c, 'a: 'c>(
    claimants: impl IntoIterator<Item = &'c ClaimFacts<'a>>,
) -> Option<&'c ClaimFacts<'a>> {
    claimants
        .into_iter()
        .filter(|c| c.published && !c.deleting)
        .min_by(|a, b| a.holder_key().cmp(&b.holder_key()))
}

/// Which Scout cluster names are registered to the same account, from the
/// `bindy-scout-sources` ConfigMap (ADR-0025).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScoutRegistry {
    /// One set of cluster names per registered account
    accounts: Vec<BTreeSet<String>>,
}

impl ScoutRegistry {
    /// The registry from the ConfigMap's data: every `<sa>.clusters` key, its
    /// value a comma-separated list of cluster names.
    #[must_use]
    pub fn from_data(data: &BTreeMap<String, String>) -> Self {
        let accounts = data
            .iter()
            .filter(|(key, _)| key.ends_with(SCOUT_SOURCES_CLUSTERS_SUFFIX))
            .map(|(_, value)| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .collect();
        Self { accounts }
    }

    /// Whether cluster names `a` and `b` are registered to one account: the
    /// same edge cluster during a rename.
    #[must_use]
    pub fn same_account(&self, a: &str, b: &str) -> bool {
        self.accounts
            .iter()
            .any(|names| names.contains(a) && names.contains(b))
    }
}

/// Reads the registry of the records namespace `namespace`. A missing
/// ConfigMap is an empty registry.
///
/// # Errors
/// Returns an error if the ConfigMap cannot be read for any reason other than
/// not existing.
pub async fn load_scout_registry(client: &Client, namespace: &str) -> Result<ScoutRegistry> {
    let api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let configmap = api
        .get_opt(SCOUT_SOURCES_CONFIGMAP_NAME)
        .await
        .with_context(|| {
            format!("failed to read ConfigMap {namespace}/{SCOUT_SOURCES_CONFIGMAP_NAME}")
        })?;
    Ok(configmap
        .and_then(|cm| cm.data)
        .map(|data| ScoutRegistry::from_data(&data))
        .unwrap_or_default())
}

/// Records in `all` from another Scout cluster that claim `me`'s RRset,
/// before the registry is consulted.
fn rivals<'c, 'a: 'c>(me: &ClaimFacts<'_>, all: &'c [ClaimFacts<'a>]) -> Vec<&'c ClaimFacts<'a>> {
    all.iter()
        .filter(|other| {
            !other.deleting
                && !other.same_object(me)
                && other.same_rrset(me)
                && other.source_cluster.is_some()
                && other.source_cluster != me.source_cluster
        })
        .collect()
}

/// Whether `me` has rivals from another cluster at all. When it has none the
/// registry need not be read.
#[must_use]
pub fn has_cross_cluster_rivals(me: &ClaimFacts<'_>, all: &[ClaimFacts<'_>]) -> bool {
    me.source_cluster.is_some() && !me.deleting && !rivals(me, all).is_empty()
}

/// The conflict verdict for one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictVerdict<'c, 'a> {
    /// The record may write: it holds the name, nobody does yet, or it has
    /// no rival.
    Clear,
    /// Another cluster's record holds the name; this one must not write.
    HeldBack(&'c ClaimFacts<'a>),
}

/// The verdict for `me` (fresh from the API or the reconcile) against `all`
/// (the kind's store, which may hold a stale copy of `me`), with `registry`
/// telling renames apart (ADR-0027 Decisions 1 and 2).
#[must_use]
pub fn scout_conflict<'c, 'a: 'c>(
    me: &ClaimFacts<'_>,
    all: &'c [ClaimFacts<'a>],
    registry: &ScoutRegistry,
) -> ConflictVerdict<'c, 'a> {
    let Some(my_cluster) = me.source_cluster else {
        return ConflictVerdict::Clear;
    };
    if me.deleting {
        return ConflictVerdict::Clear;
    }
    let contenders: Vec<&ClaimFacts<'a>> = rivals(me, all)
        .into_iter()
        .filter(|other| {
            other
                .source_cluster
                .is_some_and(|theirs| !registry.same_account(my_cluster, theirs))
        })
        .collect();
    if contenders.is_empty() {
        return ConflictVerdict::Clear;
    }
    let holder = rrset_holder(contenders.iter().copied());
    let Some(holder) = holder else {
        return ConflictVerdict::Clear;
    };
    if me.published && !me.deleting && me.holder_key() <= holder.holder_key() {
        return ConflictVerdict::Clear;
    }
    ConflictVerdict::HeldBack(holder)
}

/// The record in `live` that holds `record`'s name when `record` must hold
/// back (ADR-0027), or `None` when it may write.
///
/// `record` is the fresh object being written; `live` is its kind's store.
/// The registry is read only when another cluster's record claims the same
/// name, so the common case costs no API call.
///
/// # Errors
/// Returns an error if the registry ConfigMap cannot be read.
pub async fn held_back_by<K: RecordKind>(
    client: &Client,
    record: &K,
    live: &[std::sync::Arc<K>],
) -> Result<Option<std::sync::Arc<K>>> {
    let Some(me) = claim_facts(record) else {
        return Ok(None);
    };
    let facts: Vec<ClaimFacts<'_>> = live
        .iter()
        .filter_map(|r| claim_facts(r.as_ref()))
        .collect();
    if !has_cross_cluster_rivals(&me, &facts) {
        return Ok(None);
    }
    let registry = load_scout_registry(client, me.namespace).await?;
    let ConflictVerdict::HeldBack(holder) = scout_conflict(&me, &facts, &registry) else {
        return Ok(None);
    };
    Ok(live
        .iter()
        .find(|r| {
            r.meta().namespace.as_deref() == Some(holder.namespace)
                && r.meta().name.as_deref() == Some(holder.name)
        })
        .cloned())
}

/// The status message of a record held back by `holder` (ADR-0027
/// Decision 3).
#[must_use]
pub fn conflict_message<K: RecordKind>(holder: &K, zone_name: &str) -> String {
    let meta = holder.meta();
    let cluster = meta
        .labels
        .as_ref()
        .and_then(|l| l.get(BINDY_SOURCE_CLUSTER_LABEL))
        .map_or("?", String::as_str);
    format!(
        "{} {}/{} of cluster '{cluster}' has published '{}' in {zone_name}; this record writes nothing until it leaves (ADR-0027)",
        K::KIND,
        meta.namespace.as_deref().unwrap_or_default(),
        meta.name.as_deref().unwrap_or_default(),
        holder.spec_name(),
    )
}

#[cfg(test)]
#[path = "rrset_conflict_tests.rs"]
mod rrset_conflict_tests;
