// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

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

// Removed ANNOTATION_ZONE_OWNER - using status.zoneRef instead (event-driven architecture)
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, DNSZone, MXRecord, NSRecord, SRVRecord, TXTRecord,
};
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
    let instance_refs = match crate::reconcilers::dnszone::validation::get_instances_from_zone(
        &dnszone,
        bind9_instances_store,
    ) {
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
    let primary_refs = match crate::reconcilers::dnszone::primary::filter_primary_instances(
        client,
        &instance_refs,
    )
    .await
    {
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
/// Trait for record-specific BIND9 operations.
///
/// This trait abstracts over the different record types and provides a uniform interface
/// for adding records to BIND9 instances via the `Bind9Manager`.
///
/// Each DNS record type implements this trait to define how it should be added to BIND9
/// using dynamic DNS updates (RFC 2136 nsupdate protocol).
trait RecordOperation: Clone + Send + Sync {
    /// Get the record type name (e.g., "A", "TXT", "AAAA") for logging and events.
    fn record_type_name(&self) -> &'static str;

    /// Add this record to a BIND9 instance via the `Bind9Manager`.
    ///
    /// # Arguments
    ///
    /// * `zone_manager` - The `Bind9Manager` instance to use for the operation
    /// * `zone_name` - The DNS zone name (e.g., "example.com")
    /// * `record_name` - The record name within the zone (e.g., "www")
    /// * `ttl` - Optional TTL value
    /// * `server` - The BIND9 server endpoint (IP:port)
    /// * `key_data` - RNDC key data for authentication
    ///
    /// # Errors
    ///
    /// Returns an error if the dynamic DNS update fails.
    fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// Trait for DNS record resources that can be reconciled.
///
/// This trait provides the interface for generic record reconciliation,
/// allowing a single `reconcile_record<T>()` function to handle all record types.
/// It eliminates duplication across 8 record type reconcilers by providing
/// type-specific operations through trait methods.
///
/// # Example
///
/// ```rust,ignore
/// impl ReconcilableRecord for ARecord {
///     type Spec = crate::crd::ARecordSpec;
///     type Operation = ARecordOp;
///
///     fn get_spec(&self) -> &Self::Spec {
///         &self.spec
///     }
///
///     fn record_type_name() -> &'static str {
///         "A"
///     }
///
///     fn create_operation(spec: &Self::Spec) -> Self::Operation {
///         ARecordOp {
///             ipv4_address: spec.ipv4_address.clone(),
///         }
///     }
///
///     fn get_record_name(spec: &Self::Spec) -> &str {
///         &spec.name
///     }
///
///     fn get_ttl(spec: &Self::Spec) -> Option<i32> {
///         spec.ttl
///     }
/// }
/// ```
trait ReconcilableRecord:
    Resource<DynamicType = (), Scope = k8s_openapi::NamespaceResourceScope>
    + ResourceExt
    + Clone
    + std::fmt::Debug
    + serde::Serialize
    + for<'de> serde::Deserialize<'de>
    + Send
    + Sync
{
    /// The spec type for this record (e.g., `ARecordSpec`, `TXTRecordSpec`)
    type Spec: serde::Serialize + Clone;

    /// The operation type for BIND9 updates (e.g., `ARecordOp`, `TXTRecordOp`)
    type Operation: RecordOperation;

    /// Get the record's spec
    fn get_spec(&self) -> &Self::Spec;

    /// Get the record's status, if any
    fn get_status(&self) -> Option<&crate::crd::RecordStatus>;

    /// Get the record type name (e.g., "A", "TXT", "AAAA") for logging
    fn record_type_name() -> &'static str;

    /// Get the `hickory_proto` record type used for DNS deletion operations
    fn record_type_hickory() -> hickory_proto::rr::RecordType;

    /// Create the BIND9 operation from the spec
    fn create_operation(spec: &Self::Spec) -> Self::Operation;

    /// Get the record name from the spec
    fn get_record_name(spec: &Self::Spec) -> &str;

    /// Get the TTL from the spec
    fn get_ttl(spec: &Self::Spec) -> Option<i32>;

    /// Comma-separated display addresses for `status.addresses` (A/AAAA only).
    ///
    /// Returns `None` for record types that do not publish addresses.
    fn get_display_addresses(_spec: &Self::Spec) -> Option<String> {
        None
    }
}

/// Generic helper to add a record to all primary instances.
///
/// This function eliminates duplication across the 8 `add_*_record_to_instances` functions
/// by providing a generic implementation that works for any record type implementing
/// the `RecordOperation` trait.
///
/// # Type Parameters
///
/// * `R` - The record operation type implementing `RecordOperation`
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `stores` - Context stores for creating `Bind9Manager` instances
/// * `instance_refs` - Primary instance references
/// * `zone_name` - DNS zone name
/// * `record_name` - Record name within the zone
/// * `ttl` - Optional TTL value
/// * `record_op` - The record-specific operation to perform
///
/// # Errors
///
/// Returns an error if any dynamic DNS update fails.
async fn add_record_to_instances_generic<R>(
    client: &Client,
    stores: &crate::context::Stores,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    ttl: Option<i32>,
    record_op: R,
) -> Result<()>
where
    R: RecordOperation,
{
    use crate::reconcilers::dnszone::helpers::for_each_instance_endpoint;

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

            // Get namespace for this instance
            let instance_namespace = instance_map
                .get(&instance_name)
                .expect("Instance should be in map")
                .clone();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager =
                stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

            // Clone record_op for the async block
            let record_op_clone = record_op.clone();

            async move {
                let key_data = rndc_key.expect("RNDC key should be loaded");

                record_op_clone
                    .add_to_bind9(&zone_manager, &zone_name, &record_name, ttl, &pod_endpoint, &key_data)
                    .await
                    .context(format!(
                        "Failed to add {} record {record_name}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})",
                        record_op_clone.record_type_name()
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

// Record operation implementations for each DNS record type

/// A record operation wrapper.
#[derive(Clone)]
struct ARecordOp {
    ipv4_addresses: Vec<String>,
}

impl RecordOperation for ARecordOp {
    fn record_type_name(&self) -> &'static str {
        "A"
    }

    async fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> Result<()> {
        zone_manager
            .add_a_record(
                zone_name,
                record_name,
                &self.ipv4_addresses,
                ttl,
                server,
                key_data,
            )
            .await
    }
}

/// Implement `ReconcilableRecord` for `ARecord`.
impl ReconcilableRecord for ARecord {
    type Spec = crate::crd::ARecordSpec;
    type Operation = ARecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "A"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::A
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        ARecordOp {
            ipv4_addresses: spec.ipv4_addresses.clone(),
        }
    }

    fn get_record_name(spec: &Self::Spec) -> &str {
        &spec.name
    }

    fn get_ttl(spec: &Self::Spec) -> Option<i32> {
        spec.ttl
    }

    fn get_display_addresses(spec: &Self::Spec) -> Option<String> {
        Some(spec.ipv4_addresses.join(","))
    }
}

/// AAAA record operation wrapper.
#[derive(Clone)]
struct AAAARecordOp {
    ipv6_addresses: Vec<String>,
}

impl RecordOperation for AAAARecordOp {
    fn record_type_name(&self) -> &'static str {
        "AAAA"
    }

    async fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> Result<()> {
        zone_manager
            .add_aaaa_record(
                zone_name,
                record_name,
                &self.ipv6_addresses,
                ttl,
                server,
                key_data,
            )
            .await
    }
}

/// Implement `ReconcilableRecord` for `AAAARecord`.
impl ReconcilableRecord for AAAARecord {
    type Spec = crate::crd::AAAARecordSpec;
    type Operation = AAAARecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "AAAA"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::AAAA
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        AAAARecordOp {
            ipv6_addresses: spec.ipv6_addresses.clone(),
        }
    }

    fn get_record_name(spec: &Self::Spec) -> &str {
        &spec.name
    }

    fn get_ttl(spec: &Self::Spec) -> Option<i32> {
        spec.ttl
    }

    fn get_display_addresses(spec: &Self::Spec) -> Option<String> {
        Some(spec.ipv6_addresses.join(","))
    }
}

/// CNAME record operation wrapper.
#[derive(Clone)]
struct CNAMERecordOp {
    target: String,
}

impl RecordOperation for CNAMERecordOp {
    fn record_type_name(&self) -> &'static str {
        "CNAME"
    }

    async fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> Result<()> {
        zone_manager
            .add_cname_record(zone_name, record_name, &self.target, ttl, server, key_data)
            .await
    }
}

/// Implement `ReconcilableRecord` for `CNAMERecord`.
impl ReconcilableRecord for CNAMERecord {
    type Spec = crate::crd::CNAMERecordSpec;
    type Operation = CNAMERecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "CNAME"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::CNAME
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        CNAMERecordOp {
            target: spec.target.clone(),
        }
    }

    fn get_record_name(spec: &Self::Spec) -> &str {
        &spec.name
    }

    fn get_ttl(spec: &Self::Spec) -> Option<i32> {
        spec.ttl
    }
}

/// TXT record operation wrapper.
#[derive(Clone)]
struct TXTRecordOp {
    texts: Vec<String>,
}

impl RecordOperation for TXTRecordOp {
    fn record_type_name(&self) -> &'static str {
        "TXT"
    }

    async fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> Result<()> {
        zone_manager
            .add_txt_record(zone_name, record_name, &self.texts, ttl, server, key_data)
            .await
    }
}

