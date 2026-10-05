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
