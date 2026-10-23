// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! DNS record reconciliation logic.
//!
//! This module contains reconcilers for all DNS record types supported by Bindy.
//!
//! **Event-Driven Architecture**: DNS record reconcilers react to status changes.

// Submodules
pub mod status_helpers;
pub mod types;

// Internal imports
use crate::record_wrappers::{
    RecordOutcome, REASON_INSTANCE_FILTER_ERROR, REASON_NOT_SELECTED, REASON_NO_PRIMARY_INSTANCES,
    REASON_RECONCILE_FAILED, REASON_RECONCILE_SUCCEEDED, REASON_ZONE_NOT_CONFIGURED,
    REASON_ZONE_NOT_FOUND,
};
use bindy_api::constants::CONDITION_STATUS_TRUE;
use bindy_api::status_reasons::{CONDITION_TYPE_READY, REASON_RRSET_CONFLICT};
use status_helpers::{update_record_status, RecordStatusUpdate};

// The BIND9 write path moved to `bindy-bind9` (ADR-0009 §2, amended
// 2026-10-05), so the zone controller can replay and delete records without
// depending on this crate. Re-exported under the old paths.
pub use bindy_bind9::record_push::{
    add_record_to_instances_generic, delete_record_from_primaries, record_type, ReconcilableRecord,
};

use crate::context::RecordKind;
use crate::crd::DNSZone;
use anyhow::{Context, Result};

use kube::{client::Client, Api, ResourceExt};
use std::future::Future;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// An object from a reflector store, or from the API when the store does not
/// hold it.
///
/// The store is the normal source (ADR-0016): a watch delivers every change,
/// so re-reading an object the store holds costs a GET and tells nothing new.
/// An object the store does not hold yet (a watch that has not caught up)
/// falls back to `fetch`, as `filter_primary_instances_cached` does for
/// instances.
///
/// # Arguments
///
/// * `cached` - The object from the store, if it holds one
/// * `fetch` - The fallback read; `Ok(None)` when the object does not exist
///
/// # Returns
///
/// The object, or `None` when neither the store nor the API has it.
///
/// # Errors
///
/// Returns the fallback's error (an API failure other than not-found).
pub(crate) async fn cached_or_fetched<K, F, Fut>(
    cached: Option<Arc<K>>,
    fetch: F,
) -> Result<Option<K>>
where
    K: Clone,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Option<K>>>,
{
    if let Some(object) = cached {
        return Ok(Some((*object).clone()));
    }
    fetch().await
}

/// The `DNSZone` named by a record's `status.zoneRef`.
///
/// The `DNSZone` controller sets `status.zoneRef` when the zone's
/// `recordsFrom` selector matches the record. The zone is read from the
/// shared `DNSZone` store, with a GET only for a zone the store does not hold
/// yet (ADR-0016).
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `stores` - The shared reflector stores
/// * `zone_ref` - Zone reference from the record status
///
/// # Returns
///
/// The `DNSZone`, or `None` when it does not exist.
///
/// # Errors
///
/// Returns an error if the fallback GET fails for a reason other than
/// not-found.
async fn get_zone_from_ref(
    client: &Client,
    stores: &crate::context::Stores,
    zone_ref: &crate::crd::ZoneReference,
) -> Result<Option<DNSZone>> {
    let cached = stores.get_dnszone(&zone_ref.name, &zone_ref.namespace);
    cached_or_fetched(cached, || async {
        let api: Api<DNSZone> = Api::namespaced(client.clone(), &zone_ref.namespace);
        api.get_opt(&zone_ref.name).await.context(format!(
            "Failed to get DNSZone {}/{}",
            zone_ref.namespace, zone_ref.name
        ))
    })
    .await
}

/// Generic result type for record reconciliation helper.
///
/// Contains all the information needed to add a record to BIND9 primaries.
struct RecordReconciliationContext {
    /// Zone reference from record status
    zone_ref: crate::crd::ZoneReference,
    /// Primary instance references to use for DNS updates
    primary_refs: Vec<crate::crd::InstanceReference>,
    /// Current hash of the record spec
    current_hash: String,
}

/// What preparing a record reconciliation decided.
enum Prepared {
    /// The record is selected and has primaries: write it.
    Write(RecordReconciliationContext),
    /// Stop here; the status already says why.
    Stop(RecordOutcome),
}

