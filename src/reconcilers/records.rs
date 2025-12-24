// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! DNS record reconciliation logic.
//!
//! This module contains reconcilers for all DNS record types supported by Bindy.
//! Each reconciler adds or updates records in the appropriate zone file.

use crate::bind9::RndcKeyData;
use crate::constants::{
    API_GROUP_VERSION, KIND_AAAA_RECORD, KIND_A_RECORD, KIND_CAA_RECORD, KIND_CNAME_RECORD,
    KIND_MX_RECORD, KIND_NS_RECORD, KIND_SRV_RECORD, KIND_TXT_RECORD,
};
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, Condition, DNSZone, MXRecord, NSRecord,
    RecordStatus, SRVRecord, TXTRecord,
};
use crate::labels::{BINDY_CLUSTER_ANNOTATION, BINDY_INSTANCE_ANNOTATION, BINDY_ZONE_ANNOTATION};
use anyhow::{anyhow, Context, Result};
use k8s_openapi::api::core::v1::{Event, ObjectReference};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::chrono::Utc;
use kube::{
    api::{Patch, PatchParams, PostParams},
    client::Client,
    Api, Resource, ResourceExt,
};
use serde_json::json;
use tracing::{debug, error, info, warn};

/// Helper macro to handle zone lookup with proper error reporting
///
/// Returns a tuple of (`cluster_ref`, `zone_name`, `is_cluster_provider`) from the `DNSZone` lookup
macro_rules! get_zone_or_fail {
    ($client:expr, $namespace:expr, $zone_ref:expr, $record:expr) => {
        match get_zone_info($client, $namespace, $zone_ref).await {
            Ok((cluster_ref, zone_name, is_cluster_provider)) => {
                (cluster_ref, zone_name, is_cluster_provider)
            }
            Err(e) => {
                let error_msg = format!(
                    "Failed to lookup DNSZone '{}' in namespace {}: {}",
                    $zone_ref, $namespace, e
                );
                error!("{}", error_msg);
                update_record_status(
                    $client,
                    $record,
                    "Ready",
                    "False",
                    "ZoneNotFound",
                    &error_msg,
                )
                .await?;
                return Err(anyhow!(error_msg));
            }
        }
    };
}

