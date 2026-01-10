// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Cleanup operations for DNS zones.
//!
//! This module handles cleanup of deleted instances and stale records from zone status.

use anyhow::Result;
use kube::Client;
use tracing::{debug, info, warn};

use crate::crd::DNSZone;

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
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
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

    // Check each instance to see if it still exists
    for instance_ref in current_instances {
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        let instance_exists = instance_api.get(&instance_ref.name).await.is_ok();

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

/// Clean up stale records from zone status.
///
/// Iterates through records in zone status and removes any that no longer exist
/// in the Kubernetes API. Also performs self-healing by deleting orphaned records
/// from BIND9 if they were missed by finalizers.
///
/// # Arguments
///
/// * `client` - Kubernetes client
/// * `dnszone` - The DNSZone resource being reconciled
/// * `status_updater` - Status updater for modifying zone status
/// * `bind9_instances_store` - Reflector store for querying Bind9Instance resources
///
/// # Returns
///
/// Number of records removed from status
///
/// # Errors
///
/// Returns an error if API calls fail critically (non-NotFound errors).
#[allow(clippy::too_many_lines)]
pub async fn cleanup_stale_records(
    client: &Client,
    dnszone: &DNSZone,
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
    bind9_instances_store: &kube::runtime::reflector::Store<crate::crd::Bind9Instance>,
) -> Result<usize> {
    use crate::bind9::records::query_dns_record;
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, DNSRecordKind, MXRecord, NSRecord,
        RecordReferenceWithTimestamp, SRVRecord, TXTRecord,
    };
    use kube::{Api, ResourceExt};

    let namespace = dnszone.namespace().unwrap_or_default();
    let zone_name = &dnszone.spec.zone_name;

    // Get current records from status
    let current_records = dnszone
        .status
        .as_ref()
        .map(|s| s.records.clone())
        .unwrap_or_default();

    if current_records.is_empty() {
        debug!(
            "No records in status for zone {}/{} - skipping cleanup",
            namespace, zone_name
        );
        return Ok(0);
    }

    info!(
        "Cleaning up stale records for zone {}/{}: checking {} record(s)",
        namespace,
        zone_name,
        current_records.len()
    );

    // Get instances to query DNS and delete if needed
    let instance_refs = super::validation::get_instances_from_zone(dnszone, bind9_instances_store)?;
    let primary_refs = super::primary::filter_primary_instances(client, &instance_refs).await?;

    let mut records_to_keep: Vec<RecordReferenceWithTimestamp> = Vec::new();
    let mut stale_count = 0;

    // Check each record to see if it still exists
    for record_ref in current_records {
        let kind = DNSRecordKind::from(record_ref.kind.as_str());
        let record_exists = match kind {
            DNSRecordKind::A => {
                let api: Api<ARecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::AAAA => {
                let api: Api<AAAARecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::TXT => {
                let api: Api<TXTRecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::CNAME => {
                let api: Api<CNAMERecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::MX => {
                let api: Api<MXRecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::NS => {
                let api: Api<NSRecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::SRV => {
                let api: Api<SRVRecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::CAA => {
                let api: Api<CAARecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
        };

        if record_exists {
            // Record still exists in Kubernetes - keep it in status
            // The record reconciler will handle updating BIND9
            debug!(
                "Record {} {}/{} still exists - keeping in status",
                record_ref.kind, record_ref.namespace, record_ref.name
            );
            records_to_keep.push(record_ref);
        } else {
            // Record doesn't exist in Kubernetes - need to clean up
            info!(
                "Record {} {}/{} no longer exists in Kubernetes",
                record_ref.kind, record_ref.namespace, record_ref.name
            );

            // Self-healing: Check if record still exists in BIND9 and delete if found
            // This catches cases where the finalizer failed to delete
            let kind = DNSRecordKind::from(record_ref.kind.as_str());
            let record_type = kind.to_hickory_record_type();

            // Extract DNS record name and zone from RecordReference
            // These fields are populated from spec.name when the record is discovered
            let dns_record_name = if let Some(name) = &record_ref.record_name {
                name.as_str()
            } else {
                warn!(
                    "Record {} {}/{} has no recordName in status - skipping BIND9 cleanup",
                    record_ref.kind, record_ref.namespace, record_ref.name
                );
                stale_count += 1;
                continue;
            };

            // Check BIND9 on all primary instances and delete if found
            // Use for_each_instance_endpoint to iterate over all primary endpoints
            let dns_record_name_clone = dns_record_name.to_string();
            let dns_zone_name_clone = zone_name.clone();
            let record_kind = record_ref.kind.clone();
            let record_namespace = record_ref.namespace.clone();
            let record_name = record_ref.name.clone();

            // Query and potentially delete from each primary instance
            let _ = super::helpers::for_each_instance_endpoint(
                client,
                &primary_refs,
                true,      // with_rndc_key (needed for deletion)
                "dns-tcp", // Use DNS TCP port for queries and updates
                |pod_endpoint, _instance_name, rndc_key| {
                    let server = pod_endpoint.clone();
                    let zone = dns_zone_name_clone.clone();
                    let dns_name = dns_record_name_clone.clone();
                    let r_type = record_type;
                    let r_kind = record_kind.clone();
                    let r_namespace = record_namespace.clone();
                    let r_name = record_name.clone();

                    async move {
                        // Query DNS to check if record exists
                        match query_dns_record(&zone, &dns_name, r_type, &server).await {
                            Ok(records) if !records.is_empty() => {
                                warn!(
                                    "SELF-HEALING: Record {} {}/{} deleted from K8s but still exists in BIND9 on {}",
                                    r_kind, r_namespace, r_name, server
                                );

                                // Delete from BIND9 using the RNDC key
                                if let Some(key_data) = rndc_key {
                                    match crate::bind9::records::delete_dns_record(
                                        &zone,
                                        &dns_name,
                                        r_type,
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
                                        }
                                    }
                                } else {
                                    warn!(
                                        "No RNDC key available for {} - cannot delete orphaned record",
                                        server
                                    );
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
                                debug!(
                                    "Failed to query DNS on {} for {} (may not exist): {}",
                                    server, dns_name, e
                                );
                            }
                        }

                        Ok(())
                    }
                },
            )
            .await;

            // Remove from status regardless of whether we found it in BIND9
            stale_count += 1;
        }
    }

    // Update status with cleaned records list
    if stale_count > 0 {
        status_updater.set_records(&records_to_keep);
        info!(
            "Removed {} stale record(s) from zone {}/{} status",
            stale_count, namespace, zone_name
        );
    }

    Ok(stale_count)
}
