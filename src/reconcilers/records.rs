// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! DNS record reconciliation logic.
//!
//! This module contains reconcilers for all DNS record types supported by Bindy.
//! Each reconciler adds or updates records in the appropriate zone file.

use crate::bind9::RndcKeyData;
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, Condition, DNSZone, MXRecord, NSRecord,
    RecordStatus, SRVRecord, TXTRecord,
};
use crate::labels::{BINDY_CLUSTER_ANNOTATION, BINDY_INSTANCE_ANNOTATION, BINDY_ZONE_ANNOTATION};
use anyhow::{anyhow, Context, Result};
use k8s_openapi::api::core::v1::{Event, ObjectReference, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::chrono::Utc;
use kube::{
    api::{ListParams, Patch, PatchParams, PostParams},
    client::Client,
    Api, Resource, ResourceExt,
};
use serde_json::json;
use std::env;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Helper macro to handle zone lookup with proper error reporting
///
/// Returns a tuple of (`cluster_ref`, `zone_name`) from the `DNSZone` lookup
macro_rules! get_zone_or_fail {
    ($client:expr, $namespace:expr, $zone:expr, $zone_ref:expr, $record:expr) => {
        match get_zone_info($client, $namespace, $zone.as_ref(), $zone_ref.as_ref()).await {
            Ok((cluster_ref, zone_name)) => (cluster_ref, zone_name),
            Err(e) => {
                let error_msg = format!(
                    "Failed to lookup DNSZone in namespace {}: {}",
                    $namespace, e
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

/// Helper macro to find primary instance and load RNDC key
macro_rules! get_instance_and_key {
    ($client:expr, $namespace:expr, $cluster_ref:expr, $record:expr) => {{
        // Find an available instance (primary preferred, secondary fallback)
        let instance_name = match find_available_instance($client, $namespace, $cluster_ref).await {
            Ok(name) => name,
            Err(e) => {
                let error_msg = format!(
                    "Failed to find available instance for cluster {}: {}",
                    $cluster_ref, e
                );
                error!("{}", error_msg);
                update_record_status(
                    $client,
                    $record,
                    "Ready",
                    "False",
                    "NoAvailableInstance",
                    &error_msg,
                )
                .await?;
                return Err(anyhow!(error_msg));
            }
        };

        // Load RNDC key
        let key_data = match load_rndc_key($client, $namespace, &instance_name).await {
            Ok(key) => key,
            Err(e) => {
                let error_msg = format!("Failed to load RNDC key: {}", e);
                error!("{}", error_msg);
                update_record_status(
                    $client,
                    $record,
                    "Ready",
                    "False",
                    "RndcKeyLoadFailed",
                    &error_msg,
                )
                .await?;
                return Err(anyhow!(error_msg));
            }
        };

        // Build server address using the instance name
        let server = format!("{}.{}.svc.cluster.local:953", instance_name, $namespace);

        (instance_name, key_data, server)
    }};
}

/// Helper macro to handle record operation failures with proper error reporting
///
/// After successfully adding a record, this macro automatically triggers a NOTIFY
/// to secondary servers so they can initiate zone transfers (IXFR) to receive the update.
macro_rules! handle_record_operation {
    ($result:expr, $client:expr, $record:expr, $zone_name:expr, $server:expr, $key_data:expr, $zone_manager:expr, $success_msg:expr) => {
        match $result {
            Ok(()) => {
                // Update record status
                update_record_status(
                    $client,
                    $record,
                    "Ready",
                    "True",
                    "RecordCreated",
                    $success_msg,
                )
                .await?;

                // Notify secondaries about the zone change
                // This triggers IXFR (incremental zone transfer) from primary to secondaries
                info!(
                    "Notifying secondaries about zone {} update after record operation",
                    $zone_name
                );
                if let Err(e) = $zone_manager.notify_zone($zone_name, $server).await {
                    // Don't fail the entire operation if NOTIFY fails - log and continue
                    // The record was successfully added, secondaries will eventually catch up via SOA refresh
                    warn!(
                        "Failed to notify secondaries for zone {}: {}. Secondaries will sync via SOA refresh timer.",
                        $zone_name, e
                    );
                }

                Ok(())
            }
            Err(e) => {
                let error_msg =
                    if e.to_string().contains("connection") || e.to_string().contains("connect") {
                        format!(
                            "Cannot connect to BIND9 server at {}: {}. Will retry in {:?}",
                            $server,
                            e,
                            get_retry_interval()
                        )
                    } else {
                        format!("Failed to add DNS record: {}", e)
                    };
                error!("{}", error_msg);
                update_record_status(
                    $client,
                    $record,
                    "Ready",
                    "False",
                    "RecordAddFailed",
                    &error_msg,
                )
                .await?;
                Err(anyhow!(error_msg))
            }
        }
    };
}

/// Adds tracking annotations to a DNS record resource.
///
/// Annotations are added to help track which `Bind9Cluster`, `Bind9Instance`, and `DNSZone`
/// this record is associated with. This aids in debugging and resource management.
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

    let spec = &record.spec;
    debug!(
        record_name = %spec.name,
        ipv4_address = %spec.ipv4_address,
        ttl = ?spec.ttl,
        zone = ?spec.zone,
        zone_ref = ?spec.zone_ref,
        "ARecord configuration"
    );

    // Find the cluster and zone name for this zone
    debug!("Looking up DNSZone");
    let (cluster_ref, zone_name) =
        get_zone_or_fail!(&client, &namespace, &spec.zone, &spec.zone_ref, &record);
    debug!(cluster_ref = %cluster_ref, zone_name = %zone_name, "Found DNSZone");

    // Find primary instance and get RNDC key + server address
    let (instance_name, key_data, server) =
        get_instance_and_key!(&client, &namespace, &cluster_ref, &record);

    // Add tracking annotations
    let api: Api<ARecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &instance_name,
        &zone_name,
    )
    .await?;

    // Add the record to the zone
    let result = zone_manager
        .add_a_record(
            &zone_name,
            &spec.name,
            &spec.ipv4_address,
            spec.ttl,
            &server,
            &key_data,
        )
        .await;

    handle_record_operation!(
        result,
        &client,
        &record,
        &zone_name,
        &server,
        &key_data,
        zone_manager,
        &format!("A record {}.{} created successfully", spec.name, zone_name)
    )
}

/// Reconciles a `TXTRecord` (text) resource.
///
/// Adds or updates a TXT record in the specified zone file.
/// Commonly used for SPF, DKIM, DMARC, and domain verification.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
pub async fn reconcile_txt_record(
    client: Client,
    record: TXTRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling TXTRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    let (cluster_ref, zone_name) =
        get_zone_or_fail!(&client, &namespace, &spec.zone, &spec.zone_ref, &record);
    let (instance_name, key_data, server) =
        get_instance_and_key!(&client, &namespace, &cluster_ref, &record);

    // Add tracking annotations
    let api: Api<TXTRecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &instance_name,
        &zone_name,
    )
    .await?;

    let result = zone_manager
        .add_txt_record(
            &zone_name, &spec.name, &spec.text, spec.ttl, &server, &key_data,
        )
        .await;

    handle_record_operation!(
        result,
        &client,
        &record,
        &zone_name,
        &server,
        &key_data,
        zone_manager,
        &format!(
            "TXT record {}.{} created successfully",
            spec.name, zone_name
        )
    )
}

/// Reconciles an `AAAARecord` (IPv6 address) resource.
///
/// Adds or updates an AAAA record in the specified zone file.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
pub async fn reconcile_aaaa_record(
    client: Client,
    record: AAAARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling AAAARecord: {}/{}", namespace, name);

    let spec = &record.spec;

    let (cluster_ref, zone_name) =
        get_zone_or_fail!(&client, &namespace, &spec.zone, &spec.zone_ref, &record);
    let (instance_name, key_data, server) =
        get_instance_and_key!(&client, &namespace, &cluster_ref, &record);

    // Add tracking annotations
    let api: Api<AAAARecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &instance_name,
        &zone_name,
    )
    .await?;

    let result = zone_manager
        .add_aaaa_record(
            &zone_name,
            &spec.name,
            &spec.ipv6_address,
            spec.ttl,
            &server,
            &key_data,
        )
        .await;

    handle_record_operation!(
        result,
        &client,
        &record,
        &zone_name,
        &server,
        &key_data,
        zone_manager,
        &format!(
            "AAAA record {}.{} created successfully",
            spec.name, zone_name
        )
    )
}

/// Reconciles a `CNAMERecord` (canonical name alias) resource.
///
/// Adds or updates a CNAME record in the specified zone file.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
pub async fn reconcile_cname_record(
    client: Client,
    record: CNAMERecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CNAMERecord: {}/{}", namespace, name);

    let spec = &record.spec;

    let (cluster_ref, zone_name) =
        get_zone_or_fail!(&client, &namespace, &spec.zone, &spec.zone_ref, &record);
    let (instance_name, key_data, server) =
        get_instance_and_key!(&client, &namespace, &cluster_ref, &record);

    // Add tracking annotations
    let api: Api<CNAMERecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &instance_name,
        &zone_name,
    )
    .await?;

    let result = zone_manager
        .add_cname_record(
            &zone_name,
            &spec.name,
            &spec.target,
            spec.ttl,
            &server,
            &key_data,
        )
        .await;

    handle_record_operation!(
        result,
        &client,
        &record,
        &zone_name,
        &server,
        &key_data,
        zone_manager,
        &format!(
            "CNAME record {}.{} created successfully",
            spec.name, zone_name
        )
    )
}

/// Reconciles an `MXRecord` (mail exchange) resource.
///
/// Adds or updates an MX record in the specified zone file.
/// MX records specify mail servers for email delivery.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
pub async fn reconcile_mx_record(
    client: Client,
    record: MXRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling MXRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    let (cluster_ref, zone_name) =
        get_zone_or_fail!(&client, &namespace, &spec.zone, &spec.zone_ref, &record);
    let (instance_name, key_data, server) =
        get_instance_and_key!(&client, &namespace, &cluster_ref, &record);

    // Add tracking annotations
    let api: Api<MXRecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &instance_name,
        &zone_name,
    )
    .await?;

    let result = zone_manager
        .add_mx_record(
            &zone_name,
            &spec.name,
            spec.priority,
            &spec.mail_server,
            spec.ttl,
            &server,
            &key_data,
        )
        .await;

    handle_record_operation!(
        result,
        &client,
        &record,
        &zone_name,
        &server,
        &key_data,
        zone_manager,
        &format!("MX record {}.{} created successfully", spec.name, zone_name)
    )
}

