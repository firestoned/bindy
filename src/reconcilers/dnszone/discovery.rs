// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Record discovery logic for DNS zones.
//!
//! This module handles discovering DNS record resources that match zone label selectors,
//! tagging/untagging records, and checking record readiness.

#![allow(unused_imports)] // Some imports used in macro-generated code

use anyhow::{Context as AnyhowContext, Result};
use kube::{
    api::{ListParams, Patch, PatchParams},
    Api, Client, ResourceExt,
};
use serde_json::json;
use std::collections::HashSet;
use tracing::{debug, info, warn};

use crate::crd::DNSZone;
use crate::reconcilers::pagination::list_all_paginated;

/// Reconciles DNS records for a zone by discovering records that match the zone's label selectors.
///
/// **Event-Driven Architecture**: This function implements the core of the zone/record ownership model:
/// 1. Discovers records matching the zone's `recordsFrom` label selectors
/// 2. Tags matched records by setting `status.zoneRef` (triggers record reconciliation via watches)
/// 3. Untags previously matched records by clearing `status.zoneRef` (stops record reconciliation)
/// 4. Returns references to currently matched records for `DNSZone.status.records` tracking
///
/// Record reconcilers watch `status.zoneRef` to determine which zone they belong to.
/// When `status.zoneRef` is set, the record is reconciled to BIND9.
/// When `status.zoneRef` is cleared, the record reconciler marks it as `"NotSelected"`.
///
/// Before a record is untagged, its data is deleted from the zone's primary
/// BIND9 instances. If that DNS deletion fails, the record is kept selected
/// (and in `status.records`) so the cleanup is retried on the next
/// reconciliation instead of orphaning the data in BIND9.
///
/// # Arguments
///
/// * `client` - Kubernetes API client for querying DNS records
/// * `dnszone` - The `DNSZone` resource with label selectors
/// * `stores` - Context stores for resolving instances and creating `Bind9Manager`s
///
/// # Returns
///
/// * `Ok(Vec<RecordReference>)` - List of currently matched DNS records
/// * `Err(_)` - If record discovery or tagging fails
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_zone_records(
    client: Client,
    dnszone: DNSZone,
    stores: &crate::context::Stores,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let spec = &dnszone.spec;
    let zone_name = &spec.zone_name;

    // Early return if no label selectors are defined
    let Some(ref records_from) = spec.records_from else {
        info!(
            "No label selectors defined for zone {}, skipping record discovery",
            zone_name
        );
        // If no selectors, untag ALL previously matched records
        return Ok(Vec::new());
    };

    info!(
        "Discovering DNS records for zone {} using {} label selector(s)",
        zone_name,
        records_from.len()
    );

    let mut all_record_refs = Vec::new();

    // Query all record types and filter by label selectors
    for record_source in records_from {
        let selector = &record_source.selector;

        // Discover each record type
        all_record_refs.extend(discover_a_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_aaaa_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_txt_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_cname_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_mx_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_ns_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_srv_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_caa_records(&client, &namespace, selector, zone_name).await?);
    }

    info!(
        "Discovered {} DNS record(s) for zone {}",
        all_record_refs.len(),
        zone_name
    );

    // Get previously matched records from current status
    let previous_records: HashSet<String> = dnszone
        .status
        .as_ref()
        .map(|s| {
            s.records
                .iter()
                .map(|r| format!("{}/{}", r.kind, r.name))
                .collect()
        })
        .unwrap_or_default();

    // Create set of currently matched records
    let current_records: HashSet<String> = all_record_refs
        .iter()
        .map(|r| format!("{}/{}", r.kind, r.name))
        .collect();

    // Tag all matched records to ensure status.zoneRef is set
    // Previously we only tagged "newly matched" records, but records can exist in
    // status.records without having status.zoneRef set (e.g., from a previous
    // implementation or migration). Always tag to ensure consistency.
    for record_ref in &all_record_refs {
        let record_key = format!("{}/{}", record_ref.kind, record_ref.name);
        let is_new = !previous_records.contains(&record_key);

        if is_new {
            info!(
                "Newly matched record: {} {}/{}",
                record_ref.kind, namespace, record_ref.name
            );
        } else {
            debug!(
                "Re-tagging existing record to ensure status.zoneRef: {} {}/{}",
                record_ref.kind, namespace, record_ref.name
            );
        }

        tag_record_with_zone(
            &client,
            &namespace,
            &record_ref.kind,
            &record_ref.name,
            zone_name,
            &dnszone,
        )
        .await?;
    }

    // Untag previously matched records that no longer match or were deleted
    // (in previous but not in current). Before untagging, delete the record's
    // data from this zone's primary BIND9 instances - otherwise the data
    // would stay live in BIND9 forever (the record reconciler only marks
    // unselected records as NotSelected, it does not delete them).
    let previous_refs: Vec<crate::crd::RecordReferenceWithTimestamp> = dnszone
        .status
        .as_ref()
        .map(|s| s.records.clone())
        .unwrap_or_default();

    // Lazily resolved (and cached) primary instances for this zone
    let mut primary_refs_cache: Option<Vec<crate::crd::InstanceReference>> = None;

    for record_ref in unselected_previous_records(&previous_refs, &current_records) {
        let kind = record_ref.kind.as_str();
        let name = record_ref.name.as_str();

        warn!(
            "Record no longer matches zone {} (unmatched or deleted): {} {}/{}",
            zone_name, kind, namespace, name
        );

        // Delete the record data from BIND9 before untagging
        if let Err(e) = cleanup_unselected_record_dns(
            &client,
            stores,
            &dnszone,
            &namespace,
            &record_ref,
            &mut primary_refs_cache,
        )
        .await
        {
            // Do NOT untag: keep the record selected (zoneRef intact and
            // present in status.records) so the DNS deletion is retried on
            // the next reconciliation instead of orphaning data in BIND9.
            warn!(
                "Failed to delete DNS data for unselected record {} {}/{} from zone {}: {}. \
                 Keeping record selected for retry.",
                kind, namespace, name, zone_name, e
            );
            all_record_refs.push(record_ref.clone());
            continue;
        }

        // Try to untag the record, but don't fail if it was deleted
        // If the record was deleted, the API will return NotFound, which is fine
        if let Err(e) = untag_record_from_zone(&client, &namespace, kind, name, zone_name).await {
            // Check if error is because record was deleted (NotFound)
            if e.to_string().contains("NotFound") || e.to_string().contains("not found") {
                info!(
                    "Record {} {}/{} was deleted, removing from zone {} status",
                    kind, namespace, name, zone_name
                );
            } else {
                // Other errors should be logged but not fail the reconciliation
                warn!(
                    "Failed to untag record {} {}/{} from zone {}: {}",
                    kind, namespace, name, zone_name, e
                );
            }
        }
        // Continue regardless - the record will be removed from status.records
        // when we return all_record_refs (which doesn't include this record)
    }

    // CRITICAL: Preserve existing timestamps for records that haven't changed
    // This prevents status updates from triggering unnecessary reconciliation loops
    if let Some(status) = &dnszone.status {
        let existing_timestamps: std::collections::HashMap<String, _> = status
            .records
            .iter()
            .filter_map(|r| {
                r.last_reconciled_at
                    .as_ref()
                    .map(|timestamp| (format!("{}/{}", r.kind, r.name), timestamp.clone()))
            })
            .collect();

        // Update timestamps for records that already existed
        for record_ref in &mut all_record_refs {
            let key = format!("{}/{}", record_ref.kind, record_ref.name);
            if let Some(existing_timestamp) = existing_timestamps.get(&key) {
                record_ref.last_reconciled_at = Some(existing_timestamp.clone());
            }
        }
    }

    Ok(all_record_refs)
}

