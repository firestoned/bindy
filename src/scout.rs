// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Bindy Scout — Ingress-to-ARecord controller.
//!
//! Scout watches Kubernetes Ingresses across all namespaces (except its own and any
//! configured exclusions). When an Ingress is annotated with
//! `bindy.firestoned.io/recordKind: "ARecord"`, Scout creates an [`ARecord`] CR in the
//! configured target namespace.
//!
//! See `docs/roadmaps/bindy-scout-ingress-controller.md` for the full design.
//!
//! ## Phase 1 / 1.5 — Same-cluster mode (current)
//!
//! Scout uses a single in-cluster client. ARecords are created in the same cluster.
//!
//! ## Phase 2 — Remote cluster mode
//!
//! When `BINDY_SCOUT_REMOTE_SECRET` is set, Scout reads a kubeconfig from a Kubernetes
//! Secret and builds a second client (`remote_client`) targeting the dedicated Bindy cluster.
//! The local client still handles Ingress watching and finalizer management.
//! The remote client handles ARecord creation/deletion and DNSZone validation.

use crate::constants::{ALLOW_ZONE_NAMESPACES_WILDCARD, ANNOTATION_ALLOW_ZONE_NAMESPACES};
use crate::crd::{ARecord, ARecordSpec, DNSZone};
use anyhow::{anyhow, Result};
use k8s_openapi::api::core::v1::{Secret, Service};
use kube::api::{DeleteParams, ListParams, Patch, PatchParams};
use kube::config::{KubeConfigOptions, Kubeconfig};

/// Reconcile error type — wraps `anyhow::Error` so that it satisfies the
/// `std::error::Error` bound required by `kube::runtime::Controller::run`.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ScoutError(#[from] anyhow::Error);
use futures::StreamExt;
use k8s_openapi::api::networking::v1::Ingress;
use kube::{
    runtime::{
        controller::Action, reflector, watcher, watcher::Config as WatcherConfig, Controller,
    },
    Api, Client, Error as KubeError, ResourceExt,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tracing::{debug, error, info, warn};

// ============================================================================
// Gateway API Type Definitions
//
// HTTPRoute and TLSRoute are not in k8s_openapi yet, so we define minimal structs.
// We only care about metadata and spec.hostnames[] for Scout's reconciliation.
// ============================================================================

/// A minimal Gateway API `parentRef` — the reference from a route back to the
/// Gateway (or other parent) that serves it.
///
/// Only the fields Scout needs to walk route → Gateway are modelled. Per the
/// Gateway API defaults, an omitted `group` means `gateway.networking.k8s.io`
/// and an omitted `kind` means `Gateway`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentReference {
    /// API group of the parent. Defaults to `gateway.networking.k8s.io` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Kind of the parent. Defaults to `Gateway` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Namespace of the parent. Defaults to the route's own namespace when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Name of the parent Gateway.
    pub name: String,
}

/// Minimal HTTPRoute spec for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HTTPRouteSpec {
    /// Hostnames matching this HTTPRoute
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostnames: Option<Vec<String>>,
    /// Gateways this route attaches to. Scout follows these to discover the
    /// serving Gateway's external IP when no explicit IP annotation is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_refs: Option<Vec<ParentReference>>,
}

/// Minimal HTTPRoute definition for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HTTPRoute {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: kube::api::ObjectMeta,
    #[serde(default)]
    pub spec: Option<HTTPRouteSpec>,
}

/// Minimal TLSRoute spec for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TLSRouteSpec {
    /// Hostnames matching this TLSRoute
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostnames: Option<Vec<String>>,
    /// Rules for this TLSRoute (required by API, but Scout only uses hostnames)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<serde_json::Value>>,
    /// Gateways this route attaches to. Scout follows these to discover the
    /// serving Gateway's external IP when no explicit IP annotation is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_refs: Option<Vec<ParentReference>>,
}

/// Minimal TLSRoute definition for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TLSRoute {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: kube::api::ObjectMeta,
    #[serde(default)]
    pub spec: Option<TLSRouteSpec>,
}

/// Minimal TCPRoute spec for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TCPRouteSpec {
    /// Hostnames matching this TCPRoute when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostnames: Option<Vec<String>>,
    /// Rules for this TCPRoute (required by API, but Scout only uses hostnames).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<serde_json::Value>>,
    /// Gateways this route attaches to. Scout follows these to discover the
    /// serving Gateway's external IP when no explicit IP annotation is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_refs: Option<Vec<ParentReference>>,
}

/// Minimal TCPRoute definition for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TCPRoute {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: kube::api::ObjectMeta,
    #[serde(default)]
    pub spec: Option<TCPRouteSpec>,
}

// Implement k8s_openapi::Metadata for HTTPRoute and TLSRoute
impl k8s_openapi::Metadata for HTTPRoute {
    type Ty = kube::api::ObjectMeta;
    fn metadata(&self) -> &kube::api::ObjectMeta {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut kube::api::ObjectMeta {
        &mut self.metadata
    }
}

impl k8s_openapi::Metadata for TLSRoute {
    type Ty = kube::api::ObjectMeta;
    fn metadata(&self) -> &kube::api::ObjectMeta {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut kube::api::ObjectMeta {
        &mut self.metadata
    }
}

// Implement k8s_openapi::Resource for HTTPRoute and TLSRoute
impl k8s_openapi::Resource for HTTPRoute {
    const API_VERSION: &'static str = "gateway.networking.k8s.io/v1";
    const GROUP: &'static str = "gateway.networking.k8s.io";
    const KIND: &'static str = "HTTPRoute";
    const VERSION: &'static str = "v1";
    const URL_PATH_SEGMENT: &'static str = "httproutes";
    type Scope = k8s_openapi::NamespaceResourceScope;
}

impl k8s_openapi::Resource for TLSRoute {
    const API_VERSION: &'static str = "gateway.networking.k8s.io/v1alpha2";
    const GROUP: &'static str = "gateway.networking.k8s.io";
    const KIND: &'static str = "TLSRoute";
    const VERSION: &'static str = "v1alpha2";
    const URL_PATH_SEGMENT: &'static str = "tlsroutes";
    type Scope = k8s_openapi::NamespaceResourceScope;
}

impl k8s_openapi::Metadata for TCPRoute {
    type Ty = kube::api::ObjectMeta;
    fn metadata(&self) -> &kube::api::ObjectMeta {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut kube::api::ObjectMeta {
        &mut self.metadata
    }
}

impl k8s_openapi::Resource for TCPRoute {
    const API_VERSION: &'static str = "gateway.networking.k8s.io/v1alpha2";
    const GROUP: &'static str = "gateway.networking.k8s.io";
    const KIND: &'static str = "TCPRoute";
    const VERSION: &'static str = "v1alpha2";
    const URL_PATH_SEGMENT: &'static str = "tcproutes";
    type Scope = k8s_openapi::NamespaceResourceScope;
}

/// Gateway API group used for `parentRefs` and Gateway lookups.
pub const GATEWAY_API_GROUP: &str = "gateway.networking.k8s.io";

/// Gateway API `kind` for a Gateway parent reference.
pub const GATEWAY_KIND: &str = "Gateway";

/// A namespaced object reference (`namespace` + `name`).
///
/// Used both for the operator-configured `gatewayClass → LoadBalancer Service`
/// map and for the Gateways a route's `parentRefs` point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacedName {
    /// Object namespace.
    pub namespace: String,
    /// Object name.
    pub name: String,
}

/// How Scout locates the LoadBalancer Service backing a gateway class.
///
/// Configured per `gatewayClass` so operators can pin the exact Service — either
/// by an explicit `namespace/name`, or by a label selector scoped to a namespace
/// (useful when the Service name is generated but carries stable labels).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayServiceTarget {
    /// A specific LoadBalancer Service, addressed by namespace and name.
    Name(NamespacedName),
    /// A LoadBalancer Service found by label selector within a namespace.
    /// `selector` is a standard Kubernetes label-selector string, e.g.
    /// `app.kubernetes.io/name=traefik`.
    Labeled {
        /// Namespace to search for the Service.
        namespace: String,
        /// Kubernetes label selector identifying the Service.
        selector: String,
    },
}

impl GatewayServiceTarget {
    /// Namespace the target Service lives in.
    #[must_use]
    pub fn namespace(&self) -> &str {
        match self {
            Self::Name(nn) => &nn.namespace,
            Self::Labeled { namespace, .. } => namespace,
        }
    }
}

/// Minimal Gateway spec — only `gatewayClassName`, used to match the running class.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySpec {
    /// Name of the GatewayClass implementing this Gateway.
    pub gateway_class_name: String,
}

/// A single entry in `Gateway.status.addresses`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatusAddress {
    /// Address type, e.g. `IPAddress` or `Hostname`. Absent is treated as unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// The address value (an IP for `IPAddress`, a DNS name for `Hostname`).
    pub value: String,
}

/// Minimal Gateway status — only the assigned addresses.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    /// Addresses the controller has assigned to this Gateway (often empty when
    /// the controller publishes the external IP only on its LoadBalancer Service).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addresses: Option<Vec<GatewayStatusAddress>>,
}

/// Minimal Gateway definition for Scout's chain-following.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Gateway {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: kube::api::ObjectMeta,
    pub spec: GatewaySpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<GatewayStatus>,
}

