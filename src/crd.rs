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
//! use bindy::crd::{DNSZoneSpec, SOARecord};
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
//!     cluster_ref: Some("my-dns-cluster".to_string()),
//!     cluster_provider_ref: None,
//!     soa_record: soa,
//!     ttl: Some(3600),
//!     name_server_ips: None,
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
//!     zone_ref: "example-com".to_string(),
//!     name: "www".to_string(),
//!     ipv4_address: "192.0.2.1".to_string(),
//!     ttl: Some(300),
//! };
//!
//! // MX Record for mail routing
//! let mx_record = MXRecordSpec {
//!     zone_ref: "example-com".to_string(),
//!     name: "@".to_string(),
//!     priority: 10,
//!     mail_server: "mail.example.com.".to_string(),
//!     ttl: Some(3600),
//! };
//! ```

use k8s_openapi::api::core::v1::{EnvVar, ServiceSpec, Volume, VolumeMount};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Label selector to match Kubernetes resources.
///
/// A label selector is a label query over a set of resources. The result of matchLabels and
/// matchExpressions are `ANDed`. An empty label selector matches all objects.
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabelSelector {
    /// Map of {key,value} pairs. A single {key,value} in the matchLabels map is equivalent
    /// to an element of matchExpressions, whose key field is "key", the operator is "In",
    /// and the values array contains only "value". All requirements must be satisfied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_labels: Option<BTreeMap<String, String>>,

    /// List of label selector requirements. All requirements must be satisfied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_expressions: Option<Vec<LabelSelectorRequirement>>,
}

/// A label selector requirement is a selector that contains values, a key, and an operator
/// that relates the key and values.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabelSelectorRequirement {
    /// The label key that the selector applies to.
    pub key: String,

    /// Operator represents a key's relationship to a set of values.
    /// Valid operators are In, `NotIn`, Exists and `DoesNotExist`.
    pub operator: String,

    /// An array of string values. If the operator is In or `NotIn`,
    /// the values array must be non-empty. If the operator is Exists or `DoesNotExist`,
    /// the values array must be empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
}

/// SOA (Start of Authority) Record specification.
///
/// The SOA record defines authoritative information about a DNS zone, including
/// the primary nameserver, responsible party's email, and timing parameters for
/// zone transfers and caching.
///
/// # Example
///
/// ```rust
/// use bindy::crd::SOARecord;
///
/// let soa = SOARecord {
///     primary_ns: "ns1.example.com.".to_string(),
///     admin_email: "admin.example.com.".to_string(), // Note: @ replaced with .
///     serial: 2024010101,
///     refresh: 3600,   // Check for updates every hour
///     retry: 600,      // Retry after 10 minutes on failure
///     expire: 604800,  // Expire after 1 week
///     negative_ttl: 86400, // Cache negative responses for 1 day
/// };
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SOARecord {
    /// Primary nameserver for this zone (must be a FQDN ending with .).
    ///
    /// Example: `ns1.example.com.`
    #[schemars(regex(
        pattern = r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*\.$"
    ))]
    pub primary_ns: String,

    /// Email address of the zone administrator (@ replaced with ., must end with .).
    ///
    /// Example: `admin.example.com.` for admin@example.com
    #[schemars(regex(
        pattern = r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*\.$"
    ))]
    pub admin_email: String,

    /// Serial number for this zone. Typically in YYYYMMDDNN format.
    /// Secondaries use this to determine if they need to update.
    ///
    /// Must be a 32-bit unsigned integer (0 to 4294967295).
    /// The field is i64 to accommodate the full u32 range.
    #[schemars(range(min = 0, max = 4_294_967_295_i64))]
    pub serial: i64,

    /// Refresh interval in seconds. How often secondaries should check for updates.
    ///
    /// Typical values: 3600-86400 (1 hour to 1 day).
    #[schemars(range(min = 1, max = 2_147_483_647))]
    pub refresh: i32,

    /// Retry interval in seconds. How long to wait before retrying a failed refresh.
    ///
    /// Should be less than refresh. Typical values: 600-7200 (10 minutes to 2 hours).
    #[schemars(range(min = 1, max = 2_147_483_647))]
    pub retry: i32,

    /// Expire time in seconds. After this time, secondaries stop serving the zone
    /// if they can't contact the primary.
    ///
    /// Should be much larger than refresh+retry. Typical values: 604800-2419200 (1-4 weeks).
    #[schemars(range(min = 1, max = 2_147_483_647))]
    pub expire: i32,

    /// Negative caching TTL in seconds. How long to cache NXDOMAIN responses.
    ///
    /// Typical values: 300-86400 (5 minutes to 1 day).
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub negative_ttl: i32,
}

/// Condition represents an observation of a resource's current state.
///
/// Conditions are used in status subresources to communicate the state of
/// a resource to users and controllers.
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// Type of condition. Common types include: Ready, Available, Progressing, Degraded, Failed.
    pub r#type: String,

    /// Status of the condition: True, False, or Unknown.
    pub status: String,

    /// Brief CamelCase reason for the condition's last transition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Human-readable message indicating details about the transition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Last time the condition transitioned from one status to another (RFC3339 format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_transition_time: Option<String>,
}

/// `DNSZone` status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DNSZoneStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_count: Option<i32>,
    /// IP addresses of secondary servers configured for zone transfers.
    /// Used to detect when secondary IPs change and zones need updating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_ips: Option<Vec<String>>,
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

/// `DNSZone` defines a DNS zone to be managed by BIND9.
///
/// A `DNSZone` represents an authoritative DNS zone (e.g., example.com) that will be
/// served by a BIND9 cluster. The zone includes SOA record information and will be
/// synchronized to all instances in the referenced cluster via AXFR/IXFR.
///
/// `DNSZones` can reference either:
/// - A namespace-scoped `Bind9Cluster` (using `clusterRef`)
/// - A cluster-scoped `ClusterBind9Provider` (using `clusterProviderRef`)
///
/// Exactly one of `clusterRef` or `clusterProviderRef` must be specified.
///
/// # Example: Namespace-scoped Cluster
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: DNSZone
/// metadata:
///   name: example-com
///   namespace: dev-team-alpha
/// spec:
///   zoneName: example.com
///   clusterRef: dev-team-dns  # References Bind9Cluster in same namespace
///   soaRecord:
///     primaryNs: ns1.example.com.
///     adminEmail: admin.example.com.
///     serial: 2024010101
///     refresh: 3600
///     retry: 600
///     expire: 604800
///     negativeTtl: 86400
///   ttl: 3600
/// ```
///
/// # Example: Cluster-scoped Global Cluster
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: DNSZone
/// metadata:
///   name: production-example-com
///   namespace: production
/// spec:
///   zoneName: example.com
///   clusterProviderRef: shared-production-dns  # References ClusterBind9Provider (cluster-scoped)
///   soaRecord:
///     primaryNs: ns1.example.com.
///     adminEmail: admin.example.com.
///     serial: 2024010101
///     refresh: 3600
///     retry: 600
///     expire: 604800
///     negativeTtl: 86400
///   ttl: 3600
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "DNSZone",
    namespaced,
    shortname = "zone",
    shortname = "zones",
    shortname = "dz",
    shortname = "dzs",
    doc = "DNSZone represents an authoritative DNS zone managed by BIND9. Each DNSZone defines a zone (e.g., example.com) with SOA record parameters. Can reference either a namespace-scoped Bind9Cluster or cluster-scoped ClusterBind9Provider.",
    printcolumn = r#"{"name":"Zone","type":"string","jsonPath":".spec.zoneName"}"#,
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".spec.clusterRef"}"#,
    printcolumn = r#"{"name":"Provider","type":"string","jsonPath":".spec.clusterProviderRef"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "DNSZoneStatus")]
