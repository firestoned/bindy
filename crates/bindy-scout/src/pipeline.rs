// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Scout's one reconcile pipeline (ADR-0022).
//!
//! Every source kind Scout watches (`Ingress`, `Service`, `HTTPRoute`,
//! `TLSRoute`, `TCPRoute`) is reconciled by [`reconcile_source`]. A kind
//! supplies only what differs, through [`ScoutSource`]: its kind name, its
//! hostnames, whether it needs a `record-name` override, and where its IPs
//! come from. Every step that used to be copied per kind (excluded namespace,
//! deletion with a grace period, opt-out release, finalizer, zone resolution
//! and authorization, TTL and record-name validation, IP resolution, per-host
//! apply, pruning, legacy-record migration, stale-cluster cleanup) runs here,
//! once.

use crate::crd::ARecord;
use crate::scout::{arecord_cr_name, arecord_label_selector, stale_arecord_label_selector};
use crate::scout::{
    build_arecord, check_zone_authorization_live, cleanup_grace_expired, gateway_addresses_as_ips,
    gateway_parent_refs, get_record_name_annotation, is_being_deleted, is_loadbalancer_service,
    is_scout_opted_in, legacy_arecord_label_selector, parse_ttl_annotation, requeue_after_error,
    resolve_ip_from_lb_status, resolve_ip_from_service_lb_status, resolve_ips_from_annotation,
    resolve_record_name, resolve_usable_zone, resolve_zone, source_namespace_eligible,
    stale_cleanup_zones, stale_selector_base, tcproute_record_name, ARecordParams, Gateway,
    GatewayServiceTarget, HTTPRoute, ParentReference, ScoutContext, ScoutError, StaleKind,
    TCPRoute, TLSRoute, ZoneAuthz, ZoneResolution, EMPTY_ANNOTATIONS, FINALIZER_SCOUT,
    LABEL_SOURCE_CLUSTER, LABEL_ZONE, REMOTE_CLEANUP_GRACE_SECS, SCOUT_FIELD_MANAGER,
};
use anyhow::{anyhow, Context as _, Result};
use bindy_controller_sdk::finalizers;
use bindy_controller_sdk::kube_errors;
use bindy_controller_sdk::pagination::list_all_paginated;
use bindy_controller_sdk::status::is_ready;
use bindy_controller_sdk::watch::WatchSet;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Service;
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::jiff::Timestamp;
use k8s_openapi::NamespaceResourceScope;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::Controller;
use kube::{Api, Client, Resource, ResourceExt};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

// ============================================================================
// The source abstraction
// ============================================================================

/// Where `--default-ips` sit relative to the IPs a kind discovers itself.
///
/// An explicit `bindy.firestoned.io/ip` annotation always wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IpPrecedence {
    /// Annotation, then `--default-ips`, then the object's own status
    /// (Ingress and Service: a shared-ingress VIP overrides the LB status).
    DefaultsFirst,
    /// Annotation, then the discovered IPs, then `--default-ips` (Gateway API
    /// routes: the serving Gateway's address is more specific than a default).
    DiscoveredFirst,
}

/// What one source kind contributes to the pipeline (ADR-0022, Decision 1).
///
/// Everything else (opt-in, finalizer, zone, authorization, validation,
/// apply, prune, migration, cleanup) is the same for every kind and lives in
/// [`reconcile_source`].
pub(crate) trait ScoutSource:
    Resource<DynamicType = (), Scope = NamespaceResourceScope>
    + Clone
    + Debug
    + DeserializeOwned
    + Send
    + Sync
    + 'static
{
    /// The Kubernetes kind, stamped on every record as the source-kind label.
    const KIND: &'static str;

    /// Where `--default-ips` sit relative to [`ScoutSource::discover_ips`].
    const IP_PRECEDENCE: IpPrecedence;

    /// Whether the `bindy.firestoned.io/record-name` override is the only
    /// source of a record name (TCPRoute has no hostnames).
    const REQUIRES_RECORD_NAME_OVERRIDE: bool = false;

    /// Whether this object can publish at all, beyond its opt-in annotation.
    /// A source that stops being publishable is released like an opt-out.
    fn publishable(&self) -> bool {
        true
    }

    /// The hosts to publish, as `(index, host)` pairs; the index names the
    /// record and stays stable when other hosts are skipped. `zone` is the
    /// resolved zone, for kinds that derive their host from it.
    fn hostnames(&self, zone: &str) -> Vec<(usize, String)>;

    /// The IPv4 addresses this object's own status (or its serving Gateway)
    /// advertises. Empty when none are known yet.
    ///
    /// # Errors
    /// Returns an error when a lookup fails with anything other than 404, so
    /// the reconcile is retried rather than published with a fallback IP.
    fn discover_ips(&self, ctx: &ScoutContext) -> impl Future<Output = Result<Vec<String>>> + Send;
}

