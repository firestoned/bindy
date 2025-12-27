// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! DNS record reconciliation logic.
//!
//! This module contains reconcilers for all DNS record types supported by Bindy.
//!
//! **IMPORTANT**: With the zone ownership model, DNS record reconcilers:
//! 1. Read the `bindy.firestoned.io/zone` annotation set by the `DNSZone` controller
//! 2. If annotation is present, create/update the record in BIND9 for that zone
//! 3. If annotation is absent, the record is not selected by any zone (skip reconciliation)
//! 4. Update the record's status to reflect success or failure

use crate::constants::ANNOTATION_ZONE_OWNER;
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, Condition, DNSZone, MXRecord, NSRecord,
    RecordStatus, SRVRecord, TXTRecord,
};
use anyhow::{anyhow, Context, Result};
use k8s_openapi::api::core::v1::{Event, ObjectReference};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::chrono::Utc;
use kube::{
    api::{ListParams, Patch, PatchParams, PostParams},
    client::Client,
    Api, Resource, ResourceExt,
};
use serde_json::json;
use tracing::{debug, info, warn};

/// Retrieves the zone FQDN from the record's `bindy.firestoned.io/zone` annotation.
///
/// This annotation is set by the `DNSZone` controller when a zone's label selector
/// matches this record. The record reconciler uses this to determine which zone
/// to update in BIND9.
///
/// # Arguments
///
/// * `record` - The DNS record resource
///
/// # Returns
///
/// * `Some(zone_fqdn)` - If the annotation is present and non-empty
/// * `None` - If the annotation is missing or empty (record not selected by any zone)
fn get_zone_from_annotation<T: ResourceExt>(record: &T) -> Option<String> {
    record
        .annotations()
        .get(ANNOTATION_ZONE_OWNER)
        .filter(|zone| !zone.is_empty())
        .cloned()
}

