// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! DNS record reconciliation logic.
//!
//! This module contains reconcilers for all DNS record types supported by Bindy.
//! Each reconciler adds or updates records in the appropriate zone file.

use crate::bind9::RndcKeyData;
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, DNSZone, MXRecord, NSRecord, SRVRecord, TXTRecord,
};
use anyhow::{anyhow, Context, Result};
use k8s_openapi::api::core::v1::Secret;
use kube::{api::ListParams, client::Client, Api, ResourceExt};
use tracing::info;

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

    let spec = &record.spec;

    // Find the cluster for this zone
    let cluster_ref = get_cluster_from_zone(&client, &namespace, &spec.zone).await?;

    // Load RNDC key
    let key_data = load_rndc_key(&client, &namespace, &cluster_ref).await?;

    // Build server address
    let server = format!("{cluster_ref}.{namespace}.svc.cluster.local:953");

    // Add the record to the zone
    zone_manager
        .add_a_record(
            &spec.zone,
            &spec.name,
            &spec.ipv4_address,
            spec.ttl,
            &server,
            &key_data,
        )
        .await?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "A record created").await?;

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
pub async fn reconcile_txt_record(
    client: Client,
    record: TXTRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling TXTRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Find the cluster for this zone
    let cluster_ref = get_cluster_from_zone(&client, &namespace, &spec.zone).await?;

    // Load RNDC key
    let key_data = load_rndc_key(&client, &namespace, &cluster_ref).await?;

    // Build server address
    let server = format!("{cluster_ref}.{namespace}.svc.cluster.local:953");

    // Add the record to the zone
    zone_manager
        .add_txt_record(
            &spec.zone, &spec.name, &spec.text, spec.ttl, &server, &key_data,
        )
        .await?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "TXT record created").await?;

    Ok(())
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

    // Find the cluster for this zone
    let cluster_ref = get_cluster_from_zone(&client, &namespace, &spec.zone).await?;

    // Load RNDC key
    let key_data = load_rndc_key(&client, &namespace, &cluster_ref).await?;

    // Build server address
    let server = format!("{cluster_ref}.{namespace}.svc.cluster.local:953");

    // Add the record to the zone
    zone_manager
        .add_aaaa_record(
            &spec.zone,
            &spec.name,
            &spec.ipv6_address,
            spec.ttl,
            &server,
            &key_data,
        )
        .await?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "AAAA record created").await?;

    Ok(())
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

    // Find the cluster for this zone
    let cluster_ref = get_cluster_from_zone(&client, &namespace, &spec.zone).await?;

    // Load RNDC key
    let key_data = load_rndc_key(&client, &namespace, &cluster_ref).await?;

    // Build server address
    let server = format!("{cluster_ref}.{namespace}.svc.cluster.local:953");

    // Add the record to the zone
    zone_manager
        .add_cname_record(
            &spec.zone,
            &spec.name,
            &spec.target,
            spec.ttl,
            &server,
            &key_data,
        )
        .await?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "CNAME record created").await?;

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
pub async fn reconcile_mx_record(
    client: Client,
    record: MXRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling MXRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Find the cluster for this zone
    let cluster_ref = get_cluster_from_zone(&client, &namespace, &spec.zone).await?;

    // Load RNDC key
    let key_data = load_rndc_key(&client, &namespace, &cluster_ref).await?;

    // Build server address
    let server = format!("{cluster_ref}.{namespace}.svc.cluster.local:953");

    // Add the record to the zone
    zone_manager
        .add_mx_record(
            &spec.zone,
            &spec.name,
            spec.priority,
            &spec.mail_server,
            spec.ttl,
            &server,
            &key_data,
        )
        .await?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "MX record created").await?;

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
pub async fn reconcile_ns_record(
    client: Client,
    record: NSRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling NSRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Find the cluster for this zone
    let cluster_ref = get_cluster_from_zone(&client, &namespace, &spec.zone).await?;

    // Load RNDC key
    let key_data = load_rndc_key(&client, &namespace, &cluster_ref).await?;

    // Build server address
    let server = format!("{cluster_ref}.{namespace}.svc.cluster.local:953");

    // Add the record to the zone
    zone_manager
        .add_ns_record(
            &spec.zone,
            &spec.name,
            &spec.nameserver,
            spec.ttl,
            &server,
            &key_data,
        )
        .await?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "NS record created").await?;

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
pub async fn reconcile_srv_record(
    client: Client,
    record: SRVRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling SRVRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Find the cluster for this zone
    let cluster_ref = get_cluster_from_zone(&client, &namespace, &spec.zone).await?;

    // Load RNDC key
    let key_data = load_rndc_key(&client, &namespace, &cluster_ref).await?;

    // Build server address
    let server = format!("{cluster_ref}.{namespace}.svc.cluster.local:953");

    // Add the record to the zone
    let srv_data = crate::bind9::SRVRecordData {
        priority: spec.priority,
        weight: spec.weight,
        port: spec.port,
        target: spec.target.clone(),
        ttl: spec.ttl,
    };
    zone_manager
        .add_srv_record(&spec.zone, &spec.name, &srv_data, &server, &key_data)
        .await?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "SRV record created").await?;

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
pub async fn reconcile_caa_record(
    client: Client,
    record: CAARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CAARecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Find the cluster for this zone
    let cluster_ref = get_cluster_from_zone(&client, &namespace, &spec.zone).await?;

    // Load RNDC key
    let key_data = load_rndc_key(&client, &namespace, &cluster_ref).await?;

    // Build server address
    let server = format!("{cluster_ref}.{namespace}.svc.cluster.local:953");

    // Add the record to the zone
    zone_manager
        .add_caa_record(
            &spec.zone,
            &spec.name,
            spec.flags,
            &spec.tag,
            &spec.value,
            spec.ttl,
            &server,
            &key_data,
        )
        .await?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "CAA record created").await?;

    Ok(())
}