/// Numbers the non-empty entries of `hosts`, keeping each entry's position.
fn indexed_hosts<'a>(hosts: impl IntoIterator<Item = &'a String>) -> Vec<(usize, String)> {
    hosts
        .into_iter()
        .enumerate()
        .filter(|(_, h)| !h.is_empty())
        .map(|(i, h)| (i, h.clone()))
        .collect()
}

impl ScoutSource for Ingress {
    const KIND: &'static str = "Ingress";
    const IP_PRECEDENCE: IpPrecedence = IpPrecedence::DefaultsFirst;

    /// One host per rule (`spec.rules[].host`); rules without a host are skipped.
    fn hostnames(&self, _zone: &str) -> Vec<(usize, String)> {
        let rules = self.spec.as_ref().and_then(|s| s.rules.as_ref());
        let hosts: Vec<String> = rules
            .into_iter()
            .flatten()
            .map(|r| r.host.clone().unwrap_or_default())
            .collect();
        indexed_hosts(&hosts)
    }

    async fn discover_ips(&self, _ctx: &ScoutContext) -> Result<Vec<String>> {
        Ok(resolve_ip_from_lb_status(self).into_iter().collect())
    }
}

impl ScoutSource for Service {
    const KIND: &'static str = "Service";
    const IP_PRECEDENCE: IpPrecedence = IpPrecedence::DefaultsFirst;

    /// Only `type: LoadBalancer` Services have a routable external IP.
    fn publishable(&self) -> bool {
        is_loadbalancer_service(self)
    }

    /// A Service publishes one host: its own name in the zone.
    fn hostnames(&self, zone: &str) -> Vec<(usize, String)> {
        vec![(0, format!("{}.{zone}", self.name_any()))]
    }

    async fn discover_ips(&self, _ctx: &ScoutContext) -> Result<Vec<String>> {
        Ok(resolve_ip_from_service_lb_status(self)
            .into_iter()
            .collect())
    }
}

impl ScoutSource for HTTPRoute {
    const KIND: &'static str = "HTTPRoute";
    const IP_PRECEDENCE: IpPrecedence = IpPrecedence::DiscoveredFirst;

    fn hostnames(&self, _zone: &str) -> Vec<(usize, String)> {
        indexed_hosts(self.spec.iter().flat_map(|s| s.hostnames.iter().flatten()))
    }

    async fn discover_ips(&self, ctx: &ScoutContext) -> Result<Vec<String>> {
        let refs = self.spec.as_ref().and_then(|s| s.parent_refs.as_deref());
        route_gateway_ips(self, refs, ctx).await
    }
}

impl ScoutSource for TLSRoute {
    const KIND: &'static str = "TLSRoute";
    const IP_PRECEDENCE: IpPrecedence = IpPrecedence::DiscoveredFirst;

    fn hostnames(&self, _zone: &str) -> Vec<(usize, String)> {
        indexed_hosts(self.spec.iter().flat_map(|s| s.hostnames.iter().flatten()))
    }

    async fn discover_ips(&self, ctx: &ScoutContext) -> Result<Vec<String>> {
        let refs = self.spec.as_ref().and_then(|s| s.parent_refs.as_deref());
        route_gateway_ips(self, refs, ctx).await
    }
}

impl ScoutSource for TCPRoute {
    const KIND: &'static str = "TCPRoute";
    const IP_PRECEDENCE: IpPrecedence = IpPrecedence::DiscoveredFirst;
    const REQUIRES_RECORD_NAME_OVERRIDE: bool = true;

    /// TCPRoute has no hostnames: its one record is named by the override.
    fn hostnames(&self, _zone: &str) -> Vec<(usize, String)> {
        Vec::new()
    }

    async fn discover_ips(&self, ctx: &ScoutContext) -> Result<Vec<String>> {
        let refs = self.spec.as_ref().and_then(|s| s.parent_refs.as_deref());
        route_gateway_ips(self, refs, ctx).await
    }
}

/// Resolves a Gateway API route's IPs through its `parentRefs`.
async fn route_gateway_ips<K: Resource>(
    route: &K,
    parent_refs: Option<&[ParentReference]>,
    ctx: &ScoutContext,
) -> Result<Vec<String>> {
    let namespace = route.meta().namespace.clone().unwrap_or_default();
    resolve_ips_from_gateways(
        &ctx.client,
        &namespace,
        parent_refs.unwrap_or_default(),
        &ctx.gateway_services,
    )
    .await
}

/// The identity of one source object: its kind, namespace and name. With
/// the cluster name it is what Scout's record labels and names are built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRef {
    /// The source kind (`Ingress`, `Service`, ...).
    pub kind: &'static str,
    /// The source namespace.
    pub namespace: String,
    /// The source name.
    pub name: String,
}

impl SourceRef {
    /// The identity of `obj`.
    fn of<K: ScoutSource>(obj: &K) -> Self {
        Self {
            kind: K::KIND,
            namespace: obj.namespace().unwrap_or_default(),
            name: obj.name_any(),
        }
    }
}

// ============================================================================
// The pipeline
// ============================================================================

