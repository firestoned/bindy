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
use status_helpers::{update_record_status, RecordStatusUpdate};

// The BIND9 write path moved to `bindy-bind9` (ADR-0009 §2, amended
// 2026-10-05), so the zone controller can replay and delete records without
// depending on this crate. Re-exported under the old paths.
pub use bindy_bind9::record_push::{
    add_record_to_instances_generic, delete_record_from_primaries, ReconcilableRecord,
    RecordOperation,
};

// Removed ANNOTATION_ZONE_OWNER - using status.zoneRef instead (event-driven architecture)
use crate::crd::DNSZone;
use anyhow::{Context, Result};

use kube::{client::Client, Api, Resource, ResourceExt};
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
/// * `record` - The DNS record resource, as the reconcile received it
/// * `record_type` - Human-readable record type name (e.g., "A", "TXT", "AAAA")
/// * `spec_hashable` - The record spec to hash for change detection
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
async fn prepare_record_reconciliation<T, S>(
    client: &Client,
    stores: &crate::context::Stores,
    record: &T,
    record_type: &str,
    spec_hashable: &S,
) -> Result<Prepared>
where
    T: Resource<DynamicType = (), Scope = k8s_openapi::NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
    S: serde::Serialize,
{
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
    let current_hash = crate::ddns::calculate_record_hash(spec_hashable);

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
///    (rename detection via `status.publishedName`)
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

    debug!(
        "Reconciling {}Record: {}/{}",
        T::record_type_name(),
        namespace,
        name
    );

    let spec = record.get_spec();
    let current_generation = record.meta().generation;

    // Use generic helper to get zone and instances
    let rec_ctx = match prepare_record_reconciliation(
        &client,
        &ctx.stores,
        &record,
        T::record_type_name(),
        spec,
    )
    .await?
    {
        Prepared::Write(rec_ctx) => rec_ctx,
        Prepared::Stop(outcome) => return Ok(outcome),
    };

    // One resolver for every write this reconcile makes (the rename delete and
    // the add): each primary's RNDC key and endpoints are read once (ADR-0015).
    let resolver = bindy_bind9::instances::InstanceResolver::for_kube(&client, &ctx.stores);

    // Handle renames: if the record was previously published under a different
    // DNS name (status.publishedName), delete the old FQDN from the zone first.
    // Otherwise the old name would remain orphaned in BIND9 forever.
    if let Some(old_name) = detect_renamed_record(record.get_status(), T::get_record_name(spec)) {
        info!(
            "{} record {}/{} renamed from '{}' to '{}' - deleting old name from zone {}",
            T::record_type_name(),
            namespace,
            name,
            old_name,
            T::get_record_name(spec),
            rec_ctx.zone_ref.zone_name
        );

        if let Err(e) = delete_record_from_primaries(
            &client,
            &ctx.stores,
            &resolver,
            &rec_ctx.primary_refs,
            &rec_ctx.zone_ref.zone_name,
            &old_name,
            T::record_type_hickory(),
            true, // fail_on_error: do not publish the new name until the old one is gone
        )
        .await
        {
            warn!(
                "Failed to delete renamed {} record '{}' from zone {}: {}",
                T::record_type_name(),
                old_name,
                rec_ctx.zone_ref.zone_name,
                e
            );
            // published_name stays as it is, so the deletion is retried
            update_record_status(
                &client,
                &record,
                &RecordStatusUpdate::not_ready(
                    REASON_RECONCILE_FAILED,
                    &format!("Failed to delete renamed record '{old_name}' from zone: {e}"),
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
    let write_key = format!("{}Record/{namespace}/{name}", T::record_type_name());
    if let Some(remaining) =
        bindy_controller_sdk::retry::write_cooldown_remaining(&write_key, &rec_ctx.current_hash)
    {
        debug!(
            "Skipping {} record {}.{}: the identical spec was rejected less than {:?} ago, retrying in {:?}",
            T::record_type_name(),
            T::get_record_name(spec),
            rec_ctx.zone_ref.zone_name,
            bindy_controller_sdk::retry::REJECTED_WRITE_COOLDOWN,
            remaining
        );
        return Ok(RecordOutcome::CoolingDown { remaining });
    }

    // Create type-specific operation from spec
    let record_op = T::create_operation(spec);

    // Add record to BIND9 primaries using generic helper
    match add_record_to_instances_generic(
        &client,
        &ctx.stores,
        &resolver,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        T::get_record_name(spec),
        T::get_ttl(spec),
        record_op,
    )
    .await
    {
        Ok(()) => {
            bindy_controller_sdk::retry::clear_rejected_write(&write_key);
            debug!(
                "Successfully added {} record {}.{} via {} primary instance(s)",
                T::record_type_name(),
                T::get_record_name(spec),
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
                T::record_type_name(),
                rec_ctx.zone_ref.zone_name
            );
            update_record_status(
                &client,
                &record,
                &RecordStatusUpdate {
                    status: "True",
                    reason: REASON_RECONCILE_SUCCEEDED,
                    message: &message,
                    observed_generation: current_generation,
                    record_hash: Some(rec_ctx.current_hash),
                    last_updated: Some(chrono::Utc::now().to_rfc3339()),
                    addresses: T::get_display_addresses(spec),
                    published_name: Some(T::get_record_name(spec).to_string()),
                },
            )
            .await?;
            Ok(RecordOutcome::Published)
        }
        Err(e) => {
            bindy_controller_sdk::retry::note_rejected_write(&write_key, &rec_ctx.current_hash);
            warn!(
                "Failed to add {} record {}.{}: {e:#}",
                T::record_type_name(),
                T::get_record_name(spec),
                rec_ctx.zone_ref.zone_name
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

/// Generic function to delete a DNS record from BIND9 primaries.
///
/// This function handles deletion of any record type using the generic approach:
/// 1. Gets the zone reference from the record's status
/// 2. Looks up the `DNSZone` to get instances
/// 3. Filters to primary instances
/// 4. Deletes the record from all primaries (best-effort), using
///    `status.publishedName` when present so renamed records delete the
///    name actually published to DNS, falling back to `spec.name`
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The DNS record resource being deleted
/// * `record_type` - Human-readable record type (e.g., "A", "TXT")
/// * `record_type_hickory` - hickory-client `RecordType` enum value
/// * `stores` - Reflector stores containing `DNSZones` and instances
///
/// # Returns
///
/// Returns `Ok(())` if deletion succeeded (or if record didn't exist).
///
/// # Errors
///
/// Returns an error if instance lookup fails or DNS deletion fails critically.
///
/// # Panics
///
/// Panics if RNDC key is not found for an instance (should never happen in production).
#[allow(clippy::too_many_lines)]
pub async fn delete_record<T>(
    client: &Client,
    record: &T,
    record_type: &str,
    record_type_hickory: hickory_proto::rr::RecordType,
    stores: &crate::context::Stores,
) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = k8s_openapi::NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    debug!("Deleting {} record: {}/{}", record_type, namespace, name);

    // Extract status fields generically
    let record_json = serde_json::to_value(record).ok();
    let status = record_json.as_ref().and_then(|v| v.get("status").cloned());

    let zone_ref = status
        .as_ref()
        .and_then(|s| s.get("zoneRef"))
        .cloned()
        .and_then(|z| serde_json::from_value::<crate::crd::ZoneReference>(z).ok());

    // If no zone ref, record was never added to DNS (or already cleaned up)
    let Some(zone_ref) = zone_ref else {
        info!(
            "{} record {}/{} has no zoneRef - was never added to DNS or already cleaned up",
            record_type, namespace, name
        );
        return Ok(());
    };

    // Get the DNSZone (from the store, ADR-0016)
    let dnszone = match get_zone_from_ref(client, stores, &zone_ref).await {
        Ok(Some(zone)) => zone,
        Ok(None) => {
            warn!(
                "DNSZone {}/{} not found for {} record {}/{}. Allowing deletion anyway.",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name
            );
            return Ok(());
        }
        Err(e) => {
            warn!(
                "DNSZone {}/{} not found for {} record {}/{}: {}. Allowing deletion anyway.",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name, e
            );
            return Ok(());
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

    // Get instances from DNSZone
    let instance_refs =
        match bindy_bind9::instances::get_instances_from_zone(&dnszone, &stores.bind9_instances) {
            Ok(refs) => refs,
            Err(e) => {
                warn!(
                "DNSZone {}/{} has no instances for {} record {}/{}: {}. Allowing deletion anyway.",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name, e
            );
                return Ok(());
            }
        };

    // Filter to primary instances. Roles come from the reflector store: under
    // API pressure a failed GET here used to drop the instance and leave the
    // record's data on it after the finalizer was removed (ADR-0015).
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
                    "Failed to filter primary instances for {} record {}/{}: {}. Allowing deletion anyway.",
                    record_type, namespace, name, e
                );
            return Ok(());
        }
    };

    if primary_refs.is_empty() {
        warn!(
            "No primary instances found for {} record {}/{}. Allowing deletion anyway.",
            record_type, namespace, name
        );
        return Ok(());
    }

    // Determine the DNS name actually published to BIND9. Prefer
    // status.publishedName (handles renames), then spec.name, then the
    // resource name as a last resort.
    let record_name_str = status
        .as_ref()
        .and_then(|s| s.get("publishedName"))
        .and_then(|p| p.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            record_json
                .as_ref()
                .and_then(|v| v.get("spec"))
                .and_then(|s| s.get("name"))
                .and_then(|n| n.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| name.clone());

    // Delete record from all primaries. An instance whose data is gone does
    // not block the finalizer; a pod that still holds the zone but cannot be
    // reached right now (a container restarting) does, and the deletion is
    // retried with backoff (ADR-0015 amended: removing the finalizer then
    // left the record served from the pod's surviving emptyDir).
    let resolver = bindy_bind9::instances::InstanceResolver::for_kube(client, stores);
    delete_record_from_primaries(
        client,
        stores,
        &resolver,
        &primary_refs,
        &zone_ref.zone_name,
        &record_name_str,
        record_type_hickory,
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