/// HTTP status code returned by the Kubernetes API when a resource does not exist.
const HTTP_STATUS_NOT_FOUND: u16 = 404;

/// Returns the previously matched record references that are no longer selected.
///
/// # Arguments
///
/// * `previous` - Record references from the zone's current `status.records[]`
/// * `current_keys` - Set of `"Kind/name"` keys for currently matched records
fn unselected_previous_records(
    previous: &[crate::crd::RecordReferenceWithTimestamp],
    current_keys: &HashSet<String>,
) -> Vec<crate::crd::RecordReferenceWithTimestamp> {
    previous
        .iter()
        .filter(|r| !current_keys.contains(&format!("{}/{}", r.kind, r.name)))
        .cloned()
        .collect()
}

/// Maps a Kubernetes record kind (e.g., `"ARecord"`) to its hickory `RecordType`.
///
/// # Errors
///
/// Returns an error if the kind is not a known DNS record kind.
fn hickory_record_type_for_kind(kind: &str) -> Result<hickory_proto::rr::RecordType> {
    use crate::crd::DNSRecordKind;
    use hickory_proto::rr::RecordType;

    let record_kind = DNSRecordKind::try_from(kind)
        .map_err(|e| anyhow::anyhow!("Unknown DNS record kind '{kind}': {e}"))?;

    Ok(match record_kind {
        DNSRecordKind::A => RecordType::A,
        DNSRecordKind::AAAA => RecordType::AAAA,
        DNSRecordKind::TXT => RecordType::TXT,
        DNSRecordKind::CNAME => RecordType::CNAME,
        DNSRecordKind::MX => RecordType::MX,
        DNSRecordKind::NS => RecordType::NS,
        DNSRecordKind::SRV => RecordType::SRV,
        DNSRecordKind::CAA => RecordType::CAA,
    })
}

