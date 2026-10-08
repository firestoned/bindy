// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Writing DNS records to BIND9 primaries.
//!
//! The one path every record write takes, whoever triggers it: the record
//! controllers push a record when it changes, and the zone controller replays a
//! recreated zone's records and deletes the records it no longer selects. Both
//! go through here, so the write is byte-for-byte the same either way. It moved
//! from the record controllers to this crate so the zone controller can use it
//! without depending on another controller crate (ADR-0009 §2, amended
//! 2026-10-05).
//!
//! - [`ReconcilableRecord`] maps each record kind to its [`RecordOperation`].
//! - [`add_record_to_instances_generic`] runs an operation on every primary endpoint.
//! - [`delete_record_from_primaries`] removes a record's RRset.
//! - [`replay_zone_records`] re-pushes a zone's records after the zone is recreated.

use crate::context::StoresBind9Ext;
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, PTRRecord, SRVRecord,
    TXTRecord,
};
use anyhow::{Context, Result};
use kube::{client::Client, Api, Resource, ResourceExt};
use tracing::{debug, warn};

/// Trait for record-specific BIND9 operations.
///
/// This trait abstracts over the different record types and provides a uniform interface
/// for adding records to BIND9 instances via the `Bind9Manager`.
///
/// Each DNS record type implements this trait to define how it should be added to BIND9
/// using dynamic DNS updates (RFC 2136 nsupdate protocol).
pub trait RecordOperation: Clone + Send + Sync {
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
/// It eliminates duplication across 9 record type reconcilers by providing
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
pub trait ReconcilableRecord:
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
    type Spec: serde::Serialize + Clone + Send + Sync;

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
/// This function eliminates duplication across the 9 `add_*_record_to_instances` functions
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
/// * `resolver` - Per-reconcile resolver for instance keys and endpoints
///   (ADR-0015); share one across every write of a reconcile
/// * `instance_refs` - Primary instance references
/// * `zone_name` - DNS zone name
/// * `record_name` - Record name within the zone
/// * `ttl` - Optional TTL value
/// * `record_op` - The record-specific operation to perform
///
/// # Errors
///
/// Returns an error if any dynamic DNS update fails.
#[allow(clippy::too_many_arguments)]
pub async fn add_record_to_instances_generic<R>(
    client: &Client,
    stores: &crate::context::Stores,
    resolver: &crate::instances::InstanceResolver,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record_name: &str,
    ttl: Option<i32>,
    record_op: R,
) -> Result<()>
where
    R: RecordOperation,
{
    use crate::instances::for_each_instance_endpoint;

    // Create a map of instance name -> namespace for quick lookup
    let instance_map: std::collections::HashMap<String, String> = instance_refs
        .iter()
        .map(|inst| (inst.name.clone(), inst.namespace.clone()))
        .collect();

    let (_first, _total) = for_each_instance_endpoint(
        resolver,
        instance_refs,
        true,      // with_rndc_key
        "dns-tcp", // Use DNS TCP port for dynamic updates
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = zone_name.to_string();
            let record_name = record_name.to_string();

            // Get namespace for this instance (always present: the
            // instance name came from `instance_refs`)
            let instance_namespace = instance_map
                .get(&instance_name)
                .cloned()
                .unwrap_or_default();

            // Create Bind9Manager for this specific instance with deployment-aware auth
            let zone_manager =
                stores.create_bind9_manager_for_instance_with_client(
                    &instance_name,
                    &instance_namespace,
                    Some(client.clone()),
                );

            // Clone record_op for the async block
            let record_op_clone = record_op.clone();

            async move {
                let key_data = rndc_key
                    .ok_or_else(|| anyhow::anyhow!("RNDC key was not loaded for {instance_name}"))?;

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
pub struct ARecordOp {
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
pub struct AAAARecordOp {
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
pub struct CNAMERecordOp {
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
pub struct TXTRecordOp {
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
pub struct MXRecordOp {
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
pub struct NSRecordOp {
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
pub struct SRVRecordOp {
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
pub struct CAARecordOp {
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

/// PTR record operation wrapper.
#[derive(Clone)]
pub struct PTRRecordOp {
    target: String,
}

impl RecordOperation for PTRRecordOp {
    fn record_type_name(&self) -> &'static str {
        "PTR"
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
        let ptr_data = crate::bind9::PTRRecordData {
            target: self.target.clone(),
            ttl,
        };
        zone_manager
            .add_ptr_record(zone_name, record_name, &ptr_data, server, key_data)
            .await
    }
}

/// Implement `ReconcilableRecord` for `PTRRecord`.
impl ReconcilableRecord for PTRRecord {
    type Spec = crate::crd::PTRRecordSpec;
    type Operation = PTRRecordOp;

    fn get_spec(&self) -> &Self::Spec {
        &self.spec
    }

    fn get_status(&self) -> Option<&crate::crd::RecordStatus> {
        self.status.as_ref()
    }

    fn record_type_name() -> &'static str {
        "PTR"
    }

    fn record_type_hickory() -> hickory_proto::rr::RecordType {
        hickory_proto::rr::RecordType::PTR
    }

    fn create_operation(spec: &Self::Spec) -> Self::Operation {
        PTRRecordOp {
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
/// * `resolver` - Per-reconcile resolver for instance keys and endpoints
///   (ADR-0015); share one across every write of a reconcile
/// * `primary_refs` - Primary instance references to delete the record from
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `record_name` - Record name within the zone (e.g., "www")
/// * `record_type_hickory` - hickory-proto `RecordType` of the record
/// * `fail_on_error` - When `true`, a failed DNS deletion on any endpoint fails
///   the call (used when the record data must be gone before proceeding).
///   When `false` (the finalizer), a failure is tolerated only on an endpoint
///   whose pod no longer holds the zone (terminating, gone, or its instance
///   deleted); a pod that still holds it always fails the call, so the
///   deletion is retried instead of orphaning the record.
///
/// # Errors
///
/// Returns an error if endpoint resolution fails, if a pod that still holds
/// the zone was not reached or its deletion failed, or if any DNS deletion
/// fails and `fail_on_error` is `true`.
#[allow(clippy::too_many_arguments)]
pub async fn delete_record_from_primaries(
    client: &Client,
    stores: &crate::context::Stores,
    resolver: &crate::instances::InstanceResolver,
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
    // Every endpoint is attempted: for_each does not stop at a failure.
    let failures: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    // Finalizer cleanup (fail_on_error=false) is not blocked by an instance
    // whose data is gone (instance deleted, pods gone): SkipUnavailable skips
    // it. It IS blocked, and retried, by a pod that still holds the zone but
    // is momentarily unreachable (a container restarting): skipping it left
    // the record served from the pod's surviving emptyDir (chaos suite). The
    // coverage check in for_each_instance_endpoint_with_policy enforces that
    // for both modes. Strict callers (fail_on_error=true) also propagate
    // lookup failures and fail on any endpoint failure.
    let failure_policy = if fail_on_error {
        crate::instances::EndpointFailurePolicy::Strict
    } else {
        crate::instances::EndpointFailurePolicy::SkipUnavailable
    };

    let (_first_endpoint, _total_endpoints) =
        crate::instances::for_each_instance_endpoint_with_policy(
            resolver,
            primary_refs,
            true,      // with_rndc_key
            "dns-tcp", // Use DNS TCP port for dynamic updates
            failure_policy,
            |pod_endpoint, instance_name, rndc_key| {
                let zone_name = zone_name.to_string();
                let record_name_str = record_name.to_string();
                let instance_namespace = instance_map
                    .get(&instance_name)
                    .cloned()
                    .unwrap_or_default();
                let failures = std::sync::Arc::clone(&failures);

                // Create Bind9Manager for this specific instance with deployment-aware auth
                let zone_manager =
                    stores.create_bind9_manager_for_instance_with_client(
                    &instance_name,
                    &instance_namespace,
                    Some(client.clone()),
                );

                async move {
                    let delete_result = match rndc_key {
                        Some(key_data) => {
                            zone_manager
                                .delete_record(
                                    &zone_name,
                                    &record_name_str,
                                    record_type_hickory,
                                    &pod_endpoint,
                                    &key_data,
                                )
                                .await
                        }
                        None => Err(anyhow::anyhow!("RNDC key was not loaded")),
                    };

                    match delete_result {
                        Ok(()) => {
                            debug!(
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
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .push(format!(
                                    "endpoint {pod_endpoint} (instance: {instance_name}): {e}"
                                ));
                            // Reported, so a pod that still holds the zone
                            // counts as not reached (coverage check).
                            return Err(e);
                        }
                    }

                    Ok(())
                }
            },
        )
        .await?;

    let failures = failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

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

// ============================================================================
// Zone record replay (recovery after a BIND9 pod or Deployment is wiped)
// ============================================================================

/// Result of replaying a zone's record CRs into BIND9.
///
/// Produced by [`replay_zone_records`]. The replay is only complete - and the
/// zone only safe to report as Ready - when `failures` is empty.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecordReplayOutcome {
    /// Number of record references the replay tried to push.
    pub attempted: usize,
    /// Number of record references pushed to every primary endpoint successfully.
    pub succeeded: usize,
    /// Number of record references not replayed because the record no longer
    /// exists or is being deleted: replaying those would re-publish data the
    /// record's finalizer is removing.
    pub skipped: usize,
    /// Human-readable description of every record that could not be pushed.
    pub failures: Vec<String>,
}

impl RecordReplayOutcome {
    /// Whether every attempted record reached every primary endpoint.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty()
    }

    /// One-line summary suitable for a status condition message.
    #[must_use]
    pub fn summary(&self, zone_name: &str) -> String {
        if self.is_complete() {
            return format!(
                "Replayed {}/{} record(s) into zone {zone_name}",
                self.succeeded + self.skipped,
                self.attempted
            );
        }

        format!(
            "Replayed {}/{} record(s) into zone {zone_name}; {} failed: {}",
            self.succeeded + self.skipped,
            self.attempted,
            self.failures.len(),
            self.failures.join("; ")
        )
    }
}

/// Whether a record CR is still meant to be in DNS and may be replayed.
///
/// A record with `deletionTimestamp` set is being removed by its finalizer.
/// Replaying it would race that finalizer and could re-publish the RRset
/// after the finalizer deleted it, leaving it served with no record CR left
/// to clean it up.
#[must_use]
pub fn should_replay(meta: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta) -> bool {
    meta.deletion_timestamp.is_none()
}

/// What replaying one record did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayAction {
    /// The record was pushed to every primary endpoint.
    Pushed,
    /// The record is gone or terminating and was deliberately not pushed.
    Skipped,
}

/// Push a single record CR of a known type to every primary endpoint.
///
/// Fetches the record from the API server (the reflector store may lag behind a
/// just-created zone) and reuses the same BIND9 write path as the record
/// controller, so a replayed record is byte-for-byte what a normal reconcile
/// would have written. A record that no longer exists or is being deleted is
/// skipped (see [`should_replay`]).
async fn replay_single_record<T>(
    client: &Client,
    stores: &crate::context::Stores,
    resolver: &crate::instances::InstanceResolver,
    zone_name: &str,
    namespace: &str,
    name: &str,
    primary_refs: &[crate::crd::InstanceReference],
) -> Result<ReplayAction>
where
    T: ReconcilableRecord,
{
    let api: Api<T> = Api::namespaced(client.clone(), namespace);
    let Some(record) = api
        .get_opt(name)
        .await
        .with_context(|| format!("Failed to get {} {namespace}/{name}", T::record_type_name()))?
    else {
        debug!(
            "Not replaying {} {namespace}/{name}: it no longer exists",
            T::record_type_name()
        );
        return Ok(ReplayAction::Skipped);
    };

    if !should_replay(record.meta()) {
        debug!(
            "Not replaying {} {namespace}/{name}: it is being deleted",
            T::record_type_name()
        );
        return Ok(ReplayAction::Skipped);
    }

    let spec = record.get_spec();

    add_record_to_instances_generic(
        client,
        stores,
        resolver,
        primary_refs,
        zone_name,
        T::get_record_name(spec),
        T::get_ttl(spec),
        T::create_operation(spec),
    )
    .await?;
    Ok(ReplayAction::Pushed)
}

/// Re-push every record CR selected by a zone into that zone on BIND9.
///
/// # Why this exists
///
/// BIND9 operand pods hold zone data in ephemeral storage. When a pod - or the
/// whole Deployment - is wiped, the zone is gone, and the zone reconciler
/// recreates it from `spec`: SOA and NS records only. The server is then
/// *authoritative* for a zone with no data, so it answers authoritative
/// NXDOMAIN (or, with `recursion` and `forwarders` enabled, silently returns
/// the public answer) for every name it should be serving. Record CRs are not
/// replayed by their own controllers because, from Kubernetes' point of view,
/// nothing about them changed.
///
/// This function closes that gap: whenever the zone reconciler *creates* a zone
/// on any endpoint, it replays the zone's records immediately, in the same
/// reconciliation, rather than waiting for an unrelated record event.
///
/// The push itself is idempotent (each record type queries the server first and
/// writes an RRset only when it differs), so replaying against endpoints that
/// already hold the data costs one DNS query per record and changes nothing.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `stores` - Context stores used to build a `Bind9Manager` per instance
/// * `zone_name` - DNS zone name (e.g. "example.com")
/// * `record_refs` - The zone's `status.records[]` entries to replay
/// * `primary_refs` - PRIMARY instances to push to (secondaries pull via AXFR)
///
/// # Returns
///
/// A [`RecordReplayOutcome`] describing what was pushed. Individual record
/// failures are collected rather than propagated so that one broken record
/// cannot block the recovery of every other record in the zone.
pub async fn replay_zone_records(
    client: &Client,
    stores: &crate::context::Stores,
    zone_name: &str,
    record_refs: &[crate::crd::RecordReferenceWithTimestamp],
    primary_refs: &[crate::crd::InstanceReference],
) -> RecordReplayOutcome {
    if record_refs.is_empty() || primary_refs.is_empty() {
        return RecordReplayOutcome::default();
    }

    // One resolver for the whole replay: each primary's RNDC key and
    // endpoints are read once, not once per record (ADR-0015).
    let resolver = crate::instances::InstanceResolver::for_kube(client, stores);
    replay_zone_records_with(
        client,
        stores,
        &resolver,
        zone_name,
        record_refs,
        primary_refs,
    )
    .await
}

/// [`replay_zone_records`] through a caller-supplied resolver.
///
/// The zones-loaded readiness gate passes a resolver restricted to one pod
/// (`InstanceResolver::for_single_pod`), so the replay writes every record to
/// the pod being admitted and to no other (ADR-0017).
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `stores` - Context stores used to build a `Bind9Manager` per instance
/// * `resolver` - Where the primaries' RNDC keys and endpoints come from
/// * `zone_name` - DNS zone name (e.g. "example.com")
/// * `record_refs` - The records to replay
/// * `primary_refs` - PRIMARY instances to push to (secondaries pull via AXFR)
///
/// # Returns
///
/// A [`RecordReplayOutcome`]; individual record failures are collected, not
/// propagated.
pub async fn replay_zone_records_with(
    client: &Client,
    stores: &crate::context::Stores,
    resolver: &crate::instances::InstanceResolver,
    zone_name: &str,
    record_refs: &[crate::crd::RecordReferenceWithTimestamp],
    primary_refs: &[crate::crd::InstanceReference],
) -> RecordReplayOutcome {
    let mut outcome = RecordReplayOutcome::default();

    if record_refs.is_empty() || primary_refs.is_empty() {
        return outcome;
    }

    for record_ref in record_refs {
        outcome.attempted += 1;

        let namespace = record_ref.namespace.as_str();
        let name = record_ref.name.as_str();

        let result = match replay_dispatch(
            client,
            stores,
            resolver,
            zone_name,
            &record_ref.kind,
            namespace,
            name,
            primary_refs,
        )
        .await
        {
            Some(result) => result,
            None => {
                warn!(
                    "Cannot replay unknown record kind '{}' for {}/{} in zone {}",
                    record_ref.kind, namespace, name, zone_name
                );
                outcome.failures.push(format!(
                    "{} {namespace}/{name}: unknown record kind",
                    record_ref.kind
                ));
                continue;
            }
        };

        match result {
            Ok(ReplayAction::Skipped) => {
                outcome.skipped += 1;
            }
            Ok(ReplayAction::Pushed) => {
                outcome.succeeded += 1;
                debug!(
                    "Replayed {} {}/{} into zone {}",
                    record_ref.kind, namespace, name, zone_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to replay {} {}/{} into zone {}: {e:#}",
                    record_ref.kind, namespace, name, zone_name
                );
                outcome
                    .failures
                    .push(format!("{} {namespace}/{name}: {e}", record_ref.kind));
            }
        }
    }

    outcome
}

/// Dispatch a replay to the concrete record type named by `kind`.
///
/// `kind` is the value stored in `DNSZone.status.records[].kind`, which is
/// always [`crate::crd::DNSRecordKind::as_str`]. Returns `None` when it names a
/// kind this operator does not manage, so the caller can distinguish "unknown
/// kind" from "push failed".
#[allow(clippy::too_many_arguments)]
async fn replay_dispatch(
    client: &Client,
    stores: &crate::context::Stores,
    resolver: &crate::instances::InstanceResolver,
    zone_name: &str,
    kind: &str,
    namespace: &str,
    name: &str,
    primary_refs: &[crate::crd::InstanceReference],
) -> Option<Result<ReplayAction>> {
    use crate::crd::DNSRecordKind;

    let kind = DNSRecordKind::try_from(kind).ok()?;

    macro_rules! replay {
        ($record_type:ty) => {
            Some(
                replay_single_record::<$record_type>(
                    client,
                    stores,
                    resolver,
                    zone_name,
                    namespace,
                    name,
                    primary_refs,
                )
                .await,
            )
        };
    }

    match kind {
        DNSRecordKind::A => replay!(ARecord),
        DNSRecordKind::AAAA => replay!(AAAARecord),
        DNSRecordKind::TXT => replay!(TXTRecord),
        DNSRecordKind::CNAME => replay!(CNAMERecord),
        DNSRecordKind::MX => replay!(MXRecord),
        DNSRecordKind::NS => replay!(NSRecord),
        DNSRecordKind::SRV => replay!(SRVRecord),
        DNSRecordKind::CAA => replay!(CAARecord),
        DNSRecordKind::PTR => replay!(PTRRecord),
    }
}

#[cfg(test)]
#[path = "record_push_tests.rs"]
mod record_push_tests;