/// Gets zone information from the annotation and looks up the `DNSZone` resource.
///
/// This function reads the `bindy.firestoned.io/zone` annotation set by the `DNSZone`
/// controller, then queries the Kubernetes API to get the zone's cluster reference.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the record
/// * `zone_fqdn` - Zone FQDN from the annotation
///
/// # Returns
///
/// A tuple of (`zone_name`, `cluster_ref`, `is_cluster_provider`)
///
/// # Errors
///
/// Returns an error if the `DNSZone` resource cannot be found or queried.
async fn get_zone_info(
    client: &Client,
    namespace: &str,
    zone_fqdn: &str,
) -> Result<(String, String, bool)> {
    let dns_zones_api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

    // List all DNSZones and find the one with matching zoneName
    let zones = dns_zones_api.list(&ListParams::default()).await?;

    for zone in zones {
        if zone.spec.zone_name == zone_fqdn {
            // Determine cluster reference
            let (cluster_ref, is_cluster_provider) =
                if let Some(ref cluster) = zone.spec.cluster_ref {
                    (cluster.clone(), false)
                } else if let Some(ref provider) = zone.spec.cluster_provider_ref {
                    (provider.clone(), true)
                } else {
                    return Err(anyhow!(
                        "DNSZone {}/{} has neither clusterRef nor clusterProviderRef",
                        namespace,
                        zone.name_any()
                    ));
                };

            return Ok((zone_fqdn.to_string(), cluster_ref, is_cluster_provider));
        }
    }

    Err(anyhow!(
        "DNSZone with zoneName '{zone_fqdn}' not found in namespace '{namespace}'"
    ))
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
/// use kube::Client;
///
/// async fn handle_a_record(record: ARecord) -> anyhow::Result<()> {
///     let client = Client::try_default().await?;
///     reconcile_a_record(client, record).await?;
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_a_record(client: Client, record: ARecord) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling ARecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Get zone from annotation (set by DNSZone controller)
    // Check this FIRST before generation check, because the annotation may have been
    // added after the record was created (by DNSZone controller)
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
            debug!("Spec unchanged and no zone annotation, skipping reconciliation");
            return Ok(());
        }

        info!(
            "A record {}/{} not selected by any DNSZone (no zone annotation)",
            namespace, name
        );
        update_record_status(
            &client,
            &record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone label selector",
            current_generation,
        )
        .await?;
        return Ok(());
    };

    // Always reconcile to ensure declarative state - records are recreated if pods restart
    // The underlying add_*_record() functions are idempotent and check for existence first
    debug!(
        "Ensuring A record exists in zone {} (declarative reconciliation)",
        zone_fqdn
    );

    // Get zone information from Kubernetes
    let (zone_name, cluster_ref, is_cluster_provider) =
        match get_zone_info(&client, &namespace, &zone_fqdn).await {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "Failed to find DNSZone for {} in {}/{}: {}",
                    zone_fqdn, namespace, name, e
                );
                update_record_status(
                    &client,
                    &record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &format!("DNSZone '{zone_fqdn}' not found: {e}"),
                    current_generation,
                )
                .await?;
                return Ok(());
            }
        };

    // Create/update record in BIND9 for the zone
    let zone_manager = crate::bind9::Bind9Manager::new();

    match add_a_record_to_zone(
        &client,
        &namespace,
        &zone_name,
        &cluster_ref,
        is_cluster_provider,
        &spec.name,
        &spec.ipv4_address,
        spec.ttl,
        &zone_manager,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added A record {} to zone {} in cluster {}",
                spec.name, zone_name, cluster_ref
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("A record added to zone {zone_name}"),
                current_generation,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add A record {} to zone {}: {}",
                spec.name, zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone {zone_name}: {e}"),
                current_generation,
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an A record to a specific zone in BIND9 primaries.
///
/// Uses dynamic DNS updates (nsupdate protocol via DNS TCP port 53) to add the record
/// to all primary endpoints for the specified zone.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the zone
/// * `zone_name` - DNS zone name
/// * `cluster_ref` - Name of the `Bind9Cluster` or `ClusterBind9Provider`
/// * `is_cluster_provider` - Whether the cluster is a `ClusterBind9Provider`
/// * `record_name` - Name portion of the DNS record
/// * `ipv4_address` - IPv4 address for the record
/// * `ttl` - Optional TTL value
/// * `zone_manager` - BIND9 manager instance
///
/// # Errors
///
/// Returns an error if the BIND9 record creation fails.
#[allow(clippy::too_many_arguments)]
async fn add_a_record_to_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    record_name: &str,
    ipv4_address: &str,
    ttl: Option<i32>,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_primary_endpoint;

    let (_first, _total) = for_each_primary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let ipv4_address = ipv4_address.to_string();
            let zone_manager = zone_manager.clone();

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
#[allow(clippy::too_many_lines)]
pub async fn reconcile_txt_record(client: Client, record: TXTRecord) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling TXTRecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Get zone from annotation (set by DNSZone controller)
    // Check this FIRST before generation check, because the annotation may have been
    // added after the record was created (by DNSZone controller)
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
            debug!("Spec unchanged and no zone annotation, skipping reconciliation");
            return Ok(());
        }

        info!(
            "TXT record {}/{} not selected by any DNSZone",
            namespace, name
        );
        update_record_status(
            &client,
            &record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone label selector",
            current_generation,
        )
        .await?;
        return Ok(());
    };

    // Always reconcile to ensure declarative state - records are recreated if pods restart
    // The underlying add_*_record() functions are idempotent and check for existence first
    debug!(
        "Ensuring record exists in zone {} (declarative reconciliation)",
        zone_fqdn
    );

    // Get zone info from DNSZone resource
    let (zone_name, cluster_ref, is_cluster_provider) =
        match get_zone_info(&client, &namespace, &zone_fqdn).await {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "Failed to find DNSZone {} for TXT record {}/{}: {}",
                    zone_fqdn, namespace, name, e
                );
                update_record_status(
                    &client,
                    &record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &format!("DNSZone {zone_fqdn} not found: {e}"),
                    current_generation,
                )
                .await?;
                return Ok(());
            }
        };

    // Create/update record in BIND9
    let zone_manager = crate::bind9::Bind9Manager::new();

    match add_txt_record_to_zone(
        &client,
        &namespace,
        &zone_name,
        &cluster_ref,
        is_cluster_provider,
        &spec.name,
        &spec.text,
        spec.ttl,
        &zone_manager,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added TXT record {} to zone {} in cluster {}",
                spec.name, zone_name, cluster_ref
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "RecordAvailable",
                &format!(
                    "TXT record {} successfully added to zone {zone_name}",
                    spec.name
                ),
                current_generation,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add TXT record {} to zone {}: {}",
                spec.name, zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add TXT record to zone {zone_name}: {e}"),
                current_generation,
            )
            .await?;
        }
    }

    Ok(())
}

