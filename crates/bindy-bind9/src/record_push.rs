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
//! - [`ReconcilableRecord`] is the one hand-written part of a record kind: how
//!   its spec becomes DNS data (ADR-0020). Everything else about a kind comes
//!   from the record-kind table through [`RecordKind`].
//! - [`add_record_to_instances_generic`] writes a record to every primary endpoint.
//! - [`delete_record_from_primaries`] removes a record's RRset.
//! - [`replay_zone_records`] re-pushes a zone's records after the zone is recreated.

use crate::bind9::{Bind9Manager, PTRRecordData, RndcKeyData, SRVRecordData};
use crate::context::{RecordKind, StoresBind9Ext};
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, PTRRecord, SRVRecord,
    TXTRecord,
};
use anyhow::{Context, Result};
use kube::{client::Client, Api};
use tracing::{debug, warn};

/// How a record kind's spec becomes the DNS data BIND9 publishes.
///
/// The only per-kind code of a record kind outside the record-kind table
/// (ADR-0020): one impl per kind, below, calling the matching
/// `Bind9Manager::add_*_record`. The kind string, finalizer, hickory type,
/// record name, TTL and status come from the [`RecordKind`] supertrait.
pub trait ReconcilableRecord: RecordKind {
    /// Publish this record's data on one BIND9 endpoint via dynamic DNS
    /// update (RFC 2136), under its `spec.name` and `spec.ttl`.
    ///
    /// # Arguments
    ///
    /// * `zone_manager` - The `Bind9Manager` of the instance behind `server`
    /// * `zone_name` - The DNS zone name (e.g., "example.com")
    /// * `server` - The BIND9 server endpoint (IP:port)
    /// * `key_data` - RNDC key data for authentication
    ///
    /// # Errors
    ///
    /// Returns an error if the dynamic DNS update fails.
    fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Comma-separated display addresses for `status.addresses` (A/AAAA only).
    ///
    /// Returns `None` for record types that do not publish addresses.
    fn display_addresses(&self) -> Option<String> {
        None
    }
}

/// The DNS type of record kind `T` (e.g. `A`), as logs and status messages
/// name it.
#[must_use]
pub fn record_type<T: RecordKind>() -> hickory_proto::rr::RecordType {
    T::DNS_KIND.to_hickory_record_type()
}

/// Write `record` to all primary instances.
///
/// One implementation for every record kind: the kind-specific part is the
/// record's [`ReconcilableRecord::add_to_bind9`].
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `stores` - Context stores for creating `Bind9Manager` instances
/// * `resolver` - Per-reconcile resolver for instance keys and endpoints
///   (ADR-0015); share one across every write of a reconcile
/// * `instance_refs` - Primary instance references
/// * `zone_name` - DNS zone name
/// * `record` - The record to publish
///
/// # Errors
///
/// Returns an error if any dynamic DNS update fails.
pub async fn add_record_to_instances_generic<T>(
    client: &Client,
    stores: &crate::context::Stores,
    resolver: &crate::instances::InstanceResolver,
    instance_refs: &[crate::crd::InstanceReference],
    zone_name: &str,
    record: &T,
) -> Result<()>
where
    T: ReconcilableRecord,
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

            // Clone the record for the async block
            let record = record.clone();

            async move {
                let key_data = rndc_key
                    .ok_or_else(|| anyhow::anyhow!("RNDC key was not loaded for {instance_name}"))?;

                record
                    .add_to_bind9(&zone_manager, &zone_name, &pod_endpoint, &key_data)
                    .await
                    .context(format!(
                        "Failed to add {} record {}.{zone_name} to primary {pod_endpoint} (instance: {instance_name})",
                        record_type::<T>(),
                        record.spec_name(),
                    ))?;

                Ok(())
            }
        },
    )
    .await?;

    Ok(())
}

// How each record kind's spec becomes DNS data: the one hand-written part of
// a kind (ADR-0020). Every other per-kind fact comes from the table.

