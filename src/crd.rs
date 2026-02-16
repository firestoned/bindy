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
//! ```rust,no_run,ignore
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
//! // Example showing DNSZone spec structure
//! // Note: Actual spec fields may vary - see DNSZoneSpec definition
//! let spec = DNSZoneSpec {
//!     zone_name: "example.com".to_string(),
//!     soa_record: soa,
//!     ttl: Some(3600),
//!     name_server_ips: None,
//!     records_from: None,
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
//!     name: "www".to_string(),
//!     ipv4_addresses: vec!["192.0.2.1".to_string()],
//!     ttl: Some(300),
//! };
//!
//! // MX Record for mail routing
//! let mx_record = MXRecordSpec {
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

/// DNS record kind (type) enumeration.
///
/// Represents the Kubernetes `kind` field for all DNS record custom resources.
/// This enum eliminates magic strings when matching record types and provides
/// type-safe conversions between string representations and enum values.
///
/// # Example
///
/// ```rust,ignore
/// use bindy::crd::DNSRecordKind;
///
/// // Parse from string
/// let kind = DNSRecordKind::from("ARecord");
/// assert_eq!(kind, DNSRecordKind::A);
///
/// // Convert to string
/// assert_eq!(kind.as_str(), "ARecord");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DNSRecordKind {
    /// IPv4 address record (A)
    A,
    /// IPv6 address record (AAAA)
    AAAA,
    /// Text record (TXT)
    TXT,
    /// Canonical name record (CNAME)
    CNAME,
    /// Mail exchange record (MX)
    MX,
    /// Nameserver record (NS)
    NS,
    /// Service record (SRV)
    SRV,
    /// Certificate authority authorization record (CAA)
    CAA,
}

impl DNSRecordKind {
    /// Returns the Kubernetes `kind` string for this record type.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use bindy::crd::DNSRecordKind;
    ///
    /// assert_eq!(DNSRecordKind::A.as_str(), "ARecord");
    /// assert_eq!(DNSRecordKind::MX.as_str(), "MXRecord");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A => "ARecord",
            Self::AAAA => "AAAARecord",
            Self::TXT => "TXTRecord",
            Self::CNAME => "CNAMERecord",
            Self::MX => "MXRecord",
            Self::NS => "NSRecord",
            Self::SRV => "SRVRecord",
            Self::CAA => "CAARecord",
        }
    }

    /// Returns all DNS record kinds as a slice.
    ///
    /// Useful for iterating over all supported record types.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use bindy::crd::DNSRecordKind;
    ///
    /// for kind in DNSRecordKind::all() {
    ///     println!("Record type: {}", kind.as_str());
    /// }
    /// ```
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::A,
            Self::AAAA,
            Self::TXT,
            Self::CNAME,
            Self::MX,
            Self::NS,
            Self::SRV,
            Self::CAA,
        ]
    }

    /// Converts this DNS record kind to a Hickory DNS `RecordType`.
    ///
    /// This is useful when interfacing with the Hickory DNS library for
    /// zone file generation or DNS protocol operations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use bindy::crd::DNSRecordKind;
    /// use hickory_client::rr::RecordType;
    ///
    /// let kind = DNSRecordKind::A;
    /// let record_type = kind.to_hickory_record_type();
    /// assert_eq!(record_type, RecordType::A);
    /// ```
    #[must_use]
    pub const fn to_hickory_record_type(self) -> hickory_client::rr::RecordType {
        use hickory_client::rr::RecordType;
        match self {
            Self::A => RecordType::A,
            Self::AAAA => RecordType::AAAA,
            Self::TXT => RecordType::TXT,
            Self::CNAME => RecordType::CNAME,
            Self::MX => RecordType::MX,
            Self::NS => RecordType::NS,
            Self::SRV => RecordType::SRV,
            Self::CAA => RecordType::CAA,
        }
    }
}

impl From<&str> for DNSRecordKind {
    fn from(s: &str) -> Self {
        match s {
            "ARecord" => Self::A,
            "AAAARecord" => Self::AAAA,
            "TXTRecord" => Self::TXT,
            "CNAMERecord" => Self::CNAME,
            "MXRecord" => Self::MX,
            "NSRecord" => Self::NS,
            "SRVRecord" => Self::SRV,
            "CAARecord" => Self::CAA,
            _ => panic!("Unknown DNS record kind: {s}"),
        }
    }
}

impl From<String> for DNSRecordKind {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl std::fmt::Display for DNSRecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Label selector to match Kubernetes resources.
///
/// A label selector is a label query over a set of resources. The result of matchLabels and
/// matchExpressions are `ANDed`. An empty label selector matches all objects.
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema, PartialEq)]
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
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
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

/// Source for DNS records to include in a zone.
///
/// Specifies how DNS records should be associated with this zone using label selectors.
/// Records matching the selector criteria will be automatically included in the zone.
///
/// # Example
///
/// ```yaml
/// recordsFrom:
///   - selector:
///       matchLabels:
///         app: podinfo
///       matchExpressions:
///         - key: environment
///           operator: In
///           values:
///             - dev
///             - staging
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordSource {
    /// Label selector to match DNS records.
    ///
    /// Records (`ARecord`, `CNAMERecord`, `MXRecord`, etc.) with labels matching this selector
    /// will be automatically associated with this zone.
    ///
    /// The selector uses standard Kubernetes label selector semantics:
    /// - `matchLabels`: All specified labels must match (AND logic)
    /// - `matchExpressions`: All expressions must be satisfied (AND logic)
    /// - Both `matchLabels` and `matchExpressions` can be used together
    pub selector: LabelSelector,
}

/// Source for `Bind9Instance` resources to target for zone configuration.
///
/// Specifies how `Bind9Instance` resources should be selected using label selectors.
/// The `DNSZone` controller will configure zones on all matching instances.
///
/// # Example
///
/// ```yaml
/// instancesFrom:
///   - selector:
///       matchLabels:
///         environment: production
///         role: primary
///       matchExpressions:
///         - key: region
///           operator: In
///           values:
///             - us-east-1
///             - us-west-2
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSource {
    /// Label selector to match `Bind9Instance` resources.
    ///
    /// `Bind9Instance` resources with labels matching this selector will be automatically
    /// targeted for zone configuration by this `DNSZone`.
    ///
    /// The selector uses standard Kubernetes label selector semantics:
    /// - `matchLabels`: All specified labels must match (AND logic)
    /// - `matchExpressions`: All expressions must be satisfied (AND logic)
    /// - Both `matchLabels` and `matchExpressions` can be used together
    pub selector: LabelSelector,
}

impl LabelSelector {
    /// Checks if this label selector matches the given labels.
    ///
    /// Returns `true` if all match requirements are satisfied.
    ///
    /// # Arguments
    ///
    /// * `labels` - The labels to match against (from `metadata.labels`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use bindy::crd::LabelSelector;
    ///
    /// let selector = LabelSelector {
    ///     match_labels: Some(BTreeMap::from([
    ///         ("app".to_string(), "podinfo".to_string()),
    ///     ])),
    ///     match_expressions: None,
    /// };
    ///
    /// let labels = BTreeMap::from([
    ///     ("app".to_string(), "podinfo".to_string()),
    ///     ("env".to_string(), "dev".to_string()),
    /// ]);
    ///
    /// assert!(selector.matches(&labels));
    /// ```
    #[must_use]
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        // Check matchLabels (all must match)
        if let Some(ref match_labels) = self.match_labels {
            for (key, value) in match_labels {
                if labels.get(key) != Some(value) {
                    return false;
                }
            }
        }

        // Check matchExpressions (all must be satisfied)
        if let Some(ref expressions) = self.match_expressions {
            for expr in expressions {
                if !expr.matches(labels) {
                    return false;
                }
            }
        }

        true
    }
}

impl LabelSelectorRequirement {
    /// Checks if this requirement matches the given labels.
    ///
    /// # Arguments
    ///
    /// * `labels` - The labels to match against
    ///
    /// # Returns
    ///
    /// * `true` if the requirement is satisfied, `false` otherwise
    #[must_use]
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        match self.operator.as_str() {
            "In" => {
                // Label value must be in the values list
                if let Some(ref values) = self.values {
                    if let Some(label_value) = labels.get(&self.key) {
                        values.contains(label_value)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "NotIn" => {
                // Label value must NOT be in the values list
                if let Some(ref values) = self.values {
                    if let Some(label_value) = labels.get(&self.key) {
                        !values.contains(label_value)
                    } else {
                        true // Label doesn't exist, so it's not in the list
                    }
                } else {
                    true
                }
            }
            "Exists" => {
                // Label key must exist (any value)
                labels.contains_key(&self.key)
            }
            "DoesNotExist" => {
                // Label key must NOT exist
                !labels.contains_key(&self.key)
            }
            _ => false, // Unknown operator
        }
    }
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

/// Authoritative nameserver configuration for a DNS zone.
///
/// Defines an authoritative nameserver that will have an NS record automatically
/// generated in the zone. Optionally includes IP addresses for glue record generation
/// when the nameserver is within the zone's own domain.
///
/// # Examples
///
/// ## In-zone nameserver with glue records
///
/// ```yaml
/// nameServers:
///   - hostname: ns2.example.com.
///     ipv4Address: "192.0.2.2"
///     ipv6Address: "2001:db8::2"
/// ```
///
/// ## Out-of-zone nameserver (no glue needed)
///
/// ```yaml
/// nameServers:
///   - hostname: ns1.external-provider.net.
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NameServer {
    /// Fully qualified domain name of the nameserver.
    ///
    /// Must end with a dot (.) for FQDN. This nameserver will have an NS record
    /// automatically generated at the zone apex (@).
    ///
    /// Example: `ns2.example.com.`
    #[schemars(regex(
        pattern = r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*\.$"
    ))]
    pub hostname: String,