/// Reconciles an `NSRecord` (nameserver delegation) resource.
///
/// Adds or updates an NS record in the specified zone file.
/// NS records delegate a subdomain to different nameservers.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
pub async fn reconcile_ns_record(
    client: Client,
    record: NSRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling NSRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    let (cluster_ref, zone_name) =
        get_zone_or_fail!(&client, &namespace, &spec.zone, &spec.zone_ref, &record);
    let (instance_name, key_data, server) =
        get_instance_and_key!(&client, &namespace, &cluster_ref, &record);

    // Add tracking annotations
    let api: Api<NSRecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &instance_name,
        &zone_name,
    )
    .await?;

    let result = zone_manager
        .add_ns_record(
            &zone_name,
            &spec.name,
            &spec.nameserver,
            spec.ttl,
            &server,
            &key_data,
        )
        .await;

    handle_record_operation!(
        result,
        &client,
        &record,
        &zone_name,
        &server,
        &key_data,
        zone_manager,
        &format!("NS record {}.{} created successfully", spec.name, zone_name)
    )
}

/// Reconciles an `SRVRecord` (service location) resource.
///
/// Adds or updates an SRV record in the specified zone file.
/// SRV records specify the location of services (e.g., _ldap._tcp).
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
pub async fn reconcile_srv_record(
    client: Client,
    record: SRVRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling SRVRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    let (cluster_ref, zone_name) =
        get_zone_or_fail!(&client, &namespace, &spec.zone, &spec.zone_ref, &record);
    let (instance_name, key_data, server) =
        get_instance_and_key!(&client, &namespace, &cluster_ref, &record);

    // Add tracking annotations
    let api: Api<SRVRecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &instance_name,
        &zone_name,
    )
    .await?;

    let srv_data = crate::bind9::SRVRecordData {
        priority: spec.priority,
        weight: spec.weight,
        port: spec.port,
        target: spec.target.clone(),
        ttl: spec.ttl,
    };

    let result = zone_manager
        .add_srv_record(&zone_name, &spec.name, &srv_data, &server, &key_data)
        .await;

    handle_record_operation!(
        result,
        &client,
        &record,
        &zone_name,
        &server,
        &key_data,
        zone_manager,
        &format!(
            "SRV record {}.{} created successfully",
            spec.name, zone_name
        )
    )
}