/// Reconciles one source object of any kind (ADR-0022, Decision 1).
///
/// In order:
/// 1. an object in an excluded namespace is ignored;
/// 2. an object being deleted has its records removed (retried for
///    `REMOTE_CLEANUP_GRACE_SECS`, then given up) and its finalizer released;
/// 3. an object that is not opted in, not publishable (a non-LoadBalancer
///    Service), or (checked only after that, since it is an API call) in a
///    namespace outside `--namespace-selector` is released;
/// 4. the finalizer is added before anything is published;
/// 5. the zone, the TTL and the `record-name` override are validated, then
///    the zone's authorization of the namespace is checked live;
/// 6. the IPs are resolved, every host's record is applied, then records of
///    hosts no longer present, legacy records, and records from a previous
///    cluster name are deleted, in that order (apply before delete).
///
/// # Errors
/// Returns an error, retried by the controller's error policy, when an API
/// call fails: a finalizer patch (including one rejected because the object
/// changed concurrently), a record apply or delete, or a Gateway or Service
/// lookup that fails with anything but 404.
pub(crate) async fn reconcile_source<K: ScoutSource>(
    obj: Arc<K>,
    ctx: Arc<ScoutContext>,
) -> Result<Action, ScoutError> {
    let source = SourceRef::of(&*obj);

    if ctx.excluded_namespaces.contains(&source.namespace) {
        debug!(kind = K::KIND, name = %source.name, ns = %source.namespace, "Skipping excluded namespace");
        return Ok(Action::await_change());
    }

    let annotations = obj
        .meta()
        .annotations
        .as_ref()
        .unwrap_or(&EMPTY_ANNOTATIONS);
    let finalized = finalizers::has_finalizer(&*obj, FINALIZER_SCOUT);

    if is_being_deleted(&*obj) {
        if !finalized {
            return Ok(Action::await_change());
        }
        return finalize_deletion(&*obj, &source, annotations, &ctx).await;
    }

    // The cheap checks first: an object Scout ignores costs no API call.
    if !is_scout_opted_in(annotations) || !obj.publishable() {
        return release(&*obj, &source, annotations, finalized, &ctx).await;
    }
    if !source_namespace_eligible(
        &ctx.client,
        &source.namespace,
        ctx.namespace_selector.as_deref(),
    )
    .await?
    {
        return release(&*obj, &source, annotations, finalized, &ctx).await;
    }

    if !finalized {
        add_scout_finalizer(&ctx.client, &*obj).await?;
        debug!(kind = K::KIND, name = %source.name, ns = %source.namespace, "Finalizer added, re-queuing for record creation");
        return Ok(Action::await_change());
    }

    publish(&*obj, &source, annotations, &ctx).await
}

/// Removes a deleted object's records and releases its finalizer.
///
/// A failed cleanup is retried until the object has been terminating for
/// `REMOTE_CLEANUP_GRACE_SECS`; past that the finalizer is released anyway, so
/// an unreachable bindy cluster cannot strand the object in `Terminating`.
async fn finalize_deletion<K: ScoutSource>(
    obj: &K,
    source: &SourceRef,
    annotations: &BTreeMap<String, String>,
    ctx: &ScoutContext,
) -> Result<Action, ScoutError> {
    info!(kind = K::KIND, name = %source.name, ns = %source.namespace, "Source deleting, cleaning up ARecords");
    let zone = resolve_zone(annotations, ctx.default_zone.as_deref());
    if let Err(e) = delete_source_records(ctx, source, zone.as_deref()).await {
        if !cleanup_grace_expired(obj.meta().deletion_timestamp.as_ref(), Timestamp::now()) {
            warn!(kind = K::KIND, name = %source.name, ns = %source.namespace, error = %e, "Remote ARecord cleanup failed during deletion, retrying within the grace period");
            return Ok(requeue_after_error());
        }
        error!(kind = K::KIND, name = %source.name, ns = %source.namespace, error = %e, grace_secs = REMOTE_CLEANUP_GRACE_SECS, "Remote ARecord cleanup still failing after the grace period, releasing the finalizer to unblock deletion; remote ARecords may be orphaned and must be reconciled separately");
    }
    remove_scout_finalizer(&ctx.client, obj).await?;
    info!(kind = K::KIND, name = %source.name, ns = %source.namespace, "Finalizer removed, deletion unblocked");
    Ok(Action::await_change())
}

/// Releases an object Scout no longer publishes for (opted out, no longer
/// publishable, or its namespace left the selector): its records go, then its
/// finalizer. Nothing is done when the finalizer is absent.
async fn release<K: ScoutSource>(
    obj: &K,
    source: &SourceRef,
    annotations: &BTreeMap<String, String>,
    finalized: bool,
    ctx: &ScoutContext,
) -> Result<Action, ScoutError> {
    if !finalized {
        debug!(kind = K::KIND, name = %source.name, ns = %source.namespace, "Not published by Scout, skipping");
        return Ok(Action::await_change());
    }
    info!(kind = K::KIND, name = %source.name, ns = %source.namespace, "Source no longer publishes DNS, cleaning up ARecords and finalizer");
    let zone = resolve_zone(annotations, ctx.default_zone.as_deref());
    delete_source_records(ctx, source, zone.as_deref()).await?;
    remove_scout_finalizer(&ctx.client, obj).await?;
    Ok(Action::await_change())
}