    /// Optional IPv4 address for glue record generation.
    ///
    /// Required when the nameserver is within the zone's own domain (in-zone delegation).
    /// When provided, an A record will be automatically generated for the nameserver.
    ///
    /// Example: For `ns2.example.com.` in zone `example.com`, provide `"192.0.2.2"`
    ///
    /// Glue records allow resolvers to find the IP addresses of nameservers that are
    /// within the zone they serve, avoiding circular dependencies.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = r"^(?:[0-9]{1,3}\.){3}[0-9]{1,3}$"))]
    pub ipv4_address: Option<String>,

    /// Optional IPv6 address for glue record generation (AAAA record).
    ///
    /// When provided along with (or instead of) `ipv4Address`, an AAAA record will be
    /// automatically generated for the nameserver.
    ///
    /// Example: `"2001:db8::2"`
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(regex(
        pattern = r"^([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}$|^::$|^::1$|^([0-9a-fA-F]{1,4}:){1,7}:$|^([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}$|^([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}$|^([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}$|^([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}$|^([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}$|^[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})$|^:((:[0-9a-fA-F]{1,4}){1,7}|:)$"
    ))]
    pub ipv6_address: Option<String>,
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

/// Reference to a DNS record associated with a zone
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordReference {
    /// API version of the record (e.g., "bindy.firestoned.io/v1beta1")
    pub api_version: String,
    /// Kind of the record (e.g., `ARecord`, `CNAMERecord`, `MXRecord`)
    pub kind: String,
    /// Name of the record resource
    pub name: String,
    /// Namespace of the record resource
    pub namespace: String,
    /// DNS record name from spec.name (e.g., "www", "@", "_service._tcp")
    /// Used for self-healing cleanup when verifying records in BIND9
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_name: Option<String>,
    /// DNS zone name (e.g., "example.com")
    /// Used for self-healing cleanup when querying BIND9
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_name: Option<String>,
}

/// Reference to a DNS record with reconciliation timestamp tracking.
///
/// This struct tracks which records are assigned to a zone and whether
/// they need reconciliation based on the `lastReconciledAt` timestamp.
///
/// **Event-Driven Pattern:**
/// - Records with `lastReconciledAt == None` need reconciliation
/// - Records with `lastReconciledAt == Some(timestamp)` are already configured
///
/// This pattern prevents redundant BIND9 API calls for already-configured records,
/// following the same architecture as `Bind9Instance.status.selectedZones[]`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordReferenceWithTimestamp {
    /// API version of the record (e.g., "bindy.firestoned.io/v1beta1")
    pub api_version: String,
    /// Kind of the record (e.g., "`ARecord`", "`CNAMERecord`", "`MXRecord`")
    pub kind: String,
    /// Name of the record resource
    pub name: String,
    /// Namespace of the record resource
    pub namespace: String,
    /// DNS record name from spec.name (e.g., "www", "@", "_service._tcp")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_name: Option<String>,
    /// Timestamp when this record was last successfully reconciled to BIND9.
    ///
    /// - `None` = Record needs reconciliation (new or spec changed)
    /// - `Some(timestamp)` = Record already configured, skip reconciliation
    ///
    /// This field is set by the record operator after successful BIND9 update.
    /// The zone controller resets it to `None` when spec changes or zone is recreated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reconciled_at: Option<k8s_openapi::apimachinery::pkg::apis::meta::v1::Time>,
}

/// Status of a `Bind9Instance` relationship with a `DNSZone`.
///
/// Tracks the lifecycle of zone assignment from initial selection through configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "PascalCase")]
pub enum InstanceStatus {
    /// Zone has selected this instance via bind9InstancesFrom, but zone not yet configured
    Claimed,
    /// Zone successfully configured on instance
    Configured,
    /// Zone configuration failed on instance
    Failed,
    /// Instance no longer selected by this zone (cleanup pending)
    Unclaimed,
}

impl InstanceStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            InstanceStatus::Claimed => "Claimed",
            InstanceStatus::Configured => "Configured",
            InstanceStatus::Failed => "Failed",
            InstanceStatus::Unclaimed => "Unclaimed",
        }
    }
}

/// Reference to a `Bind9Instance` with status and timestamp.
///
/// Extends `InstanceReference` with status tracking for zone claiming and configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct InstanceReferenceWithStatus {
    /// API version of the `Bind9Instance` resource
    pub api_version: String,
    /// Kind of the resource (always "`Bind9Instance`")
    pub kind: String,
    /// Name of the `Bind9Instance` resource
    pub name: String,
    /// Namespace of the `Bind9Instance` resource
    pub namespace: String,
    /// Current status of this instance's relationship with the zone
    pub status: InstanceStatus,
    /// Timestamp when the instance status was last reconciled for this zone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reconciled_at: Option<String>,
    /// Additional message (for Failed status, error details, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// `DNSZone` status
