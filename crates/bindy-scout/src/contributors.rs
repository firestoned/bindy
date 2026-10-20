// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! One `ARecord` per DNS name, built from its contributors (ADR-0026).
//!
//! Every source of one Scout that resolves to the same zone and DNS name
//! writes the same record. The record carries, in the
//! [`ANNOTATION_CONTRIBUTORS`] annotation, a map from each contributing source
//! to the addresses and TTL it resolved. `spec.ipv4Addresses` is always the
//! union of the map and `spec.ttl` the lowest TTL any contributor sets, so a
//! record is never written with a partial set.
//!
//! A source changes only its own entry, with a read, a rebuild from the whole
//! map, and a write conditional on the `resourceVersion` it read, retried on
//! 409 ([`contribute`], [`withdraw`]). The pure planning steps
//! ([`plan_contribute`], [`plan_withdraw`]) decide what that write is.

use crate::crd::{ARecord, ARecordSpec};
use crate::scout::{
    nul_joined_sha256_hex, LABEL_MANAGED_BY, LABEL_MANAGED_BY_SCOUT, LABEL_SOURCE_CLUSTER,
    LABEL_ZONE, SCOUT_FIELD_MANAGER,
};
use anyhow::{anyhow, Context as _, Result};
use bindy_controller_sdk::kube_errors;
use kube::api::{DeleteParams, ObjectMeta, PostParams, Preconditions};
use kube::{Api, ResourceExt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use tracing::{debug, info, warn};

/// Annotation holding the contributor map, as JSON (ADR-0026, Decision 2).
pub(crate) const ANNOTATION_CONTRIBUTORS: &str = "bindy.firestoned.io/scout-contributors";

/// Prefix of the label each contributor sets on the records it contributes
/// to, so a source finds them with one label selector.
pub(crate) const CONTRIBUTOR_LABEL_PREFIX: &str = "contributor.scout.bindy.firestoned.io/";

/// Value of a contributor label; only the key carries information.
pub(crate) const CONTRIBUTOR_LABEL_VALUE: &str = "true";

/// Digest bytes in a contributor label's hash (16 hex characters).
const CONTRIBUTOR_HASH_BYTES: usize = 8;

/// How many times a conditional write is rebuilt after losing a race (409)
/// before the reconcile fails and is retried by the error policy.
pub(crate) const MAX_WRITE_ATTEMPTS: usize = 5;

/// What one source contributes to a DNS name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Contribution {
    /// The IPv4 addresses the source resolved
    pub ips: Vec<String>,
    /// The source's TTL override, if it sets one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<i32>,
}

/// The contributor map: contributor key (see [`contributor_key`]) to its
/// contribution. Ordered, so it serializes the same way every time.
pub(crate) type Contributors = BTreeMap<String, Contribution>;

/// The key a source is recorded under: `kind/namespace/name`.
#[must_use]
pub(crate) fn contributor_key(kind: &str, namespace: &str, name: &str) -> String {
    format!("{kind}/{namespace}/{name}")
}

/// The label a source sets on every record it contributes to:
/// [`CONTRIBUTOR_LABEL_PREFIX`] and a hash of its kind, namespace and name.
///
/// The cluster is left out, so a Scout renamed with `--cluster-name` still
/// finds the records its sources contributed to under the old name.
#[must_use]
pub(crate) fn contributor_label(kind: &str, namespace: &str, name: &str) -> String {
    format!(
        "{CONTRIBUTOR_LABEL_PREFIX}{}",
        nul_joined_sha256_hex(&[kind, namespace, name], CONTRIBUTOR_HASH_BYTES)
    )
}

/// [`contributor_label`] for a contributor key, or `None` when the key is not
/// `kind/namespace/name`.
fn contributor_label_of_key(key: &str) -> Option<String> {
    let mut parts = key.splitn(3, '/');
    let (kind, namespace, name) = (parts.next()?, parts.next()?, parts.next()?);
    Some(contributor_label(kind, namespace, name))
}

/// Selects this cluster's records that the contributor with `label`
/// contributes to, in any zone.
#[must_use]
pub(crate) fn name_record_selector(cluster: &str, label: &str) -> String {
    format!("{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}={cluster},{label}")
}

/// Selects the records another cluster name left in `zone` that the
/// contributor with `label` contributes to (a Scout renamed with
/// `--cluster-name`). Scoped to the zone for the same reason as the
/// per-source stale selector (#474).
#[must_use]
pub(crate) fn stale_name_record_selector(current_cluster: &str, zone: &str, label: &str) -> String {
    format!(
        "{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}!={current_cluster},\
         {LABEL_ZONE}={zone},{label}"
    )
}