/// Reconciles a `CAARecord` (certificate authority authorization) resource.
///
/// Adds or updates a CAA record in the specified zone file.
/// CAA records specify which certificate authorities can issue certificates.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations or BIND9 record operations fail.
pub async fn reconcile_caa_record(
    client: Client,
    record: CAARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CAARecord: {}/{}", namespace, name);

    let spec = &record.spec;

    let (cluster_ref, zone_name) =
        get_zone_or_fail!(&client, &namespace, &spec.zone, &spec.zone_ref, &record);
    let (instance_name, key_data, server) =
        get_instance_and_key!(&client, &namespace, &cluster_ref, &record);

    // Add tracking annotations
    let api: Api<CAARecord> = Api::namespaced(client.clone(), &namespace);
    add_record_annotations(
        &client,
        &api,
        &name,
        &cluster_ref,
        &instance_name,
        &zone_name,
    )
    .await?;

    let result = zone_manager
        .add_caa_record(
            &zone_name,
            &spec.name,
            spec.flags,
            &spec.tag,
            &spec.value,
            spec.ttl,
            &server,
            &key_data,
        )
        .await;

    handle_record_operation!(
        result,
        &client,
        &record,
        &zone_name,
        &server,
        &key_data,
        zone_manager,
        &format!(
            "CAA record {}.{} created successfully",
            spec.name, zone_name
        )
    )
}