#[derive(Clone, Debug, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DNSZoneStatus {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    /// Count of records selected by recordsFrom label selectors.
    ///
    /// This field is automatically calculated from the length of `records`.
    /// It provides a quick view of how many records are associated with this zone.
    ///
    /// Defaults to 0 when no records are selected.
    #[serde(default)]
    pub records_count: i32,
    /// List of DNS records selected by recordsFrom label selectors.
    ///
    /// **Event-Driven Pattern:**
    /// - Records with `lastReconciledAt == None` need reconciliation
    /// - Records with `lastReconciledAt == Some(timestamp)` are already configured
    ///
    /// This field is populated by the `DNSZone` controller when evaluating `recordsFrom` selectors.
    /// The timestamp is set by the record operator after successful BIND9 update.
    ///
    /// **Single Source of Truth:**
    /// This status field is authoritative for which records belong to this zone and whether
    /// they need reconciliation, preventing redundant BIND9 API calls.
    #[serde(default)]
    pub records: Vec<RecordReferenceWithTimestamp>,
    /// List of `Bind9Instance` resources and their status for this zone.
    ///
    /// **Single Source of Truth for Instance-Zone Relationships:**
    /// This field tracks all `Bind9Instances` selected by this zone via `bind9InstancesFrom` selectors,
    /// along with the current status of zone configuration on each instance.
    ///
    /// **Status Lifecycle:**
    /// - `Claimed`: Zone selected this instance (via `bind9InstancesFrom`), waiting for configuration
    /// - `Configured`: Zone successfully configured on instance
    /// - `Failed`: Zone configuration failed on instance
    /// - `Unclaimed`: Instance no longer selected by this zone (cleanup pending)
    ///
    /// **Event-Driven Pattern:**
    /// - `DNSZone` controller evaluates `bind9InstancesFrom` selectors to find matching instances
    /// - `DNSZone` controller reads this field to track configuration status
    /// - `DNSZone` controller updates status after configuration attempts
    ///
    /// **Automatic Selection:**
    /// When a `DNSZone` reconciles, the controller automatically:
    /// 1. Queries all `Bind9Instances` matching `bind9InstancesFrom` selectors
    /// 2. Adds them to this list with status="Claimed"
    /// 3. Configures zones on each instance
    ///
    /// # Example
    ///
    /// ```yaml
    /// status:
    ///   bind9Instances:
    ///     - apiVersion: bindy.firestoned.io/v1beta1
    ///       kind: Bind9Instance
    ///       name: primary-dns-0
    ///       namespace: dns-system
    ///       status: Configured
    ///       lastReconciledAt: "2026-01-03T20:00:00Z"
    ///     - apiVersion: bindy.firestoned.io/v1beta1
    ///       kind: Bind9Instance
    ///       name: secondary-dns-0
    ///       namespace: dns-system
    ///       status: Claimed
    ///       lastReconciledAt: "2026-01-03T20:01:00Z"
    /// ```
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bind9_instances: Vec<InstanceReferenceWithStatus>,
    /// Number of `Bind9Instance` resources in the `bind9_instances` list.
    ///
    /// This field is automatically updated whenever the `bind9_instances` list changes.
    /// It provides a quick view of how many instances are serving this zone without
    /// requiring clients to count array elements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind9_instances_count: Option<i32>,

    /// DNSSEC signing status for this zone
    ///
    /// Populated when DNSSEC signing is enabled. Contains DS records,
    /// key tags, and rotation information.
    ///
    /// **Important**: DS records must be published in the parent zone
    /// to complete the DNSSEC chain of trust.
    ///
    /// # Example
    ///
    /// ```yaml
    /// dnssec:
    ///   signed: true
    ///   dsRecords:
    ///     - "example.com. IN DS 12345 13 2 ABC123..."
    ///   keyTag: 12345
    ///   algorithm: "ECDSAP256SHA256"
    ///   nextKeyRollover: "2026-04-02T00:00:00Z"
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dnssec: Option<DNSSECStatus>,
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
/// apiVersion: bindy.firestoned.io/v1beta1
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
/// apiVersion: bindy.firestoned.io/v1beta1
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
    version = "v1beta1",
    kind = "DNSZone",
    namespaced,
    shortname = "zone",
    shortname = "zones",
    shortname = "dz",
    shortname = "dzs",
    doc = "DNSZone represents an authoritative DNS zone managed by BIND9. Each DNSZone defines a zone (e.g., example.com) with SOA record parameters. Can reference either a namespace-scoped Bind9Cluster or cluster-scoped ClusterBind9Provider.",
    printcolumn = r#"{"name":"Zone","type":"string","jsonPath":".spec.zoneName"}"#,
    printcolumn = r#"{"name":"Provider","type":"string","jsonPath":".spec.clusterProviderRef"}"#,
    printcolumn = r#"{"name":"Records","type":"integer","jsonPath":".status.recordsCount"}"#,
    printcolumn = r#"{"name":"Instances","type":"integer","jsonPath":".status.bind9InstancesCount"}"#,
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

    /// Reference to a `Bind9Cluster` or `ClusterBind9Provider` to serve this zone.
    ///
    /// When specified, this zone will be automatically configured on all `Bind9Instance`
    /// resources that belong to the referenced cluster. This provides a simple way to
    /// assign zones to entire clusters.
    ///
    /// **Relationship with `bind9_instances_from`:**
    /// - If only `cluster_ref` is specified: Zone targets all instances in that cluster
    /// - If only `bind9_instances_from` is specified: Zone targets instances matching label selectors
    /// - If both are specified: Zone targets union of cluster instances AND label-selected instances
    ///
    /// # Example
    ///
    /// ```yaml
    /// spec:
    ///   clusterRef: production-dns  # Target all instances in this cluster
    ///   zoneName: example.com
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_ref: Option<String>,

    /// Authoritative nameservers for this zone (v0.4.0+).
    ///
    /// NS records are automatically generated at the zone apex (@) for all entries.
    /// The primary nameserver from `soaRecord.primaryNs` is always included automatically.
    ///
    /// Each entry can optionally include IP addresses to generate glue records (A/AAAA)
    /// for in-zone nameservers. Glue records are required when the nameserver is within
    /// the zone's own domain to avoid circular dependencies.
    ///
    /// # Examples
    ///
    /// ```yaml
    /// # In-zone nameservers with glue records
    /// nameServers:
    ///   - hostname: ns2.example.com.
    ///     ipv4Address: "192.0.2.2"
    ///   - hostname: ns3.example.com.
    ///     ipv4Address: "192.0.2.3"
    ///     ipv6Address: "2001:db8::3"
    ///
    /// # Out-of-zone nameserver (no glue needed)
    ///   - hostname: ns4.external-provider.net.
    /// ```
    ///
    /// **Generated Records:**
    /// - `@ IN NS ns2.example.com.` (NS record)
    /// - `ns2.example.com. IN A 192.0.2.2` (glue record for in-zone NS)
    /// - `@ IN NS ns3.example.com.` (NS record)
    /// - `ns3.example.com. IN A 192.0.2.3` (IPv4 glue)
    /// - `ns3.example.com. IN AAAA 2001:db8::3` (IPv6 glue)
    /// - `@ IN NS ns4.external-provider.net.` (NS record only, no glue)
    ///
    /// **Benefits over `nameServerIps` (deprecated):**
    /// - Clearer purpose: authoritative nameservers, not just glue records
    /// - IPv6 support via `ipv6Address` field
    /// - Automatic NS record generation (no manual `NSRecord` CRs needed)
    ///
    /// **Migration:** See [docs/src/operations/migration-guide.md](../operations/migration-guide.md)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_servers: Option<Vec<NameServer>>,

    /// (DEPRECATED in v0.4.0) Map of nameserver hostnames to IP addresses for glue records.
    ///
    /// **Use `nameServers` instead.** This field will be removed in v1.0.0.
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
    ///
    /// **Migration to `nameServers`:**
    /// ```yaml
    /// # Old (deprecated):
    /// nameServerIps:
    ///   ns2.example.com.: "192.0.2.2"
    ///
    /// # New (recommended):
    /// nameServers:
    ///   - hostname: ns2.example.com.
    ///     ipv4Address: "192.0.2.2"
    /// ```
    #[deprecated(
        since = "0.4.0",
        note = "Use `name_servers` instead. This field will be removed in v1.0.0. See migration guide at docs/src/operations/migration-guide.md"
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_server_ips: Option<HashMap<String, String>>,

    /// Sources for DNS records to include in this zone.
    ///
    /// This field defines label selectors that automatically associate DNS records with this zone.
    /// Records with matching labels will be included in the zone's DNS configuration.
    ///
    /// This follows the standard Kubernetes selector pattern used by Services, `NetworkPolicies`,
    /// and other resources for declarative resource association.
    ///
    /// # Example: Match podinfo records in dev/staging environments
    ///
    /// ```yaml
    /// recordsFrom:
    ///   - selector:
    ///       matchLabels:
    ///         app: podinfo
    ///       matchExpressions:
    ///         - key: environment
    ///           operator: In
    ///           values:
    ///             - dev
    ///             - staging
    /// ```
    ///
    /// # Selector Operators
    ///
    /// - **In**: Label value must be in the specified values list
    /// - **`NotIn`**: Label value must NOT be in the specified values list
    /// - **Exists**: Label key must exist (any value)
    /// - **`DoesNotExist`**: Label key must NOT exist
    ///
    /// # Use Cases
    ///
    /// - **Multi-environment zones**: Dynamically include records based on environment labels
    /// - **Application-specific zones**: Group all records for an application using `app` label
    /// - **Team-based zones**: Use team labels to automatically route records to team-owned zones
    /// - **Temporary records**: Use labels to include/exclude records without changing `zoneRef`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records_from: Option<Vec<RecordSource>>,

    /// Select `Bind9Instance` resources to target for zone configuration using label selectors.
    ///
    /// This field enables dynamic, label-based selection of DNS instances to serve this zone.
    /// Instances matching these selectors will automatically receive zone configuration from
    /// the `DNSZone` controller.
    ///
    /// This follows the standard Kubernetes selector pattern used by Services, `NetworkPolicies`,
    /// and other resources for declarative resource association.
    ///
    /// **IMPORTANT**: This is the **preferred** method for zone-instance association. It provides:
    /// - **Decoupled Architecture**: Zones select instances, not vice versa
    /// - **Zone Ownership**: Zone authors control which instances serve their zones
    /// - **Dynamic Scaling**: New instances matching labels automatically pick up zones
    /// - **Multi-Tenancy**: Zones can target specific instance groups (prod, staging, team-specific)
    ///
    /// # Example: Target production primary instances
    ///
    /// ```yaml
    /// apiVersion: bindy.firestoned.io/v1beta1
    /// kind: DNSZone
    /// metadata:
    ///   name: example-com
    ///   namespace: dns-system
    /// spec:
    ///   zoneName: example.com
    ///   bind9InstancesFrom:
    ///     - selector:
    ///         matchLabels:
    ///           environment: production
    ///           bindy.firestoned.io/role: primary
    /// ```
    ///
    /// # Example: Target instances by region and tier
    ///
    /// ```yaml
    /// bind9InstancesFrom:
    ///   - selector:
    ///       matchLabels:
    ///         tier: frontend
    ///       atchExpressions:
    ///         - key: region
    ///           operator: In
    ///           values:
    ///             - us-east-1
    ///             - us-west-2
    /// ```
    ///
    /// # Selector Operators
    ///
    /// - **In**: Label value must be in the specified values list
    /// - **`NotIn`**: Label value must NOT be in the specified values list
    /// - **Exists**: Label key must exist (any value)
    /// - **`DoesNotExist`**: Label key must NOT exist
    ///
    /// # Use Cases
    ///
    /// - **Environment Isolation**: Target only production instances (`environment: production`)
    /// - **Role-Based Selection**: Select only primary or secondary instances
    /// - **Geographic Distribution**: Target instances in specific regions
    /// - **Team Boundaries**: Select instances managed by specific teams
    /// - **Testing Zones**: Target staging instances for non-production zones
    ///
    /// # Relationship with `clusterRef`
    ///
    /// - **`clusterRef`**: Explicitly assigns zone to ALL instances in a cluster
    /// - **`bind9InstancesFrom`**: Dynamically selects specific instances using labels (more flexible)
    ///
    /// You can use both approaches together - the zone will target the **union** of:
    /// - All instances in `clusterRef` cluster
    /// - Plus any additional instances matching `bind9InstancesFrom` selectors
    ///
    /// # Event-Driven Architecture
    ///
    /// The `DNSZone` controller watches both `DNSZone` and `Bind9Instance` resources.
    /// When labels change on either:
    /// 1. Controller re-evaluates label selector matching
    /// 2. Automatically configures zones on newly-matched instances
    /// 3. Removes zone configuration from instances that no longer match
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind9_instances_from: Option<Vec<InstanceSource>>,

    /// Override DNSSEC policy for this zone
    ///
    /// Allows per-zone override of the cluster's global DNSSEC signing policy.
    /// If not specified, the zone inherits the DNSSEC configuration from the
    /// cluster's `global.dnssec.signing.policy`.
    ///
    /// Use this to:
    /// - Disable signing for specific zones in a signing-enabled cluster
    /// - Use stricter security policies for sensitive zones
    /// - Test different signing algorithms on specific zones
    ///
    /// # Example: Custom High-Security Policy
    ///
    /// ```yaml
    /// apiVersion: bindy.firestoned.io/v1beta1
    /// kind: DNSZone
    /// metadata:
    ///   name: secure-zone
    /// spec:
    ///   zoneName: secure.example.com
    ///   clusterRef: production-dns
    ///   dnssecPolicy: "high-security"  # Override cluster default
    /// ```
    ///
    /// # Example: Disable Signing for One Zone
    ///
    /// ```yaml
    /// dnssecPolicy: "none"  # Disable signing (cluster has signing enabled)
    /// ```
    ///
    /// **Note**: Custom policies require BIND9 `dnssec-policy` configuration.
    /// Built-in policies: `"default"`, `"none"`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dnssec_policy: Option<String>,
}

