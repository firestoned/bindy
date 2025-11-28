// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Custom Resource Definitions (CRDs) for DNS management.
//!
//! This module defines all Kubernetes Custom Resource Definitions used by Bindy
//! to manage BIND9 DNS infrastructure declaratively.
//!
//! # Resource Types
//!
//! ## Infrastructure
//!
//! - [`Bind9Instance`] - Represents a BIND9 DNS server deployment
//!
//! ## DNS Zones
//!
//! - [`DNSZone`] - Defines DNS zones with SOA records and instance targeting
//!
//! ## DNS Records
//!
//! - [`ARecord`] - IPv4 address records
//! - [`AAAARecord`] - IPv6 address records  
//! - [`CNAMERecord`] - Canonical name (alias) records
//! - [`MXRecord`] - Mail exchange records
//! - [`TXTRecord`] - Text records (SPF, DKIM, DMARC, etc.)
//! - [`NSRecord`] - Nameserver delegation records
//! - [`SRVRecord`] - Service location records
//! - [`CAARecord`] - Certificate authority authorization records
//!
//! # Example: Creating a DNS Zone
//!
//! ```rust,no_run
//! use bindy::crd::{DNSZoneSpec, SOARecord, LabelSelector};
//! use std::collections::BTreeMap;
//!
//! let mut match_labels = BTreeMap::new();
//! match_labels.insert("dns-role".to_string(), "primary".to_string());
//!
//! let soa = SOARecord {
//!     primary_ns: "ns1.example.com.".to_string(),
//!     admin_email: "admin@example.com".to_string(),
//!     serial: 2024010101,
//!     refresh: 3600,
//!     retry: 600,
//!     expire: 604800,
//!     negative_ttl: 86400,
//! };
//!
//! let spec = DNSZoneSpec {
//!     zone_name: "example.com".to_string(),
//!     zone_type: Some("primary".to_string()),
//!     instance_selector: LabelSelector {
//!         match_labels: Some(match_labels),
//!         match_expressions: None,
//!     },
//!     soa_record: Some(soa),
//!     secondary_config: None,
//!     ttl: Some(3600),
//! };
//! ```
//!
//! # Example: Creating DNS Records
//!
//! ```rust,no_run
//! use bindy::crd::{ARecordSpec, MXRecordSpec};
//!
//! // A Record for www.example.com
//! let a_record = ARecordSpec {
//!     zone: "example-com".to_string(),
//!     name: "www".to_string(),
//!     ipv4_address: "192.0.2.1".to_string(),
//!     ttl: Some(300),
//! };
//!
//! // MX Record for mail routing
//! let mx_record = MXRecordSpec {
//!     zone: "example-com".to_string(),
//!     name: "@".to_string(),
//!     priority: 10,
//!     mail_server: "mail.example.com.".to_string(),
//!     ttl: Some(3600),
//! };
//! ```

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Label selector to match resources
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabelSelector {
    /// matchLabels is a map of {key,value} pairs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_labels: Option<BTreeMap<String, String>>,
    /// matchExpressions is a list of label selector requirements
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_expressions: Option<Vec<LabelSelectorRequirement>>,
}

/// Label selector requirement
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LabelSelectorRequirement {
    pub key: String,
    pub operator: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
}

/// SOA Record specification
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SOARecord {
    pub primary_ns: String,
    pub admin_email: String,
    pub serial: i64,
    pub refresh: i32,
    pub retry: i32,
    pub expire: i32,
    pub negative_ttl: i32,
}

/// Condition status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct Condition {
    pub r#type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_transition_time: Option<String>,
}

/// DNSZone status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct DNSZoneStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_count: Option<i32>,
}

/// Secondary Zone configuration
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryZoneConfig {
    /// Primary server addresses for zone transfer
    pub primary_servers: Vec<String>,
    /// Optional TSIG key for authenticated transfers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tsig_key: Option<String>,
}

/// DNSZone CRD
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "DNSZone",
    namespaced
)]
#[kube(status = "DNSZoneStatus")]
#[serde(rename_all = "camelCase")]
pub struct DNSZoneSpec {
    pub zone_name: String,
    /// Zone type: "primary" or "secondary". Defaults to "primary"
    #[serde(default)]
    pub zone_type: Option<String>,
    pub instance_selector: LabelSelector,
    /// SOA record - required for primary zones, optional for secondary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soa_record: Option<SOARecord>,
    /// Secondary zone configuration - required for secondary zones
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_config: Option<SecondaryZoneConfig>,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// ARecord specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "ARecord",
    namespaced
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct ARecordSpec {
    pub zone: String,
    pub name: String,
    pub ipv4_address: String,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// AAAARecord specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "AAAARecord",
    namespaced
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct AAAARecordSpec {
    pub zone: String,
    pub name: String,
    pub ipv6_address: String,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// TXTRecord specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "TXTRecord",
    namespaced
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct TXTRecordSpec {
    pub zone: String,
    pub name: String,
    pub text: Vec<String>,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// CNAMERecord specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "CNAMERecord",
    namespaced
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct CNAMERecordSpec {
    pub zone: String,
    pub name: String,
    pub target: String,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// MXRecord specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "MXRecord",
    namespaced
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct MXRecordSpec {
    pub zone: String,
    pub name: String,
    pub priority: i32,
    pub mail_server: String,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// NSRecord specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "NSRecord",
    namespaced
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct NSRecordSpec {
    pub zone: String,
    pub name: String,
    pub nameserver: String,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// SRVRecord specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "SRVRecord",
    namespaced
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct SRVRecordSpec {
    pub zone: String,
    pub name: String,
    pub priority: i32,
    pub weight: i32,
    pub port: i32,
    pub target: String,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// CAARecord specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "CAARecord",
    namespaced
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct CAARecordSpec {
    pub zone: String,
    pub name: String,
    pub flags: i32,
    pub tag: String,
    pub value: String,
    #[serde(default)]
    pub ttl: Option<i32>,
}

/// Generic record status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct RecordStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}

/// Bind9Config options
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Bind9Config {
    #[serde(default)]
    pub recursion: Option<bool>,
    #[serde(default)]
    pub allow_query: Option<Vec<String>>,
    #[serde(default)]
    pub allow_transfer: Option<Vec<String>>,
    #[serde(default)]
    pub dnssec: Option<DNSSECConfig>,
    #[serde(default)]
    pub forwarders: Option<Vec<String>>,
    #[serde(default)]
    pub listen_on: Option<Vec<String>>,
    #[serde(default)]
    pub listen_on_v6: Option<Vec<String>>,
}

/// DNSSEC configuration
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DNSSECConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub validation: Option<bool>,
}

/// Bind9Instance specification
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "dns.firestoned.io",
    version = "v1alpha1",
    kind = "Bind9Instance",
    namespaced
)]
#[kube(status = "Bind9InstanceStatus")]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceSpec {
    #[serde(default)]
    pub replicas: Option<i32>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub config: Option<Bind9Config>,
}

/// Bind9Instance status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct Bind9InstanceStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_replicas: Option<i32>,
}