#[serde(rename_all = "camelCase")]
pub struct DNSZoneSpec {
    /// DNS zone name (e.g., "example.com").
    ///
    /// Must be a valid DNS zone name. Can be a domain or subdomain.
    /// Examples: "example.com", "internal.example.com", "10.in-addr.arpa"
    #[schemars(regex(
        pattern = r"^([a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)*[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$"
    ))]
    pub zone_name: String,

    /// Reference to a namespace-scoped `Bind9Cluster` in the same namespace.
    ///
    /// Must match the name of a `Bind9Cluster` resource in the same namespace.
    /// The zone will be added to all instances in this cluster.
    ///
    /// Either `clusterRef` or `clusterProviderRef` must be specified (not both).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_ref: Option<String>,

    /// Reference to a cluster-scoped `ClusterBind9Provider`.
    ///
    /// Must match the name of a `ClusterBind9Provider` resource (cluster-scoped).
    /// The zone will be added to all instances in this provider.
    ///
    /// Either `clusterRef` or `clusterProviderRef` must be specified (not both).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_provider_ref: Option<String>,

    /// SOA (Start of Authority) record - defines zone authority and refresh parameters.
    ///
    /// The SOA record is required for all authoritative zones and contains
    /// timing information for zone transfers and caching.
    pub soa_record: SOARecord,

    /// Default TTL (Time To Live) for records in this zone, in seconds.
    ///
    /// If not specified, individual records must specify their own TTL.
    /// Typical values: 300-86400 (5 minutes to 1 day).
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,

    /// Map of nameserver hostnames to IP addresses for glue records.
    ///
    /// Glue records provide IP addresses for nameservers within the zone's own domain.
    /// This is necessary when delegating subdomains where the nameserver is within the
    /// delegated zone itself.
    ///
    /// Example: When delegating `sub.example.com` with nameserver `ns1.sub.example.com`,
    /// you must provide the IP address of `ns1.sub.example.com` as a glue record.
    ///
    /// Format: `{"ns1.example.com.": "192.0.2.1", "ns2.example.com.": "192.0.2.2"}`
    ///
    /// Note: Nameserver hostnames should end with a dot (.) for FQDN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_server_ips: Option<HashMap<String, String>>,
}

/// `ARecord` maps a DNS name to an IPv4 address.
///
/// A records are the most common DNS record type, mapping hostnames to IPv4 addresses.
/// Multiple A records can exist for the same name (round-robin DNS).
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: ARecord
/// metadata:
///   name: www-example-com
///   namespace: dns-system
/// spec:
///   zoneRef: example-com
///   name: www
///   ipv4Address: 192.0.2.1
///   ttl: 300
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "ARecord",
    namespaced,
    shortname = "a",
    doc = "ARecord maps a DNS hostname to an IPv4 address. Multiple A records for the same name enable round-robin DNS load balancing.",
    printcolumn = r#"{"name":"ZoneRef","type":"string","jsonPath":".spec.zoneRef"}"#,
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct ARecordSpec {
    /// Reference to a `DNSZone` resource by metadata.name.
    ///
    /// Directly references a `DNSZone` resource in the same namespace by its Kubernetes resource name.
    /// This is more efficient than searching by zone name.
    ///
    /// Example: If the `DNSZone` is named "example-com", use `zoneRef: example-com`
    pub zone_ref: String,

    /// Record name within the zone. Use "@" for the zone apex.
    ///
    /// Examples: "www", "mail", "ftp", "@"
    /// The full DNS name will be: {name}.{zone}
    pub name: String,

    /// IPv4 address in dotted-decimal notation.
    ///
    /// Must be a valid IPv4 address (e.g., "192.0.2.1").
    #[schemars(regex(
        pattern = r"^((25[0-5]|(2[0-4]|1\d|[1-9]|)\d)\.){3}(25[0-5]|(2[0-4]|1\d|[1-9]|)\d)$"
    ))]
    pub ipv4_address: String,

    /// Time To Live in seconds. Overrides zone default TTL if specified.
    ///
    /// Typical values: 60-86400 (1 minute to 1 day).
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,
}

/// `AAAARecord` maps a DNS name to an IPv6 address.
///
/// AAAA records are the IPv6 equivalent of A records, mapping hostnames to IPv6 addresses.
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: AAAARecord
/// metadata:
///   name: www-example-com-ipv6
/// spec:
///   zoneRef: example-com
///   name: www
///   ipv6Address: "2001:db8::1"
///   ttl: 300
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "AAAARecord",
    namespaced,
    shortname = "aaaa",
    doc = "AAAARecord maps a DNS hostname to an IPv6 address. This is the IPv6 equivalent of an A record.",
    printcolumn = r#"{"name":"ZoneRef","type":"string","jsonPath":".spec.zoneRef"}"#,
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct AAAARecordSpec {
    /// Reference to a `DNSZone` resource by metadata.name.
    ///
    /// Directly references a `DNSZone` resource in the same namespace by its Kubernetes resource name.
    pub zone_ref: String,

    /// Record name within the zone.
    pub name: String,

    /// IPv6 address in standard notation.
    ///
    /// Examples: `2001:db8::1`, `fe80::1`, `::1`
    pub ipv6_address: String,

    /// Time To Live in seconds.
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,
}

/// `TXTRecord` holds arbitrary text data.
///
/// TXT records are commonly used for SPF, DKIM, DMARC, domain verification,
/// and other text-based metadata.
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: TXTRecord
/// metadata:
///   name: spf-example-com
/// spec:
///   zoneRef: example-com
///   name: "@"
///   text:
///     - "v=spf1 include:_spf.google.com ~all"
///   ttl: 3600
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "TXTRecord",
    namespaced,
    shortname = "txt",
    doc = "TXTRecord stores arbitrary text data in DNS. Commonly used for SPF, DKIM, DMARC policies, and domain verification.",
    printcolumn = r#"{"name":"ZoneRef","type":"string","jsonPath":".spec.zoneRef"}"#,
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct TXTRecordSpec {
    /// Reference to a `DNSZone` resource by metadata.name.
    ///
    /// Directly references a `DNSZone` resource in the same namespace by its Kubernetes resource name.
    pub zone_ref: String,

    /// Record name within the zone.
    pub name: String,

    /// Array of text strings. Each string can be up to 255 characters.
    ///
    /// Multiple strings are concatenated by DNS resolvers.
    /// For long text, split into multiple strings.
    pub text: Vec<String>,

    /// Time To Live in seconds.
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,
}