impl k8s_openapi::Metadata for Gateway {
    type Ty = kube::api::ObjectMeta;
    fn metadata(&self) -> &kube::api::ObjectMeta {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut kube::api::ObjectMeta {
        &mut self.metadata
    }
}

impl k8s_openapi::Resource for Gateway {
    const API_VERSION: &'static str = "gateway.networking.k8s.io/v1";
    const GROUP: &'static str = "gateway.networking.k8s.io";
    const KIND: &'static str = "Gateway";
    const VERSION: &'static str = "v1";
    const URL_PATH_SEGMENT: &'static str = "gateways";
    type Scope = k8s_openapi::NamespaceResourceScope;
}

// ============================================================================
// Constants
// ============================================================================

/// Annotation specifying the DNS record kind Scout should create for this Ingress.
/// Set to `"ARecord"` to create an A record. Any other value (or absent) is ignored.
pub const ANNOTATION_RECORD_KIND: &str = "bindy.firestoned.io/recordKind";

/// Expected value of [`ANNOTATION_RECORD_KIND`] for A record creation.
pub const RECORD_KIND_ARECORD: &str = "ARecord";

/// Annotation specifying which DNS zone owns this Ingress host
pub const ANNOTATION_ZONE: &str = "bindy.firestoned.io/zone";

/// Simplified opt-in annotation — set to `"true"` to enable Scout for this Ingress.
/// Takes precedence over (and is preferred to) [`ANNOTATION_RECORD_KIND`] for new users.
/// Both annotations are accepted for backward compatibility.
pub const ANNOTATION_SCOUT_ENABLED: &str = "bindy.firestoned.io/scout-enabled";

/// Annotation for explicitly overriding the IP(s) used in the ARecord.
///
/// Accepts a single IP (`"10.0.0.1"`) or a comma-separated list of IPs
/// (`"10.0.0.1,10.0.0.2"`) — every entry becomes an address on the resulting
/// `ARecord`, in the order given. Whitespace around each entry is trimmed and
/// empty entries are skipped. When set, takes precedence over `--default-ips`
/// and any LoadBalancer status IP.
pub const ANNOTATION_IP: &str = "bindy.firestoned.io/ip";

/// Annotation for overriding the TTL (in seconds) on the created ARecord.
/// When absent, the ARecord inherits the TTL from the DNSZone spec.
pub const ANNOTATION_TTL: &str = "bindy.firestoned.io/ttl";

/// Annotation for overriding the DNS record name (`spec.name`) on the created ARecord.
///
/// When set, the value replaces the name normally derived from the source resource's
/// host/hostname. Use `"@"` to target the zone apex. When absent or empty, Scout falls
/// back to deriving the name from the host stripped of the zone suffix.
///
/// On multi-host resources (Ingress / HTTPRoute / TLSRoute) the override is applied to
/// every ARecord produced from that resource, so it is intended for single-host use cases.
pub const ANNOTATION_RECORD_NAME: &str = "bindy.firestoned.io/record-name";

/// Finalizer added to Ingresses managed by Scout to ensure cleanup on deletion
pub const FINALIZER_SCOUT: &str = "bindy.firestoned.io/arecord-finalizer";

/// Label placed on created ARecords identifying Scout as the manager
pub const LABEL_MANAGED_BY: &str = "bindy.firestoned.io/managed-by";

/// Label value for ARecords created by Scout
pub const LABEL_MANAGED_BY_SCOUT: &str = "scout";

/// Label identifying the source cluster on created ARecords
pub const LABEL_SOURCE_CLUSTER: &str = "bindy.firestoned.io/source-cluster";

/// Label identifying the source namespace on created ARecords
pub const LABEL_SOURCE_NAMESPACE: &str = "bindy.firestoned.io/source-namespace";

/// Label identifying the source resource name on created ARecords.
/// Used for all resource kinds (Ingress, Service, HTTPRoute, TLSRoute).
pub const LABEL_SOURCE_NAME: &str = "bindy.firestoned.io/source-name";

/// Label carrying the DNS zone name on created ARecords (for DNSZone selector matching)
pub const LABEL_ZONE: &str = "bindy.firestoned.io/zone";

/// Default namespace where ARecords are created when `BINDY_SCOUT_NAMESPACE` is not set
pub const DEFAULT_SCOUT_NAMESPACE: &str = "bindy-system";

/// Maximum Kubernetes resource name length in characters
const MAX_K8S_NAME_LEN: usize = 253;

/// Prefix applied to all ARecord CR names created by Scout
const ARECORD_NAME_PREFIX: &str = "scout";

/// Requeue delay for non-fatal errors (seconds)
const SCOUT_ERROR_REQUEUE_SECS: u64 = 30;

/// Backoff delay before re-polling the DNSZone reflector after a connection error (seconds).
/// The kube-runtime watcher has no built-in backoff — consumers must apply their own by
/// delaying the next poll. Without this, a failed LIST/WATCH causes a tight retry loop.
const REFLECTOR_ERROR_BACKOFF_SECS: u64 = 5;

// ============================================================================
// Context
// ============================================================================

/// Shared context passed to every reconciler invocation.
pub struct ScoutContext {
    /// Local Kubernetes client — Ingress watching and finalizer management.
    /// Always the in-cluster client regardless of mode.
    pub client: Client,
    /// Remote Kubernetes client — ARecord creation/deletion and DNSZone validation.
    /// In same-cluster mode (Phase 1) this is identical to `client`.
    /// In remote mode (Phase 2+) this targets the dedicated Bindy cluster.
    pub remote_client: Client,
    /// Namespace where ARecords are created (on the remote/target cluster)
    pub target_namespace: String,
    /// Logical cluster name stamped on created ARecord labels
    pub cluster_name: String,
    /// Namespaces excluded from Ingress watching (always includes Scout's own namespace)
    pub excluded_namespaces: Vec<String>,
    /// Default IPs used when no annotation override and no LB status IP is available.
    /// Intended for shared-ingress topologies (e.g. Traefik) where all Ingresses resolve
    /// to the same IP(s). Set via `BINDY_SCOUT_DEFAULT_IPS` or `--default-ips`.
    pub default_ips: Vec<String>,
    /// Operator-configured `gatewayClass → LoadBalancer Service` map. When an HTTPRoute
    /// or TLSRoute has no explicit IP annotation, Scout follows its `parentRefs` to a
    /// Gateway of one of these classes and reads the mapped Service's external IP.
    /// Set via `BINDY_SCOUT_GATEWAY_SERVICES` or `--gateway-service`.
    pub gateway_services: BTreeMap<String, GatewayServiceTarget>,
    /// Default DNS zone applied to all Ingresses when no `bindy.firestoned.io/zone` annotation
    /// is present. Set via `BINDY_SCOUT_DEFAULT_ZONE` or `--default-zone`.
    pub default_zone: Option<String>,
    /// Read-only store of DNSZone resources for zone validation.
    /// Populated from the remote client so zones are validated against the bindy cluster.
    pub zone_store: reflector::Store<DNSZone>,
}

// ============================================================================
// Pure helper functions (tested in scout_tests.rs)
// ============================================================================

/// Returns `true` if the Ingress is annotated for ARecord creation.
///
/// The annotation `bindy.firestoned.io/recordKind` must have the value `"ARecord"` (case-sensitive).
/// Any other value (or absence of the annotation) returns `false`.
pub fn is_arecord_enabled(annotations: &BTreeMap<String, String>) -> bool {
    annotations
        .get(ANNOTATION_RECORD_KIND)
        .map(|v| v == RECORD_KIND_ARECORD)
        .unwrap_or(false)
}

/// Returns `true` if Scout should manage this Ingress.
///
/// Accepts either the simplified opt-in annotation:
/// - `bindy.firestoned.io/scout-enabled: "true"` (preferred for new deployments)
///
/// Or the legacy annotation for backward compatibility:
/// - `bindy.firestoned.io/recordKind: "ARecord"`
///
/// The record kind always defaults to `ARecord` — no further annotation is needed.
pub fn is_scout_opted_in(annotations: &BTreeMap<String, String>) -> bool {
    annotations
        .get(ANNOTATION_SCOUT_ENABLED)
        .map(|v| v == "true")
        .unwrap_or(false)
        || is_arecord_enabled(annotations)
}

/// Resolves the DNS zone for an Ingress, in priority order:
///
/// 1. `bindy.firestoned.io/zone` annotation — per-Ingress explicit override
/// 2. `default_zone` — operator-configured default zone (e.g. `"example.com"`)
///
/// Returns `None` if neither is available. When `None`, Scout logs a warning and skips the Ingress.
pub fn resolve_zone(
    annotations: &BTreeMap<String, String>,
    default_zone: Option<&str>,
) -> Option<String> {
    get_zone_annotation(annotations).or_else(|| default_zone.map(ToString::to_string))
}

/// Returns the DNS zone specified by the `bindy.firestoned.io/zone` annotation.
///
/// Returns `None` if the annotation is absent or has an empty value.
pub fn get_zone_annotation(annotations: &BTreeMap<String, String>) -> Option<String> {
    annotations
        .get(ANNOTATION_ZONE)
        .filter(|v| !v.is_empty())
        .cloned()
}

/// Derives the DNS record name from a hostname and zone.
///
/// - `host.zone` → `host` (e.g. `"app.example.com"` + `"example.com"` → `"app"`)
/// - `zone` (apex) → `"@"`
/// - `deep.sub.zone` → `"deep.sub"`
///
/// Trailing dots on `host` are stripped before comparison.
///
/// # Errors
///
/// Returns an error if `host` does not end with the zone suffix.
pub fn derive_record_name(host: &str, zone: &str) -> Result<String> {
    // Strip trailing dot if present (some Ingress controllers append it)
    let host = host.trim_end_matches('.');

    // Apex record
    if host == zone {
        return Ok("@".to_string());
    }

    let zone_suffix = format!(".{zone}");
    if !host.ends_with(&zone_suffix) {
        return Err(anyhow!(
            "host \"{host}\" does not belong to zone \"{zone}\""
        ));
    }

    let record_name = &host[..host.len() - zone_suffix.len()];
    Ok(record_name.to_string())
}

/// Returns the explicit DNS record name override from `bindy.firestoned.io/record-name`.
///
/// The annotation value is trimmed of surrounding whitespace. Returns `None` if the
/// annotation is absent, empty, or whitespace-only.
pub fn get_record_name_annotation(annotations: &BTreeMap<String, String>) -> Option<String> {
    annotations
        .get(ANNOTATION_RECORD_NAME)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Resolves the DNS record name for an ARecord, in priority order:
///
/// 1. `bindy.firestoned.io/record-name` annotation — explicit override (e.g. `"myapp"`, `"@"`)
/// 2. Derived from `host` by stripping the zone suffix (see [`derive_record_name`])
///
/// When the override annotation is present, the host is **not** validated against the zone:
/// the operator has explicitly chosen the record name and is responsible for its correctness.
///
/// # Errors
///
/// Returns the error from [`derive_record_name`] only when no override is set and the host
/// does not belong to the zone.
pub fn resolve_record_name(
    annotations: &BTreeMap<String, String>,
    host: &str,
    zone: &str,
) -> Result<String> {
    if let Some(override_name) = get_record_name_annotation(annotations) {
        return Ok(override_name);
    }
    derive_record_name(host, zone)
}

/// Returns the explicit IP overrides from the `bindy.firestoned.io/ip` annotation.
///
/// The value may be a single IP (`"10.0.0.1"`) or a comma-separated list
/// (`"10.0.0.1,10.0.0.2,10.0.0.3"`). Whitespace around each entry is trimmed
/// and empty entries are skipped, preserving order and duplicates.
///
/// Returns `None` if the annotation is absent, empty, or contains only
/// separators/whitespace.
pub fn resolve_ips_from_annotation(annotations: &BTreeMap<String, String>) -> Option<Vec<String>> {
    let raw = annotations.get(ANNOTATION_IP)?;
    let ips: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if ips.is_empty() {
        None
    } else {
        Some(ips)
    }
}

/// Whether a `DNSZone` authorizes DNS records sourced from `source_namespace`.
///
/// A DNSZone in the *same* namespace as the source object (Ingress / Service /
/// Route) is always authorized. A DNSZone in a different namespace must opt in
/// via the [`ANNOTATION_ALLOW_ZONE_NAMESPACES`] annotation — a comma-separated
/// namespace list, or the [`ALLOW_ZONE_NAMESPACES_WILDCARD`] `*`.
///
/// This mirrors the cross-namespace gate the DNSZone reconciler already
/// enforces for instance targeting, and closes audit finding H1: without it,
/// any tenant's opted-in Ingress could publish records into *any* zone Scout
/// served (a confused-deputy cross-tenant DNS hijack), because Scout writes
/// with a cluster-privileged remote client.
#[must_use]
pub fn zone_allows_source_namespace(zone: &DNSZone, source_namespace: &str) -> bool {
    if zone.namespace().as_deref() == Some(source_namespace) {
        return true;
    }
    let Some(annotations) = zone.metadata.annotations.as_ref() else {
        return false;
    };
    let Some(value) = annotations.get(ANNOTATION_ALLOW_ZONE_NAMESPACES) else {
        return false;
    };
    value
        .split(',')
        .map(str::trim)
        .any(|entry| entry == ALLOW_ZONE_NAMESPACES_WILDCARD || entry == source_namespace)
}

/// Outcome of resolving a zone name against the DNSZone store for a given
/// source namespace.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ZoneAuthz {
    /// A matching DNSZone exists and authorizes the source namespace.
    Authorized,
    /// A matching DNSZone exists but does not authorize the source namespace.
    Forbidden,
    /// No DNSZone with the requested name is present in the store yet.
    NotFound,
}

/// Resolve `zone_name` against the DNSZone `zones` for `source_namespace`.
///
/// Returns [`ZoneAuthz::Authorized`] if any matching DNSZone authorizes the
/// namespace (see [`zone_allows_source_namespace`]), [`ZoneAuthz::Forbidden`]
/// if the zone exists but no matching DNSZone authorizes it, or
/// [`ZoneAuthz::NotFound`] if no DNSZone with that name is in the store.
pub(crate) fn check_zone_authorization(
    zones: &[Arc<DNSZone>],
    zone_name: &str,
    source_namespace: &str,
) -> ZoneAuthz {
    let mut found = false;
    for zone in zones {
        if zone.spec.zone_name != zone_name {
            continue;
        }
        found = true;
        if zone_allows_source_namespace(zone, source_namespace) {
            return ZoneAuthz::Authorized;
        }
    }
    if found {
        ZoneAuthz::Forbidden
    } else {
        ZoneAuthz::NotFound
    }
}

/// Resolves the IP address(es) to use for an ARecord, in priority order:
///
/// 1. `bindy.firestoned.io/ip` annotation — explicit override (single IP or comma-separated list)
/// 2. `default_ips` — operator-configured default IPs (e.g. shared Traefik ingress VIP)
/// 3. Ingress LoadBalancer status — first non-empty IP
///
/// Returns `None` if no IP can be determined from any source.
pub fn resolve_ips(
    annotations: &BTreeMap<String, String>,
    default_ips: &[String],
    ingress: &Ingress,
) -> Option<Vec<String>> {
    if let Some(ips) = resolve_ips_from_annotation(annotations) {
        return Some(ips);
    }
    if !default_ips.is_empty() {
        return Some(default_ips.to_vec());
    }
    resolve_ip_from_lb_status(ingress).map(|ip| vec![ip])
}

/// Resolves the IP to use for an ARecord from the Ingress load-balancer status.
///
/// Returns the first non-empty `ip` field found in `status.loadBalancer.ingress`.
/// Hostname-only entries (no IP) are ignored; a warning is logged for each.
pub fn resolve_ip_from_lb_status(ingress: &Ingress) -> Option<String> {
    let lb_ingresses = ingress
        .status
        .as_ref()?
        .load_balancer
        .as_ref()?
        .ingress
        .as_ref()?;

    for lb in lb_ingresses {
        if let Some(ip) = &lb.ip {
            if !ip.is_empty() {
                return Some(ip.clone());
            }
        }
        if lb.hostname.is_some() {
            warn!(
                ingress = %ingress.name_any(),
                "Ingress LB status has hostname but no IP — A record requires an IP address; skipping"
            );
        }
    }
    None
}

/// Builds a sanitized Kubernetes resource name for an ARecord CR.
///
/// Format: `scout-{cluster}-{namespace}-{ingress}-{index}`
///
/// All characters are lowercased. Underscores and any non-alphanumeric characters
/// (other than hyphens) are replaced with hyphens. The result is truncated to
/// 253 characters to stay within the Kubernetes name limit.
pub fn arecord_cr_name(
    cluster: &str,
    namespace: &str,
    ingress_name: &str,
    host_index: usize,
) -> String {
    let raw = format!("{ARECORD_NAME_PREFIX}-{cluster}-{namespace}-{ingress_name}-{host_index}");
    let sanitized = sanitize_k8s_name(&raw);
    sanitized[..sanitized.len().min(MAX_K8S_NAME_LEN)].to_string()
}

/// Sanitizes a string for use as a Kubernetes resource name.
///
/// - Lowercases all characters
/// - Replaces any character that is not `[a-z0-9-]` with `-`
/// - Collapses consecutive hyphens into one
/// - Strips leading and trailing hyphens
fn sanitize_k8s_name(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut result = String::with_capacity(lower.len());
    let mut last_was_hyphen = false;

    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch);
            last_was_hyphen = false;
        } else {
            // Replace any non-alphanumeric character with a hyphen (collapsing runs)
            if !last_was_hyphen {
                result.push('-');
                last_was_hyphen = true;
            }
        }
    }

    // Strip trailing hyphens
    let trimmed = result.trim_end_matches('-');
    // Strip leading hyphens
    trimmed.trim_start_matches('-').to_string()
}

/// Returns `true` if the Scout finalizer is present on the Ingress.
pub fn has_finalizer(ingress: &Ingress) -> bool {
    ingress
        .metadata
        .finalizers
        .as_ref()
        .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
        .unwrap_or(false)
}

/// Returns `true` if the Ingress has been marked for deletion.
pub fn is_being_deleted(ingress: &Ingress) -> bool {
    ingress.metadata.deletion_timestamp.is_some()
}