/// `ARecord` maps a DNS name to an IPv4 address.
///
/// A records are the most common DNS record type, mapping hostnames to IPv4 addresses.
/// Multiple A records can exist for the same name (round-robin DNS).
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1beta1
/// kind: ARecord
/// metadata:
///   name: www-example-com
///   namespace: dns-system
///   labels:
///     zone: example.com
/// spec:
///   name: www
///   ipv4Address: 192.0.2.1
///   ttl: 300
/// ```
///
/// Records are associated with `DNSZones` via label selectors.
/// The `DNSZone` must have a `recordsFrom` selector that matches this record's labels.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "ARecord",
    namespaced,
    shortname = "a",
    doc = "ARecord maps a DNS hostname to an IPv4 address. Multiple A records for the same name enable round-robin DNS load balancing.",
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"Addresses","type":"string","jsonPath":".spec.ipv4Addresses"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct ARecordSpec {
    /// Record name within the zone. Use "@" for the zone apex.
    ///
    /// Examples: "www", "mail", "ftp", "@"
    /// The full DNS name will be: {name}.{zone}
    pub name: String,

    /// List of IPv4 addresses for this DNS record.
    ///
    /// Multiple addresses create round-robin DNS (load balancing).
    /// All addresses in the list belong to the same DNS name.
    ///
    /// Must contain at least one valid IPv4 address in dotted-decimal notation.
    ///
    /// Examples: `["192.0.2.1"]`, `["192.0.2.1", "192.0.2.2", "192.0.2.3"]`
    #[schemars(length(min = 1))]
    pub ipv4_addresses: Vec<String>,

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
/// apiVersion: bindy.firestoned.io/v1beta1
/// kind: AAAARecord
/// metadata:
///   name: www-example-com-ipv6
///   namespace: dns-system
///   labels:
///     zone: example.com
/// spec:
///   name: www
///   ipv6Address: "2001:db8::1"
///   ttl: 300
/// ```
///
/// Records are associated with `DNSZones` via label selectors.
/// The `DNSZone` must have a `recordsFrom` selector that matches this record's labels.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "AAAARecord",
    namespaced,
    shortname = "aaaa",
    doc = "AAAARecord maps a DNS hostname to an IPv6 address. This is the IPv6 equivalent of an A record.",
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"Addresses","type":"string","jsonPath":".spec.ipv6Addresses"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct AAAARecordSpec {
    /// Record name within the zone.
    pub name: String,

    /// List of IPv6 addresses for this DNS record.
    ///
    /// Multiple addresses create round-robin DNS (load balancing).
    /// All addresses in the list belong to the same DNS name.
    ///
    /// Must contain at least one valid IPv6 address in standard notation.
    ///
    /// Examples: `["2001:db8::1"]`, `["2001:db8::1", "2001:db8::2"]`
    #[schemars(length(min = 1))]
    pub ipv6_addresses: Vec<String>,

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
/// apiVersion: bindy.firestoned.io/v1beta1
/// kind: TXTRecord
/// metadata:
///   name: spf-example-com
///   namespace: dns-system
///   labels:
///     zone: example.com
/// spec:
///   name: "@"
///   text:
///     - "v=spf1 include:_spf.google.com ~all"
///   ttl: 3600
/// ```
///
/// Records are associated with `DNSZones` via label selectors.
/// The `DNSZone` must have a `recordsFrom` selector that matches this record's labels.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "TXTRecord",
    namespaced,
    shortname = "txt",
    doc = "TXTRecord stores arbitrary text data in DNS. Commonly used for SPF, DKIM, DMARC policies, and domain verification.",
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct TXTRecordSpec {
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
/// apiVersion: bindy.firestoned.io/v1beta1
/// kind: CNAMERecord
/// metadata:
///   name: blog-example-com
///   namespace: dns-system
///   labels:
///     zone: example.com
/// spec:
///   name: blog
///   target: example.github.io.
///   ttl: 3600
/// ```
///
/// Records are associated with `DNSZones` via label selectors.
/// The `DNSZone` must have a `recordsFrom` selector that matches this record's labels.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "CNAMERecord",
    namespaced,
    shortname = "cname",
    doc = "CNAMERecord creates a DNS alias from one hostname to another. A CNAME cannot coexist with other record types for the same name.",
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct CNAMERecordSpec {
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
/// apiVersion: bindy.firestoned.io/v1beta1
/// kind: MXRecord
/// metadata:
///   name: mail-example-com
///   namespace: dns-system
///   labels:
///     zone: example.com
/// spec:
///   name: "@"
///   priority: 10
///   mailServer: mail.example.com.
///   ttl: 3600
/// ```
///
/// Records are associated with `DNSZones` via label selectors.
/// The `DNSZone` must have a `recordsFrom` selector that matches this record's labels.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "MXRecord",
    namespaced,
    shortname = "mx",
    doc = "MXRecord specifies mail exchange servers for a domain. Lower priority values indicate higher preference for mail delivery.",
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct MXRecordSpec {
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
/// apiVersion: bindy.firestoned.io/v1beta1
/// kind: NSRecord
/// metadata:
///   name: subdomain-ns
///   namespace: dns-system
///   labels:
///     zone: example.com
/// spec:
///   name: subdomain
///   nameserver: ns1.other-provider.com.
///   ttl: 86400
/// ```
///
/// Records are associated with `DNSZones` via label selectors.
/// The `DNSZone` must have a `recordsFrom` selector that matches this record's labels.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "NSRecord",
    namespaced,
    shortname = "ns",
    doc = "NSRecord delegates a subdomain to authoritative nameservers. Used for subdomain delegation to different DNS providers or servers.",
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct NSRecordSpec {
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
/// apiVersion: bindy.firestoned.io/v1beta1
/// kind: SRVRecord
/// metadata:
///   name: ldap-srv
///   namespace: dns-system
///   labels:
///     zone: example.com
/// spec:
///   name: _ldap._tcp
///   priority: 10
///   weight: 60
///   port: 389
///   target: ldap.example.com.
///   ttl: 3600
/// ```
///
/// Records are associated with `DNSZones` via label selectors.
/// The `DNSZone` must have a `recordsFrom` selector that matches this record's labels.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "SRVRecord",
    namespaced,
    shortname = "srv",
    doc = "SRVRecord specifies the hostname and port of servers for specific services. The record name follows the format _service._proto (e.g., _ldap._tcp).",
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct SRVRecordSpec {
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
/// apiVersion: bindy.firestoned.io/v1beta1
/// kind: CAARecord
/// metadata:
///   name: caa-letsencrypt
///   namespace: dns-system
///   labels:
///     zone: example.com
/// spec:
///   name: "@"
///   flags: 0
///   tag: issue
///   value: letsencrypt.org
///   ttl: 86400
/// ```
///
/// Records are associated with `DNSZones` via label selectors.
/// The `DNSZone` must have a `recordsFrom` selector that matches this record's labels.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "CAARecord",
    namespaced,
    shortname = "caa",
    doc = "CAARecord specifies which certificate authorities are authorized to issue certificates for a domain. Enhances domain security and certificate issuance control.",
    printcolumn = r#"{"name":"Name","type":"string","jsonPath":".spec.name"}"#,
    printcolumn = r#"{"name":"TTL","type":"integer","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[kube(status = "RecordStatus")]
#[serde(rename_all = "camelCase")]
pub struct CAARecordSpec {
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
    /// The FQDN of the zone that owns this record (set by `DNSZone` controller).
    ///
    /// When a `DNSZone`'s label selector matches this record, the `DNSZone` controller
    /// sets this field to the zone's FQDN (e.g., `"example.com"`). The record reconciler
    /// uses this to determine which zone to update in BIND9.
    ///
    /// If this field is empty, the record is not matched by any zone and should not
    /// be reconciled into BIND9.
    ///
    /// **DEPRECATED**: Use `zone_ref` instead for structured zone reference.
    #[deprecated(
        since = "0.2.0",
        note = "Use zone_ref instead for structured zone reference"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// Structured reference to the `DNSZone` that owns this record.
    ///
    /// Set by the `DNSZone` controller when the zone's `recordsFrom` selector matches
    /// this record's labels. Contains the complete Kubernetes object reference including
    /// apiVersion, kind, name, namespace, and zoneName.
    ///
    /// The record reconciler uses this to:
    /// 1. Look up the parent `DNSZone` resource
    /// 2. Find the zone's primary `Bind9Instance` servers
    /// 3. Add this record to BIND9 on primaries
    /// 4. Trigger zone transfer (retransfer) on secondaries
    ///
    /// If this field is None, the record is not selected by any zone and will not
    /// be added to BIND9.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_ref: Option<ZoneReference>,
    /// SHA-256 hash of the record's spec data.
    ///
    /// Used to detect when a record's data has actually changed, avoiding
    /// unnecessary BIND9 updates and zone transfers.
    ///
    /// The hash is calculated from all fields in the record's spec that affect
    /// the DNS record data (name, addresses, TTL, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_hash: Option<String>,
    /// Timestamp of the last successful update to BIND9.
    ///
    /// This is updated after a successful nsupdate operation.
    /// Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
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
/// apiVersion: bindy.firestoned.io/v1beta1
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

fn default_rotate_after() -> String {
    crate::constants::DEFAULT_ROTATION_INTERVAL.to_string()
}

fn default_secret_type() -> String {
    "Opaque".to_string()
}

/// RNDC key lifecycle configuration with automatic rotation support.
///
/// Provides three configuration modes:
/// 1. **Auto-generated with optional rotation** (default) - Operator creates and manages keys
/// 2. **Reference to existing Secret** - Use pre-existing Kubernetes Secret (no rotation)
/// 3. **Inline Secret specification** - Define Secret inline with optional rotation
///
/// When `auto_rotate` is enabled, the operator automatically rotates keys after the
/// `rotate_after` duration has elapsed. Rotation timestamps are tracked in Secret annotations.
///
/// # Examples
///
/// ```yaml
/// # Auto-generated with 30-day rotation
/// rndcKeys:
///   autoRotate: true
///   rotateAfter: 720h
///   algorithm: hmac-sha256
///
/// # Reference existing Secret (no rotation)
/// rndcKeys:
///   secretRef:
///     name: my-rndc-key
///     algorithm: hmac-sha256
///
/// # Inline Secret with rotation
/// rndcKeys:
///   autoRotate: true
///   rotateAfter: 2160h  # 90 days
///   secret:
///     metadata:
///       name: custom-rndc-key
///       labels:
///         app: bindy
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RndcKeyConfig {
    /// Enable automatic key rotation (default: false for backward compatibility).
    ///
    /// When `true`, the operator automatically rotates the RNDC key after the
    /// `rotate_after` interval. When `false`, keys are generated once and never rotated.
    ///
    /// **Important**: Rotation only applies to operator-managed Secrets. If you
    /// specify `secret_ref`, that Secret will NOT be rotated automatically.
    ///
    /// Default: `false`
    #[serde(default)]
    pub auto_rotate: bool,

    /// Duration after which to rotate the key (Go duration format: "720h", "30d").
    ///
    /// Supported units:
    /// - `h` (hours): "720h" = 30 days
    /// - `d` (days): "30d" = 30 days
    /// - `w` (weeks): "4w" = 28 days
    ///
    /// Constraints:
    /// - Minimum: 1h (1 hour)
    /// - Maximum: 8760h (365 days / 1 year)
    ///
    /// Only applies when `auto_rotate` is `true`.
    ///
    /// Default: `"720h"` (30 days)
    #[serde(default = "default_rotate_after")]
    pub rotate_after: String,

    /// Reference to an existing Kubernetes Secret containing RNDC credentials.
    ///
    /// When specified, the operator uses this existing Secret instead of auto-generating
    /// one. The Secret must contain the `rndc.key` field with BIND9 key file content.
    ///
    /// **Mutually exclusive with `secret`** - if both are specified, `secret_ref` takes
    /// precedence and `secret` is ignored.
    ///
    /// **Rotation note**: User-managed Secrets (via `secret_ref`) are NOT automatically
    /// rotated even if `auto_rotate` is `true`. You must rotate these manually.
    ///
    /// Default: `None` (auto-generate key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<RndcSecretRef>,

    /// Inline Secret specification for operator-managed Secret with optional rotation.
    ///
    /// Embeds a full Kubernetes Secret specification. The operator will create and
    /// manage this Secret, and rotate it if `auto_rotate` is `true`.
    ///
    /// **Mutually exclusive with `secret_ref`** - if both are specified, `secret_ref`
    /// takes precedence and this field is ignored.
    ///
    /// Default: `None` (auto-generate key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<SecretSpec>,

    /// HMAC algorithm for the RNDC key.
    ///
    /// Only used when auto-generating keys (when neither `secret_ref` nor `secret` are
    /// specified). If using `secret_ref`, the algorithm is specified in that reference.
    ///
    /// Default: `hmac-sha256`
    #[serde(default)]
    pub algorithm: RndcAlgorithm,
}

/// Kubernetes Secret specification for inline Secret creation.
///
/// Used when the operator should create and manage the Secret (with optional rotation).
/// This is a subset of the Kubernetes Secret API focusing on fields relevant for
/// RNDC key management.
///
/// # Example
///
/// ```yaml
/// secret:
///   metadata:
///     name: my-rndc-key
///     labels:
///       app: bindy
///       tier: infrastructure
///   stringData:
///     rndc.key: |
///       key "bindy-operator" {
///           algorithm hmac-sha256;
///           secret "dGVzdHNlY3JldA==";
///       };
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretSpec {
    /// Secret metadata (name, labels, annotations).
    ///
    /// **Required**: You must specify `metadata.name` for the Secret name.
    pub metadata: SecretMetadata,

    /// Secret type (default: "Opaque").
    ///
    /// For RNDC keys, use the default "Opaque" type.
    #[serde(default = "default_secret_type")]
    #[serde(rename = "type")]
    pub type_: String,

    /// String data (keys and values as strings).
    ///
    /// For RNDC keys, you should provide:
    /// - `rndc.key`: Full BIND9 key file content (required by BIND9)
    ///
    /// Optional metadata (auto-populated by operator if omitted):
    /// - `key-name`: Name of the TSIG key
    /// - `algorithm`: HMAC algorithm
    /// - `secret`: Base64-encoded key material
    ///
    /// Kubernetes automatically base64-encodes string data when creating the Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_data: Option<std::collections::BTreeMap<String, String>>,

    /// Binary data (keys and values as base64 strings).
    ///
    /// Alternative to `string_data` if you want to provide already-base64-encoded values.
    /// Most users should use `string_data` instead for readability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<std::collections::BTreeMap<String, String>>,
}

/// Minimal Secret metadata for inline Secret specifications.
///
/// This is a subset of Kubernetes `ObjectMeta` focusing on commonly-used fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretMetadata {
    /// Secret name (required).
    ///
    /// Must be a valid Kubernetes resource name (lowercase alphanumeric, hyphens, dots).
    pub name: String,

    /// Labels to apply to the Secret.
    ///
    /// Useful for organizing and selecting Secrets via label selectors.
    ///
    /// Example:
    /// ```yaml
    /// labels:
    ///   app: bindy
    ///   tier: infrastructure
    ///   environment: production
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<std::collections::BTreeMap<String, String>>,

    /// Annotations to apply to the Secret.
    ///
    /// **Note**: The operator will add rotation tracking annotations:
    /// - `bindy.firestoned.io/rndc-created-at` - Key creation timestamp
    /// - `bindy.firestoned.io/rndc-rotate-at` - Next rotation timestamp
    /// - `bindy.firestoned.io/rndc-rotation-count` - Number of rotations
    ///
    /// Do not manually set these rotation tracking annotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<std::collections::BTreeMap<String, String>>,
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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
/// This configuration supports both DNSSEC validation (verifying signatures from upstream)
/// and DNSSEC signing (cryptographically signing your own zones).
///
/// # Example
///
/// ```yaml
/// dnssec:
///   validation: true  # Validate upstream DNSSEC responses
///   signing:
///     enabled: true
///     policy: "default"
///     algorithm: "ECDSAP256SHA256"
///     kskLifetime: "365d"
///     zskLifetime: "90d"
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

    /// Enable DNSSEC zone signing configuration
    ///
    /// Configures automatic DNSSEC signing for zones served by this cluster.
    /// When enabled, BIND9 will automatically generate keys, sign zones, and
    /// rotate keys based on the configured policy.
    ///
    /// **Important**: Requires BIND 9.16+ for modern `dnssec-policy` support.
    #[serde(default)]
    pub signing: Option<DNSSECSigningConfig>,
}

/// DNSSEC zone signing configuration
///
/// Configures automatic DNSSEC key generation, zone signing, and key rotation.
/// Uses BIND9's modern `dnssec-policy` for declarative key management.
///
/// # Key Management Options
///
/// 1. **User-Supplied Keys** (Production): Keys managed externally via Secrets
/// 2. **Auto-Generated Keys** (Dev/Test): BIND9 generates keys, operator backs up to Secrets
/// 3. **Persistent Storage** (Legacy): Keys stored in `PersistentVolume`
///
/// # Example
///
/// ```yaml
/// signing:
///   enabled: true
///   policy: "default"
///   algorithm: "ECDSAP256SHA256"
///   kskLifetime: "365d"
///   zskLifetime: "90d"
///   nsec3: true
///   nsec3Iterations: 0
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DNSSECSigningConfig {
    /// Enable DNSSEC signing for zones
    ///
    /// When true, zones will be automatically signed with DNSSEC.
    /// Keys are generated and managed according to the configured policy.
    ///
    /// Default: `false`
    #[serde(default)]
    pub enabled: bool,

    /// DNSSEC policy name
    ///
    /// Name of the DNSSEC policy to apply. Built-in policies:
    /// - `"default"` - Standard policy with ECDSA P-256, 365d KSK, 90d ZSK
    ///
    /// Custom policies can be defined in future enhancements.
    ///
    /// Default: `"default"`
    #[serde(default)]
    pub policy: Option<String>,

    /// DNSSEC algorithm
    ///
    /// Cryptographic algorithm for DNSSEC signing. Supported algorithms:
    /// - `"ECDSAP256SHA256"` (13) - ECDSA P-256 with SHA-256 (recommended, fast)
    /// - `"ECDSAP384SHA384"` (14) - ECDSA P-384 with SHA-384 (higher security)
    /// - `"RSASHA256"` (8) - RSA with SHA-256 (widely compatible)
    ///
    /// ECDSA algorithms are recommended for performance and smaller key sizes.
    ///
    /// Default: `"ECDSAP256SHA256"`
    #[serde(default)]
    pub algorithm: Option<String>,

    /// Key Signing Key (KSK) lifetime
    ///
    /// Duration before KSK is rotated. Format: "365d", "1y", "8760h"
    ///
    /// KSK signs the `DNSKEY` `RRset` and is published in the parent zone as a `DS` record.
    /// Longer lifetimes reduce `DS` update frequency but increase impact of key compromise.
    ///
    /// Default: `"365d"` (1 year)
    #[serde(default)]
    pub ksk_lifetime: Option<String>,

    /// Zone Signing Key (ZSK) lifetime
    ///
    /// Duration before ZSK is rotated. Format: "90d", "3m", "2160h"
    ///
    /// ZSK signs all other records in the zone. Shorter lifetimes improve security
    /// but increase signing overhead.
    ///
    /// Default: `"90d"` (3 months)
    #[serde(default)]
    pub zsk_lifetime: Option<String>,

    /// Use NSEC3 instead of NSEC for authenticated denial of existence
    ///
    /// NSEC3 hashes zone names to prevent zone enumeration attacks.
    /// Recommended for privacy-sensitive zones.
    ///
    /// Default: `false` (use NSEC)
    #[serde(default)]
    pub nsec3: Option<bool>,

    /// NSEC3 salt (hex string)
    ///
    /// Salt value for NSEC3 hashing. If not specified, BIND9 auto-generates.
    /// Format: hex string (e.g., "AABBCCDD")
    ///
    /// Default: Auto-generated by BIND9
    #[serde(default)]
    pub nsec3_salt: Option<String>,

    /// NSEC3 iterations
    ///
    /// Number of hash iterations for NSEC3. RFC 9276 recommends 0 for performance.
    ///
    /// **Important**: Higher values significantly impact query performance.
    ///
    /// Default: `0` (per RFC 9276 recommendation)
    #[serde(default)]
    pub nsec3_iterations: Option<u32>,

    /// DNSSEC key source configuration
    ///
    /// Specifies where DNSSEC keys come from:
    /// - User-supplied Secret (recommended for production)
    /// - Persistent storage (legacy)
    ///
    /// If not specified and `auto_generate` is true, keys are generated in emptyDir
    /// and optionally backed up to Secrets.
    #[serde(default)]
    pub keys_from: Option<DNSSECKeySource>,

    /// Auto-generate DNSSEC keys if no `keys_from` specified
    ///
    /// When true, BIND9 generates keys automatically using the configured policy.
    /// Recommended for development and testing.
    ///
    /// Default: `true`
    #[serde(default)]
    pub auto_generate: Option<bool>,

    /// Export auto-generated keys to Secret for backup/restore
    ///
    /// When true, operator exports BIND9-generated keys to a Kubernetes Secret.
    /// Enables self-healing: keys are restored from Secret on pod restart.
    ///
    /// Secret name format: `dnssec-keys-<zone-name>-generated`
    ///
    /// Default: `true`
    #[serde(default)]
    pub export_to_secret: Option<bool>,
}

/// DNSSEC key source configuration
///
/// Defines where DNSSEC keys are loaded from. Supports multiple patterns:
///
/// 1. **User-Supplied Secret** (Production):
///    - Keys managed externally (`Vault`, `ExternalSecrets`, `sealed-secrets`)
///    - User controls rotation timing
///    - `GitOps` friendly
///
/// 2. **Persistent Storage** (Legacy):
///    - Keys stored in `PersistentVolume`
///    - Traditional BIND9 pattern
///
/// # Example: User-Supplied Keys
///
/// ```yaml
/// keysFrom:
///   secretRef:
///     name: "dnssec-keys-example-com"
/// ```
///
/// # Example: Persistent Storage
///
/// ```yaml
/// keysFrom:
///   persistentVolume:
///     accessModes:
///       - ReadWriteOnce
///     resources:
///       requests:
///         storage: 100Mi
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DNSSECKeySource {
    /// Secret containing DNSSEC keys
    ///
    /// Reference to a Kubernetes Secret with DNSSEC key files.
    ///
    /// Secret data format:
    /// - `K<zone>.+<alg>+<tag>.key` - Public key file
    /// - `K<zone>.+<alg>+<tag>.private` - Private key file
    ///
    /// Example: `Kexample.com.+013+12345.key`
    #[serde(default)]
    pub secret_ref: Option<SecretReference>,

    /// Persistent volume for DNSSEC keys (legacy/compatibility)
    ///
    /// **Note**: Not cloud-native. Use `secret_ref` for production.
    #[serde(default)]
    pub persistent_volume: Option<k8s_openapi::api::core::v1::PersistentVolumeClaimSpec>,
}

/// Reference to a Kubernetes Secret
///
/// Used for referencing external Secrets containing DNSSEC keys,
/// certificates, or other sensitive data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretReference {
    /// Secret name
    pub name: String,

    /// Optional namespace (defaults to same namespace as the resource)
    #[serde(default)]
    pub namespace: Option<String>,
}

/// DNSSEC status information for a signed zone
///
/// Tracks DNSSEC signing status, DS records for parent zones,
/// and key rotation timestamps.
///
/// This status is populated by the `DNSZone` controller after
/// successful zone signing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DNSSECStatus {
    /// Zone is signed with DNSSEC
    pub signed: bool,

    /// DS (Delegation Signer) records for parent zone delegation
    ///
    /// These records must be published in the parent zone to complete
    /// the DNSSEC chain of trust.
    ///
    /// Format: `<zone> IN DS <keytag> <algorithm> <digesttype> <digest>`
    ///
    /// Example: `["example.com. IN DS 12345 13 2 ABC123..."]`
    #[serde(default)]
    pub ds_records: Vec<String>,

    /// KSK key tag (numeric identifier)
    ///
    /// Identifies the Key Signing Key used to sign the DNSKEY `RRset`.
    /// This value appears in the DS record.
    #[serde(default)]
    pub key_tag: Option<u32>,

    /// DNSSEC algorithm name
    ///
    /// Example: `"ECDSAP256SHA256"`, `"RSASHA256"`
    #[serde(default)]
    pub algorithm: Option<String>,

    /// Next scheduled key rollover timestamp (ISO 8601)
    ///
    /// When the next automatic key rotation will occur.
    ///
    /// Example: `"2026-04-02T00:00:00Z"`
    #[serde(default)]
    pub next_key_rollover: Option<String>,

    /// Last key rollover timestamp (ISO 8601)
    ///
    /// When the most recent key rotation occurred.
    ///
    /// Example: `"2025-04-02T00:00:00Z"`
    #[serde(default)]
    pub last_key_rollover: Option<String>,
}

/// Container image configuration for BIND9 instances
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryConfig {
    /// Number of primary instance replicas (default: 1)
    ///
    /// This controls how many replicas each primary instance in this cluster should have.
    /// Can be overridden at the instance level.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0, max = 100))]
    pub replicas: Option<i32>,

    /// Additional labels to apply to primary `Bind9Instance` resources
    ///
    /// These labels are propagated from the cluster/provider to all primary instances.
    /// They are merged with standard labels (app.kubernetes.io/*) and can be used for:
    /// - Instance selection via `DNSZone.spec.bind9InstancesFrom` label selectors
    /// - Pod selectors in network policies
    /// - Monitoring and alerting label filters
    /// - Custom organizational taxonomy
    ///
    /// Example:
    /// ```yaml
    /// primary:
    ///   labels:
    ///     environment: production
    ///     tier: frontend
    ///     region: us-east-1
    /// ```
    ///
    /// These labels will appear on the `Bind9Instance` metadata and can be referenced
    /// by `DNSZone` resources using `bind9InstancesFrom.selector.matchLabels`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,

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
    #[deprecated(
        since = "0.6.0",
        note = "Use `rndc_key` instead. This field will be removed in v1.0.0"
    )]
    pub rndc_secret_ref: Option<RndcSecretRef>,

    /// RNDC key configuration for all primary instances with lifecycle management.
    ///
    /// Supports automatic key rotation, Secret references, and inline Secret specifications.
    /// Overrides global RNDC configuration for primary instances.
    ///
    /// **Precedence order**:
    /// 1. Instance level (`spec.rndcKey`)
    /// 2. Role level (`spec.primary.rndcKey` or `spec.secondary.rndcKey`)
    /// 3. Global level (cluster-wide RNDC configuration)
    /// 4. Auto-generated (default)
    ///
    /// Can be overridden at the instance level via `spec.rndcKey`.
    ///
    /// **Backward compatibility**: If both `rndc_key` and `rndc_secret_ref` are specified,
    /// `rndc_key` takes precedence. For smooth migration, `rndc_secret_ref` will continue
    /// to work but is deprecated.
    ///
    /// # Example
    ///
    /// ```yaml
    /// primary:
    ///   replicas: 1
    ///   rndcKey:
    ///     autoRotate: true
    ///     rotateAfter: 720h  # 30 days
    ///     algorithm: hmac-sha256
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rndc_key: Option<RndcKeyConfig>,
}

/// Secondary instance configuration
///
/// Groups all configuration specific to secondary (replica) DNS instances.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryConfig {
    /// Number of secondary instance replicas (default: 1)
    ///
    /// This controls how many replicas each secondary instance in this cluster should have.
    /// Can be overridden at the instance level.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0, max = 100))]
    pub replicas: Option<i32>,

    /// Additional labels to apply to secondary `Bind9Instance` resources
    ///
    /// These labels are propagated from the cluster/provider to all secondary instances.
    /// They are merged with standard labels (app.kubernetes.io/*) and can be used for:
    /// - Instance selection via `DNSZone.spec.bind9InstancesFrom` label selectors
    /// - Pod selectors in network policies
    /// - Monitoring and alerting label filters
    /// - Custom organizational taxonomy
    ///
    /// Example:
    /// ```yaml
    /// secondary:
    ///   labels:
    ///     environment: production
    ///     tier: backend
    ///     region: us-west-2
    /// ```
    ///
    /// These labels will appear on the `Bind9Instance` metadata and can be referenced
    /// by `DNSZone` resources using `bind9InstancesFrom.selector.matchLabels`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,

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
    #[deprecated(
        since = "0.6.0",
        note = "Use `rndc_key` instead. This field will be removed in v1.0.0"
    )]
    pub rndc_secret_ref: Option<RndcSecretRef>,

    /// RNDC key configuration for all secondary instances with lifecycle management.
    ///
    /// Supports automatic key rotation, Secret references, and inline Secret specifications.
    /// Overrides global RNDC configuration for secondary instances.
    ///
    /// **Precedence order**:
    /// 1. Instance level (`spec.rndcKey`)
    /// 2. Role level (`spec.primary.rndcKey` or `spec.secondary.rndcKey`)
    /// 3. Global level (cluster-wide RNDC configuration)
    /// 4. Auto-generated (default)
    ///
    /// Can be overridden at the instance level via `spec.rndcKey`.
    ///
    /// **Backward compatibility**: If both `rndc_key` and `rndc_secret_ref` are specified,
    /// `rndc_key` takes precedence. For smooth migration, `rndc_secret_ref` will continue
    /// to work but is deprecated.
    ///
    /// # Example
    ///
    /// ```yaml
    /// secondary:
    ///   replicas: 2
    ///   rndcKey:
    ///     autoRotate: true
    ///     rotateAfter: 720h  # 30 days
    ///     algorithm: hmac-sha256
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rndc_key: Option<RndcKeyConfig>,
}