/// Generic helper function for record reconciliation.
///
/// This function handles the common logic for all record types:
/// 1. Check if record has status.zoneRef (set by `DNSZone` controller)
/// 2. Look up the `DNSZone` (from the store)
/// 3. Get instances from the zone
/// 4. Filter to primary instances only
/// 5. Return context for adding record to BIND9
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `stores` - The shared reflector stores
/// * `record` - The DNS record resource, as the reconcile received it; its
///   spec is hashed for change detection
///
/// # Returns
///
/// * `Ok(Prepared::Write(context))` - Record is selected and ready to be added to BIND9
/// * `Ok(Prepared::Stop(outcome))` - The record waits on another object, or a
///   lookup failed; its status already says which
///
/// # Errors
///
/// Returns an error if a status update fails.
#[allow(clippy::too_many_lines)]
async fn prepare_record_reconciliation<T: RecordKind>(
    client: &Client,
    stores: &crate::context::Stores,
    record: &T,
) -> Result<Prepared> {
    let record_type = record_type::<T>();
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();
    let current_generation = record.meta().generation;
    let zone_ref = status_helpers::cached_record_status(record).and_then(|s| s.zone_ref);

    // Check if record has zoneRef (set by DNSZone controller). The record's
    // primary stream passes a change of status.zoneRef, so being tagged wakes
    // it: no timer needed (ADR-0016).
    let Some(zone_ref) = zone_ref else {
        info!(
            "{} record {}/{} not selected by any DNSZone (no zoneRef in status)",
            record_type, namespace, name
        );
        update_record_status(
            client,
            record,
            &RecordStatusUpdate::not_ready(
                REASON_NOT_SELECTED,
                "Record not selected by any DNSZone recordsFrom selector",
                current_generation,
            ),
        )
        .await?;
        return Ok(Prepared::Stop(RecordOutcome::Waiting {
            reason: REASON_NOT_SELECTED,
        }));
    };

    // Calculate hash of current spec to detect actual data changes
    let current_hash = crate::ddns::calculate_record_hash(record.spec());

    // Get the DNSZone via zoneRef, from the store (ADR-0016)
    let dnszone = match get_zone_from_ref(client, stores, &zone_ref).await {
        Ok(Some(zone)) => zone,
        Ok(None) => {
            warn!(
                "DNSZone {}/{} for {} record {}/{} does not exist",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name
            );
            update_record_status(
                client,
                record,
                &RecordStatusUpdate::not_ready(
                    REASON_ZONE_NOT_FOUND,
                    &format!(
                        "Referenced DNSZone {}/{} not found",
                        zone_ref.namespace, zone_ref.name
                    ),
                    current_generation,
                ),
            )
            .await?;
            return Ok(Prepared::Stop(RecordOutcome::Waiting {
                reason: REASON_ZONE_NOT_FOUND,
            }));
        }
        Err(e) => {
            warn!(
                "Failed to get DNSZone {}/{} for {} record {}/{}: {}",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name, e
            );
            update_record_status(
                client,
                record,
                &RecordStatusUpdate::not_ready(
                    REASON_ZONE_NOT_FOUND,
                    &format!(
                        "Referenced DNSZone {}/{} could not be read: {e}",
                        zone_ref.namespace, zone_ref.name
                    ),
                    current_generation,
                ),
            )
            .await?;
            return Ok(Prepared::Stop(RecordOutcome::Failed {
                reason: REASON_ZONE_NOT_FOUND,
            }));
        }
    };

    // Get instances from the DNSZone
    let instance_refs =
        match bindy_bind9::instances::get_instances_from_zone(&dnszone, &stores.bind9_instances) {
            Ok(refs) => refs,
            Err(e) => {
                warn!(
                    "DNSZone {}/{} has no instances assigned for {} record {}/{}: {}",
                    zone_ref.namespace, zone_ref.name, record_type, namespace, name, e
                );
                update_record_status(
                    client,
                    record,
                    &RecordStatusUpdate::not_ready(
                        REASON_ZONE_NOT_CONFIGURED,
                        &format!("DNSZone has no instances: {e}"),
                        current_generation,
                    ),
                )
                .await?;
                return Ok(Prepared::Stop(RecordOutcome::Waiting {
                    reason: REASON_ZONE_NOT_CONFIGURED,
                }));
            }
        };

    // Filter to PRIMARY instances only, reading roles from the reflector
    // store rather than one GET per instance (ADR-0015)
    let primary_refs = match bindy_bind9::primary::filter_primary_instances_cached(
        client,
        &stores.bind9_instances,
        &instance_refs,
    )
    .await
    {
        Ok(refs) => refs,
        Err(e) => {
            warn!(
                "Failed to filter primary instances for {} record {}/{}: {}",
                record_type, namespace, name, e
            );
            update_record_status(
                client,
                record,
                &RecordStatusUpdate::not_ready(
                    REASON_INSTANCE_FILTER_ERROR,
                    &format!("Failed to filter primary instances: {e}"),
                    current_generation,
                ),
            )
            .await?;
            return Ok(Prepared::Stop(RecordOutcome::Failed {
                reason: REASON_INSTANCE_FILTER_ERROR,
            }));
        }
    };

    if primary_refs.is_empty() {
        warn!(
            "DNSZone {}/{} has no primary instances for {} record {}/{}",
            zone_ref.namespace, zone_ref.name, record_type, namespace, name
        );
        update_record_status(
            client,
            record,
            &RecordStatusUpdate::not_ready(
                REASON_NO_PRIMARY_INSTANCES,
                "DNSZone has no primary instances configured",
                current_generation,
            ),
        )
        .await?;
        return Ok(Prepared::Stop(RecordOutcome::Waiting {
            reason: REASON_NO_PRIMARY_INSTANCES,
        }));
    }

    Ok(Prepared::Write(RecordReconciliationContext {
        zone_ref,
        primary_refs,
        current_hash,
    }))
}

