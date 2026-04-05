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

/// Minimal HTTPRoute spec for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HTTPRouteSpec {
    /// Hostnames matching this HTTPRoute
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostnames: Option<Vec<String>>,
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

/// Annotation for explicitly overriding the IP used in the ARecord.
/// When set, takes precedence over the IP resolved from Ingress LoadBalancer status.
pub const ANNOTATION_IP: &str = "bindy.firestoned.io/ip";

/// Annotation for overriding the TTL (in seconds) on the created ARecord.
/// When absent, the ARecord inherits the TTL from the DNSZone spec.
pub const ANNOTATION_TTL: &str = "bindy.firestoned.io/ttl";

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

/// Returns the explicit IP override from `bindy.firestoned.io/ip`, if present.
///
/// Returns `None` if the annotation is absent or empty.
pub fn resolve_ip_from_annotation(annotations: &BTreeMap<String, String>) -> Option<String> {
    annotations
        .get(ANNOTATION_IP)
        .filter(|v| !v.is_empty())
        .cloned()
}

/// Resolves the IP address(es) to use for an ARecord, in priority order:
///
/// 1. `bindy.firestoned.io/ip` annotation — explicit single-IP override
/// 2. `default_ips` — operator-configured default IPs (e.g. shared Traefik ingress VIP)
/// 3. Ingress LoadBalancer status — first non-empty IP
///
/// Returns `None` if no IP can be determined from any source.
pub fn resolve_ips(
    annotations: &BTreeMap<String, String>,
    default_ips: &[String],
    ingress: &Ingress,
) -> Option<Vec<String>> {
    if let Some(ip) = resolve_ip_from_annotation(annotations) {
        return Some(vec![ip]);
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

    // Guard: zone must exist in the local DNSZone store
    let zone_exists = ctx
        .zone_store
        .state()
        .iter()
        .any(|z| z.spec.zone_name == zone);
    if !zone_exists {
        warn!(
            ingress = %name,
            ns = %namespace,
            zone = %zone,
            "Zone not found in DNSZone store — skipping until zone appears"
        );
        return Ok(Action::requeue(Duration::from_secs(
            SCOUT_ERROR_REQUEUE_SECS,
        )));
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

        let record_name = match derive_record_name(host, &zone) {
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

    // Guard: zone must exist in the DNSZone store
    let zone_exists = ctx
        .zone_store
        .state()
        .iter()
        .any(|z| z.spec.zone_name == zone);
    if !zone_exists {
        warn!(service = %name, ns = %namespace, zone = %zone, "Zone not found in DNSZone store — requeuing");
        return Ok(Action::requeue(Duration::from_secs(
            SCOUT_ERROR_REQUEUE_SECS,
        )));
    }

    // Resolve IP: annotation → default_ips → LB status
    let ips = {
        let from_annotation = annotations
            .get(ANNOTATION_IP)
            .filter(|v| !v.is_empty())
            .map(|v| vec![v.clone()]);
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

    // Derive the DNS record name: "{service_name}.{zone}" → strip zone → service_name
    let fqdn = format!("{name}.{zone}");
    let record_name = match derive_record_name(&fqdn, &zone) {
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

    // Guard: zone must exist in the DNSZone store
    let zone_exists = ctx
        .zone_store
        .state()
        .iter()
        .any(|z| z.spec.zone_name == zone);
    if !zone_exists {
        warn!(httproute = %name, ns = %namespace, zone = %zone, "Zone not found in DNSZone store — requeuing");
        return Ok(Action::requeue(Duration::from_secs(
            SCOUT_ERROR_REQUEUE_SECS,
        )));
    }

    // Resolve IPs: annotation → default_ips → no routable IP = requeue
    let ips = {
        let from_annotation = annotations
            .get(ANNOTATION_IP)
            .filter(|v| !v.is_empty())
            .map(|v| vec![v.clone()]);
        let from_defaults = if ctx.default_ips.is_empty() {
            None
        } else {
            Some(ctx.default_ips.clone())
        };

        match from_annotation.or(from_defaults) {
            Some(ips) => ips,
            None => {
                warn!(httproute = %name, ns = %namespace, "No IP available (no annotation override, no default IPs) — requeuing");
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

        let record_name = match derive_record_name(hostname, &zone) {
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

    // Guard: zone must exist in the DNSZone store
    let zone_exists = ctx
        .zone_store
        .state()
        .iter()
        .any(|z| z.spec.zone_name == zone);
    if !zone_exists {
        warn!(tlsroute = %name, ns = %namespace, zone = %zone, "Zone not found in DNSZone store — requeuing");
        return Ok(Action::requeue(Duration::from_secs(
            SCOUT_ERROR_REQUEUE_SECS,
        )));
    }

    // Resolve IPs: annotation → default_ips → no routable IP = requeue
    let ips = {
        let from_annotation = annotations
            .get(ANNOTATION_IP)
            .filter(|v| !v.is_empty())
            .map(|v| vec![v.clone()]);
        let from_defaults = if ctx.default_ips.is_empty() {
            None
        } else {
            Some(ctx.default_ips.clone())
        };

        match from_annotation.or(from_defaults) {
            Some(ips) => ips,
            None => {
                warn!(tlsroute = %name, ns = %namespace, "No IP available (no annotation override, no default IPs) — requeuing");
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

        let record_name = match derive_record_name(hostname, &zone) {
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
    cli_default_zone: Option<String>,
) -> Result<()> {
    let config = ScoutConfig::from_env(
        cli_cluster_name,
        cli_namespace,
        cli_default_ips,
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

    info!("Scout controller running — watching Ingresses, Services, HTTPRoutes, and TLSRoutes");

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
        .run(reconcile_tlsroute, tlsroute_error_policy, ctx)
        .for_each(|res| async move {
            match res {
                Ok(obj) => debug!(obj = ?obj, "Reconciled TLSRoute"),
                Err(e) => error!(error = %e, "TLSRoute reconcile failed"),
            }
        });

    futures::future::join4(
        ingress_controller,
        service_controller,
        httproute_controller,
        tlsroute_controller,
    )
    .await;

    Ok(())
}
