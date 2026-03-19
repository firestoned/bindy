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
//! ## Phase 1 — Same-cluster mode
//!
//! In Phase 1, Scout uses a single Kubernetes client and creates ARecords in the same
//! cluster. Phase 2 will add a remote client that talks to a dedicated bindy cluster.

use crate::crd::{ARecord, ARecordSpec, DNSZone};
use anyhow::{anyhow, Result};

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
    Api, Client, ResourceExt,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tracing::{debug, error, info, warn};

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

/// Label identifying the source Ingress name on created ARecords
pub const LABEL_SOURCE_INGRESS: &str = "bindy.firestoned.io/source-ingress";

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

// ============================================================================
// Context
// ============================================================================

/// Shared context passed to every reconciler invocation.
pub struct ScoutContext {
    /// Kubernetes client (same-cluster in Phase 1, remote in Phase 2)
    pub client: Client,
    /// Namespace where ARecords are created
    pub target_namespace: String,
    /// Logical cluster name stamped on created ARecord labels
    pub cluster_name: String,
    /// Namespaces excluded from Ingress watching (always includes Scout's own namespace)
    pub excluded_namespaces: Vec<String>,
    /// Read-only store of DNSZone resources for zone validation
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
    /// IPv4 address to use for the record
    pub ip: &'a str,
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
        LABEL_SOURCE_INGRESS.to_string(),
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
            ipv4_addresses: vec![params.ip.to_string()],
            ttl: params.ttl,
        },
        status: None,
    }
}

// ============================================================================
// Reconciler
// ============================================================================

/// Reconciles a single Ingress, creating or updating ARecord CRs as needed.
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

    let annotations = ingress
        .metadata
        .annotations
        .as_ref()
        .cloned()
        .unwrap_or_default();

    // Guard: opt-in annotation required
    if !is_arecord_enabled(&annotations) {
        debug!(ingress = %name, ns = %namespace, "No arecord annotation — skipping");
        return Ok(Action::await_change());
    }

    // Guard: zone annotation required
    let zone = match get_zone_annotation(&annotations) {
        Some(z) => z,
        None => {
            warn!(ingress = %name, ns = %namespace, "Missing bindy.firestoned.io/zone annotation — skipping");
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

    // Resolve IP: annotation override first, then LB status
    let ip = if let Some(override_ip) = resolve_ip_from_annotation(&annotations) {
        override_ip
    } else if let Some(lb_ip) = resolve_ip_from_lb_status(&ingress) {
        lb_ip
    } else {
        warn!(ingress = %name, ns = %namespace, "No IP available (no annotation override, no LB status IP) — requeuing");
        return Ok(Action::requeue(Duration::from_secs(
            SCOUT_ERROR_REQUEUE_SECS,
        )));
    };

    // Optional TTL override
    let ttl: Option<i32> = annotations.get(ANNOTATION_TTL).and_then(|v| v.parse().ok());

    let spec_rules = ingress
        .spec
        .as_ref()
        .and_then(|s| s.rules.as_ref())
        .cloned()
        .unwrap_or_default();

    let arecord_api: Api<ARecord> = Api::namespaced(ctx.client.clone(), &ctx.target_namespace);

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
            ip: &ip,
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
                info!(arecord = %cr_name, ingress = %name, host = %host, ip = %ip, "ARecord created/updated");
            }
            Err(e) => {
                error!(arecord = %cr_name, ingress = %name, error = %e, "Failed to apply ARecord");
                return Err(ScoutError::from(anyhow!(
                    "Failed to apply ARecord {cr_name}: {e}"
                )));
            }
        }
    }

    Ok(Action::await_change())
}

/// Error policy: requeue with a fixed backoff on any reconcile error.
fn error_policy(_obj: Arc<Ingress>, error: &ScoutError, _ctx: Arc<ScoutContext>) -> Action {
    error!(error = %error, "Scout reconcile error — requeuing");
    Action::requeue(Duration::from_secs(SCOUT_ERROR_REQUEUE_SECS))
}

