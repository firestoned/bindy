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
use status_helpers::update_record_status;

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
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;

use kube::{
    api::{Patch, PatchParams},
    client::Client,
    Api, Resource, ResourceExt,
};
use serde_json::json;
use tracing::{debug, info, warn};

/// Gets the `DNSZone` reference from the record's status.
///
/// The `DNSZone` controller sets `status.zoneRef` when the zone's `recordsFrom` selector
/// matches this record's labels. This field contains the complete Kubernetes object reference.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `zone_ref` - Zone reference from record status
///
/// # Returns
///
/// The `DNSZone` resource
///
/// # Errors
///
/// Returns an error if the `DNSZone` resource cannot be found or queried.
async fn get_zone_from_ref(
    client: &Client,
    zone_ref: &crate::crd::ZoneReference,
) -> Result<DNSZone> {
    let dns_zones_api: Api<DNSZone> = Api::namespaced(client.clone(), &zone_ref.namespace);

    dns_zones_api.get(&zone_ref.name).await.context(format!(
        "Failed to get DNSZone {}/{}",
        zone_ref.namespace, zone_ref.name
    ))
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

/// Generic helper function for record reconciliation.
///
/// This function handles the common logic for all record types:
/// 1. Check if record has status.zoneRef (set by `DNSZone` controller)
/// 2. Look up the `DNSZone` resource
/// 3. Get instances from the zone
/// 4. Filter to primary instances only
/// 5. Return context for adding record to BIND9
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The DNS record resource
/// * `record_type` - Human-readable record type name (e.g., "A", "TXT", "AAAA")
/// * `spec_hashable` - The record spec to hash for change detection
///
/// # Returns
///
/// * `Ok(Some(context))` - Record is selected and ready to be added to BIND9
/// * `Ok(None)` - Record is not selected or generation unchanged (status already updated)
/// * `Err(_)` - Fatal error occurred
///
/// # Errors
///
/// Returns an error if status updates fail or critical Kubernetes API errors occur.
#[allow(clippy::too_many_lines)]
async fn prepare_record_reconciliation<T, S>(
    client: &Client,
    record: &T,
    record_type: &str,
    spec_hashable: &S,
    bind9_instances_store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
) -> Result<Option<RecordReconciliationContext>>
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

    // Extract status fields generically
    let record_json = serde_json::to_value(record)?;
    let status = record_json.get("status");

    let zone_ref = status
        .and_then(|s| s.get("zoneRef"))
        .and_then(|z| serde_json::from_value::<crate::crd::ZoneReference>(z.clone()).ok());

    let observed_generation = status
        .and_then(|s| s.get("observedGeneration"))
        .and_then(serde_json::Value::as_i64);

    let current_generation = record.meta().generation;

    // Check if record has zoneRef (set by DNSZone controller)
    let Some(zone_ref) = zone_ref else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !bindy_controller_sdk::status::should_reconcile(current_generation, observed_generation)
        {
            debug!("Spec unchanged and no zoneRef, skipping reconciliation");
            return Ok(None);
        }

        info!(
            "{} record {}/{} not selected by any DNSZone (no zoneRef in status)",
            record_type, namespace, name
        );
        update_record_status(
            client,
            record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone recordsFrom selector",
            current_generation,
            None, // record_hash
            None, // last_updated
            None, // addresses
            None, // published_name
        )
        .await?;
        return Ok(None);
    };

    // Calculate hash of current spec to detect actual data changes
    let current_hash = crate::ddns::calculate_record_hash(spec_hashable);

    // Get the DNSZone resource via zoneRef
    let dnszone = match get_zone_from_ref(client, &zone_ref).await {
        Ok(zone) => zone,
        Err(e) => {
            warn!(
                "Failed to get DNSZone {}/{} for {} record {}/{}: {}",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name, e
            );
            update_record_status(
                client,
                record,
                "Ready",
                "False",
                "ZoneNotFound",
                &format!(
                    "Referenced DNSZone {}/{} not found: {e}",
                    zone_ref.namespace, zone_ref.name
                ),
                current_generation,
                None, // record_hash
                None, // last_updated
                None, // addresses
                None, // published_name
            )
            .await?;
            return Ok(None);
        }
    };

    // Get instances from the DNSZone
    let instance_refs =
        match bindy_bind9::instances::get_instances_from_zone(&dnszone, bind9_instances_store) {
            Ok(refs) => refs,
            Err(e) => {
                warn!(
                    "DNSZone {}/{} has no instances assigned for {} record {}/{}: {}",
                    zone_ref.namespace, zone_ref.name, record_type, namespace, name, e
                );
                update_record_status(
                    client,
                    record,
                    "Ready",
                    "False",
                    "ZoneNotConfigured",
                    &format!("DNSZone has no instances: {e}"),
                    current_generation,
                    None, // record_hash
                    None, // last_updated
                    None, // addresses
                    None, // published_name
                )
                .await?;
                return Ok(None);
            }
        };

    // Filter to PRIMARY instances only
    let primary_refs =
        match bindy_bind9::primary::filter_primary_instances(client, &instance_refs).await {
            Ok(refs) => refs,
            Err(e) => {
                warn!(
                    "Failed to filter primary instances for {} record {}/{}: {}",
                    record_type, namespace, name, e
                );
                update_record_status(
                    client,
                    record,
                    "Ready",
                    "False",
                    "InstanceFilterError",
                    &format!("Failed to filter primary instances: {e}"),
                    current_generation,
                    None, // record_hash
                    None, // last_updated
                    None, // addresses
                    None, // published_name
                )
                .await?;
                return Ok(None);
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
            "Ready",
            "False",
            "NoPrimaryInstances",
            "DNSZone has no primary instances configured",
            current_generation,
            None, // record_hash
            None, // last_updated
            None, // addresses
            None, // published_name
        )
        .await?;
        return Ok(None);
    }

    Ok(Some(RecordReconciliationContext {
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
/// 2. Looks up the `DNSZone` and gets primary instances
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
/// * `Ok(())` - If reconciliation succeeded or record is not selected
/// * `Err(_)` - If a fatal error occurred
///
/// # Errors
///
/// Returns an error if status updates fail or BIND9 record creation fails.
pub(crate) async fn reconcile_record<T>(
    ctx: std::sync::Arc<crate::context::Context>,
    record: T,
) -> Result<()>
where
    T: ReconcilableRecord,
{
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!(
        "Reconciling {}Record: {}/{}",
        T::record_type_name(),
        namespace,
        name
    );

    let spec = record.get_spec();
    let current_generation = record.meta().generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) = prepare_record_reconciliation(
        &client,
        &record,
        T::record_type_name(),
        spec,
        bind9_instances_store,
    )
    .await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

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
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to delete renamed record '{old_name}' from zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
                None, // addresses
                None, // published_name (preserve old name so deletion is retried)
            )
            .await?;
            return Ok(());
        }
    }

    // A write BIND9 rejected is re-attempted on this record's own timed requeue
    // and on nothing else. The reconciler is woken by every status patch on the
    // owning zone and on each primary instance, so without this a permanently
    // rejected record (an MX whose exchange has no address record, say) drives a
    // sustained delete/add storm against named. See `REJECTED_WRITE_COOLDOWN`.
    let write_key = format!("{}Record/{namespace}/{name}", T::record_type_name());
    if bindy_controller_sdk::retry::write_in_cooldown(&write_key, &rec_ctx.current_hash) {
        debug!(
            "Skipping {} record {}.{}: the identical spec was rejected less than {:?} ago",
            T::record_type_name(),
            T::get_record_name(spec),
            rec_ctx.zone_ref.zone_name,
            bindy_controller_sdk::retry::REJECTED_WRITE_COOLDOWN
        );
        return Ok(());
    }

    // Create type-specific operation from spec
    let record_op = T::create_operation(spec);

    // Add record to BIND9 primaries using generic helper
    match add_record_to_instances_generic(
        &client,
        &ctx.stores,
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
            info!(
                "Successfully added {} record {}.{} via {} primary instance(s)",
                T::record_type_name(),
                T::get_record_name(spec),
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.records[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                &format!("{}Record", T::record_type_name()),
                &name,
                &namespace,
            )
            .await?;

            // Update record status to Ready. Addresses (A/AAAA display field) and
            // publishedName are only set after a successful, selected reconcile.
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!(
                    "{} record added to zone {}",
                    T::record_type_name(),
                    rec_ctx.zone_ref.zone_name
                ),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
                T::get_display_addresses(spec),
                Some(T::get_record_name(spec).to_string()),
            )
            .await?;
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
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e:#}"),
                current_generation,
                None, // record_hash
                None, // last_updated
                None, // addresses
                None, // published_name
            )
            .await?;
        }
    }

    Ok(())
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

    info!("Deleting {} record: {}/{}", record_type, namespace, name);

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

    // Get the DNSZone
    let dnszone = match get_zone_from_ref(client, &zone_ref).await {
        Ok(zone) => zone,
        Err(e) => {
            warn!(
                "DNSZone {}/{} not found for {} record {}/{}: {}. Allowing deletion anyway.",
                zone_ref.namespace, zone_ref.name, record_type, namespace, name, e
            );
            return Ok(());
        }
    };

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

    // Filter to primary instances
    let primary_refs = match bindy_bind9::primary::filter_primary_instances(client, &instance_refs)
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

    // Delete record from all primaries (best-effort: finalizer removal must
    // not be blocked by unreachable endpoints)
    delete_record_from_primaries(
        client,
        stores,
        &primary_refs,
        &zone_ref.zone_name,
        &record_name_str,
        record_type_hickory,
        false, // fail_on_error: allow Kubernetes deletion to proceed
    )
    .await?;

    info!(
        "Successfully deleted {} record {}/{} from {} primary instance(s)",
        record_type,
        namespace,
        name,
        primary_refs.len()
    );

    Ok(())
}