/// Retrieves zone information (`cluster_ref` and `zone_name`) for a DNS record.
///
/// This function supports two lookup methods:
/// - `zone`: Searches for a `DNSZone` by matching `spec.zoneName` (e.g., "example.com")
/// - `zone_ref`: Directly retrieves a `DNSZone` by its Kubernetes resource name (more efficient)
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for the `DNSZone`
/// * `zone` - Optional DNS zone name to match against `spec.zoneName`
/// * `zone_ref` - Optional reference to a `DNSZone` resource by metadata.name
///
/// # Returns
///
/// A tuple of `(cluster_ref, zone_name)` where:
/// - `cluster_ref` identifies which `Bind9Instance` serves the zone
/// - `zone_name` is the actual DNS zone name (e.g., "example.com")
///
/// # Errors
///
/// Returns an error if:
/// - Neither `zone` nor `zone_ref` is specified
/// - Both `zone` and `zone_ref` are specified
/// - The `DNSZone` API is unavailable
/// - No `DNSZone` matches the given `zone` name
/// - The specified `zone_ref` does not exist
async fn get_zone_info(
    client: &Client,
    namespace: &str,
    zone: Option<&String>,
    zone_ref: Option<&String>,
) -> Result<(String, String)> {
    let zone_api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

    match (zone, zone_ref) {
        (Some(zone_name), None) => {
            // Lookup by zoneName - search all zones for matching spec.zoneName
            let zones = zone_api.list(&ListParams::default()).await?;

            for z in zones.items {
                if &z.spec.zone_name == zone_name {
                    return Ok((z.spec.cluster_ref, z.spec.zone_name));
                }
            }

            Err(anyhow!(
                "No DNSZone found with zoneName={zone_name} in namespace {namespace}"
            ))
        }
        (None, Some(ref_name)) => {
            // Lookup by zoneRef - direct get by resource name (more efficient)
            let zone = zone_api.get(ref_name).await.with_context(|| {
                format!("Failed to get DNSZone {ref_name} in namespace {namespace}")
            })?;

            Ok((zone.spec.cluster_ref, zone.spec.zone_name))
        }
        (Some(_), Some(_)) => Err(anyhow!(
            "Cannot specify both 'zone' and 'zoneRef'. Use only one."
        )),
        (None, None) => Err(anyhow!(
            "Must specify either 'zone' or 'zoneRef' to identify the DNS zone"
        )),
    }
}