impl ReconcilableRecord for ARecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        zone_manager
            .add_a_record(
                zone_name,
                &spec.name,
                &spec.ipv4_addresses,
                spec.ttl,
                server,
                key_data,
            )
            .await
    }

    fn display_addresses(&self) -> Option<String> {
        Some(self.spec.ipv4_addresses.join(","))
    }
}

impl ReconcilableRecord for AAAARecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        zone_manager
            .add_aaaa_record(
                zone_name,
                &spec.name,
                &spec.ipv6_addresses,
                spec.ttl,
                server,
                key_data,
            )
            .await
    }

    fn display_addresses(&self) -> Option<String> {
        Some(self.spec.ipv6_addresses.join(","))
    }
}

impl ReconcilableRecord for TXTRecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        zone_manager
            .add_txt_record(
                zone_name, &spec.name, &spec.text, spec.ttl, server, key_data,
            )
            .await
    }
}

impl ReconcilableRecord for CNAMERecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        zone_manager
            .add_cname_record(
                zone_name,
                &spec.name,
                &spec.target,
                spec.ttl,
                server,
                key_data,
            )
            .await
    }
}

impl ReconcilableRecord for MXRecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        zone_manager
            .add_mx_record(
                zone_name,
                &spec.name,
                spec.priority,
                &spec.mail_server,
                spec.ttl,
                server,
                key_data,
            )
            .await
    }
}

impl ReconcilableRecord for NSRecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        zone_manager
            .add_ns_record(
                zone_name,
                &spec.name,
                &spec.nameserver,
                spec.ttl,
                server,
                key_data,
            )
            .await
    }
}

impl ReconcilableRecord for SRVRecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        let srv_data = SRVRecordData {
            priority: spec.priority,
            weight: spec.weight,
            port: spec.port,
            target: spec.target.clone(),
            ttl: spec.ttl,
        };
        zone_manager
            .add_srv_record(zone_name, &spec.name, &srv_data, server, key_data)
            .await
    }
}

impl ReconcilableRecord for CAARecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        zone_manager
            .add_caa_record(
                zone_name,
                &spec.name,
                spec.flags,
                &spec.tag,
                &spec.value,
                spec.ttl,
                server,
                key_data,
            )
            .await
    }
}

impl ReconcilableRecord for PTRRecord {
    async fn add_to_bind9(
        &self,
        zone_manager: &Bind9Manager,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let spec = &self.spec;
        let ptr_data = PTRRecordData {
            target: spec.target.clone(),
            ttl: spec.ttl,
        };
        zone_manager
            .add_ptr_record(zone_name, &spec.name, &ptr_data, server, key_data)
            .await
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
        .with_context(|| format!("Failed to get {} {namespace}/{name}", record_type::<T>()))?
    else {
        debug!(
            "Not replaying {} {namespace}/{name}: it no longer exists",
            record_type::<T>()
        );
        return Ok(ReplayAction::Skipped);
    };

    if !should_replay(record.meta()) {
        debug!(
            "Not replaying {} {namespace}/{name}: it is being deleted",
            record_type::<T>()
        );
        return Ok(ReplayAction::Skipped);
    }

    // ADR-0027 Decision 4: a record held back by another cluster's holder is
    // not replayed, or the last record in the zone's list would be the one
    // served.
    let live = stores.records.get::<T>().state();
    if let Some(holder) =
        bindy_controller_sdk::rrset_conflict::held_back_by(client, &record, &live).await?
    {
        debug!(
            "Not replaying {} {namespace}/{name}: held back by {}/{} (ADR-0027)",
            record_type::<T>(),
            holder.meta().namespace.as_deref().unwrap_or_default(),
            holder.meta().name.as_deref().unwrap_or_default()
        );
        return Ok(ReplayAction::Skipped);
    }

    add_record_to_instances_generic(client, stores, resolver, primary_refs, zone_name, &record)
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

    // One arm per row of the record-kind table (ADR-0020).
    Some(bindy_api::match_record_kind!(kind, K => {
        replay_single_record::<K>(client, stores, resolver, zone_name, namespace, name, primary_refs)
            .await
    }))
}

#[cfg(test)]
#[path = "record_push_tests.rs"]
mod record_push_tests;