/// Common specification fields shared between namespace-scoped and cluster-scoped BIND9 clusters.
///
/// This struct contains all configuration that is common to both `Bind9Cluster` (namespace-scoped)
/// and `ClusterBind9Provider` (cluster-scoped). By using this shared struct, we avoid code duplication
/// and ensure consistency between the two cluster types.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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
/// apiVersion: bindy.firestoned.io/v1beta1
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
    version = "v1beta1",
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
/// apiVersion: bindy.firestoned.io/v1beta1
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
    version = "v1beta1",
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

impl ServerRole {
    /// Convert `ServerRole` to its string representation.
    ///
    /// Returns the lowercase zone type string used in BIND9 configuration
    /// and bindcar API calls.
    ///
    /// # Returns
    /// * `"primary"` for `ServerRole::Primary`
    /// * `"secondary"` for `ServerRole::Secondary`
    ///
    /// # Examples
    ///
    /// ```
    /// use bindy::crd::ServerRole;
    ///
    /// assert_eq!(ServerRole::Primary.as_str(), "primary");
    /// assert_eq!(ServerRole::Secondary.as_str(), "secondary");
    /// ```
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
        }
    }
}

/// `Bind9Instance` represents a BIND9 DNS server deployment in Kubernetes.
///
/// Each `Bind9Instance` creates a Deployment, Service, `ConfigMap`, and Secret for managing
/// a BIND9 server. The instance communicates with the controller via RNDC protocol.
///
/// # Example
///
/// ```yaml
/// apiVersion: bindy.firestoned.io/v1beta1
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
    version = "v1beta1",
    kind = "Bind9Instance",
    namespaced,
    shortname = "b9",
    shortname = "b9s",
    doc = "Bind9Instance represents a BIND9 DNS server deployment in Kubernetes. Each instance creates a Deployment, Service, ConfigMap, and Secret for managing a BIND9 server with RNDC protocol communication.",
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".spec.clusterRef"}"#,
    printcolumn = r#"{"name":"Role","type":"string","jsonPath":".spec.role"}"#,
    printcolumn = r#"{"name":"Replicas","type":"integer","jsonPath":".spec.replicas"}"#,
    printcolumn = r#"{"name":"Zones","type":"integer","jsonPath":".status.zonesCount"}"#,
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
    #[deprecated(
        since = "0.6.0",
        note = "Use `rndc_key` instead. This field will be removed in v1.0.0"
    )]
    pub rndc_secret_ref: Option<RndcSecretRef>,

    /// Instance-level RNDC key configuration with lifecycle management.
    ///
    /// Supports automatic key rotation, Secret references, and inline Secret specifications.
    /// Overrides role-level and global RNDC configuration for this specific instance.
    ///
    /// **Precedence order**:
    /// 1. **Instance level** (`spec.rndcKey`) - Highest priority
    /// 2. Role level (`spec.primary.rndcKey` or `spec.secondary.rndcKey`)
    /// 3. Global level (cluster-wide RNDC configuration)
    /// 4. Auto-generated (default)
    ///
    /// **Backward compatibility**: If both `rndc_key` and `rndc_secret_ref` are specified,
    /// `rndc_key` takes precedence. For smooth migration, `rndc_secret_ref` will continue
    /// to work but is deprecated.
    ///
    /// # Example
    ///
    /// ```yaml
    /// apiVersion: bindy.firestoned.io/v1beta1
    /// kind: Bind9Instance
    /// spec:
    ///   rndcKey:
    ///     autoRotate: true
    ///     rotateAfter: 2160h  # 90 days
    ///     algorithm: hmac-sha512
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rndc_key: Option<RndcKeyConfig>,

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
    /// IP or hostname of this instance's service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_address: Option<String>,
    /// Resolved cluster reference with full object details.
    ///
    /// This field is populated by the instance reconciler and contains the full Kubernetes
    /// object reference (kind, apiVersion, namespace, name) of the cluster this instance
    /// belongs to. This provides backward compatibility with `spec.clusterRef` (which is
    /// just a string name) and enables proper Kubernetes object references.
    ///
    /// For namespace-scoped `Bind9Cluster`, includes namespace.
    /// For cluster-scoped `ClusterBind9Provider`, namespace will be empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_ref: Option<ClusterReference>,
    /// List of DNS zones that have selected this instance.
    ///
    /// This field is automatically populated by a status-only watcher on `DNSZones`.
    /// When a `DNSZone`'s `status.bind9Instances` includes this instance, the zone
    /// is added to this list. This provides a reverse lookup: instance → zones.
    ///
    /// Updated by: `DNSZone` status watcher (not by instance reconciler)
    /// Used for: Observability, debugging zone assignments
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<ZoneReference>,

    /// Number of zones in the `zones` list.
    ///
    /// This field is automatically updated whenever the `zones` list changes.
    /// It provides a quick way to see how many zones are selecting this instance
    /// without having to count the array elements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zones_count: Option<i32>,

    /// RNDC key rotation status and tracking information.
    ///
    /// Populated when `auto_rotate` is enabled in the RNDC configuration. Provides
    /// visibility into key lifecycle: creation time, next rotation time, and rotation count.
    ///
    /// This field is automatically updated by the instance reconciler whenever:
    /// - A new RNDC key is generated
    /// - An RNDC key is rotated
    /// - The rotation configuration changes
    ///
    /// **Note**: Only present when using operator-managed RNDC keys. If you specify
    /// `secret_ref` to use an external Secret, this field will be empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rndc_key_rotation: Option<RndcKeyRotationStatus>,
}