/// Finds an available `Bind9Instance` for a given cluster.
///
/// Searches for `Bind9Instances` with matching `clusterRef`. Prefers primary instances
/// but falls back to secondary instances if no primary is available. This allows DNS
/// updates to continue even when the primary instance is down. Secondary instances will
/// sync changes back to the primary via zone transfers when it comes back up.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search in
/// * `cluster_ref` - Name of the `Bind9Cluster`
///
/// # Returns
///
/// The name of an available `Bind9Instance` (primary if available, otherwise secondary)
///
/// # Errors
///
/// Returns an error if:
/// - No instances found for the cluster
/// - Neither primary nor secondary instances are found
async fn find_available_instance(
    client: &Client,
    namespace: &str,
    cluster_ref: &str,
) -> Result<String> {
    use crate::crd::{Bind9Instance, ServerRole};
    let instance_api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    let instances = instance_api.list(&ListParams::default()).await?;

    info!(
        "Searching for Bind9Instance with clusterRef='{}' in namespace '{}' (found {} total instances)",
        cluster_ref,
        namespace,
        instances.items.len()
    );

    let mut primary_instance: Option<String> = None;
    let mut secondary_instance: Option<String> = None;

    // Find both primary and secondary instances
    for instance in instances.items {
        let instance_name = instance.metadata.name.unwrap_or_default();
        let instance_cluster_ref = &instance.spec.cluster_ref;
        let instance_role = &instance.spec.role;

        info!(
            "Found Bind9Instance '{}' with clusterRef='{}' and role={:?}",
            instance_name, instance_cluster_ref, instance_role
        );

        if instance_cluster_ref == cluster_ref {
            match instance_role {
                ServerRole::Primary => {
                    primary_instance = Some(instance_name.clone());
                    info!(
                        "Matched primary instance '{}' for cluster '{}'",
                        instance_name, cluster_ref
                    );
                }
                ServerRole::Secondary => {
                    if secondary_instance.is_none() {
                        secondary_instance = Some(instance_name.clone());
                        info!(
                            "Matched secondary instance '{}' for cluster '{}'",
                            instance_name, cluster_ref
                        );
                    }
                }
            }
        }
    }

    // Prefer primary, fall back to secondary
    if let Some(primary) = primary_instance {
        info!("Using primary instance '{primary}' for cluster '{cluster_ref}'");
        Ok(primary)
    } else if let Some(secondary) = secondary_instance {
        warn!(
            "Primary instance not available for cluster '{cluster_ref}', \
            using secondary instance '{secondary}'. Changes will sync to primary when it returns."
        );
        Ok(secondary)
    } else {
        Err(anyhow!(
            "No available Bind9Instance (primary or secondary) found for cluster '{cluster_ref}' in namespace '{namespace}'. \
            Check that a Bind9Instance exists with spec.clusterRef='{cluster_ref}'"
        ))
    }
}

/// Loads RNDC key data from the instance's Kubernetes Secret.
///
/// Retrieves the Secret named `{instance_name}-rndc-key` from the specified
/// namespace and parses it into an `RndcKeyData` structure for authenticating
/// RNDC protocol connections.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace containing the Secret
/// * `instance_name` - Name of the `Bind9Instance` (used to construct Secret name)
///
/// # Returns
///
/// The parsed RNDC key data containing name, algorithm, and secret
///
/// # Errors
///
/// Returns an error if:
/// - The Secret does not exist
/// - The Secret has no data field
/// - Required fields (key-name, algorithm, secret) are missing
/// - Field values contain invalid UTF-8
async fn load_rndc_key(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<RndcKeyData> {
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret_name = format!("{instance_name}-rndc-key");

    let secret = secret_api.get(&secret_name).await.context(format!(
        "Failed to get RNDC secret {secret_name} in namespace {namespace}"
    ))?;

    let data = secret
        .data
        .as_ref()
        .ok_or_else(|| anyhow!("Secret {secret_name} has no data"))?;

    // Convert ByteString to Vec<u8>
    let mut converted_data = std::collections::BTreeMap::new();
    for (key, value) in data {
        converted_data.insert(key.clone(), value.0.clone());
    }

    crate::bind9::Bind9Manager::parse_rndc_secret_data(&converted_data)
}

/// Get the retry interval from environment variable or use default.
///
/// Reads the `BINDY_RECORD_RETRY_SECONDS` environment variable to determine
/// how long to wait before retrying failed record reconciliations.
///
/// Default: 30 seconds
fn get_retry_interval() -> Duration {
    let default_seconds = 30;
    let seconds = env::var("BINDY_RECORD_RETRY_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default_seconds);
    Duration::from_secs(seconds)
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
                    if let Some(cond) = conditions.first() {
                        let status_matches =
                            cond.get("status").and_then(|s| s.as_str()) == Some(status);
                        let reason_matches =
                            cond.get("reason").and_then(|r| r.as_str()) == Some(reason);
                        !(status_matches && reason_matches)
                    } else {
                        true // No conditions, need to add one
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
            if let Some(cond) = conditions.first() {
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
