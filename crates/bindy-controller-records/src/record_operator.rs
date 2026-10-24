// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Generic DNS record operator implementation.
//!
//! One controller, generic over the record kind: every kind in the
//! record-kind table runs it (ADR-0020). What a kind is comes from
//! [`RecordKind`]; how its data is written, from
//! [`ReconcilableRecord`].

use crate::context::{Context, RecordKind};
use crate::crd::DNSZone;
use crate::record_wrappers::{action_for_outcome, ready_state, ReadyState};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use kube::api::Api;
use kube::runtime::controller::Action;
use kube::runtime::finalizer;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::{Controller, Predicate, WatchStreamExt};
use kube::ResourceExt;
use std::sync::Arc;
use tracing::{info, warn};

use bindy_bind9::record_push::ReconcilableRecord;
use bindy_controller_sdk::error::error_policy;
pub use bindy_controller_sdk::error::ReconcileError;
use bindy_controller_sdk::namespace_scope::{owned_targets, run_per_target, target_label};
use bindy_controller_sdk::reconcile::{finalizer_error, instrumented_action};

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
/// Returns an error when a namespace target's controller exits before the
/// shutdown signal fired (see [`run_per_target`]), so the caller's
/// `try_join_all` stops at the first one.
pub async fn run_generic_record_operator<T>(context: Arc<Context>) -> Result<()>
where
    T: ReconcilableRecord,
{
    info!("Starting {} operator", T::KIND);

    // Record kinds are namespaced: one controller per watched namespace, all sharing
    // the reconciler and context. Cluster-wide mode yields exactly one.
    let targets = owned_targets(&context.namespace_scope);
    run_per_target(targets, &context.shutdown, |target| {
        run_generic_record_controller::<T>(context.clone(), target)
    })
    .await
}

/// Run the record controller for one record kind in a single namespace target.
///
/// `target` is `None` for cluster-wide, or `Some(namespace)`.
///
/// # Errors
///
/// Never fails on its own: the controller stream only ends on shutdown.
async fn run_generic_record_controller<T>(
    context: Arc<Context>,
    target: Option<String>,
) -> Result<()>
where
    T: ReconcilableRecord,
{
    tracing::debug!(
        kind = T::KIND,
        namespace = target_label(target.as_deref()),
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
    // ADR-0027 Decision 5: a change to a Scout record, status-only changes
    // included (a cleared zoneRef, a new publishedName), wakes the other Scout
    // records that claim its name, so a held-back record takes over when the
    // holder leaves. The unfiltered subscription carries what the primary
    // stream's predicate drops.
    let rival_store = ws.store::<T>(target);
    Controller::for_stream(primary, ws.store::<T>(target))
        .watches_stream(ws.subscribe::<T>(target), move |changed| {
            scout_rivals_to_wake::<T>(&changed, &rival_store.state())
        })
        .watches_stream(ws.subscribe::<DNSZone>(target), move |zone| {
            records_to_wake_for_zone::<T>(&zone, |record_ref| {
                record_store.get(record_ref).is_some_and(|record| {
                    matches!(ready_state(record.record_status()), ReadyState::Ready)
                })
            })
        })
        .graceful_shutdown_on(context.shutdown.wait())
        .run(reconcile_wrapper::<T>, error_policy, context.clone())
        .for_each(|_| futures::future::ready(()))
        .await;
    Ok(())
}

/// The Scout records in `live` a change of the Scout record `changed` must
/// wake (ADR-0027 Decision 5): the other Scout records of the kind that
/// declare the same name (ignoring case) in the same zone. A `changed` record
/// no zone binds any more (its holder role just ended) wakes the name in every
/// zone. A record that is not Scout-managed wakes nothing. Pure: no I/O.
///
/// # Arguments
///
/// * `changed` - The record that changed
/// * `live` - The kind's reflector store
///
/// # Returns
///
/// References to the records to reconcile.
#[must_use]
pub fn scout_rivals_to_wake<T: RecordKind>(changed: &T, live: &[Arc<T>]) -> Vec<ObjectRef<T>> {
    if !is_scout_record(changed) {
        return vec![];
    }
    let zone_of = |record: &T| {
        record
            .record_status()
            .and_then(|s| s.zone_ref.as_ref())
            .map(|z| (z.namespace.clone(), z.name.clone()))
    };
    let changed_zone = zone_of(changed);
    let dns_name = changed.spec_name();
    live.iter()
        .filter(|record| {
            record.meta().name != changed.meta().name
                || record.meta().namespace != changed.meta().namespace
        })
        .filter(|record| is_scout_record(record.as_ref()))
        .filter(|record| record.spec_name().eq_ignore_ascii_case(dns_name))
        .filter(|record| match &changed_zone {
            Some(zone) => zone_of(record.as_ref()).as_ref() == Some(zone),
            None => true,
        })
        .map(|record| ObjectRef::from_obj(record.as_ref()))
        .collect()
}

/// Whether `record` was written by Scout (`bindy.firestoned.io/managed-by:
/// scout`).
fn is_scout_record<T: RecordKind>(record: &T) -> bool {
    record
        .meta()
        .labels
        .as_ref()
        .and_then(|l| l.get(bindy_api::labels::BINDY_MANAGED_BY_LABEL))
        .is_some_and(|v| v == bindy_api::labels::MANAGED_BY_SCOUT)
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
pub fn records_to_wake_for_zone<T: RecordKind>(
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
pub fn zone_ref_hash<T: RecordKind>(record: &T) -> Option<u64> {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let zone_ref = record.record_status().and_then(|s| s.zone_ref.as_ref());
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
    T: ReconcilableRecord,
{
    let client = context.client.clone();
    let namespace = record
        .meta()
        .namespace
        .as_ref()
        .ok_or_else(|| ReconcileError::from(anyhow!("{} has no namespace", T::KIND)))?;
    let api: Api<T> = Api::namespaced(client.clone(), namespace);

    // Handle deletion with finalizer
    let reconcile = finalizer(
        &api,
        T::DNS_KIND.finalizer(),
        record.clone(),
        |event| async {
            match event {
                finalizer::Event::Apply(rec) => {
                    // Create or update the record. The outcome is returned, not
                    // re-read with a GET (ADR-0016): a failed BIND9 write is
                    // reported through the record's own conditions and through
                    // the outcome, so the log below cannot contradict the status.
                    let outcome = crate::records::reconcile_record(context.clone(), (*rec).clone())
                        .await
                        .map_err(ReconcileError::from)?;
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

                    delete_record(&client, &*rec, &context.stores)
                        .await
                        .map_err(ReconcileError::from)?;

                    // The object is gone; a recreated one must not inherit its
                    // rejection cooldown.
                    bindy_controller_sdk::retry::clear_rejected_write(
                        &crate::records::rejected_write_key(&*rec),
                    );

                    info!(
                        "Successfully deleted {} from BIND9: {}",
                        T::KIND,
                        rec.name_any()
                    );
                    crate::metrics::record_resource_deleted(T::KIND);
                    Ok(Action::await_change())
                }
            }
        },
    );

    instrumented_action(T::KIND, reconcile)
        .await
        .map_err(|e| finalizer_error(T::KIND, e))
}