/// `CNAMERecord` creates an alias from one name to another.
///
/// CNAME (Canonical Name) records create an alias from one DNS name to another.
/// The target can be in the same zone or a different zone.
///
/// **Important**: A CNAME cannot coexist with other record types for the same name.
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: CNAMERecord
/// metadata:
///   name: blog-example-com
/// spec:
///   zoneRef: example-com
///   name: blog
///   target: example.github.io.
///   ttl: 3600
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "CNAMERecord",
    namespaced,
    shortname = "cname",
    doc = "CNAMERecord creates a DNS alias from one hostname to another. A CNAME cannot coexist with other record types for the same name.",
    printcolumn = r#"{"name":"ZoneRef","type":"string","jsonPath":".spec.zoneRef"}"#,
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct CNAMERecordSpec {
    /// Reference to a `DNSZone` resource by metadata.name.
    ///
    /// Directly references a `DNSZone` resource in the same namespace by its Kubernetes resource name.
    pub zone_ref: String,

    /// Record name within the zone.
    ///
    /// Note: CNAME records cannot be created at the zone apex (@).
    pub name: String,

    /// Target hostname (canonical name).
    ///
    /// Should be a fully qualified domain name ending with a dot.
    /// Example: "example.com." or "www.example.com."
    pub target: String,

    /// Time To Live in seconds.
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,
}

/// `MXRecord` specifies mail servers for a domain.
///
/// MX (Mail Exchange) records specify the mail servers responsible for accepting email
/// for a domain. Lower priority values indicate higher preference.
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: MXRecord
/// metadata:
///   name: mail-example-com
/// spec:
///   zoneRef: example-com
///   name: "@"
///   priority: 10
///   mailServer: mail.example.com.
///   ttl: 3600
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "MXRecord",
    namespaced,
    shortname = "mx",
    doc = "MXRecord specifies mail exchange servers for a domain. Lower priority values indicate higher preference for mail delivery.",
    printcolumn = r#"{"name":"ZoneRef","type":"string","jsonPath":".spec.zoneRef"}"#,
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct MXRecordSpec {
    /// Reference to a `DNSZone` resource by metadata.name.
    ///
    /// Directly references a `DNSZone` resource in the same namespace by its Kubernetes resource name.
    pub zone_ref: String,

    /// Record name within the zone. Use "@" for the zone apex.
    pub name: String,

    /// Priority (preference) of this mail server. Lower values = higher priority.
    ///
    /// Common values: 0-100. Multiple MX records can exist with different priorities.
    #[schemars(range(min = 0, max = 65535))]
    pub priority: i32,

    /// Fully qualified domain name of the mail server.
    ///
    /// Must end with a dot. Example: "mail.example.com."
    pub mail_server: String,

    /// Time To Live in seconds.
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,
}

/// `NSRecord` delegates a subdomain to other nameservers.
///
/// NS (Nameserver) records specify which DNS servers are authoritative for a subdomain.
/// They are used for delegating subdomains to different nameservers.
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: NSRecord
/// metadata:
///   name: subdomain-ns
/// spec:
///   zoneRef: example-com
///   name: subdomain
///   nameserver: ns1.other-provider.com.
///   ttl: 86400
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "NSRecord",
    namespaced,
    shortname = "ns",
    doc = "NSRecord delegates a subdomain to authoritative nameservers. Used for subdomain delegation to different DNS providers or servers.",
    printcolumn = r#"{"name":"ZoneRef","type":"string","jsonPath":".spec.zoneRef"}"#,
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct NSRecordSpec {
    /// Reference to a `DNSZone` resource by metadata.name.
    ///
    /// Directly references a `DNSZone` resource in the same namespace by its Kubernetes resource name.
    pub zone_ref: String,

    /// Subdomain to delegate. For zone apex, use "@".
    pub name: String,

    /// Fully qualified domain name of the nameserver.
    ///
    /// Must end with a dot. Example: "ns1.example.com."
    pub nameserver: String,

    /// Time To Live in seconds.
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,
}

/// `SRVRecord` specifies the location of services.
///
/// SRV (Service) records specify the hostname and port of servers for specific services.
/// The name format is: _service._proto (e.g., _ldap._tcp, _sip._udp).
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: SRVRecord
/// metadata:
///   name: ldap-srv
/// spec:
///   zoneRef: example-com
///   name: _ldap._tcp
///   priority: 10
///   weight: 60
///   port: 389
///   target: ldap.example.com.
///   ttl: 3600
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "SRVRecord",
    namespaced,
    shortname = "srv",
    doc = "SRVRecord specifies the hostname and port of servers for specific services. The record name follows the format _service._proto (e.g., _ldap._tcp).",
    printcolumn = r#"{"name":"ZoneRef","type":"string","jsonPath":".spec.zoneRef"}"#,
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct SRVRecordSpec {
    /// Reference to a `DNSZone` resource by metadata.name.
    ///
    /// Directly references a `DNSZone` resource in the same namespace by its Kubernetes resource name.
    pub zone_ref: String,

    /// Service and protocol in the format: _service._proto
    ///
    /// Example: "_ldap._tcp", "_sip._udp", "_http._tcp"
    pub name: String,

    /// Priority of the target host. Lower values = higher priority.
    #[schemars(range(min = 0, max = 65535))]
    pub priority: i32,

    /// Relative weight for records with the same priority.
    ///
    /// Higher values = higher probability of selection.
    #[schemars(range(min = 0, max = 65535))]
    pub weight: i32,

    /// TCP or UDP port where the service is available.
    #[schemars(range(min = 0, max = 65535))]
    pub port: i32,

    /// Fully qualified domain name of the target host.
    ///
    /// Must end with a dot. Use "." for "service not available".
    pub target: String,

    /// Time To Live in seconds.
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,
}

