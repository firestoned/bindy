// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Generic DNS record operator implementation.
//!
//! This module provides a generic operator pattern for all DNS record types,
//! eliminating code duplication across A, AAAA, TXT, CNAME, MX, NS, SRV, and CAA records.

use crate::context::Context;
use crate::crd::{DNSZone, RecordStatus};
use crate::record_wrappers::{action_for_outcome, ready_state, ReadyState, RecordOutcome};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::Api;
use kube::runtime::controller::Action;
use kube::runtime::finalizer;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::{Controller, Predicate, WatchStreamExt};
use kube::ResourceExt;
use serde::Serialize;
use std::sync::Arc;
use tracing::{info, warn};

use bindy_controller_sdk::error::error_policy;
pub use bindy_controller_sdk::error::ReconcileError;

/// Trait for DNS record types that can be reconciled with a generic operator.
///
/// This trait abstracts over the common operations needed for all DNS record types,
/// allowing a single operator implementation to handle all record types.
/// The kind name comes from the supertrait
/// [`RecordKind`](crate::context::RecordKind), so each record type declares it
/// once for the stores, the watch layer and this controller; the BIND9 write
/// comes from [`ReconcilableRecord`](bindy_bind9::record_push::ReconcilableRecord).
pub trait DnsRecordType:
    crate::context::RecordKind + bindy_bind9::record_push::ReconcilableRecord + Serialize
{
    /// The finalizer name for this record type
    const FINALIZER: &'static str;

    /// The DNS record type string (e.g., `A`, `TXT`)
    const RECORD_TYPE_STR: &'static str;

    /// Get the `hickory_proto` `RecordType` value
    fn hickory_record_type() -> hickory_proto::rr::RecordType;

    /// Reconcile this record (create/update in BIND9).
    ///
    /// Every kind goes through the one generic path
    /// ([`crate::records::reconcile_record`]), so this default is the only
    /// implementation; the nine per-kind `reconcile_*_record` wrappers that used
    /// to sit in front of it are gone (roadmap 01 Phase D). Returns how the
    /// reconcile ended, so the wrapper does not re-read the record (ADR-0016).
    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<RecordOutcome, ReconcileError>> + Send {
        async move {
            crate::records::reconcile_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
    }

    /// Get the metadata for this resource
    fn metadata(&self) -> &ObjectMeta;

    /// Get the status for this resource
    fn status(&self) -> &Option<RecordStatus>;
}

/// Run a generic DNS record operator.
///
/// This function creates an operator that watches both the record type and `DNSZone` resources,
/// triggering reconciliation when zones discover new records that need configuration.
///
/// # Arguments
///
/// * `context` - The operator context with API client and stores
///
/// # Errors
///
/// Returns an error if the operator fails to start or encounters a fatal error.
pub async fn run_generic_record_operator<T>(context: Arc<Context>) -> Result<()>
where
    T: DnsRecordType,
{
    info!("Starting {} operator", T::KIND);

    // Record kinds are namespaced: one controller per watched namespace, all sharing
    // the reconciler and context. Cluster-wide mode yields exactly one.
    let targets = bindy_controller_sdk::namespace_scope::owned_targets(&context.namespace_scope);
    futures::future::join_all(
        targets
            .into_iter()
            .map(|target| run_generic_record_controller::<T>(context.clone(), target)),
    )
    .await;

    Ok(())
}

/// Run the record controller for one record kind in a single namespace target.
///
/// `target` is `None` for cluster-wide, or `Some(namespace)`.
async fn run_generic_record_controller<T>(context: Arc<Context>, target: Option<String>)
where
    T: DnsRecordType,
{
    tracing::debug!(
        kind = T::KIND,
        namespace = target.as_deref().unwrap_or("<all>"),
        "Starting record controller"
    );

    // The record kind and DNSZone streams come from the shared WatchSet, which
    // forwards every change, status updates included (ADR-0009 §3).
    let ws = context.watch.clone();
    let target = target.as_deref();

    // The primary stream passes spec, finalizer, label and annotation changes
    // (`primary_predicate`) plus a change of `status.zoneRef`, which is how a
    // DNSZone hands a record to this controller. It drops the reconciler's own
    // condition writes, each of which used to wake the record again for a
    // full no-op reconcile (ADR-0015).
    let primary = ws.subscribe::<T>(target).predicate_filter(
        bindy_controller_sdk::watch::primary_predicate::<T>()
            .combine(zone_ref_hash::<T> as fn(&T) -> Option<u64>),
        Default::default(),
    );

    // The DNSZone mapper wakes the records a zone lists that still need
    // work: never stamped, or not Ready in the record store. A record waiting
    // on its zone (ZoneNotFound, ZoneNotConfigured, NoPrimaryInstances) is
    // woken by the zone's next status change; there is no timer (ADR-0016).
    let record_store = ws.store::<T>(target);
    Controller::for_stream(primary, ws.store::<T>(target))
        .watches_stream(ws.subscribe::<DNSZone>(target), move |zone| {
            records_to_wake_for_zone::<T>(&zone, |record_ref| {
                record_store
                    .get(record_ref)
                    .is_some_and(|record| matches!(ready_state(record.status()), ReadyState::Ready))
            })
        })
        .graceful_shutdown_on(context.shutdown.wait())
        .run(reconcile_wrapper::<T>, error_policy, context.clone())
        .for_each(|_| futures::future::ready(()))
        .await;
}

/// The records of kind `T` a change of `zone` must wake.
///
/// A record listed in `zone.status.records` is woken when the zone has not
/// stamped it yet (`lastReconciledAt` is unset: it was never published) or
/// when `is_ready` says its cached status is not Ready. The second case is
/// what replaced the 30 s not-Ready requeue: a record waiting on its zone
/// gaining instances or primaries is woken by the zone's status write, and a
/// Ready record is left alone so zone status writes do not fan out. Pure: no
/// I/O (ADR-0009 §5).
///
/// # Arguments
///
/// * `zone` - The zone that changed
/// * `is_ready` - Whether the record store holds the record as Ready
///
/// # Returns
///
/// References to the records to reconcile.
#[must_use]
pub fn records_to_wake_for_zone<T: DnsRecordType>(
    zone: &DNSZone,
    is_ready: impl Fn(&ObjectRef<T>) -> bool,
) -> Vec<ObjectRef<T>> {
    let Some(namespace) = zone.namespace() else {
        return vec![];
    };
    let Some(status) = zone.status.as_ref() else {
        return vec![];
    };

    status
        .records
        .iter()
        .filter(|record_ref| record_ref.kind == T::KIND && record_ref.namespace == namespace)
        .filter_map(|record_ref| {
            let object_ref = ObjectRef::new(&record_ref.name).within(&record_ref.namespace);
            let needs_work = record_ref.last_reconciled_at.is_none() || !is_ready(&object_ref);
            needs_work.then_some(object_ref)
        })
        .collect()
}

/// Hash of a record's `status.zoneRef`: the one status field another
/// controller writes that this controller must react to.
///
/// Used as a `kube` predicate on the record controller's primary stream.
/// Always returns `Some`, because a predicate returning `None` passes every
/// event.
///
/// # Arguments
///
/// * `record` - The record whose zone assignment is hashed
#[must_use]
pub fn zone_ref_hash<T: DnsRecordType>(record: &T) -> Option<u64> {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let zone_ref = record.status().as_ref().and_then(|s| s.zone_ref.as_ref());
    match zone_ref {
        Some(z) => {
            true.hash(&mut hasher);
            z.namespace.hash(&mut hasher);
            z.name.hash(&mut hasher);
            z.zone_name.hash(&mut hasher);
        }
        None => false.hash(&mut hasher),
    }
    Some(hasher.finish())
}

/// Generic reconciliation wrapper with finalizer support.
///
/// This function handles the common reconciliation pattern for all DNS record types:
/// 1. Finalizer management (add on apply, remove on cleanup)
/// 2. Reconciliation logic (create/update or delete)
/// 3. Metrics recording
/// 4. Error handling
async fn reconcile_wrapper<T>(
    record: Arc<T>,
    context: Arc<Context>,
) -> Result<Action, ReconcileError>
where
    T: DnsRecordType,
{
    let start = std::time::Instant::now();

    let client = context.client.clone();
    let namespace = record
        .metadata()
        .namespace
        .as_ref()
        .ok_or_else(|| ReconcileError::from(anyhow!("{} has no namespace", T::KIND)))?;
    let api: Api<T> = Api::namespaced(client.clone(), namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, T::FINALIZER, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record. The outcome is returned, not
                // re-read with a GET (ADR-0016): a failed BIND9 write is
                // reported through the record's own conditions and through
                // the outcome, so the log below cannot contradict the status.
                let outcome = T::reconcile_record(context.clone(), (*rec).clone()).await?;
                if outcome.is_ready() {
                    info!("Successfully reconciled {}: {}", T::KIND, rec.name_any());
                } else {
                    warn!(
                        "Reconciled {} {} but it is not Ready: {}",
                        T::KIND,
                        rec.name_any(),
                        outcome.describe()
                    );
                }

                Ok(action_for_outcome(rec.as_ref(), &outcome))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use crate::records::delete_record;

                delete_record(
                    &client,
                    &*rec,
                    T::RECORD_TYPE_STR,
                    T::hickory_record_type(),
                    &context.stores,
                )
                .await
                .map_err(ReconcileError::from)?;

                // The object is gone; a recreated one must not inherit its
                // rejection cooldown.
                bindy_controller_sdk::retry::clear_rejected_write(&format!(
                    "{}/{}/{}",
                    T::KIND,
                    rec.namespace().unwrap_or_default(),
                    rec.name_any()
                ));

                info!(
                    "Successfully deleted {} from BIND9: {}",
                    T::KIND,
                    rec.name_any()
                );
                crate::metrics::record_resource_deleted(T::KIND);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        crate::metrics::record_reconciliation_success(T::KIND, duration);
    } else {
        crate::metrics::record_reconciliation_error(T::KIND, duration);
        crate::metrics::record_error(T::KIND, crate::record_wrappers::ERROR_TYPE_RECONCILE);
    }

    result.map_err(|e| bindy_controller_sdk::reconcile::finalizer_error(T::KIND, e))
}