/// Generic record reconciliation function.
///
/// This function handles reconciliation for all DNS record types that implement
/// the `ReconcilableRecord` trait. It eliminates duplication across 9 record types
/// by providing a single implementation of the reconciliation logic.
///
/// The function:
/// 1. Checks if the record is selected by a `DNSZone` (via status.zoneRef)
/// 2. Looks up the `DNSZone` (from the store) and gets primary instances
/// 3. Deletes the previously published name from BIND9 if `spec.name` changed
///    (rename detection via `status.publishedName`); when another live
///    record of the zone still declares the old name, that record is
///    re-pushed instead (ADR-0022 Decision 5)
/// 4. Adds the record to BIND9 primaries using dynamic DNS updates
/// 5. Updates the record status based on success/failure; `status.addresses`
///    and `status.publishedName` are only set after a successful reconcile
///
/// # Type Parameters
///
/// * `T` - The record type (e.g., `ARecord`, `TXTRecord`) implementing `ReconcilableRecord`
///
/// # Arguments
///
/// * `ctx` - Operator context with Kubernetes client and reflector stores
/// * `record` - The DNS record resource to reconcile
///
/// # Returns
///
/// The [`RecordOutcome`]: published, waiting on another object, or failed
/// (to be retried with backoff). The controller picks its `Action` from it
/// without re-reading the record (ADR-0016).
///
/// # Errors
///
/// Returns an error if a status update fails.
#[allow(clippy::too_many_lines)]
pub(crate) async fn reconcile_record<T>(
    ctx: std::sync::Arc<crate::context::Context>,
    record: T,
) -> Result<RecordOutcome>
where
    T: ReconcilableRecord,
{
    let client = ctx.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    let record_type = record_type::<T>();
    let record_name = record.spec_name();

    debug!("Reconciling {}: {}/{}", T::KIND, namespace, name);

    let current_generation = record.meta().generation;

    // Use generic helper to get zone and instances
    let rec_ctx = match prepare_record_reconciliation(&client, &ctx.stores, &record).await? {
        Prepared::Write(rec_ctx) => rec_ctx,
        Prepared::Stop(outcome) => return Ok(outcome),
    };

    // One resolver for every write this reconcile makes (the rename delete and
    // the add): each primary's RNDC key and endpoints are read once (ADR-0015).
    let resolver = bindy_bind9::instances::InstanceResolver::for_kube(&client, &ctx.stores);

    // ADR-0027: a Scout record whose name another cluster's record has
    // published writes nothing (no rename delete, no add) until it leaves.
    if let Some(outcome) = hold_back_on_conflict(
        &client,
        &ctx.stores,
        &resolver,
        &record,
        &rec_ctx,
        current_generation,
    )
    .await?
    {
        return Ok(outcome);
    }

    // Handle renames: if the record was previously published under a different
    // DNS name (status.publishedName), delete the old FQDN from the zone first.
    // Otherwise the old name would remain orphaned in BIND9 forever.
    if let Some(old_name) = detect_renamed_record(record.record_status(), record_name) {
        info!(
            "{} record {}/{} renamed from '{}' to '{}' - deleting old name from zone {}",
            record_type, namespace, name, old_name, record_name, rec_ctx.zone_ref.zone_name
        );

        // The old name stays when another live record still declares it
        // (ADR-0022 Decision 5): the RRset already holds that record's values.
        let live = ctx.stores.records.get::<T>().state();
        let claimant = rrset_claimant(&record, &rec_ctx.zone_ref, &old_name, &live);
        // This record may have written the RRset last, so the claimant is
        // re-pushed, exactly as the finalizer does.
        let handover = match claimant {
            Some(claimant) => {
                info!(
                    "Keeping {} RRset '{}' in zone {} after renaming {}/{}: still declared by {}/{}, re-pushing it",
                    record_type,
                    old_name,
                    rec_ctx.zone_ref.zone_name,
                    namespace,
                    name,
                    claimant.namespace().unwrap_or_default(),
                    claimant.name_any()
                );
                repush_claimant(
                    &client,
                    &ctx.stores,
                    &resolver,
                    &rec_ctx.primary_refs,
                    &rec_ctx.zone_ref.zone_name,
                    claimant.as_ref(),
                    &old_name,
                )
                .await
            }
            None => {
                delete_record_from_primaries(
                    &client,
                    &ctx.stores,
                    &resolver,
                    &rec_ctx.primary_refs,
                    &rec_ctx.zone_ref.zone_name,
                    &old_name,
                    record_type,
                    true, // fail_on_error: do not publish the new name until the old one is gone
                )
                .await
            }
        };
        if let Err(e) = handover {
            warn!(
                "Failed to hand over renamed {} record '{}' in zone {}: {:#}",
                record_type, old_name, rec_ctx.zone_ref.zone_name, e
            );
            // published_name stays as it is, so the deletion is retried
            update_record_status(
                &client,
                &record,
                &RecordStatusUpdate::not_ready(
                    REASON_RECONCILE_FAILED,
                    &format!("Failed to hand over renamed record '{old_name}' in zone: {e:#}"),
                    current_generation,
                ),
            )
            .await?;
            return Ok(RecordOutcome::WriteRejected);
        }
    }

    // A write BIND9 rejected is re-attempted no sooner than its cooldown. The
    // reconciler is woken by status writes on the owning zone, so without this
    // a permanently rejected record (an MX whose exchange has no address
    // record, say) drives a sustained delete/add storm against named. With no
    // periodic resync the retry is scheduled for when the cooldown ends
    // (ADR-0016). See `REJECTED_WRITE_COOLDOWN`.
    let write_key = rejected_write_key(&record);
    if let Some(remaining) =
        bindy_controller_sdk::retry::write_cooldown_remaining(&write_key, &rec_ctx.current_hash)
    {
        debug!(
            "Skipping {} record {}.{}: the identical spec was rejected less than {:?} ago, retrying in {:?}",
            record_type,
            record_name,
            rec_ctx.zone_ref.zone_name,
            bindy_controller_sdk::retry::REJECTED_WRITE_COOLDOWN,
            remaining
        );
        return Ok(RecordOutcome::CoolingDown { remaining });
    }

    // Add record to BIND9 primaries using generic helper
    match add_record_to_instances_generic(
        &client,
        &ctx.stores,
        &resolver,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &record,
    )
    .await
    {
        Ok(()) => {
            bindy_controller_sdk::retry::clear_rejected_write(&write_key);
            debug!(
                "Successfully added {} record {}.{} via {} primary instance(s)",
                record_type,
                record_name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // DNSZone.status.records[].lastReconciledAt is NOT written from
            // here. The zone controller derives the stamp from this record's
            // status.lastUpdated (set below) on its next reconcile, which this
            // status write triggers (ADR-0015).

            // Update record status to Ready. Addresses (A/AAAA display field) and
            // publishedName are only set after a successful, selected reconcile.
            let message = format!(
                "{} record added to zone {}",
                record_type, rec_ctx.zone_ref.zone_name
            );
            update_record_status(
                &client,
                &record,
                &RecordStatusUpdate {
                    status: CONDITION_STATUS_TRUE,
                    reason: REASON_RECONCILE_SUCCEEDED,
                    message: &message,
                    observed_generation: current_generation,
                    record_hash: Some(rec_ctx.current_hash),
                    last_updated: Some(chrono::Utc::now().to_rfc3339()),
                    addresses: record.display_addresses(),
                    published_name: Some(record_name.to_string()),
                },
            )
            .await?;
            Ok(RecordOutcome::Published)
        }
        Err(e) => {
            bindy_controller_sdk::retry::note_rejected_write(&write_key, &rec_ctx.current_hash);
            warn!(
                "Failed to add {} record {}.{}: {e:#}",
                record_type, record_name, rec_ctx.zone_ref.zone_name
            );
            update_record_status(
                &client,
                &record,
                &RecordStatusUpdate::not_ready(
                    REASON_RECONCILE_FAILED,
                    &format!("Failed to add record to zone: {e:#}"),
                    current_generation,
                ),
            )
            .await?;
            Ok(RecordOutcome::WriteRejected)
        }
    }
}

/// Holds `record` back when another cluster's Scout record has published its
/// name (ADR-0027 Decision 3).
///
/// The first time the conflict is found (the record is not already
/// `RRsetConflict`), the holder is re-pushed, so a name this record
/// overwrote before goes back to the holder's values (the write compares by
/// value first), and one warning is logged. Then the record is marked
/// `Ready=False` with reason `RRsetConflict` naming the holder.
///
/// # Returns
/// `Some(Waiting)` when the record is held back (it is woken when a rival
/// changes, Decision 5); `None` when it may write.
///
/// # Errors
/// Returns an error if the registry cannot be read, the holder cannot be
/// re-pushed (so the next reconcile tries again), or the status write fails.
async fn hold_back_on_conflict<T: ReconcilableRecord>(
    client: &Client,
    stores: &crate::context::Stores,
    resolver: &bindy_bind9::instances::InstanceResolver,
    record: &T,
    rec_ctx: &RecordReconciliationContext,
    current_generation: Option<i64>,
) -> Result<Option<RecordOutcome>> {
    let live = stores.records.get::<T>().state();
    let Some(holder) =
        bindy_controller_sdk::rrset_conflict::held_back_by(client, record, &live).await?
    else {
        return Ok(None);
    };
    let message = bindy_controller_sdk::rrset_conflict::conflict_message(
        holder.as_ref(),
        &rec_ctx.zone_ref.zone_name,
    );
    let already_held_back = record
        .record_status()
        .and_then(|s| {
            s.conditions
                .iter()
                .find(|c| c.r#type == CONDITION_TYPE_READY)
        })
        .is_some_and(|c| c.reason.as_deref() == Some(REASON_RRSET_CONFLICT));
    if !already_held_back {
        warn!(
            "{} {}/{} is held back: {message}",
            T::KIND,
            record.namespace().unwrap_or_default(),
            record.name_any()
        );
        add_record_to_instances_generic(
            client,
            stores,
            resolver,
            &rec_ctx.primary_refs,
            &rec_ctx.zone_ref.zone_name,
            holder.as_ref(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to re-push {} {}/{}, which holds the name",
                T::KIND,
                holder.namespace().unwrap_or_default(),
                holder.name_any()
            )
        })?;
    }
    update_record_status(
        client,
        record,
        &RecordStatusUpdate::not_ready(REASON_RRSET_CONFLICT, &message, current_generation),
    )
    .await?;
    Ok(Some(RecordOutcome::Waiting {
        reason: REASON_RRSET_CONFLICT,
    }))
}

/// The rejection-cooldown key of `record`: `<Kind>/<namespace>/<name>`.
///
/// The one place it is built, for the reconcile that notes a rejected write
/// and the finalizer that clears it (ADR-0020).
///
/// # Arguments
///
/// * `record` - The record whose writes the cooldown tracks
///
/// # Returns
///
/// The key, e.g. `ARecord/team/web`.
#[must_use]
pub(crate) fn rejected_write_key<T: RecordKind>(record: &T) -> String {
    format!(
        "{}/{}/{}",
        T::KIND,
        record.namespace().unwrap_or_default(),
        record.name_any()
    )
}

/// Detects whether a record was renamed since it was last published to BIND9.
///
/// Compares the record's `status.publishedName` (the DNS name most recently
/// written to BIND9) against the current `spec.name`.
///
/// # Arguments
///
/// * `status` - The record's current status, if any
/// * `current_name` - The record name from the current spec
///
/// # Returns
///
/// * `Some(old_name)` - The record was renamed; `old_name` must be deleted from DNS
/// * `None` - No rename occurred (never published, or name unchanged)
pub(crate) fn detect_renamed_record(
    status: Option<&crate::crd::RecordStatus>,
    current_name: &str,
) -> Option<String> {
    let published = status?.published_name.as_deref()?;
    if published == current_name {
        return None;
    }
    Some(published.to_string())
}

/// What one lookup in [`delete_record`] decided about the record's finalizer.
///
/// Deleting a record must not drop its finalizer while the record may still be
/// published in BIND9: a removed finalizer is never retried, so the record
/// would be served forever (bug B2, the failure class of ADR-0015). Only a
/// definite "nothing is published" allows the deletion; an API failure
/// retries it with backoff through the finalizer's error policy.
#[derive(Debug)]
pub(crate) enum DeleteStep<T> {
    /// The lookup succeeded: carry on with its result.
    Proceed(T),
    /// The record is definitely not published here: the finalizer may go.
    AllowDeletion,
    /// The lookup failed: keep the finalizer and retry.
    Retry(anyhow::Error),
}

/// Map the record's `DNSZone` lookup to a deletion step.
///
/// A zone that does not exist allows the deletion: a deleted zone takes all
/// of its data off every server (ADR-0015 amended). A failed lookup (the
/// fallback GET errored) retries, because the zone may still exist and still
/// publish the record.
pub(crate) fn zone_lookup_step<Z>(lookup: Result<Option<Z>>) -> DeleteStep<Z> {
    match lookup {
        Ok(Some(zone)) => DeleteStep::Proceed(zone),
        Ok(None) => DeleteStep::AllowDeletion,
        Err(e) => DeleteStep::Retry(e),
    }
}

/// Map the zone's instance selection to a deletion step.
///
/// `get_instances_from_zone` makes no API call: it evaluates the zone's
/// `spec.bind9InstancesFrom` selectors (and the F-003 namespace gate) against
/// the `Bind9Instance` reflector store. Its `Err` therefore never means "the
/// API is failing"; it means the zone has no selectors or selects no
/// instance, so no server holds the zone and the record is not published.
/// That allows the deletion, as does an empty list.
///
/// The store is never read empty at startup: no controller runs before every
/// store has finished its initial list (`Stores::wait_until_ready`). A retry
/// here would be wrong: on "no instances" it would block deletion forever for
/// a zone whose instances are genuinely gone.
pub(crate) fn instances_step<I>(selection: Result<Vec<I>>) -> DeleteStep<Vec<I>> {
    match selection {
        Ok(refs) if !refs.is_empty() => DeleteStep::Proceed(refs),
        Ok(_) | Err(_) => DeleteStep::AllowDeletion,
    }
}

/// Map the primary-role filter to a deletion step.
///
/// `filter_primary_instances_cached` reads roles from the store and GETs only
/// an instance the store does not hold yet; its `Err` is that GET failing, so
/// the instance may be a primary still holding the record: retry. An empty
/// list (every selected instance is a secondary) allows the deletion, since
/// records are only ever written to primaries.
pub(crate) fn primaries_step<I>(filtered: Result<Vec<I>>) -> DeleteStep<Vec<I>> {
    match filtered {
        Ok(refs) if !refs.is_empty() => DeleteStep::Proceed(refs),
        Ok(_) => DeleteStep::AllowDeletion,
        Err(e) => DeleteStep::Retry(e),
    }
}

/// The live record of kind `T` that still declares the RRset `record` is
/// about to delete, if any.
///
/// Deleting a record from BIND9 removes its whole RRset (zone, name, type).
/// When another live record of the same kind in the same zone declares the
/// same name, the RRset must stay (ADR-0022 Decision 5): since ADR-0021 every
/// write replaces the whole RRset, so it already holds the claimant's values,
/// and with no periodic resync (ADR-0016) nothing would re-push them. The
/// rule itself is [`bindy_controller_sdk::rrset_claim::claims_rrset`], shared
/// with the zone controller's stale-record cleanup.
///
/// # Arguments
///
/// * `record` - The record whose RRset would be deleted
/// * `zone_ref` - The zone the RRset belongs to (the record's `status.zoneRef`)
/// * `dns_name` - The DNS name being deleted: the published name on a
///   finalizer, the old name on a rename
/// * `live` - The records of kind `T` (the kind's reflector store)
///
/// # Returns
///
/// The claimant, or `None` when the RRset may be deleted.
pub(crate) fn rrset_claimant<'a, T: RecordKind>(
    record: &T,
    zone_ref: &crate::crd::ZoneReference,
    dns_name: &str,
    live: &'a [Arc<T>],
) -> Option<&'a Arc<T>> {
    let gone = bindy_controller_sdk::rrset_claim::claim_of(record, zone_ref, dns_name);
    bindy_controller_sdk::rrset_claim::live_claimant(&gone, live)
}

