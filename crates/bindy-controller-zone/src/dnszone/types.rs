// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Types used in DNS zone reconciliation.

// Moved to `bindy-bind9` (ADR-0009 §2, amended 2026-10-05).

/// Information about a duplicate zone conflict.
#[derive(Debug, Clone)]
pub struct DuplicateZoneInfo {
    /// The zone name that has a conflict
    pub zone_name: String,
    /// List of conflicting zones that already claim this zone name
    pub conflicting_zones: Vec<ConflictingZone>,
}

/// Information about a zone that conflicts with the current zone.
#[derive(Debug, Clone)]
pub struct ConflictingZone {
    /// Name of the conflicting DNSZone resource
    pub name: String,
    /// Namespace of the conflicting DNSZone resource
    pub namespace: String,
}

/// Outcome of configuring a zone across a set of BIND9 instances.
///
/// Tracks success in two different units so readiness can be computed in
/// INSTANCE units (comparable with the expected instance counts) while still
/// reporting per-endpoint detail for observability. An instance counts as
/// configured only if ALL of its ready endpoints accepted the zone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ZoneConfigOutcome {
    /// Number of instances where EVERY ready endpoint accepted the zone.
    pub instances_configured: usize,
    /// Total number of endpoints that accepted the zone (including
    /// endpoints where the zone already existed).
    pub endpoints_configured: usize,
    /// Number of endpoints where the zone was NEWLY created by this
    /// reconciliation (it did not exist on that endpoint beforehand).
    ///
    /// A newly created zone holds only the SOA and NS records rendered from
    /// `spec` - every record CR that belongs to it is missing. This is the
    /// signal that a pod (or a whole Deployment) was wiped and came back with
    /// empty storage, and it is what drives the record replay: any value
    /// greater than zero means the zone's records must be pushed again before
    /// the zone can be reported Ready.
    pub zones_created: usize,
}

/// Outcome reason: the zone has no `bind9InstancesFrom` selector, or no
/// instance matches it.
pub const REASON_NO_INSTANCES: &str = "NoInstances";

/// Outcome reason: another, older zone already claims this zone name.
pub const REASON_DUPLICATE_ZONE: &str = "DuplicateZone";

/// Outcome reason: the zone is `Degraded` (an instance or endpoint rejected
/// it, or a record replay is incomplete).
pub const REASON_DEGRADED: &str = "Degraded";

/// Outcome reason: a deleted instance or record could not be cleaned up yet
/// (its DNS data is not confirmed gone), so the pass must run again.
pub const REASON_CLEANUP_PENDING: &str = "CleanupPending";

/// Outcome reason: the zone requests a DNSSEC policy but its keys are still
/// being generated, which no Kubernetes event announces.
pub const REASON_DNSSEC_KEYS_PENDING: &str = "DnssecKeysPending";

/// How one `DNSZone` reconcile ended, decided from the in-memory status the
/// reconcile built (no re-GET), so the controller can pick its `Action`
/// (ADR-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneOutcome {
    /// The zone is served as declared. `next_wake` is set only for an instant
    /// the sidecar announced and no Kubernetes event will (a KSK rollover).
    Converged {
        /// Delay until the next scheduled wake, if any
        next_wake: Option<std::time::Duration>,
    },
    /// Waiting on another object; its watch event resumes the zone.
    Waiting {
        /// Why, e.g. [`REASON_DUPLICATE_ZONE`]
        reason: &'static str,
    },
    /// Failed against BIND9 or bindcar, or waiting on something no event
    /// announces: retried with the per-object backoff.
    Retry {
        /// Why, e.g. [`REASON_DEGRADED`]
        reason: &'static str,
    },
}