/// Builds a dynamic API client for a DNS record kind in the given namespace.
fn dynamic_record_api(
    client: &Client,
    namespace: &str,
    kind: &str,
) -> kube::api::Api<kube::api::DynamicObject> {
    // Convert kind to plural resource name (e.g., "ARecord" -> "arecords")
    let plural = format!("{}s", kind.to_lowercase());

    let gvk = kube::core::GroupVersionKind {
        group: "bindy.firestoned.io".to_string(),
        version: "v1beta1".to_string(),
        kind: kind.to_string(),
    };

    let api_resource = kube::api::ApiResource::from_gvk_with_plural(&gvk, &plural);

    kube::api::Api::<kube::api::DynamicObject>::namespaced_with(
        client.clone(),
        namespace,
        &api_resource,
    )
}

/// Deletes the DNS data of a record that is no longer selected by a zone.
///
/// Called by `reconcile_zone_records()` before untagging a record. Without this,
/// unselecting a record (label/selector change) would leave its data live in
/// BIND9 forever, since the record reconciler only marks unselected records
/// as `NotSelected`.
///
/// The deletion is skipped (returns `Ok`) when:
/// - The record resource no longer exists (its finalizer handles DNS cleanup)
/// - The record has no `status.zoneRef` (it was never published to DNS)
/// - The record's `status.zoneRef` points at a different zone (not ours to delete)
/// - The zone has no instances or no primary instances (nowhere to delete from)
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `stores` - Context stores for resolving instances and creating `Bind9Manager`s
/// * `dnszone` - The zone that previously selected this record
/// * `namespace` - Namespace of the record
/// * `record_ref` - Reference to the unselected record
/// * `primary_refs_cache` - Cache of the zone's primary instances (resolved once)
///
/// # Errors
///
/// Returns an error if the record cannot be fetched (other than `NotFound`),
/// primary instances cannot be resolved, or the DNS deletion fails. Callers
/// must NOT untag the record in that case, so the cleanup is retried.
async fn cleanup_unselected_record_dns(
    client: &Client,
    stores: &crate::context::Stores,
    dnszone: &DNSZone,
    namespace: &str,
    record_ref: &crate::crd::RecordReferenceWithTimestamp,
    primary_refs_cache: &mut Option<Vec<crate::crd::InstanceReference>>,
) -> Result<()> {
    let kind = record_ref.kind.as_str();
    let name = record_ref.name.as_str();

    // Fetch the record to read its published DNS name and zone ownership
    let api = dynamic_record_api(client, namespace, kind);
    let record = match api.get(name).await {
        Ok(record) => record,
        Err(kube::Error::Api(ae)) if ae.code == HTTP_STATUS_NOT_FOUND => {
            // Record resource was deleted - its finalizer handles DNS cleanup
            debug!(
                "Record {} {}/{} no longer exists, skipping DNS cleanup",
                kind, namespace, name
            );
            return Ok(());
        }
        Err(e) => {
            return Err(anyhow::Error::from(e)
                .context(format!("Failed to fetch {kind} {namespace}/{name}")));
        }
    };

    let record_json = serde_json::to_value(&record)?;
    let status = record_json.get("status");

    // No zoneRef means the record was never published to DNS
    let Some(zone_ref) = status
        .and_then(|s| s.get("zoneRef"))
        .filter(|z| !z.is_null())
    else {
        debug!(
            "Record {} {}/{} has no status.zoneRef, skipping DNS cleanup",
            kind, namespace, name
        );
        return Ok(());
    };

    // Only delete data for records this zone actually owns
    let owned_by_this_zone = zone_ref.get("name").and_then(|v| v.as_str())
        == Some(dnszone.name_any().as_str())
        && zone_ref.get("namespace").and_then(|v| v.as_str())
            == Some(dnszone.namespace().unwrap_or_default().as_str());
    if !owned_by_this_zone {
        debug!(
            "Record {} {}/{} is owned by a different zone, skipping DNS cleanup",
            kind, namespace, name
        );
        return Ok(());
    }

    // Resolve this zone's primary instances once and cache across records
    if primary_refs_cache.is_none() {
        let Ok(instance_refs) = crate::reconcilers::dnszone::validation::get_instances_from_zone(
            dnszone,
            &stores.bind9_instances,
        ) else {
            // Zone has no instances - there is nowhere the data could live
            debug!(
                "Zone {} has no instances, skipping DNS cleanup for {} {}/{}",
                dnszone.spec.zone_name, kind, namespace, name
            );
            return Ok(());
        };

        let primaries =
            crate::reconcilers::dnszone::primary::filter_primary_instances(client, &instance_refs)
                .await?;
        *primary_refs_cache = Some(primaries);
    }

    let primary_refs = primary_refs_cache.as_deref().unwrap_or_default();
    if primary_refs.is_empty() {
        debug!(
            "Zone {} has no primary instances, skipping DNS cleanup for {} {}/{}",
            dnszone.spec.zone_name, kind, namespace, name
        );
        return Ok(());
    }

    // The DNS name actually published (handles renames), falling back to spec.name
    let record_name = status
        .and_then(|s| s.get("publishedName"))
        .and_then(|p| p.as_str())
        .or_else(|| {
            record_json
                .get("spec")
                .and_then(|s| s.get("name"))
                .and_then(|n| n.as_str())
        })
        .unwrap_or(name);

    let record_type_hickory = hickory_record_type_for_kind(kind)?;

    crate::reconcilers::records::delete_record_from_primaries(
        client,
        stores,
        primary_refs,
        &dnszone.spec.zone_name,
        record_name,
        record_type_hickory,
        true, // fail_on_error: do not untag until the data is really gone
    )
    .await?;

    info!(
        "Deleted DNS data for unselected record {} {}/{} ('{}') from zone {}",
        kind, namespace, name, record_name, dnszone.spec.zone_name
    );

    Ok(())
}