/// Adds tracking annotations to a DNS record resource.
///
/// Annotations are added to help track which `Bind9Cluster`, `Bind9Instance`, and `DNSZone`
/// this record is associated with. This aids in debugging and resource management.
///
/// **IMPORTANT**: Only patches if annotations are missing or differ to avoid reconciliation loops.
///
/// # Arguments
///
/// * `_client` - Kubernetes API client (unused, reserved for future use)
/// * `api` - API client for the specific record type
/// * `record_name` - Name of the record resource
/// * `cluster_ref` - Name of the `Bind9Cluster`
/// * `instance_name` - Name of the `Bind9Instance` being used
/// * `zone_name` - Name of the DNS zone
///
/// # Errors
///
/// Returns an error if the Kubernetes API patch operation fails.
async fn add_record_annotations<T>(
    _client: &Client,
    api: &Api<T>,
    record_name: &str,
    cluster_ref: &str,
    instance_name: &str,
    zone_name: &str,
) -> Result<()>
where
    T: Resource + Clone + std::fmt::Debug + serde::de::DeserializeOwned + serde::Serialize,
{
    // Fetch current record to check if annotations already exist
    let record = api.get(record_name).await?;

    // Check if annotations are already set with correct values
    let needs_update = if let Some(metadata) = record.meta().annotations.as_ref() {
        metadata.get(BINDY_CLUSTER_ANNOTATION) != Some(&cluster_ref.to_string())
            || metadata.get(BINDY_INSTANCE_ANNOTATION) != Some(&instance_name.to_string())
            || metadata.get(BINDY_ZONE_ANNOTATION) != Some(&zone_name.to_string())
    } else {
        // No annotations at all
        true
    };

    // Only patch if annotations are missing or differ
    if !needs_update {
        debug!(
            record = record_name,
            "Tracking annotations already present and correct, skipping update"
        );
        return Ok(());
    }

    let patch = json!({
        "metadata": {
            "annotations": {
                BINDY_CLUSTER_ANNOTATION: cluster_ref,
                BINDY_INSTANCE_ANNOTATION: instance_name,
                BINDY_ZONE_ANNOTATION: zone_name,
            }
        }
    });

    api.patch(record_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .context("Failed to add annotations to record")?;

    debug!(
        record = record_name,
        cluster = cluster_ref,
        instance = instance_name,
        zone = zone_name,
        "Added tracking annotations to DNS record"
    );

    Ok(())
}

/// Generic helper to add a DNS record to ALL primary pods across ALL instances.
///
/// This function handles the common pattern of:
/// 1. Getting the zone info
/// 2. Iterating through all primary endpoints
/// 3. Calling a record-specific add operation for each endpoint
/// 4. Updating status and annotations
/// 5. Notifying secondaries
///
/// # Type Parameters
///
/// * `R` - The record type (`ARecord`, `TXTRecord`, etc.)
/// * `F` - Async function type for the record-specific add operation
/// * `Fut` - Future returned by the add operation
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The record resource to reconcile
/// * `zone_manager` - BIND9 manager for DNS operations
/// * `record_type_name` - Human-readable record type name (e.g., "A", "TXT")
/// * `add_operation` - Async closure that adds the record to a single endpoint
///   - Arguments: `(zone_manager, zone_name, pod_endpoint, key_data)`
///   - Returns: `Result<String>` where String is a description of what was added
///
/// # Returns
///
/// Returns `Ok(usize)` with the number of endpoints successfully configured
///
/// # Errors
///
/// Returns error if any endpoint operation fails
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
async fn add_record_to_all_endpoints<R, F, Fut>(
    client: &Client,
    record: &R,
    zone_manager: &crate::bind9::Bind9Manager,
    record_type_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    zone_name: &str,
    add_operation: F,
) -> Result<usize>
where
    R: Resource<DynamicType = (), Scope = k8s_openapi::NamespaceResourceScope>
        + Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned
        + serde::Serialize,
    F: Fn(crate::bind9::Bind9Manager, String, String, RndcKeyData) -> Fut + Clone,
    Fut: std::future::Future<Output = Result<String>>,
{
    let namespace = record.namespace().unwrap_or_default();

    // First, check if the zone exists before attempting DNS UPDATE operations
    // This prevents NOTAUTH errors when the zone hasn't been loaded by BIND9 yet
    let primary_pods =
        super::dnszone::find_all_primary_pods(client, &namespace, cluster_ref, is_cluster_provider)
            .await?;

    if primary_pods.is_empty() {
        return Err(anyhow::anyhow!(
            "No primary servers found for cluster {cluster_ref} - cannot add {record_type_name} record"
        ));
    }

    // Check if zone exists on the first primary pod
    // Use HTTP API endpoint for zone status check
    let first_pod = &primary_pods[0];

    // Get HTTP endpoint for zone existence check
    // Don't hardcode port - retrieve from service endpoint definition
    let http_endpoints = super::dnszone::get_endpoint(
        client,
        &first_pod.namespace,
        &first_pod.instance_name,
        "http", // Port name for HTTP API
    )
    .await?;

    let http_endpoint = if let Some(endpoint) = http_endpoints.first() {
        format!("{}:{}", endpoint.ip, endpoint.port)
    } else {
        return Err(anyhow::anyhow!(
            "No HTTP endpoint found for instance {}/{} - cannot check zone existence",
            first_pod.namespace,
            first_pod.instance_name
        ));
    };

    debug!(
        "Checking if zone {} exists on {} before adding {} record",
        zone_name, http_endpoint, record_type_name
    );

    if !zone_manager.zone_exists(zone_name, &http_endpoint).await {
        warn!(
            "Zone {} does not exist yet on {}, skipping {} record reconciliation (will retry on next reconciliation loop)",
            zone_name, http_endpoint, record_type_name
        );
        return Ok(0); // Return 0 endpoints updated, will retry later
    }

    info!(
        "Zone {} exists on {}, proceeding with {} record reconciliation",
        zone_name, http_endpoint, record_type_name
    );

    // Add the record to ALL primary pods across ALL instances
    // With EmptyDir storage, each pod maintains its own zone files
    let zone_name_clone = zone_name.to_string();
    let zone_manager_clone = zone_manager.clone();

    let (_first_endpoint, total_endpoints) = super::dnszone::for_each_primary_endpoint(
        client,
        &namespace,
        cluster_ref,
        is_cluster_provider,
        true, // with_rndc_key = true for record operations
        "dns-tcp", // Use DNS TCP port for dynamic DNS updates (RFC 2136)
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name_clone.clone();
            let zone_manager = zone_manager_clone.clone();
            let add_op = add_operation.clone();
            let pod_endpoint_clone = pod_endpoint.clone();
            let record_type = record_type_name.to_string();

            async move {
                // SAFETY: RNDC key is guaranteed to be Some when with_rndc_key=true
                let key_data = rndc_key.expect("RNDC key should be loaded");

                info!(
                    "Reconciling {} record in zone {} at endpoint {} (instance: {})",
                    record_type, zone_name, pod_endpoint, instance_name
                );

                let description = add_op(
                    zone_manager,
                    zone_name.clone(),
                    pod_endpoint.clone(),
                    key_data,
                )
                .await
                .context(format!(
                    "Failed to reconcile {record_type} record in zone {zone_name} at endpoint {pod_endpoint}"
                ))?;

                debug!(
                    "Successfully reconciled {} record in zone {} at endpoint {}: {}",
                    record_type, zone_name, pod_endpoint_clone, description
                );

                Ok(())
            }
        },
    )
    .await?;

    info!(
        "Successfully reconciled {} record in zone {} at {} endpoint(s) for cluster {}",
        record_type_name, zone_name, total_endpoints, cluster_ref
    );

    // Notify secondaries about the record change via HTTP API
    // Don't reuse DNS endpoint (port 53) - get HTTP endpoint (port name "http")
    if !primary_pods.is_empty() {
        let first_pod = &primary_pods[0];

        // Get HTTP endpoint for notify operation
        let http_endpoints = super::dnszone::get_endpoint(
            client,
            &first_pod.namespace,
            &first_pod.instance_name,
            "http", // Port name for HTTP API
        )
        .await;

        match http_endpoints {
            Ok(endpoints) if !endpoints.is_empty() => {
                let http_endpoint = format!("{}:{}", endpoints[0].ip, endpoints[0].port);
                if let Err(e) = zone_manager.notify_zone(zone_name, &http_endpoint).await {
                    warn!(
                        "Failed to notify secondaries for zone {}: {}. Secondaries will sync via SOA refresh timer.",
                        zone_name, e
                    );
                }
            }
            Ok(_) => {
                warn!(
                    "No HTTP endpoint found for instance {}/{}, cannot notify secondaries for zone {}. Secondaries will sync via SOA refresh timer.",
                    first_pod.namespace, first_pod.instance_name, zone_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to get HTTP endpoint for notify operation: {}. Secondaries will sync via SOA refresh timer.",
                    e
                );
            }
        }
    }

    Ok(total_endpoints)
}

