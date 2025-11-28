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
//!     cluster_ref: "my-dns-cluster".to_string(),
//!     soa_record: soa,
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

use k8s_openapi::api::core::v1::{Volume, VolumeMount};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// `DNSZone` defines a DNS zone to be managed by BIND9.
///
/// A `DNSZone` represents an authoritative DNS zone (e.g., example.com) that will be
/// served by a BIND9 cluster. The zone includes SOA record information and will be
/// synchronized to all instances in the referenced cluster via AXFR/IXFR.
///
/// # Example
///
/// ```yaml
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: DNSZone
/// metadata:
///   name: example-com
///   namespace: dns-system
/// spec:
///   zoneName: example.com
///   clusterRef: my-dns-cluster
///   soa_record:
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
    doc = "DNSZone represents an authoritative DNS zone managed by BIND9. Each DNSZone defines a zone (e.g., example.com) with SOA record parameters and is served by a specified Bind9Instance."
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

    /// Reference to the `Bind9Instance` that serves this zone.
    ///
    /// Must match the name of a `Bind9Instance` in the same namespace.
    /// The zone will be added to this instance via rndc addzone.
    pub cluster_ref: String,

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
}

/// `ARecord` maps a DNS name to an IPv4 address.
///
/// A records are the most common DNS record type, mapping hostnames to IPv4 addresses.
/// Multiple A records can exist for the same name (round-robin DNS).
///
/// # Example
///
/// ```yaml
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: ARecord
/// metadata:
///   name: www-example-com
///   namespace: dns-system
/// spec:
///   zone: example.com
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
    doc = "ARecord maps a DNS hostname to an IPv4 address. Multiple A records for the same name enable round-robin DNS load balancing."
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct ARecordSpec {
    /// DNS zone this record belongs to (e.g., "example.com").
    ///
    /// Must match the zoneName of an existing `DNSZone` resource.
    pub zone: String,

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
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: AAAARecord
/// metadata:
///   name: www-example-com-ipv6
/// spec:
///   zone: example.com
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
    doc = "AAAARecord maps a DNS hostname to an IPv6 address. This is the IPv6 equivalent of an A record."
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct AAAARecordSpec {
    /// DNS zone this record belongs to.
    pub zone: String,

    /// Record name within the zone.
    pub name: String,

    /// IPv6 address in standard notation.
    ///
    /// Examples: "`2001:db8::1`", "`fe80::1`", "`::1`"
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
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: TXTRecord
/// metadata:
///   name: spf-example-com
/// spec:
///   zone: example.com
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
    doc = "TXTRecord stores arbitrary text data in DNS. Commonly used for SPF, DKIM, DMARC policies, and domain verification."
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct TXTRecordSpec {
    /// DNS zone this record belongs to.
    pub zone: String,

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
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: CNAMERecord
/// metadata:
///   name: blog-example-com
/// spec:
///   zone: example.com
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
    doc = "CNAMERecord creates a DNS alias from one hostname to another. A CNAME cannot coexist with other record types for the same name."
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct CNAMERecordSpec {
    /// DNS zone this record belongs to.
    pub zone: String,

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
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: MXRecord
/// metadata:
///   name: mail-example-com
/// spec:
///   zone: example.com
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
    doc = "MXRecord specifies mail exchange servers for a domain. Lower priority values indicate higher preference for mail delivery."
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct MXRecordSpec {
    /// DNS zone this record belongs to.
    pub zone: String,

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
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: NSRecord
/// metadata:
///   name: subdomain-ns
/// spec:
///   zone: example.com
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
    doc = "NSRecord delegates a subdomain to authoritative nameservers. Used for subdomain delegation to different DNS providers or servers."
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct NSRecordSpec {
    /// DNS zone this record belongs to.
    pub zone: String,

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
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: SRVRecord
/// metadata:
///   name: ldap-srv
/// spec:
///   zone: example.com
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
    doc = "SRVRecord specifies the hostname and port of servers for specific services. The record name follows the format _service._proto (e.g., _ldap._tcp)."
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct SRVRecordSpec {
    /// DNS zone this record belongs to.
    pub zone: String,

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
/// apiVersion: dns.firestoned.io/v1alpha1
/// kind: CAARecord
/// metadata:
///   name: caa-letsencrypt
/// spec:
///   zone: example.com
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
    doc = "CAARecord specifies which certificate authorities are authorized to issue certificates for a domain. Enhances domain security and certificate issuance control."
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct CAARecordSpec {
    /// DNS zone this record belongs to.
    pub zone: String,

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
pub struct RecordStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}

/// TSIG Key configuration for authenticated zone transfers
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TSIGKey {
    /// Name of the TSIG key
    pub name: String,
    /// Algorithm (e.g., "hmac-sha256")
    pub algorithm: String,
    /// Secret key (base64 encoded) - should reference a Secret
    pub secret: String,
}

/// `Bind9Config` options
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
    /// Example: "`IfNotPresent`", "Always", "Never"
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
}

/// `BIND9Cluster` - Defines a logical DNS cluster
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1alpha1",
    kind = "Bind9Cluster",
    namespaced,
    doc = "Bind9Cluster defines a logical grouping of BIND9 DNS server instances with shared configuration. Provides centralized management of BIND9 version, container images, and common settings across multiple instances."
)]
#[kube(status = "Bind9ClusterStatus")]
#[serde(rename_all = "camelCase")]
pub struct Bind9ClusterSpec {
    /// Shared BIND9 version for the cluster
    #[serde(default)]
    pub version: Option<String>,

    /// Container image configuration
    #[serde(default)]
    pub image: Option<ImageConfig>,

    /// `ConfigMap` references for BIND9 configuration files
    #[serde(default)]
    pub config_map_refs: Option<ConfigMapRefs>,

    /// Shared configuration for all instances in the cluster
    #[serde(default)]
    pub config: Option<Bind9Config>,

    /// TSIG keys for authenticated zone transfers
    #[serde(default)]
    pub tsig_keys: Option<Vec<TSIGKey>>,

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

/// `Bind9Cluster` status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct Bind9ClusterStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    /// Number of instances in this cluster
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_count: Option<i32>,
    /// Number of ready instances
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_instances: Option<i32>,
}

/// Server role in the DNS cluster.
///
/// Determines whether the instance is authoritative (primary) or replicates from primaries (secondary).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ServerRole {
    /// Primary (master) DNS server - authoritative source for zones.
    ///
    /// Primary servers hold the original zone data and process dynamic updates.
    /// Changes to zones are made on primaries and transferred to secondaries.
    Primary,

    /// Secondary (slave) DNS server - replicates zones from primary servers.
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
/// apiVersion: dns.firestoned.io/v1alpha1
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
    doc = "Bind9Instance represents a BIND9 DNS server deployment in Kubernetes. Each instance creates a Deployment, Service, ConfigMap, and Secret for managing a BIND9 server with RNDC protocol communication."
)]
#[kube(status = "Bind9InstanceStatus")]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceSpec {
    /// Reference to the `Bind9Cluster` this instance belongs to.
    ///
    /// The cluster provides shared configuration and defines the logical grouping.
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
}

/// `Bind9Instance` status
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
    /// IP or hostname of this instance's service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_address: Option<String>,
}
