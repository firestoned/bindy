// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! DNS record reconciliation logic.
//!
//! This module contains reconcilers for all DNS record types supported by Bindy.
//!
//! **Event-Driven Architecture**: DNS record reconcilers react to status changes:
//! 1. The `DNSZone` controller sets `status.zoneRef` when recordsFrom selector matches
//! 2. Record controller watches status changes and reconciles when `status.zoneRef` is set
//! 3. If `status.zoneRef` is absent, the record is not selected by any zone (mark as `NotSelected`)
//! 4. Record controller looks up the `DNSZone` via `status.zoneRef` to get instances
//! 5. Record is added to BIND9 primaries using instances from the zone

// Removed ANNOTATION_ZONE_OWNER - using status.zoneRef instead (event-driven architecture)
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, Condition, DNSZone, MXRecord, NSRecord,
    RecordStatus, SRVRecord, TXTRecord,
};
use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::{Event, ObjectReference};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::chrono::Utc;
use kube::{
    api::{Patch, PatchParams, PostParams},
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
    bind9_instances_store: &kube::runtime::reflector::Store<crate::crd::Bind9Instance>,
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
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
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
            )
            .await?;
            return Ok(None);
        }
    };

    // Get instances from the DNSZone
    let instance_refs =
        match crate::reconcilers::dnszone::get_instances_from_zone(&dnszone, bind9_instances_store)
        {
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
                )
                .await?;
                return Ok(None);
            }
        };

    // Filter to PRIMARY instances only
    let primary_refs =
        match crate::reconcilers::dnszone::filter_primary_instances(client, &instance_refs).await {
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

/// Reconciles an `ARecord` (IPv4 address) resource.
///
/// Finds `DNSZones` that have selected this record via label selectors and creates/updates
/// the record in BIND9 primaries for those zones using dynamic DNS updates.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The `ARecord` resource to reconcile
///
/// # Example
///
/// ```rust,no_run
/// use bindy::reconcilers::reconcile_a_record;
/// use bindy::crd::ARecord;
/// use bindy::context::Context;
/// use std::sync::Arc;
///
/// async fn handle_a_record(ctx: Arc<Context>, record: ARecord) -> anyhow::Result<()> {
///     reconcile_a_record(ctx, record).await?;
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
pub async fn reconcile_a_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: ARecord,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling ARecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) =
        prepare_record_reconciliation(&client, &record, "A", spec, bind9_instances_store).await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

    // Add record to BIND9 primaries using instances
    match add_a_record_to_instances(
        &client,
        &ctx.stores,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &spec.name,
        &spec.ipv4_address,
        spec.ttl,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added A record {}.{} via {} primary instance(s)",
                spec.name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.selectedRecords[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                "ARecord",
                &name,
                &namespace,
            )
            .await?;

            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("A record added to zone {}", rec_ctx.zone_ref.zone_name),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add A record {}.{}: {}",
                spec.name, rec_ctx.zone_ref.zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an A record to BIND9 primaries using instance references.
///
/// Uses dynamic DNS updates (nsupdate protocol via DNS TCP port 53) to add the record
/// to all primary endpoints for the specified instances.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Primary instance references
/// * `zone_name` - DNS zone name
/// * `record_name` - Name portion of the DNS record
/// * `ipv4_address` - IPv4 address for the record
/// * `ttl` - Optional TTL value
/// * `zone_manager` - BIND9 manager instance
///
/// # Errors
///
/// Returns an error if the BIND9 record creation fails.
#[allow(clippy::too_many_arguments)]
async fn add_a_record_to_instances(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    ipv4_address: &str,
    ttl: Option<i32>,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        client,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let ipv4_address = ipv4_address.to_string();

            // Get namespace for this instance
            let instance_namespace = instance_map.get(&instance_name).expect("Instance should be in map").clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                zone_manager
                    .add_a_record(
                        &zone_name,
                        &record_name,
                        &ipv4_address,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                    .context(format!(
                        "Failed to add A record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})"
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

/// Reconciles a `TXTRecord` (text) resource.
///
/// Finds `DNSZones` that have selected this record via label selectors and creates/updates
/// the record in BIND9 primaries for those zones using dynamic DNS updates.
/// Commonly used for SPF, DKIM, DMARC, and domain verification.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
pub async fn reconcile_txt_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: TXTRecord,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling TXTRecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) =
        prepare_record_reconciliation(&client, &record, "TXT", spec, bind9_instances_store).await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

    // Add record to BIND9 primaries using instances
    match add_txt_record_to_instances(
        &client,
        &ctx.stores,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &spec.name,
        &spec.text,
        spec.ttl,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added TXT record {}.{} via {} primary instance(s)",
                spec.name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.selectedRecords[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                "TXTRecord",
                &name,
                &namespace,
            )
            .await?;

            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("TXT record added to zone {}", rec_ctx.zone_ref.zone_name),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add TXT record {}.{}: {}",
                spec.name, rec_ctx.zone_ref.zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
            )
            .await?;
        }
    }

    Ok(())
}

/// Add a TXT record to BIND9 primaries using instance references.
#[allow(clippy::too_many_arguments)]
async fn add_txt_record_to_instances(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    text: &[String],
    ttl: Option<i32>,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        client,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let text = text.to_vec();
            let instance_namespace = instance_map
                .get(&instance_name)
                .expect("Instance should be in map")
                .clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                zone_manager
                    .add_txt_record(
                        &zone_name,
                        &record_name,
                        &text,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                    .context(format!(
                        "Failed to add TXT record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})"
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

/// Reconciles an `AAAARecord` (IPv6 address) resource.
///
/// Finds `DNSZones` that have selected this record via label selectors and creates/updates
/// the record in BIND9 primaries for those zones using dynamic DNS updates.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
pub async fn reconcile_aaaa_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: AAAARecord,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling AAAARecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) =
        prepare_record_reconciliation(&client, &record, "AAAA", spec, bind9_instances_store)
            .await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

    // Add record to BIND9 primaries using instances
    match add_aaaa_record_to_instances(
        &client,
        &ctx.stores,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &spec.name,
        &spec.ipv6_address,
        spec.ttl,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added AAAA record {}.{} via {} primary instance(s)",
                spec.name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.selectedRecords[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                "AAAARecord",
                &name,
                &namespace,
            )
            .await?;

            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("AAAA record added to zone {}", rec_ctx.zone_ref.zone_name),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add AAAA record {}.{}: {}",
                spec.name, rec_ctx.zone_ref.zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an AAAA record to BIND9 primaries using instance references.
#[allow(clippy::too_many_arguments)]
async fn add_aaaa_record_to_instances(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    ipv6_address: &str,
    ttl: Option<i32>,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        client,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let ipv6_address = ipv6_address.to_string();
            let instance_namespace = instance_map
                .get(&instance_name)
                .expect("Instance should be in map")
                .clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                zone_manager
                    .add_aaaa_record(
                        &zone_name,
                        &record_name,
                        &ipv6_address,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                    .context(format!(
                        "Failed to add AAAA record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})"
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

/// Reconciles a `CNAMERecord` (canonical name alias) resource.
///
/// Finds `DNSZones` that have selected this record via label selectors and creates/updates
/// the record in BIND9 primaries for those zones using dynamic DNS updates.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_cname_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: CNAMERecord,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CNAMERecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) =
        prepare_record_reconciliation(&client, &record, "CNAME", spec, bind9_instances_store)
            .await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

    // Add record to BIND9 primaries using instances
    match add_cname_record_to_instances(
        &client,
        &ctx.stores,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &spec.name,
        &spec.target,
        spec.ttl,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added CNAME record {}.{} via {} primary instance(s)",
                spec.name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.selectedRecords[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                "CNAMERecord",
                &name,
                &namespace,
            )
            .await?;

            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("CNAME record added to zone {}", rec_ctx.zone_ref.zone_name),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add CNAME record {}.{}: {}",
                spec.name, rec_ctx.zone_ref.zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
            )
            .await?;
        }
    }

    Ok(())
}

/// Add a CNAME record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_cname_record_to_instances(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    target: &str,
    ttl: Option<i32>,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        client,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates,
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let target = target.to_string();
            let instance_namespace = instance_map
                .get(&instance_name)
                .expect("Instance should be in map")
                .clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                zone_manager
                    .add_cname_record(
                        &zone_name,
                        &record_name,
                        &target,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                    .context(format!(
                        "Failed to add CNAME record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})"
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

/// Reconciles an `MXRecord` (mail exchange) resource.
///
/// Finds `DNSZones` that have selected this record via label selectors and creates/updates
/// the record in BIND9 primaries for those zones using dynamic DNS updates.
/// MX records specify mail servers for email delivery.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_mx_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: MXRecord,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling MXRecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) =
        prepare_record_reconciliation(&client, &record, "MX", spec, bind9_instances_store).await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

    // Add record to BIND9 primaries using instances
    match add_mx_record_to_instances(
        &client,
        &ctx.stores,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &spec.name,
        spec.priority,
        &spec.mail_server,
        spec.ttl,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added MX record {}.{} via {} primary instance(s)",
                spec.name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.selectedRecords[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                "MXRecord",
                &name,
                &namespace,
            )
            .await?;

            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("MX record added to zone {}", rec_ctx.zone_ref.zone_name),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add MX record {}.{}: {}",
                spec.name, rec_ctx.zone_ref.zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an MX record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_mx_record_to_instances(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    priority: i32,
    mail_server: &str,
    ttl: Option<i32>,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        client,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates,
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let mail_server = mail_server.to_string();
            let instance_namespace = instance_map
                .get(&instance_name)
                .expect("Instance should be in map")
                .clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                zone_manager
                    .add_mx_record(
                        &zone_name,
                        &record_name,
                        priority,
                        &mail_server,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                    .context(format!(
                        "Failed to add MX record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})"
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

/// Reconciles an `NSRecord` (nameserver delegation) resource.
///
/// Finds `DNSZones` that have selected this record via label selectors and creates/updates
/// the record in BIND9 primaries for those zones using dynamic DNS updates.
/// NS records delegate a subdomain to different nameservers.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_ns_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: NSRecord,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling NSRecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) =
        prepare_record_reconciliation(&client, &record, "NS", spec, bind9_instances_store).await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

    // Add record to BIND9 primaries using instances
    match add_ns_record_to_instances(
        &client,
        &ctx.stores,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &spec.name,
        &spec.nameserver,
        spec.ttl,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added NS record {}.{} via {} primary instance(s)",
                spec.name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.selectedRecords[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                "NSRecord",
                &name,
                &namespace,
            )
            .await?;

            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("NS record added to zone {}", rec_ctx.zone_ref.zone_name),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add NS record {}.{}: {}",
                spec.name, rec_ctx.zone_ref.zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an NS record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_ns_record_to_instances(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    nameserver: &str,
    ttl: Option<i32>,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        client,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates,
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let nameserver = nameserver.to_string();
            let instance_namespace = instance_map
                .get(&instance_name)
                .expect("Instance should be in map")
                .clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                zone_manager
                    .add_ns_record(
                        &zone_name,
                        &record_name,
                        &nameserver,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                    .context(format!(
                        "Failed to add NS record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})"
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

/// Reconciles an `SRVRecord` (service location) resource.
///
/// Finds `DNSZones` that have selected this record via label selectors and creates/updates
/// the record in BIND9 primaries for those zones using dynamic DNS updates.
/// SRV records specify the location of services (e.g., _ldap._tcp).
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_srv_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: SRVRecord,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling SRVRecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) =
        prepare_record_reconciliation(&client, &record, "SRV", spec, bind9_instances_store).await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

    // Add record to BIND9 primaries using instances
    match add_srv_record_to_instances(
        &client,
        &ctx.stores,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &spec.name,
        spec.priority,
        spec.weight,
        spec.port,
        &spec.target,
        spec.ttl,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added SRV record {}.{} via {} primary instance(s)",
                spec.name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.selectedRecords[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                "SRVRecord",
                &name,
                &namespace,
            )
            .await?;

            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("SRV record added to zone {}", rec_ctx.zone_ref.zone_name),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add SRV record {}.{}: {}",
                spec.name, rec_ctx.zone_ref.zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an SRV record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_srv_record_to_instances(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    priority: i32,
    weight: i32,
    port: i32,
    target: &str,
    ttl: Option<i32>,
) -> Result<()> {
    use crate::bind9::types::SRVRecordData;
    use crate::reconcilers::dnszone::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        client,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates,
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let srv_data = SRVRecordData {
                priority,
                weight,
                port,
                target: target.to_string(),
                ttl,
            };
            let instance_namespace = instance_map
                .get(&instance_name)
                .expect("Instance should be in map")
                .clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                zone_manager
                    .add_srv_record(
                        &zone_name,
                        &record_name,
                        &srv_data,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                    .context(format!(
                        "Failed to add SRV record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})"
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

/// Reconciles a `CAARecord` (certificate authority authorization) resource.
///
/// Finds `DNSZones` that have selected this record via label selectors and creates/updates
/// the record in BIND9 primaries for those zones using dynamic DNS updates.
/// CAA records specify which certificate authorities can issue certificates.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_caa_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: CAARecord,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CAARecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;

    // Use generic helper to get zone and instances
    let Some(rec_ctx) =
        prepare_record_reconciliation(&client, &record, "CAA", spec, bind9_instances_store).await?
    else {
        return Ok(()); // Record not selected or status already updated
    };

    // Add record to BIND9 primaries using instances
    match add_caa_record_to_instances(
        &client,
        &ctx.stores,
        &rec_ctx.primary_refs,
        &rec_ctx.zone_ref.zone_name,
        &spec.name,
        spec.flags,
        &spec.tag,
        &spec.value,
        spec.ttl,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added CAA record {}.{} via {} primary instance(s)",
                spec.name,
                rec_ctx.zone_ref.zone_name,
                rec_ctx.primary_refs.len()
            );

            // Update lastReconciledAt timestamp in DNSZone.status.selectedRecords[]
            update_record_reconciled_timestamp(
                &client,
                &rec_ctx.zone_ref.namespace,
                &rec_ctx.zone_ref.name,
                "CAARecord",
                &name,
                &namespace,
            )
            .await?;

            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("CAA record added to zone {}", rec_ctx.zone_ref.zone_name),
                current_generation,
                Some(rec_ctx.current_hash),
                Some(chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add CAA record {}.{}: {}",
                spec.name, rec_ctx.zone_ref.zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone: {e}"),
                current_generation,
                None, // record_hash
                None, // last_updated
            )
            .await?;
        }
    }

    Ok(())
}

/// Add a CAA record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_caa_record_to_instances(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    flags: i32,
    tag: &str,
    value: &str,
    ttl: Option<i32>,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        client,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates,
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let tag = tag.to_string();
            let value = value.to_string();
            let instance_namespace = instance_map
                .get(&instance_name)
                .expect("Instance should be in map")
                .clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                zone_manager
                    .add_caa_record(
                        &zone_name,
                        &record_name,
                        flags,
                        &tag,
                        &value,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                    .context(format!(
                        "Failed to add CAA record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})"
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

/// Create a Kubernetes Event for a DNS record.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The DNS record resource
/// * `event_type` - Type of event ("Normal" or "Warning")
/// * `reason` - Short reason for the event
/// * `message` - Human-readable message describing the event
async fn create_event<T>(
    client: &Client,
    record: &T,
    event_type: &str,
    reason: &str,
    message: &str,
) -> Result<()>
where
    T: Resource<DynamicType = ()> + ResourceExt,
{
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();
    let event_api: Api<Event> = Api::namespaced(client.clone(), &namespace);

    let now = Time(Utc::now());
    let event = Event {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            generate_name: Some(format!("{name}-")),
            namespace: Some(namespace.clone()),
            ..Default::default()
        },
        involved_object: ObjectReference {
            api_version: Some(T::api_version(&()).to_string()),
            kind: Some(T::kind(&()).to_string()),
            name: Some(name.clone()),
            namespace: Some(namespace),
            uid: record.meta().uid.clone(),
            ..Default::default()
        },
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
        type_: Some(event_type.to_string()),
        first_timestamp: Some(now.clone()),
        last_timestamp: Some(now),
        count: Some(1),
        ..Default::default()
    };

    match event_api.create(&PostParams::default(), &event).await {
        Ok(_) => Ok(()),
        Err(e) => {
            warn!("Failed to create event for {}: {}", name, e);
            Ok(()) // Don't fail reconciliation if event creation fails
        }
    }
}

/// Updates the status of a DNS record resource.
///
/// Updates the status subresource with appropriate conditions following
/// Kubernetes conventions. Also creates a Kubernetes Event for visibility.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The DNS record resource to update
/// * `condition_type` - Type of condition (e.g., "Ready", "Failed")
/// * `status` - Status value (e.g., "True", "False", "Unknown")
/// * `reason` - Short reason code (e.g., "`ReconcileSucceeded`", "`ZoneNotFound`")
/// * `message` - Human-readable message describing the status
/// * `observed_generation` - Optional generation to set in status (defaults to record's current generation)
///
/// # Errors
///
/// Returns an error if the status update fails.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
async fn update_record_status<T>(
    client: &Client,
    record: &T,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
    observed_generation: Option<i64>,
    record_hash: Option<String>,
    last_updated: Option<String>,
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
    let api: Api<T> = Api::namespaced(client.clone(), &namespace);

    // Fetch current resource to check existing status
    let current = api
        .get(&name)
        .await
        .context("Failed to fetch current resource")?;

    // Check if we need to update
    // Extract status from the current resource using json
    let current_json = serde_json::to_value(&current)?;
    let needs_update = if let Some(current_status) = current_json.get("status") {
        if let Some(observed_gen) = current_status.get("observedGeneration") {
            // If observed generation matches current generation and condition hasn't changed, skip update
            if observed_gen == &json!(record.meta().generation) {
                if let Some(conditions) =
                    current_status.get("conditions").and_then(|c| c.as_array())
                {
                    // Find the condition with matching type (not just first condition)
                    let matching_condition = conditions.iter().find(|cond| {
                        cond.get("type").and_then(|t| t.as_str()) == Some(condition_type)
                    });

                    if let Some(cond) = matching_condition {
                        let status_matches =
                            cond.get("status").and_then(|s| s.as_str()) == Some(status);
                        let reason_matches =
                            cond.get("reason").and_then(|r| r.as_str()) == Some(reason);
                        let message_matches =
                            cond.get("message").and_then(|m| m.as_str()) == Some(message);
                        // Only update if any field has changed
                        !(status_matches && reason_matches && message_matches)
                    } else {
                        true // Condition type not found, need to add it
                    }
                } else {
                    true // No conditions array, need to update
                }
            } else {
                true // Generation changed, need to update
            }
        } else {
            true // No observed generation, need to update
        }
    } else {
        true // No status, need to update
    };

    if !needs_update {
        // Status is already correct, skip update to avoid reconciliation loop
        return Ok(());
    }

    // Determine last_transition_time
    let last_transition_time = if let Some(current_status) = current_json.get("status") {
        if let Some(conditions) = current_status.get("conditions").and_then(|c| c.as_array()) {
            // Find the condition with matching type (same as above)
            let matching_condition = conditions
                .iter()
                .find(|cond| cond.get("type").and_then(|t| t.as_str()) == Some(condition_type));

            if let Some(cond) = matching_condition {
                let status_changed = cond.get("status").and_then(|s| s.as_str()) != Some(status);
                if status_changed {
                    // Status changed, use current time
                    Utc::now().to_rfc3339()
                } else {
                    // Status unchanged, preserve existing timestamp
                    cond.get("lastTransitionTime")
                        .and_then(|t| t.as_str())
                        .unwrap_or(&Utc::now().to_rfc3339())
                        .to_string()
                }
            } else {
                // Condition type not found, use current time
                Utc::now().to_rfc3339()
            }
        } else {
            Utc::now().to_rfc3339()
        }
    } else {
        Utc::now().to_rfc3339()
    };

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(last_transition_time),
    };

    // Preserve existing zone field if it exists (set by DNSZone controller)
    let zone = current_json
        .get("status")
        .and_then(|s| s.get("zone"))
        .and_then(|z| z.as_str())
        .map(ToString::to_string);

    // Preserve existing zone_ref field if it exists (set by DNSZone controller)
    let zone_ref = current_json
        .get("status")
        .and_then(|s| s.get("zoneRef"))
        .and_then(|z| serde_json::from_value::<crate::crd::ZoneReference>(z.clone()).ok());

    #[allow(deprecated)] // Maintain backward compatibility with deprecated zone field
    let record_status = RecordStatus {
        conditions: vec![condition],
        observed_generation: observed_generation.or(record.meta().generation),
        zone,
        zone_ref, // Preserved from existing status (set by DNSZone controller)
        record_hash,
        last_updated,
    };

    let status_patch = json!({
        "status": record_status
    });

    api.patch_status(&name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .context("Failed to update record status")?;

    info!(
        "Updated status for {}/{}: {} = {}",
        namespace, name, condition_type, status
    );

    // Create event for visibility
    let event_type = if status == "True" {
        "Normal"
    } else {
        "Warning"
    };
    create_event(client, record, event_type, reason, message).await?;

    Ok(())
}

/// Generic function to delete a DNS record from BIND9 primaries.
///
/// This function handles deletion of any record type using the generic approach:
/// 1. Gets the zone reference from the record's status
/// 2. Looks up the `DNSZone` to get instances
/// 3. Filters to primary instances
/// 4. Deletes the record from all primaries
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The DNS record resource being deleted
/// * `record_type` - Human-readable record type (e.g., "A", "TXT")
/// * `record_type_hickory` - hickory-client `RecordType` enum value
/// * `zone_name` - The DNS zone name
/// * `record_name` - The DNS record name
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
    record_type_hickory: hickory_client::rr::RecordType,
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
    let status = serde_json::to_value(record)
        .ok()
        .and_then(|v| v.get("status").cloned());

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
    let instance_refs = match crate::reconcilers::dnszone::get_instances_from_zone(
        &dnszone,
        &stores.bind9_instances,
    ) {
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
    let primary_refs = match crate::reconcilers::dnszone::filter_primary_instances(
        client,
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

    // Delete record from all primaries
    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = primary_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first_endpoint, total_endpoints) =
        crate::reconcilers::dnszone::for_each_instance_endpoint(
            client,
            &primary_refs,
            true,      // with_rndc_key
            "dns-tcp", // Use DNS TCP port for dynamic updates
            |pod_endpoint, instance_name, rndc_key| {
                let zone_name = zone_ref.zone_name.clone();
                let record_name_str = if let Some(record_spec) = serde_json::to_value(record)
                    .ok()
                    .and_then(|v| v.get("spec").cloned())
                {
                    record_spec
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or(&name)
                        .to_string()
                } else {
                    name.clone()
                };
                let instance_namespace = instance_map
                    .get(&instance_name)
                    .expect("Instance should be in map")
                    .clone();

                // Create Bind9Manager for this specific instance with deployment-aware auth
                let zone_manager = stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

                async move {
                    let key_data = rndc_key.expect("RNDC key should be loaded");

                    // Attempt to delete - if it fails, log warning but don't fail the deletion
                    if let Err(e) = zone_manager
                        .delete_record(
                            &zone_name,
                            &record_name_str,
                            record_type_hickory,
                            &pod_endpoint,
                            &key_data,
                        )
                        .await
                    {
                        warn!(
                            "Failed to delete {} record {}.{} from endpoint {} (instance: {}): {}. Continuing with deletion anyway.",
                            record_type, record_name_str, zone_name, pod_endpoint, instance_name, e
                        );
                    } else {
                        info!(
                            "Successfully deleted {} record {}.{} from endpoint {} (instance: {})",
                            record_type, record_name_str, zone_name, pod_endpoint, instance_name
                        );
                    }

                    Ok(())
                }
            },
        )
        .await?;

    info!(
        "Successfully deleted {} record {}/{} from {} primary endpoint(s)",
        record_type, namespace, name, total_endpoints
    );

    Ok(())
}

/// Update lastReconciledAt timestamp for a record in DNSZone.status.selectedRecords[].
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
/// - Record is not found in zone's `selectedRecords[]` array
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
                record_ref.last_reconciled_at = Some(Time(Utc::now()));
                found = true;
                break;
            }
        }
    }

    if !found {
        warn!(
            "Record {} {}/{} not found in DNSZone {}/{} selectedRecords[] - cannot update timestamp",
            record_kind, record_namespace, record_name, zone_namespace, zone_name
        );
        return Ok(());
    }

    // Patch the status with updated timestamp
    let status_patch = json!({
        "status": {
            "selectedRecords": zone.status.as_ref().map(|s| &s.records)
        }
    });

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