/// Tags a DNS record with zone ownership by setting `status.zoneRef`.
///
/// **Event-Driven Architecture**: This function is called when a `DNSZone`'s label selector
/// matches a record. It sets `status.zoneRef` with a structured reference to the zone,
/// which triggers the record operator via Kubernetes watch to reconcile the record to BIND9.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the record
/// * `kind` - Record kind (e.g., `ARecord`, `CNAMERecord`)
/// * `name` - Record name
/// * `zone_fqdn` - Fully qualified domain name of the zone (e.g., `"example.com"`)
///
/// # Returns
///
/// * `Ok(())` - If the record was tagged successfully
/// * `Err(_)` - If tagging failed
async fn tag_record_with_zone(
    client: &Client,
    namespace: &str,
    kind: &str,
    name: &str,
    zone_fqdn: &str,
    dnszone: &DNSZone,
) -> Result<()> {
    debug!(
        "Tagging {} {}/{} with zone {}",
        kind, namespace, name, zone_fqdn
    );

    // Create a dynamic API client
    let api = dynamic_record_api(client, namespace, kind);

    // Create ZoneReference for status.zoneRef (event-driven architecture)
    let zone_ref = crate::crd::ZoneReference {
        api_version: crate::constants::API_GROUP_VERSION.to_string(),
        kind: crate::constants::KIND_DNS_ZONE.to_string(),
        name: dnszone.name_any(),
        namespace: dnszone.namespace().unwrap_or_default(),
        zone_name: zone_fqdn.to_string(),
        last_reconciled_at: None, // Not used in DNSZone status
    };

    // Patch status to set zone field (backward compatibility) AND zoneRef (new event-driven field)
    let status_patch = json!({
        "status": {
            "zone": zone_fqdn,
            "zoneRef": zone_ref
        }
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .with_context(|| {
            format!("Failed to set status.zone and status.zoneRef on {kind} {namespace}/{name}")
        })?;

    info!(
        "Successfully tagged {} {}/{} with zone {} (set status.zoneRef)",
        kind, namespace, name, zone_fqdn
    );

    Ok(())
}

/// Untags a DNS record that no longer matches a zone's selector.
///
/// This function clears the `status.zoneRef` field (event-driven architecture)
/// and the deprecated `status.zone` field for backward compatibility.
///
/// **Event-Driven Architecture**: Records use `status.zoneRef` (not annotations) to track
/// which zone they belong to. When a record no longer matches a zone's selector, this
/// function clears the status fields so the record reconciler knows it's no longer selected.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the record
/// * `kind` - Record kind (e.g., `ARecord`, `CNAMERecord`)
/// * `name` - Record name
/// * `previous_zone_fqdn` - FQDN of the zone that previously owned this record
///
/// # Returns
///
/// * `Ok(())` - If the record was untagged successfully
/// * `Err(_)` - If untagging failed
async fn untag_record_from_zone(
    client: &Client,
    namespace: &str,
    kind: &str,
    name: &str,
    previous_zone_fqdn: &str,
) -> Result<()> {
    debug!(
        "Untagging {} {}/{} from zone {} (clearing status.zoneRef)",
        kind, namespace, name, previous_zone_fqdn
    );

    // Create a dynamic API client
    let api = dynamic_record_api(client, namespace, kind);

    // Patch status to remove zoneRef (event-driven architecture uses status.zoneRef, not annotations)
    let status_patch = json!({
        "status": {
            "zoneRef": null,
            "zone": null  // Also clear deprecated zone field for backward compatibility
        }
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .with_context(|| format!("Failed to clear status.zoneRef on {kind} {namespace}/{name}"))?;

    info!(
        "Successfully untagged {} {}/{} from zone {} (cleared status.zoneRef)",
        kind, namespace, name, previous_zone_fqdn
    );

    Ok(())
}

/// Trait for DNS record types that can be discovered by DNSZone controllers.
///
/// This trait provides the minimal interface needed for the generic record discovery
/// function to work across all DNS record types.
trait DiscoverableRecord:
    kube::Resource<DynamicType = (), Scope = k8s_openapi::NamespaceResourceScope>
    + Clone
    + std::fmt::Debug
    + serde::de::DeserializeOwned
    + kube::ResourceExt
{
    /// Get the DNS record kind enum variant for this record type.
    fn dns_record_kind() -> crate::crd::DNSRecordKind;

    /// Get the record name from the spec (e.g., "www", "mail", "@").
    fn spec_name(&self) -> &str;

    /// Get the record status.
    fn record_status(&self) -> Option<&crate::crd::RecordStatus>;
}

// Implementations of DiscoverableRecord for all DNS record types

impl DiscoverableRecord for crate::crd::ARecord {
    fn dns_record_kind() -> crate::crd::DNSRecordKind {
        crate::crd::DNSRecordKind::A
    }

    fn spec_name(&self) -> &str {
        &self.spec.name
    }

    fn record_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }
}

impl DiscoverableRecord for crate::crd::AAAARecord {
    fn dns_record_kind() -> crate::crd::DNSRecordKind {
        crate::crd::DNSRecordKind::AAAA
    }

    fn spec_name(&self) -> &str {
        &self.spec.name
    }

    fn record_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }
}

impl DiscoverableRecord for crate::crd::TXTRecord {
    fn dns_record_kind() -> crate::crd::DNSRecordKind {
        crate::crd::DNSRecordKind::TXT
    }

    fn spec_name(&self) -> &str {
        &self.spec.name
    }

    fn record_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }
}

impl DiscoverableRecord for crate::crd::CNAMERecord {
    fn dns_record_kind() -> crate::crd::DNSRecordKind {
        crate::crd::DNSRecordKind::CNAME
    }

    fn spec_name(&self) -> &str {
        &self.spec.name
    }

    fn record_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }
}