/// Add a TXT record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_txt_record_to_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    record_name: &str,
    text: &[String],
    ttl: Option<i32>,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_primary_endpoint;

    let (_first, _total) = for_each_primary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        true,
        "dns-tcp",
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let text = text.to_vec();
            let zone_manager = zone_manager.clone();

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
#[allow(clippy::too_many_lines)]
pub async fn reconcile_aaaa_record(client: Client, record: AAAARecord) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling AAAARecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Get zone from annotation (set by DNSZone controller)
    // Check this FIRST before generation check, because the annotation may have been
    // added after the record was created (by DNSZone controller)
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
            debug!("Spec unchanged and no zone annotation, skipping reconciliation");
            return Ok(());
        }

        info!(
            "AAAA record {}/{} not selected by any DNSZone (no zone annotation)",
            namespace, name
        );
        update_record_status(
            &client,
            &record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone label selector",
            current_generation,
        )
        .await?;
        return Ok(());
    };

    // Always reconcile to ensure declarative state - records are recreated if pods restart
    // The underlying add_*_record() functions are idempotent and check for existence first
    debug!(
        "Ensuring record exists in zone {} (declarative reconciliation)",
        zone_fqdn
    );

    // Get zone information from Kubernetes
    let (zone_name, cluster_ref, is_cluster_provider) =
        match get_zone_info(&client, &namespace, &zone_fqdn).await {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "Failed to find DNSZone for {} in {}/{}: {}",
                    zone_fqdn, namespace, name, e
                );
                update_record_status(
                    &client,
                    &record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &format!("DNSZone '{zone_fqdn}' not found: {e}"),
                    current_generation,
                )
                .await?;
                return Ok(());
            }
        };

    // Create/update record in BIND9 for the zone
    let zone_manager = crate::bind9::Bind9Manager::new();

    match add_aaaa_record_to_zone(
        &client,
        &namespace,
        &zone_name,
        &cluster_ref,
        is_cluster_provider,
        &spec.name,
        &spec.ipv6_address,
        spec.ttl,
        &zone_manager,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added AAAA record {} to zone {} in cluster {}",
                spec.name, zone_name, cluster_ref
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "ReconcileSucceeded",
                &format!("AAAA record added to zone {zone_name}"),
                current_generation,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add AAAA record {} to zone {}: {}",
                spec.name, zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add record to zone {zone_name}: {e}"),
                current_generation,
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an AAAA record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_aaaa_record_to_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    record_name: &str,
    ipv6_address: &str,
    ttl: Option<i32>,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_primary_endpoint;

    let (_first, _total) = for_each_primary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        true,
        "dns-tcp",
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let ipv6_address = ipv6_address.to_string();
            let zone_manager = zone_manager.clone();

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
pub async fn reconcile_cname_record(client: Client, record: CNAMERecord) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CNAMERecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Get zone from annotation (set by DNSZone controller)
    // Check this FIRST before generation check, because the annotation may have been
    // added after the record was created (by DNSZone controller)
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
            debug!("Spec unchanged and no zone annotation, skipping reconciliation");
            return Ok(());
        }

        info!(
            "CNAME record {}/{} not selected by any DNSZone",
            namespace, name
        );
        update_record_status(
            &client,
            &record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone label selector",
            current_generation,
        )
        .await?;
        return Ok(());
    };

    // Always reconcile to ensure declarative state - records are recreated if pods restart
    // The underlying add_*_record() functions are idempotent and check for existence first
    debug!(
        "Ensuring record exists in zone {} (declarative reconciliation)",
        zone_fqdn
    );

    // Get zone info from DNSZone resource
    let (zone_name, cluster_ref, is_cluster_provider) =
        match get_zone_info(&client, &namespace, &zone_fqdn).await {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "Failed to find DNSZone {} for CNAME record {}/{}: {}",
                    zone_fqdn, namespace, name, e
                );
                update_record_status(
                    &client,
                    &record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &format!("DNSZone {zone_fqdn} not found: {e}"),
                    current_generation,
                )
                .await?;
                return Ok(());
            }
        };

    // Create/update record in BIND9
    let zone_manager = crate::bind9::Bind9Manager::new();

    match add_cname_record_to_zone(
        &client,
        &namespace,
        &zone_name,
        &cluster_ref,
        is_cluster_provider,
        &spec.name,
        &spec.target,
        spec.ttl,
        &zone_manager,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added CNAME record {} to zone {} in cluster {}",
                spec.name, zone_name, cluster_ref
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "RecordAvailable",
                &format!(
                    "CNAME record {} successfully added to zone {zone_name}",
                    spec.name
                ),
                current_generation,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add CNAME record {} to zone {}: {}",
                spec.name, zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add CNAME record to zone {zone_name}: {e}"),
                current_generation,
            )
            .await?;
        }
    }

    Ok(())
}