/// The record names one source publishes, as `(index, record name)` pairs.
///
/// The `record-name` override is validated once, up front, so an invalid one
/// stops publication for the whole source rather than skipping every host
/// (and then pruning every record). A host outside the zone is skipped with a
/// warning; its record, if any, is pruned.
///
/// # Errors
/// Returns an error when the override is present but invalid, or when the
/// kind requires an override (TCPRoute) and none is set.
pub(crate) fn planned_records(
    kind: &str,
    hosts: Vec<(usize, String)>,
    annotations: &BTreeMap<String, String>,
    zone: &str,
    requires_override: bool,
) -> Result<Vec<(usize, String)>> {
    if requires_override {
        let name = tcproute_record_name(annotations)?.ok_or_else(|| {
            anyhow!("{kind} has no record-name override (add the bindy.firestoned.io/record-name annotation)")
        })?;
        return Ok(vec![(0, name)]);
    }
    if get_record_name_annotation(annotations).is_some() {
        // P2-7: fail the whole source on an illegal override. The host argument
        // is irrelevant once an override is set.
        resolve_record_name(annotations, zone, zone)?;
    }
    Ok(hosts
        .into_iter()
        .filter_map(|(idx, host)| match resolve_record_name(annotations, &host, zone) {
            Ok(name) => Some((idx, name)),
            Err(e) => {
                warn!(kind = %kind, host = %host, zone = %zone, error = %e, "Host does not belong to the zone, skipping");
                None
            }
        })
        .collect())
}

/// Publishes an opted-in, finalized object's records, then prunes, migrates
/// and cleans up (the last steps of [`reconcile_source`]).
async fn publish<K: ScoutSource>(
    obj: &K,
    source: &SourceRef,
    annotations: &BTreeMap<String, String>,
    ctx: &ScoutContext,
) -> Result<Action, ScoutError> {
    let (kind, name, ns) = (K::KIND, &source.name, &source.namespace);

    let zone = match resolve_usable_zone(annotations, ctx.default_zone.as_deref()) {
        ZoneResolution::Usable(z) => z,
        ZoneResolution::Invalid(z) => {
            warn!(kind, name = %name, ns = %ns, zone = %z, "Resolved DNS zone is not a usable Kubernetes label value (a short label value with an alphanumeric first and last character, and only '-', '_' or '.' between), skipping; Scout stamps the zone onto every ARecord as a label and matches on it when cleaning up records from a previous cluster name");
            return Ok(requeue_after_error());
        }
        ZoneResolution::Missing => {
            warn!(kind, name = %name, ns = %ns, "No DNS zone available (set the bindy.firestoned.io/zone annotation or BINDY_SCOUT_DEFAULT_ZONE), skipping");
            return Ok(requeue_after_error());
        }
    };

    let ttl = match parse_ttl_annotation(annotations) {
        Ok(ttl) => ttl,
        Err(e) => {
            warn!(kind, name = %name, ns = %ns, error = %e, "Invalid TTL annotation, publishing nothing for this source");
            return Ok(requeue_after_error());
        }
    };

    let records = match planned_records(
        kind,
        obj.hostnames(&zone),
        annotations,
        &zone,
        K::REQUIRES_RECORD_NAME_OVERRIDE,
    ) {
        Ok(records) => records,
        Err(e) => {
            warn!(kind, name = %name, ns = %ns, zone = %zone, error = %e, "Invalid or missing record-name override, publishing nothing for this source");
            return Ok(requeue_after_error());
        }
    };

    // H1: a matching DNSZone must exist AND authorize this namespace,
    // re-checked live just before the write (P3-4).
    match check_zone_authorization_live(&ctx.remote_client, &ctx.zone_store.state(), &zone, ns)
        .await
    {
        ZoneAuthz::Authorized => {}
        ZoneAuthz::Forbidden => {
            warn!(kind, name = %name, ns = %ns, zone = %zone, "Namespace not authorized for the zone: the DNSZone must live in this namespace or list it (or '*') in bindy.firestoned.io/allow-zone-namespaces, skipping");
            return Ok(requeue_after_error());
        }
        ZoneAuthz::NotFound => {
            warn!(kind, name = %name, ns = %ns, zone = %zone, "Zone not found in the DNSZone store, skipping until it appears");
            return Ok(requeue_after_error());
        }
    }

    let Some(ips) = resolve_source_ips(obj, annotations, ctx).await? else {
        warn!(kind, name = %name, ns = %ns, "No IP available (no annotation override, no discovered IP, no default IPs), requeuing");
        return Ok(requeue_after_error());
    };

    let api: Api<ARecord> = Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);
    let apply = PatchParams::apply(SCOUT_FIELD_MANAGER).force();
    let mut applied = BTreeSet::new();
    let mut all_served = true;
    for (idx, record_name) in records {
        let cr_name = arecord_cr_name(&ctx.cluster_name, kind, ns, name, idx);
        let arecord = build_arecord(ARecordParams {
            name: &cr_name,
            target_namespace: &ctx.target_namespace,
            record_name: &record_name,
            ips: &ips,
            ttl,
            cluster_name: &ctx.cluster_name,
            source_kind: kind,
            source_namespace: ns,
            source_name: name,
            zone: &zone,
        });
        let live = api
            .patch(&cr_name, &apply, &Patch::Apply(&arecord))
            .await
            .with_context(|| format!("Failed to apply ARecord {cr_name}"))?;
        info!(arecord = %cr_name, kind, name = %name, record_name = %record_name, ips = ?ips, "ARecord created/updated");
        all_served &= arecord_served(&live);
        applied.insert(cr_name);
    }

    prune_unapplied_arecords(ctx, source, &applied).await?;

    // Legacy and previous-cluster records publish the same names as the
    // records just applied. Deleting one before its replacement is served
    // would take the name off the air (ADR-0022, Decision 4).
    let mut replaced = list_arecords(
        &api,
        &legacy_arecord_label_selector(&ctx.cluster_name, ns, name),
    )
    .await?;
    replaced.extend(stale_cluster_arecords(&api, &ctx.cluster_name, source, &zone).await?);
    match handover_step(replaced.len(), all_served) {
        Handover::Nothing => {}
        Handover::Wait => {
            info!(kind, name = %name, ns = %ns, waiting = replaced.len(), "Keeping the records this source replaces until its new ARecords are bound to the zone and Ready");
            return Ok(Action::requeue(Duration::from_secs(HANDOVER_RECHECK_SECS)));
        }
        Handover::Delete => {
            delete_arecords(&api, replaced, "replaced by this source's served record").await?;
        }
    }

    Ok(Action::await_change())
}