/// `CAARecord` specifies Certificate Authority Authorization.
///
/// CAA (Certification Authority Authorization) records specify which certificate
/// authorities are allowed to issue certificates for a domain.
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: CAARecord
/// metadata:
///   name: caa-letsencrypt
/// spec:
///   zoneRef: example-com
///   name: "@"
///   flags: 0
///   tag: issue
///   value: letsencrypt.org
///   ttl: 86400
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "CAARecord",
    namespaced,
    shortname = "caa",
    doc = "CAARecord specifies which certificate authorities are authorized to issue certificates for a domain. Enhances domain security and certificate issuance control.",
    printcolumn = r#"{"name":"ZoneRef","type":"string","jsonPath":".spec.zoneRef"}"#,
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct CAARecordSpec {
    /// Reference to a `DNSZone` resource by metadata.name.
    ///
    /// Directly references a `DNSZone` resource in the same namespace by its Kubernetes resource name.
    pub zone_ref: String,

    /// Record name within the zone. Use "@" for the zone apex.
    pub name: String,

    /// Flags byte. Use 0 for non-critical, 128 for critical.
    ///
    /// Critical flag (128) means CAs must understand the tag.
    #[schemars(range(min = 0, max = 255))]
    pub flags: i32,

    /// Property tag. Common values: "issue", "issuewild", "iodef".
    ///
    /// - "issue": Authorize CA to issue certificates
    /// - "issuewild": Authorize CA to issue wildcard certificates
    /// - "iodef": URL/email for violation reports
    pub tag: String,

    /// Property value. Format depends on the tag.
    ///
    /// For "issue"/"issuewild": CA domain (e.g., "letsencrypt.org")
    /// For "iodef": mailto: or https: URL
    pub value: String,

    /// Time To Live in seconds.
    #[serde(default)]
    #[schemars(range(min = 0, max = 2_147_483_647))]
    pub ttl: Option<i32>,
}

/// Generic record status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}

/// RNDC/TSIG algorithm for authenticated communication and zone transfers.
///
/// These HMAC algorithms are supported by BIND9 for securing RNDC communication
/// and zone transfers (AXFR/IXFR).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RndcAlgorithm {
    /// HMAC-MD5 (legacy, not recommended for new deployments)
    HmacMd5,
    /// HMAC-SHA1
    HmacSha1,
    /// HMAC-SHA224
    HmacSha224,
    /// HMAC-SHA256 (recommended)
    #[default]
    HmacSha256,
    /// HMAC-SHA384
    HmacSha384,
    /// HMAC-SHA512
    HmacSha512,
}

impl RndcAlgorithm {
    /// Convert enum to string representation expected by BIND9
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HmacMd5 => "hmac-md5",
            Self::HmacSha1 => "hmac-sha1",
            Self::HmacSha224 => "hmac-sha224",
            Self::HmacSha256 => "hmac-sha256",
            Self::HmacSha384 => "hmac-sha384",
            Self::HmacSha512 => "hmac-sha512",
        }
    }

    /// Convert enum to string format expected by the rndc Rust crate.
    ///
    /// The rndc crate expects algorithm strings without the "hmac-" prefix
    /// (e.g., "sha256" instead of "hmac-sha256").
    #[must_use]
    pub fn as_rndc_str(&self) -> &'static str {
        match self {
            Self::HmacMd5 => "md5",
            Self::HmacSha1 => "sha1",
            Self::HmacSha224 => "sha224",
            Self::HmacSha256 => "sha256",
            Self::HmacSha384 => "sha384",
            Self::HmacSha512 => "sha512",
        }
    }
}

/// Reference to a Kubernetes Secret containing RNDC/TSIG credentials.
///
/// This allows you to use an existing external Secret for RNDC authentication instead
/// of having the operator auto-generate one. The Secret is mounted as a directory at
/// `/etc/bind/keys/` in the BIND9 container, and BIND9 uses the `rndc.key` file.
///
/// # External (User-Managed) Secrets
///
/// For external secrets, you ONLY need to provide the `rndc.key` field containing
/// the complete BIND9 key file content. The other fields (`key-name`, `algorithm`,
/// `secret`) are optional metadata used by operator-generated secrets.
///
/// ## Minimal External Secret Example
///
/// ```yaml
/// apiVersion: v1
/// kind: Secret
/// metadata:
///   name: my-rndc-key
///   namespace: dns-system
/// type: Opaque
/// stringData:
///   rndc.key: |
///     key "bindy-operator" {
///         algorithm hmac-sha256;
///         secret "base64EncodedSecretKeyMaterial==";
///     };
/// ```
///
/// # Auto-Generated (Operator-Managed) Secrets
///
/// When the operator auto-generates a Secret (no `rndcSecretRef` specified), it
/// creates a Secret with all 4 fields for internal metadata tracking:
///
/// ```yaml
/// apiVersion: v1
/// kind: Secret
/// metadata:
///   name: bind9-instance-rndc
///   namespace: dns-system
/// type: Opaque
/// stringData:
///   key-name: "bindy-operator"     # Operator metadata
///   algorithm: "hmac-sha256"       # Operator metadata
///   secret: "randomBase64Key=="    # Operator metadata
///   rndc.key: |                    # Used by BIND9
///     key "bindy-operator" {
///         algorithm hmac-sha256;
///         secret "randomBase64Key==";
///     };
/// ```
///
/// # Using with `Bind9Instance`
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: Bind9Instance
/// metadata:
///   name: production-dns-primary-0
/// spec:
///   clusterRef: production-dns
///   role: primary
///   rndcSecretRef:
///     name: my-rndc-key
///     algorithm: hmac-sha256
/// ```
///
/// # How It Works
///
/// When the Secret is mounted at `/etc/bind/keys/`, Kubernetes creates individual
/// files for each Secret key:
/// - `/etc/bind/keys/rndc.key` (the BIND9 key file) ← **This is what BIND9 uses**
/// - `/etc/bind/keys/key-name` (optional metadata for operator-generated secrets)
/// - `/etc/bind/keys/algorithm` (optional metadata for operator-generated secrets)
/// - `/etc/bind/keys/secret` (optional metadata for operator-generated secrets)
///
/// The `rndc.conf` file includes `/etc/bind/keys/rndc.key`, so BIND9 only needs
/// that one file to exist
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RndcSecretRef {
    /// Name of the Kubernetes Secret containing RNDC credentials
    pub name: String,

    /// HMAC algorithm for this key
    #[serde(default)]
    pub algorithm: RndcAlgorithm,

    /// Key within the secret for the key name (default: "key-name")
    #[serde(default = "default_key_name_key")]
    pub key_name_key: String,

    /// Key within the secret for the secret value (default: "secret")
    #[serde(default = "default_secret_key")]
    pub secret_key: String,
}

fn default_key_name_key() -> String {
    "key-name".to_string()
}

fn default_secret_key() -> String {
    "secret".to_string()
}

/// Default BIND9 version for clusters when not specified
#[allow(clippy::unnecessary_wraps)]
fn default_bind9_version() -> Option<String> {
    Some(crate::constants::DEFAULT_BIND9_VERSION.to_string())
}

/// TSIG Key configuration for authenticated zone transfers (deprecated in favor of `RndcSecretRef`)
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TSIGKey {
    /// Name of the TSIG key
    pub name: String,
    /// Algorithm for HMAC-based authentication
    pub algorithm: RndcAlgorithm,
    /// Secret key (base64 encoded) - should reference a Secret
    pub secret: String,
}