/// Re-pushes `claimant`, the live record that still declares `dns_name`,
/// after another record stopped declaring it (deleted or renamed).
///
/// The departing record may have written the RRset last, so it can still
/// hold that record's values (ADR-0022 Decision 5). The write compares by
/// value first (ADR-0021) and sends nothing when BIND9 already serves the
/// claimant.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `stores` - Reflector stores
/// * `resolver` - Resolves each primary's RNDC key and endpoints
/// * `primary_refs` - The zone's primary instances
/// * `zone_name` - The zone the RRset belongs to
/// * `claimant` - The record that still declares the name
/// * `dns_name` - The name the departing record published
///
/// # Errors
///
/// Returns an error when the re-push fails, so the caller keeps its
/// finalizer (or its `status.publishedName`) and retries.
async fn repush_claimant<T: ReconcilableRecord>(
    client: &Client,
    stores: &crate::context::Stores,
    resolver: &bindy_bind9::instances::InstanceResolver,
    primary_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    claimant: &T,
    dns_name: &str,
) -> Result<()> {
    add_record_to_instances_generic(client, stores, resolver, primary_refs, zone_name, claimant)
        .await
        .with_context(|| {
            format!(
                "Failed to re-push {} record {}/{}, which still declares '{}' in zone {}",
                T::KIND,
                claimant.namespace().unwrap_or_default(),
                claimant.name_any(),
                dns_name,
                zone_name
            )
        })
}