/// How often a source rechecks whether its new ARecords are served while a
/// legacy or previous-cluster record waits to be replaced.
pub(crate) const HANDOVER_RECHECK_SECS: u64 = 10;

/// What [`publish`] does with the records its new ARecords replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handover {
    /// There is nothing to replace.
    Nothing,
    /// A new record is not served yet: keep the old ones and recheck.
    Wait,
    /// Every new record is served: delete the old ones.
    Delete,
}

/// Decides the handover from `replaced` legacy or previous-cluster records
/// to the source's new ones, given whether every new record is served.
pub(crate) fn handover_step(replaced: usize, all_served: bool) -> Handover {
    if replaced == 0 {
        return Handover::Nothing;
    }
    if all_served {
        return Handover::Delete;
    }
    Handover::Wait
}

/// Whether the operator serves `record`: it is bound to its zone
/// (`status.zoneRef`) and its `Ready` condition is `True`.
pub(crate) fn arecord_served(record: &ARecord) -> bool {
    record
        .status
        .as_ref()
        .is_some_and(|s| s.zone_ref.is_some() && is_ready(&s.conditions))
}

/// Resolves the IPs for a source: the `bindy.firestoned.io/ip` annotation,
/// then `--default-ips` and the discovered IPs in the kind's
/// [`IpPrecedence`]. Discovery (an API call for routes) is skipped when an
/// earlier source already answered.
async fn resolve_source_ips<K: ScoutSource>(
    obj: &K,
    annotations: &BTreeMap<String, String>,
    ctx: &ScoutContext,
) -> Result<Option<Vec<String>>> {
    if let Some(ips) = resolve_ips_from_annotation(annotations) {
        return Ok(Some(ips));
    }
    let defaults = (!ctx.default_ips.is_empty()).then(|| ctx.default_ips.clone());
    if K::IP_PRECEDENCE == IpPrecedence::DefaultsFirst && defaults.is_some() {
        return Ok(defaults);
    }
    let discovered = obj.discover_ips(ctx).await?;
    if discovered.is_empty() {
        return Ok(defaults);
    }
    Ok(Some(discovered))
}

// ============================================================================
// Record operations (remote client)
// ============================================================================

/// Lists the ARecords in `target_namespace` matching `selector`.
async fn list_arecords(api: &Api<ARecord>, selector: &str) -> Result<Vec<ARecord>> {
    list_all_paginated(api, ListParams::default().labels(selector))
        .await
        .with_context(|| format!("failed to list ARecords matching {selector:?}"))
}

/// Deletes `records`, returning their `zone` labels.
async fn delete_arecords(
    api: &Api<ARecord>,
    records: Vec<ARecord>,
    reason: &str,
) -> Result<BTreeSet<String>> {
    let mut zones = BTreeSet::new();
    for ar in records {
        let ar_name = ar.name_any();
        if let Some(zone) = ar.labels().get(LABEL_ZONE) {
            zones.insert(zone.clone());
        }
        api.delete(&ar_name, &DeleteParams::default())
            .await
            .with_context(|| format!("failed to delete ARecord {ar_name}"))?;
        info!(arecord = %ar_name, reason = %reason, "Deleted ARecord");
    }
    Ok(zones)
}