/// Implement `ReconcilableRecord` for `TXTRecord`.
impl ReconcilableRecord for TXTRecord {
    type Spec = crate::crd::TXTRecordSpec;
    type Operation = TXTRecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "TXT"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::TXT
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        TXTRecordOp {
            texts: spec.text.clone(),
        }
    }

    fn get_record_name(spec: &Self::Spec) -> &str {
        &spec.name
    }

    fn get_ttl(spec: &Self::Spec) -> Option<i32> {
        spec.ttl
    }
}

/// MX record operation wrapper.
#[derive(Clone)]
struct MXRecordOp {
    priority: i32,
    mail_server: String,
}

impl RecordOperation for MXRecordOp {
    fn record_type_name(&self) -> &'static str {
        "MX"
    }

    async fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> Result<()> {
        zone_manager
            .add_mx_record(
                zone_name,
                record_name,
                self.priority,
                &self.mail_server,
                ttl,
                server,
                key_data,
            )
            .await
    }
}

/// Implement `ReconcilableRecord` for `MXRecord`.
impl ReconcilableRecord for MXRecord {
    type Spec = crate::crd::MXRecordSpec;
    type Operation = MXRecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "MX"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::MX
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        MXRecordOp {
            priority: spec.priority,
            mail_server: spec.mail_server.clone(),
        }
    }

    fn get_record_name(spec: &Self::Spec) -> &str {
        &spec.name
    }

    fn get_ttl(spec: &Self::Spec) -> Option<i32> {
        spec.ttl
    }
}