/// Add a CNAME record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_cname_record_to_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    record_name: &str,
    target: &str,
    ttl: Option<i32>,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_primary_endpoint;

    let (_first, _total) = for_each_primary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        true,
        "dns-tcp",
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let target = target.to_string();
            let zone_manager = zone_manager.clone();

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
pub async fn reconcile_mx_record(client: Client, record: MXRecord) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling MXRecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Get zone from annotation (set by DNSZone controller)
    // Check this FIRST before generation check, because the annotation may have been
    // added after the record was created (by DNSZone controller)
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
            debug!("Spec unchanged and no zone annotation, skipping reconciliation");
            return Ok(());
        }

        info!(
            "MX record {}/{} not selected by any DNSZone",
            namespace, name
        );
        update_record_status(
            &client,
            &record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone label selector",
            current_generation,
        )
        .await?;
        return Ok(());
    };

    // Always reconcile to ensure declarative state - records are recreated if pods restart
    // The underlying add_*_record() functions are idempotent and check for existence first
    debug!(
        "Ensuring record exists in zone {} (declarative reconciliation)",
        zone_fqdn
    );

    // Get zone info from DNSZone resource
    let (zone_name, cluster_ref, is_cluster_provider) =
        match get_zone_info(&client, &namespace, &zone_fqdn).await {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "Failed to find DNSZone {} for MX record {}/{}: {}",
                    zone_fqdn, namespace, name, e
                );
                update_record_status(
                    &client,
                    &record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &format!("DNSZone {zone_fqdn} not found: {e}"),
                    current_generation,
                )
                .await?;
                return Ok(());
            }
        };

    // Create/update record in BIND9
    let zone_manager = crate::bind9::Bind9Manager::new();

    match add_mx_record_to_zone(
        &client,
        &namespace,
        &zone_name,
        &cluster_ref,
        is_cluster_provider,
        &spec.name,
        spec.priority,
        &spec.mail_server,
        spec.ttl,
        &zone_manager,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added MX record {} to zone {} in cluster {}",
                spec.name, zone_name, cluster_ref
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "RecordAvailable",
                &format!(
                    "MX record {} successfully added to zone {zone_name}",
                    spec.name
                ),
                current_generation,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add MX record {} to zone {}: {}",
                spec.name, zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add MX record to zone {zone_name}: {e}"),
                current_generation,
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an MX record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_mx_record_to_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    record_name: &str,
    priority: i32,
    mail_server: &str,
    ttl: Option<i32>,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_primary_endpoint;

    let (_first, _total) = for_each_primary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        true,
        "dns-tcp",
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let mail_server = mail_server.to_string();
            let zone_manager = zone_manager.clone();

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
pub async fn reconcile_ns_record(client: Client, record: NSRecord) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling NSRecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Get zone from annotation (set by DNSZone controller)
    // Check this FIRST before generation check, because the annotation may have been
    // added after the record was created (by DNSZone controller)
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
            debug!("Spec unchanged and no zone annotation, skipping reconciliation");
            return Ok(());
        }

        info!(
            "NS record {}/{} not selected by any DNSZone",
            namespace, name
        );
        update_record_status(
            &client,
            &record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone label selector",
            current_generation,
        )
        .await?;
        return Ok(());
    };

    // Always reconcile to ensure declarative state - records are recreated if pods restart
    // The underlying add_*_record() functions are idempotent and check for existence first
    debug!(
        "Ensuring record exists in zone {} (declarative reconciliation)",
        zone_fqdn
    );

    // Get zone info from DNSZone resource
    let (zone_name, cluster_ref, is_cluster_provider) =
        match get_zone_info(&client, &namespace, &zone_fqdn).await {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "Failed to find DNSZone {} for NS record {}/{}: {}",
                    zone_fqdn, namespace, name, e
                );
                update_record_status(
                    &client,
                    &record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &format!("DNSZone {zone_fqdn} not found: {e}"),
                    current_generation,
                )
                .await?;
                return Ok(());
            }
        };

    // Create/update record in BIND9
    let zone_manager = crate::bind9::Bind9Manager::new();

    match add_ns_record_to_zone(
        &client,
        &namespace,
        &zone_name,
        &cluster_ref,
        is_cluster_provider,
        &spec.name,
        &spec.nameserver,
        spec.ttl,
        &zone_manager,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added NS record {} to zone {} in cluster {}",
                spec.name, zone_name, cluster_ref
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "RecordAvailable",
                &format!(
                    "NS record {} successfully added to zone {zone_name}",
                    spec.name
                ),
                current_generation,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add NS record {} to zone {}: {}",
                spec.name, zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add NS record to zone {zone_name}: {e}"),
                current_generation,
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an NS record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_ns_record_to_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    record_name: &str,
    nameserver: &str,
    ttl: Option<i32>,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_primary_endpoint;

    let (_first, _total) = for_each_primary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        true,
        "dns-tcp",
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let nameserver = nameserver.to_string();
            let zone_manager = zone_manager.clone();

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
pub async fn reconcile_srv_record(client: Client, record: SRVRecord) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling SRVRecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Get zone from annotation (set by DNSZone controller)
    // Check this FIRST before generation check, because the annotation may have been
    // added after the record was created (by DNSZone controller)
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
            debug!("Spec unchanged and no zone annotation, skipping reconciliation");
            return Ok(());
        }

        info!(
            "SRV record {}/{} not selected by any DNSZone",
            namespace, name
        );
        update_record_status(
            &client,
            &record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone label selector",
            current_generation,
        )
        .await?;
        return Ok(());
    };

    // Always reconcile to ensure declarative state - records are recreated if pods restart
    // The underlying add_*_record() functions are idempotent and check for existence first
    debug!(
        "Ensuring record exists in zone {} (declarative reconciliation)",
        zone_fqdn
    );

    // Get zone info from DNSZone resource
    let (zone_name, cluster_ref, is_cluster_provider) =
        match get_zone_info(&client, &namespace, &zone_fqdn).await {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "Failed to find DNSZone {} for SRV record {}/{}: {}",
                    zone_fqdn, namespace, name, e
                );
                update_record_status(
                    &client,
                    &record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &format!("DNSZone {zone_fqdn} not found: {e}"),
                    current_generation,
                )
                .await?;
                return Ok(());
            }
        };

    // Create/update record in BIND9
    let zone_manager = crate::bind9::Bind9Manager::new();

    match add_srv_record_to_zone(
        &client,
        &namespace,
        &zone_name,
        &cluster_ref,
        is_cluster_provider,
        &spec.name,
        spec.priority,
        spec.weight,
        spec.port,
        &spec.target,
        spec.ttl,
        &zone_manager,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added SRV record {} to zone {} in cluster {}",
                spec.name, zone_name, cluster_ref
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "RecordAvailable",
                &format!(
                    "SRV record {} successfully added to zone {zone_name}",
                    spec.name
                ),
                current_generation,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add SRV record {} to zone {}: {}",
                spec.name, zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add SRV record to zone {zone_name}: {e}"),
                current_generation,
            )
            .await?;
        }
    }

    Ok(())
}