/// Builds a Kubernetes label selector string matching all ARecords created
/// by Scout for a specific Ingress.
///
/// Selects on `managed-by=scout`, `source-cluster`, `source-namespace`, and
/// `source-name` to precisely target only the records owned by this Ingress.
pub fn arecord_label_selector(cluster: &str, namespace: &str, ingress_name: &str) -> String {
    format!(
        "{}={},{cluster_key}={cluster},{ns_key}={namespace},{name_key}={ingress_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

/// Builds a label selector string matching ARecords for the given Ingress that
/// belong to **any cluster other than `current_cluster`**.
///
/// Used to detect and clean up stale ARecords left behind when the scout is
/// restarted with a different `--cluster-name`.  The `!=` operator is supported
/// by the Kubernetes label selector language for equality-based requirements.
pub fn stale_arecord_label_selector(
    current_cluster: &str,
    namespace: &str,
    ingress_name: &str,
) -> String {
    format!(
        "{}={},{cluster_key}!={current_cluster},{ns_key}={namespace},{name_key}={ingress_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

// ============================================================================
// ARecord builder
// ============================================================================

/// Parameters for building an ARecord CR.
pub struct ARecordParams<'a> {
    /// Kubernetes resource name for the ARecord CR
    pub name: &'a str,
    /// Namespace where the ARecord CR will be created
    pub target_namespace: &'a str,
    /// DNS record name within the zone (e.g. `"app"` or `"@"`)
    pub record_name: &'a str,
    /// IPv4 addresses to use for the record (one or more)
    pub ips: &'a [String],
    /// Optional TTL override in seconds
    pub ttl: Option<i32>,
    /// Logical name of the source cluster (for labels)
    pub cluster_name: &'a str,
    /// Source Ingress namespace (for labels)
    pub ingress_namespace: &'a str,
    /// Source Ingress name (for labels)
    pub ingress_name: &'a str,
    /// DNS zone name (for labels)
    pub zone: &'a str,
}

/// Builds the ARecord CR that Scout will create on the target cluster.
pub fn build_arecord(params: ARecordParams<'_>) -> ARecord {
    let mut labels = BTreeMap::new();
    labels.insert(
        LABEL_MANAGED_BY.to_string(),
        LABEL_MANAGED_BY_SCOUT.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_CLUSTER.to_string(),
        params.cluster_name.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_NAMESPACE.to_string(),
        params.ingress_namespace.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_NAME.to_string(),
        params.ingress_name.to_string(),
    );
    labels.insert(LABEL_ZONE.to_string(), params.zone.to_string());

    let meta = kube::api::ObjectMeta {
        name: Some(params.name.to_string()),
        namespace: Some(params.target_namespace.to_string()),
        labels: Some(labels),
        ..Default::default()
    };

    ARecord {
        metadata: meta,
        spec: ARecordSpec {
            name: params.record_name.to_string(),
            ipv4_addresses: params.ips.to_vec(),
            ttl: params.ttl,
        },
        status: None,
    }
}

// ============================================================================
// Service helpers
// ============================================================================

/// Returns `true` if the Service is of type `LoadBalancer`.
///
/// `ClusterIP` and `NodePort` services have no routable external IP, so
/// Scout silently skips them without warning.
pub fn is_loadbalancer_service(svc: &Service) -> bool {
    svc.spec
        .as_ref()
        .and_then(|s| s.type_.as_deref())
        .map(|t| t == "LoadBalancer")
        .unwrap_or(false)
}

/// Extracts the first non-empty IP from the Service's LoadBalancer status.
///
/// Returns `None` if the status has no entries, or the first entry has no IP
/// (hostname-only entries are ignored). Scout re-queues and waits for the
/// cloud provider to assign an external IP.
pub fn resolve_ip_from_service_lb_status(svc: &Service) -> Option<String> {
    svc.status
        .as_ref()?
        .load_balancer
        .as_ref()?
        .ingress
        .as_ref()?
        .iter()
        .find_map(|entry| entry.ip.clone().filter(|ip| !ip.is_empty()))
}

/// Parses a `namespace/name` string into a [`NamespacedName`].
///
/// Whitespace around each segment is trimmed. Returns `None` unless the input
/// has exactly two non-empty segments separated by a single `/`.
#[must_use]
pub fn service_ref_from_str(s: &str) -> Option<NamespacedName> {
    let mut parts = s.split('/');
    let namespace = parts.next()?.trim();
    let name = parts.next()?.trim();
    if namespace.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(NamespacedName {
        namespace: namespace.to_string(),
        name: name.to_string(),
    })
}

/// Parses a single `gatewayClass` target: either `namespace/name` (explicit
/// Service) or `namespace/<label-selector>` (any entry whose Service part
/// contains `=`, since Service names never do).
///
/// The namespace is the segment before the first `/`; namespaces cannot contain
/// `/`, so a label selector's own `/` (e.g. `app.kubernetes.io/name=x`) is
/// preserved. Returns `None` for empty namespace or empty target.
#[must_use]
pub fn gateway_service_target_from_str(s: &str) -> Option<GatewayServiceTarget> {
    let (namespace, rest) = s.split_once('/')?;
    let namespace = namespace.trim();
    let rest = rest.trim();
    if namespace.is_empty() || rest.is_empty() {
        return None;
    }
    if rest.contains('=') {
        return Some(GatewayServiceTarget::Labeled {
            namespace: namespace.to_string(),
            selector: rest.to_string(),
        });
    }
    // No `=` → an explicit Service name. Reuse the strict `ns/name` parser,
    // which rejects extra slashes.
    service_ref_from_str(s).map(GatewayServiceTarget::Name)
}

/// Parses a single `class=<target>` mapping entry.
///
/// Splits on the first `=` (so a label selector's own `=` stays in the target).
/// Returns `None` for an empty class or an unparseable target. Used per-entry so
/// the repeatable `--gateway-service` CLI flag can carry multi-label selectors
/// (which contain commas) without them being mistaken for entry separators.
#[must_use]
pub fn parse_gateway_service_entry(entry: &str) -> Option<(String, GatewayServiceTarget)> {
    let (class, target) = entry.trim().split_once('=')?;
    let class = class.trim();
    if class.is_empty() {
        return None;
    }
    let target = gateway_service_target_from_str(target)?;
    Some((class.to_string(), target))
}

/// Parses the operator's `gatewayClass → LoadBalancer Service` map from a single
/// comma-separated string (the `BINDY_SCOUT_GATEWAY_SERVICES` env form).
///
/// Each entry is `class=<target>`, where `<target>` is either `namespace/name`
/// or `namespace/<label-selector>`, e.g.
/// `traefik=traefik/traefik,cilium=kube-system/app.kubernetes.io/name=cilium`.
/// Malformed entries are skipped. Because commas separate entries here,
/// multi-label selectors (which use commas) must be supplied via the repeatable
/// `--gateway-service` CLI flag instead. The map's keys double as the allow-list
/// of gateway classes Scout will follow.
#[must_use]
pub fn parse_gateway_services(raw: &str) -> BTreeMap<String, GatewayServiceTarget> {
    raw.split(',')
        .filter(|e| !e.trim().is_empty())
        .filter_map(parse_gateway_service_entry)
        .collect()
}

/// Extracts the IP-typed addresses from a Gateway's `status.addresses`.
///
/// An entry is treated as an IP when its `type` is `IPAddress`, or when the
/// `type` is absent but the `value` parses as an [`IpAddr`](std::net::IpAddr).
/// `Hostname`-typed entries are ignored. Returns an empty vec when the Gateway
/// has no addresses (the common case that forces the LoadBalancer-Service hop).
#[must_use]
pub fn gateway_addresses_as_ips(gw: &Gateway) -> Vec<String> {
    let Some(addresses) = gw.status.as_ref().and_then(|s| s.addresses.as_ref()) else {
        return Vec::new();
    };
    addresses
        .iter()
        .filter(|addr| match addr.r#type.as_deref() {
            Some("IPAddress") => true,
            Some("Hostname") => false,
            _ => addr.value.parse::<std::net::IpAddr>().is_ok(),
        })
        .map(|addr| addr.value.clone())
        .filter(|v| !v.is_empty())
        .collect()
}

/// Resolves a route's `parentRefs` to the Gateways they point at.
///
/// Only references to Gateway API Gateways are returned — an entry is kept when
/// its `group` is absent or `gateway.networking.k8s.io` AND its `kind` is absent
/// or `Gateway`. Each ref's namespace defaults to `route_namespace` when omitted.
#[must_use]
pub fn gateway_parent_refs(
    parent_refs: &[ParentReference],
    route_namespace: &str,
) -> Vec<NamespacedName> {
    parent_refs
        .iter()
        .filter(|r| {
            let group_ok = r
                .group
                .as_deref()
                .is_none_or(|g| g.is_empty() || g == GATEWAY_API_GROUP);
            let kind_ok = r.kind.as_deref().is_none_or(|k| k == GATEWAY_KIND);
            group_ok && kind_ok
        })
        .map(|r| NamespacedName {
            namespace: r
                .namespace
                .clone()
                .filter(|ns| !ns.is_empty())
                .unwrap_or_else(|| route_namespace.to_string()),
            name: r.name.clone(),
        })
        .collect()
}

/// Resolves the external IP of the LoadBalancer Service a gateway class maps to.
///
/// For a [`GatewayServiceTarget::Name`] the Service is fetched directly; for a
/// [`GatewayServiceTarget::Labeled`] target the namespace is listed by label
/// selector and the first LoadBalancer Service with an external IP is used.
/// Returns `None` (with a debug log) when the Service is unreachable, absent, or
/// has no external IP assigned yet.
async fn resolve_ip_from_gateway_service(
    client: &Client,
    target: &GatewayServiceTarget,
) -> Option<String> {
    match target {
        GatewayServiceTarget::Name(svc_ref) => {
            let svc_api: Api<Service> = Api::namespaced(client.clone(), &svc_ref.namespace);
            match svc_api.get(&svc_ref.name).await {
                Ok(svc) => resolve_ip_from_service_lb_status(&svc).or_else(|| {
                    debug!(service = %svc_ref.name, ns = %svc_ref.namespace,
                        "Gateway LoadBalancer Service has no external IP yet");
                    None
                }),
                Err(e) => {
                    debug!(service = %svc_ref.name, ns = %svc_ref.namespace, error = %e,
                        "Could not fetch Gateway's LoadBalancer Service");
                    None
                }
            }
        }
        GatewayServiceTarget::Labeled {
            namespace,
            selector,
        } => {
            let svc_api: Api<Service> = Api::namespaced(client.clone(), namespace);
            let lp = kube::api::ListParams::default().labels(selector);
            match svc_api.list(&lp).await {
                Ok(list) => list
                    .items
                    .iter()
                    .filter(|svc| is_loadbalancer_service(svc))
                    .find_map(resolve_ip_from_service_lb_status)
                    .or_else(|| {
                        debug!(ns = %namespace, selector = %selector,
                            "No LoadBalancer Service with an external IP matched the selector");
                        None
                    }),
                Err(e) => {
                    debug!(ns = %namespace, selector = %selector, error = %e,
                        "Could not list Gateway LoadBalancer Services by selector");
                    None
                }
            }
        }
    }
}

/// Follows a route's `parentRefs` back to the serving Gateway(s) and resolves
/// their external IP(s).
///
/// For each `parentRef` that points at a Gateway whose `gatewayClassName` is in
/// the operator-configured `gateway_services` map (the "running gateway
/// class"), Scout resolves the IP by:
///   1. reading the Gateway's `status.addresses` (IP-typed) when present, else
///   2. reading the mapped LoadBalancer Service's external IP.
///
/// Returns the de-duplicated IPs in discovery order, or `None` when nothing is
/// configured or nothing resolves. Individual lookup failures are logged and
/// skipped so one unreachable Gateway does not blank out the others.
///
/// # Arguments
/// * `client` - Local cluster client (Gateways and Services live on the workload cluster)
/// * `route_namespace` - Namespace of the route, used as the default parentRef namespace
/// * `parent_refs` - The route's `spec.parentRefs`
/// * `gateway_services` - Operator-configured `gatewayClass → Service` map / allow-list
pub async fn resolve_ips_from_gateways(
    client: &Client,
    route_namespace: &str,
    parent_refs: &[ParentReference],
    gateway_services: &BTreeMap<String, GatewayServiceTarget>,
) -> Option<Vec<String>> {
    if gateway_services.is_empty() || parent_refs.is_empty() {
        return None;
    }

    let mut ips: Vec<String> = Vec::new();
    for gw_ref in gateway_parent_refs(parent_refs, route_namespace) {
        let gw_api: Api<Gateway> = Api::namespaced(client.clone(), &gw_ref.namespace);
        let gateway = match gw_api.get(&gw_ref.name).await {
            Ok(gw) => gw,
            Err(e) => {
                debug!(gateway = %gw_ref.name, ns = %gw_ref.namespace, error = %e,
                    "Skipping parentRef Gateway that could not be fetched");
                continue;
            }
        };

        let class = &gateway.spec.gateway_class_name;
        let Some(target) = gateway_services.get(class) else {
            debug!(gateway = %gw_ref.name, class = %class,
                "Gateway class not in configured gateway-services — skipping");
            continue;
        };

        // Prefer the Gateway's own advertised addresses when present.
        let gw_ips = gateway_addresses_as_ips(&gateway);
        if !gw_ips.is_empty() {
            ips.extend(gw_ips);
            continue;
        }

        // Otherwise hop to the mapped LoadBalancer Service for its external IP.
        if let Some(ip) = resolve_ip_from_gateway_service(client, target).await {
            ips.push(ip);
        }
    }

    // De-duplicate preserving discovery order.
    let mut seen = std::collections::HashSet::new();
    ips.retain(|ip| seen.insert(ip.clone()));

    if ips.is_empty() {
        None
    } else {
        Some(ips)
    }
}

/// Derives the ARecord CR name for a Service.
///
/// Format: `scout-{cluster}-{namespace}-{service_name}`
///
/// No index suffix — unlike Ingress, a Service produces exactly one ARecord.
/// Applies the same sanitisation and 253-char truncation as Ingress CR names.
pub fn service_arecord_cr_name(cluster: &str, namespace: &str, service_name: &str) -> String {
    let raw = format!("{ARECORD_NAME_PREFIX}-{cluster}-{namespace}-{service_name}");
    let sanitized = sanitize_k8s_name(&raw);
    sanitized[..sanitized.len().min(MAX_K8S_NAME_LEN)].to_string()
}

/// Builds a Kubernetes label selector matching all ARecords created by Scout
/// for a specific Service.
pub fn service_arecord_label_selector(
    cluster: &str,
    namespace: &str,
    service_name: &str,
) -> String {
    format!(
        "{}={},{cluster_key}={cluster},{ns_key}={namespace},{name_key}={service_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

/// Parameters for building a Service-sourced ARecord CR.
pub struct ServiceARecordParams<'a> {
    /// Kubernetes resource name for the ARecord CR
    pub name: &'a str,
    /// Namespace where the ARecord CR will be created
    pub target_namespace: &'a str,
    /// DNS record name within the zone (e.g. `"my-svc"`)
    pub record_name: &'a str,
    /// IPv4 addresses for the record
    pub ips: &'a [String],
    /// Optional TTL override in seconds
    pub ttl: Option<i32>,
    /// Logical name of the source cluster (for labels)
    pub cluster_name: &'a str,
    /// Source Service namespace (for labels)
    pub service_namespace: &'a str,
    /// Source Service name (for labels)
    pub service_name: &'a str,
    /// DNS zone name (for labels)
    pub zone: &'a str,
}

/// Builds the ARecord CR that Scout will create for a `LoadBalancer` Service.
pub fn build_service_arecord(params: ServiceARecordParams<'_>) -> ARecord {
    let mut labels = BTreeMap::new();
    labels.insert(
        LABEL_MANAGED_BY.to_string(),
        LABEL_MANAGED_BY_SCOUT.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_CLUSTER.to_string(),
        params.cluster_name.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_NAMESPACE.to_string(),
        params.service_namespace.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_NAME.to_string(),
        params.service_name.to_string(),
    );
    labels.insert(LABEL_ZONE.to_string(), params.zone.to_string());

    let meta = kube::api::ObjectMeta {
        name: Some(params.name.to_string()),
        namespace: Some(params.target_namespace.to_string()),
        labels: Some(labels),
        ..Default::default()
    };

    ARecord {
        metadata: meta,
        spec: ARecordSpec {
            name: params.record_name.to_string(),
            ipv4_addresses: params.ips.to_vec(),
            ttl: params.ttl,
        },
        status: None,
    }
}

// ============================================================================
// Gateway API (HTTPRoute / TLSRoute) helpers
// ============================================================================

/// Derives the ARecord CR name for an HTTPRoute.
///
/// Format: `scout-{cluster}-{namespace}-{route_name}-{hostname_index}`
///
/// One ARecord per hostname in `spec.hostnames[]`. Index tracks which hostname
/// this ARecord is for. Applies the same sanitisation and 253-char truncation
/// as Ingress CR names.
pub fn httproute_arecord_cr_name(
    cluster: &str,
    namespace: &str,
    route_name: &str,
    hostname_index: usize,
) -> String {
    let raw = format!("{ARECORD_NAME_PREFIX}-{cluster}-{namespace}-{route_name}-{hostname_index}");
    let sanitized = sanitize_k8s_name(&raw);
    sanitized[..sanitized.len().min(MAX_K8S_NAME_LEN)].to_string()
}

/// Derives the ARecord CR name for a TLSRoute.
///
/// Format: `scout-{cluster}-{namespace}-{route_name}-{hostname_index}`
///
/// One ARecord per hostname in `spec.hostnames[]`. Index tracks which hostname
/// this ARecord is for. Applies the same sanitisation and 253-char truncation.
pub fn tlsroute_arecord_cr_name(
    cluster: &str,
    namespace: &str,
    route_name: &str,
    hostname_index: usize,
) -> String {
    let raw = format!("{ARECORD_NAME_PREFIX}-{cluster}-{namespace}-{route_name}-{hostname_index}");
    let sanitized = sanitize_k8s_name(&raw);
    sanitized[..sanitized.len().min(MAX_K8S_NAME_LEN)].to_string()
}

/// Builds a Kubernetes label selector matching all ARecords created by Scout
/// for a specific HTTPRoute.
pub fn httproute_arecord_label_selector(
    cluster: &str,
    namespace: &str,
    route_name: &str,
) -> String {
    format!(
        "{}={},{cluster_key}={cluster},{ns_key}={namespace},{name_key}={route_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

/// Builds a Kubernetes label selector matching all ARecords created by Scout
/// for a specific TLSRoute.
pub fn tlsroute_arecord_label_selector(cluster: &str, namespace: &str, route_name: &str) -> String {
    format!(
        "{}={},{cluster_key}={cluster},{ns_key}={namespace},{name_key}={route_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

/// Derives the ARecord CR name for a TCPRoute.
pub fn tcproute_arecord_cr_name(
    cluster: &str,
    namespace: &str,
    route_name: &str,
    hostname_index: usize,
) -> String {
    let raw = format!("{ARECORD_NAME_PREFIX}-{cluster}-{namespace}-{route_name}-{hostname_index}");
    let sanitized = sanitize_k8s_name(&raw);
    sanitized[..sanitized.len().min(MAX_K8S_NAME_LEN)].to_string()
}

/// Returns the effective hostnames for a TCPRoute.
///
/// When `spec.hostnames[]` is present, those values are used directly. When it is
/// absent but a record-name override is configured, Scout falls back to a single
/// placeholder hostname so the override can still be applied without requiring a
/// hostname to be present.
pub fn effective_tcproute_hostnames(
    annotations: &BTreeMap<String, String>,
    declared_hostnames: Option<&Vec<String>>,
) -> Vec<String> {
    if let Some(hostnames) = declared_hostnames {
        if !hostnames.is_empty() {
            return hostnames.clone();
        }
    }

    if get_record_name_annotation(annotations).is_some() {
        vec![String::new()]
    } else {
        Vec::new()
    }
}

/// Builds a Kubernetes label selector matching all ARecords created by Scout
/// for a specific TCPRoute.
pub fn tcproute_arecord_label_selector(cluster: &str, namespace: &str, route_name: &str) -> String {
    format!(
        "{}={},{cluster_key}={cluster},{ns_key}={namespace},{name_key}={route_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

/// Builds a label selector string matching ARecords for the given HTTPRoute that
/// belong to **any cluster other than `current_cluster`**.
///
/// Used to detect and clean up stale ARecords left behind when scout is
/// restarted with a different `--cluster-name`.
pub fn stale_httproute_arecord_label_selector(
    current_cluster: &str,
    namespace: &str,
    route_name: &str,
) -> String {
    format!(
        "{}={},{cluster_key}!={current_cluster},{ns_key}={namespace},{name_key}={route_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

/// Builds a label selector string matching ARecords for the given TLSRoute that
/// belong to **any cluster other than `current_cluster`**.
pub fn stale_tlsroute_arecord_label_selector(
    current_cluster: &str,
    namespace: &str,
    route_name: &str,
) -> String {
    format!(
        "{}={},{cluster_key}!={current_cluster},{ns_key}={namespace},{name_key}={route_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

/// Builds a label selector string matching ARecords for the given TCPRoute that
/// belong to **any cluster other than `current_cluster`**.
pub fn stale_tcproute_arecord_label_selector(
    current_cluster: &str,
    namespace: &str,
    route_name: &str,
) -> String {
    format!(
        "{}={},{cluster_key}!={current_cluster},{ns_key}={namespace},{name_key}={route_name}",
        LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT,
        cluster_key = LABEL_SOURCE_CLUSTER,
        ns_key = LABEL_SOURCE_NAMESPACE,
        name_key = LABEL_SOURCE_NAME,
    )
}

/// Parameters for building an ARecord CR from an HTTPRoute.
pub struct HTTPRouteARecordParams<'a> {
    /// Kubernetes resource name for the ARecord CR
    pub name: &'a str,
    /// Namespace where the ARecord CR will be created
    pub target_namespace: &'a str,
    /// DNS record name within the zone (e.g. `"api"`)
    pub record_name: &'a str,
    /// IPv4 addresses for the record
    pub ips: &'a [String],
    /// Optional TTL override in seconds
    pub ttl: Option<i32>,
    /// Logical name of the source cluster (for labels)
    pub cluster_name: &'a str,
    /// Source HTTPRoute namespace (for labels)
    pub route_namespace: &'a str,
    /// Source HTTPRoute name (for labels)
    pub route_name: &'a str,
    /// DNS zone name (for labels)
    pub zone: &'a str,
}

/// Builds the ARecord CR that Scout will create for an HTTPRoute.
pub fn build_httproute_arecord(params: HTTPRouteARecordParams<'_>) -> ARecord {
    let mut labels = BTreeMap::new();
    labels.insert(
        LABEL_MANAGED_BY.to_string(),
        LABEL_MANAGED_BY_SCOUT.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_CLUSTER.to_string(),
        params.cluster_name.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_NAMESPACE.to_string(),
        params.route_namespace.to_string(),
    );
    labels.insert(LABEL_SOURCE_NAME.to_string(), params.route_name.to_string());
    labels.insert(LABEL_ZONE.to_string(), params.zone.to_string());

    let meta = kube::api::ObjectMeta {
        name: Some(params.name.to_string()),
        namespace: Some(params.target_namespace.to_string()),
        labels: Some(labels),
        ..Default::default()
    };

    ARecord {
        metadata: meta,
        spec: ARecordSpec {
            name: params.record_name.to_string(),
            ipv4_addresses: params.ips.to_vec(),
            ttl: params.ttl,
        },
        status: None,
    }
}

/// Parameters for building an ARecord CR from a TLSRoute.
pub struct TLSRouteARecordParams<'a> {
    /// Kubernetes resource name for the ARecord CR
    pub name: &'a str,
    /// Namespace where the ARecord CR will be created
    pub target_namespace: &'a str,
    /// DNS record name within the zone (e.g. `"secure"`)
    pub record_name: &'a str,
    /// IPv4 addresses for the record
    pub ips: &'a [String],
    /// Optional TTL override in seconds
    pub ttl: Option<i32>,
    /// Logical name of the source cluster (for labels)
    pub cluster_name: &'a str,
    /// Source TLSRoute namespace (for labels)
    pub route_namespace: &'a str,
    /// Source TLSRoute name (for labels)
    pub route_name: &'a str,
    /// DNS zone name (for labels)
    pub zone: &'a str,
}

/// Builds the ARecord CR that Scout will create for a TLSRoute.
pub fn build_tlsroute_arecord(params: TLSRouteARecordParams<'_>) -> ARecord {
    let mut labels = BTreeMap::new();
    labels.insert(
        LABEL_MANAGED_BY.to_string(),
        LABEL_MANAGED_BY_SCOUT.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_CLUSTER.to_string(),
        params.cluster_name.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_NAMESPACE.to_string(),
        params.route_namespace.to_string(),
    );
    labels.insert(LABEL_SOURCE_NAME.to_string(), params.route_name.to_string());
    labels.insert(LABEL_ZONE.to_string(), params.zone.to_string());

    let meta = kube::api::ObjectMeta {
        name: Some(params.name.to_string()),
        namespace: Some(params.target_namespace.to_string()),
        labels: Some(labels),
        ..Default::default()
    };

    ARecord {
        metadata: meta,
        spec: ARecordSpec {
            name: params.record_name.to_string(),
            ipv4_addresses: params.ips.to_vec(),
            ttl: params.ttl,
        },
        status: None,
    }
}

/// Parameters for building an ARecord CR from a TCPRoute.
pub struct TCPRouteARecordParams<'a> {
    /// Kubernetes resource name for the ARecord CR
    pub name: &'a str,
    /// Namespace where the ARecord CR will be created
    pub target_namespace: &'a str,
    /// DNS record name within the zone (e.g. `"db"`)
    pub record_name: &'a str,
    /// IPv4 addresses for the record
    pub ips: &'a [String],
    /// Optional TTL override in seconds
    pub ttl: Option<i32>,
    /// Logical name of the source cluster (for labels)
    pub cluster_name: &'a str,
    /// Source TCPRoute namespace (for labels)
    pub route_namespace: &'a str,
    /// Source TCPRoute name (for labels)
    pub route_name: &'a str,
    /// DNS zone name (for labels)
    pub zone: &'a str,
}

/// Builds the ARecord CR that Scout will create for a TCPRoute.
pub fn build_tcproute_arecord(params: TCPRouteARecordParams<'_>) -> ARecord {
    let mut labels = BTreeMap::new();
    labels.insert(
        LABEL_MANAGED_BY.to_string(),
        LABEL_MANAGED_BY_SCOUT.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_CLUSTER.to_string(),
        params.cluster_name.to_string(),
    );
    labels.insert(
        LABEL_SOURCE_NAMESPACE.to_string(),
        params.route_namespace.to_string(),
    );
    labels.insert(LABEL_SOURCE_NAME.to_string(), params.route_name.to_string());
    labels.insert(LABEL_ZONE.to_string(), params.zone.to_string());

    let meta = kube::api::ObjectMeta {
        name: Some(params.name.to_string()),
        namespace: Some(params.target_namespace.to_string()),
        labels: Some(labels),
        ..Default::default()
    };

    ARecord {
        metadata: meta,
        spec: ARecordSpec {
            name: params.record_name.to_string(),
            ipv4_addresses: params.ips.to_vec(),
            ttl: params.ttl,
        },
        status: None,
    }
}

// ============================================================================
// Finalizer helpers (async — require Kubernetes API access)
// ============================================================================

/// Adds the Scout finalizer to an Ingress.
///
/// Merges the finalizer into the existing list so any other finalizers
/// already present are preserved.
async fn add_finalizer(client: &Client, ingress: &Ingress) -> Result<()> {
    let namespace = ingress.namespace().unwrap_or_default();
    let name = ingress.name_any();
    let api: Api<Ingress> = Api::namespaced(client.clone(), &namespace);

    let mut finalizers = ingress.metadata.finalizers.clone().unwrap_or_default();
    if !finalizers.contains(&FINALIZER_SCOUT.to_string()) {
        finalizers.push(FINALIZER_SCOUT.to_string());
    }

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Removes the Scout finalizer from an Ingress.
///
/// Preserves any other finalizers that may be present.
async fn remove_finalizer(client: &Client, ingress: &Ingress) -> Result<()> {
    let namespace = ingress.namespace().unwrap_or_default();
    let name = ingress.name_any();
    let api: Api<Ingress> = Api::namespaced(client.clone(), &namespace);

    let finalizers: Vec<String> = ingress
        .metadata
        .finalizers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f != FINALIZER_SCOUT)
        .collect();

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Adds the Scout finalizer to a Service.
async fn add_finalizer_to_service(client: &Client, svc: &Service) -> Result<()> {
    let namespace = svc.namespace().unwrap_or_default();
    let name = svc.name_any();
    let api: Api<Service> = Api::namespaced(client.clone(), &namespace);

    let mut finalizers = svc.metadata.finalizers.clone().unwrap_or_default();
    if !finalizers.contains(&FINALIZER_SCOUT.to_string()) {
        finalizers.push(FINALIZER_SCOUT.to_string());
    }

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Removes the Scout finalizer from a Service.
async fn remove_finalizer_from_service(client: &Client, svc: &Service) -> Result<()> {
    let namespace = svc.namespace().unwrap_or_default();
    let name = svc.name_any();
    let api: Api<Service> = Api::namespaced(client.clone(), &namespace);

    let finalizers: Vec<String> = svc
        .metadata
        .finalizers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f != FINALIZER_SCOUT)
        .collect();

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Adds the Scout finalizer to an HTTPRoute.
async fn add_finalizer_to_httproute(client: &Client, route: &HTTPRoute) -> Result<()> {
    let namespace = route.namespace().unwrap_or_default();
    let name = route.name_any();
    let api: Api<HTTPRoute> = Api::namespaced(client.clone(), &namespace);

    let mut finalizers = route.metadata.finalizers.clone().unwrap_or_default();
    if !finalizers.contains(&FINALIZER_SCOUT.to_string()) {
        finalizers.push(FINALIZER_SCOUT.to_string());
    }

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Removes the Scout finalizer from an HTTPRoute.
async fn remove_finalizer_from_httproute(client: &Client, route: &HTTPRoute) -> Result<()> {
    let namespace = route.namespace().unwrap_or_default();
    let name = route.name_any();
    let api: Api<HTTPRoute> = Api::namespaced(client.clone(), &namespace);

    let finalizers: Vec<String> = route
        .metadata
        .finalizers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f != FINALIZER_SCOUT)
        .collect();

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Adds the Scout finalizer to a TLSRoute.
async fn add_finalizer_to_tlsroute(client: &Client, route: &TLSRoute) -> Result<()> {
    let namespace = route.namespace().unwrap_or_default();
    let name = route.name_any();
    let api: Api<TLSRoute> = Api::namespaced(client.clone(), &namespace);

    let mut finalizers = route.metadata.finalizers.clone().unwrap_or_default();
    if !finalizers.contains(&FINALIZER_SCOUT.to_string()) {
        finalizers.push(FINALIZER_SCOUT.to_string());
    }

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Removes the Scout finalizer from a TLSRoute.
async fn remove_finalizer_from_tlsroute(client: &Client, route: &TLSRoute) -> Result<()> {
    let namespace = route.namespace().unwrap_or_default();
    let name = route.name_any();
    let api: Api<TLSRoute> = Api::namespaced(client.clone(), &namespace);

    let finalizers: Vec<String> = route
        .metadata
        .finalizers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f != FINALIZER_SCOUT)
        .collect();

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Adds the Scout finalizer to a TCPRoute.
async fn add_finalizer_to_tcproute(client: &Client, route: &TCPRoute) -> Result<()> {
    let namespace = route.namespace().unwrap_or_default();
    let name = route.name_any();
    let api: Api<TCPRoute> = Api::namespaced(client.clone(), &namespace);

    let mut finalizers = route.metadata.finalizers.clone().unwrap_or_default();
    if !finalizers.contains(&FINALIZER_SCOUT.to_string()) {
        finalizers.push(FINALIZER_SCOUT.to_string());
    }

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Removes the Scout finalizer from a TCPRoute.
async fn remove_finalizer_from_tcproute(client: &Client, route: &TCPRoute) -> Result<()> {
    let namespace = route.namespace().unwrap_or_default();
    let name = route.name_any();
    let api: Api<TCPRoute> = Api::namespaced(client.clone(), &namespace);

    let finalizers: Vec<String> = route
        .metadata
        .finalizers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f != FINALIZER_SCOUT)
        .collect();

    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

/// Deletes all ARecords in `target_namespace` that were created by Scout for
/// the given Ingress (identified by cluster + namespace + ingress name labels).
///
/// Must be called with the **remote** client so it targets the cluster where
/// ARecords live (which may differ from the local cluster in Phase 2+).
async fn delete_arecords_for_ingress(
    remote_client: &Client,
    target_namespace: &str,
    cluster: &str,
    ingress_namespace: &str,
    ingress_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector = arecord_label_selector(cluster, ingress_namespace, ingress_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            ingress = %ingress_name,
            ns = %ingress_namespace,
            "Deleted ARecord during Ingress cleanup"
        );
    }
    Ok(())
}

/// Deletes all ARecords in `target_namespace` that were created by Scout for
/// the given Ingress by a **previous** cluster name — i.e., any ARecord whose
/// `source-cluster` label differs from `current_cluster`.
///
/// This is called after every successful reconcile so that a scout restarted
/// with a new `--cluster-name` automatically cleans up the orphaned records
/// it left behind under the old name.
async fn delete_stale_cluster_arecords(
    remote_client: &Client,
    target_namespace: &str,
    current_cluster: &str,
    ingress_namespace: &str,
    ingress_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector = stale_arecord_label_selector(current_cluster, ingress_namespace, ingress_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        let old_cluster = ar
            .metadata
            .labels
            .as_ref()
            .and_then(|l| l.get(LABEL_SOURCE_CLUSTER))
            .map(String::as_str)
            .unwrap_or("unknown");
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            old_cluster = %old_cluster,
            new_cluster = %current_cluster,
            ingress = %ingress_name,
            ns = %ingress_namespace,
            "Deleted stale ARecord after cluster-name change"
        );
    }
    Ok(())
}

/// Deletes all ARecords in `target_namespace` that Scout created for the given Service.
///
/// Called during Service deletion and opt-out annotation removal.
async fn delete_arecords_for_service(
    remote_client: &Client,
    target_namespace: &str,
    cluster: &str,
    svc_namespace: &str,
    svc_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector = service_arecord_label_selector(cluster, svc_namespace, svc_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            service = %svc_name,
            ns = %svc_namespace,
            "Deleted ARecord during Service cleanup"
        );
    }
    Ok(())
}

/// Deletes all ARecords created by Scout for a specific HTTPRoute.
async fn delete_arecords_for_httproute(
    remote_client: &Client,
    target_namespace: &str,
    cluster: &str,
    route_namespace: &str,
    route_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector = httproute_arecord_label_selector(cluster, route_namespace, route_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            httproute = %route_name,
            ns = %route_namespace,
            "Deleted ARecord during HTTPRoute cleanup"
        );
    }
    Ok(())
}

/// Deletes all ARecords created by Scout for a specific TLSRoute.
async fn delete_arecords_for_tlsroute(
    remote_client: &Client,
    target_namespace: &str,
    cluster: &str,
    route_namespace: &str,
    route_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector = tlsroute_arecord_label_selector(cluster, route_namespace, route_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            tlsroute = %route_name,
            ns = %route_namespace,
            "Deleted ARecord during TLSRoute cleanup"
        );
    }
    Ok(())
}

/// Deletes stale ARecords for an HTTPRoute from previous cluster names.
async fn delete_stale_cluster_httproute_arecords(
    remote_client: &Client,
    target_namespace: &str,
    current_cluster: &str,
    route_namespace: &str,
    route_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector =
        stale_httproute_arecord_label_selector(current_cluster, route_namespace, route_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            httproute = %route_name,
            "Deleted stale HTTPRoute ARecord from previous cluster name"
        );
    }
    Ok(())
}

/// Deletes stale ARecords for a TLSRoute from previous cluster names.
async fn delete_stale_cluster_tlsroute_arecords(
    remote_client: &Client,
    target_namespace: &str,
    current_cluster: &str,
    route_namespace: &str,
    route_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector =
        stale_tlsroute_arecord_label_selector(current_cluster, route_namespace, route_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            tlsroute = %route_name,
            "Deleted stale TLSRoute ARecord from previous cluster name"
        );
    }
    Ok(())
}

/// Deletes all ARecords created by Scout for a specific TCPRoute.
async fn delete_arecords_for_tcproute(
    remote_client: &Client,
    target_namespace: &str,
    cluster: &str,
    route_namespace: &str,
    route_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector = tcproute_arecord_label_selector(cluster, route_namespace, route_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            tcproute = %route_name,
            ns = %route_namespace,
            "Deleted ARecord during TCPRoute cleanup"
        );
    }
    Ok(())
}

/// Deletes stale ARecords for a TCPRoute from previous cluster names.
async fn delete_stale_cluster_tcproute_arecords(
    remote_client: &Client,
    target_namespace: &str,
    current_cluster: &str,
    route_namespace: &str,
    route_name: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    let selector =
        stale_tcproute_arecord_label_selector(current_cluster, route_namespace, route_name);
    let lp = ListParams::default().labels(&selector);

    let arecords = api.list(&lp).await?;
    for ar in arecords.items {
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default()).await?;
        info!(
            arecord = %ar_name,
            tcproute = %route_name,
            "Deleted stale TCPRoute ARecord from previous cluster name"
        );
    }
    Ok(())
}

// ============================================================================
// Reconciler
// ============================================================================

/// Reconciles a single Ingress, creating or updating ARecord CRs as needed.
///
/// Handles the full lifecycle:
/// - Adds a finalizer to opted-in Ingresses so deletion is intercepted.
/// - On deletion, removes all ARecords Scout created then releases the finalizer.
/// - If the opt-in annotation is removed, cleans up ARecords and the finalizer.
///
/// # Errors
///
/// Returns an error that will be retried by the controller runtime.
async fn reconcile(ingress: Arc<Ingress>, ctx: Arc<ScoutContext>) -> Result<Action, ScoutError> {
    let name = ingress.name_any();
    let namespace = ingress.namespace().unwrap_or_default();

    // Skip excluded namespaces
    if ctx.excluded_namespaces.contains(&namespace) {
        debug!(ingress = %name, ns = %namespace, "Skipping excluded namespace");
        return Ok(Action::await_change());
    }

    // Handle Ingress deletion — remove ARecords and release the finalizer
    if is_being_deleted(&ingress) {
        if has_finalizer(&ingress) {
            info!(ingress = %name, ns = %namespace, "Ingress deleting — cleaning up ARecords");
            delete_arecords_for_ingress(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            delete_stale_cluster_arecords(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer(&ctx.client, &ingress)
                .await
                .map_err(ScoutError::from)?;
            info!(ingress = %name, ns = %namespace, "Finalizer removed — Ingress deletion unblocked");
        }
        return Ok(Action::await_change());
    }

    let annotations = ingress
        .metadata
        .annotations
        .as_ref()
        .cloned()
        .unwrap_or_default();

    // Guard: opt-in annotation required (scout-enabled: "true" or recordKind: "ARecord")
    if !is_scout_opted_in(&annotations) {
        // Annotation may have been removed after a finalizer was added — clean up
        if has_finalizer(&ingress) {
            info!(ingress = %name, ns = %namespace, "Scout opt-in annotation removed — cleaning up ARecords and finalizer");
            delete_arecords_for_ingress(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            delete_stale_cluster_arecords(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer(&ctx.client, &ingress)
                .await
                .map_err(ScoutError::from)?;
        }
        debug!(ingress = %name, ns = %namespace, "No arecord annotation — skipping");
        return Ok(Action::await_change());
    }

    // Ensure our finalizer is present before creating any ARecords.
    // Adding the finalizer triggers a re-reconcile; return early to avoid
    // doing record creation twice.
    if !has_finalizer(&ingress) {
        add_finalizer(&ctx.client, &ingress)
            .await
            .map_err(ScoutError::from)?;
        debug!(ingress = %name, ns = %namespace, "Finalizer added — re-queuing for record creation");
        return Ok(Action::await_change());
    }

    // Guard: zone required (annotation or operator default)
    let zone = match resolve_zone(&annotations, ctx.default_zone.as_deref()) {
        Some(z) => z,
        None => {
            warn!(ingress = %name, ns = %namespace, "No DNS zone available (set bindy.firestoned.io/zone annotation or BINDY_SCOUT_DEFAULT_ZONE) — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    };

    // Guard: a matching DNSZone must exist AND authorize this Ingress's
    // namespace (finding H1 — otherwise any tenant's Ingress could publish
    // into any zone Scout serves).
    match check_zone_authorization(&ctx.zone_store.state(), &zone, &namespace) {
        ZoneAuthz::Authorized => {}
        ZoneAuthz::Forbidden => {
            warn!(
                ingress = %name, ns = %namespace, zone = %zone,
                "Ingress namespace not authorized for zone — the DNSZone must live in this \
                 namespace or set annotation {ANNOTATION_ALLOW_ZONE_NAMESPACES} to include it \
                 (or '*') — skipping"
            );
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
        ZoneAuthz::NotFound => {
            warn!(
                ingress = %name, ns = %namespace, zone = %zone,
                "Zone not found in DNSZone store — skipping until zone appears"
            );
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    }

    // Resolve IPs: annotation override → default_ips → LB status
    let ips = match resolve_ips(&annotations, &ctx.default_ips, &ingress) {
        Some(ips) => ips,
        None => {
            warn!(ingress = %name, ns = %namespace, "No IP available (no annotation override, no default IPs, no LB status IP) — requeuing");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    };

    // Optional TTL override
    let ttl: Option<i32> = annotations.get(ANNOTATION_TTL).and_then(|v| v.parse().ok());

    let spec_rules = ingress
        .spec
        .as_ref()
        .and_then(|s| s.rules.as_ref())
        .cloned()
        .unwrap_or_default();

    let arecord_api: Api<ARecord> =
        Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);

    for (idx, rule) in spec_rules.iter().enumerate() {
        let host = match rule.host.as_deref() {
            Some(h) if !h.is_empty() => h,
            _ => {
                debug!(ingress = %name, rule_index = idx, "Ingress rule has no host — skipping");
                continue;
            }
        };

        let record_name = match resolve_record_name(&annotations, host, &zone) {
            Ok(n) => n,
            Err(e) => {
                warn!(ingress = %name, host = %host, zone = %zone, error = %e, "Host does not belong to zone — skipping rule");
                continue;
            }
        };

        let cr_name = arecord_cr_name(&ctx.cluster_name, &namespace, &name, idx);
        let arecord = build_arecord(ARecordParams {
            name: &cr_name,
            target_namespace: &ctx.target_namespace,
            record_name: &record_name,
            ips: &ips,
            ttl,
            cluster_name: &ctx.cluster_name,
            ingress_namespace: &namespace,
            ingress_name: &name,
            zone: &zone,
        });

        // Server-side apply
        let ssapply = kube::api::PatchParams::apply("bindy-scout").force();
        match arecord_api
            .patch(&cr_name, &ssapply, &kube::api::Patch::Apply(&arecord))
            .await
        {
            Ok(_) => {
                info!(arecord = %cr_name, ingress = %name, host = %host, ips = ?ips, "ARecord created/updated");
            }
            Err(e) => {
                error!(arecord = %cr_name, ingress = %name, error = %e, "Failed to apply ARecord");
                return Err(ScoutError::from(anyhow!(
                    "Failed to apply ARecord {cr_name}: {e}"
                )));
            }
        }
    }

    // Clean up any ARecords that were created by a previous cluster name for
    // this same Ingress — happens when scout is restarted with a new --cluster-name.
    delete_stale_cluster_arecords(
        &ctx.remote_client,
        &ctx.target_namespace,
        &ctx.cluster_name,
        &namespace,
        &name,
    )
    .await
    .map_err(ScoutError::from)?;

    Ok(Action::await_change())
}

///// Reconciles a single `LoadBalancer` Service, creating or updating an ARecord CR as needed.
///
/// Mirrors the Ingress reconciler lifecycle:
/// - Opts in via `bindy.firestoned.io/scout-enabled: "true"`.
/// - Silently skips non-`LoadBalancer` Services (no warning — ClusterIP/NodePort are intra-cluster).
/// - Adds a finalizer; on deletion removes the ARecord and releases it.
/// - If the opt-in annotation is removed, cleans up the ARecord and finalizer.
/// - Re-queues if no external IP is available yet (cloud provider may not have assigned one).
///
/// # Errors
///
/// Returns an error that will be retried by the controller runtime.
async fn reconcile_service(
    svc: Arc<Service>,
    ctx: Arc<ScoutContext>,
) -> Result<Action, ScoutError> {
    let name = svc.name_any();
    let namespace = svc.namespace().unwrap_or_default();

    if ctx.excluded_namespaces.contains(&namespace) {
        debug!(service = %name, ns = %namespace, "Skipping excluded namespace");
        return Ok(Action::await_change());
    }

    // Handle Service deletion — remove ARecord and release the finalizer
    if svc.metadata.deletion_timestamp.is_some() {
        if svc
            .metadata
            .finalizers
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
            .unwrap_or(false)
        {
            info!(service = %name, ns = %namespace, "Service deleting — cleaning up ARecord");
            delete_arecords_for_service(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer_from_service(&ctx.client, &svc)
                .await
                .map_err(ScoutError::from)?;
            info!(service = %name, ns = %namespace, "Finalizer removed — Service deletion unblocked");
        }
        return Ok(Action::await_change());
    }

    let annotations = svc
        .metadata
        .annotations
        .as_ref()
        .cloned()
        .unwrap_or_default();

    // Guard: opt-in annotation required
    if !is_scout_opted_in(&annotations) {
        let has_fin = svc
            .metadata
            .finalizers
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
            .unwrap_or(false);
        if has_fin {
            info!(service = %name, ns = %namespace, "Scout opt-in annotation removed — cleaning up ARecord and finalizer");
            delete_arecords_for_service(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer_from_service(&ctx.client, &svc)
                .await
                .map_err(ScoutError::from)?;
        }
        debug!(service = %name, ns = %namespace, "No scout-enabled annotation — skipping");
        return Ok(Action::await_change());
    }

    // Guard: only LoadBalancer services have routable external IPs
    if !is_loadbalancer_service(&svc) {
        debug!(service = %name, ns = %namespace, "Service is not LoadBalancer type — skipping");
        return Ok(Action::await_change());
    }

    // Ensure finalizer before creating any ARecord
    let has_fin = svc
        .metadata
        .finalizers
        .as_ref()
        .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
        .unwrap_or(false);
    if !has_fin {
        add_finalizer_to_service(&ctx.client, &svc)
            .await
            .map_err(ScoutError::from)?;
        debug!(service = %name, ns = %namespace, "Finalizer added — re-queuing for record creation");
        return Ok(Action::await_change());
    }

    // Guard: zone required
    let zone = match resolve_zone(&annotations, ctx.default_zone.as_deref()) {
        Some(z) => z,
        None => {
            warn!(service = %name, ns = %namespace, "No DNS zone available — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    };

    // Guard: a matching DNSZone must exist AND authorize this Service's
    // namespace (finding H1).
    match check_zone_authorization(&ctx.zone_store.state(), &zone, &namespace) {
        ZoneAuthz::Authorized => {}
        ZoneAuthz::Forbidden => {
            warn!(service = %name, ns = %namespace, zone = %zone, "Service namespace not authorized for zone — the DNSZone must live in this namespace or set annotation {ANNOTATION_ALLOW_ZONE_NAMESPACES} to include it (or '*') — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
        ZoneAuthz::NotFound => {
            warn!(service = %name, ns = %namespace, zone = %zone, "Zone not found in DNSZone store — requeuing");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    }

    // Resolve IPs: annotation (single or comma-separated) → default_ips → LB status
    let ips = {
        let from_annotation = resolve_ips_from_annotation(&annotations);
        let from_defaults = if ctx.default_ips.is_empty() {
            None
        } else {
            Some(ctx.default_ips.clone())
        };
        let from_lb = resolve_ip_from_service_lb_status(&svc).map(|ip| vec![ip]);

        match from_annotation.or(from_defaults).or(from_lb) {
            Some(ips) => ips,
            None => {
                warn!(service = %name, ns = %namespace, "No external IP yet — requeuing in {}s", SCOUT_ERROR_REQUEUE_SECS);
                return Ok(Action::requeue(Duration::from_secs(
                    SCOUT_ERROR_REQUEUE_SECS,
                )));
            }
        }
    };

    let ttl: Option<i32> = annotations.get(ANNOTATION_TTL).and_then(|v| v.parse().ok());

    // Derive the DNS record name: annotation override → "{service_name}.{zone}" stripped of zone
    let fqdn = format!("{name}.{zone}");
    let record_name = match resolve_record_name(&annotations, &fqdn, &zone) {
        Ok(n) => n,
        Err(e) => {
            warn!(service = %name, zone = %zone, error = %e, "Cannot derive record name — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    };

    let cr_name = service_arecord_cr_name(&ctx.cluster_name, &namespace, &name);
    let arecord = build_service_arecord(ServiceARecordParams {
        name: &cr_name,
        target_namespace: &ctx.target_namespace,
        record_name: &record_name,
        ips: &ips,
        ttl,
        cluster_name: &ctx.cluster_name,
        service_namespace: &namespace,
        service_name: &name,
        zone: &zone,
    });

    let arecord_api: Api<ARecord> =
        Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);
    let ssapply = kube::api::PatchParams::apply("bindy-scout").force();
    match arecord_api
        .patch(&cr_name, &ssapply, &kube::api::Patch::Apply(&arecord))
        .await
    {
        Ok(_) => {
            info!(arecord = %cr_name, service = %name, ips = ?ips, "ARecord created/updated for Service");
        }
        Err(e) => {
            error!(arecord = %cr_name, service = %name, error = %e, "Failed to apply ARecord for Service");
            return Err(ScoutError::from(anyhow!(
                "Failed to apply ARecord {cr_name}: {e}"
            )));
        }
    }

    Ok(Action::await_change())
}

/// Error policy for the Service controller: requeue with a fixed backoff.
fn service_error_policy(_obj: Arc<Service>, error: &ScoutError, _ctx: Arc<ScoutContext>) -> Action {
    error!(error = %error, "Scout service reconcile error — requeuing");
    Action::requeue(Duration::from_secs(SCOUT_ERROR_REQUEUE_SECS))
}

/// Error policy: requeue with a fixed backoff on any reconcile error.
fn error_policy(_obj: Arc<Ingress>, error: &ScoutError, _ctx: Arc<ScoutContext>) -> Action {
    error!(error = %error, "Scout reconcile error — requeuing");
    Action::requeue(Duration::from_secs(SCOUT_ERROR_REQUEUE_SECS))
}

// ============================================================================
// Gateway API (HTTPRoute / TLSRoute) Reconciliation
//
// Note: HTTPRoute and TLSRoute reconciliation follows the same pattern as
// Ingress reconciliation, with these differences:
//
// 1. HTTPRoute sources: `spec.hostnames[]` (array) instead of `spec.rules[].host`
// 2. TLSRoute sources: `spec.hostnames[]` (array) instead of routes with hosts
// 3. One ARecord per hostname with index suffix (like Ingress has one per rule)
// 4. Zone and IP resolution use the same annotation scheme as Ingress/Service
// ============================================================================

/// Reconciles a single `HTTPRoute` resource, creating or updating ARecord CRs as needed.
///
/// Mirrors the Ingress reconciler lifecycle:
/// - Opts in via `bindy.firestoned.io/scout-enabled: "true"`.
/// - Adds a finalizer; on deletion removes ARecords and releases it.
/// - If the opt-in annotation is removed, cleans up ARecords and finalizer.
/// - One ARecord created per hostname in `spec.hostnames[]` with an index suffix.
/// - Re-queues if zone is not found or no IP is available yet.
///
/// # Errors
///
/// Returns `ScoutError` if API calls fail (apply, delete, patch).
async fn reconcile_httproute(
    route: Arc<HTTPRoute>,
    ctx: Arc<ScoutContext>,
) -> Result<Action, ScoutError> {
    let name = route.name_any();
    let namespace = route.namespace().unwrap_or_default();

    // Guard: Skip excluded namespaces
    if ctx.excluded_namespaces.contains(&namespace) {
        debug!(httproute = %name, ns = %namespace, "Skipping excluded namespace");
        return Ok(Action::await_change());
    }

    // Handle HTTPRoute deletion — remove ARecords and release the finalizer
    if route.metadata.deletion_timestamp.is_some() {
        if route
            .metadata
            .finalizers
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
            .unwrap_or(false)
        {
            info!(httproute = %name, ns = %namespace, "HTTPRoute deleting — cleaning up ARecords");
            delete_arecords_for_httproute(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            delete_stale_cluster_httproute_arecords(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer_from_httproute(&ctx.client, &route)
                .await
                .map_err(ScoutError::from)?;
            info!(httproute = %name, ns = %namespace, "Finalizer removed — HTTPRoute deletion unblocked");
        }
        return Ok(Action::await_change());
    }

    let annotations = route
        .metadata
        .annotations
        .as_ref()
        .cloned()
        .unwrap_or_default();

    // Guard: opt-in annotation required
    if !is_scout_opted_in(&annotations) {
        let has_fin = route
            .metadata
            .finalizers
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
            .unwrap_or(false);
        if has_fin {
            info!(httproute = %name, ns = %namespace, "Scout opt-in annotation removed — cleaning up ARecords and finalizer");
            delete_arecords_for_httproute(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            delete_stale_cluster_httproute_arecords(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer_from_httproute(&ctx.client, &route)
                .await
                .map_err(ScoutError::from)?;
        }
        debug!(httproute = %name, ns = %namespace, "No scout-enabled annotation — skipping");
        return Ok(Action::await_change());
    }

    // Ensure finalizer before creating any ARecord
    let has_fin = route
        .metadata
        .finalizers
        .as_ref()
        .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
        .unwrap_or(false);
    if !has_fin {
        add_finalizer_to_httproute(&ctx.client, &route)
            .await
            .map_err(ScoutError::from)?;
        debug!(httproute = %name, ns = %namespace, "Finalizer added — re-queuing for record creation");
        return Ok(Action::await_change());
    }

    // Guard: zone required
    let zone = match resolve_zone(&annotations, ctx.default_zone.as_deref()) {
        Some(z) => z,
        None => {
            warn!(httproute = %name, ns = %namespace, "No DNS zone available — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    };

    // Guard: a matching DNSZone must exist AND authorize this HTTPRoute's
    // namespace (finding H1).
    match check_zone_authorization(&ctx.zone_store.state(), &zone, &namespace) {
        ZoneAuthz::Authorized => {}
        ZoneAuthz::Forbidden => {
            warn!(httproute = %name, ns = %namespace, zone = %zone, "HTTPRoute namespace not authorized for zone — the DNSZone must live in this namespace or set annotation {ANNOTATION_ALLOW_ZONE_NAMESPACES} to include it (or '*') — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
        ZoneAuthz::NotFound => {
            warn!(httproute = %name, ns = %namespace, zone = %zone, "Zone not found in DNSZone store — requeuing");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    }

    // Resolve IPs: annotation → gateway chain (parentRefs → Gateway → LB Service)
    // → default_ips → no routable IP = requeue
    let ips = {
        let from_annotation = resolve_ips_from_annotation(&annotations);
        let from_gateway = if from_annotation.is_some() {
            None
        } else {
            let parent_refs = route
                .spec
                .as_ref()
                .and_then(|s| s.parent_refs.as_ref())
                .cloned()
                .unwrap_or_default();
            resolve_ips_from_gateways(&ctx.client, &namespace, &parent_refs, &ctx.gateway_services)
                .await
        };
        let from_defaults = if ctx.default_ips.is_empty() {
            None
        } else {
            Some(ctx.default_ips.clone())
        };

        match from_annotation.or(from_gateway).or(from_defaults) {
            Some(ips) => ips,
            None => {
                warn!(httproute = %name, ns = %namespace, "No IP available (no annotation override, no gateway IP, no default IPs) — requeuing");
                return Ok(Action::requeue(Duration::from_secs(
                    SCOUT_ERROR_REQUEUE_SECS,
                )));
            }
        }
    };

    let ttl: Option<i32> = annotations.get(ANNOTATION_TTL).and_then(|v| v.parse().ok());

    // Extract hostnames from spec.hostnames[]
    let hostnames = route
        .spec
        .as_ref()
        .and_then(|s| s.hostnames.as_ref())
        .cloned()
        .unwrap_or_default();

    let arecord_api: Api<ARecord> =
        Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);

    for (idx, hostname) in hostnames.iter().enumerate() {
        if hostname.is_empty() {
            debug!(httproute = %name, hostname_index = idx, "HTTPRoute hostname is empty — skipping");
            continue;
        }

        let record_name = match resolve_record_name(&annotations, hostname, &zone) {
            Ok(n) => n,
            Err(e) => {
                warn!(httproute = %name, hostname = %hostname, zone = %zone, error = %e, "Hostname does not belong to zone — skipping");
                continue;
            }
        };

        let cr_name = httproute_arecord_cr_name(&ctx.cluster_name, &namespace, &name, idx);
        let arecord = build_httproute_arecord(HTTPRouteARecordParams {
            name: &cr_name,
            target_namespace: &ctx.target_namespace,
            record_name: &record_name,
            ips: &ips,
            ttl,
            cluster_name: &ctx.cluster_name,
            route_namespace: &namespace,
            route_name: &name,
            zone: &zone,
        });

        // Server-side apply
        let ssapply = kube::api::PatchParams::apply("bindy-scout").force();
        match arecord_api
            .patch(&cr_name, &ssapply, &kube::api::Patch::Apply(&arecord))
            .await
        {
            Ok(_) => {
                info!(arecord = %cr_name, httproute = %name, hostname = %hostname, ips = ?ips, "ARecord created/updated for HTTPRoute");
            }
            Err(e) => {
                error!(arecord = %cr_name, httproute = %name, error = %e, "Failed to apply ARecord for HTTPRoute");
                return Err(ScoutError::from(anyhow!(
                    "Failed to apply ARecord {cr_name}: {e}"
                )));
            }
        }
    }

    // Clean up stale ARecords from old cluster names
    delete_stale_cluster_httproute_arecords(
        &ctx.remote_client,
        &ctx.target_namespace,
        &ctx.cluster_name,
        &namespace,
        &name,
    )
    .await
    .map_err(ScoutError::from)?;

    Ok(Action::await_change())
}

/// Reconciles a single `TLSRoute` resource, creating or updating ARecord CRs as needed.
///
/// Identical to HTTPRoute reconciliation: both resources have `spec.hostnames[]`
/// and use the same annotation/IP resolution scheme.
///
/// # Errors
///
/// Returns `ScoutError` if API calls fail.
async fn reconcile_tlsroute(
    route: Arc<TLSRoute>,
    ctx: Arc<ScoutContext>,
) -> Result<Action, ScoutError> {
    let name = route.name_any();
    let namespace = route.namespace().unwrap_or_default();

    // Guard: Skip excluded namespaces
    if ctx.excluded_namespaces.contains(&namespace) {
        debug!(tlsroute = %name, ns = %namespace, "Skipping excluded namespace");
        return Ok(Action::await_change());
    }

    // Handle TLSRoute deletion — remove ARecords and release the finalizer
    if route.metadata.deletion_timestamp.is_some() {
        if route
            .metadata
            .finalizers
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
            .unwrap_or(false)
        {
            info!(tlsroute = %name, ns = %namespace, "TLSRoute deleting — cleaning up ARecords");
            delete_arecords_for_tlsroute(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            delete_stale_cluster_tlsroute_arecords(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer_from_tlsroute(&ctx.client, &route)
                .await
                .map_err(ScoutError::from)?;
            info!(tlsroute = %name, ns = %namespace, "Finalizer removed — TLSRoute deletion unblocked");
        }
        return Ok(Action::await_change());
    }

    let annotations = route
        .metadata
        .annotations
        .as_ref()
        .cloned()
        .unwrap_or_default();

    // Guard: opt-in annotation required
    if !is_scout_opted_in(&annotations) {
        let has_fin = route
            .metadata
            .finalizers
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
            .unwrap_or(false);
        if has_fin {
            info!(tlsroute = %name, ns = %namespace, "Scout opt-in annotation removed — cleaning up ARecords and finalizer");
            delete_arecords_for_tlsroute(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            delete_stale_cluster_tlsroute_arecords(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer_from_tlsroute(&ctx.client, &route)
                .await
                .map_err(ScoutError::from)?;
        }
        debug!(tlsroute = %name, ns = %namespace, "No scout-enabled annotation — skipping");
        return Ok(Action::await_change());
    }

    // Ensure finalizer before creating any ARecord
    let has_fin = route
        .metadata
        .finalizers
        .as_ref()
        .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
        .unwrap_or(false);
    if !has_fin {
        add_finalizer_to_tlsroute(&ctx.client, &route)
            .await
            .map_err(ScoutError::from)?;
        debug!(tlsroute = %name, ns = %namespace, "Finalizer added — re-queuing for record creation");
        return Ok(Action::await_change());
    }

    // Guard: zone required
    let zone = match resolve_zone(&annotations, ctx.default_zone.as_deref()) {
        Some(z) => z,
        None => {
            warn!(tlsroute = %name, ns = %namespace, "No DNS zone available — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    };

    // Guard: a matching DNSZone must exist AND authorize this TLSRoute's
    // namespace (finding H1).
    match check_zone_authorization(&ctx.zone_store.state(), &zone, &namespace) {
        ZoneAuthz::Authorized => {}
        ZoneAuthz::Forbidden => {
            warn!(tlsroute = %name, ns = %namespace, zone = %zone, "TLSRoute namespace not authorized for zone — the DNSZone must live in this namespace or set annotation {ANNOTATION_ALLOW_ZONE_NAMESPACES} to include it (or '*') — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
        ZoneAuthz::NotFound => {
            warn!(tlsroute = %name, ns = %namespace, zone = %zone, "Zone not found in DNSZone store — requeuing");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    }

    // Resolve IPs: annotation → gateway chain (parentRefs → Gateway → LB Service)
    // → default_ips → no routable IP = requeue
    let ips = {
        let from_annotation = resolve_ips_from_annotation(&annotations);
        let from_gateway = if from_annotation.is_some() {
            None
        } else {
            let parent_refs = route
                .spec
                .as_ref()
                .and_then(|s| s.parent_refs.as_ref())
                .cloned()
                .unwrap_or_default();
            resolve_ips_from_gateways(&ctx.client, &namespace, &parent_refs, &ctx.gateway_services)
                .await
        };
        let from_defaults = if ctx.default_ips.is_empty() {
            None
        } else {
            Some(ctx.default_ips.clone())
        };

        match from_annotation.or(from_gateway).or(from_defaults) {
            Some(ips) => ips,
            None => {
                warn!(tlsroute = %name, ns = %namespace, "No IP available (no annotation override, no gateway IP, no default IPs) — requeuing");
                return Ok(Action::requeue(Duration::from_secs(
                    SCOUT_ERROR_REQUEUE_SECS,
                )));
            }
        }
    };

    let ttl: Option<i32> = annotations.get(ANNOTATION_TTL).and_then(|v| v.parse().ok());

    // Extract hostnames from spec.hostnames[]
    let hostnames = route
        .spec
        .as_ref()
        .and_then(|s| s.hostnames.as_ref())
        .cloned()
        .unwrap_or_default();

    let arecord_api: Api<ARecord> =
        Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);

    for (idx, hostname) in hostnames.iter().enumerate() {
        if hostname.is_empty() {
            debug!(tlsroute = %name, hostname_index = idx, "TLSRoute hostname is empty — skipping");
            continue;
        }

        let record_name = match resolve_record_name(&annotations, hostname, &zone) {
            Ok(n) => n,
            Err(e) => {
                warn!(tlsroute = %name, hostname = %hostname, zone = %zone, error = %e, "Hostname does not belong to zone — skipping");
                continue;
            }
        };

        let cr_name = tlsroute_arecord_cr_name(&ctx.cluster_name, &namespace, &name, idx);
        let arecord = build_tlsroute_arecord(TLSRouteARecordParams {
            name: &cr_name,
            target_namespace: &ctx.target_namespace,
            record_name: &record_name,
            ips: &ips,
            ttl,
            cluster_name: &ctx.cluster_name,
            route_namespace: &namespace,
            route_name: &name,
            zone: &zone,
        });

        // Server-side apply
        let ssapply = kube::api::PatchParams::apply("bindy-scout").force();
        match arecord_api
            .patch(&cr_name, &ssapply, &kube::api::Patch::Apply(&arecord))
            .await
        {
            Ok(_) => {
                info!(arecord = %cr_name, tlsroute = %name, hostname = %hostname, ips = ?ips, "ARecord created/updated for TLSRoute");
            }
            Err(e) => {
                error!(arecord = %cr_name, tlsroute = %name, error = %e, "Failed to apply ARecord for TLSRoute");
                return Err(ScoutError::from(anyhow!(
                    "Failed to apply ARecord {cr_name}: {e}"
                )));
            }
        }
    }

    // Clean up stale ARecords from old cluster names
    delete_stale_cluster_tlsroute_arecords(
        &ctx.remote_client,
        &ctx.target_namespace,
        &ctx.cluster_name,
        &namespace,
        &name,
    )
    .await
    .map_err(ScoutError::from)?;

    Ok(Action::await_change())
}

/// Reconciles a single `TCPRoute` resource, creating or updating ARecord CRs as needed.
async fn reconcile_tcproute(
    route: Arc<TCPRoute>,
    ctx: Arc<ScoutContext>,
) -> Result<Action, ScoutError> {
    let name = route.name_any();
    let namespace = route.namespace().unwrap_or_default();

    if ctx.excluded_namespaces.contains(&namespace) {
        debug!(tcproute = %name, ns = %namespace, "Skipping excluded namespace");
        return Ok(Action::await_change());
    }

    if route.metadata.deletion_timestamp.is_some() {
        if route
            .metadata
            .finalizers
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
            .unwrap_or(false)
        {
            info!(tcproute = %name, ns = %namespace, "TCPRoute deleting — cleaning up ARecords");
            delete_arecords_for_tcproute(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            delete_stale_cluster_tcproute_arecords(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer_from_tcproute(&ctx.client, &route)
                .await
                .map_err(ScoutError::from)?;
            info!(tcproute = %name, ns = %namespace, "Finalizer removed — TCPRoute deletion unblocked");
        }
        return Ok(Action::await_change());
    }

    let annotations = route
        .metadata
        .annotations
        .as_ref()
        .cloned()
        .unwrap_or_default();

    if !is_scout_opted_in(&annotations) {
        let has_fin = route
            .metadata
            .finalizers
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
            .unwrap_or(false);
        if has_fin {
            info!(tcproute = %name, ns = %namespace, "Scout opt-in annotation removed — cleaning up ARecords and finalizer");
            delete_arecords_for_tcproute(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            delete_stale_cluster_tcproute_arecords(
                &ctx.remote_client,
                &ctx.target_namespace,
                &ctx.cluster_name,
                &namespace,
                &name,
            )
            .await
            .map_err(ScoutError::from)?;
            remove_finalizer_from_tcproute(&ctx.client, &route)
                .await
                .map_err(ScoutError::from)?;
        }
        debug!(tcproute = %name, ns = %namespace, "No scout-enabled annotation — skipping");
        return Ok(Action::await_change());
    }

    let has_fin = route
        .metadata
        .finalizers
        .as_ref()
        .map(|fs| fs.iter().any(|f| f == FINALIZER_SCOUT))
        .unwrap_or(false);
    if !has_fin {
        add_finalizer_to_tcproute(&ctx.client, &route)
            .await
            .map_err(ScoutError::from)?;
        debug!(tcproute = %name, ns = %namespace, "Finalizer added — re-queuing for record creation");
        return Ok(Action::await_change());
    }

    let zone = match resolve_zone(&annotations, ctx.default_zone.as_deref()) {
        Some(z) => z,
        None => {
            warn!(tcproute = %name, ns = %namespace, "No DNS zone available — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    };

    match check_zone_authorization(&ctx.zone_store.state(), &zone, &namespace) {
        ZoneAuthz::Authorized => {}
        ZoneAuthz::Forbidden => {
            warn!(tcproute = %name, ns = %namespace, zone = %zone, "TCPRoute namespace not authorized for zone — the DNSZone must live in this namespace or set annotation {ANNOTATION_ALLOW_ZONE_NAMESPACES} to include it (or '*') — skipping");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
        ZoneAuthz::NotFound => {
            warn!(tcproute = %name, ns = %namespace, zone = %zone, "Zone not found in DNSZone store — requeuing");
            return Ok(Action::requeue(Duration::from_secs(
                SCOUT_ERROR_REQUEUE_SECS,
            )));
        }
    }

    let ips = {
        let from_annotation = resolve_ips_from_annotation(&annotations);
        let from_gateway = if from_annotation.is_some() {
            None
        } else {
            let parent_refs = route
                .spec
                .as_ref()
                .and_then(|s| s.parent_refs.as_ref())
                .cloned()
                .unwrap_or_default();
            resolve_ips_from_gateways(&ctx.client, &namespace, &parent_refs, &ctx.gateway_services)
                .await
        };
        let from_defaults = if ctx.default_ips.is_empty() {
            None
        } else {
            Some(ctx.default_ips.clone())
        };

        match from_annotation.or(from_gateway).or(from_defaults) {
            Some(ips) => ips,
            None => {
                warn!(tcproute = %name, ns = %namespace, "No IP available (no annotation override, no gateway IP, no default IPs) — requeuing");
                return Ok(Action::requeue(Duration::from_secs(
                    SCOUT_ERROR_REQUEUE_SECS,
                )));
            }
        }
    };

    let ttl: Option<i32> = annotations.get(ANNOTATION_TTL).and_then(|v| v.parse().ok());

    let hostnames = effective_tcproute_hostnames(
        &annotations,
        route.spec.as_ref().and_then(|s| s.hostnames.as_ref()),
    );

    let arecord_api: Api<ARecord> =
        Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);

    for (idx, hostname) in hostnames.iter().enumerate() {
        let record_name = if let Some(override_name) = get_record_name_annotation(&annotations) {
            override_name
        } else if hostname.is_empty() {
            debug!(tcproute = %name, hostname_index = idx, "TCPRoute has no hostname and no record-name override — skipping");
            continue;
        } else {
            match resolve_record_name(&annotations, hostname, &zone) {
                Ok(name) => name,
                Err(e) => {
                    debug!(tcproute = %name, hostname = %hostname, zone = %zone, error = %e, "TCPRoute hostname did not resolve to a record name — using the hostname as-is");
                    hostname.to_string()
                }
            }
        };

        let cr_name = tcproute_arecord_cr_name(&ctx.cluster_name, &namespace, &name, idx);
        let arecord = build_tcproute_arecord(TCPRouteARecordParams {
            name: &cr_name,
            target_namespace: &ctx.target_namespace,
            record_name: &record_name,
            ips: &ips,
            ttl,
            cluster_name: &ctx.cluster_name,
            route_namespace: &namespace,
            route_name: &name,
            zone: &zone,
        });

        let ssapply = kube::api::PatchParams::apply("bindy-scout").force();
        match arecord_api
            .patch(&cr_name, &ssapply, &kube::api::Patch::Apply(&arecord))
            .await
        {
            Ok(_) => {
                info!(arecord = %cr_name, tcproute = %name, hostname = %hostname, ips = ?ips, "ARecord created/updated for TCPRoute");
            }
            Err(e) => {
                error!(arecord = %cr_name, tcproute = %name, error = %e, "Failed to apply ARecord for TCPRoute");
                return Err(ScoutError::from(anyhow!(
                    "Failed to apply ARecord {cr_name}: {e}"
                )));
            }
        }
    }

    delete_stale_cluster_tcproute_arecords(
        &ctx.remote_client,
        &ctx.target_namespace,
        &ctx.cluster_name,
        &namespace,
        &name,
    )
    .await
    .map_err(ScoutError::from)?;

    Ok(Action::await_change())
}

/// Error policy for Gateway API routes: requeue with a fixed backoff.
fn gateway_route_error_policy(
    _obj: Arc<HTTPRoute>,
    error: &ScoutError,
    _ctx: Arc<ScoutContext>,
) -> Action {
    error!(error = %error, "Scout HTTPRoute reconcile error — requeuing");
    Action::requeue(Duration::from_secs(SCOUT_ERROR_REQUEUE_SECS))
}

/// Error policy for TLSRoute: requeue with a fixed backoff.
fn tlsroute_error_policy(
    _obj: Arc<TLSRoute>,
    error: &ScoutError,
    _ctx: Arc<ScoutContext>,
) -> Action {
    error!(error = %error, "Scout TLSRoute reconcile error — requeuing");
    Action::requeue(Duration::from_secs(SCOUT_ERROR_REQUEUE_SECS))
}

/// Error policy for TCPRoute: requeue with a fixed backoff.
fn tcproute_error_policy(
    _obj: Arc<TCPRoute>,
    error: &ScoutError,
    _ctx: Arc<ScoutContext>,
) -> Action {
    error!(error = %error, "Scout TCPRoute reconcile error — requeuing");
    Action::requeue(Duration::from_secs(SCOUT_ERROR_REQUEUE_SECS))
}

// ============================================================================
// Remote client builder (Phase 2)
// ============================================================================

/// Builds a Kubernetes client from a kubeconfig stored in a Kubernetes Secret.
///
/// The Secret must contain a `kubeconfig` key in `.data` with a valid kubeconfig
/// YAML document. Used in Phase 2 to connect Scout (running in the workload cluster)
/// to the remote Bindy cluster where ARecords and DNSZones live.
///
/// # Errors
///
/// Returns an error if the Secret cannot be read, the `kubeconfig` key is absent,
/// the YAML is malformed, or the resulting client configuration is invalid.
async fn build_remote_client(
    local_client: &Client,
    secret_name: &str,
    secret_namespace: &str,
) -> Result<Client> {
    let api: Api<Secret> = Api::namespaced(local_client.clone(), secret_namespace);
    let secret = api.get(secret_name).await.map_err(|e| {
        anyhow!("Failed to read kubeconfig Secret {secret_namespace}/{secret_name}: {e}")
    })?;

    let kubeconfig_bytes = secret
        .data
        .as_ref()
        .and_then(|d| d.get("kubeconfig"))
        .ok_or_else(|| {
            anyhow!("Secret {secret_namespace}/{secret_name} has no 'kubeconfig' key in .data")
        })?;

    let kubeconfig_str = std::str::from_utf8(&kubeconfig_bytes.0)
        .map_err(|e| anyhow!("kubeconfig in Secret is not valid UTF-8: {e}"))?;

    let kubeconfig = Kubeconfig::from_yaml(kubeconfig_str)
        .map_err(|e| anyhow!("Failed to parse kubeconfig from Secret: {e}"))?;

    let config = kube::Config::from_custom_kubeconfig(kubeconfig, &KubeConfigOptions::default())
        .await
        .map_err(|e| anyhow!("Failed to build client config from kubeconfig: {e}"))?;

    Client::try_from(config).map_err(|e| anyhow!("Failed to create remote Kubernetes client: {e}"))
}

// ============================================================================
// Entry point
// ============================================================================

/// Reads scout configuration from environment variables.
struct ScoutConfig {
    target_namespace: String,
    cluster_name: String,
    excluded_namespaces: Vec<String>,
    /// Default IPs used when no per-Ingress annotation override or LB status IP is available.
    /// Set via `BINDY_SCOUT_DEFAULT_IPS` (comma-separated) or `--default-ips` CLI flag.
    default_ips: Vec<String>,
    /// `gatewayClass → LoadBalancer Service` map for gateway-chain IP resolution.
    /// Set via `BINDY_SCOUT_GATEWAY_SERVICES` (`class=ns/name,...`) or `--gateway-service`.
    gateway_services: BTreeMap<String, GatewayServiceTarget>,
    /// Default DNS zone applied to all Ingresses when no `bindy.firestoned.io/zone` annotation
    /// is present. Set via `BINDY_SCOUT_DEFAULT_ZONE` or `--default-zone` CLI flag.
    default_zone: Option<String>,
    /// Name of the Secret containing the remote cluster kubeconfig (Phase 2).
    /// When `None`, Scout operates in same-cluster mode.
    remote_secret_name: Option<String>,
    /// Namespace of the remote kubeconfig Secret. Defaults to Scout's own namespace.
    remote_secret_namespace: String,
}

impl ScoutConfig {
    /// Build configuration from environment variables, with optional CLI overrides.
    ///
    /// CLI arguments take precedence over environment variables when provided.
    fn from_env(
        cli_cluster_name: Option<String>,
        cli_namespace: Option<String>,
        cli_default_ips: Vec<String>,
        cli_gateway_services: Vec<String>,
        cli_default_zone: Option<String>,
    ) -> Result<Self> {
        let target_namespace = cli_namespace
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("BINDY_SCOUT_NAMESPACE").ok())
            .unwrap_or_else(|| DEFAULT_SCOUT_NAMESPACE.to_string());

        let cluster_name = cli_cluster_name
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("BINDY_SCOUT_CLUSTER_NAME").ok())
            .ok_or_else(|| {
                anyhow!("BINDY_SCOUT_CLUSTER_NAME is required (set via --cluster-name or env var)")
            })?;

        let own_namespace =
            std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "default".to_string());

        let mut excluded_namespaces: Vec<String> = std::env::var("BINDY_SCOUT_EXCLUDE_NAMESPACES")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect();

        // Always exclude Scout's own namespace
        if !excluded_namespaces.contains(&own_namespace) {
            excluded_namespaces.push(own_namespace.clone());
        }

        // CLI --default-ips takes precedence over BINDY_SCOUT_DEFAULT_IPS env var
        let default_ips = if !cli_default_ips.is_empty() {
            cli_default_ips
        } else {
            std::env::var("BINDY_SCOUT_DEFAULT_IPS")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect()
        };

        // CLI --gateway-service takes precedence over BINDY_SCOUT_GATEWAY_SERVICES env var.
        // CLI entries are parsed one-by-one (each flag is a single `class=target`) so a
        // multi-label selector's commas are not mistaken for entry separators; the env
        // form is a single comma-separated string.
        let gateway_services = if cli_gateway_services.is_empty() {
            parse_gateway_services(
                &std::env::var("BINDY_SCOUT_GATEWAY_SERVICES").unwrap_or_default(),
            )
        } else {
            cli_gateway_services
                .iter()
                .filter_map(|e| parse_gateway_service_entry(e))
                .collect()
        };

        // CLI --default-zone takes precedence over BINDY_SCOUT_DEFAULT_ZONE env var
        let default_zone = cli_default_zone.filter(|s| !s.is_empty()).or_else(|| {
            std::env::var("BINDY_SCOUT_DEFAULT_ZONE")
                .ok()
                .filter(|s| !s.is_empty())
        });

        let remote_secret_name = std::env::var("BINDY_SCOUT_REMOTE_SECRET")
            .ok()
            .filter(|s| !s.is_empty());

        let remote_secret_namespace =
            std::env::var("BINDY_SCOUT_REMOTE_SECRET_NAMESPACE").unwrap_or(own_namespace);

        Ok(Self {
            target_namespace,
            cluster_name,
            excluded_namespaces,
            default_ips,
            gateway_services,
            default_zone,
            remote_secret_name,
            remote_secret_namespace,
        })
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Converts a [`watcher::Error`] into a short, human-readable diagnosis string.
///
/// The kube-runtime watcher wraps all errors in a thin enum. This function
/// peels back the layers to surface the actionable cause: connection refused,
/// unauthorized, RBAC-forbidden, or a generic API / transport error.
fn diagnose_reflector_error(e: &watcher::Error) -> String {
    // Extract the phase label and the inner kube client error, handling the
    // two variants that don't carry a kube::Error directly.
    let (phase, client_err) = match e {
        watcher::Error::InitialListFailed(e) => ("initial list", e),
        watcher::Error::WatchStartFailed(e) => ("watch start", e),
        watcher::Error::WatchFailed(e) => ("watch stream", e),
        watcher::Error::WatchError(status) => {
            return format!(
                "API server returned error during watch: {} (HTTP {})",
                status.message, status.code
            );
        }
        watcher::Error::NoResourceVersion => {
            return "resource does not support watch (no resourceVersion returned)".to_string();
        }
    };

    let detail = match client_err {
        KubeError::Api(status) => match status.code {
            401 => format!(
                "unauthorized — check credentials/token ({})",
                status.message
            ),
            403 => format!("forbidden — check RBAC permissions ({})", status.message),
            code => format!("API error HTTP {code} — {}", status.message),
        },
        KubeError::Auth(e) => format!("authentication error — {e}"),
        KubeError::Service(e) => format!("cannot connect to API server — {e}"),
        KubeError::HyperError(e) => format!("HTTP transport error — {e}"),
        other => format!("{other}"),
    };

    format!("{phase} failed: {detail}")
}

/// Entry point for the `bindy scout` subcommand.
///
/// Initialises the Kubernetes client, builds reflector stores for `DNSZone`
/// resources (for zone validation), then runs the Ingress controller loop.
///
/// # Errors
///
/// Returns an error if the Kubernetes client cannot be initialised or if the
/// cluster name is not provided via CLI or the `BINDY_SCOUT_CLUSTER_NAME` env var.
pub async fn run_scout(
    cli_cluster_name: Option<String>,
    cli_namespace: Option<String>,
    cli_default_ips: Vec<String>,
    cli_gateway_services: Vec<String>,
    cli_default_zone: Option<String>,
) -> Result<()> {
    let config = ScoutConfig::from_env(
        cli_cluster_name,
        cli_namespace,
        cli_default_ips,
        cli_gateway_services,
        cli_default_zone,
    )?;

    let local_client = Client::try_default().await?;

    let remote_client = if let Some(ref secret_name) = config.remote_secret_name {
        info!(
            cluster = %config.cluster_name,
            target_ns = %config.target_namespace,
            secret = %secret_name,
            secret_ns = %config.remote_secret_namespace,
            excluded = ?config.excluded_namespaces,
            default_ips = ?config.default_ips,
            default_zone = ?config.default_zone,
            "Starting bindy scout in remote cluster mode"
        );
        build_remote_client(&local_client, secret_name, &config.remote_secret_namespace).await?
    } else {
        info!(
            cluster = %config.cluster_name,
            target_ns = %config.target_namespace,
            excluded = ?config.excluded_namespaces,
            default_ips = ?config.default_ips,
            default_zone = ?config.default_zone,
            "Starting bindy scout in same-cluster mode"
        );
        local_client.clone()
    };

    // Build a reflector store for DNSZone resources using the REMOTE client.
    // In same-cluster mode this is the local cluster; in Phase 2 this is the bindy cluster.
    // Scoped to the target namespace: DNSZones and ARecords always live in the same namespace
    // on the bindy cluster, so a namespaced watch is sufficient and avoids the need for a
    // cluster-scoped ClusterRole.
    let dnszone_api: Api<DNSZone> =
        Api::namespaced(remote_client.clone(), &config.target_namespace);
    let (dnszone_reader, dnszone_writer) = reflector::store();
    let dnszone_reflector = reflector(
        dnszone_writer,
        watcher(dnszone_api, WatcherConfig::default()),
    );

    // Start the DNSZone reflector in the background.
    // The kube-runtime watcher relies on the consumer to apply backoff: "You can apply your own
    // backoff by not polling the stream for a duration after errors." We sleep on each error so
    // that a repeated Connect failure doesn't spin in a tight logging loop.
    tokio::spawn(async move {
        dnszone_reflector
            .for_each(|event| async move {
                match event {
                    Ok(_) => {}
                    Err(e) => {
                        error!(diagnosis = %diagnose_reflector_error(&e), "DNSZone reflector error");
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            REFLECTOR_ERROR_BACKOFF_SECS,
                        ))
                        .await;
                    }
                }
            })
            .await;
    });

    let ctx = Arc::new(ScoutContext {
        client: local_client.clone(),
        remote_client,
        target_namespace: config.target_namespace,
        cluster_name: config.cluster_name,
        excluded_namespaces: config.excluded_namespaces,
        default_ips: config.default_ips,
        gateway_services: config.gateway_services,
        default_zone: config.default_zone,
        zone_store: dnszone_reader,
    });

    // Watch Ingresses across all namespaces using the LOCAL client
    let ingress_api: Api<Ingress> = Api::all(local_client.clone());
    // Watch Services across all namespaces using the LOCAL client
    let svc_api: Api<Service> = Api::all(local_client.clone());
    // Watch HTTPRoutes across all namespaces using the LOCAL client
    let httproute_api: Api<HTTPRoute> = Api::all(local_client.clone());
    // Watch TLSRoutes across all namespaces using the LOCAL client
    let tlsroute_api: Api<TLSRoute> = Api::all(local_client.clone());
    // Watch TCPRoutes across all namespaces using the LOCAL client
    let tcproute_api: Api<TCPRoute> = Api::all(local_client.clone());

    info!("Scout controller running — watching Ingresses, Services, HTTPRoutes, TLSRoutes, and TCPRoutes");

    let ingress_controller = Controller::new(ingress_api, WatcherConfig::default())
        .run(reconcile, error_policy, ctx.clone())
        .for_each(|res| async move {
            match res {
                Ok(obj) => debug!(obj = ?obj, "Reconciled Ingress"),
                Err(e) => error!(error = %e, "Ingress reconcile failed"),
            }
        });

    let service_controller = Controller::new(svc_api, WatcherConfig::default())
        .run(reconcile_service, service_error_policy, ctx.clone())
        .for_each(|res| async move {
            match res {
                Ok(obj) => debug!(obj = ?obj, "Reconciled Service"),
                Err(e) => error!(error = %e, "Service reconcile failed"),
            }
        });

    let httproute_controller = Controller::new(httproute_api, WatcherConfig::default())
        .run(reconcile_httproute, gateway_route_error_policy, ctx.clone())
        .for_each(|res| async move {
            match res {
                Ok(obj) => debug!(obj = ?obj, "Reconciled HTTPRoute"),
                Err(e) => error!(error = %e, "HTTPRoute reconcile failed"),
            }
        });

    let tlsroute_controller = Controller::new(tlsroute_api, WatcherConfig::default())
        .run(reconcile_tlsroute, tlsroute_error_policy, ctx.clone())
        .for_each(|res| async move {
            match res {
                Ok(obj) => debug!(obj = ?obj, "Reconciled TLSRoute"),
                Err(e) => error!(error = %e, "TLSRoute reconcile failed"),
            }
        });

    let tcproute_controller = Controller::new(tcproute_api, WatcherConfig::default())
        .run(reconcile_tcproute, tcproute_error_policy, ctx)
        .for_each(|res| async move {
            match res {
                Ok(obj) => debug!(obj = ?obj, "Reconciled TCPRoute"),
                Err(e) => error!(error = %e, "TCPRoute reconcile failed"),
            }
        });

    futures::future::join5(
        ingress_controller,
        service_controller,
        httproute_controller,
        tlsroute_controller,
        tcproute_controller,
    )
    .await;

    Ok(())
}