/// Builds the merge patch that updates `DNSZone.status.records[]`.
///
/// The `DNSZoneStatus` field is named `records` on the wire (camelCase of
/// `pub records`). Using any other key (e.g., the old `selectedRecords`) is
/// silently pruned by the CRD structural schema, so timestamps never persist.
#[must_use]
pub(crate) fn build_records_timestamp_patch(
    records: &[crate::crd::RecordReferenceWithTimestamp],
) -> serde_json::Value {
    json!({
        "status": {
            "records": records
        }
    })
}

/// Update lastReconciledAt timestamp for a record in `DNSZone.status.records[]`.
///
/// This signals that the record has been successfully configured in BIND9.
/// Future reconciliations will skip this record until the timestamp is reset.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `zone_namespace` - Namespace of the `DNSZone`
/// * `zone_name` - Name of the `DNSZone`
/// * `record_kind` - Kind of the record (e.g., "`ARecord`", "`CNAMERecord`")
/// * `record_name` - Name of the record resource
/// * `record_namespace` - Namespace of the record resource
///
/// # Errors
///
/// Returns an error if:
/// - `DNSZone` cannot be fetched from Kubernetes API
/// - Status patch operation fails
pub async fn update_record_reconciled_timestamp(
    client: &Client,
    zone_namespace: &str,
    zone_name: &str,
    record_kind: &str,
    record_name: &str,
    record_namespace: &str,
) -> Result<()> {
    let api: Api<DNSZone> = Api::namespaced(client.clone(), zone_namespace);

    // Re-fetch zone to get latest status
    let mut zone = api.get(zone_name).await?;

    // Find the record reference and update its timestamp
    let mut found = false;
    if let Some(status) = &mut zone.status {
        for record_ref in &mut status.records {
            if record_ref.kind == record_kind
                && record_ref.name == record_name
                && record_ref.namespace == record_namespace
            {
                record_ref.last_reconciled_at = Some(Time(k8s_openapi::jiff::Timestamp::now()));
                found = true;
                break;
            }
        }
    }

    if !found {
        warn!(
            "Record {} {}/{} not found in DNSZone {}/{} status.records[] - cannot update timestamp",
            record_kind, record_namespace, record_name, zone_namespace, zone_name
        );
        return Ok(());
    }

    // Patch the status with updated timestamp (key MUST be `records` - see
    // build_records_timestamp_patch)
    let status_patch = zone
        .status
        .as_ref()
        .map(|s| build_records_timestamp_patch(&s.records))
        .unwrap_or_else(|| build_records_timestamp_patch(&[]));

    api.patch_status(
        zone_name,
        &PatchParams::default(),
        &Patch::Merge(status_patch),
    )
    .await?;

    info!(
        "Updated lastReconciledAt for {} record {}/{} in zone {}/{}",
        record_kind, record_namespace, record_name, zone_namespace, zone_name
    );

    Ok(())
}