/// Generic function to delete a DNS record from BIND9 primaries.
///
/// This function handles deletion of any record type using the generic approach:
/// 1. Gets the zone reference from the record's status
/// 2. Looks up the `DNSZone` to get instances
/// 3. Filters to primary instances
/// 4. Deletes the record from all primaries (best-effort), using
///    `status.publishedName` when present so renamed records delete the
///    name actually published to DNS, falling back to `spec.name`
/// 5. Skips the BIND9 delete when another live record of the same kind in
///    the zone still declares that name (ADR-0022 Decision 5)
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The DNS record resource being deleted
/// * `stores` - Reflector stores containing `DNSZones` and instances
///
/// # Returns
///
/// Returns `Ok(())` if deletion succeeded, if another live record still
/// declares the RRset, or if the record is definitely not published: no
/// `zoneRef`, the zone does not exist or is being deleted, the zone selects
/// no instances, or none of them is a primary.
///
/// # Errors
///
/// Returns an error, so the finalizer is kept and the deletion retried with
/// backoff, if the `DNSZone` lookup fails, a primary-role GET fails, or a
/// primary that still holds the zone cannot be reached.
///
/// # Panics
///
/// Panics if RNDC key is not found for an instance (should never happen in production).
#[allow(clippy::too_many_lines)]
pub async fn delete_record<T: ReconcilableRecord>(
    client: &Client,
    record: &T,
    stores: &crate::context::Stores,
) -> Result<()> {
    let record_type = record_type::<T>();
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    debug!("Deleting {} record: {}/{}", record_type, namespace, name);

    let status = record.record_status();
    let zone_ref = status.and_then(|s| s.zone_ref.clone());

    // If no zone ref, record was never added to DNS (or already cleaned up)
    let Some(zone_ref) = zone_ref else {
        info!(
            "{} record {}/{} has no zoneRef - was never added to DNS or already cleaned up",
            record_type, namespace, name
        );
        return Ok(());
    };

    // Get the DNSZone (from the store, ADR-0016). A failed lookup keeps the
    // finalizer: the zone may still exist and publish the record (bug B2).
    let dnszone = match zone_lookup_step(get_zone_from_ref(client, stores, &zone_ref).await) {
        DeleteStep::Proceed(zone) => zone,
        DeleteStep::AllowDeletion => {
            warn!(
                "DNSZone {}/{} not found for {} record {}/{}. Allowing deletion.",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name
            );
            return Ok(());
        }
        DeleteStep::Retry(e) => {
            warn!(
                "Failed to look up DNSZone {}/{} for {} record {}/{}: {:#}. Keeping the finalizer; deletion will be retried.",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name, e
            );
            return Err(e.context(format!(
                "deleting {record_type} record {namespace}/{name}: DNSZone {}/{} lookup failed",
                zone_ref.namespace, zone_ref.name
            )));
        }
    };

    // A zone being deleted takes all of its data off every server; its own
    // finalizer is retried until that is confirmed (ADR-0015 amended).
    if dnszone.metadata.deletion_timestamp.is_some() {
        info!(
            "DNSZone {}/{} is being deleted; its deletion removes {} record {}/{} with the zone",
            zone_ref.namespace, zone_ref.name, record_type, namespace, name
        );
        return Ok(());
    }

    // Get instances from DNSZone. This is a store-only selector evaluation, so
    // an error means "no instance selected", never an API failure (see
    // `instances_step`): the zone is on no server and neither is the record.
    let selection =
        bindy_bind9::instances::get_instances_from_zone(&dnszone, &stores.bind9_instances);
    let DeleteStep::Proceed(instance_refs) = instances_step(selection) else {
        warn!(
            "DNSZone {}/{} selects no instances for {} record {}/{}. Allowing deletion.",
            zone_ref.namespace, zone_ref.name, record_type, namespace, name
        );
        return Ok(());
    };

    // Filter to primary instances. Roles come from the reflector store: under
    // API pressure a failed GET here used to drop the instance and leave the
    // record's data on it after the finalizer was removed (ADR-0015). A failed
    // GET for an instance the store does not hold yet keeps the finalizer.
    let filtered = bindy_bind9::primary::filter_primary_instances_cached(
        client,
        &stores.bind9_instances,
        &instance_refs,
    )
    .await;
    let primary_refs = match primaries_step(filtered) {
        DeleteStep::Proceed(refs) => refs,
        DeleteStep::AllowDeletion => {
            warn!(
                "No primary instances found for {} record {}/{}. Allowing deletion.",
                record_type, namespace, name
            );
            return Ok(());
        }
        DeleteStep::Retry(e) => {
            warn!(
                "Failed to filter primary instances for {} record {}/{}: {:#}. Keeping the finalizer; deletion will be retried.",
                record_type, namespace, name, e
            );
            return Err(e.context(format!(
                "deleting {record_type} record {namespace}/{name}: primary instance lookup failed"
            )));
        }
    };

    // Determine the DNS name actually published to BIND9. Prefer
    // status.publishedName (handles renames), then spec.name.
    let record_name_str = status
        .and_then(|s| s.published_name.as_deref())
        .unwrap_or_else(|| record.spec_name());

    let resolver = bindy_bind9::instances::InstanceResolver::for_kube(client, stores);

    // Another live record of this kind in the zone still declares the name:
    // deleting the RRset would take the survivor's data off the air with
    // nothing to re-push it (ADR-0022 Decision 5). The RRset may still hold
    // the deleted record's values if it wrote last, so the survivor is
    // re-pushed instead; its write compares by value first (ADR-0021) and
    // sends nothing when BIND9 already serves it. A failed re-push keeps the
    // finalizer, so the deletion is retried.
    let live = stores.records.get::<T>().state();
    if let Some(claimant) = rrset_claimant(record, &zone_ref, record_name_str, &live) {
        info!(
            "Keeping {} RRset '{}' in zone {} for deleted record {}/{}: still declared by {}/{}, re-pushing it",
            record_type,
            record_name_str,
            zone_ref.zone_name,
            namespace,
            name,
            claimant.namespace().unwrap_or_default(),
            claimant.name_any()
        );
        repush_claimant(
            client,
            stores,
            &resolver,
            &primary_refs,
            &zone_ref.zone_name,
            claimant.as_ref(),
            record_name_str,
        )
        .await?;
        return Ok(());
    }

    // Delete record from all primaries. An instance whose data is gone does
    // not block the finalizer; a pod that still holds the zone but cannot be
    // reached right now (a container restarting) does, and the deletion is
    // retried with backoff (ADR-0015 amended: removing the finalizer then
    // left the record served from the pod's surviving emptyDir).
    delete_record_from_primaries(
        client,
        stores,
        &resolver,
        &primary_refs,
        &zone_ref.zone_name,
        record_name_str,
        record_type,
        false, // fail_on_error: tolerate endpoints whose pod no longer holds the zone
    )
    .await?;

    debug!(
        "Successfully deleted {} record {}/{} from {} primary instance(s)",
        record_type,
        namespace,
        name,
        primary_refs.len()
    );

    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