/// Reconciles an `ARecord` (IPv4 address) resource.
///
/// Adds or updates an A record in the specified zone file.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The `ARecord` resource to reconcile
/// * `zone_manager` - BIND9 manager for updating zone files
///
/// # Example
///
/// ```rust,no_run
/// use bindy::reconcilers::reconcile_a_record;
/// use bindy::crd::ARecord;
/// use bindy::bind9::Bind9Manager;
/// use kube::Client;
///
/// async fn handle_a_record(record: ARecord) -> anyhow::Result<()> {
///     let client = Client::try_default().await?;
///     let manager = Bind9Manager::new();
///     reconcile_a_record(client, record, &manager).await?;
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_a_record(
    client: Client,
    record: ARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling ARecord: {}/{}", namespace, name);
    debug!(
        namespace = %namespace,
        name = %name,
        generation = ?record.metadata.generation,
        "Starting ARecord reconciliation"
    );

    // Check if spec has changed using generation tracking
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Early return if spec hasn't changed (avoids reconciliation loop from status updates)
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    let spec = &record.spec;
    debug!(
        record_name = %spec.name,
        ipv4_address = %spec.ipv4_address,
        ttl = ?spec.ttl,
        zone_ref = ?spec.zone_ref,
        "ARecord configuration"
    );

    // Set initial Progressing status
    update_record_status(
        &client,
        &record,
        "Progressing",
        "True",
        "RecordReconciling",
        "Configuring A record on primary servers",
    )
    .await?;

    // Find the cluster and zone name for this zone
    debug!("Looking up DNSZone");
    let (cluster_ref, zone_name, is_cluster_provider) =
        get_zone_or_fail!(&client, &namespace, &spec.zone_ref, &record);
    debug!(cluster_ref = %cluster_ref, zone_name = %zone_name, is_cluster_provider = %is_cluster_provider, "Found DNSZone");

    // Add tracking annotations
    let api: Api<ARecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &cluster_ref, // Use cluster_ref since we update all instances
        &zone_name,
    )
    .await?;

    // Add the record to all primary pods using the generic helper
    let record_name = spec.name.clone();
    let ipv4_address = spec.ipv4_address.clone();
    let ttl = spec.ttl;

    let endpoint_count = match add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "A",
        &cluster_ref,
        is_cluster_provider,
        &zone_name,
        move |zone_manager, zone_name, pod_endpoint, key_data| {
            let record_name = record_name.clone();
            let ipv4_address = ipv4_address.clone();
            async move {
                zone_manager
                    .add_a_record(
                        &zone_name,
                        &record_name,
                        &ipv4_address,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await?;
                Ok(format!("{record_name}.{zone_name} -> {ipv4_address}"))
            }
        },
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            update_record_status(
                &client,
                &record,
                "Degraded",
                "True",
                "RecordFailed",
                &format!("Failed to configure A record: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Check if zone exists - if not, set Progressing status and return (will retry later)
    if endpoint_count == 0 {
        update_record_status(
            &client,
            &record,
            "Progressing",
            "True",
            "WaitingForZone",
            &format!("Zone {zone_name} does not exist yet, waiting for DNSZone to be created"),
        )
        .await?;
        return Ok(()); // Return success but not Ready - will retry on next reconciliation
    }

    // All done - set Ready status
    update_record_status(
        &client,
        &record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "A record {} in zone {} configured on {} endpoint(s)",
            spec.name, zone_name, endpoint_count
        ),
    )
    .await?;

    // Add this record to the DNSZone's status.records list
    add_record_to_zone_status(
        &client,
        &namespace,
        &spec.zone_ref,
        API_GROUP_VERSION,
        KIND_A_RECORD,
        &name,
    )
    .await?;

    Ok(())
}

/// Reconciles a `TXTRecord` (text) resource.
///
/// Adds or updates a TXT record in the specified zone file.
/// Commonly used for SPF, DKIM, DMARC, and domain verification.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_txt_record(
    client: Client,
    record: TXTRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling TXTRecord: {}/{}", namespace, name);

    // Check if spec has changed using generation tracking
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Early return if spec hasn't changed (avoids reconciliation loop from status updates)
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    let spec = &record.spec;

    // Set initial Progressing status
    update_record_status(
        &client,
        &record,
        "Progressing",
        "True",
        "RecordReconciling",
        "Configuring TXT record on primary servers",
    )
    .await?;

    let (cluster_ref, zone_name, is_cluster_provider) =
        get_zone_or_fail!(&client, &namespace, &spec.zone_ref, &record);

    // Add tracking annotations
    let api: Api<TXTRecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        "bindy-instance-0", // Placeholder instance name
        &zone_name,
    )
    .await?;

    // Add record to all primary endpoints
    let record_name = spec.name.clone();
    let text_value = spec.text.clone();
    let ttl = spec.ttl;

    let endpoint_count = match add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "TXT",
        &cluster_ref,
        is_cluster_provider,
        &zone_name,
        move |zone_manager, zone_name, pod_endpoint, key_data| {
            let record_name = record_name.clone();
            let text_value = text_value.clone();
            async move {
                zone_manager
                    .add_txt_record(
                        &zone_name,
                        &record_name,
                        &text_value,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await?;
                Ok(format!(
                    "{record_name}.{zone_name} TXT \"{}\"",
                    text_value.join(" ")
                ))
            }
        },
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            update_record_status(
                &client,
                &record,
                "Degraded",
                "True",
                "RecordFailed",
                &format!("Failed to configure TXT record: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Check if zone exists - if not, set Progressing status and return (will retry later)
    if endpoint_count == 0 {
        update_record_status(
            &client,
            &record,
            "Progressing",
            "True",
            "WaitingForZone",
            &format!("Zone {zone_name} does not exist yet, waiting for DNSZone to be created"),
        )
        .await?;
        return Ok(()); // Return success but not Ready - will retry on next reconciliation
    }

    // All done - set Ready status
    update_record_status(
        &client,
        &record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "TXT record {} in zone {} configured on {} endpoint(s)",
            spec.name, zone_name, endpoint_count
        ),
    )
    .await?;

    // Add this record to the DNSZone's status.records list
    add_record_to_zone_status(
        &client,
        &namespace,
        &spec.zone_ref,
        API_GROUP_VERSION,
        KIND_TXT_RECORD,
        &name,
    )
    .await?;

    Ok(())
}

/// Reconciles an `AAAARecord` (IPv6 address) resource.
///
/// Adds or updates an AAAA record in the specified zone file.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_aaaa_record(
    client: Client,
    record: AAAARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling AAAARecord: {}/{}", namespace, name);

    // Check if spec has changed using generation tracking
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Early return if spec hasn't changed (avoids reconciliation loop from status updates)
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    let spec = &record.spec;

    // Set initial Progressing status
    update_record_status(
        &client,
        &record,
        "Progressing",
        "True",
        "RecordReconciling",
        "Configuring AAAA record on primary servers",
    )
    .await?;

    let (cluster_ref, zone_name, is_cluster_provider) =
        get_zone_or_fail!(&client, &namespace, &spec.zone_ref, &record);

    // Add tracking annotations
    let api: Api<AAAARecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        "bindy-instance-0", // Placeholder instance name
        &zone_name,
    )
    .await?;

    // Add record to all primary endpoints
    let record_name = spec.name.clone();
    let ipv6_address = spec.ipv6_address.clone();
    let ttl = spec.ttl;

    let endpoint_count = match add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "AAAA",
        &cluster_ref,
        is_cluster_provider,
        &zone_name,
        move |zone_manager, zone_name, pod_endpoint, key_data| {
            let record_name = record_name.clone();
            let ipv6_address = ipv6_address.clone();
            async move {
                zone_manager
                    .add_aaaa_record(
                        &zone_name,
                        &record_name,
                        &ipv6_address,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await?;
                Ok(format!("{record_name}.{zone_name} -> {ipv6_address}"))
            }
        },
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            update_record_status(
                &client,
                &record,
                "Degraded",
                "True",
                "RecordFailed",
                &format!("Failed to configure AAAA record: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Check if zone exists - if not, set Progressing status and return (will retry later)
    if endpoint_count == 0 {
        update_record_status(
            &client,
            &record,
            "Progressing",
            "True",
            "WaitingForZone",
            &format!("Zone {zone_name} does not exist yet, waiting for DNSZone to be created"),
        )
        .await?;
        return Ok(()); // Return success but not Ready - will retry on next reconciliation
    }

    // All done - set Ready status
    update_record_status(
        &client,
        &record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "AAAA record {} in zone {} configured on {} endpoint(s)",
            spec.name, zone_name, endpoint_count
        ),
    )
    .await?;

    // Add this record to the DNSZone's status.records list
    add_record_to_zone_status(
        &client,
        &namespace,
        &spec.zone_ref,
        API_GROUP_VERSION,
        KIND_AAAA_RECORD,
        &name,
    )
    .await?;

    Ok(())
}

/// Reconciles a `CNAMERecord` (canonical name alias) resource.
///
/// Adds or updates a CNAME record in the specified zone file.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_cname_record(
    client: Client,
    record: CNAMERecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CNAMERecord: {}/{}", namespace, name);

    // Check if spec has changed using generation tracking
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Early return if spec hasn't changed (avoids reconciliation loop from status updates)
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    let spec = &record.spec;

    // Set initial Progressing status
    update_record_status(
        &client,
        &record,
        "Progressing",
        "True",
        "RecordReconciling",
        "Configuring CNAME record on primary servers",
    )
    .await?;

    let (cluster_ref, zone_name, is_cluster_provider) =
        get_zone_or_fail!(&client, &namespace, &spec.zone_ref, &record);

    // Add tracking annotations
    let api: Api<CNAMERecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        "bindy-instance-0", // Placeholder instance name
        &zone_name,
    )
    .await?;

    // Add record to all primary endpoints
    let record_name = spec.name.clone();
    let target = spec.target.clone();
    let ttl = spec.ttl;

    let endpoint_count = match add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "CNAME",
        &cluster_ref,
        is_cluster_provider,
        &zone_name,
        move |zone_manager, zone_name, pod_endpoint, key_data| {
            let record_name = record_name.clone();
            let target = target.clone();
            async move {
                zone_manager
                    .add_cname_record(
                        &zone_name,
                        &record_name,
                        &target,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await?;
                Ok(format!("{record_name}.{zone_name} -> {target}"))
            }
        },
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            update_record_status(
                &client,
                &record,
                "Degraded",
                "True",
                "RecordFailed",
                &format!("Failed to configure CNAME record: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Check if zone exists - if not, set Progressing status and return (will retry later)
    if endpoint_count == 0 {
        update_record_status(
            &client,
            &record,
            "Progressing",
            "True",
            "WaitingForZone",
            &format!("Zone {zone_name} does not exist yet, waiting for DNSZone to be created"),
        )
        .await?;
        return Ok(()); // Return success but not Ready - will retry on next reconciliation
    }

    // All done - set Ready status
    update_record_status(
        &client,
        &record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "CNAME record {} in zone {} configured on {} endpoint(s)",
            spec.name, zone_name, endpoint_count
        ),
    )
    .await?;

    // Add this record to the DNSZone's status.records list
    add_record_to_zone_status(
        &client,
        &namespace,
        &spec.zone_ref,
        API_GROUP_VERSION,
        KIND_CNAME_RECORD,
        &name,
    )
    .await?;

    Ok(())
}

/// Reconciles an `MXRecord` (mail exchange) resource.
///
/// Adds or updates an MX record in the specified zone file.
/// MX records specify mail servers for email delivery.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_mx_record(
    client: Client,
    record: MXRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling MXRecord: {}/{}", namespace, name);

    // Check if spec has changed using generation tracking
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Early return if spec hasn't changed (avoids reconciliation loop from status updates)
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    let spec = &record.spec;

    // Set initial Progressing status
    update_record_status(
        &client,
        &record,
        "Progressing",
        "True",
        "RecordReconciling",
        "Configuring MX record on primary servers",
    )
    .await?;

    let (cluster_ref, zone_name, is_cluster_provider) =
        get_zone_or_fail!(&client, &namespace, &spec.zone_ref, &record);

    // Add tracking annotations
    let api: Api<MXRecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        "bindy-instance-0", // Placeholder instance name
        &zone_name,
    )
    .await?;

    // Add record to all primary endpoints
    let record_name = spec.name.clone();
    let priority = spec.priority;
    let mail_server = spec.mail_server.clone();
    let ttl = spec.ttl;

    let endpoint_count = match add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "MX",
        &cluster_ref,
        is_cluster_provider,
        &zone_name,
        move |zone_manager, zone_name, pod_endpoint, key_data| {
            let record_name = record_name.clone();
            let mail_server = mail_server.clone();
            async move {
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
                    .await?;
                Ok(format!(
                    "{record_name}.{zone_name} MX {priority} {mail_server}"
                ))
            }
        },
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            update_record_status(
                &client,
                &record,
                "Degraded",
                "True",
                "RecordFailed",
                &format!("Failed to configure MX record: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Check if zone exists - if not, set Progressing status and return (will retry later)
    if endpoint_count == 0 {
        update_record_status(
            &client,
            &record,
            "Progressing",
            "True",
            "WaitingForZone",
            &format!("Zone {zone_name} does not exist yet, waiting for DNSZone to be created"),
        )
        .await?;
        return Ok(()); // Return success but not Ready - will retry on next reconciliation
    }

    // All done - set Ready status
    update_record_status(
        &client,
        &record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "MX record {} in zone {} configured on {} endpoint(s)",
            spec.name, zone_name, endpoint_count
        ),
    )
    .await?;

    // Add this record to the DNSZone's status.records list
    add_record_to_zone_status(
        &client,
        &namespace,
        &spec.zone_ref,
        API_GROUP_VERSION,
        KIND_MX_RECORD,
        &name,
    )
    .await?;

    Ok(())
}

