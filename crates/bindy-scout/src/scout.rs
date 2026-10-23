// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Bindy Scout: publishes `ARecord`s for opted-in workload-cluster objects.
//!
//! Scout watches Ingresses, LoadBalancer Services and Gateway API routes
//! (`HTTPRoute`, `TLSRoute`, `TCPRoute`) across all namespaces (except its own
//! and any configured exclusions). Every kind goes through one reconcile
//! pipeline (ADR-0022, see `crate::pipeline`); this module holds the types,
//! the pure helpers the pipeline is built from, the startup configuration and
//! [`run_scout`].
//!
//! See `.github/community/12-scout-ingress-controller.md` for the full design.
//!
//! ## Same-cluster mode
//!
//! Scout uses a single in-cluster client. ARecords are created in the same cluster.
//!
//! ## Remote cluster mode
//!
//! When `BINDY_SCOUT_REMOTE_SECRET` (or the ADR-0008 endpoint mode) is set,
//! Scout builds a second client (`remote_client`) targeting the dedicated Bindy
//! cluster. The local client watches sources and manages their finalizers; the
//! remote client creates and deletes ARecords and validates DNSZones.

use crate::constants::{ALLOW_ZONE_NAMESPACES_WILDCARD, ANNOTATION_ALLOW_ZONE_NAMESPACES};
use crate::crd::DNSZone;
use crate::guardrails::{
    first_ip_outside, parse_allowed_ip_ranges, parse_allowed_zones, zone_is_allowed, Ipv4Range,
};
use crate::pipeline::source_controller;
use anyhow::{anyhow, Context, Result};
use bindy_controller_sdk::kube_errors;
use bindy_controller_sdk::namespace_scope::NamespaceScope;
use bindy_controller_sdk::pagination::list_all_paginated;
use bindy_controller_sdk::shutdown::ShutdownSignal;
use bindy_controller_sdk::watch::WatchSet;
use k8s_openapi::api::core::v1::{Namespace, Service};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::jiff::Timestamp;
use kube::api::ListParams;
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::{
    runtime::{controller::Action, reflector},
    Api, Client, Resource, ResourceExt,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::fmt::{Debug, Write as _};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use tracing::{info, warn};

/// Reconcile error type: wraps `anyhow::Error` so that it satisfies the
/// `std::error::Error` bound required by `kube::runtime::Controller::run`.
///
/// It displays the whole cause chain (`outer: inner: ...`, anyhow's alternate
/// format), not only the outermost context: the reconcile-error log line is
/// where an operator sees why a write failed, and the reason (an admission
/// policy's denial, a conflict, a timeout) is the innermost cause.
#[derive(Debug, thiserror::Error)]
#[error("{0:#}")]
pub(crate) struct ScoutError(#[from] anyhow::Error);

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
pub(crate) struct ParentReference {
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
pub(crate) struct HTTPRouteSpec {
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
pub(crate) struct HTTPRoute {
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
pub(crate) struct TLSRouteSpec {
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
pub(crate) struct TLSRoute {
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
pub(crate) struct TCPRouteSpec {
    /// Rules for this TCPRoute (kept for schema compatibility with the
    /// Gateway API). Scout does not currently parse individual rule contents
    /// for TCPRoute (L4), but keeping the field avoids schema drift when the
    /// CR is round-tripped by the controller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<serde_json::Value>>,
    /// Gateways this route attaches to. Scout follows these to discover the
    /// serving Gateway's external IP when no explicit IP annotation is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_refs: Option<Vec<ParentReference>>,
}

/// Minimal TCPRoute definition for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TCPRoute {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: kube::api::ObjectMeta,
    #[serde(default)]
    pub spec: Option<TCPRouteSpec>,
}

/// Minimal GRPCRoute spec for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GRPCRouteSpec {
    /// Hostnames matching this GRPCRoute
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostnames: Option<Vec<String>>,
    /// Gateways this route attaches to. Scout follows these to discover the
    /// serving Gateway's external IP when no explicit IP annotation is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_refs: Option<Vec<ParentReference>>,
}

/// Minimal GRPCRoute definition for Scout's use case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct GRPCRoute {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: kube::api::ObjectMeta,
    #[serde(default)]
    pub spec: Option<GRPCRouteSpec>,
}

// Implement k8s_openapi::Metadata for HTTPRoute, GRPCRoute and TLSRoute
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

impl k8s_openapi::Metadata for GRPCRoute {
    type Ty = kube::api::ObjectMeta;
    fn metadata(&self) -> &kube::api::ObjectMeta {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut kube::api::ObjectMeta {
        &mut self.metadata
    }
}

// Implement k8s_openapi::Resource for HTTPRoute, GRPCRoute and TLSRoute
impl k8s_openapi::Resource for HTTPRoute {
    const API_VERSION: &'static str = "gateway.networking.k8s.io/v1";
    const GROUP: &'static str = "gateway.networking.k8s.io";
    const KIND: &'static str = "HTTPRoute";
    const VERSION: &'static str = "v1";
    const URL_PATH_SEGMENT: &'static str = "httproutes";
    type Scope = k8s_openapi::NamespaceResourceScope;
}

impl k8s_openapi::Resource for GRPCRoute {
    const API_VERSION: &'static str = "gateway.networking.k8s.io/v1";
    const GROUP: &'static str = "gateway.networking.k8s.io";
    const KIND: &'static str = "GRPCRoute";
    const VERSION: &'static str = "v1";
    const URL_PATH_SEGMENT: &'static str = "grpcroutes";
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
pub(crate) const GATEWAY_API_GROUP: &str = "gateway.networking.k8s.io";

/// Gateway API `kind` for a Gateway parent reference.
pub(crate) const GATEWAY_KIND: &str = "Gateway";

/// A namespaced object reference (`namespace` + `name`).
///
/// Used both for the operator-configured `gatewayClass → LoadBalancer Service`
/// map and for the Gateways a route's `parentRefs` point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamespacedName {
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
pub(crate) enum GatewayServiceTarget {
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

/// Minimal Gateway spec — only `gatewayClassName`, used to match the running class.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewaySpec {
    /// Name of the GatewayClass implementing this Gateway.
    pub gateway_class_name: String,
}

/// A single entry in `Gateway.status.addresses`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayStatusAddress {
    /// Address type, e.g. `IPAddress` or `Hostname`. Absent is treated as unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// The address value (an IP for `IPAddress`, a DNS name for `Hostname`).
    pub value: String,
}

/// Minimal Gateway status — only the assigned addresses.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayStatus {
    /// Addresses the controller has assigned to this Gateway (often empty when
    /// the controller publishes the external IP only on its LoadBalancer Service).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addresses: Option<Vec<GatewayStatusAddress>>,
}

/// Minimal Gateway definition for Scout's chain-following.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Gateway {
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
pub(crate) const ANNOTATION_RECORD_KIND: &str = "bindy.firestoned.io/recordKind";

/// Expected value of [`ANNOTATION_RECORD_KIND`] for A record creation.
pub(crate) const RECORD_KIND_ARECORD: &str = "ARecord";

/// Annotation specifying which DNS zone owns this Ingress host
pub(crate) const ANNOTATION_ZONE: &str = "bindy.firestoned.io/zone";

/// Simplified opt-in annotation — set to `"true"` to enable Scout for this Ingress.
/// Takes precedence over (and is preferred to) [`ANNOTATION_RECORD_KIND`] for new users.
/// Both annotations are accepted for backward compatibility.
pub(crate) const ANNOTATION_SCOUT_ENABLED: &str = "bindy.firestoned.io/scout-enabled";

/// Annotation for explicitly overriding the IP(s) used in the ARecord.
///
/// Accepts a single IP (`"10.0.0.1"`) or a comma-separated list of IPs
/// (`"10.0.0.1,10.0.0.2"`) — every entry becomes an address on the resulting
/// `ARecord`, in the order given. Whitespace around each entry is trimmed and
/// empty entries are skipped. When set, takes precedence over `--default-ips`
/// and any LoadBalancer status IP.
pub(crate) const ANNOTATION_IP: &str = "bindy.firestoned.io/ip";

/// Annotation for overriding the TTL (in seconds) on the created ARecord.
/// When absent, the ARecord inherits the TTL from the DNSZone spec.
pub(crate) const ANNOTATION_TTL: &str = "bindy.firestoned.io/ttl";

/// Annotation for overriding the DNS record name (`spec.name`) on the created ARecord.
///
/// When set, the value replaces the name normally derived from the source resource's
/// host/hostname. Use `"@"` to target the zone apex. When absent or empty, Scout falls
/// back to deriving the name from the host stripped of the zone suffix.
///
/// On multi-host resources (Ingress / HTTPRoute / GRPCRoute / TLSRoute) the override is applied to
/// every ARecord produced from that resource, so it is intended for single-host use cases.
pub(crate) const ANNOTATION_RECORD_NAME: &str = "bindy.firestoned.io/record-name";

/// Finalizer added to Ingresses managed by Scout to ensure cleanup on deletion
pub(crate) const FINALIZER_SCOUT: &str = "bindy.firestoned.io/arecord-finalizer";

/// Label placed on created ARecords identifying Scout as the manager
pub(crate) const LABEL_MANAGED_BY: &str = bindy_api::labels::BINDY_MANAGED_BY_LABEL;

/// Label value for ARecords created by Scout
pub(crate) const LABEL_MANAGED_BY_SCOUT: &str = bindy_api::labels::MANAGED_BY_SCOUT;

/// Label identifying the source cluster on created ARecords
pub(crate) const LABEL_SOURCE_CLUSTER: &str = bindy_api::labels::BINDY_SOURCE_CLUSTER_LABEL;

/// Label identifying the source namespace on the per-source `ARecord`s
/// written before ADR-0026 (read only, to hand them over)
pub(crate) const LABEL_SOURCE_NAMESPACE: &str = "bindy.firestoned.io/source-namespace";

/// Label identifying the source resource name on the per-source `ARecord`s
/// written before ADR-0026 (read only, to hand them over).
pub(crate) const LABEL_SOURCE_NAME: &str = "bindy.firestoned.io/source-name";

/// Label identifying the source resource kind (`Ingress`, `Service`,
/// `HTTPRoute`, `TLSRoute`, `TCPRoute`) on created ARecords (ADR-0022).
///
/// With the cluster, namespace and name labels it identified exactly one
/// source on the per-source records Scout wrote before ADR-0026. Scout no
/// longer sets these labels (a per-name record has a contributor label per
/// source instead); it reads them only to find and hand over such records.
/// Records without the kind label predate ADR-0022 and are migrated the same
/// way.
pub(crate) const LABEL_SOURCE_KIND: &str = "bindy.firestoned.io/source-kind";

/// Label carrying the DNS zone name on created ARecords (for DNSZone selector matching)
pub(crate) const LABEL_ZONE: &str = bindy_api::labels::BINDY_ZONE_LABEL;

/// Default namespace where ARecords are created when `BINDY_SCOUT_NAMESPACE` is not set
pub(crate) const DEFAULT_SCOUT_NAMESPACE: &str = "bindy-system";

/// Maximum Kubernetes resource name length in characters
const MAX_K8S_NAME_LEN: usize = 253;

/// Borrowed in place of a real annotations map on delete paths, so resolving
/// the zone for a deleted object's cleanup doesn't need to clone its
/// annotations just to read one key. Must be `static`, not `const` — a
/// `const` re-materializes a fresh temporary at each use site, which cannot
/// outlive the statement that borrows it.
pub(crate) static EMPTY_ANNOTATIONS: BTreeMap<String, String> = BTreeMap::new();

/// Prefix applied to all ARecord CR names created by Scout
const ARECORD_NAME_PREFIX: &str = "scout";

/// Requeue delay for non-fatal errors (seconds)
pub(crate) const SCOUT_ERROR_REQUEUE_SECS: u64 = 30;

/// Field manager Scout's server-side applies are recorded under.
pub(crate) const SCOUT_FIELD_MANAGER: &str = "bindy-scout";

/// The action every Scout reconcile returns when it must try again later:
/// a requeue after [`SCOUT_ERROR_REQUEUE_SECS`].
#[must_use]
pub(crate) fn requeue_after_error() -> Action {
    Action::requeue(Duration::from_secs(SCOUT_ERROR_REQUEUE_SECS))
}

/// Grace period, in seconds, that Scout keeps retrying remote ARecord cleanup
/// for an object that is being deleted before it releases its finalizer anyway.
///
/// The finalizer exists so Scout gets a chance to delete the ARecords it created
/// on the (possibly remote, Phase 2) Bindy cluster before the source object
/// disappears. But if that remote cluster is unreachable — a broken/expired
/// kubeconfig, a network partition — blocking finalizer removal on the remote
/// call would strand the tenant's Ingress/Service/route in `Terminating`
/// indefinitely, and because Scout adds this finalizer cluster-wide a single
/// broken remote connection could block deletions across every namespace.
///
/// Within the grace period Scout requeues and retries (so transient failures
/// still get cleaned up); past it, Scout releases the finalizer and logs that
/// the remote ARecords may be orphaned and must be reconciled separately.
pub(crate) const REMOTE_CLEANUP_GRACE_SECS: i64 = 300;

// ============================================================================
// Context
// ============================================================================

/// Shared context passed to every reconciler invocation.
pub(crate) struct ScoutContext {
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
    /// Kubernetes label selector restricting which namespaces Scout will act in (e.g.
    /// `"bindy.firestoned.io/scout-enabled=true"`). A source object's own per-resource opt-in
    /// annotation is still required in addition — the namespace must match this selector AND
    /// the object must carry the annotation. `None` means every namespace is eligible, matching
    /// pre-selector behavior; this is the default for backward compatibility, but production
    /// deployments are strongly encouraged to set it rather than run with cluster-wide scope
    /// (see `namespace_matches_selector` and the Scout guide). Set via
    /// `BINDY_SCOUT_NAMESPACE_SELECTOR` or `--namespace-selector`.
    pub namespace_selector: Option<String>,
    /// Zones Scout may publish into, with their subzones (ADR-0025). `None`
    /// means every zone. Set via `BINDY_SCOUT_ALLOWED_ZONES` or `--allowed-zones`.
    pub allowed_zones: Option<Vec<String>>,
    /// IPv4 ranges every published address must fall in (ADR-0025). `None`
    /// means every address. Set via `BINDY_SCOUT_ALLOWED_IP_RANGES` or
    /// `--allowed-ip-ranges`.
    pub allowed_ip_ranges: Option<Vec<Ipv4Range>>,
    /// Read-only store of DNSZone resources for zone validation.
    /// Populated from the remote client so zones are validated against the bindy cluster.
    pub zone_store: reflector::Store<DNSZone>,
}

// ============================================================================
// Namespace-selector helpers (async — require Kubernetes API access)
// ============================================================================

/// Returns `true` if `namespace` currently carries labels matching `selector`.
///
/// `selector` is a standard Kubernetes label selector expression (e.g.
/// `"bindy.firestoned.io/scout-enabled=true"`, or any expression accepted by
/// `kubectl -l`). Rather than re-implement label-selector parsing/matching
/// client-side, this delegates to the API server: it issues a `List` on
/// `Namespace` scoped to the single namespace by name (`fieldSelector:
/// metadata.name=<namespace>`) combined with the caller's label selector, and
/// checks whether the (0-or-1-item) result is non-empty. This guarantees
/// exactly the same selector semantics as `kubectl get ns -l <selector>`.
///
/// # Errors
///
/// Returns an error if the `List` call fails (e.g. RBAC-forbidden, transport
/// error, or a malformed selector rejected by the API server) — callers should
/// propagate this as a reconcile error (triggering a requeue with backoff)
/// rather than treat a failed check as "not matched", to avoid spuriously
/// cleaning up ARecords/finalizers on a transient API hiccup.
async fn namespace_matches_selector(
    client: &Client,
    namespace: &str,
    selector: &str,
) -> Result<bool> {
    let ns_api: Api<Namespace> = Api::all(client.clone());
    let lp = ListParams::default()
        .labels(selector)
        .fields(&format!("metadata.name={namespace}"));
    let list = list_all_paginated(&ns_api, lp).await.with_context(|| {
        format!("failed to check namespace '{namespace}' against selector '{selector}'")
    })?;
    Ok(!list.is_empty())
}

/// Returns whether `namespace` is eligible for Scout to act in.
///
/// When `selector` is `None` (the default), every namespace is eligible —
/// preserving pre-selector behavior for backward compatibility. When `selector`
/// is `Some`, this delegates to [`namespace_matches_selector`], so only
/// namespaces whose labels match the configured selector are eligible.
///
/// This is combined (AND) with the existing per-resource opt-in annotation
/// check (`is_scout_opted_in`) in each reconciler: both the namespace and the
/// individual Ingress/Service/route object must opt in.
///
/// # Errors
///
/// Propagates any error from [`namespace_matches_selector`].
pub(crate) async fn source_namespace_eligible(
    client: &Client,
    namespace: &str,
    selector: Option<&str>,
) -> Result<bool> {
    match selector {
        None => Ok(true),
        Some(sel) => namespace_matches_selector(client, namespace, sel).await,
    }
}

// ============================================================================
// Pure helper functions (tested in scout_tests.rs)
// ============================================================================

/// Returns `true` when an object that entered `Terminating` at
/// `deletion_timestamp` has been terminating for at least
/// [`REMOTE_CLEANUP_GRACE_SECS`] as of `now`.
///
/// Used during finalizer handling: when remote ARecord cleanup keeps failing,
/// this decides whether Scout should give up and release its finalizer (grace
/// expired) rather than strand the source object in `Terminating` forever.
///
/// Returns `false` when `deletion_timestamp` is `None` (the object is not being
/// deleted, so there is nothing to time out), when the grace period has not yet
/// elapsed (keep retrying), or when the timestamp is in the future (clock skew).
pub(crate) fn cleanup_grace_expired(deletion_timestamp: Option<&Time>, now: Timestamp) -> bool {
    match deletion_timestamp {
        Some(Time(started)) => now.duration_since(*started).as_secs() >= REMOTE_CLEANUP_GRACE_SECS,
        None => false,
    }
}

/// Returns `true` if the Ingress is annotated for ARecord creation.
///
/// The annotation `bindy.firestoned.io/recordKind` must have the value `"ARecord"` (case-sensitive).
/// Any other value (or absence of the annotation) returns `false`.
pub(crate) fn is_arecord_enabled(annotations: &BTreeMap<String, String>) -> bool {
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
pub(crate) fn is_scout_opted_in(annotations: &BTreeMap<String, String>) -> bool {
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
pub(crate) fn resolve_zone(
    annotations: &BTreeMap<String, String>,
    default_zone: Option<&str>,
) -> Option<String> {
    get_zone_annotation(annotations).or_else(|| default_zone.map(ToString::to_string))
}

/// Returns the DNS zone specified by the `bindy.firestoned.io/zone` annotation.
///
/// Returns `None` if the annotation is absent or has an empty value.
pub(crate) fn get_zone_annotation(annotations: &BTreeMap<String, String>) -> Option<String> {
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
pub(crate) fn derive_record_name(host: &str, zone: &str) -> Result<String> {
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

/// Maximum length of a single DNS label (RFC 1035 §2.3.4).
const MAX_DNS_LABEL_LEN: usize = 63;

/// Maximum length of a full DNS name in presentation form (RFC 1035 §2.3.4).
const MAX_DNS_NAME_LEN: usize = 253;

/// Validate a `bindy.firestoned.io/record-name` override as a legal relative DNS name.
///
/// The override deliberately bypasses host->zone matching — the annotator has
/// explicitly chosen the record name (see [`resolve_record_name`]). But the value
/// is set by whoever can edit the source Ingress / Service / Route, and it is
/// written into a zone file, so it must still be syntactically legal (audit
/// finding P2-7). Without this, a newline or space in the annotation could close
/// the current record and append another.
///
/// Accepts `@` (zone apex), and otherwise a dot-separated sequence of labels:
/// each 1-[`MAX_DNS_LABEL_LEN`] characters of ASCII alphanumerics, `-` or `_`,
/// starting and ending with an alphanumeric or `_`; a whole label of `*` is
/// allowed so wildcard records (`*`, `*.api`) keep working. The full name is
/// capped at [`MAX_DNS_NAME_LEN`].
///
/// # Errors
/// Returns an error describing the first violation found.
fn validate_record_name_override(name: &str) -> Result<()> {
    // Guard clause: the apex is a legal name but not a legal label.
    if name == "@" {
        return Ok(());
    }

    if name.len() > MAX_DNS_NAME_LEN {
        return Err(anyhow!(
            "record-name override {name:?} is {} characters; the DNS limit is {MAX_DNS_NAME_LEN}",
            name.len()
        ));
    }

    for label in name.split('.') {
        if label.is_empty() {
            return Err(anyhow!(
                "record-name override {name:?} has an empty label (leading, trailing or doubled '.')"
            ));
        }
        if label == "*" {
            continue;
        }
        if label.len() > MAX_DNS_LABEL_LEN {
            return Err(anyhow!(
                "record-name override {name:?} has a {}-character label; the DNS limit is {MAX_DNS_LABEL_LEN}",
                label.len()
            ));
        }
        if let Some(bad) = label
            .chars()
            .find(|c| !c.is_ascii_alphanumeric() && *c != '-' && *c != '_')
        {
            return Err(anyhow!(
                "record-name override {name:?} contains illegal character {bad:?} \
                 (allowed: ASCII letters, digits, '-', '_')"
            ));
        }
        // Unwraps are safe: the label is non-empty per the guard above.
        let first = label.chars().next().unwrap_or_default();
        let last = label.chars().next_back().unwrap_or_default();
        if !(first.is_ascii_alphanumeric() || first == '_')
            || !(last.is_ascii_alphanumeric() || last == '_')
        {
            return Err(anyhow!(
                "record-name override {name:?} has a label that starts or ends with '-'"
            ));
        }
    }

    Ok(())
}

/// Maximum length of a Kubernetes label value (a resolved zone is interpolated
/// directly into a `labelSelector`, which is subject to this same limit).
const MAX_K8S_LABEL_VALUE_LEN: usize = 63;

/// Returns `true` if `zone` is a legal Kubernetes label value, so it is safe
/// to interpolate into a `labelSelector` string.
///
/// A resolved zone comes from the `bindy.firestoned.io/zone` annotation
/// ([`get_zone_annotation`]) unvalidated — unlike
/// [`validate_record_name_override`], nothing rejects a DNS-idiomatic but
/// label-illegal value such as a trailing dot (`"example.com."`) or a name
/// over [`MAX_K8S_LABEL_VALUE_LEN`] characters. On the create path that is
/// harmless (zone authorization fails first), but on the stale-cluster
/// delete/opt-out paths the zone feeds a `list` call's `labelSelector`
/// directly — the API server rejects the request with a 400, which the
/// caller cannot distinguish from a transient error and retries for the
/// full cleanup grace period. Callers should validate the zone with this
/// function before using it in a stale-cluster selector, and fall back to
/// the no-zone-available skip path when it fails.
pub(crate) fn is_valid_zone_label_value(zone: &str) -> bool {
    if zone.is_empty() || zone.len() > MAX_K8S_LABEL_VALUE_LEN {
        return false;
    }
    let first = zone.chars().next().unwrap_or_default();
    let last = zone.chars().next_back().unwrap_or_default();
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return false;
    }
    zone.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Outcome of resolving a DNS zone that Scout can actually manage records for.
///
/// Distinguishes "no zone configured" from "a zone was configured but Scout
/// cannot use it", because the two need different operator-facing messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ZoneResolution {
    /// A zone resolved and is a legal Kubernetes label value.
    Usable(String),
    /// A zone resolved but cannot be used as a label value — carries the
    /// offending value so it can be named in the warning.
    Invalid(String),
    /// Neither the `bindy.firestoned.io/zone` annotation nor the operator
    /// default supplied a zone.
    Missing,
}

/// Resolves the DNS zone for a reconcile and checks Scout can actually use it.
///
/// [`resolve_zone`] returns the annotation (or operator default) verbatim. That
/// value is both stamped onto every ARecord as the [`LABEL_ZONE`] label *and*
/// interpolated into the stale-cluster `labelSelector`, so a value that is a
/// legal DNS name but not a legal label value breaks both: the server-side
/// apply fails with a 422, and the stale-cluster `list` fails with a 400 that
/// the reconcile error path retries indefinitely.
///
/// The `DNSZone` CRD constrains `zoneName` per DNS label but not in total, so
/// an ordinary internal zone such as
/// `payments-gateway.team-checkout.production.eu-west-1.example.internal`
/// (68 characters) passes admission and still fails the label-value rule.
/// Callers reject [`ZoneResolution::Invalid`] up front rather than letting it
/// reach either call.
///
/// # Arguments
/// * `annotations` - Annotations of the resource being reconciled
/// * `default_zone` - Operator-wide fallback (`BINDY_SCOUT_DEFAULT_ZONE`)
pub(crate) fn resolve_usable_zone(
    annotations: &BTreeMap<String, String>,
    default_zone: Option<&str>,
) -> ZoneResolution {
    match resolve_zone(annotations, default_zone) {
        None => ZoneResolution::Missing,
        Some(zone) if is_valid_zone_label_value(&zone) => ZoneResolution::Usable(zone),
        Some(zone) => ZoneResolution::Invalid(zone),
    }
}

/// Returns every DNS zone a delete/opt-out path should run stale-cluster
/// cleanup for: the zone resolved from the object's annotations, unioned with
/// the `zone` labels of the ARecords that were just deleted.
///
/// The annotation alone is not sufficient on these paths. Opting out is
/// documented as removing the `bindy.firestoned.io/*` annotations, and users
/// routinely strip them all in a single edit — so by the time cleanup runs the
/// zone annotation may already be gone. The deleted records' [`LABEL_ZONE`]
/// label is authoritative (Scout wrote it when it created them) and survives
/// that edit, so it recovers the zone the annotation no longer carries. It
/// also covers a changed `BINDY_SCOUT_DEFAULT_ZONE`, where the resolved zone
/// no longer matches what was actually written.
///
/// Values that are not legal Kubernetes label values are dropped, since they
/// cannot appear in a `labelSelector` — see [`is_valid_zone_label_value`].
///
/// # Arguments
/// * `resolved` - Zone from annotations/operator default, if any
/// * `from_records` - `zone` labels of the ARecords just deleted
pub(crate) fn stale_cleanup_zones(
    resolved: Option<&str>,
    from_records: &BTreeSet<String>,
) -> BTreeSet<String> {
    resolved
        .into_iter()
        .map(ToString::to_string)
        .chain(from_records.iter().cloned())
        .filter(|z| is_valid_zone_label_value(z))
        .collect()
}

/// Returns the explicit DNS record name override from `bindy.firestoned.io/record-name`.
///
/// The annotation value is trimmed of surrounding whitespace. Returns `None` if the
/// annotation is absent, empty, or whitespace-only.
pub(crate) fn get_record_name_annotation(annotations: &BTreeMap<String, String>) -> Option<String> {
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
/// The override's *syntax* is still checked by `validate_record_name_override`, because the
/// value reaches a zone file and the annotation is tenant-writable (audit finding P2-7).
///
/// # Errors
///
/// Returns an error when the override is present but is not a legal DNS name, or — when no
/// override is set — the error from [`derive_record_name`] if the host does not belong to
/// the zone.
pub(crate) fn resolve_record_name(
    annotations: &BTreeMap<String, String>,
    host: &str,
    zone: &str,
) -> Result<String> {
    if let Some(override_name) = get_record_name_annotation(annotations) {
        // P2-7: the override skips zone matching by design, but it still has to be
        // a legal DNS name — it ends up in a zone file.
        validate_record_name_override(&override_name)?;
        return Ok(override_name);
    }
    derive_record_name(host, zone)
}

/// Resolves the DNS record name for a `TCPRoute` ARecord.
///
/// `TCPRoute` has no hostnames, so the `bindy.firestoned.io/record-name`
/// override is its only source of a record name. The override still gets the
/// same syntax check as every other kind (audit finding P2-7), because it is
/// tenant-writable and ends up in a zone file.
///
/// Returns `Ok(None)` when no override is set, and `Ok(Some(name))` for a
/// legal override.
///
/// # Errors
///
/// Returns an error when the override is present but is not a legal DNS name.
pub(crate) fn tcproute_record_name(
    annotations: &BTreeMap<String, String>,
) -> Result<Option<String>> {
    let Some(override_name) = get_record_name_annotation(annotations) else {
        return Ok(None);
    };
    validate_record_name_override(&override_name)?;
    Ok(Some(override_name))
}

/// Returns the explicit IP overrides from the `bindy.firestoned.io/ip` annotation.
///
/// The value may be a single IP (`"10.0.0.1"`) or a comma-separated list
/// (`"10.0.0.1,10.0.0.2,10.0.0.3"`). Whitespace around each entry is trimmed
/// and empty entries are skipped, preserving order and duplicates.
///
/// Every entry is validated as an **IPv4 dotted-quad** before it is accepted
/// (audit finding P2-6). This annotation is set by whoever can edit the source
/// Ingress / Service / Route — i.e. a namespace tenant — and whatever survives
/// here is written verbatim into `ARecordSpec.ipv4Addresses` and rendered into a
/// zone file. IPv6 literals are rejected too: `ARecord` is IPv4-only, so an IPv6
/// address here would produce an invalid A record.
///
/// Invalid entries are dropped with a warning rather than failing the whole
/// record, so one fat-fingered entry in a list does not take down valid DNS.
///
/// Returns `None` if the annotation is absent, empty, contains only
/// separators/whitespace, or if **no** entry is a valid IPv4 address. `None`
/// (rather than an empty `Vec`) matters: it lets [`resolve_ips`] fall through to
/// `default_ips` and then the load-balancer status, whereas `Some(vec![])` would
/// create a record with no addresses.
pub(crate) fn resolve_ips_from_annotation(
    annotations: &BTreeMap<String, String>,
) -> Option<Vec<String>> {
    let raw = annotations.get(ANNOTATION_IP)?;
    let ips: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|entry| {
            if entry.parse::<std::net::Ipv4Addr>().is_ok() {
                return true;
            }
            warn!(
                annotation = ANNOTATION_IP,
                value = %entry,
                "Ignoring invalid IPv4 address in annotation — entries must be dotted-quad IPv4 (ARecord is IPv4-only)"
            );
            false
        })
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
pub(crate) fn zone_allows_source_namespace(zone: &DNSZone, source_namespace: &str) -> bool {
    let grant = zone_namespace_grant(zone, source_namespace);

    // P3-3: `*` re-opens the cross-tenant path that H1 closed. It stays supported —
    // some platform teams genuinely want a shared zone — but an armed wildcard should
    // never be silent, because it looks identical to an explicit grant from outside.
    if grant == NamespaceGrant::Wildcard {
        warn!(
            zone = %zone.name_any(),
            zone_namespace = %zone.namespace().unwrap_or_default(),
            source_namespace = %source_namespace,
            annotation = ANNOTATION_ALLOW_ZONE_NAMESPACES,
            "Cross-namespace DNS grant allowed by WILDCARD '*' — any namespace in the cluster \
             may create records in this zone. Replace '*' with an explicit namespace list \
             unless this zone is deliberately cluster-public."
        );
    }

    grant.is_authorized()
}

/// Why a [`DNSZone`] did (or did not) authorize a source namespace.
///
/// Kept separate from the boolean so a wildcard grant can be distinguished from an
/// explicit one — they authorize identically but have very different blast radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NamespaceGrant {
    /// The zone lives in the source's own namespace — no cross-namespace access.
    SameNamespace,
    /// The source namespace is named explicitly in the allow-list.
    ExplicitlyListed,
    /// The allow-list contains `*`; every namespace in the cluster is permitted.
    Wildcard,
    /// No rule authorizes this source namespace.
    Denied,
}

impl NamespaceGrant {
    /// Whether this grant permits the operation.
    #[must_use]
    pub fn is_authorized(self) -> bool {
        !matches!(self, Self::Denied)
    }
}

/// Classify how `zone` authorizes `source_namespace`, without logging.
///
/// An explicit listing is reported in preference to a wildcard when both are
/// present: the grant would have been made regardless of the `*`, so it is not the
/// wildcard's doing and should not raise a cross-namespace warning.
#[must_use]
pub(crate) fn zone_namespace_grant(zone: &DNSZone, source_namespace: &str) -> NamespaceGrant {
    if zone.namespace().as_deref() == Some(source_namespace) {
        return NamespaceGrant::SameNamespace;
    }
    let Some(annotations) = zone.metadata.annotations.as_ref() else {
        return NamespaceGrant::Denied;
    };
    let Some(value) = annotations.get(ANNOTATION_ALLOW_ZONE_NAMESPACES) else {
        return NamespaceGrant::Denied;
    };

    let mut wildcard = false;
    for entry in value.split(',').map(str::trim) {
        if entry == source_namespace {
            return NamespaceGrant::ExplicitlyListed;
        }
        if entry == ALLOW_ZONE_NAMESPACES_WILDCARD {
            wildcard = true;
        }
    }

    if wildcard {
        NamespaceGrant::Wildcard
    } else {
        NamespaceGrant::Denied
    }
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

/// The DNSZone that authorizes `source_namespace` for `zone_name`, if any.
///
/// Same matching rules as [`check_zone_authorization`], but returns the granting
/// object so the caller can re-verify it against the API server.
#[must_use]
pub(crate) fn authorizing_zone(
    zones: &[Arc<DNSZone>],
    zone_name: &str,
    source_namespace: &str,
) -> Option<Arc<DNSZone>> {
    zones
        .iter()
        .find(|zone| {
            zone.spec.zone_name == zone_name && zone_allows_source_namespace(zone, source_namespace)
        })
        .map(Arc::clone)
}

/// Authorize `zone_name` for `source_namespace`, re-checking the grant live.
///
/// [`check_zone_authorization`] reads a reflector cache, which lags the API server by
/// the watch latency. Between that read and the write of the `ARecord` there
/// is a window in which a DNSZone's
/// [`ANNOTATION_ALLOW_ZONE_NAMESPACES`] may have been tightened — the record would
/// then be published under a grant that no longer exists (audit finding P3-4).
///
/// This re-reads the *specific* zone that granted access, immediately before the
/// caller writes, shrinking the window from "watch latency" to "one API round trip".
/// It is a narrowing, not an elimination: a true elimination needs the write itself to
/// be conditional on the zone's `resourceVersion`, which a write to a
/// *different* object cannot express.
///
/// A failed live read is treated as **still authorized**: the cached grant was
/// affirmative, and failing closed on a transient API error would drop legitimate DNS
/// records during an API server blip. The error is logged.
pub(crate) async fn check_zone_authorization_live(
    client: &Client,
    zones: &[Arc<DNSZone>],
    zone_name: &str,
    source_namespace: &str,
) -> ZoneAuthz {
    let cached = check_zone_authorization(zones, zone_name, source_namespace);
    if cached != ZoneAuthz::Authorized {
        return cached;
    }

    let Some(granting) = authorizing_zone(zones, zone_name, source_namespace) else {
        return cached;
    };
    let (Some(ns), Some(name)) = (granting.namespace(), granting.metadata.name.clone()) else {
        return cached;
    };

    let api: Api<DNSZone> = Api::namespaced(client.clone(), &ns);
    match api.get(&name).await {
        Ok(live) => {
            if zone_allows_source_namespace(&live, source_namespace) {
                return ZoneAuthz::Authorized;
            }
            warn!(
                zone = %zone_name,
                dnszone = %name,
                dnszone_namespace = %ns,
                source_namespace = %source_namespace,
                "Zone authorization was revoked between the cached check and the write — \
                 refusing to publish (audit finding P3-4)"
            );
            ZoneAuthz::Forbidden
        }
        Err(e) => {
            warn!(
                zone = %zone_name,
                dnszone = %name,
                error = %e,
                "Could not re-verify zone authorization live; proceeding on the cached grant"
            );
            ZoneAuthz::Authorized
        }
    }
}

/// Keeps the entries of `ips` that are IPv4 addresses, warning about each
/// one that is not (ADR-0022, Decision 6).
///
/// `ARecord` is IPv4-only, so an IPv6 literal or a malformed value read from
/// a status field (Ingress or Service `status.loadBalancer`, Gateway
/// `status.addresses`) must never reach `ipv4Addresses`. `origin` names the
/// field in the warning.
///
/// # Arguments
/// * `ips` - Candidate addresses, in discovery order
/// * `origin` - Where they were read from, for the warning
pub(crate) fn retain_ipv4(ips: impl IntoIterator<Item = String>, origin: &str) -> Vec<String> {
    ips.into_iter()
        .filter(|ip| {
            if ip.parse::<std::net::Ipv4Addr>().is_ok() {
                return true;
            }
            warn!(origin = %origin, value = %ip, "Skipping a status address that is not IPv4 (ARecord is IPv4-only)");
            false
        })
        .collect()
}

/// [`retain_ipv4`], then drops repeated addresses, keeping the first
/// occurrence of each.
///
/// A load balancer can list one address more than once (one entry per port
/// or per pool); the `ARecord` holds each address once.
fn unique_ipv4(ips: impl IntoIterator<Item = String>, origin: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ips = retain_ipv4(ips, origin);
    ips.retain(|ip| seen.insert(ip.clone()));
    ips
}

/// Resolves the IPs to use for an ARecord from the Ingress load-balancer status.
///
/// Returns every IPv4 `ip` in `status.loadBalancer.ingress`, in status order
/// and without duplicates (issue #556). Hostname-only entries are ignored with
/// a warning, and non-IPv4 entries are skipped by [`retain_ipv4`]. Empty when
/// the load balancer has no IPv4 address yet.
pub(crate) fn resolve_ips_from_lb_status(ingress: &Ingress) -> Vec<String> {
    let Some(lb_ingresses) = ingress
        .status
        .as_ref()
        .and_then(|s| s.load_balancer.as_ref())
        .and_then(|lb| lb.ingress.as_ref())
    else {
        return Vec::new();
    };

    let mut ips = Vec::new();
    for lb in lb_ingresses {
        match lb.ip.as_deref() {
            Some(ip) if !ip.is_empty() => ips.push(ip.to_string()),
            _ if lb.hostname.is_some() => warn!(
                ingress = %ingress.name_any(),
                "Ingress LB status has hostname but no IP; an A record requires an IP address, skipping"
            ),
            _ => {}
        }
    }
    unique_ipv4(ips, "Ingress status.loadBalancer")
}

/// Parses the `bindy.firestoned.io/ttl` annotation.
///
/// Returns `Ok(None)` when the annotation is absent (the record inherits the
/// zone's TTL) and `Ok(Some(ttl))` for a positive integer.
///
/// # Errors
/// Returns an error when the annotation is present but is not a positive
/// integer. The pipeline treats that like an invalid `record-name` override:
/// it warns, publishes nothing for the source and requeues (ADR-0022,
/// Decision 6), instead of silently dropping the override.
pub(crate) fn parse_ttl_annotation(annotations: &BTreeMap<String, String>) -> Result<Option<i32>> {
    let Some(raw) = annotations.get(ANNOTATION_TTL) else {
        return Ok(None);
    };
    match raw.trim().parse::<i32>() {
        Ok(ttl) if ttl > 0 => Ok(Some(ttl)),
        _ => Err(anyhow!(
            "{ANNOTATION_TTL} annotation {raw:?} is not a positive integer number of seconds"
        )),
    }
}

/// Number of hex characters of the name-identity hash in a record name.
const NAME_HASH_HEX_LEN: usize = 8;

/// Hex characters needed to print one byte.
const HEX_CHARS_PER_BYTE: usize = 2;

/// Number of digest bytes that make up [`NAME_HASH_HEX_LEN`] hex characters.
const NAME_HASH_BYTES: usize = NAME_HASH_HEX_LEN / HEX_CHARS_PER_BYTE;

/// The readable stand-in for the zone apex (`@`) in a record name.
const APEX_READABLE: &str = "apex";

/// The first `bytes` bytes, in lowercase hex, of the SHA-256 of `parts`
/// joined by NUL.
///
/// The NUL separators make the identity unambiguous: no Kubernetes name,
/// kind or DNS name contains NUL, so `team-a`/`web` and `team`/`a-web` hash
/// differently even though their hyphen-joined forms are equal.
#[must_use]
pub(crate) fn nul_joined_sha256_hex(parts: &[&str], bytes: usize) -> String {
    Sha256::digest(parts.join("\0").as_bytes())
        .iter()
        .take(bytes)
        .fold(String::new(), |mut hex, b| {
            // Writing to a String cannot fail.
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// The short hash that makes a record name unique to its DNS name (ADR-0026,
/// Decision 1): the first [`NAME_HASH_HEX_LEN`] hex characters of the
/// SHA-256 of `cluster \0 zone \0 dns_name`, with the zone and DNS name
/// lowercased (DNS names are case-insensitive).
#[must_use]
pub(crate) fn name_identity_hash(cluster: &str, zone: &str, dns_name: &str) -> String {
    nul_joined_sha256_hex(
        &[cluster, &zone.to_lowercase(), &dns_name.to_lowercase()],
        NAME_HASH_BYTES,
    )
}

/// Builds the Kubernetes name of the one `ARecord` Scout writes for a DNS name
/// (ADR-0026, Decision 1): `scout-{cluster}-{dns-name}-{zone}-{hash}`, with
/// the apex spelled `apex`.
///
/// Every source of this Scout that resolves to the same zone and DNS name
/// writes this one record. The readable part is lowercased and sanitized
/// (see `sanitize_k8s_name`) and truncated so the whole name fits
/// [`MAX_K8S_NAME_LEN`]; a hyphen left at the cut is removed, so the result
/// is always a valid Kubernetes name. The hash (see [`name_identity_hash`])
/// keeps names unique across clusters, across names whose sanitized forms
/// collide (`a.b` and `a-b`, or the apex and a name called `apex`), and after
/// truncation.
///
/// # Arguments
/// * `cluster` - Scout's cluster name
/// * `zone` - The DNS zone
/// * `dns_name` - The record name within the zone (`@` for the apex)
#[must_use]
pub(crate) fn dns_name_record_cr_name(cluster: &str, zone: &str, dns_name: &str) -> String {
    let suffix = format!("-{}", name_identity_hash(cluster, zone, dns_name));
    let readable_name = if dns_name == "@" {
        APEX_READABLE
    } else {
        dns_name
    };
    let readable = sanitize_k8s_name(&format!(
        "{ARECORD_NAME_PREFIX}-{cluster}-{readable_name}-{zone}"
    ));
    let budget = MAX_K8S_NAME_LEN.saturating_sub(suffix.len());
    // `sanitize_k8s_name` yields ASCII only, so a byte cut is a char cut.
    let readable = readable[..readable.len().min(budget)].trim_end_matches('-');
    format!("{readable}{suffix}")
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
            continue;
        }
        // Replace any non-alphanumeric character with a hyphen (collapsing runs)
        if !last_was_hyphen {
            result.push('-');
            last_was_hyphen = true;
        }
    }

    result.trim_matches('-').to_string()
}

/// Returns `true` if `obj` has been marked for deletion (it carries a
/// `deletionTimestamp`), whatever its kind.
pub(crate) fn is_being_deleted<K: Resource>(obj: &K) -> bool {
    obj.meta().deletion_timestamp.is_some()
}

/// The label selector matching the per-source `ARecord`s Scout wrote for one
/// source object under the current cluster name before ADR-0026:
/// `managed-by=scout`, `source-cluster`, `source-kind`, `source-namespace`
/// and `source-name`. The pipeline hands such records over to per-name
/// records, and deletes them when the source goes away.
///
/// Every per-source lookup (handover, opt-out, release and deletion cleanup)
/// uses this one selector, so a record is owned by exactly one
/// (cluster, kind, namespace, name) source (ADR-0022, Decision 2).
#[must_use]
pub fn arecord_label_selector(cluster: &str, kind: &str, namespace: &str, name: &str) -> String {
    format!(
        "{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}={cluster},\
         {LABEL_SOURCE_KIND}={kind},{LABEL_SOURCE_NAMESPACE}={namespace},{LABEL_SOURCE_NAME}={name}"
    )
}

/// The label selector matching the legacy ARecords an earlier Scout wrote
/// for one source object: the source's cluster, namespace and name labels,
/// and **no** `source-kind` label (`!bindy.firestoned.io/source-kind`).
///
/// Records from before ADR-0022 carry no kind, so they cannot be told apart
/// by kind; the pipeline deletes them after it has applied the source's
/// new-named records (ADR-0022, Decision 4).
#[must_use]
pub fn legacy_arecord_label_selector(cluster: &str, namespace: &str, name: &str) -> String {
    format!(
        "{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}={cluster},\
         !{LABEL_SOURCE_KIND},{LABEL_SOURCE_NAMESPACE}={namespace},{LABEL_SOURCE_NAME}={name}"
    )
}

/// The kind clause of a stale-cluster selector: either one source kind, or
/// (for records written before ADR-0022) no kind label at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleKind<'a> {
    /// Records carrying `source-kind=<kind>`.
    Kind(&'a str),
    /// Legacy records with no `source-kind` label.
    Legacy,
}

/// Builds the zone-less core of every stale-cluster selector:
/// `managed-by=scout,source-cluster!=<current>,<kind clause>,
/// source-namespace=<ns>,source-name=<name>`.
///
/// The stale-cluster cleanup and the diagnostic listing for a missing zone
/// are both built from this one function, so the diagnostic cannot report
/// different records than the cleanup would delete.
///
/// # Arguments
/// * `current_cluster` - Cluster name to exclude (the one running this Scout)
/// * `kind` - The source kind, or [`StaleKind::Legacy`] for unlabelled records
/// * `namespace` - Source namespace
/// * `name` - Source name
#[must_use]
pub(crate) fn stale_selector_base(
    current_cluster: &str,
    kind: StaleKind<'_>,
    namespace: &str,
    name: &str,
) -> String {
    let kind_clause = match kind {
        StaleKind::Kind(kind) => format!("{LABEL_SOURCE_KIND}={kind}"),
        StaleKind::Legacy => format!("!{LABEL_SOURCE_KIND}"),
    };
    format!(
        "{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}!={current_cluster},\
         {kind_clause},{LABEL_SOURCE_NAMESPACE}={namespace},{LABEL_SOURCE_NAME}={name}"
    )
}

/// Builds a label selector matching a source's ARecords that belong to
/// **any cluster other than `current_cluster`, in the same DNS zone**.
///
/// Used to clean up the records a Scout restarted with a different
/// `--cluster-name` left behind. The `zone=current_zone` clause narrows the
/// match to clusters publishing into the *same* DNS zone: without it, two
/// unrelated clusters sharing a namespace and source name but publishing into
/// **different** zones would delete each other's live records (#474).
///
/// **This does not fully identify "this same Scout under a prior cluster
/// name."** Any cluster publishing into the same zone with a matching kind,
/// namespace and name still matches. Closing that gap needs a stable
/// instance marker, which this selector does not provide.
///
/// `current_zone` must be a legal Kubernetes label value (at most 63
/// characters, alphanumeric at both ends); an illegal one makes the API
/// server reject the `list` with a 400.
#[must_use]
pub fn stale_arecord_label_selector(
    current_cluster: &str,
    kind: StaleKind<'_>,
    namespace: &str,
    name: &str,
    current_zone: &str,
) -> String {
    format!(
        "{},{LABEL_ZONE}={current_zone}",
        stale_selector_base(current_cluster, kind, namespace, name)
    )
}

// ============================================================================
// Service helpers
// ============================================================================

/// Returns `true` if the Service is of type `LoadBalancer`.
///
/// `ClusterIP` and `NodePort` services have no routable external IP, so
/// Scout silently skips them without warning.
pub(crate) fn is_loadbalancer_service(svc: &Service) -> bool {
    svc.spec
        .as_ref()
        .and_then(|s| s.type_.as_deref())
        .map(|t| t == "LoadBalancer")
        .unwrap_or(false)
}

/// Extracts every IPv4 address from the Service's LoadBalancer status, in
/// status order and without duplicates (issue #556).
///
/// Returns an empty list if the status has no entries or no entry carries an
/// IPv4 address (hostname-only entries are ignored, non-IPv4 ones are skipped by
/// [`retain_ipv4`]). Scout re-queues and waits for the cloud provider to
/// assign an external IP.
pub(crate) fn resolve_ips_from_service_lb_status(svc: &Service) -> Vec<String> {
    let Some(entries) = svc
        .status
        .as_ref()
        .and_then(|s| s.load_balancer.as_ref())
        .and_then(|lb| lb.ingress.as_ref())
    else {
        return Vec::new();
    };
    let ips = entries
        .iter()
        .filter_map(|entry| entry.ip.clone().filter(|ip| !ip.is_empty()));
    unique_ipv4(ips, "Service status.loadBalancer")
}

/// Parses a `namespace/name` string into a [`NamespacedName`].
///
/// Whitespace around each segment is trimmed. Returns `None` unless the input
/// has exactly two non-empty segments separated by a single `/`.
#[must_use]
pub(crate) fn service_ref_from_str(s: &str) -> Option<NamespacedName> {
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
pub(crate) fn gateway_service_target_from_str(s: &str) -> Option<GatewayServiceTarget> {
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
pub(crate) fn parse_gateway_service_entry(entry: &str) -> Option<(String, GatewayServiceTarget)> {
    let (class, target) = entry.trim().split_once('=')?;
    let class = class.trim();
    if class.is_empty() {
        return None;
    }
    let target = gateway_service_target_from_str(target)?;
    Some((class.to_string(), target))
}

/// Parses the operator's `gatewayClass -> LoadBalancer Service` map from a
/// single comma-separated string (the `BINDY_SCOUT_GATEWAY_SERVICES` env form).
///
/// Each entry is `class=<target>`, where `<target>` is either `namespace/name`
/// or `namespace/<label-selector>`, e.g.
/// `traefik=traefik/traefik,cilium=kube-system/app.kubernetes.io/name=cilium`.
/// Empty entries (a trailing comma) are skipped. Because commas separate
/// entries here, multi-label selectors (which use commas) must be supplied via
/// the repeatable `--gateway-service` CLI flag instead. The map's keys double
/// as the allow-list of gateway classes Scout will follow.
///
/// # Errors
/// Returns an error naming the first malformed entry: a misconfigured map
/// fails Scout at startup instead of silently dropping a gateway class
/// (ADR-0022, Decision 6).
pub(crate) fn parse_gateway_services(raw: &str) -> Result<BTreeMap<String, GatewayServiceTarget>> {
    parse_gateway_service_entries(raw.split(',').filter(|e| !e.trim().is_empty()))
}

/// Parses each `class=<target>` entry, failing on the first malformed one.
///
/// # Errors
/// Returns an error naming the malformed entry.
pub(crate) fn parse_gateway_service_entries<'a>(
    entries: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeMap<String, GatewayServiceTarget>> {
    entries
        .into_iter()
        .map(|entry| {
            parse_gateway_service_entry(entry).ok_or_else(|| {
                anyhow!(
                    "malformed gateway-service entry {entry:?}: expected \
                     class=namespace/name or class=namespace/<label-selector>"
                )
            })
        })
        .collect()
}

/// Extracts the IPv4 addresses from a Gateway's `status.addresses`.
///
/// An entry is a candidate when its `type` is `IPAddress`, or when the `type`
/// is absent but the `value` parses as an [`IpAddr`](std::net::IpAddr).
/// `Hostname`-typed entries are ignored, and candidates that are not IPv4 are
/// skipped with a warning by [`retain_ipv4`] (`ARecord` is IPv4-only).
/// Returns an empty vec when the Gateway has no addresses (the common case
/// that forces the LoadBalancer-Service hop).
#[must_use]
pub(crate) fn gateway_addresses_as_ips(gw: &Gateway) -> Vec<String> {
    let Some(addresses) = gw.status.as_ref().and_then(|s| s.addresses.as_ref()) else {
        return Vec::new();
    };
    let candidates = addresses
        .iter()
        .filter(|addr| match addr.r#type.as_deref() {
            Some("IPAddress") => true,
            Some("Hostname") => false,
            _ => addr.value.parse::<std::net::IpAddr>().is_ok(),
        })
        .map(|addr| addr.value.clone())
        .filter(|v| !v.is_empty());
    retain_ipv4(candidates, "Gateway status.addresses")
}

/// Resolves a route's `parentRefs` to the Gateways they point at.
///
/// Only references to Gateway API Gateways are returned — an entry is kept when
/// its `group` is absent or `gateway.networking.k8s.io` AND its `kind` is absent
/// or `Gateway`. Each ref's namespace defaults to `route_namespace` when omitted.
#[must_use]
pub(crate) fn gateway_parent_refs(
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

// ============================================================================
// Remote client builder (Phase 2)
// ============================================================================

/// How Scout reaches the cluster that holds `ARecord`s and `DNSZone`s (ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoteTransport {
    /// No remote configuration: the local cluster is the bindy cluster.
    SameCluster,
    /// Phase 2: kubeconfig blob read from a local Secret.
    KubeconfigSecret {
        /// Secret name (`BINDY_SCOUT_REMOTE_SECRET`).
        name: String,
        /// Secret namespace (`BINDY_SCOUT_REMOTE_SECRET_NAMESPACE`).
        namespace: String,
    },
    /// Phase 3 (ADR-0008): explicit endpoint + file-based credentials.
    Endpoint {
        /// Bindy cluster API URL (Linkerd-mirrored proxy, konnectivity, or direct).
        endpoint: String,
        /// Bearer-token file minted by the bindy cluster; re-read on rotation.
        token_file: String,
        /// Endpoint CA bundle (PEM). `None` means webpki public roots.
        ca_file: Option<String>,
    },
}

/// Resolve which remote transport the configuration selects, failing closed
/// on ambiguous or incomplete combinations (ADR-0008).
///
/// # Arguments
/// * `endpoint` - `BINDY_SCOUT_REMOTE_ENDPOINT`, if set
/// * `token_file` - `BINDY_SCOUT_REMOTE_TOKEN_FILE`, if set
/// * `ca_file` - `BINDY_SCOUT_REMOTE_CA_FILE`, if set
/// * `secret_name` - `BINDY_SCOUT_REMOTE_SECRET`, if set
/// * `secret_namespace` - Effective remote-Secret namespace
///
/// # Errors
/// Returns an error when both remote modes are configured at once, when the
/// endpoint mode is missing its token file, or when credential files are
/// given without an endpoint.
pub(crate) fn resolve_remote_transport(
    endpoint: Option<String>,
    token_file: Option<String>,
    ca_file: Option<String>,
    secret_name: Option<String>,
    secret_namespace: &str,
) -> Result<RemoteTransport> {
    if let Some(endpoint) = endpoint {
        if secret_name.is_some() {
            return Err(anyhow!(
                "BINDY_SCOUT_REMOTE_ENDPOINT and BINDY_SCOUT_REMOTE_SECRET are both set; \
                 the two remote modes are mutually exclusive: configure exactly one"
            ));
        }
        let Some(token_file) = token_file else {
            return Err(anyhow!(
                "BINDY_SCOUT_REMOTE_ENDPOINT is set but BINDY_SCOUT_REMOTE_TOKEN_FILE is not; \
                 the endpoint mode requires a bearer-token file minted by the bindy cluster"
            ));
        };
        return Ok(RemoteTransport::Endpoint {
            endpoint,
            token_file,
            ca_file,
        });
    }

    if token_file.is_some() || ca_file.is_some() {
        return Err(anyhow!(
            "BINDY_SCOUT_REMOTE_TOKEN_FILE / BINDY_SCOUT_REMOTE_CA_FILE are set without \
             BINDY_SCOUT_REMOTE_ENDPOINT; credential files only apply to the endpoint mode"
        ));
    }

    if let Some(name) = secret_name {
        return Ok(RemoteTransport::KubeconfigSecret {
            name,
            namespace: secret_namespace.to_string(),
        });
    }

    Ok(RemoteTransport::SameCluster)
}

/// Kubeconfig entry names used by the synthesized endpoint-mode config.
const ENDPOINT_KUBECONFIG_NAME: &str = "bindy-remote";

/// Synthesize an in-memory kubeconfig for the endpoint transport (ADR-0008).
///
/// Feeding this through `Config::from_custom_kubeconfig` reuses kube-rs's
/// PEM handling and token-file refresh instead of hand-rolling either.
///
/// # Arguments
/// * `endpoint` - Bindy cluster API URL
/// * `token_file` - Path to the bearer-token file
/// * `ca_file` - Path to the endpoint CA bundle, if pinned
#[must_use]
pub(crate) fn build_endpoint_kubeconfig(
    endpoint: &str,
    token_file: &str,
    ca_file: Option<&str>,
) -> Kubeconfig {
    use kube::config::{
        AuthInfo, Cluster, Context as KubeContext, NamedAuthInfo, NamedCluster, NamedContext,
    };

    Kubeconfig {
        clusters: vec![NamedCluster {
            name: ENDPOINT_KUBECONFIG_NAME.to_string(),
            cluster: Some(Cluster {
                server: Some(endpoint.to_string()),
                certificate_authority: ca_file.map(str::to_string),
                ..Default::default()
            }),
            other: Default::default(),
        }],
        auth_infos: vec![NamedAuthInfo {
            name: ENDPOINT_KUBECONFIG_NAME.to_string(),
            auth_info: Some(AuthInfo {
                token_file: Some(token_file.to_string()),
                ..Default::default()
            }),
            other: Default::default(),
        }],
        contexts: vec![NamedContext {
            name: ENDPOINT_KUBECONFIG_NAME.to_string(),
            context: Some(KubeContext {
                cluster: ENDPOINT_KUBECONFIG_NAME.to_string(),
                user: Some(ENDPOINT_KUBECONFIG_NAME.to_string()),
                ..Default::default()
            }),
            other: Default::default(),
        }],
        current_context: Some(ENDPOINT_KUBECONFIG_NAME.to_string()),
        ..Default::default()
    }
}

/// Build the remote [`Client`] for the endpoint transport (ADR-0008).
///
/// # Arguments
/// * `endpoint` - Bindy cluster API URL
/// * `token_file` - Path to the bearer-token file
/// * `ca_file` - Path to the endpoint CA bundle, if pinned
///
/// # Errors
/// Returns an error if the synthesized configuration cannot be turned into a
/// client (e.g., an unreadable CA file — fails closed at startup).
pub(crate) async fn build_remote_client_from_endpoint(
    endpoint: &str,
    token_file: &str,
    ca_file: Option<&str>,
) -> Result<Client> {
    let kubeconfig = build_endpoint_kubeconfig(endpoint, token_file, ca_file);
    let config = kube::Config::from_custom_kubeconfig(kubeconfig, &KubeConfigOptions::default())
        .await
        .map_err(|e| {
            anyhow!("Failed to build client config for remote endpoint {endpoint}: {e}")
        })?;
    Client::try_from(config)
        .map_err(|e| anyhow!("Failed to create remote Kubernetes client for {endpoint}: {e}"))
}

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
    use bindy_controller_sdk::audit::{Actor, AuditReason, SecretApi, SecretPurpose, SecretUse};
    let api = SecretApi::namespaced(local_client.clone(), secret_namespace, Actor::Scout);
    let secret_use = SecretUse::new(SecretPurpose::ScoutKubeconfig, AuditReason::ClientBuild);
    let secret = api.get(secret_name, &secret_use).await.map_err(|e| {
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

/// Scout's startup configuration, read from CLI flags and environment
/// variables (a flag takes precedence over its variable).
///
/// Every value is checked once, here, so a misconfigured Scout fails at
/// startup instead of running degraded (ADR-0022, Decision 6).
#[derive(Debug)]
pub(crate) struct ScoutConfig {
    pub(crate) target_namespace: String,
    pub(crate) cluster_name: String,
    pub(crate) excluded_namespaces: Vec<String>,
    /// Default IPs used when no per-resource annotation override or discovered IP
    /// applies. Set via `BINDY_SCOUT_DEFAULT_IPS` (comma-separated) or `--default-ips`.
    pub(crate) default_ips: Vec<String>,
    /// `gatewayClass -> LoadBalancer Service` map for gateway-chain IP resolution.
    /// Set via `BINDY_SCOUT_GATEWAY_SERVICES` (`class=ns/name,...`) or `--gateway-service`.
    pub(crate) gateway_services: BTreeMap<String, GatewayServiceTarget>,
    /// Default DNS zone applied when no `bindy.firestoned.io/zone` annotation
    /// is present. Set via `BINDY_SCOUT_DEFAULT_ZONE` or `--default-zone`.
    pub(crate) default_zone: Option<String>,
    /// Kubernetes label selector restricting which namespaces Scout will act in. `None` means
    /// every namespace is eligible (backward-compatible default). Set via
    /// `BINDY_SCOUT_NAMESPACE_SELECTOR` or `--namespace-selector`.
    pub(crate) namespace_selector: Option<String>,
    /// Zones Scout may publish into (ADR-0025); `None` means every zone.
    /// Set via `BINDY_SCOUT_ALLOWED_ZONES` or `--allowed-zones`.
    pub(crate) allowed_zones: Option<Vec<String>>,
    /// IPv4 ranges published addresses must fall in (ADR-0025); `None` means
    /// every address. Set via `BINDY_SCOUT_ALLOWED_IP_RANGES` or
    /// `--allowed-ip-ranges`.
    pub(crate) allowed_ip_ranges: Option<Vec<Ipv4Range>>,
    /// How to reach the bindy cluster: same-cluster, kubeconfig Secret
    /// (Phase 2) or endpoint + token file (Phase 3, ADR-0008).
    pub(crate) remote_transport: RemoteTransport,
}

/// CLI overrides for the remote transport (ADR-0008); each takes precedence
/// over its `BINDY_SCOUT_REMOTE_*` environment variable when set.
#[derive(Debug, Default, Clone)]
pub struct ScoutRemoteOverrides {
    /// Overrides `BINDY_SCOUT_REMOTE_ENDPOINT`.
    pub endpoint: Option<String>,
    /// Overrides `BINDY_SCOUT_REMOTE_TOKEN_FILE`.
    pub token_file: Option<String>,
    /// Overrides `BINDY_SCOUT_REMOTE_CA_FILE`.
    pub ca_file: Option<String>,
}

/// The CLI flags `bindy scout` passes to [`run_scout`]. Each one, when set,
/// takes precedence over its `BINDY_SCOUT_*` environment variable.
#[derive(Debug, Default, Clone)]
pub struct ScoutCli {
    /// Overrides `BINDY_SCOUT_CLUSTER_NAME`.
    pub cluster_name: Option<String>,
    /// Overrides `BINDY_SCOUT_NAMESPACE`.
    pub namespace: Option<String>,
    /// Overrides `BINDY_SCOUT_DEFAULT_IPS`.
    pub default_ips: Vec<String>,
    /// Overrides `BINDY_SCOUT_GATEWAY_SERVICES`, one `class=ns/name` each.
    pub gateway_services: Vec<String>,
    /// Overrides `BINDY_SCOUT_DEFAULT_ZONE`.
    pub default_zone: Option<String>,
    /// Overrides `BINDY_SCOUT_NAMESPACE_SELECTOR`.
    pub namespace_selector: Option<String>,
    /// Overrides `BINDY_SCOUT_ALLOWED_ZONES` (comma-separated).
    pub allowed_zones: Option<String>,
    /// Overrides `BINDY_SCOUT_ALLOWED_IP_RANGES` (comma-separated).
    pub allowed_ip_ranges: Option<String>,
    /// Overrides for the endpoint remote transport (ADR-0008).
    pub remote: ScoutRemoteOverrides,
}

/// Namespace assumed for Scout itself when `POD_NAMESPACE` is unset.
const FALLBACK_POD_NAMESPACE: &str = "default";

/// Returns the value a setting takes: the CLI flag when given, otherwise the
/// environment variable `var` read through `lookup`, otherwise `None`.
///
/// This is the one place Scout reads a setting, so every flag and variable
/// gets the same rule.
///
/// # Errors
/// Returns an error when the flag or the variable is set to an empty or
/// all-whitespace value: an empty value is a mistake (a templated value that
/// rendered to nothing, say), and silently treating it as unset would run
/// Scout with a configuration nobody asked for (ADR-0022, Decision 6).
pub(crate) fn env_or(
    cli: Option<String>,
    var: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<String>> {
    let Some(value) = cli.or_else(|| lookup(var)) else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Err(anyhow!(
            "{var} (or its CLI flag) is set but empty; unset it or give it a value"
        ));
    }
    Ok(Some(value))
}

/// Splits a comma-separated setting into its trimmed, non-empty entries.
fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Checks that every default IP is an IPv4 address (`ARecord` is IPv4-only).
///
/// # Errors
/// Returns an error naming the first entry that is not IPv4.
fn validate_default_ips(ips: Vec<String>) -> Result<Vec<String>> {
    if let Some(bad) = ips
        .iter()
        .find(|ip| ip.trim().parse::<std::net::Ipv4Addr>().is_err())
    {
        return Err(anyhow!(
            "default IP {bad:?} (BINDY_SCOUT_DEFAULT_IPS / --default-ips) is not an IPv4 address"
        ));
    }
    Ok(ips.into_iter().map(|ip| ip.trim().to_string()).collect())
}

impl ScoutConfig {
    /// Builds the configuration from CLI flags and the process environment.
    ///
    /// # Errors
    /// See [`ScoutConfig::from_lookup`].
    fn from_env(cli: ScoutCli) -> Result<Self> {
        Self::from_lookup(cli, &|var| std::env::var(var).ok())
    }

    /// Builds the configuration from CLI flags and environment variables read
    /// through `lookup` (a seam for tests). A flag takes precedence over its
    /// variable.
    ///
    /// # Errors
    /// Returns an error when `BINDY_SCOUT_CLUSTER_NAME` is missing, when any
    /// setting is present but empty (see [`env_or`]), when a default IP is not
    /// IPv4, when a gateway-service entry is malformed, or when the remote
    /// transport is ambiguous or incomplete (see `resolve_remote_transport`).
    pub(crate) fn from_lookup(
        cli: ScoutCli,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let target_namespace = env_or(cli.namespace, "BINDY_SCOUT_NAMESPACE", lookup)?
            .unwrap_or_else(|| DEFAULT_SCOUT_NAMESPACE.to_string());

        let cluster_name = env_or(cli.cluster_name, "BINDY_SCOUT_CLUSTER_NAME", lookup)?
            .ok_or_else(|| {
                anyhow!("BINDY_SCOUT_CLUSTER_NAME is required (set via --cluster-name or env var)")
            })?;

        let own_namespace = env_or(None, "POD_NAMESPACE", lookup)?
            .unwrap_or_else(|| FALLBACK_POD_NAMESPACE.to_string());

        let mut excluded_namespaces = env_or(None, "BINDY_SCOUT_EXCLUDE_NAMESPACES", lookup)?
            .map(|raw| split_list(&raw))
            .unwrap_or_default();
        // Always exclude Scout's own namespace
        if !excluded_namespaces.contains(&own_namespace) {
            excluded_namespaces.push(own_namespace.clone());
        }

        // CLI --default-ips takes precedence over BINDY_SCOUT_DEFAULT_IPS.
        let default_ips = if cli.default_ips.is_empty() {
            env_or(None, "BINDY_SCOUT_DEFAULT_IPS", lookup)?
                .map(|raw| split_list(&raw))
                .unwrap_or_default()
        } else {
            cli.default_ips
        };
        let default_ips = validate_default_ips(default_ips)?;

        // CLI --gateway-service takes precedence over BINDY_SCOUT_GATEWAY_SERVICES.
        // CLI entries are parsed one by one (each flag is a single `class=target`)
        // so a multi-label selector's commas are not mistaken for entry
        // separators; the env form is a single comma-separated string.
        let gateway_services = if cli.gateway_services.is_empty() {
            env_or(None, "BINDY_SCOUT_GATEWAY_SERVICES", lookup)?
                .map(|raw| parse_gateway_services(&raw))
                .transpose()?
                .unwrap_or_default()
        } else {
            parse_gateway_service_entries(cli.gateway_services.iter().map(String::as_str))?
        };

        let default_zone = env_or(cli.default_zone, "BINDY_SCOUT_DEFAULT_ZONE", lookup)?;

        // Unset (None) means every namespace is eligible: the backward-compatible default.
        let namespace_selector = env_or(
            cli.namespace_selector,
            "BINDY_SCOUT_NAMESPACE_SELECTOR",
            lookup,
        )?;

        let allowed_zones = env_or(cli.allowed_zones, "BINDY_SCOUT_ALLOWED_ZONES", lookup)?
            .map(|raw| parse_allowed_zones(&raw))
            .transpose()?;
        if let (Some(allowed), Some(zone)) = (&allowed_zones, &default_zone) {
            if !zone_is_allowed(allowed, zone) {
                return Err(anyhow!(
                    "default zone {zone:?} (BINDY_SCOUT_DEFAULT_ZONE / --default-zone) is not in \
                     BINDY_SCOUT_ALLOWED_ZONES / --allowed-zones"
                ));
            }
        }

        let allowed_ip_ranges = env_or(
            cli.allowed_ip_ranges,
            "BINDY_SCOUT_ALLOWED_IP_RANGES",
            lookup,
        )?
        .map(|raw| parse_allowed_ip_ranges(&raw))
        .transpose()?;
        if let Some(outside) = allowed_ip_ranges
            .as_deref()
            .and_then(|ranges| first_ip_outside(ranges, &default_ips))
        {
            return Err(anyhow!(
                "default IP {outside:?} (BINDY_SCOUT_DEFAULT_IPS / --default-ips) is outside \
                 BINDY_SCOUT_ALLOWED_IP_RANGES / --allowed-ip-ranges"
            ));
        }

        let remote_secret_name = env_or(None, "BINDY_SCOUT_REMOTE_SECRET", lookup)?;
        let remote_secret_namespace = env_or(None, "BINDY_SCOUT_REMOTE_SECRET_NAMESPACE", lookup)?
            .unwrap_or_else(|| own_namespace.clone());

        let remote_transport = resolve_remote_transport(
            env_or(cli.remote.endpoint, "BINDY_SCOUT_REMOTE_ENDPOINT", lookup)?,
            env_or(
                cli.remote.token_file,
                "BINDY_SCOUT_REMOTE_TOKEN_FILE",
                lookup,
            )?,
            env_or(cli.remote.ca_file, "BINDY_SCOUT_REMOTE_CA_FILE", lookup)?,
            remote_secret_name,
            &remote_secret_namespace,
        )?;

        Ok(Self {
            target_namespace,
            cluster_name,
            excluded_namespaces,
            default_ips,
            gateway_services,
            default_zone,
            namespace_selector,
            allowed_zones,
            allowed_ip_ranges,
            remote_transport,
        })
    }
}

// ============================================================================
// Entry point
// ============================================================================

/// Entry point for the `bindy scout` subcommand.
///
/// Validates the configuration, initialises the Kubernetes clients, builds the
/// `DNSZone` store (for zone validation), then runs one controller per served
/// source kind, each driving the shared pipeline (ADR-0022). Returns once
/// every controller has drained after `shutdown` fires (SIGTERM or SIGINT,
/// wired by the `bindy scout` command).
///
/// # Errors
///
/// Returns an error if the configuration is invalid (see
/// `ScoutConfig::from_lookup`: a missing cluster name, an empty setting, a
/// malformed `--gateway-service` entry, a non-IPv4 default IP, a malformed
/// allowlist or a default zone or IP outside it, an ambiguous remote
/// transport) or if a Kubernetes client cannot be initialised.
pub async fn run_scout(cli: ScoutCli, shutdown: ShutdownSignal) -> Result<()> {
    let config = ScoutConfig::from_env(cli)?;

    let local_client = Client::try_default().await?;

    let remote_client = match &config.remote_transport {
        RemoteTransport::KubeconfigSecret { name, namespace } => {
            info!(
                cluster = %config.cluster_name,
                target_ns = %config.target_namespace,
                secret = %name,
                secret_ns = %namespace,
                excluded = ?config.excluded_namespaces,
                default_ips = ?config.default_ips,
                default_zone = ?config.default_zone,
                namespace_selector = ?config.namespace_selector,
                "Starting bindy scout in remote cluster mode (kubeconfig Secret)"
            );
            build_remote_client(&local_client, name, namespace).await?
        }
        RemoteTransport::Endpoint {
            endpoint,
            token_file,
            ca_file,
        } => {
            info!(
                cluster = %config.cluster_name,
                target_ns = %config.target_namespace,
                endpoint = %endpoint,
                token_file = %token_file,
                ca_file = ?ca_file,
                excluded = ?config.excluded_namespaces,
                default_ips = ?config.default_ips,
                default_zone = ?config.default_zone,
                namespace_selector = ?config.namespace_selector,
                "Starting bindy scout in remote cluster mode (endpoint override, ADR-0008)"
            );
            build_remote_client_from_endpoint(endpoint, token_file, ca_file.as_deref()).await?
        }
        RemoteTransport::SameCluster => {
            info!(
                cluster = %config.cluster_name,
                target_ns = %config.target_namespace,
                excluded = ?config.excluded_namespaces,
                default_ips = ?config.default_ips,
                default_zone = ?config.default_zone,
                namespace_selector = ?config.namespace_selector,
                "Starting bindy scout in same-cluster mode"
            );
            local_client.clone()
        }
    };

    if config.namespace_selector.is_none() {
        warn!(
            "No --namespace-selector / BINDY_SCOUT_NAMESPACE_SELECTOR configured — scout will \
             act in EVERY namespace in the cluster (subject only to each source object's own \
             opt-in annotation and --exclude-namespaces). Setting a namespace-selector so scout \
             only considers explicitly-whitelisted namespaces is strongly recommended for \
             production deployments; running without one is not recommended."
        );
    }
    if let Some(zones) = &config.allowed_zones {
        info!(allowed_zones = ?zones, "Publishing only into the allowed zones and their subzones");
    } else {
        warn!(
            "No --allowed-zones / BINDY_SCOUT_ALLOWED_ZONES configured: any tenant who can \
             annotate a source may name any zone Scout's DNSZone grants allow. Set it to the \
             zones this cluster owns (ADR-0025)."
        );
    }
    if config.allowed_ip_ranges.is_none() {
        warn!(
            "No --allowed-ip-ranges / BINDY_SCOUT_ALLOWED_IP_RANGES configured: the \
             bindy.firestoned.io/ip annotation may point a name at any address. Set it to this \
             cluster's load-balancer ranges (ADR-0025)."
        );
    }

    // The bindy cluster's DNSZones, read through the shared watch layer
    // (ADR-0009 §3) over the REMOTE client: in same-cluster mode that is the
    // local cluster, in remote mode the bindy cluster. Scoped to the target
    // namespace: DNSZones and ARecords always live there on the bindy cluster,
    // so a namespaced watch suffices and needs no cluster-scoped ClusterRole.
    // The WatchSet restarts the watch with backoff and logs a diagnosis of
    // each watch error.
    let mut remote_watch = WatchSet::new(
        remote_client.clone(),
        NamespaceScope::Namespaces(vec![config.target_namespace.clone()]),
    );
    let _zones = remote_watch.register::<DNSZone>("DNSZone");
    let dnszone_reader = remote_watch.store::<DNSZone>(Some(&config.target_namespace));

    let ctx = Arc::new(ScoutContext {
        client: local_client.clone(),
        remote_client,
        target_namespace: config.target_namespace,
        cluster_name: config.cluster_name,
        excluded_namespaces: config.excluded_namespaces,
        default_ips: config.default_ips,
        gateway_services: config.gateway_services,
        default_zone: config.default_zone,
        namespace_selector: config.namespace_selector,
        allowed_zones: config.allowed_zones,
        allowed_ip_ranges: config.allowed_ip_ranges,
        zone_store: dnszone_reader,
    });

    // The route APIs below only probe whether each kind is served; the
    // source objects themselves are watched through the shared watch layer.
    let httproute_api: Api<HTTPRoute> = Api::all(local_client.clone());
    // Watch GRPCRoutes across all namespaces using the LOCAL client
    let grpcroute_api: Api<GRPCRoute> = Api::all(local_client.clone());
    // Watch TLSRoutes across all namespaces using the LOCAL client
    let tlsroute_api: Api<TLSRoute> = Api::all(local_client.clone());
    // Watch TCPRoutes across all namespaces using the LOCAL client
    let tcproute_api: Api<TCPRoute> = Api::all(local_client.clone());

    // Probe EACH route kind, not one as a proxy for the others. Gateway API
    // ships in two channels and the kinds graduated at different times:
    // HTTPRoute Standard since v1.0, GRPCRoute since v1.1, TLSRoute since
    // v1.5, TCPRoute since v1.6. A standard-channel install older than those
    // serves HTTPRoute and not the rest. Inferring from one kind would leave
    // the other controllers retrying a 404 forever, which is exactly what
    // this check prevents.
    //
    // Ingress and Service are core API and always served, so they are not
    // probed: a check that can never fail is noise.
    let httproute_enabled = kind_served(&httproute_api).await;
    let grpcroute_enabled = kind_served(&grpcroute_api).await;
    let tlsroute_enabled = kind_served(&tlsroute_api).await;
    let tcproute_enabled = kind_served(&tcproute_api).await;

    let mut watching = vec!["Ingresses", "Services"];
    if httproute_enabled {
        watching.push("HTTPRoutes");
    }
    if tlsroute_enabled {
        watching.push("TLSRoutes");
    }
    if grpcroute_enabled {
        watching.push("GRPCRoutes");
    }
    if tcproute_enabled {
        watching.push("TCPRoutes");
    }
    info!(
        "Scout controller running — watching {}",
        watching.join(", ")
    );

    let mut disabled = vec![];
    if !httproute_enabled {
        disabled.push("HTTPRoute");
    }
    if !tlsroute_enabled {
        disabled.push("TLSRoute");
    }
    if !grpcroute_enabled {
        disabled.push("GRPCRoute");
    }
    if !tcproute_enabled {
        disabled.push("TCPRoute");
    }
    if !disabled.is_empty() {
        info!(
            "Gateway API CRDs not found for {}; watching disabled for those kinds",
            disabled.join("/")
        );
    }

    // Ingresses, Services and the served route kinds across all namespaces,
    // through the shared watch layer over the LOCAL client: one watch and
    // cache per kind, restarted with backoff (ADR-0009 §3). A route kind whose
    // CRD is absent is never registered, so nothing retries a 404.
    let mut local_watch = WatchSet::new(local_client.clone(), NamespaceScope::All);
    let _ingresses = local_watch.register::<Ingress>("Ingress");
    let _services = local_watch.register::<Service>("Service");
    if httproute_enabled {
        let _ = local_watch.register::<HTTPRoute>("HTTPRoute");
    }
    if grpcroute_enabled {
        let _ = local_watch.register::<GRPCRoute>("GRPCRoute");
    }
    if tlsroute_enabled {
        let _ = local_watch.register::<TLSRoute>("TLSRoute");
    }
    if tcproute_enabled {
        let _ = local_watch.register::<TCPRoute>("TCPRoute");
    }
    let local_watch = Arc::new(local_watch);

    // A Gateway API controller is built only when its kind is served (and so
    // registered above): subscribing to an unregistered kind would panic.
    let mut controllers = vec![
        source_controller::<Ingress>(&local_watch, ctx.clone(), shutdown.clone()),
        source_controller::<Service>(&local_watch, ctx.clone(), shutdown.clone()),
    ];
    if httproute_enabled {
        controllers.push(source_controller::<HTTPRoute>(
            &local_watch,
            ctx.clone(),
            shutdown.clone(),
        ));
    }
    if grpcroute_enabled {
        controllers.push(source_controller::<GRPCRoute>(
            &local_watch,
            ctx.clone(),
            shutdown.clone(),
        ));
    }
    if tlsroute_enabled {
        controllers.push(source_controller::<TLSRoute>(
            &local_watch,
            ctx.clone(),
            shutdown.clone(),
        ));
    }
    if tcproute_enabled {
        controllers.push(source_controller::<TCPRoute>(
            &local_watch,
            ctx.clone(),
            shutdown.clone(),
        ));
    }

    futures::future::join_all(controllers).await;

    Ok(())
}

/// Returns whether the cluster serves a given resource kind.
///
/// Gateway API is not installed by default in Kubernetes, and — critically —
/// its route kinds do NOT arrive together. Gateway API ships in two channels:
/// `HTTPRoute` has been Standard since v1.0, `GRPCRoute` became Standard in
/// v1.1, `TLSRoute` only became Standard in v1.5 and `TCPRoute` in v1.6. A
/// standard-channel install older than those has `HTTPRoute` and neither of the
/// others, so probing one kind and inferring the rest re-creates the very
/// error loop this check exists to prevent.
///
/// A [`Controller`] started against a kind whose CRD is absent does not fail
/// loudly: it retries forever, logging an error per attempt for an API that
/// will never appear. Probing once at startup turns that into a single
/// informational line.
///
/// # Arguments
/// * `api` - Cluster-wide handle for the kind to probe.
///
/// # Returns
/// `true` when the kind is served, `false` only when the API server answers
/// `404`. Any other error is reported as `true`, so a transient problem during
/// startup cannot silently disable watching for the lifetime of the process —
/// loud-but-working is the safer failure here.
///
/// The probe runs ONCE, at startup. Installing the CRDs later requires a Scout
/// restart before the corresponding controller begins watching.
pub(crate) async fn kind_served<R>(api: &Api<R>) -> bool
where
    R: Clone + DeserializeOwned + Debug + k8s_openapi::Resource,
{
    match api.list(&ListParams::default().limit(1)).await {
        Ok(_) => true,
        Err(err) if kube_errors::is_not_found(&err) => false,
        Err(err) => {
            warn!(
                kind = R::KIND,
                error = %err,
                "could not determine whether this kind is served; assuming it is"
            );
            true
        }
    }
}