/// Add an SRV record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_srv_record_to_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    record_name: &str,
    priority: i32,
    weight: i32,
    port: i32,
    target: &str,
    ttl: Option<i32>,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    use crate::bind9::types::SRVRecordData;
    use crate::reconcilers::dnszone::for_each_primary_endpoint;

    let (_first, _total) = for_each_primary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        true,
        "dns-tcp",
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
            let zone_manager = zone_manager.clone();

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
pub async fn reconcile_caa_record(client: Client, record: CAARecord) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CAARecord: {}/{}", namespace, name);

    let spec = &record.spec;
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Get zone from annotation (set by DNSZone controller)
    // Check this FIRST before generation check, because the annotation may have been
    // added after the record was created (by DNSZone controller)
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        // Only skip reconciliation if generation hasn't changed AND already marked as NotSelected
        if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
            debug!("Spec unchanged and no zone annotation, skipping reconciliation");
            return Ok(());
        }

        info!(
            "CAA record {}/{} not selected by any DNSZone",
            namespace, name
        );
        update_record_status(
            &client,
            &record,
            "Ready",
            "False",
            "NotSelected",
            "Record not selected by any DNSZone label selector",
            current_generation,
        )
        .await?;
        return Ok(());
    };

    // Always reconcile to ensure declarative state - records are recreated if pods restart
    // The underlying add_*_record() functions are idempotent and check for existence first
    debug!(
        "Ensuring record exists in zone {} (declarative reconciliation)",
        zone_fqdn
    );

    // Get zone info from DNSZone resource
    let (zone_name, cluster_ref, is_cluster_provider) =
        match get_zone_info(&client, &namespace, &zone_fqdn).await {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "Failed to find DNSZone {} for CAA record {}/{}: {}",
                    zone_fqdn, namespace, name, e
                );
                update_record_status(
                    &client,
                    &record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &format!("DNSZone {zone_fqdn} not found: {e}"),
                    current_generation,
                )
                .await?;
                return Ok(());
            }
        };

    // Create/update record in BIND9
    let zone_manager = crate::bind9::Bind9Manager::new();

    match add_caa_record_to_zone(
        &client,
        &namespace,
        &zone_name,
        &cluster_ref,
        is_cluster_provider,
        &spec.name,
        spec.flags,
        &spec.tag,
        &spec.value,
        spec.ttl,
        &zone_manager,
    )
    .await
    {
        Ok(()) => {
            info!(
                "Successfully added CAA record {} to zone {} in cluster {}",
                spec.name, zone_name, cluster_ref
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "True",
                "RecordAvailable",
                &format!(
                    "CAA record {} successfully added to zone {zone_name}",
                    spec.name
                ),
                current_generation,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to add CAA record {} to zone {}: {}",
                spec.name, zone_name, e
            );
            update_record_status(
                &client,
                &record,
                "Ready",
                "False",
                "ReconcileFailed",
                &format!("Failed to add CAA record to zone {zone_name}: {e}"),
                current_generation,
            )
            .await?;
        }
    }

    Ok(())
}

/// Add a CAA record to a specific zone in BIND9 primaries.
#[allow(clippy::too_many_arguments)]
async fn add_caa_record_to_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    record_name: &str,
    flags: i32,
    tag: &str,
    value: &str,
    ttl: Option<i32>,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    use crate::reconcilers::dnszone::for_each_primary_endpoint;

    let (_first, _total) = for_each_primary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        true,
        "dns-tcp",
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();
            let tag = tag.to_string();
            let value = value.to_string();
            let zone_manager = zone_manager.clone();

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
#[allow(clippy::too_many_lines)]
async fn update_record_status<T>(
    client: &Client,
    record: &T,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
    observed_generation: Option<i64>,
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

    let record_status = RecordStatus {
        conditions: vec![condition],
        observed_generation: observed_generation.or(record.meta().generation),
        zone,
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