/// BIND9 server configuration options
///
/// These settings configure the BIND9 DNS server behavior including recursion,
/// access control lists, DNSSEC, and network listeners.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bind9Config {
    /// Enable or disable recursive DNS queries
    ///
    /// When enabled (`true`), the DNS server will recursively resolve queries by
    /// contacting other authoritative nameservers. When disabled (`false`), the
    /// server only answers for zones it is authoritative for.
    ///
    /// Default: `false` (authoritative-only mode)
    ///
    /// **Important**: Recursive resolvers should not be publicly accessible due to
    /// security risks (DNS amplification attacks, cache poisoning).
    #[serde(default)]
    pub recursion: Option<bool>,

    /// Access control list for DNS queries
    ///
    /// Specifies which IP addresses or networks are allowed to query this DNS server.
    /// Supports CIDR notation and special keywords.
    ///
    /// Default: Not set (BIND9 defaults to localhost only)
    ///
    /// Examples:
    /// - `["0.0.0.0/0"]` - Allow queries from any IPv4 address
    /// - `["10.0.0.0/8", "172.16.0.0/12"]` - Allow queries from private networks
    /// - `["any"]` - Allow queries from any IP (IPv4 and IPv6)
    /// - `["none"]` - Deny all queries
    /// - `["localhost"]` - Allow only from localhost
    #[serde(default)]
    pub allow_query: Option<Vec<String>>,

    /// Access control list for zone transfers (AXFR/IXFR)
    ///
    /// Specifies which IP addresses or networks are allowed to perform zone transfers
    /// from this server. Zone transfers are used for replication between primary and
    /// secondary DNS servers.
    ///
    /// Default: Auto-detected cluster Pod CIDRs (e.g., `["10.42.0.0/16"]`)
    ///
    /// Examples:
    /// - `["10.42.0.0/16"]` - Allow transfers from specific Pod network
    /// - `["10.0.0.0/8"]` - Allow transfers from entire private network
    /// - `[]` - Deny all zone transfers (empty list means "none")
    /// - `["any"]` - Allow transfers from any IP (not recommended for production)
    ///
    /// Can be overridden at cluster level via `spec.primary.allowTransfer` or
    /// `spec.secondary.allowTransfer` for role-specific ACLs.
    #[serde(default)]
    pub allow_transfer: Option<Vec<String>>,

    /// DNSSEC (DNS Security Extensions) configuration
    ///
    /// Configures DNSSEC signing and validation. DNSSEC provides cryptographic
    /// authentication of DNS data to prevent spoofing and cache poisoning attacks.
    ///
    /// See `DNSSECConfig` for detailed options.
    #[serde(default)]
    pub dnssec: Option<DNSSECConfig>,

    /// DNS forwarders for recursive resolution
    ///
    /// List of upstream DNS servers to forward queries to when recursion is enabled.
    /// Used for hybrid authoritative/recursive configurations.
    ///
    /// Only relevant when `recursion: true`.
    ///
    /// Examples:
    /// - `["8.8.8.8", "8.8.4.4"]` - Google Public DNS
    /// - `["1.1.1.1", "1.0.0.1"]` - Cloudflare DNS
    /// - `["10.0.0.53"]` - Internal corporate DNS resolver
    #[serde(default)]
    pub forwarders: Option<Vec<String>>,

    /// IPv4 addresses to listen on for DNS queries
    ///
    /// Specifies which IPv4 interfaces and ports the DNS server should bind to.
    ///
    /// Default: All IPv4 interfaces on port 53
    ///
    /// Examples:
    /// - `["any"]` - Listen on all IPv4 interfaces
    /// - `["127.0.0.1"]` - Listen only on localhost
    /// - `["10.0.0.1"]` - Listen on specific IP address
    #[serde(default)]
    pub listen_on: Option<Vec<String>>,

    /// IPv6 addresses to listen on for DNS queries
    ///
    /// Specifies which IPv6 interfaces and ports the DNS server should bind to.
    ///
    /// Default: All IPv6 interfaces on port 53 (if IPv6 is available)
    ///
    /// Examples:
    /// - `["any"]` - Listen on all IPv6 interfaces
    /// - `["::1"]` - Listen only on IPv6 localhost
    /// - `["none"]` - Disable IPv6 listening
    #[serde(default)]
    pub listen_on_v6: Option<Vec<String>>,

    /// Reference to an existing Kubernetes Secret containing RNDC key.
    ///
    /// If specified at the global config level, all instances in the cluster will use
    /// this existing Secret instead of auto-generating individual secrets, unless
    /// overridden at the role (primary/secondary) or instance level.
    ///
    /// This allows centralized RNDC key management for the entire cluster.
    ///
    /// Precedence order (highest to lowest):
    /// 1. Instance level (`spec.rndcSecretRef`)
    /// 2. Role level (`spec.primary.rndcSecretRef` or `spec.secondary.rndcSecretRef`)
    /// 3. Global level (`spec.global.rndcSecretRef`)
    /// 4. Auto-generated (default)
    #[serde(default)]
    pub rndc_secret_ref: Option<RndcSecretRef>,

    /// Bindcar RNDC API sidecar container configuration.
    ///
    /// The API container provides an HTTP interface for managing zones via rndc.
    /// This configuration is inherited by all instances unless overridden.
    #[serde(default)]
    pub bindcar_config: Option<BindcarConfig>,
}

/// DNSSEC (DNS Security Extensions) configuration
///
/// DNSSEC adds cryptographic signatures to DNS records to ensure authenticity and integrity.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DNSSECConfig {
    /// Enable DNSSEC validation of responses
    ///
    /// When enabled, BIND will validate DNSSEC signatures on responses from other
    /// nameservers. Invalid or missing signatures will cause queries to fail.
    ///
    /// Default: `false`
    ///
    /// **Important**: Requires valid DNSSEC trust anchors and proper network connectivity
    /// to root DNS servers. May cause resolution failures if DNSSEC is broken upstream.
    #[serde(default)]
    pub validation: Option<bool>,
}

/// Container image configuration for BIND9 instances
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    /// Container image repository and tag for BIND9
    ///
    /// Example: "internetsystemsconsortium/bind9:9.18"
    #[serde(default)]
    pub image: Option<String>,

    /// Image pull policy
    ///
    /// Example: `IfNotPresent`, `Always`, `Never`
    #[serde(default)]
    pub image_pull_policy: Option<String>,

    /// Reference to image pull secrets for private registries
    #[serde(default)]
    pub image_pull_secrets: Option<Vec<String>>,
}

/// `ConfigMap` references for BIND9 configuration files
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMapRefs {
    /// `ConfigMap` containing named.conf file
    ///
    /// If not specified, a default configuration will be generated
    #[serde(default)]
    pub named_conf: Option<String>,

    /// `ConfigMap` containing named.conf.options file
    ///
    /// If not specified, a default configuration will be generated
    #[serde(default)]
    pub named_conf_options: Option<String>,

    /// `ConfigMap` containing named.conf.zones file
    ///
    /// Optional. If specified, the zones file from this `ConfigMap` will be included in named.conf.
    /// If not specified, no zones file will be included (zones can be added dynamically via RNDC).
    /// Use this for pre-configured zones or to import existing BIND9 zone configurations.
    #[serde(default)]
    pub named_conf_zones: Option<String>,
}

