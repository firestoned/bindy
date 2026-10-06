// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Cleanup operations for DNS zones.
//!
//! This module handles cleanup of deleted instances and stale records from zone status.

use anyhow::Result;
use kube::{Api, Client};
use tracing::{debug, info, warn};

use super::helpers::HTTP_STATUS_NOT_FOUND;
use crate::crd::DNSZone;

/// Converts a Kubernetes `get` result into an existence check.
///
/// CRITICAL: Only a 404 (`NotFound`) response means the resource is deleted.
/// Any other error (timeout, 429, 5xx, auth failure, ...) is potentially
/// transient and MUST be propagated - treating it as "deleted" would trigger
/// the self-healing cleanup path and delete live DNS data for a resource that
/// still exists.
///
/// # Arguments
///
/// * `result` - The result of an `Api::get` call
///
/// # Returns
///
/// * `Ok(true)` - The resource exists
/// * `Ok(false)` - The API returned 404: the resource is deleted
///
/// # Errors
///
/// Returns the original error for any non-404 failure so the caller aborts
/// this cleanup pass and retries on the next reconciliation.
pub(super) fn existence_from_get_result<K>(
    result: std::result::Result<K, kube::Error>,
) -> Result<bool> {
    match result {
        Ok(_) => Ok(true),
        Err(kube::Error::Api(ae)) if ae.code == HTTP_STATUS_NOT_FOUND => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Checks whether a namespaced resource exists, distinguishing 404 from
/// transient API errors (see [`existence_from_get_result`]).
///
/// # Errors
///
/// Returns an error for any non-404 API failure.
async fn resource_exists<K>(api: &Api<K>, name: &str) -> Result<bool>
where
    K: kube::Resource + Clone + std::fmt::Debug + serde::de::DeserializeOwned,
{
    existence_from_get_result(api.get(name).await)
}

/// Clean up deleted instances from zone status.
///
/// Iterates through instances in zone status and removes any that no longer exist
/// in the Kubernetes API.
///
/// # Arguments
///
/// * `client` - Kubernetes client
/// * `dnszone` - The DNSZone resource being reconciled
/// * `status_updater` - Status updater for modifying zone status
///
/// # Returns
///
/// Number of instances removed from status
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail critically.
pub async fn cleanup_deleted_instances(
    client: &Client,
    dnszone: &DNSZone,
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
) -> Result<usize> {
    use crate::crd::Bind9Instance;
    use kube::{Api, ResourceExt};

    let namespace = dnszone.namespace().unwrap_or_default();
    let zone_name = &dnszone.spec.zone_name;

    // Get current instances from status
    let current_instances = dnszone
        .status
        .as_ref()
        .map(|s| s.bind9_instances.clone())
        .unwrap_or_default();

    if current_instances.is_empty() {
        debug!(
            "No instances in status for zone {}/{} - skipping cleanup",
            namespace, zone_name
        );
        return Ok(0);
    }

    info!(
        "Cleaning up deleted instances for zone {}/{}: checking {} instance(s)",
        namespace,
        zone_name,
        current_instances.len()
    );

    let mut deleted_count = 0;

    // Check each instance to see if it still exists.
    // Only a 404 means "deleted": transient API errors abort this cleanup
    // pass (via `?`) so a live instance is never removed from status by mistake.
    for instance_ref in current_instances {
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        let instance_exists = resource_exists(&instance_api, &instance_ref.name).await?;

        if !instance_exists {
            info!(
                "Instance {}/{} no longer exists - removing from zone {}/{}",
                instance_ref.namespace, instance_ref.name, namespace, zone_name
            );
            status_updater.remove_instance(&instance_ref.name, &instance_ref.namespace);
            deleted_count += 1;
        }
    }

    Ok(deleted_count)
}

/// Names of the record resources that exist, keyed by `(kind, namespace)`.
pub(super) type ListedRecords =
    std::collections::HashMap<(String, String), std::collections::HashSet<String>>;

/// Whether a record of `kind` named `name` in `namespace` was listed.
#[must_use]
pub(super) fn record_listed(
    existing: &ListedRecords,
    kind: &str,
    namespace: &str,
    name: &str,
) -> bool {
    existing
        .get(&(kind.to_string(), namespace.to_string()))
        .is_some_and(|names| names.contains(name))
}

/// Names of every resource of `T` in `namespace`.
///
/// # Errors
///
/// Returns an error if the LIST fails.
async fn list_names<T>(
    client: &Client,
    namespace: &str,
) -> Result<std::collections::HashSet<String>>
where
    T: kube::Resource<DynamicType = (), Scope = k8s_openapi::NamespaceResourceScope>
        + Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned,
{
    use kube::ResourceExt;
    let api: Api<T> = Api::namespaced(client.clone(), namespace);
    let items = bindy_controller_sdk::pagination::list_all_paginated(
        &api,
        kube::api::ListParams::default(),
    )
    .await?;
    Ok(items.iter().map(ResourceExt::name_any).collect())
}

/// Names of every record of `kind` in `namespace`, by one LIST.
///
/// # Errors
///
/// Returns an error if the kind is unknown or the LIST fails. A failed LIST
/// must abort the stale-record pass: reading it as "nothing exists" would
/// trigger the self-healing path and delete live DNS data.
async fn existing_record_names(
    client: &Client,
    kind: &str,
    namespace: &str,
) -> Result<std::collections::HashSet<String>> {
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, DNSRecordKind, MXRecord, NSRecord, PTRRecord,
        SRVRecord, TXTRecord,
    };

    match DNSRecordKind::try_from(kind)? {
        DNSRecordKind::A => list_names::<ARecord>(client, namespace).await,
        DNSRecordKind::AAAA => list_names::<AAAARecord>(client, namespace).await,
        DNSRecordKind::TXT => list_names::<TXTRecord>(client, namespace).await,
        DNSRecordKind::CNAME => list_names::<CNAMERecord>(client, namespace).await,
        DNSRecordKind::MX => list_names::<MXRecord>(client, namespace).await,
        DNSRecordKind::NS => list_names::<NSRecord>(client, namespace).await,
        DNSRecordKind::SRV => list_names::<SRVRecord>(client, namespace).await,
        DNSRecordKind::CAA => list_names::<CAARecord>(client, namespace).await,
        DNSRecordKind::PTR => list_names::<PTRRecord>(client, namespace).await,
    }
}

/// What [`cleanup_stale_records`] did.
#[derive(Debug, Default)]
pub struct StaleRecordCleanup {
    /// Deleted records dropped from `status.records`
    pub removed: usize,
    /// Deleted records whose DNS data could not be confirmed gone on every
    /// primary endpoint. They stay in `status.records`, so the next
    /// reconciliation retries the cleanup instead of forgetting the data.
    pub retained: Vec<crate::crd::RecordReferenceWithTimestamp>,
}

/// Clean up stale records from zone status.
///
/// Iterates through records in zone status and removes any that no longer exist
/// in the Kubernetes API. Also performs self-healing by deleting orphaned records
/// from BIND9 if they were missed by finalizers, unless another record the
/// zone selects still declares the same name and type (a renamed record):
/// that data is kept.
///
/// Existence is read with one LIST per record kind and namespace present in
/// the status, and every self-healing write shares one
/// [`bindy_bind9::instances::InstanceResolver`], so the API cost of this pass
/// does not grow with the number of records (ADR-0015).
///
/// A deleted record is dropped from status only once its data is confirmed
/// gone from every primary endpoint. Before, a failed lookup or DNS query was
/// ignored and the reference dropped anyway, so a record whose finalizer had
/// also failed stayed served with nothing left to clean it up.
///
/// # Arguments
///
/// * `client` - Kubernetes client
/// * `dnszone` - The DNSZone resource being reconciled
/// * `status_updater` - Status updater for modifying zone status
/// * `stores` - The shared reflector stores
///
/// # Returns
///
/// How many records were removed, and which were retained for a retry
///
/// # Errors
///
/// Returns an error if API calls fail critically (non-NotFound errors).
#[allow(clippy::too_many_lines)]
pub async fn cleanup_stale_records(
    client: &Client,
    dnszone: &DNSZone,
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    stores: &crate::context::Stores,
) -> Result<StaleRecordCleanup> {
    use crate::bind9::records::query_dns_record;
    use crate::crd::{DNSRecordKind, RecordReferenceWithTimestamp};
    use kube::ResourceExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let namespace = dnszone.namespace().unwrap_or_default();
    let zone_name = &dnszone.spec.zone_name;

    // Get current records from status
    let current_records = dnszone
        .status
        .as_ref()
        .map(|s| s.records.clone())
        .unwrap_or_default();

    let mut outcome = StaleRecordCleanup::default();

    if current_records.is_empty() {
        debug!(
            "No records in status for zone {}/{} - skipping cleanup",
            namespace, zone_name
        );
        return Ok(outcome);
    }

    debug!(
        "Cleaning up stale records for zone {}/{}: checking {} record(s)",
        namespace,
        zone_name,
        current_records.len()
    );

    // One LIST per (kind, namespace) present. Only a successful LIST counts:
    // an error aborts the pass (`?`), never reads as "deleted".
    let mut existing: ListedRecords = ListedRecords::new();
    for record_ref in &current_records {
        let key = (record_ref.kind.clone(), record_ref.namespace.clone());
        if existing.contains_key(&key) {
            continue;
        }
        let names = existing_record_names(client, &record_ref.kind, &record_ref.namespace).await?;
        existing.insert(key, names);
    }

    // Get instances to query DNS and delete if needed
    let instance_refs =
        super::validation::get_instances_from_zone(dnszone, &stores.bind9_instances)?;
    let primary_refs = bindy_bind9::primary::filter_primary_instances_cached(
        client,
        &stores.bind9_instances,
        &instance_refs,
    )
    .await?;
    let resolver = bindy_bind9::instances::InstanceResolver::for_kube(client, stores);

    let mut records_to_keep: Vec<RecordReferenceWithTimestamp> = Vec::new();

    // The records the zone selects right now, listed the first time a deleted
    // one turns up. Status alone is not enough: this cleanup runs before
    // discovery, so a record created moments ago is not in status yet.
    let mut live_records: Option<Vec<RecordReferenceWithTimestamp>> = None;

    for record_ref in current_records {
        if record_listed(
            &existing,
            &record_ref.kind,
            &record_ref.namespace,
            &record_ref.name,
        ) {
            // Record still exists in Kubernetes - keep it in status
            // The record reconciler will handle updating BIND9
            records_to_keep.push(record_ref);
            continue;
        }

        // Record doesn't exist in Kubernetes - need to clean up
        info!(
            "Record {} {}/{} no longer exists in Kubernetes",
            record_ref.kind, record_ref.namespace, record_ref.name
        );

        // Another live record declares the same RRset (e.g. this record was
        // renamed): drop the stale reference but keep the DNS data. A
        // listing error aborts the pass (`?`) rather than risk deleting
        // data that is still wanted.
        if live_records.is_none() {
            live_records =
                Some(super::discovery::discover_selected_records(client, dnszone).await?);
        }
        if let Some(claimant) = live_records
            .as_deref()
            .and_then(|live| super::discovery::claimed_by_live_record(&record_ref, live))
        {
            info!(
                "Keeping DNS data of deleted {} {}/{}: still declared by {} {}/{}",
                record_ref.kind,
                record_ref.namespace,
                record_ref.name,
                claimant.kind,
                claimant.namespace,
                claimant.name
            );
            outcome.removed += 1;
            continue;
        }

        // Self-healing: Check if record still exists in BIND9 and delete if found
        // This catches cases where the finalizer failed to delete
        let kind = DNSRecordKind::try_from(record_ref.kind.as_str())?;
        let record_type = kind.to_hickory_record_type();

        // Extract DNS record name and zone from RecordReference
        // These fields are populated from spec.name when the record is discovered
        let Some(dns_record_name) = record_ref.record_name.clone() else {
            warn!(
                "Record {} {}/{} has no recordName in status - skipping BIND9 cleanup",
                record_ref.kind, record_ref.namespace, record_ref.name
            );
            outcome.removed += 1;
            continue;
        };

        // Cleared by any endpoint where the data could not be confirmed gone
        let verified = Arc::new(AtomicBool::new(true));

        // Query and potentially delete from each primary instance
        let lookup = super::helpers::for_each_instance_endpoint(
            &resolver,
            &primary_refs,
            true,      // with_rndc_key (needed for deletion)
            "dns-tcp", // Use DNS TCP port for queries and updates
            |pod_endpoint, _instance_name, rndc_key| {
                let server = pod_endpoint.clone();
                let zone = zone_name.clone();
                let dns_name = dns_record_name.clone();
                let r_kind = record_ref.kind.clone();
                let r_namespace = record_ref.namespace.clone();
                let r_name = record_ref.name.clone();
                let verified = Arc::clone(&verified);

                async move {
                    // Query DNS to check if record exists
                    match query_dns_record(&zone, &dns_name, record_type, &server).await {
                        Ok(records) if !records.is_empty() => {
                            warn!(
                                "SELF-HEALING: Record {} {}/{} deleted from K8s but still exists in BIND9 on {}",
                                r_kind, r_namespace, r_name, server
                            );

                            let Some(key_data) = rndc_key else {
                                warn!(
                                    "No RNDC key available for {} - cannot delete orphaned record",
                                    server
                                );
                                verified.store(false, Ordering::SeqCst);
                                return Ok(());
                            };
                            match crate::bind9::records::delete_dns_record(
                                &zone,
                                &dns_name,
                                record_type,
                                &server,
                                &key_data,
                            )
                            .await
                            {
                                Ok(()) => {
                                    info!(
                                        "SELF-HEALING: Successfully deleted orphaned {} record {} from BIND9 on {}",
                                        r_kind, dns_name, server
                                    );
                                }
                                Err(e) => {
                                    warn!(
                                        "SELF-HEALING: Failed to delete orphaned record from BIND9 on {}: {}",
                                        server, e
                                    );
                                    verified.store(false, Ordering::SeqCst);
                                }
                            }
                        }
                        Ok(_) => {
                            // Record doesn't exist in BIND9 - good, finalizer worked
                            debug!(
                                "Record {} not found in BIND9 on {} - already cleaned up",
                                dns_name, server
                            );
                        }
                        Err(e) => {
                            warn!(
                                "SELF-HEALING: could not query {} on {} ({}); will retry",
                                dns_name, server, e
                            );
                            verified.store(false, Ordering::SeqCst);
                        }
                    }

                    Ok(())
                }
            },
        )
        .await;

        if let Err(e) = &lookup {
            warn!(
                "SELF-HEALING: could not reach every primary for deleted {} {}/{} ({e:#}); will retry",
                record_ref.kind, record_ref.namespace, record_ref.name
            );
        }

        if lookup.is_ok() && verified.load(Ordering::SeqCst) {
            outcome.removed += 1;
            continue;
        }

        // Not confirmed gone everywhere: keep tracking it so the next
        // reconciliation retries instead of orphaning the data in BIND9.
        records_to_keep.push(record_ref.clone());
        outcome.retained.push(record_ref);
    }

    // Update status with cleaned records list
    if outcome.removed > 0 {
        status_updater.set_records(&records_to_keep);
        info!(
            "Removed {} stale record(s) from zone {}/{} status",
            outcome.removed, namespace, zone_name
        );
    }

    Ok(outcome)
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod cleanup_tests;