/// Deletes every ARecord of one source: its own, its legacy records, and
/// the stale-cluster records of every zone in play.
///
/// `resolved_zone` (annotation or default) is unioned with the zones of the
/// records just deleted (see `stale_cleanup_zones`), since an opt-out edit
/// often strips the zone annotation too. When no usable zone is known, the
/// stale-cluster candidates are only logged, never deleted.
///
/// # Errors
/// Returns an error if listing or deleting ARecords on the remote cluster fails.
pub(crate) async fn delete_source_records(
    ctx: &ScoutContext,
    source: &SourceRef,
    resolved_zone: Option<&str>,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);
    let own = list_arecords(
        &api,
        &arecord_label_selector(
            &ctx.cluster_name,
            source.kind,
            &source.namespace,
            &source.name,
        ),
    )
    .await?;
    let mut zones = delete_arecords(&api, own, "source cleanup").await?;
    zones.extend(delete_legacy_arecords(ctx, source).await?);

    let cleanup_zones = stale_cleanup_zones(resolved_zone, &zones);
    if cleanup_zones.is_empty() {
        warn!(kind = source.kind, name = %source.name, ns = %source.namespace, "No usable DNS zone (missing or invalid bindy.firestoned.io/zone annotation, and no BINDY_SCOUT_DEFAULT_ZONE), skipping stale-cluster ARecord cleanup; this source's own ARecords were still removed");
        log_unscoped_stale_cluster_arecord_candidates(
            &ctx.remote_client,
            &ctx.target_namespace,
            &ctx.cluster_name,
            source,
        )
        .await;
        return Ok(());
    }
    for zone in &cleanup_zones {
        delete_stale_cluster_arecords(
            &ctx.remote_client,
            &ctx.target_namespace,
            &ctx.cluster_name,
            source,
            zone,
        )
        .await?;
    }
    Ok(())
}

/// Deletes the source's own records that were not applied in this pass:
/// hosts dropped from the source, or no longer in the zone.
///
/// The selector carries the source kind, so a same-named object of another
/// kind is never touched (ADR-0022, Decision 2).
///
/// # Errors
/// Returns an error if listing or deleting ARecords on the remote cluster fails.
pub(crate) async fn prune_unapplied_arecords(
    ctx: &ScoutContext,
    source: &SourceRef,
    applied: &BTreeSet<String>,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);
    let selector = arecord_label_selector(
        &ctx.cluster_name,
        source.kind,
        &source.namespace,
        &source.name,
    );
    let stale: Vec<ARecord> = list_arecords(&api, &selector)
        .await?
        .into_iter()
        .filter(|ar| !applied.contains(&ar.name_any()))
        .collect();
    delete_arecords(&api, stale, "host no longer published").await?;
    Ok(())
}

/// Deletes the source's legacy records: those an earlier Scout wrote under
/// this cluster, namespace and name, with no source-kind label (ADR-0022,
/// Decision 4). Used when the source itself goes away; the publish path
/// hands its legacy records over in [`publish`] instead, once the new records
/// are served.
///
/// # Errors
/// Returns an error if listing or deleting ARecords on the remote cluster fails.
pub(crate) async fn delete_legacy_arecords(
    ctx: &ScoutContext,
    source: &SourceRef,
) -> Result<BTreeSet<String>> {
    let api: Api<ARecord> = Api::namespaced(ctx.remote_client.clone(), &ctx.target_namespace);
    let legacy = list_arecords(
        &api,
        &legacy_arecord_label_selector(&ctx.cluster_name, &source.namespace, &source.name),
    )
    .await?;
    delete_arecords(
        &api,
        legacy,
        "legacy record replaced by a source-kind record",
    )
    .await
}

/// Deletes the source's records left by a **previous** cluster name in the
/// same DNS zone: `source-cluster != current_cluster`, `zone = current_zone`,
/// and either this source's kind or (a record from before ADR-0022) no kind.
///
/// Used when the source goes away. On publish, [`publish`] hands these
/// records over once the new ones are served, so a Scout restarted with a new
/// `--cluster-name` cleans up after itself. The zone match keeps it from
/// deleting a live record of an unrelated cluster that publishes into a
/// different zone (#474).
///
/// # Errors
/// Returns an error if listing or deleting ARecords on the remote cluster fails.
pub(crate) async fn delete_stale_cluster_arecords(
    remote_client: &Client,
    target_namespace: &str,
    current_cluster: &str,
    source: &SourceRef,
    current_zone: &str,
) -> Result<()> {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    for ar in stale_cluster_arecords(&api, current_cluster, source, current_zone).await? {
        let old_cluster = ar
            .labels()
            .get(LABEL_SOURCE_CLUSTER)
            .cloned()
            .unwrap_or_default();
        let ar_name = ar.name_any();
        api.delete(&ar_name, &DeleteParams::default())
            .await
            .with_context(|| format!("failed to delete ARecord {ar_name}"))?;
        info!(arecord = %ar_name, old_cluster = %old_cluster, new_cluster = %current_cluster, kind = source.kind, name = %source.name, ns = %source.namespace, "Deleted stale ARecord after a cluster-name change");
    }
    Ok(())
}