impl DiscoverableRecord for crate::crd::MXRecord {
    fn dns_record_kind() -> crate::crd::DNSRecordKind {
        crate::crd::DNSRecordKind::MX
    }

    fn spec_name(&self) -> &str {
        &self.spec.name
    }

    fn record_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }
}

impl DiscoverableRecord for crate::crd::NSRecord {
    fn dns_record_kind() -> crate::crd::DNSRecordKind {
        crate::crd::DNSRecordKind::NS
    }

    fn spec_name(&self) -> &str {
        &self.spec.name
    }

    fn record_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }
}

impl DiscoverableRecord for crate::crd::SRVRecord {
    fn dns_record_kind() -> crate::crd::DNSRecordKind {
        crate::crd::DNSRecordKind::SRV
    }

    fn spec_name(&self) -> &str {
        &self.spec.name
    }

    fn record_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }
}

impl DiscoverableRecord for crate::crd::CAARecord {
    fn dns_record_kind() -> crate::crd::DNSRecordKind {
        crate::crd::DNSRecordKind::CAA
    }

    fn spec_name(&self) -> &str {
        &self.spec.name
    }

    fn record_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }
}

/// Generic helper function to discover DNS records matching a label selector.
///
/// This function eliminates duplication across the 8 record-type-specific discovery functions.
/// It works for any record type implementing the `DiscoverableRecord` trait.
///
/// # Type Parameters
///
/// * `T` - The DNS record type to discover (e.g., `ARecord`, `TXTRecord`)
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for records
/// * `selector` - Label selector to match records against
/// * `_zone_name` - Zone name (unused but kept for API compatibility)
///
/// # Returns
///
/// Vector of record references with timestamps for records that match the selector
///
/// # Errors
///
/// Returns an error if listing records from the Kubernetes API fails
async fn discover_records_generic<T>(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>>
where
    T: DiscoverableRecord,
{
    use std::collections::BTreeMap;

    let api: kube::Api<T> = kube::Api::namespaced(client.clone(), namespace);
    let records = list_all_paginated(&api, kube::api::ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.meta().labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!(
            "Discovered {} record {}/{}",
            T::dns_record_kind().as_str(),
            namespace,
            record.name_any()
        );

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .record_status()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                // Parse ISO8601 timestamp string into k8s Time
                ts.parse::<k8s_openapi::jiff::Timestamp>()
                    .ok()
                    .map(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time)
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: T::dns_record_kind().as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec_name().to_string()),
            last_reconciled_at,
        });
    }

    Ok(record_refs)
}