// ============================================================================
// Entry point
// ============================================================================

/// Reads scout configuration from environment variables.
struct ScoutConfig {
    target_namespace: String,
    cluster_name: String,
    excluded_namespaces: Vec<String>,
}

impl ScoutConfig {
    /// Build configuration from environment variables, with optional CLI overrides.
    ///
    /// CLI arguments take precedence over environment variables when provided.
    fn from_env(cli_cluster_name: Option<String>, cli_namespace: Option<String>) -> Result<Self> {
        let target_namespace = cli_namespace
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("BINDY_SCOUT_NAMESPACE").ok())
            .unwrap_or_else(|| DEFAULT_SCOUT_NAMESPACE.to_string());

        let cluster_name = cli_cluster_name
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("BINDY_SCOUT_CLUSTER_NAME").ok())
            .ok_or_else(|| {
                anyhow!(
                    "BINDY_SCOUT_CLUSTER_NAME is required (set via --bind9-cluster-name or env var)"
                )
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
            excluded_namespaces.push(own_namespace);
        }

        Ok(Self {
            target_namespace,
            cluster_name,
            excluded_namespaces,
        })
    }
}

/// Entry point for the `bindy scout` subcommand.
///
/// Initialises the Kubernetes client, builds reflector stores for `DNSZone`
/// resources (for zone validation), then runs the Ingress controller loop.
///
/// # Arguments
///
/// * `cli_cluster_name` — Optional cluster name from `--bind9-cluster-name` CLI arg.
///   Takes precedence over the `BINDY_SCOUT_CLUSTER_NAME` environment variable.
/// * `cli_namespace` — Optional namespace from `--namespace` CLI arg.
///   Takes precedence over the `BINDY_SCOUT_NAMESPACE` environment variable.
///
/// # Errors
///
/// Returns an error if the Kubernetes client cannot be initialised or if the
/// cluster name is not provided via CLI or the `BINDY_SCOUT_CLUSTER_NAME` env var.
pub async fn run_scout(
    cli_cluster_name: Option<String>,
    cli_namespace: Option<String>,
) -> Result<()> {
    let config = ScoutConfig::from_env(cli_cluster_name, cli_namespace)?;

    info!(
        cluster = %config.cluster_name,
        target_ns = %config.target_namespace,
        excluded = ?config.excluded_namespaces,
        "Starting bindy scout (Phase 1 — same-cluster mode)"
    );

    let client = Client::try_default().await?;

    // Build a reflector store for DNSZone resources (zone validation cache)
    let dnszone_api: Api<DNSZone> = Api::all(client.clone());
    let (dnszone_reader, dnszone_writer) = reflector::store();
    let dnszone_reflector = reflector(
        dnszone_writer,
        watcher(dnszone_api, WatcherConfig::default()),
    );

    // Start the DNSZone reflector in the background
    tokio::spawn(async move {
        dnszone_reflector
            .for_each(|event| async move {
                match event {
                    Ok(_) => {}
                    Err(e) => error!(error = %e, "DNSZone reflector error"),
                }
            })
            .await;
    });

    let ctx = Arc::new(ScoutContext {
        client: client.clone(),
        target_namespace: config.target_namespace,
        cluster_name: config.cluster_name,
        excluded_namespaces: config.excluded_namespaces,
        zone_store: dnszone_reader,
    });

    // Watch Ingresses across all namespaces
    let ingress_api: Api<Ingress> = Api::all(client.clone());

    info!("Scout controller running — watching Ingresses");

    Controller::new(ingress_api, WatcherConfig::default())
        .run(reconcile, error_policy, ctx)
        .for_each(|res| async move {
            match res {
                Ok(obj) => debug!(obj = ?obj, "Reconciled"),
                Err(e) => error!(error = %e, "Reconcile failed"),
            }
        })
        .await;

    Ok(())
}