/// Lists the source's records left by a previous cluster name in
/// `current_zone`, of this source's kind or (from before ADR-0022) of no kind.
///
/// # Errors
/// Returns an error if listing ARecords on the remote cluster fails.
async fn stale_cluster_arecords(
    api: &Api<ARecord>,
    current_cluster: &str,
    source: &SourceRef,
    current_zone: &str,
) -> Result<Vec<ARecord>> {
    let mut stale = Vec::new();
    for kind in [StaleKind::Kind(source.kind), StaleKind::Legacy] {
        let selector = stale_arecord_label_selector(
            current_cluster,
            kind,
            &source.namespace,
            &source.name,
            current_zone,
        );
        stale.extend(list_arecords(api, &selector).await?);
    }
    Ok(stale)
}

/// Logs, without deleting, the source's records from other cluster names,
/// deliberately unscoped by zone.
///
/// Used only when a delete or release path has no usable zone. The finalizer
/// is removed right after, so silently skipping would orphan such records
/// with no trace; without a zone Scout cannot tell its own stale records from
/// a live one of another cluster, so it only names them for an operator.
///
/// Never fails the caller: list errors are logged and swallowed.
pub(crate) async fn log_unscoped_stale_cluster_arecord_candidates(
    remote_client: &Client,
    target_namespace: &str,
    current_cluster: &str,
    source: &SourceRef,
) {
    let api: Api<ARecord> = Api::namespaced(remote_client.clone(), target_namespace);
    for kind in [StaleKind::Kind(source.kind), StaleKind::Legacy] {
        let selector = stale_selector_base(current_cluster, kind, &source.namespace, &source.name);
        match list_arecords(&api, &selector).await {
            Ok(records) if records.is_empty() => {}
            Ok(records) => {
                let candidates: Vec<String> = records.iter().map(ResourceExt::name_any).collect();
                warn!(kind = source.kind, name = %source.name, ns = %source.namespace, candidates = ?candidates, "No usable DNS zone for stale-cluster ARecord cleanup; these ARecords from other cluster names were left behind and must be reviewed and deleted manually");
            }
            Err(e) => {
                warn!(kind = source.kind, name = %source.name, ns = %source.namespace, error = %e, "Failed to list unscoped stale-cluster ARecord candidates for diagnostic logging");
            }
        }
    }
}

// ============================================================================
// Gateway API IP discovery (local client)
// ============================================================================

/// Resolves the external IP of the LoadBalancer Service a gateway class maps to.
///
/// For a [`GatewayServiceTarget::Name`] the Service is fetched directly; for a
/// [`GatewayServiceTarget::Labeled`] target the namespace is listed by label
/// selector and the first LoadBalancer Service with an external IPv4 is used.
/// Returns `Ok(None)` when the Service does not exist (404) or has no external
/// IP yet.
///
/// # Errors
/// Returns an error for any other lookup failure (ADR-0022, Decision 6).
async fn resolve_ip_from_gateway_service(
    client: &Client,
    target: &GatewayServiceTarget,
) -> Result<Option<String>> {
    match target {
        GatewayServiceTarget::Name(svc_ref) => {
            let api: Api<Service> = Api::namespaced(client.clone(), &svc_ref.namespace);
            match api.get(&svc_ref.name).await {
                Ok(svc) => Ok(resolve_ip_from_service_lb_status(&svc)),
                Err(e) if kube_errors::is_not_found(&e) => {
                    debug!(service = %svc_ref.name, ns = %svc_ref.namespace, "Gateway LoadBalancer Service not found");
                    Ok(None)
                }
                Err(e) => Err(anyhow!(
                    "failed to fetch gateway LoadBalancer Service {}/{}: {e}",
                    svc_ref.namespace,
                    svc_ref.name
                )),
            }
        }
        GatewayServiceTarget::Labeled {
            namespace,
            selector,
        } => {
            let api: Api<Service> = Api::namespaced(client.clone(), namespace);
            let services = list_all_paginated(&api, ListParams::default().labels(selector))
                .await
                .with_context(|| {
                    format!("failed to list gateway LoadBalancer Services in {namespace} matching {selector:?}")
                })?;
            Ok(services
                .iter()
                .filter(|svc| is_loadbalancer_service(svc))
                .find_map(resolve_ip_from_service_lb_status))
        }
    }
}