/// Service configuration including spec and annotations
///
/// Allows customization of both the Kubernetes Service spec and metadata annotations.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfig {
    /// Annotations to apply to the Service metadata
    ///
    /// Common use cases:
    /// - `MetalLB` address pool selection: `metallb.universe.tf/address-pool: my-ip-pool`
    /// - AWS load balancer configuration: `service.beta.kubernetes.io/aws-load-balancer-type: nlb`
    /// - External DNS hostname: `external-dns.alpha.kubernetes.io/hostname: dns.example.com`
    ///
    /// Example:
    /// ```yaml
    /// annotations:
    ///   metallb.universe.tf/address-pool: my-ip-pool
    ///   external-dns.alpha.kubernetes.io/hostname: ns1.example.com
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<BTreeMap<String, String>>,

    /// Custom Kubernetes Service spec
    ///
    /// Allows full customization of the Kubernetes Service created for DNS servers.
    /// This accepts the same fields as the standard Kubernetes Service `spec`.
    ///
    /// Common fields:
    /// - `type`: Service type (`ClusterIP`, `NodePort`, `LoadBalancer`)
    /// - `loadBalancerIP`: Specific IP for `LoadBalancer` type
    /// - `externalTrafficPolicy`: `Local` or `Cluster`
    /// - `sessionAffinity`: `ClientIP` or `None`
    /// - `clusterIP`: Specific cluster IP (use with caution)
    ///
    /// Fields specified here are merged with defaults. Unspecified fields use safe defaults:
    /// - `type: ClusterIP` (if not specified)
    /// - Ports 53/TCP and 53/UDP (always set)
    /// - Selector matching the instance labels (always set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<ServiceSpec>,
}

/// Primary instance configuration
///
/// Groups all configuration specific to primary (authoritative) DNS instances.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryConfig {
    /// Number of primary instance replicas (default: 1)
    ///
    /// This controls how many replicas each primary instance in this cluster should have.
    /// Can be overridden at the instance level.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0, max = 100))]
    pub replicas: Option<i32>,

    /// Custom Kubernetes Service configuration for primary instances
    ///
    /// Allows full customization of the Kubernetes Service created for primary DNS servers,
    /// including both Service spec fields and metadata annotations.
    ///
    /// Annotations are commonly used for:
    /// - `MetalLB` address pool selection
    /// - Cloud provider load balancer configuration
    /// - External DNS integration
    /// - Linkerd service mesh annotations
    ///
    /// Fields specified here are merged with defaults. Unspecified fields use safe defaults:
    /// - `type: ClusterIP` (if not specified)
    /// - Ports 53/TCP and 53/UDP (always set)
    /// - Selector matching the instance labels (always set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceConfig>,

    /// Allow-transfer ACL for primary instances
    ///
    /// Overrides the default auto-detected Pod CIDR allow-transfer configuration
    /// for all primary instances in this cluster. Use this to restrict or expand
    /// which IP addresses can perform zone transfers from primary servers.
    ///
    /// If not specified, defaults to cluster Pod CIDRs (auto-detected from Kubernetes Nodes).
    ///
    /// Examples:
    /// - `["10.0.0.0/8"]` - Allow transfers from entire 10.x network
    /// - `["any"]` - Allow transfers from any IP (public internet)
    /// - `[]` - Deny all zone transfers (empty list means "none")
    ///
    /// Can be overridden at the instance level via `spec.config.allowTransfer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_transfer: Option<Vec<String>>,

    /// Reference to an existing Kubernetes Secret containing RNDC key for all primary instances.
    ///
    /// If specified, all primary instances in this cluster will use this existing Secret
    /// instead of auto-generating individual secrets. This allows sharing the same RNDC key
    /// across all primary instances.
    ///
    /// Can be overridden at the instance level via `spec.rndcSecretRef`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rndc_secret_ref: Option<RndcSecretRef>,
}

/// Secondary instance configuration
///
/// Groups all configuration specific to secondary (replica) DNS instances.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryConfig {
    /// Number of secondary instance replicas (default: 1)
    ///
    /// This controls how many replicas each secondary instance in this cluster should have.
    /// Can be overridden at the instance level.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0, max = 100))]
    pub replicas: Option<i32>,

    /// Custom Kubernetes Service configuration for secondary instances
    ///
    /// Allows full customization of the Kubernetes Service created for secondary DNS servers,
    /// including both Service spec fields and metadata annotations.
    ///
    /// Annotations are commonly used for:
    /// - `MetalLB` address pool selection
    /// - Cloud provider load balancer configuration
    /// - External DNS integration
    /// - Linkerd service mesh annotations
    ///
    /// Allows different service configurations for primary vs secondary instances.
    /// Example: Primaries use `LoadBalancer` with specific annotations, secondaries use `ClusterIP`
    ///
    /// See `PrimaryConfig.service` for detailed field documentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceConfig>,

    /// Allow-transfer ACL for secondary instances
    ///
    /// Overrides the default auto-detected Pod CIDR allow-transfer configuration
    /// for all secondary instances in this cluster. Use this to restrict or expand
    /// which IP addresses can perform zone transfers from secondary servers.
    ///
    /// If not specified, defaults to cluster Pod CIDRs (auto-detected from Kubernetes Nodes).
    ///
    /// Examples:
    /// - `["10.0.0.0/8"]` - Allow transfers from entire 10.x network
    /// - `["any"]` - Allow transfers from any IP (public internet)
    /// - `[]` - Deny all zone transfers (empty list means "none")
    ///
    /// Can be overridden at the instance level via `spec.config.allowTransfer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_transfer: Option<Vec<String>>,

    /// Reference to an existing Kubernetes Secret containing RNDC key for all secondary instances.
    ///
    /// If specified, all secondary instances in this cluster will use this existing Secret
    /// instead of auto-generating individual secrets. This allows sharing the same RNDC key
    /// across all secondary instances.
    ///
    /// Can be overridden at the instance level via `spec.rndcSecretRef`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rndc_secret_ref: Option<RndcSecretRef>,
}