/// Retrieves the cluster reference for a given zone name.
///
/// Looks up the `DNSZone` resource in the specified namespace and returns
/// the `cluster_ref` field. This is used to identify which `Bind9Instance`
/// should serve the DNS records.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for the `DNSZone`
/// * `zone_name` - Name of the zone (e.g., "example.com")
///
/// # Returns
///
/// The `cluster_ref` string from the matching `DNSZone` resource
///
/// # Errors
///
/// Returns an error if:
/// - The `DNSZone` API is unavailable
/// - No `DNSZone` matches the given `zone_name`
/// - Multiple `DNSZones` have the same `zone_name` (should not happen in practice)
async fn get_cluster_from_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
) -> Result<String> {
    let zone_api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

    // List all zones in the namespace and find the one matching the zone_name
    let zones = zone_api.list(&ListParams::default()).await?;

    for zone in zones.items {
        if zone.spec.zone_name == zone_name {
            return Ok(zone.spec.cluster_ref);
        }
    }

    Err(anyhow!(
        "No DNSZone found for zone {zone_name} in namespace {namespace}"
    ))
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

/// Updates the status of a DNS record resource.
///
/// This is a placeholder implementation that logs the status update.
/// In production, this should update the Kubernetes resource status
/// subresource with appropriate conditions.
///
/// # Arguments
///
/// * `_client` - Kubernetes API client (unused in current implementation)
/// * `record` - The DNS record resource to update
/// * `_condition_type` - Type of condition (e.g., "Ready", "Failed")
/// * `_status` - Status value (e.g., "True", "False", "Unknown")
/// * `_message` - Human-readable message describing the status
///
/// # Returns
///
/// Always returns Ok in the current implementation
///
/// # TODO
///
/// Implement actual status update logic using the Kubernetes API:
/// - Create/update condition in `status.conditions` array
/// - Set `status.observed_generation`
/// - Call `client.replace_status()` to persist
#[allow(clippy::unused_async)]
async fn update_record_status(
    _client: &Client,
    record: &impl ResourceExt,
    _condition_type: &str,
    _status: &str,
    _message: &str,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    // TODO: In production you'd want type-specific status implementations
    // For now, we just log the status update
    info!("Updated status for {}/{}", namespace, name);

    Ok(())
}
