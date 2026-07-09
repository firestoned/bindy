// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Types used in DNS zone reconciliation.

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
    /// Instance names where this zone is configured
    pub instance_names: Vec<String>,
}

/// Information about a BIND9 pod discovered during reconciliation.
#[derive(Debug, Clone)]
pub struct PodInfo {
    /// Pod name
    pub name: String,
    /// Pod IP address
    pub ip: String,
    /// Name of the Bind9Instance this pod belongs to
    pub instance_name: String,
    /// Namespace of the pod
    pub namespace: String,
}

/// Endpoint address (IP + port) for connecting to BIND9 API.
#[derive(Debug, Clone)]
pub struct EndpointAddress {
    /// IP address of the pod
    pub ip: String,
    /// Container port number
    pub port: i32,
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
}