/// Common specification fields shared between namespace-scoped and cluster-scoped BIND9 clusters.
///
/// This struct contains all configuration that is common to both `Bind9Cluster` (namespace-scoped)
/// and `ClusterBind9Provider` (cluster-scoped). By using this shared struct, we avoid code duplication
/// and ensure consistency between the two cluster types.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bind9ClusterCommonSpec {
    /// Shared BIND9 version for the cluster
    ///
    /// If not specified, defaults to "9.18".
    #[serde(default = "default_bind9_version")]
    #[schemars(default = "default_bind9_version")]
    pub version: Option<String>,

    /// Primary instance configuration
    ///
    /// Configuration specific to primary (authoritative) DNS instances,
    /// including replica count and service specifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<PrimaryConfig>,

    /// Secondary instance configuration
    ///
    /// Configuration specific to secondary (replica) DNS instances,
    /// including replica count and service specifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<SecondaryConfig>,

    /// Container image configuration
    #[serde(default)]
    pub image: Option<ImageConfig>,

    /// `ConfigMap` references for BIND9 configuration files
    #[serde(default)]
    pub config_map_refs: Option<ConfigMapRefs>,

    /// Global configuration shared by all instances in the cluster
    ///
    /// This configuration applies to all instances (both primary and secondary)
    /// unless overridden at the instance level or by role-specific configuration.
    #[serde(default)]
    pub global: Option<Bind9Config>,

    /// References to Kubernetes Secrets containing RNDC/TSIG keys for authenticated zone transfers.
    ///
    /// Each secret should contain the key name, algorithm, and base64-encoded secret value.
    /// These secrets are used for secure communication with BIND9 instances via RNDC and
    /// for authenticated zone transfers (AXFR/IXFR) between primary and secondary servers.
    #[serde(default)]
    pub rndc_secret_refs: Option<Vec<RndcSecretRef>>,

    /// ACLs that can be referenced by instances
    #[serde(default)]
    pub acls: Option<BTreeMap<String, Vec<String>>>,

    /// Volumes that can be mounted by instances in this cluster
    ///
    /// These volumes are inherited by all instances unless overridden.
    /// Common use cases include `PersistentVolumeClaims` for zone data storage.
    #[serde(default)]
    pub volumes: Option<Vec<Volume>>,

    /// Volume mounts that specify where volumes should be mounted in containers
    ///
    /// These mounts are inherited by all instances unless overridden.
    #[serde(default)]
    pub volume_mounts: Option<Vec<VolumeMount>>,
}

/// `Bind9Cluster` - Namespace-scoped DNS cluster for tenant-managed infrastructure.
///
/// A namespace-scoped cluster allows development teams to run their own isolated BIND9
/// DNS infrastructure within their namespace. Each team can manage their own cluster
/// independently, with RBAC controlling who can create and manage resources.
///
/// For platform-managed, cluster-wide DNS infrastructure, use `ClusterBind9Provider` instead.
///
/// # Use Cases
///
/// - Development teams need isolated DNS infrastructure for testing
/// - Multi-tenant environments where each team manages their own DNS
/// - Namespaced DNS services that don't need cluster-wide visibility
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: Bind9Cluster
/// metadata:
///   name: dev-team-dns
///   namespace: dev-team-alpha
/// spec:
///   version: "9.18"
///   primary:
///     replicas: 1
///   secondary:
///     replicas: 1
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "Bind9Cluster",
    namespaced,
    shortname = "b9c",
    shortname = "b9cs",
    doc = "Bind9Cluster defines a namespace-scoped logical grouping of BIND9 DNS server instances. Use this for tenant-managed DNS infrastructure isolated to a specific namespace. For platform-managed cluster-wide DNS, use ClusterBind9Provider instead.",
    printcolumn = r#"{"name":"Version","type":"string","jsonPath":".spec.version"}"#,
    printcolumn = r#"{"name":"Primary","type":"integer","jsonPath":".spec.primary.replicas"}"#,
    printcolumn = r#"{"name":"Secondary","type":"integer","jsonPath":".spec.secondary.replicas"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "Bind9ClusterStatus")]
#[serde(rename_all = "camelCase")]
pub struct Bind9ClusterSpec {
    /// All cluster configuration is flattened from the common spec
    #[serde(flatten)]
    pub common: Bind9ClusterCommonSpec,
}

/// `ClusterBind9Provider` - Cluster-scoped BIND9 DNS provider for platform teams.
///
/// A cluster-scoped provider allows platform teams to provision shared BIND9 DNS infrastructure
/// that is accessible from any namespace. This is ideal for shared services, production DNS,
/// or platform-managed infrastructure that multiple teams use.
///
/// `DNSZones` in any namespace can reference a `ClusterBind9Provider` using the `clusterProviderRef` field.
///
/// # Use Cases
///
/// - Platform team provides shared DNS infrastructure for all namespaces
/// - Production DNS services that serve multiple applications
/// - Centrally managed DNS with governance and compliance requirements
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: ClusterBind9Provider
/// metadata:
///   name: shared-production-dns
///   # No namespace - cluster-scoped
/// spec:
///   version: "9.18"
///   primary:
///     replicas: 3
///     service:
///       type: LoadBalancer
///   secondary:
///     replicas: 2
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "ClusterBind9Provider",
    // NOTE: No 'namespaced' attribute = cluster-scoped
    shortname = "cb9p",
    shortname = "cb9ps",
    doc = "ClusterBind9Provider defines a cluster-scoped BIND9 DNS provider that manages DNS infrastructure accessible from all namespaces. Use this for platform-managed DNS infrastructure. For tenant-managed namespace-scoped DNS, use Bind9Cluster instead.",
    printcolumn = r#"{"name":"Version","type":"string","jsonPath":".spec.version"}"#,
    printcolumn = r#"{"name":"Primary","type":"integer","jsonPath":".spec.primary.replicas"}"#,
    printcolumn = r#"{"name":"Secondary","type":"integer","jsonPath":".spec.secondary.replicas"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "Bind9ClusterStatus")]
#[serde(rename_all = "camelCase")]
pub struct ClusterBind9ProviderSpec {
    /// Namespace where `Bind9Instance` resources will be created
    ///
    /// Since `ClusterBind9Provider` is cluster-scoped, instances need to be created in a specific namespace.
    /// Typically this would be a platform-managed namespace like `dns-system`.
    ///
    /// All managed instances (primary and secondary) will be created in this namespace.
    /// `DNSZones` from any namespace can reference this provider via `clusterProviderRef`.
    ///
    /// **Default:** If not specified, instances will be created in the same namespace where the
    /// Bindy operator is running (from `POD_NAMESPACE` environment variable).
    ///
    /// Example: `dns-system` for platform DNS infrastructure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// All cluster configuration is flattened from the common spec
    #[serde(flatten)]
    pub common: Bind9ClusterCommonSpec,
}

/// `Bind9Cluster` status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bind9ClusterStatus {
    /// Status conditions for this cluster
    #[serde(default)]
    pub conditions: Vec<Condition>,

    /// Observed generation for optimistic concurrency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,

    /// Number of instances in this cluster
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_count: Option<i32>,

    /// Number of ready instances
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_instances: Option<i32>,

    /// Names of `Bind9Instance` resources created for this cluster
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<String>,
}

/// Server role in the DNS cluster.
///
/// Determines whether the instance is authoritative (primary) or replicates from primaries (secondary).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ServerRole {
    /// Primary DNS server - authoritative source for zones.
    ///
    /// Primary servers hold the original zone data and process dynamic updates.
    /// Changes to zones are made on primaries and transferred to secondaries.
    Primary,

    /// Secondary DNS server - replicates zones from primary servers.
    ///
    /// Secondary servers receive zone data via AXFR (full) or IXFR (incremental)
    /// zone transfers. They provide redundancy and geographic distribution.
    Secondary,
}