/// NS record operation wrapper.
#[derive(Clone)]
struct NSRecordOp {
    nameserver: String,
}

impl RecordOperation for NSRecordOp {
    fn record_type_name(&self) -> &'static str {
        "NS"
    }

    async fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> Result<()> {
        zone_manager
            .add_ns_record(
                zone_name,
                record_name,
                &self.nameserver,
                ttl,
                server,
                key_data,
            )
            .await
    }
}

/// Implement `ReconcilableRecord` for `NSRecord`.
impl ReconcilableRecord for NSRecord {
    type Spec = crate::crd::NSRecordSpec;
    type Operation = NSRecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "NS"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::NS
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        NSRecordOp {
            nameserver: spec.nameserver.clone(),
        }
    }

    fn get_record_name(spec: &Self::Spec) -> &str {
        &spec.name
    }

    fn get_ttl(spec: &Self::Spec) -> Option<i32> {
        spec.ttl
    }
}

/// SRV record operation wrapper.
#[derive(Clone)]
struct SRVRecordOp {
    priority: i32,
    weight: i32,
    port: i32,
    target: String,
}

impl RecordOperation for SRVRecordOp {
    fn record_type_name(&self) -> &'static str {
        "SRV"
    }

    async fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> Result<()> {
        let srv_data = crate::bind9::SRVRecordData {
            priority: self.priority,
            weight: self.weight,
            port: self.port,
            target: self.target.clone(),
            ttl,
        };
        zone_manager
            .add_srv_record(zone_name, record_name, &srv_data, server, key_data)
            .await
    }
}

/// Implement `ReconcilableRecord` for `SRVRecord`.
impl ReconcilableRecord for SRVRecord {
    type Spec = crate::crd::SRVRecordSpec;
    type Operation = SRVRecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "SRV"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::SRV
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        SRVRecordOp {
            priority: spec.priority,
            weight: spec.weight,
            port: spec.port,
            target: spec.target.clone(),
        }
    }

    fn get_record_name(spec: &Self::Spec) -> &str {
        &spec.name
    }

    fn get_ttl(spec: &Self::Spec) -> Option<i32> {
        spec.ttl
    }
}

