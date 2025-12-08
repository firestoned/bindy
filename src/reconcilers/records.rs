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
use k8s_openapi::api::core::v1::{Event, ObjectReference};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::chrono::Utc;
use kube::{
    api::{ListParams, Patch, PatchParams, PostParams},
    client::Client,
    Api, Resource, ResourceExt,
};
use serde_json::json;
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
/// Returns `Ok(())` if the record was successfully added to all endpoints
///
/// # Errors
///
/// Returns error if any endpoint operation fails
async fn add_record_to_all_endpoints<R, F, Fut>(
    client: &Client,
    record: &R,
    zone_manager: &crate::bind9::Bind9Manager,
    record_type_name: &str,
    cluster_ref: &str,
    zone_name: &str,
    add_operation: F,
) -> Result<()>
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

    // Add the record to ALL primary pods across ALL instances
    // With EmptyDir storage, each pod maintains its own zone files
    let zone_name_clone = zone_name.to_string();
    let zone_manager_clone = zone_manager.clone();

    let (first_endpoint, total_endpoints) = super::dnszone::for_each_primary_endpoint(
        client,
        &namespace,
        cluster_ref,
        true, // with_rndc_key = true for record operations
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
                    "Adding {} record to zone {} at endpoint {} (instance: {})",
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
                    "Failed to add {record_type} record to zone {zone_name} at endpoint {pod_endpoint}"
                ))?;

                debug!(
                    "Successfully added {} record to zone {} at endpoint {}: {}",
                    record_type, zone_name, pod_endpoint_clone, description
                );

                Ok(())
            }
        },
    )
    .await?;

    info!(
        "Successfully added {} record to zone {} at {} endpoint(s) for cluster {}",
        record_type_name, zone_name, total_endpoints, cluster_ref
    );

    // Notify secondaries about the record change via the first endpoint
    if let Some(first_pod_endpoint) = first_endpoint {
        if let Err(e) = zone_manager
            .notify_zone(zone_name, &first_pod_endpoint)
            .await
        {
            warn!(
                "Failed to notify secondaries for zone {}: {}. Secondaries will sync via SOA refresh timer.",
                zone_name, e
            );
        }
    }

    // Update status to success
    update_record_status(
        client,
        record,
        "Ready",
        "True",
        "ReconcileSucceeded",
        &format!("{record_type_name} record in zone {zone_name} created successfully"),
    )
    .await?;

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

    add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "A",
        &cluster_ref,
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

    add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "TXT",
        &cluster_ref,
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

    add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "AAAA",
        &cluster_ref,
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

    add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "CNAME",
        &cluster_ref,
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

    add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "MX",
        &cluster_ref,
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

    add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "NS",
        &cluster_ref,
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

    add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "SRV",
        &cluster_ref,
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

    add_record_to_all_endpoints(
        &client,
        &record,
        zone_manager,
        "CAA",
        &cluster_ref,
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