/// Helper function to discover A records matching a label selector.
async fn discover_a_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    discover_records_generic::<crate::crd::ARecord>(client, namespace, selector, zone_name).await
}

/// Helper function to discover AAAA records matching a label selector.
async fn discover_aaaa_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    discover_records_generic::<crate::crd::AAAARecord>(client, namespace, selector, zone_name).await
}

/// Helper function to discover TXT records matching a label selector.
async fn discover_txt_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    discover_records_generic::<crate::crd::TXTRecord>(client, namespace, selector, zone_name).await
}

/// Helper function to discover CNAME records matching a label selector.
async fn discover_cname_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    discover_records_generic::<crate::crd::CNAMERecord>(client, namespace, selector, zone_name)
        .await
}

/// Helper function to discover MX records matching a label selector.
async fn discover_mx_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    discover_records_generic::<crate::crd::MXRecord>(client, namespace, selector, zone_name).await
}

/// Helper function to discover NS records matching a label selector.
async fn discover_ns_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    discover_records_generic::<crate::crd::NSRecord>(client, namespace, selector, zone_name).await
}

/// Helper function to discover SRV records matching a label selector.
async fn discover_srv_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    discover_records_generic::<crate::crd::SRVRecord>(client, namespace, selector, zone_name).await
}

/// Helper function to discover CAA records matching a label selector.
async fn discover_caa_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    discover_records_generic::<crate::crd::CAARecord>(client, namespace, selector, zone_name).await
}