/// The identity of one per-name record.
pub(crate) struct NameRecord<'a> {
    /// Kubernetes name (see `dns_name_record_cr_name`)
    pub cr_name: &'a str,
    /// Namespace the record lives in on the bindy cluster
    pub target_namespace: &'a str,
    /// Scout's cluster name
    pub cluster: &'a str,
    /// The DNS zone
    pub zone: &'a str,
    /// The record name within the zone (`@` for the apex)
    pub dns_name: &'a str,
}

/// A per-name record with no contributors yet: the base labels
/// (`managed-by`, `source-cluster`, `zone`) and the DNS name.
#[must_use]
pub(crate) fn new_name_record(id: &NameRecord<'_>) -> ARecord {
    let labels: BTreeMap<String, String> = [
        (LABEL_MANAGED_BY, LABEL_MANAGED_BY_SCOUT),
        (LABEL_SOURCE_CLUSTER, id.cluster),
        (LABEL_ZONE, id.zone),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    ARecord {
        metadata: ObjectMeta {
            name: Some(id.cr_name.to_string()),
            namespace: Some(id.target_namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: ARecordSpec {
            name: id.dns_name.to_string(),
            ipv4_addresses: Vec::new(),
            ttl: None,
        },
        status: None,
    }
}

/// Reads a record's contributor map. A record without the annotation has
/// none.
///
/// # Errors
/// Returns an error naming the record when the annotation is not a valid
/// contributor map: Scout will not rebuild a record from a map it cannot
/// read, since that would drop the other contributors.
pub(crate) fn contributors_of(record: &ARecord) -> Result<Contributors> {
    let Some(raw) = record.annotations().get(ANNOTATION_CONTRIBUTORS) else {
        return Ok(Contributors::new());
    };
    serde_json::from_str(raw).with_context(|| {
        format!(
            "ARecord {} has a malformed {ANNOTATION_CONTRIBUTORS} annotation; fix or remove it",
            record.name_any()
        )
    })
}

/// The union of every contributor's addresses, each once, sorted
/// numerically (ADR-0026, Decision 2).
#[must_use]
pub(crate) fn derived_ips(contributors: &Contributors) -> Vec<String> {
    let mut ips: Vec<String> = contributors
        .values()
        .flat_map(|c| c.ips.iter().cloned())
        .collect();
    ips.sort_by_key(|ip| (ip.parse::<Ipv4Addr>().ok(), ip.clone()));
    ips.dedup();
    ips
}

/// The lowest TTL any contributor sets, or `None` (the zone's TTL) when none
/// does (ADR-0026, Decision 2).
#[must_use]
pub(crate) fn derived_ttl(contributors: &Contributors) -> Option<i32> {
    contributors.values().filter_map(|c| c.ttl).min()
}

/// Whether the contributors set more than one distinct TTL.
#[must_use]
pub(crate) fn ttls_disagree(contributors: &Contributors) -> bool {
    let mut ttls = contributors.values().filter_map(|c| c.ttl);
    let Some(first) = ttls.next() else {
        return false;
    };
    ttls.any(|ttl| ttl != first)
}

/// Writes `contributors` onto `record`: the annotation, one contributor label
/// per contributor (labels of contributors no longer in the map are
/// removed, every other label is kept), and the derived spec.
///
/// # Errors
/// Returns an error if the map cannot be serialized.
pub(crate) fn render(record: &mut ARecord, contributors: &Contributors) -> Result<()> {
    let raw = serde_json::to_string(contributors).context("serializing the contributor map")?;
    record
        .annotations_mut()
        .insert(ANNOTATION_CONTRIBUTORS.to_string(), raw);
    let labels = record.labels_mut();
    labels.retain(|k, _| !k.starts_with(CONTRIBUTOR_LABEL_PREFIX));
    for label in contributors
        .keys()
        .filter_map(|k| contributor_label_of_key(k))
    {
        labels.insert(label, CONTRIBUTOR_LABEL_VALUE.to_string());
    }
    record.spec.ipv4_addresses = derived_ips(contributors);
    record.spec.ttl = derived_ttl(contributors);
    Ok(())
}

/// The one write a contribution or withdrawal needs.
#[derive(Debug)]
pub(crate) enum Plan {
    /// The record already holds the result: write nothing.
    Nothing,
    /// Create the record (it did not exist).
    Create(ARecord),
    /// Replace the record, conditional on the `resourceVersion` it carries.
    Replace(ARecord),
    /// Delete the record (its last contributor left), conditional on this
    /// `resourceVersion`.
    Delete {
        /// The version read; the delete fails with 409 if it changed
        resource_version: Option<String>,
    },
}

/// Plans setting `key`'s entry to `contribution` on the record `current`
/// (as read; `None` when it does not exist yet), whose identity `template`
/// gives (see [`new_name_record`]).
///
/// The spec is always rebuilt from the whole map, so a hand-edited spec is
/// repaired. Nothing is written when the record already holds the result.
///
/// # Errors
/// Returns an error when `current` has a malformed contributor map.
pub(crate) fn plan_contribute(
    current: Option<&ARecord>,
    template: &ARecord,
    key: &str,
    contribution: &Contribution,
) -> Result<Plan> {
    let Some(current) = current else {
        let mut record = template.clone();
        render(
            &mut record,
            &Contributors::from([(key.to_string(), contribution.clone())]),
        )?;
        return Ok(Plan::Create(record));
    };
    let mut contributors = contributors_of(current)?;
    contributors.insert(key.to_string(), contribution.clone());
    let mut record = current.clone();
    record.spec.name.clone_from(&template.spec.name);
    render(&mut record, &contributors)?;
    if same_content(&record, current) {
        return Ok(Plan::Nothing);
    }
    Ok(Plan::Replace(record))
}

/// Plans removing `key`'s entry from the record `current` (as read; `None`
/// when it does not exist). The last contributor leaving deletes the record.
///
/// # Errors
/// Returns an error when `current` has a malformed contributor map.
pub(crate) fn plan_withdraw(current: Option<&ARecord>, key: &str) -> Result<Plan> {
    let Some(current) = current else {
        return Ok(Plan::Nothing);
    };
    let mut contributors = contributors_of(current)?;
    if contributors.remove(key).is_none() {
        return Ok(Plan::Nothing);
    }
    if contributors.is_empty() {
        return Ok(Plan::Delete {
            resource_version: current.resource_version(),
        });
    }
    let mut record = current.clone();
    render(&mut record, &contributors)?;
    Ok(Plan::Replace(record))
}

/// Whether two versions of a record have the same labels, annotations and
/// spec (what Scout writes).
fn same_content(a: &ARecord, b: &ARecord) -> bool {
    a.metadata.labels == b.metadata.labels
        && a.metadata.annotations == b.metadata.annotations
        && serde_json::to_value(&a.spec).ok() == serde_json::to_value(&b.spec).ok()
}

/// Parameters for the conditional writes.
fn post_params() -> PostParams {
    PostParams {
        dry_run: false,
        field_manager: Some(SCOUT_FIELD_MANAGER.to_string()),
    }
}

/// Sets `key`'s entry on the per-name record `template` identifies, creating
/// the record if needed (ADR-0026, Decision 3).
///
/// Reads the record, plans the write ([`plan_contribute`]) and makes it
/// conditional on the version read; a lost race (409) starts over, up to
/// [`MAX_WRITE_ATTEMPTS`] times.
///
/// # Returns
/// The record as the API server now holds it (for the served check).
///
/// # Errors
/// Returns an error when a read or write fails for any reason other than a
/// lost race, when the record's contributor map is malformed, or when every
/// attempt lost a race.
pub(crate) async fn contribute(
    api: &Api<ARecord>,
    template: &ARecord,
    key: &str,
    contribution: &Contribution,
) -> Result<ARecord> {
    let cr_name = template.name_any();
    for attempt in 1..=MAX_WRITE_ATTEMPTS {
        let current = api
            .get_opt(&cr_name)
            .await
            .with_context(|| format!("failed to read ARecord {cr_name}"))?;
        let before = current.as_ref().map(contributors_of).transpose()?;
        let result = match plan_contribute(current.as_ref(), template, key, contribution)? {
            Plan::Nothing => {
                return current.ok_or_else(|| anyhow!("ARecord {cr_name} vanished"));
            }
            Plan::Create(record) => api.create(&post_params(), &record).await,
            Plan::Replace(record) => api.replace(&cr_name, &post_params(), &record).await,
            Plan::Delete { .. } => {
                return Err(anyhow!("a contribution never deletes ARecord {cr_name}"))
            }
        };
        match result {
            Ok(written) => {
                log_contribution(&written, before.as_ref(), key, contribution);
                return Ok(written);
            }
            Err(e) if kube_errors::is_conflict(&e) => {
                debug!(arecord = %cr_name, attempt, "ARecord changed since it was read, rebuilding the write");
            }
            Err(e) => return Err(e).with_context(|| format!("failed to write ARecord {cr_name}")),
        }
    }
    Err(anyhow!(
        "ARecord {cr_name} kept changing while this source wrote it ({MAX_WRITE_ATTEMPTS} attempts)"
    ))
}

/// Logs what a contribution changed: a source joining a name others already
/// publish (ADR-0026, Decision 6) and contributors disagreeing on the TTL
/// (Decision 2). Called only after a write, so each change is logged once.
fn log_contribution(
    written: &ARecord,
    before: Option<&Contributors>,
    key: &str,
    contribution: &Contribution,
) {
    let record = written.name_any();
    let dns_name = &written.spec.name;
    if let Some(before) = before.filter(|b| !b.is_empty() && !b.contains_key(key)) {
        let others: Vec<&String> = before.keys().collect();
        info!(arecord = %record, dns_name = %dns_name, contributor = %key, others = ?others, "Source joins a DNS name other sources already publish; the record now holds every source's addresses");
    }
    if let Ok(after) = contributors_of(written) {
        if ttls_disagree(&after) {
            warn!(arecord = %record, dns_name = %dns_name, ttl = ?derived_ttl(&after), "Sources publishing this DNS name set different TTLs; publishing the lowest");
        }
    }
    info!(arecord = %record, dns_name = %dns_name, contributor = %key, ips = ?contribution.ips, addresses = ?written.spec.ipv4_addresses, "ARecord written");
}

/// What [`withdraw`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Withdrawal {
    /// The record or the entry was not there.
    Absent,
    /// The entry was removed; other contributors remain.
    Updated,
    /// The entry was the last one; the record was deleted.
    Deleted,
    /// The API server refused the write (HTTP 403 or 422): under ADR-0025
    /// the record belongs to a cluster name this Scout is not registered for.
    Refused,
}

/// Removes `key`'s entry from the record `cr_name`, deleting the record when
/// it was the last (ADR-0026, Decision 4). Conditional on the version read,
/// retried on a lost race like [`contribute`].
///
/// # Errors
/// Returns an error when a read or write fails for any reason other than a
/// lost race or a refusal, when the record's contributor map is malformed,
/// or when every attempt lost a race.
pub(crate) async fn withdraw(api: &Api<ARecord>, cr_name: &str, key: &str) -> Result<Withdrawal> {
    for attempt in 1..=MAX_WRITE_ATTEMPTS {
        let current = api
            .get_opt(cr_name)
            .await
            .with_context(|| format!("failed to read ARecord {cr_name}"))?;
        let (result, done) = match plan_withdraw(current.as_ref(), key)? {
            Plan::Nothing => return Ok(Withdrawal::Absent),
            Plan::Replace(record) => (
                api.replace(cr_name, &post_params(), &record)
                    .await
                    .map(drop),
                Withdrawal::Updated,
            ),
            Plan::Delete { resource_version } => {
                let params = DeleteParams {
                    preconditions: Some(Preconditions {
                        resource_version,
                        uid: None,
                    }),
                    ..DeleteParams::default()
                };
                (
                    api.delete(cr_name, &params).await.map(drop),
                    Withdrawal::Deleted,
                )
            }
            Plan::Create(_) => return Err(anyhow!("a withdrawal never creates ARecord {cr_name}")),
        };
        match result {
            Ok(()) => {
                info!(arecord = %cr_name, contributor = %key, outcome = ?done, "Source withdrew from ARecord");
                return Ok(done);
            }
            Err(e) if kube_errors::is_not_found(&e) => return Ok(Withdrawal::Absent),
            Err(e) if kube_errors::is_conflict(&e) => {
                debug!(arecord = %cr_name, attempt, "ARecord changed since it was read, rebuilding the withdrawal");
            }
            Err(e) if kube_errors::is_refused(&e) => return Ok(Withdrawal::Refused),
            Err(e) => {
                return Err(e).with_context(|| format!("failed to withdraw from ARecord {cr_name}"))
            }
        }
    }
    Err(anyhow!(
        "ARecord {cr_name} kept changing while this source withdrew from it ({MAX_WRITE_ATTEMPTS} attempts)"
    ))
}

#[cfg(test)]
#[path = "contributors_tests.rs"]
mod contributors_tests;
