// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Whether an RRset is still claimed by another live record.
//!
//! Deleting a record from BIND9 removes its whole RRset (owner name and type).
//! When two record resources declare the same name and type in the same zone,
//! deleting one of them must leave the RRset in place: the other still wants
//! it, and since ADR-0021 every write replaces the whole RRset, so the RRset
//! already holds the survivor's values. With no periodic resync (ADR-0016)
//! nothing would re-push it.
//!
//! The zone controller's stale-record cleanup (bug-211) and the record
//! finalizer (ADR-0022 Decision 5) both ask this question; [`claims_rrset`]
//! is the one place that answers it.

use crate::context::RecordKind;
use bindy_api::crd::ZoneReference;
use std::sync::Arc;

/// One record's claim on an RRset: which object declares which DNS name, of
/// which kind, in which zone.
///
/// The zone is the `DNSZone` object (namespace and name, as in a record's
/// `status.zoneRef`), not the zone's domain name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RrsetClaim<'a> {
    /// The record kind, e.g. `ARecord`; together with the name it picks the
    /// RRset's type
    pub kind: &'a str,
    /// Namespace of the record resource
    pub namespace: &'a str,
    /// Name of the record resource
    pub name: &'a str,
    /// Namespace of the `DNSZone` the record belongs to
    pub zone_namespace: &'a str,
    /// Name of the `DNSZone` the record belongs to
    pub zone_name: &'a str,
    /// The DNS owner name the record declares (e.g. `www`, `@`)
    pub dns_name: &'a str,
    /// Whether the record resource is being deleted
    pub deleting: bool,
}

/// Whether `candidate` still claims the RRset `gone` published.
///
/// It does when it is the same record kind, in the same zone, declares the
/// same DNS owner name (compared case-insensitively, as DNS does), is a
/// different object, and is not itself being deleted.
///
/// # Arguments
///
/// * `candidate` - A record that may still want the RRset
/// * `gone` - The record whose RRset is about to be deleted
///
/// # Returns
///
/// `true` when deleting the RRset would remove data `candidate` declares.
#[must_use]
pub fn claims_rrset(candidate: &RrsetClaim<'_>, gone: &RrsetClaim<'_>) -> bool {
    if candidate.deleting || candidate.kind != gone.kind {
        return false;
    }
    if candidate.zone_namespace != gone.zone_namespace || candidate.zone_name != gone.zone_name {
        return false;
    }
    if candidate.namespace == gone.namespace && candidate.name == gone.name {
        return false;
    }
    candidate.dns_name.eq_ignore_ascii_case(gone.dns_name)
}

/// The claim a record of kind `K` makes on the RRset named `dns_name` in the
/// zone `zone_ref`.
///
/// # Arguments
///
/// * `record` - The record resource
/// * `zone_ref` - The `DNSZone` the RRset belongs to
/// * `dns_name` - The DNS owner name of the RRset
///
/// # Returns
///
/// The claim, carrying the record's kind, namespace, name and deletion state.
#[must_use]
pub fn claim_of<'a, K: RecordKind>(
    record: &'a K,
    zone_ref: &'a ZoneReference,
    dns_name: &'a str,
) -> RrsetClaim<'a> {
    let meta = record.meta();
    RrsetClaim {
        kind: K::KIND,
        namespace: meta.namespace.as_deref().unwrap_or_default(),
        name: meta.name.as_deref().unwrap_or_default(),
        zone_namespace: &zone_ref.namespace,
        zone_name: &zone_ref.name,
        dns_name,
        deleting: meta.deletion_timestamp.is_some(),
    }
}

/// The first record in `live` that still claims the RRset `gone` published.
///
/// Each candidate's claim is its `spec.name` in the zone its
/// `status.zoneRef` names; a record no zone has tagged claims nothing.
///
/// # Arguments
///
/// * `gone` - The record whose RRset is about to be deleted
/// * `live` - The records of the same kind, e.g. the kind's reflector store
///
/// # Returns
///
/// The claimant, or `None` when the RRset is no longer wanted.
#[must_use]
pub fn live_claimant<'a, K: RecordKind>(
    gone: &RrsetClaim<'_>,
    live: &'a [Arc<K>],
) -> Option<&'a Arc<K>> {
    live.iter().find(|record| {
        record
            .record_status()
            .and_then(|status| status.zone_ref.as_ref())
            .is_some_and(|zone_ref| {
                claims_rrset(
                    &claim_of(record.as_ref(), zone_ref, record.spec_name()),
                    gone,
                )
            })
    })
}

#[cfg(test)]
#[path = "rrset_claim_tests.rs"]
mod rrset_claim_tests;