/// Reconciles an `NSRecord` (nameserver delegation) resource.
///
/// Adds or updates an NS record in the specified zone file.
/// NS records delegate a subdomain to different nameservers.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_ns_record(
    client: Client,
    record: NSRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling NSRecord: {}/{}", namespace, name);

    // Check if spec has changed using generation tracking
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Early return if spec hasn't changed (avoids reconciliation loop from status updates)
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    let spec = &record.spec;

    // Set initial Progressing status
    update_record_status(
        &client,
        &record,
        "Progressing",
        "True",
        "RecordReconciling",
        "Configuring NS record on primary servers",
    )
    .await?;

    let (cluster_ref, zone_name, is_cluster_provider) =
        get_zone_or_fail!(&client, &namespace, &spec.zone_ref, &record);

    // Add tracking annotations
    let api: Api<NSRecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        "bindy-instance-0", // Placeholder instance name
        &zone_name,
    )
    .await?;

    // Add record to all primary endpoints
    let record_name = spec.name.clone();
    let nameserver = spec.nameserver.clone();
    let ttl = spec.ttl;

    let endpoint_count = match add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "NS",
        &cluster_ref,
        is_cluster_provider,
        &zone_name,
        move |zone_manager, zone_name, pod_endpoint, key_data| {
            let record_name = record_name.clone();
            let nameserver = nameserver.clone();
            async move {
                zone_manager
                    .add_ns_record(
                        &zone_name,
                        &record_name,
                        &nameserver,
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await?;
                Ok(format!("{record_name}.{zone_name} NS {nameserver}"))
            }
        },
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            update_record_status(
                &client,
                &record,
                "Degraded",
                "True",
                "RecordFailed",
                &format!("Failed to configure NS record: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Check if zone exists - if not, set Progressing status and return (will retry later)
    if endpoint_count == 0 {
        update_record_status(
            &client,
            &record,
            "Progressing",
            "True",
            "WaitingForZone",
            &format!("Zone {zone_name} does not exist yet, waiting for DNSZone to be created"),
        )
        .await?;
        return Ok(()); // Return success but not Ready - will retry on next reconciliation
    }

    // All done - set Ready status
    update_record_status(
        &client,
        &record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "NS record {} in zone {} configured on {} endpoint(s)",
            spec.name, zone_name, endpoint_count
        ),
    )
    .await?;

    // Add this record to the DNSZone's status.records list
    add_record_to_zone_status(
        &client,
        &namespace,
        &spec.zone_ref,
        API_GROUP_VERSION,
        KIND_NS_RECORD,
        &name,
    )
    .await?;

    Ok(())
}

/// Reconciles an `SRVRecord` (service location) resource.
///
/// Adds or updates an SRV record in the specified zone file.
/// SRV records specify the location of services (e.g., _ldap._tcp).
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_srv_record(
    client: Client,
    record: SRVRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling SRVRecord: {}/{}", namespace, name);

    // Check if spec has changed using generation tracking
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Early return if spec hasn't changed (avoids reconciliation loop from status updates)
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    let spec = &record.spec;

    // Set initial Progressing status
    update_record_status(
        &client,
        &record,
        "Progressing",
        "True",
        "RecordReconciling",
        "Configuring SRV record on primary servers",
    )
    .await?;

    let (cluster_ref, zone_name, is_cluster_provider) =
        get_zone_or_fail!(&client, &namespace, &spec.zone_ref, &record);

    // Add tracking annotations
    let api: Api<SRVRecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        "bindy-instance-0", // Placeholder instance name
        &zone_name,
    )
    .await?;

    // Add record to all primary endpoints
    let record_name = spec.name.clone();
    let srv_data = crate::bind9::SRVRecordData {
        priority: spec.priority,
        weight: spec.weight,
        port: spec.port,
        target: spec.target.clone(),
        ttl: spec.ttl,
    };

    let endpoint_count = match add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "SRV",
        &cluster_ref,
        is_cluster_provider,
        &zone_name,
        move |zone_manager, zone_name, pod_endpoint, key_data| {
            let record_name = record_name.clone();
            let srv_data = srv_data.clone();
            async move {
                zone_manager
                    .add_srv_record(
                        &zone_name,
                        &record_name,
                        &srv_data,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await?;
                Ok(format!(
                    "{record_name}.{zone_name} SRV {} {} {} {}",
                    srv_data.priority, srv_data.weight, srv_data.port, srv_data.target
                ))
            }
        },
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            update_record_status(
                &client,
                &record,
                "Degraded",
                "True",
                "RecordFailed",
                &format!("Failed to configure SRV record: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Check if zone exists - if not, set Progressing status and return (will retry later)
    if endpoint_count == 0 {
        update_record_status(
            &client,
            &record,
            "Progressing",
            "True",
            "WaitingForZone",
            &format!("Zone {zone_name} does not exist yet, waiting for DNSZone to be created"),
        )
        .await?;
        return Ok(()); // Return success but not Ready - will retry on next reconciliation
    }

    // All done - set Ready status
    update_record_status(
        &client,
        &record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "SRV record {} in zone {} configured on {} endpoint(s)",
            spec.name, zone_name, endpoint_count
        ),
    )
    .await?;

    // Add this record to the DNSZone's status.records list
    add_record_to_zone_status(
        &client,
        &namespace,
        &spec.zone_ref,
        API_GROUP_VERSION,
        KIND_SRV_RECORD,
        &name,
    )
    .await?;

    Ok(())
}

/// Reconciles a `CAARecord` (certificate authority authorization) resource.
///
/// Adds or updates a CAA record in the specified zone file.
/// CAA records specify which certificate authorities can issue certificates.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_caa_record(
    client: Client,
    record: CAARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CAARecord: {}/{}", namespace, name);

    // Check if spec has changed using generation tracking
    let current_generation = record.metadata.generation;
    let observed_generation = record.status.as_ref().and_then(|s| s.observed_generation);

    // Early return if spec hasn't changed (avoids reconciliation loop from status updates)
    if !crate::reconcilers::should_reconcile(current_generation, observed_generation) {
        debug!(
            "Spec unchanged (generation={:?}), skipping reconciliation",
            current_generation
        );
        return Ok(());
    }

    let spec = &record.spec;

    // Set initial Progressing status
    update_record_status(
        &client,
        &record,
        "Progressing",
        "True",
        "RecordReconciling",
        "Configuring CAA record on primary servers",
    )
    .await?;

    let (cluster_ref, zone_name, is_cluster_provider) =
        get_zone_or_fail!(&client, &namespace, &spec.zone_ref, &record);

    // Add tracking annotations
    let api: Api<CAARecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        "bindy-instance-0", // Placeholder instance name
        &zone_name,
    )
    .await?;

    // Add record to all primary endpoints
    let record_name = spec.name.clone();
    let flags = spec.flags;
    let tag = spec.tag.clone();
    let value = spec.value.clone();
    let ttl = spec.ttl;

    let endpoint_count = match add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "CAA",
        &cluster_ref,
        is_cluster_provider,
        &zone_name,
        move |zone_manager, zone_name, pod_endpoint, key_data| {
            let record_name = record_name.clone();
            let tag = tag.clone();
            let value = value.clone();
            async move {
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
                    .await?;
                Ok(format!(
                    "{record_name}.{zone_name} CAA {flags} {tag} \"{value}\""
                ))
            }
        },
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            update_record_status(
                &client,
                &record,
                "Degraded",
                "True",
                "RecordFailed",
                &format!("Failed to configure CAA record: {e}"),
            )
            .await?;
            return Err(e);
        }
    };

    // Check if zone exists - if not, set Progressing status and return (will retry later)
    if endpoint_count == 0 {
        update_record_status(
            &client,
            &record,
            "Progressing",
            "True",
            "WaitingForZone",
            &format!("Zone {zone_name} does not exist yet, waiting for DNSZone to be created"),
        )
        .await?;
        return Ok(()); // Return success but not Ready - will retry on next reconciliation
    }

    // All done - set Ready status
    update_record_status(
        &client,
        &record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!(
            "CAA record {} in zone {} configured on {} endpoint(s)",
            spec.name, zone_name, endpoint_count
        ),
    )
    .await?;

    // Add this record to the DNSZone's status.records list
    add_record_to_zone_status(
        &client,
        &namespace,
        &spec.zone_ref,
        API_GROUP_VERSION,
        KIND_CAA_RECORD,
        &name,
    )
    .await?;

    Ok(())
}