/// CAA record operation wrapper.
#[derive(Clone)]
struct CAARecordOp {
    flags: i32,
    tag: String,
    value: String,
}

impl RecordOperation for CAARecordOp {
    fn record_type_name(&self) -> &'static str {
        "CAA"
    }

    async fn add_to_bind9(
        &self,
        zone_manager: &crate::bind9::Bind9Manager,
        zone_name: &str,
        record_name: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &crate::bind9::RndcKeyData,
    ) -> Result<()> {
        zone_manager
            .add_caa_record(
                zone_name,
                record_name,
                self.flags,
                &self.tag,
                &self.value,
                ttl,
                server,
                key_data,
            )
            .await
    }
}

/// Implement `ReconcilableRecord` for `CAARecord`.
impl ReconcilableRecord for CAARecord {
    type Spec = crate::crd::CAARecordSpec;
    type Operation = CAARecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "CAA"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::CAA
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        CAARecordOp {
            flags: spec.flags,
            tag: spec.tag.clone(),
            value: spec.value.clone(),
        }
    }

    fn get_record_name(spec: &Self::Spec) -> &str {
        &spec.name
    }

    fn get_ttl(spec: &Self::Spec) -> Option<i32> {
        spec.ttl
    }
}

/// Generic record reconciliation function.
///
/// This function handles reconciliation for all DNS record types that implement
/// the `ReconcilableRecord` trait. It eliminates duplication across 8 record types
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
async fn reconcile_record<T>(ctx: std::sync::Arc<crate::context::Context>, record: T) -> Result<()>
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
            warn!(
                "Failed to add {} record {}.{}: {}",
                T::record_type_name(),
                T::get_record_name(spec),
                rec_ctx.zone_ref.zone_name,
                e
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

/// Reconciles an `ARecord` (IPv4 address) resource.
///
/// This is a thin wrapper around the generic `reconcile_record<T>()` function.
/// It finds `DNSZones` that have selected this record via label selectors and
/// creates/updates the record in BIND9 primaries for those zones using dynamic DNS updates.
///
/// `status.addresses` (comma-separated IPv4 addresses for kubectl output) is only
/// published after a successful, selected reconcile.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
pub async fn reconcile_a_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: ARecord,
) -> Result<()> {
    reconcile_record(ctx, record).await
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
    reconcile_record(ctx, record).await
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
    reconcile_record(ctx, record).await
}

/// Reconciles a `CNAMERecord` \(canonical name alias\) resource.
///
/// This is a thin wrapper around the generic `reconcile_record<T>()` function.
/// It finds `DNSZones` that have selected this record via label selectors and
/// creates/updates the record in BIND9 primaries for those zones using dynamic DNS updates.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
pub async fn reconcile_cname_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: CNAMERecord,
) -> Result<()> {
    reconcile_record(ctx, record).await
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
pub async fn reconcile_mx_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: MXRecord,
) -> Result<()> {
    reconcile_record(ctx, record).await
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
pub async fn reconcile_ns_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: NSRecord,
) -> Result<()> {
    reconcile_record(ctx, record).await
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
pub async fn reconcile_srv_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: SRVRecord,
) -> Result<()> {
    reconcile_record(ctx, record).await
}