/// `Bind9Instance` represents a BIND9 DNS server deployment in Kubernetes.
///
/// Each `Bind9Instance` creates a Deployment, Service, `ConfigMap`, and Secret for managing
/// a BIND9 server. The instance communicates with the controller via RNDC protocol.
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1alpha1
/// kind: Bind9Instance
/// metadata:
///   name: dns-primary
///   namespace: dns-system
/// spec:
///   clusterRef: my-dns-cluster
///   role: primary
///   replicas: 2
///   version: "9.18"
/// ```
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "Bind9Instance",
    namespaced,
    shortname = "b9",
    shortname = "b9s",
    doc = "Bind9Instance represents a BIND9 DNS server deployment in Kubernetes. Each instance creates a Deployment, Service, ConfigMap, and Secret for managing a BIND9 server with RNDC protocol communication.",
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".spec.clusterRef"}"#,
    printcolumn = r#"{"name":"Role","type":"string","jsonPath":".spec.role"}"#,
    printcolumn = r#"{"name":"Replicas","type":"integer","jsonPath":".spec.replicas"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "Bind9InstanceStatus")]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceSpec {
    /// Reference to the cluster this instance belongs to.
    ///
    /// Can reference either:
    /// - A namespace-scoped `Bind9Cluster` (must be in the same namespace as this instance)
    /// - A cluster-scoped `ClusterBind9Provider` (cluster-wide, accessible from any namespace)
    ///
    /// The cluster provides shared configuration and defines the logical grouping.
    /// The controller will automatically detect whether this references a namespace-scoped
    /// or cluster-scoped cluster resource.
    pub cluster_ref: String,

    /// Role of this instance (primary or secondary).
    ///
    /// Primary instances are authoritative for zones. Secondary instances
    /// replicate zones from primaries via AXFR/IXFR.
    pub role: ServerRole,

    /// Number of pod replicas for high availability.
    ///
    /// Defaults to 1 if not specified. For production, use 2+ replicas.
    #[serde(default)]
    #[schemars(range(min = 0, max = 100))]
    pub replicas: Option<i32>,

    /// BIND9 version override. Inherits from cluster if not specified.
    ///
    /// Example: "9.18", "9.16"
    #[serde(default)]
    pub version: Option<String>,

    /// Container image configuration override. Inherits from cluster if not specified.
    #[serde(default)]
    pub image: Option<ImageConfig>,

    /// `ConfigMap` references override. Inherits from cluster if not specified.
    #[serde(default)]
    pub config_map_refs: Option<ConfigMapRefs>,

    /// Instance-specific BIND9 configuration overrides.
    ///
    /// Overrides cluster-level configuration for this instance only.
    #[serde(default)]
    pub config: Option<Bind9Config>,

    /// Primary server addresses for zone transfers (required for secondary instances).
    ///
    /// List of IP addresses or hostnames of primary servers to transfer zones from.
    /// Example: `["10.0.1.10", "primary.example.com"]`
    #[serde(default)]
    pub primary_servers: Option<Vec<String>>,

    /// Volumes override for this instance. Inherits from cluster if not specified.
    ///
    /// These volumes override cluster-level volumes. Common use cases include
    /// instance-specific `PersistentVolumeClaims` for zone data storage.
    #[serde(default)]
    pub volumes: Option<Vec<Volume>>,

    /// Volume mounts override for this instance. Inherits from cluster if not specified.
    ///
    /// These mounts override cluster-level volume mounts.
    #[serde(default)]
    pub volume_mounts: Option<Vec<VolumeMount>>,

    /// Reference to an existing Kubernetes Secret containing RNDC key.
    ///
    /// If specified, uses this existing Secret instead of auto-generating one.
    /// The Secret must contain the keys specified in the reference (defaults: "key-name", "algorithm", "secret", "rndc.key").
    /// This allows sharing RNDC keys across instances or using externally managed secrets.
    ///
    /// If not specified, a Secret will be auto-generated for this instance.
    #[serde(default)]
    pub rndc_secret_ref: Option<RndcSecretRef>,

    /// Storage configuration for zone files.
    ///
    /// Specifies how zone files should be stored. Defaults to emptyDir (ephemeral storage).
    /// For persistent storage, use persistentVolumeClaim.
    #[serde(default)]
    pub storage: Option<StorageConfig>,

    /// Bindcar RNDC API sidecar container configuration.
    ///
    /// The API container provides an HTTP interface for managing zones via rndc.
    /// If not specified, uses default configuration.
    #[serde(default)]
    pub bindcar_config: Option<BindcarConfig>,
}

/// `Bind9Instance` status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_replicas: Option<i32>,
    /// IP or hostname of this instance's service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_address: Option<String>,
}

/// Storage configuration for zone files
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfig {
    /// Storage type (emptyDir or persistentVolumeClaim)
    #[serde(default = "default_storage_type")]
    pub storage_type: StorageType,

    /// `EmptyDir` configuration (used when storageType is emptyDir)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_dir: Option<k8s_openapi::api::core::v1::EmptyDirVolumeSource>,

    /// `PersistentVolumeClaim` configuration (used when storageType is persistentVolumeClaim)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistent_volume_claim: Option<PersistentVolumeClaimConfig>,
}

fn default_storage_type() -> StorageType {
    StorageType::EmptyDir
}

/// Storage type for zone files
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StorageType {
    /// Ephemeral storage (default) - data is lost when pod restarts
    EmptyDir,
    /// Persistent storage - data survives pod restarts
    PersistentVolumeClaim,
}

/// `PersistentVolumeClaim` configuration
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PersistentVolumeClaimConfig {
    /// Name of an existing PVC to use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_name: Option<String>,

    /// Storage class name for dynamic provisioning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_class_name: Option<String>,

    /// Storage size (e.g., "10Gi", "1Ti")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    /// Access modes (`ReadWriteOnce`, `ReadOnlyMany`, `ReadWriteMany`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_modes: Option<Vec<String>>,
}

/// Bindcar container configuration
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BindcarConfig {
    /// Container image for the RNDC API sidecar
    ///
    /// Example: "ghcr.io/firestoned/bindcar:latest"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Image pull policy (`Always`, `IfNotPresent`, `Never`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_pull_policy: Option<String>,

    /// Resource requirements for the Bindcar container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<k8s_openapi::api::core::v1::ResourceRequirements>,

    /// API server port (default: 8080)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Log level for the Bindcar container (`debug`, `info`, `warn`, `error`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,

    /// Environment variables for the Bindcar container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_vars: Option<Vec<EnvVar>>,

    /// Volumes that can be mounted by the Bindcar container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<Vec<Volume>>,

    /// Volume mounts for the Bindcar container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_mounts: Option<Vec<VolumeMount>>,
}