/// Retrieves zone information (`cluster_ref` and `zone_name`) for a DNS record.
///
/// Directly retrieves a `DNSZone` by its Kubernetes resource name.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for the `DNSZone`
/// * `zone_ref` - Reference to a `DNSZone` resource by metadata.name
///
/// # Returns
///
/// A tuple of `(cluster_ref, zone_name, is_cluster_provider)` where:
/// - `cluster_ref` identifies which `Bind9Cluster` or `ClusterBind9Provider` serves the zone
/// - `zone_name` is the actual DNS zone name (e.g., "example.com")
/// - `is_cluster_provider` indicates if this references a `ClusterBind9Provider`
///
/// # Errors
///
/// Returns an error if:
/// - The `DNSZone` API is unavailable
/// - The specified `zone_ref` does not exist
async fn get_zone_info(
    client: &Client,
    namespace: &str,
    zone_ref: &str,
) -> Result<(String, String, bool)> {
    let zone_api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

    // Lookup by zoneRef - direct get by resource name (efficient)
    let zone = zone_api
        .get(zone_ref)
        .await
        .with_context(|| format!("Failed to get DNSZone {zone_ref} in namespace {namespace}"))?;

    // Extract cluster_ref (handles both clusterRef and globalClusterRef)
    let cluster_ref =
        crate::reconcilers::dnszone::get_cluster_ref_from_spec(&zone.spec, namespace, zone_ref)?;
    let is_cluster_provider = zone.spec.cluster_provider_ref.is_some();
    Ok((cluster_ref, zone.spec.zone_name, is_cluster_provider))
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
///
/// # Errors
///
/// Returns an error if the status update fails.
async fn update_record_status<T>(
    client: &Client,
    record: &T,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
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

    let record_status = RecordStatus {
        conditions: vec![condition],
        observed_generation: record.meta().generation,
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

/// Adds or updates a record reference in the `DNSZone`'s `status.records` list.
///
/// When a DNS record is successfully reconciled, this function updates the associated
/// `DNSZone`'s status to include a reference to the record. This provides visibility into
/// which records are associated with each zone.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the `DNSZone`
/// * `zone_ref` - Name of the `DNSZone`
/// * `api_version` - API version of the record (e.g., "bindy.firestoned.io/v1beta1")
/// * `kind` - Kind of the record (e.g., `ARecord`, `CNAMERecord`)
/// * `record_name` - Name of the record resource
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail.
async fn add_record_to_zone_status(
    client: &Client,
    namespace: &str,
    zone_ref: &str,
    api_version: &str,
    kind: &str,
    record_name: &str,
) -> Result<()> {
    let zone_api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

    // Get current zone to check existing records
    let zone = zone_api
        .get(zone_ref)
        .await
        .context(format!("Failed to get DNSZone {namespace}/{zone_ref}"))?;

    let new_record_ref = crate::crd::RecordReference {
        api_version: api_version.to_string(),
        kind: kind.to_string(),
        name: record_name.to_string(),
    };

    // Get current records list, or create empty vec
    let mut records = zone
        .status
        .as_ref()
        .map(|s| s.records.clone())
        .unwrap_or_default();

    // Check if this record is already in the list (avoid duplicates)
    if records.iter().any(|r| {
        r.api_version == new_record_ref.api_version
            && r.kind == new_record_ref.kind
            && r.name == new_record_ref.name
    }) {
        debug!(
            zone = zone_ref,
            record = record_name,
            kind = kind,
            "Record reference already exists in DNSZone status"
        );
        return Ok(()); // No update needed
    }

    records.push(new_record_ref.clone());
    debug!(
        zone = zone_ref,
        record = record_name,
        kind = kind,
        "Adding record reference to DNSZone status"
    );

    // Preserve all other status fields
    let current_status = zone.status.as_ref();
    let updated_status = crate::crd::DNSZoneStatus {
        conditions: current_status
            .map(|s| s.conditions.clone())
            .unwrap_or_default(),
        observed_generation: current_status.and_then(|s| s.observed_generation),
        record_count: current_status.and_then(|s| s.record_count),
        secondary_ips: current_status.and_then(|s| s.secondary_ips.clone()),
        records,
    };

    let status_patch = json!({
        "status": updated_status
    });

    zone_api
        .patch_status(
            zone_ref,
            &PatchParams::default(),
            &Patch::Merge(&status_patch),
        )
        .await
        .context(format!(
            "Failed to update DNSZone status for {namespace}/{zone_ref}"
        ))?;

    info!(
        zone = zone_ref,
        record = record_name,
        kind = kind,
        "Added record reference to DNSZone status.records"
    );

    Ok(())
}