/// Checks if all DNS records are ready.
///
/// Iterates through all record references and verifies their readiness status.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to check records in
/// * `record_refs` - List of record references to check
///
/// # Returns
///
/// `true` if all records are ready, `false` otherwise
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail
pub async fn check_all_records_ready(
    client: &Client,
    namespace: &str,
    record_refs: &[crate::crd::RecordReferenceWithTimestamp],
) -> Result<bool> {
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, DNSRecordKind, MXRecord, NSRecord, SRVRecord,
        TXTRecord,
    };

    for record_ref in record_refs {
        let kind = DNSRecordKind::try_from(record_ref.kind.as_str())?;
        let is_ready = match kind {
            DNSRecordKind::A => {
                let api: Api<ARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::AAAA => {
                let api: Api<AAAARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::TXT => {
                let api: Api<TXTRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::CNAME => {
                let api: Api<CNAMERecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::MX => {
                let api: Api<MXRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::NS => {
                let api: Api<NSRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::SRV => {
                let api: Api<SRVRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::CAA => {
                let api: Api<CAARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
        };

        if !is_ready {
            debug!(
                "Record {}/{} (kind: {}) is not ready yet",
                namespace, record_ref.name, record_ref.kind
            );
            return Ok(false);
        }
    }

    Ok(true)
}

/// Check if a specific record is ready by examining its status conditions.
async fn check_record_ready<T>(api: &Api<T>, name: &str) -> Result<bool>
where
    T: kube::Resource<DynamicType = ()>
        + Clone
        + serde::de::DeserializeOwned
        + serde::Serialize
        + std::fmt::Debug
        + Send
        + Sync,
    <T as kube::Resource>::DynamicType: Default,
{
    let record = match api.get(name).await {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to get record {}: {}", name, e);
            return Ok(false);
        }
    };

    // Use serde_json to access the status field dynamically
    let record_json = serde_json::to_value(&record)?;
    let status = record_json.get("status");

    if let Some(status_obj) = status {
        if let Some(conditions) = status_obj.get("conditions").and_then(|c| c.as_array()) {
            for condition in conditions {
                if let (Some(type_val), Some(status_val)) = (
                    condition.get("type").and_then(|t| t.as_str()),
                    condition.get("status").and_then(|s| s.as_str()),
                ) {
                    if type_val == "Ready" && status_val == "True" {
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(false)
}

/// Find all `DNSZones` that have selected a given record via label selectors.
///
/// This function is used by the watch mapper to determine which `DNSZones` should be
/// reconciled when a DNS record changes. It checks each `DNSZone`'s `status.records` list
/// to see if the record is present.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record_namespace` - Namespace of the record
/// * `record_kind` - Kind of the record (e.g., `"ARecord"`, `"TXTRecord"`)
/// * `record_name` - Name of the record resource
///
/// # Returns
///
/// A vector of tuples containing `(zone_name, zone_namespace)` for all `DNSZones` that have
/// selected this record.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail.
pub async fn find_zones_selecting_record(
    client: &Client,
    record_namespace: &str,
    record_kind: &str,
    record_name: &str,
) -> Result<Vec<(String, String)>> {
    let api: Api<DNSZone> = Api::namespaced(client.clone(), record_namespace);
    let zones = list_all_paginated(&api, ListParams::default()).await?;

    let mut selecting_zones = vec![];

    for zone in zones {
        let Some(ref status) = zone.status else {
            continue;
        };

        // Check if this record is in the zone's status.records list
        let is_selected = status
            .records
            .iter()
            .any(|r| r.kind == record_kind && r.name == record_name);

        if is_selected {
            let zone_name = zone.name_any();
            let zone_namespace = zone.namespace().unwrap_or_default();
            selecting_zones.push((zone_name, zone_namespace));
        }
    }

    Ok(selecting_zones)
}
/// Counts DNS records matching a zone for logging purposes.
///
/// **Event-Driven Architecture**: This function only counts and logs records that have
/// `status.zoneRef.zoneName` matching the zone. The actual reconciliation is triggered
/// automatically by Kubernetes watches - when the `DNSZone` status changes, record operators
/// are notified via watch events and reconcile automatically.
///
/// This function is called after zone recreation (e.g., pod restarts) to log how many
/// records will be automatically reconciled via the event-driven architecture.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for records
/// * `zone_name` - Zone FQDN to match
///
/// # Errors
///
/// Returns an error if listing records fails. Errors are logged but don't fail
/// the parent `DNSZone` reconciliation.
pub async fn trigger_record_reconciliation(
    client: &Client,
    namespace: &str,
    zone_name: &str,
) -> Result<()> {
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, TXTRecord,
    };

    debug!(
        "Triggering record reconciliation for zone {} in namespace {}",
        zone_name, namespace
    );

    // Helper macro to count records by status.zoneRef
    // Note: We don't need to patch anything - the event-driven architecture (watches)
    // will automatically trigger reconciliation when records see zone status changes
    macro_rules! count_records {
        ($record_type:ty, $type_name:expr) => {{
            let api: Api<$record_type> = Api::namespaced(client.clone(), namespace);
            let lp = ListParams::default();

            match api.list(&lp).await {
                Ok(records) => {
                    let matching_count = records
                        .items
                        .iter()
                        .filter(|r| {
                            // Check if status.zoneRef.zoneName matches
                            r.status
                                .as_ref()
                                .and_then(|s| s.zone_ref.as_ref())
                                .map(|zr| zr.zone_name == zone_name)
                                .unwrap_or(false)
                        })
                        .count();

                    debug!(
                        "Found {} {} record(s) for zone {} (event-driven watches will trigger reconciliation)",
                        matching_count,
                        $type_name,
                        zone_name
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to list {} records in namespace {}: {}",
                        $type_name, namespace, e
                    );
                }
            }
        }};
    }

    // Count records for each type (event-driven watches will trigger reconciliation automatically)
    count_records!(ARecord, "A");
    count_records!(AAAARecord, "AAAA");
    count_records!(TXTRecord, "TXT");
    count_records!(CNAMERecord, "CNAME");
    count_records!(MXRecord, "MX");
    count_records!(NSRecord, "NS");
    count_records!(SRVRecord, "SRV");
    count_records!(CAARecord, "CAA");

    Ok(())
}

/// Discover and update DNSZone status with DNS records.
///
/// This wrapper function orchestrates record discovery and status updates:
/// 1. Sets "Progressing" status condition
/// 2. Calls `reconcile_zone_records()` to discover records
/// 3. Updates DNSZone status with discovered records
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `dnszone` - The DNSZone resource being reconciled
/// * `status_updater` - Status updater for setting conditions and records
/// * `stores` - Context stores for resolving instances and creating `Bind9Manager`s
///
/// # Returns
///
/// Tuple of (record_refs, records_count) - the discovered record references and their count
///
/// # Errors
///
/// Returns an error if record discovery fails (e.g., a transient record list
/// failure). The existing `status.records` list is left untouched in that case
/// so the watch mapper keeps working until discovery succeeds again; wiping it
/// on a transient error would orphan the zone's records.
pub async fn discover_and_update_records(
    client: &kube::Client,
    dnszone: &crate::crd::DNSZone,
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
    stores: &crate::context::Stores,
) -> Result<(Vec<crate::crd::RecordReferenceWithTimestamp>, usize)> {
    let spec = &dnszone.spec;

    // Set progressing status
    status_updater.set_condition(
        "Progressing",
        "True",
        "RecordsDiscovering",
        "Discovering DNS records via label selectors",
    );

    // Discover records. On failure, propagate the error WITHOUT touching
    // status.records: overwriting it with an empty list on a transient list
    // failure would break the record watch mapper until the next successful
    // discovery.
    let record_refs = reconcile_zone_records(client.clone(), dnszone.clone(), stores)
        .await
        .map_err(|e| {
            warn!(
                "Failed to discover DNS records for zone {}: {}. Keeping existing status.records for retry.",
                spec.zone_name, e
            );
            e.context(format!(
                "Failed to discover DNS records for zone {}",
                spec.zone_name
            ))
        })?;

    info!(
        "Discovered {} DNS record(s) for zone {} via label selectors",
        record_refs.len(),
        spec.zone_name
    );

    let records_count = record_refs.len();

    // Update DNSZone status with discovered records (in-memory)
    status_updater.set_records(&record_refs);

    Ok((record_refs, records_count))
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod discovery_tests;