/// RNDC key rotation status and tracking information.
///
/// Tracks the lifecycle of operator-managed RNDC keys including creation time,
/// next rotation time, last rotation time, and rotation count.
///
/// This status is automatically updated by the instance reconciler whenever keys
/// are created or rotated. It provides visibility into key age and rotation history
/// for compliance and operational purposes.
///
/// # Examples
///
/// ```yaml
/// # Initial key creation (no rotation yet)
/// rndcKeyRotation:
///   createdAt: "2025-01-26T10:00:00Z"
///   rotateAt: "2025-02-25T10:00:00Z"
///   rotationCount: 0
///
/// # After first rotation
/// rndcKeyRotation:
///   createdAt: "2025-02-25T10:00:00Z"
///   rotateAt: "2025-03-27T10:00:00Z"
///   lastRotatedAt: "2025-02-25T10:00:00Z"
///   rotationCount: 1
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RndcKeyRotationStatus {
    /// Timestamp when the current key was created (ISO 8601 format).
    ///
    /// This timestamp is set when:
    /// - A new RNDC key is generated for the first time
    /// - An existing key is rotated (timestamp updates to rotation time)
    ///
    /// Example: `"2025-01-26T10:00:00Z"`
    pub created_at: String,

    /// Timestamp when the key will be rotated next (ISO 8601 format).
    ///
    /// Calculated as: `created_at + rotate_after`
    ///
    /// Only present if `auto_rotate` is enabled. When `auto_rotate` is disabled,
    /// this field will be empty as no automatic rotation is scheduled.
    ///
    /// Example: `"2025-02-25T10:00:00Z"` (30 days after creation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotate_at: Option<String>,

    /// Timestamp of the last successful rotation (ISO 8601 format).
    ///
    /// Only present after at least one rotation has occurred. For newly-created
    /// keys that have never been rotated, this field will be empty.
    ///
    /// This is useful for tracking the actual rotation history and verifying that
    /// rotation is working as expected.
    ///
    /// Example: `"2025-02-25T10:00:00Z"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_rotated_at: Option<String>,

    /// Number of times the key has been rotated.
    ///
    /// Starts at `0` for newly-created keys and increments by 1 each time the
    /// key is rotated. This counter persists across key rotations and provides
    /// a historical count for compliance and audit purposes.
    ///
    /// Example: `5` (key has been rotated 5 times)
    #[serde(default)]
    pub rotation_count: u32,
}

