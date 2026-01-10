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