/// Follows a route's `parentRefs` back to the serving Gateway(s) and resolves
/// their external IPv4 address(es).
///
/// For each `parentRef` that resolves to a Gateway, Scout reads the Gateway's
/// IPv4 `status.addresses`; when there are none, it reads the external IP of
/// the LoadBalancer Service mapped to the Gateway's class in
/// `--gateway-service`. A Gateway that does not exist (404) is skipped.
/// Returns the de-duplicated IPs in discovery order (empty when none resolve).
///
/// # Arguments
/// * `client` - Local cluster client (Gateways and Services live on the workload cluster)
/// * `route_namespace` - Namespace of the route, the default parentRef namespace
/// * `parent_refs` - The route's `spec.parentRefs`
/// * `gateway_services` - `gatewayClass -> Service` map used when a Gateway has
///   no `status.addresses`
///
/// # Errors
/// Returns an error when a Gateway or Service lookup fails with anything other
/// than 404, so the reconcile is retried (ADR-0022, Decision 6).
pub(crate) async fn resolve_ips_from_gateways(
    client: &Client,
    route_namespace: &str,
    parent_refs: &[ParentReference],
    gateway_services: &BTreeMap<String, GatewayServiceTarget>,
) -> Result<Vec<String>> {
    let mut ips: Vec<String> = Vec::new();
    for gw_ref in gateway_parent_refs(parent_refs, route_namespace) {
        let api: Api<Gateway> = Api::namespaced(client.clone(), &gw_ref.namespace);
        let gateway = match api.get(&gw_ref.name).await {
            Ok(gw) => gw,
            Err(e) if kube_errors::is_not_found(&e) => {
                debug!(gateway = %gw_ref.name, ns = %gw_ref.namespace, "Skipping a parentRef Gateway that does not exist");
                continue;
            }
            Err(e) => {
                return Err(anyhow!(
                    "failed to fetch parentRef Gateway {}/{}: {e}",
                    gw_ref.namespace,
                    gw_ref.name
                ))
            }
        };

        let gw_ips = gateway_addresses_as_ips(&gateway);
        if !gw_ips.is_empty() {
            ips.extend(gw_ips);
            continue;
        }

        let class = &gateway.spec.gateway_class_name;
        let Some(target) = gateway_services.get(class) else {
            debug!(gateway = %gw_ref.name, class = %class, "Gateway has no status.addresses and its class is not in --gateway-service, skipping");
            continue;
        };
        ips.extend(resolve_ip_from_gateway_service(client, target).await?);
    }

    let mut seen = BTreeSet::new();
    ips.retain(|ip| seen.insert(ip.clone()));
    Ok(ips)
}

// ============================================================================
// Finalizers (local client)
// ============================================================================

/// Adds the Scout finalizer to a source object if it is not already there.
///
/// Delegates to the SDK's [`finalizers::ensure_finalizer`], whose JSON Patch
/// is guarded by a `test` operation, so a finalizer another controller added
/// or removed since Scout read the object is never overwritten.
///
/// # Errors
/// Returns an error if the patch fails, including when the API server rejects
/// it because the object changed concurrently; the requeued reconcile then
/// works from fresh state.
pub(crate) async fn add_scout_finalizer<K: ScoutSource>(client: &Client, obj: &K) -> Result<()> {
    let api: Api<K> = Api::namespaced(client.clone(), &obj.namespace().unwrap_or_default());
    finalizers::ensure_finalizer(&api, obj, FINALIZER_SCOUT).await
}

/// Removes the Scout finalizer from a source object if it is there.
///
/// Delegates to the SDK's [`finalizers::remove_finalizer`], which removes
/// Scout's entry by index after `test`ing the index still holds it.
///
/// # Errors
/// Returns an error if the patch fails, including when the list changed
/// concurrently; the record cleanup before it is idempotent and runs again.
pub(crate) async fn remove_scout_finalizer<K: ScoutSource>(client: &Client, obj: &K) -> Result<()> {
    let api: Api<K> = Api::namespaced(client.clone(), &obj.namespace().unwrap_or_default());
    finalizers::remove_finalizer(&api, obj, FINALIZER_SCOUT).await
}

// ============================================================================
// Controllers
// ============================================================================

/// The error policy for every source kind: log, then requeue after
/// `SCOUT_ERROR_REQUEUE_SECS`.
fn error_policy<K: ScoutSource>(
    obj: Arc<K>,
    error: &ScoutError,
    _ctx: Arc<ScoutContext>,
) -> Action {
    error!(kind = K::KIND, name = %obj.name_any(), ns = %obj.namespace().unwrap_or_default(), error = %error, "Scout reconcile error, requeuing");
    requeue_after_error()
}

/// Builds the controller for one source kind: `K`'s watch from `watch`
/// (which must have registered `K`), driven through [`reconcile_source`].
pub(crate) fn source_controller<K: ScoutSource>(
    watch: &WatchSet,
    ctx: Arc<ScoutContext>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(
        Controller::for_stream(watch.subscribe::<K>(None), watch.store::<K>(None))
            .run(reconcile_source::<K>, error_policy::<K>, ctx)
            .for_each(|res| async move {
                match res {
                    Ok(obj) => debug!(kind = K::KIND, obj = ?obj, "Reconciled"),
                    Err(e) => error!(kind = K::KIND, error = %e, "Reconcile failed"),
                }
            }),
    )
}