/// Reference to a DNS zone selected by an instance.
///
/// This structure follows Kubernetes object reference conventions and stores
/// the complete information needed to reference a `DNSZone` resource.
///
/// **Note on Equality:** `PartialEq`, `Eq`, and `Hash` are implemented to compare only the
/// identity fields (`api_version`, `kind`, `name`, `namespace`, `zone_name`), ignoring `last_reconciled_at`.
/// This ensures that zones are correctly identified as duplicates even when their
/// reconciliation timestamps differ.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ZoneReference {
    /// API version of the `DNSZone` resource (e.g., "bindy.firestoned.io/v1beta1")
    pub api_version: String,
    /// Kind of the resource (always "`DNSZone`")
    pub kind: String,
    /// Name of the `DNSZone` resource
    pub name: String,
    /// Namespace of the `DNSZone` resource
    pub namespace: String,
    /// Fully qualified domain name from the zone's spec (e.g., "example.com")
    pub zone_name: String,
    /// Timestamp when this zone was last successfully configured on the instance.
    ///
    /// This field is set by the `DNSZone` controller after successfully applying zone configuration
    /// to the instance. It is reset to `None` when:
    /// - The instance's pod restarts (requiring zone reconfiguration)
    /// - The instance's spec changes (requiring reconfiguration)
    ///
    /// The `DNSZone` controller uses this field to determine which instances need zone configuration.
    /// If this field is `None`, the zone needs to be configured on the instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reconciled_at: Option<String>,
}