/// Reconciles a `CAARecord` \(certificate authority authorization\) resource.
///
/// This is a thin wrapper around the generic `reconcile_record<T>()` function.
/// It finds `DNSZones` that have selected this record via label selectors and
/// creates/updates the record in BIND9 primaries for those zones using dynamic DNS updates.
/// CAA records specify which certificate authorities can issue certificates.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 record creation fails.
pub async fn reconcile_caa_record(
    ctx: std::sync::Arc<crate::context::Context>,
    record: CAARecord,
) -> Result<()> {
    reconcile_record(ctx, record).await
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
    let instance_refs = match crate::reconcilers::dnszone::validation::get_instances_from_zone(
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
    let primary_refs = match crate::reconcilers::dnszone::primary::filter_primary_instances(
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

/// Deletes a DNS record (by name and type) from all given primary instances.
///
/// Shared by the record finalizer (`delete_record`), the rename cleanup in
/// `reconcile_record`, and the `DNSZone` controller when a record is no longer
/// selected by the zone's `recordsFrom` selectors.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `stores` - Context stores for creating `Bind9Manager` instances
/// * `primary_refs` - Primary instance references to delete the record from
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `record_name` - Record name within the zone (e.g., "www")
/// * `record_type_hickory` - hickory-proto `RecordType` of the record
/// * `fail_on_error` - When `true`, a failed DNS deletion on any endpoint fails
///   the call (used when the record data must be gone before proceeding).
///   When `false`, failures are logged and skipped (best-effort finalizer cleanup).
///
/// # Errors
///
/// Returns an error if endpoint resolution fails, or if a DNS deletion fails
/// and `fail_on_error` is `true`.
pub(crate) async fn delete_record_from_primaries(
    client: &Client,
    stores: &crate::context::Stores,
    primary_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    record_type_hickory: hickory_proto::rr::RecordType,
    fail_on_error: bool,
) -> Result<()> {
    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = primary_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    // Collect per-endpoint failures ourselves: for_each_instance_endpoint only
    // fails when ALL endpoints fail, but with fail_on_error we must also fail
    // on PARTIAL failures (a record left on any endpoint is still an orphan).
    // The closure always returns Ok so every endpoint is attempted.
    let failures: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    // Best-effort finalizer cleanup (fail_on_error=false) must not be blocked
    // forever by an instance whose RNDC Secret is gone or that has zero ready
    // endpoints - the DNS data there is unreachable anyway. Strict callers
    // (fail_on_error=true) keep propagating those lookup failures.
    let failure_policy = if fail_on_error {
        crate::reconcilers::dnszone::helpers::EndpointFailurePolicy::Strict
    } else {
        crate::reconcilers::dnszone::helpers::EndpointFailurePolicy::SkipUnavailable
    };

    let (_first_endpoint, _total_endpoints) =
        crate::reconcilers::dnszone::helpers::for_each_instance_endpoint_with_policy(
            client,
            primary_refs,
            true,      // with_rndc_key
            "dns-tcp", // Use DNS TCP port for dynamic updates
            failure_policy,
            |pod_endpoint, instance_name, rndc_key| {
                let zone_name = zone_name.to_string();
                let record_name_str = record_name.to_string();
                let instance_namespace = instance_map
                    .get(&instance_name)
                    .expect("Instance should be in map")
                    .clone();
                let failures = std::sync::Arc::clone(&failures);

                // Create Bind9Manager for this specific instance with deployment-aware auth
                let zone_manager =
                    stores.create_bind9_manager_for_instance(&instance_name, &instance_namespace);

                async move {
                    let key_data = rndc_key.expect("RNDC key should be loaded");

                    let delete_result = zone_manager
                        .delete_record(
                            &zone_name,
                            &record_name_str,
                            record_type_hickory,
                            &pod_endpoint,
                            &key_data,
                        )
                        .await;

                    match delete_result {
                        Ok(()) => {
                            info!(
                                "Successfully deleted {} record {}.{} from endpoint {} (instance: {})",
                                record_type_hickory, record_name_str, zone_name, pod_endpoint, instance_name
                            );
                        }
                        Err(e) => {
                            warn!(
                                "Failed to delete {} record {}.{} from endpoint {} (instance: {}): {}",
                                record_type_hickory, record_name_str, zone_name, pod_endpoint, instance_name, e
                            );
                            failures
                                .lock()
                                .expect("delete failures mutex should not be poisoned")
                                .push(format!(
                                    "endpoint {pod_endpoint} (instance: {instance_name}): {e}"
                                ));
                        }
                    }

                    Ok(())
                }
            },
        )
        .await?;

    let failures = failures
        .lock()
        .expect("delete failures mutex should not be poisoned");

    if fail_on_error && !failures.is_empty() {
        return Err(anyhow::anyhow!(
            "Failed to delete {} record {}.{} from {} endpoint(s): {}",
            record_type_hickory,
            record_name,
            zone_name,
            failures.len(),
            failures.join("; ")
        ));
    }

    if !failures.is_empty() {
        warn!(
            "Failed to delete {} record {}.{} from {} endpoint(s); continuing anyway (best-effort)",
            record_type_hickory,
            record_name,
            zone_name,
            failures.len()
        );
    }

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