// Implement PartialEq to compare only identity fields, ignoring last_reconciled_at
impl PartialEq for ZoneReference {
    fn eq(&self, other: &Self) -> bool {
        self.api_version == other.api_version
            && self.kind == other.kind
            && self.name == other.name
            && self.namespace == other.namespace
            && self.zone_name == other.zone_name
    }
}

// Implement Eq for ZoneReference
impl Eq for ZoneReference {}

// Implement Hash to hash only identity fields
impl std::hash::Hash for ZoneReference {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.api_version.hash(state);
        self.kind.hash(state);
        self.name.hash(state);
        self.namespace.hash(state);
        self.zone_name.hash(state);
        // Deliberately exclude last_reconciled_at from hash
    }
}

/// Full Kubernetes object reference to a cluster resource.
///
/// This structure follows Kubernetes object reference conventions and stores
/// the complete information needed to reference either a namespace-scoped
/// `Bind9Cluster` or cluster-scoped `ClusterBind9Provider`.
///
/// This enables proper object references and provides backward compatibility
/// with `spec.clusterRef` (which stores only the name as a string).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClusterReference {
    /// API version of the referenced cluster (e.g., "bindy.firestoned.io/v1beta1")
    pub api_version: String,
    /// Kind of the referenced cluster ("`Bind9Cluster`" or "`ClusterBind9Provider`")
    pub kind: String,
    /// Name of the cluster resource
    pub name: String,
    /// Namespace of the cluster resource.
    ///
    /// For namespace-scoped `Bind9Cluster`, this is the cluster's namespace.
    /// For cluster-scoped `ClusterBind9Provider`, this field is empty/None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// Full Kubernetes object reference to a `Bind9Instance` resource.
///
/// This structure follows Kubernetes object reference conventions and stores
/// the complete information needed to reference a namespace-scoped `Bind9Instance`.
///
/// Used in `DNSZone.status.bind9Instances` for tracking instances that have claimed the zone
/// (via `bind9InstancesFrom` label selectors or `clusterRef`).
///
/// **Note on Equality:** `PartialEq`, `Eq`, and `Hash` are implemented to compare only the
/// identity fields (`api_version`, `kind`, `name`, `namespace`), ignoring `last_reconciled_at`.
/// This ensures that instances are correctly identified as duplicates even when their
/// reconciliation timestamps differ.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceReference {
    /// API version of the `Bind9Instance` resource (e.g., "bindy.firestoned.io/v1beta1")
    pub api_version: String,
    /// Kind of the resource (always "`Bind9Instance`")
    pub kind: String,
    /// Name of the `Bind9Instance` resource
    pub name: String,
    /// Namespace of the `Bind9Instance` resource
    pub namespace: String,
    /// Timestamp when this instance was last successfully reconciled with zone configuration.
    ///
    /// This field is set when the zone configuration is successfully applied to the instance.
    /// It is reset (cleared) when:
    /// - The instance is deleted
    /// - The instance's pod IP changes (requiring zone reconfiguration)
    /// - The zone spec changes (requiring reconfiguration)
    ///
    /// The reconciler uses this field to determine which instances need zone configuration.
    /// If this field is `None` or the timestamp is before the last spec change, the instance
    /// will be reconfigured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reconciled_at: Option<String>,
}

// Implement PartialEq to compare only identity fields, ignoring last_reconciled_at
impl PartialEq for InstanceReference {
    fn eq(&self, other: &Self) -> bool {
        self.api_version == other.api_version
            && self.kind == other.kind
            && self.name == other.name
            && self.namespace == other.namespace
    }
}

// Implement Eq for InstanceReference
impl Eq for InstanceReference {}

// Implement Hash to hash only identity fields
impl std::hash::Hash for InstanceReference {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.api_version.hash(state);
        self.kind.hash(state);
        self.name.hash(state);
        self.namespace.hash(state);
        self.last_reconciled_at.hash(state);
    }
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BindcarConfig {
    /// Container image for the RNDC API sidecar
    ///
    /// Example: "ghcr.io/firestoned/bindcar:v0.5.1"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Image pull policy (`Always`, `IfNotPresent`, `Never`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_pull_policy: Option<String>,

    /// Resource requirements for the Bindcar container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<k8s_openapi::api::core::v1::ResourceRequirements>,

    /// API server container port (default: 8080)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Custom Kubernetes Service spec for the bindcar HTTP API
    ///
    /// Allows full customization of the Service that exposes the bindcar API.
    /// This is merged with the default Service spec, allowing overrides of ports,
    /// type, sessionAffinity, and other Service configurations.
    ///
    /// Example:
    /// ```yaml
    /// serviceSpec:
    ///   type: NodePort
    ///   ports:
    ///     - name: http
    ///       port: 8000
    ///       targetPort: 8080
    ///       nodePort: 30080
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_spec: Option<k8s_openapi::api::core::v1::ServiceSpec>,

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
