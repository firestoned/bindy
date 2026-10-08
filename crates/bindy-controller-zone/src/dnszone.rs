// Copyright (c) 2025 Erick Bourgeois, firestoned
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::doc_markdown)]
// SPDX-License-Identifier: Apache-2.0

//! DNS zone reconciliation logic.
//!
//! This module handles the creation and management of DNS zones on BIND9 servers.
//! It supports both primary and secondary zone configurations.

// Module imports
pub mod bind9_config;
pub mod cleanup;
pub mod discovery;
pub mod helpers;
pub use bindy_bind9::primary;
pub mod secondary;
pub mod status_helpers;
pub mod transfer_peers;
pub mod types;
pub mod validation;

#[cfg(test)]
#[path = "dnszone/helpers_tests.rs"]
mod helpers_tests;

// Bind9Instance and InstanceReferenceWithStatus are used by dead_code marked functions (Phase 2 cleanup)
use self::types::DuplicateZoneInfo;
#[allow(unused_imports)]
use crate::bind9::zone_ops::{dns_query_endpoint, extract_ds_records, DsRecordInfo};
use crate::context::StoresBind9Ext;
use crate::crd::DNSZone;
use anyhow::{anyhow, Result};
use bindcar::{ZONE_TYPE_PRIMARY, ZONE_TYPE_SECONDARY};
use futures::stream::{self, StreamExt};
use k8s_openapi::api::core::v1::{Pod, Service};
use kube::{api::ListParams, client::Client, Api, ResourceExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// Creates a map of nameserver hostnames to IP addresses by:
/// 1. Checking for Service external IPs first (`LoadBalancer` or `NodePort`)
/// 2. Falling back to pod IPs if no external IPs are available
///
/// Nameservers are named: `ns1.{zone_name}.`, `ns2.{zone_name}.`, etc.
/// Order: Primary instances first, then secondary instances.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `instance_refs` - All instance references (primaries and secondaries)
///
/// # Returns
///
/// `HashMap` of nameserver hostnames to IP addresses, or None if no IPs found
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail.
pub async fn generate_nameserver_ips(
    client: &Client,
    zone_name: &str,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Option<HashMap<String, String>>> {
    if instance_refs.is_empty() {
        return Ok(None);
    }

    let mut nameserver_ips = HashMap::new();
    let mut ns_index = 1;

    // Process primaries first, then secondaries
    for instance_ref in instance_refs {
        // Try to get Service external IP first
        let service_api: Api<Service> = Api::namespaced(client.clone(), &instance_ref.namespace);

        let ip = match service_api.get(&instance_ref.name).await {
            Ok(service) => {
                // Check for LoadBalancer external IP
                if let Some(status) = &service.status {
                    if let Some(load_balancer) = &status.load_balancer {
                        if let Some(ingress_list) = &load_balancer.ingress {
                            if let Some(ingress) = ingress_list.first() {
                                if let Some(lb_ip) = &ingress.ip {
                                    debug!(
                                        "Using LoadBalancer IP {} for instance {}/{}",
                                        lb_ip, instance_ref.namespace, instance_ref.name
                                    );
                                    Some(lb_ip.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Err(e) => {
                debug!(
                    "Failed to get service for instance {}/{}: {}. Will try pod IP.",
                    instance_ref.namespace, instance_ref.name, e
                );
                None
            }
        };

        // If no service external IP, fallback to pod IP
        let ip = if ip.is_none() {
            // Get pod IP
            let pod_api: Api<Pod> = Api::namespaced(client.clone(), &instance_ref.namespace);
            let label_selector = format!("app=bind9,instance={}", instance_ref.name);
            let lp = ListParams::default().labels(&label_selector);

            match bindy_controller_sdk::pagination::list_all_paginated(&pod_api, lp).await {
                Ok(pods) => {
                    // Find first running pod
                    pods.iter()
                        .find(|pod| {
                            let phase = pod
                                .status
                                .as_ref()
                                .and_then(|s| s.phase.as_ref())
                                .map_or("Unknown", std::string::String::as_str);
                            phase == "Running"
                        })
                        .and_then(|pod| {
                            pod.status
                                .as_ref()
                                .and_then(|s| s.pod_ip.as_ref())
                                .map(|ip| {
                                    debug!(
                                        "Using pod IP {} for instance {}/{}",
                                        ip, instance_ref.namespace, instance_ref.name
                                    );
                                    ip.clone()
                                })
                        })
                }
                Err(e) => {
                    warn!(
                        "Failed to list pods for instance {}/{}: {}. Skipping.",
                        instance_ref.namespace, instance_ref.name, e
                    );
                    None
                }
            }
        } else {
            ip
        };

        // Add to nameserver map if we found an IP
        if let Some(ip) = ip {
            let ns_hostname = format!("ns{ns_index}.{zone_name}.");
            nameserver_ips.insert(ns_hostname, ip);
            ns_index += 1;
        }
    }

    if nameserver_ips.is_empty() {
        Ok(None)
    } else {
        Ok(Some(nameserver_ips))
    }
}

/// Get the effective nameservers list for a DNSZone, handling both new and deprecated fields.
///
/// This function provides backward compatibility by:
/// 1. Preferring the new `name_servers` field if present
/// 2. Falling back to the deprecated `name_server_ips` field with automatic migration
/// 3. Logging deprecation warnings when the old field is used
///
/// # Arguments
/// * `spec` - The DNSZone spec containing nameserver configuration
///
/// # Returns
/// `Option<Vec<NameServer>>` - The effective list of nameservers, or `None` if neither field is set
///
/// # Examples
///
/// ```text
/// # #[allow(deprecated)]
/// # use bindy::crd::{DNSZoneSpec, NameServer, SOARecord};
/// # use std::collections::HashMap;
/// // New field takes precedence
/// let spec = DNSZoneSpec {
///     zone_name: "example.com".into(),
///     soa_record: SOARecord {
///         primary_ns: "ns1.example.com.".into(),
///         admin_email: "admin.example.com.".into(),
///         serial: 1,
///         refresh: 3600,
///         retry: 600,
///         expire: 604800,
///         negative_ttl: 86400,
///     },
///     ttl: None,
///     cluster_ref: None,
///     name_servers: Some(vec![NameServer {
///         hostname: "ns2.example.com.".into(),
///         ipv4_address: None,
///         ipv6_address: None,
///     }]),
///     name_server_ips: Some(HashMap::from([("ns3.example.com.".into(), "192.0.2.3".into())])),
///     records_from: None,
///     bind9_instances_from: None,
///     dnssec_policy: None,
/// };
/// // Returns name_servers (new field), ignoring name_server_ips
/// ```
fn get_effective_name_servers(
    spec: &crate::crd::DNSZoneSpec,
) -> Option<Vec<crate::crd::NameServer>> {
    use crate::crd::NameServer;

    // New field takes precedence
    if let Some(ref new_servers) = spec.name_servers {
        debug!(
            "Using new `nameServers` field with {} server(s)",
            new_servers.len()
        );
        return Some(new_servers.clone());
    }

    // Fallback to deprecated field with migration warning
    #[allow(deprecated)]
    if let Some(ref old_ips) = spec.name_server_ips {
        warn!(
            "DNSZone uses deprecated `nameServerIps` field. \
             Migrate to `nameServers` for better functionality and IPv6 support. \
             See migration guide at docs/src/operations/migration-guide.md"
        );

        // Convert HashMap<String, String> to Vec<NameServer>
        // Old format: {"ns2.example.com.": "192.0.2.2"}
        // New format: vec![NameServer { hostname: "ns2.example.com.", ipv4_address: Some("192.0.2.2"), .. }]
        let servers: Vec<NameServer> = old_ips
            .iter()
            .map(|(hostname, ip)| NameServer {
                hostname: hostname.clone(),
                ipv4_address: Some(ip.clone()),
                ipv6_address: None, // Old field doesn't support IPv6
            })
            .collect();

        debug!(
            "Migrated {} server(s) from deprecated `nameServerIps` to new format",
            servers.len()
        );

        return Some(servers);
    }

    // Neither field set
    None
}

/// Re-fetch a DNSZone to get the latest status.
///
/// The `dnszone` parameter from the watch event might have stale status from the cache.
/// We need the latest `status.bind9Instances` which may have been updated by the
/// Bind9Instance reconciler.
///
/// # Arguments
/// * `client` - Kubernetes client
/// * `namespace` - Namespace of the DNSZone
/// * `name` - Name of the DNSZone
///
/// # Returns
/// The freshly fetched DNSZone with current status
///
/// # Errors
/// Returns an error if the Kubernetes API call fails
async fn refetch_zone(client: &kube::Client, namespace: &str, name: &str) -> Result<DNSZone> {
    let zones_api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);
    let zone = zones_api.get(name).await?;
    Ok(zone)
}

/// Handle duplicate zone conflicts by setting Ready=False and stopping reconciliation.
///
/// When a duplicate zone is detected, this function:
/// 1. Logs a warning with details about the conflict
/// 2. Updates the status with Ready=False and DuplicateZone condition
/// 3. Applies the status to the API server
///
/// # Arguments
/// * `client` - Kubernetes client
/// * `namespace` - Namespace of the conflicting DNSZone
/// * `name` - Name of the conflicting DNSZone
/// * `duplicate_info` - Information about the duplicate zone conflict
/// * `status_updater` - Status updater to apply the condition
///
/// # Errors
/// Returns an error if the status update fails
async fn handle_duplicate_zone(
    client: &kube::Client,
    namespace: &str,
    name: &str,
    duplicate_info: &DuplicateZoneInfo,
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
) -> Result<()> {
    warn!(
        "Duplicate zone detected: {}/{} cannot claim '{}' because it is already configured by: {:?}",
        namespace, name, duplicate_info.zone_name, duplicate_info.conflicting_zones
    );

    // Build list of conflicting zones in namespace/name format
    let conflicting_zone_refs: Vec<String> = duplicate_info
        .conflicting_zones
        .iter()
        .map(|z| format!("{}/{}", z.namespace, z.name))
        .collect();

    // Set Ready=False with DuplicateZone reason
    status_updater.set_duplicate_zone_condition(&duplicate_info.zone_name, &conflicting_zone_refs);

    // Apply status and stop processing
    status_updater.apply(client).await?;

    Ok(())
}

/// Detect if the zone spec has changed since last reconciliation.
///
/// Compares current generation with observed generation to determine
/// if this is first reconciliation or if spec changed.
///
/// # Arguments
///
/// * `zone` - The DNSZone resource
///
/// # Returns
///
/// Tuple of (first_reconciliation, spec_changed)
fn detect_spec_changes(zone: &DNSZone) -> (bool, bool) {
    let current_generation = zone.metadata.generation;
    let observed_generation = zone.status.as_ref().and_then(|s| s.observed_generation);

    let first_reconciliation = observed_generation.is_none();
    let spec_changed =
        bindy_controller_sdk::status::should_reconcile(current_generation, observed_generation);

    (first_reconciliation, spec_changed)
}

/// Detect if the instance list changed between watch event and re-fetch.
///
/// This is critical for detecting when:
/// 1. New instances are added to `status.bind9Instances` (via `bind9InstancesFrom` selectors)
/// 2. Instance `lastReconciledAt` timestamps are cleared (e.g., instance deleted, needs reconfiguration)
///
/// NOTE: `InstanceReference` `PartialEq` ignores `lastReconciledAt`, so we must check timestamps separately!
///
/// # Arguments
///
/// * `namespace` - Namespace for logging
/// * `name` - Zone name for logging
/// * `watch_instances` - Instances from the watch event that triggered reconciliation
/// * `current_instances` - Instances after re-fetching (current state)
///
/// # Returns
///
/// `true` if instances changed (list or timestamps), `false` otherwise
fn detect_instance_changes(
    namespace: &str,
    name: &str,
    watch_instances: Option<&Vec<crate::crd::InstanceReference>>,
    current_instances: &[crate::crd::InstanceReference],
) -> bool {
    let Some(watch_instances) = watch_instances else {
        // No instances in watch event, first reconciliation or error
        return true;
    };

    // Get the instance names from the watch event (what triggered us)
    let watch_instance_names: std::collections::HashSet<_> =
        watch_instances.iter().map(|r| &r.name).collect();

    // Get the instance names after re-fetching (current state)
    let current_instance_names: std::collections::HashSet<_> =
        current_instances.iter().map(|r| &r.name).collect();

    // Check if instance list changed (added/removed instances)
    let list_changed = watch_instance_names != current_instance_names;

    if list_changed {
        info!(
            "Instance list changed during reconciliation for zone {}/{}: watch_event={:?}, current={:?}",
            namespace, name, watch_instance_names, current_instance_names
        );
        return true;
    }

    // List is the same, but check if any lastReconciledAt timestamps changed
    // Use InstanceReference as HashMap key (uses its Hash impl which hashes identity fields)
    let watch_timestamps: std::collections::HashMap<&crate::crd::InstanceReference, Option<&str>> =
        watch_instances
            .iter()
            .map(|inst| (inst, inst.last_reconciled_at.as_deref()))
            .collect();

    let current_timestamps: std::collections::HashMap<
        &crate::crd::InstanceReference,
        Option<&str>,
    > = current_instances
        .iter()
        .map(|inst| (inst, inst.last_reconciled_at.as_deref()))
        .collect();

    let timestamps_changed = watch_timestamps.iter().any(|(inst_ref, watch_ts)| {
        current_timestamps
            .get(inst_ref)
            .is_some_and(|current_ts| current_ts != watch_ts)
    });

    if timestamps_changed {
        info!(
            "Instance lastReconciledAt timestamps changed for zone {}/{}",
            namespace, name
        );
    }

    timestamps_changed
}

/// Reconciles a `DNSZone` resource.
///
/// Creates or updates DNS zone files on BIND9 instances that match the zone's
/// instance selector. Supports both primary and secondary zone types.
///
/// # Zone Types
///
/// - **Primary**: Authoritative zone with SOA record and local zone file
/// - **Secondary**: Replica zone that transfers from primary servers
///
/// # Arguments
///
/// * `client` - Kubernetes API client for finding matching `Bind9Instances`
/// * `dnszone` - The `DNSZone` resource to reconcile
/// * `zone_manager` - BIND9 manager for creating zone files
///
/// # Returns
///
/// * `Ok(outcome)` - How the reconcile ended ([`types::ZoneOutcome`]):
///   converged, waiting on another object, or to be retried with backoff.
///   Decided from the status this reconcile built, so the controller does
///   not re-read the zone to pick its `Action` (ADR-0016).
/// * `Err(_)` - If zone creation failed or a Kubernetes API call failed
///
/// # Example
///
/// ```rust,no_run,ignore
/// use bindy::reconcilers::reconcile_dnszone;
/// use bindy::crd::DNSZone;
/// use bindy::context::Context;
/// use std::sync::Arc;
///
/// async fn handle_zone(ctx: Arc<Context>, zone: DNSZone) -> anyhow::Result<()> {
///     let outcome = reconcile_dnszone(ctx, zone).await?;
///     println!("{outcome:?}");
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 zone operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_dnszone(
    ctx: Arc<crate::context::Context>,
    dnszone: DNSZone,
) -> Result<types::ZoneOutcome> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;

    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();

    debug!("Reconciling DNSZone: {}/{}", namespace, name);
    debug!(
        namespace = %namespace,
        name = %name,
        generation = ?dnszone.metadata.generation,
        "Starting DNSZone reconciliation"
    );

    // Save the instance list from the watch event (before re-fetching)
    // This represents the instances that triggered this reconciliation
    let watch_event_instances =
        validation::get_instances_from_zone(&dnszone, bind9_instances_store).ok();

    // CRITICAL: Re-fetch the zone to get the latest status
    let dnszone = refetch_zone(&client, &namespace, &name).await?;

    // Create centralized status updater to batch all status changes
    let mut status_updater = bindy_controller_sdk::status::DNSZoneStatusUpdater::new(&dnszone);

    // Extract spec
    let spec = &dnszone.spec;

    // Validate that zone has instances assigned (via its bind9InstancesFrom
    // selectors). With none, the zone waits: a Bind9Instance whose labels
    // match wakes it through the instance mapper (`zones_selecting_instance`),
    // and a spec change through the primary stream (ADR-0016).
    let instance_refs = match validation::get_instances_from_zone(&dnszone, bind9_instances_store) {
        Ok(refs) => refs,
        Err(e) => {
            warn!(
                "DNSZone {}/{} is waiting for instances: {}",
                namespace, name, e
            );
            return Ok(types::ZoneOutcome::Waiting {
                reason: types::REASON_NO_INSTANCES,
            });
        }
    };

    debug!(
        "DNSZone {}/{} is assigned to {} instance(s): {:?}",
        namespace,
        name,
        instance_refs.len(),
        instance_refs.iter().map(|r| &r.name).collect::<Vec<_>>()
    );

    // CRITICAL: Check for duplicate zones BEFORE any configuration
    // If another zone already claims this zone name, set Ready=False with DuplicateZone reason
    // and stop processing to prevent conflicting DNS configurations
    let zones_store = &ctx.stores.dnszones;
    if let Some(duplicate_info) = validation::check_for_duplicate_zones(&dnszone, zones_store) {
        handle_duplicate_zone(
            &client,
            &namespace,
            &name,
            &duplicate_info,
            &mut status_updater,
        )
        .await?;
        // The winning zone's change or deletion wakes this one through the
        // DNSZone mapper (`zones_contending_for_name`), not a timer (ADR-0016).
        return Ok(types::ZoneOutcome::Waiting {
            reason: types::REASON_DUPLICATE_ZONE,
        });
    }

    // Determine if this is the first reconciliation or if spec has changed
    let (first_reconciliation, spec_changed) = detect_spec_changes(&dnszone);

    // Check if the instance list or lastReconciledAt timestamps changed between watch event and re-fetch
    let instances_changed = detect_instance_changes(
        &namespace,
        &name,
        watch_event_instances.as_ref(),
        &instance_refs,
    );

    // Check if any instances need reconciliation (never reconciled or reconciliation failed)
    let unreconciled_instances =
        validation::filter_instances_needing_reconciliation(&instance_refs);
    let has_unreconciled_instances = !unreconciled_instances.is_empty();

    if has_unreconciled_instances {
        info!(
            "Found {} unreconciled instance(s) for zone {}/{}: {:?}",
            unreconciled_instances.len(),
            namespace,
            name,
            unreconciled_instances
                .iter()
                .map(|i| format!("{}/{}", i.namespace, i.name))
                .collect::<Vec<_>>()
        );
    } else {
        debug!(
            "No unreconciled instances for zone {}/{} - all {} instance(s) already configured (lastReconciledAt set)",
            namespace,
            name,
            instance_refs.len()
        );
    }

    // Whether a cleanup pass below left work to retry. The passes only run
    // inside a reconcile, so with no periodic resync the zone must ask for the
    // retry itself (ADR-0016).
    let mut cleanup_incomplete = false;

    // CRITICAL: Cleanup deleted instances BEFORE early return check
    // If we skip reconciliation due to no changes, we still need to remove deleted instances from status
    match cleanup::cleanup_deleted_instances(&client, &dnszone, &mut status_updater).await {
        Ok(deleted_count) if deleted_count > 0 => {
            info!(
                "Cleaned up {} deleted instance(s) from zone {}/{} status",
                deleted_count, namespace, name
            );
        }
        Ok(_) => {
            debug!(
                "No deleted instances found for zone {}/{} status",
                namespace, name
            );
        }
        Err(e) => {
            warn!(
                "Failed to cleanup deleted instances for zone {}/{}: {} (continuing with reconciliation)",
                namespace, name, e
            );
            // Don't fail reconciliation for cleanup errors; retry it
            cleanup_incomplete = true;
        }
    }

    // CRITICAL: We CANNOT skip reconciliation entirely, even if spec and instances haven't changed.
    // Reconciliation may be triggered by ARecord/AAAA/TXT/etc changes via watches, and we MUST
    // run record discovery to tag newly created records with status.zoneRef.
    //
    // However, we CAN skip BIND9 configuration if nothing changed (handled later in the flow).
    // This ensures record discovery ALWAYS runs while still optimizing BIND9 API calls.

    if instances_changed {
        info!(
            "Instances changed for zone {}/{} - reconciling to configure new instances",
            namespace, name
        );
    }

    debug!(
        "Reconciling zone {} (first_reconciliation={}, spec_changed={})",
        spec.zone_name, first_reconciliation, spec_changed
    );

    // Cleanup stale records from status.records[] before main reconciliation
    // This ensures status stays in sync with actual Kubernetes resources.
    // Deleted records whose DNS data could not be confirmed gone are retained
    // and handed to discovery, so they stay tracked until the cleanup succeeds.
    let retained_records = match cleanup::cleanup_stale_records(
        &client,
        &dnszone,
        &mut status_updater,
        &ctx.stores,
    )
    .await
    {
        Ok(outcome) => {
            if outcome.removed > 0 {
                info!(
                    "Cleaned up {} stale record(s) from zone {}/{} status",
                    outcome.removed, namespace, name
                );
            }
            if !outcome.retained.is_empty() {
                cleanup_incomplete = true;
                warn!(
                    "{} deleted record(s) of zone {}/{} may still be served; their DNS cleanup is retried",
                    outcome.retained.len(),
                    namespace,
                    name
                );
            }
            outcome.retained
        }
        Err(e) => {
            warn!(
                "Failed to cleanup stale records for zone {}/{}: {} (continuing with reconciliation)",
                namespace, name, e
            );
            // Don't fail reconciliation for cleanup errors; retry it
            cleanup_incomplete = true;
            Vec::new()
        }
    };

    // BIND9 configuration: Always ensure zones exist on all instances
    // This implements true declarative reconciliation - if a pod restarts without
    // persistent storage, the reconciler will detect the missing zone and recreate it.
    // The add_zones() function is idempotent, so this is safe to call every reconciliation.
    //
    // NOTE: We ALWAYS configure zones, not just when spec changes. This ensures:
    // - Zones are recreated if pods restart without persistent volumes
    // - New instances added to the cluster get zones automatically
    // - Drift detection: if someone manually deletes a zone, it's recreated
    // - True Kubernetes declarative reconciliation: actual state continuously matches desired state
    let (primary_outcome, secondary_outcome) = bind9_config::configure_zone_on_instances(
        ctx.clone(),
        &dnszone,
        &mut status_updater,
        &instance_refs,
        &unreconciled_instances,
    )
    .await?;

    // Discover DNS records and update status
    let (record_refs, records_count, unselected_cleanup_pending) =
        discovery::discover_and_update_records(
            &client,
            &dnszone,
            &mut status_updater,
            &ctx.stores,
            &retained_records,
        )
        .await?;
    cleanup_incomplete |= unselected_cleanup_pending;

    // Truthful status: a record that should no longer be served may still
    // be, until its deletion is confirmed on every pod holding the zone.
    let pending_deletions: Vec<String> = retained_records
        .iter()
        .map(|r| format!("{}/{}", r.kind, r.name))
        .collect();
    status_helpers::mark_record_deletions_pending(
        &mut status_updater,
        &spec.zone_name,
        &pending_deletions,
        unselected_cleanup_pending,
    );

    // Replay the zone's records whenever the zone had to be (re)created on any
    // server, or a previous replay did not finish. Without this a wiped pod
    // comes back authoritative for a zone containing only SOA and NS - see
    // `records::replay_zone_records` for the full rationale.
    replay_records_if_zone_was_recreated(
        &ctx,
        &dnszone,
        &mut status_updater,
        &instance_refs,
        &record_refs,
        primary_outcome.zones_created + secondary_outcome.zones_created,
    )
    .await?;

    // Record readiness is not polled here: it used to cost one GET per record
    // per zone reconcile only to log whether every record was Ready. Each
    // record reports its own Ready condition, and BIND9 notifies secondaries
    // itself when a primary's serial advances (ADR-0015).

    // Calculate expected counts and finalize status
    let (expected_primary_count, expected_secondary_count) =
        status_helpers::calculate_expected_instance_counts(
            &client,
            bind9_instances_store,
            &instance_refs,
        )
        .await?;

    status_helpers::finalize_zone_status(
        &mut status_updater,
        &client,
        &spec.zone_name,
        &namespace,
        &name,
        primary_outcome,
        secondary_outcome,
        expected_primary_count,
        expected_secondary_count,
        records_count,
        dnszone.metadata.generation,
    )
    .await?;

    Ok(status_helpers::zone_outcome(
        status_updater.has_degraded_condition(),
        cleanup_incomplete,
        status_updater.dnssec(),
        k8s_openapi::jiff::Timestamp::now(),
    ))
}

/// Replay all of a zone's records into BIND9 when the zone was (re)created.
///
/// # Why
///
/// BIND9 operand pods keep zone data in ephemeral storage. Any event that
/// replaces a pod - an operator upgrade, a `placement` change that rolls the
/// Deployment, an eviction, a node reboot, a manual `kubectl delete pod` -
/// brings the pod back with no zones. The zone reconciler then recreates the
/// zone from `spec`, which yields SOA and NS records ONLY. The pod is now
/// *authoritative* for a zone with no data: it answers authoritative NXDOMAIN,
/// or, when `global.recursion` and `global.forwarders` are set, silently
/// forwards the query and returns the PUBLIC answer for an internal name.
///
/// Nothing about the record CRs changed, so their own controllers have no
/// reason to act. This function is the missing link: the moment the zone
/// reconciler observes that it created a zone, it pushes every record CR the
/// zone selects back into BIND9.
///
/// The intent is recorded in `status.recordsResyncPending` before the replay is
/// attempted, so an operator crash mid-replay, or a partial failure, is retried
/// on the next reconciliation instead of being forgotten. While the flag is set
/// the zone reports `Ready=False` / `Degraded=True`, so a server authoritative
/// for an empty zone is never advertised as healthy.
///
/// # Arguments
///
/// * `ctx` - Application context (Kubernetes client and reflector stores)
/// * `dnszone` - The zone being reconciled
/// * `status_updater` - Status updater collecting in-memory condition changes
/// * `instance_refs` - All instances assigned to the zone
/// * `record_refs` - The records just discovered for the zone
/// * `zones_created` - Number of endpoints where the zone was newly created
///
/// # Errors
///
/// Returns an error if the PRIMARY instances cannot be determined. Individual
/// record push failures are reported through the `Degraded` condition and
/// retried on the next reconciliation rather than aborting the zone reconcile.
async fn replay_records_if_zone_was_recreated(
    ctx: &Arc<crate::context::Context>,
    dnszone: &DNSZone,
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    instance_refs: &[crate::crd::InstanceReference],
    record_refs: &[crate::crd::RecordReferenceWithTimestamp],
    zones_created: usize,
) -> Result<()> {
    let client = ctx.client.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let zone_name = &dnszone.spec.zone_name;

    // A replay left over from a previous reconciliation is just as binding as
    // one triggered right now.
    let resync_outstanding = dnszone
        .status
        .as_ref()
        .is_some_and(|status| status.records_resync_pending);

    if zones_created == 0 && !resync_outstanding {
        return Ok(());
    }

    if zones_created > 0 {
        warn!(
            "Zone {} was created on {} endpoint(s) during this reconciliation of DNSZone {}/{} - \
             those servers hold SOA and NS records only. Replaying {} record(s).",
            zone_name,
            zones_created,
            namespace,
            name,
            record_refs.len()
        );
    } else {
        info!(
            "DNSZone {}/{} still has an outstanding record resync - retrying {} record(s) for zone {}",
            namespace,
            name,
            record_refs.len(),
            zone_name
        );
    }

    // A zone with no records is fully described by its SOA and NS records, so
    // recreating it already restored the declared state - nothing to replay and
    // nothing to keep the zone out of Ready.
    if record_refs.is_empty() {
        debug!(
            "Zone {} selects no records - nothing to replay for DNSZone {}/{}",
            zone_name, namespace, name
        );
        status_updater.set_records_resync_pending(false);
        return Ok(());
    }

    // Persist the intent BEFORE touching BIND9: if the operator dies mid-replay
    // the flag survives and the next reconciliation retries.
    status_updater.set_records_resync_pending(true);
    status_updater.apply(&client).await?;

    // Records are written to PRIMARY servers only; secondaries pull the zone
    // via AXFR once the primary's serial advances.
    let primary_refs = bindy_bind9::primary::filter_primary_instances_cached(
        &client,
        &ctx.stores.bind9_instances,
        instance_refs,
    )
    .await?;

    if primary_refs.is_empty() {
        let message = format!(
            "Zone {zone_name} must replay {} record(s) but has no primary instances to write them to",
            record_refs.len()
        );
        warn!("DNSZone {}/{}: {}", namespace, name, message);
        status_updater.set_condition("Degraded", "True", "RecordsResyncPending", &message);
        return Ok(());
    }

    let outcome = bindy_bind9::record_push::replay_zone_records(
        &client,
        &ctx.stores,
        zone_name,
        record_refs,
        &primary_refs,
    )
    .await;

    if outcome.is_complete() {
        info!(
            "Record resync complete for DNSZone {}/{}: {}",
            namespace,
            name,
            outcome.summary(zone_name)
        );
        status_updater.set_records_resync_pending(false);
        return Ok(());
    }

    let message = outcome.summary(zone_name);
    warn!(
        "Record resync incomplete for DNSZone {}/{}: {} - zone stays Degraded and will be retried",
        namespace, name, message
    );
    status_updater.set_condition("Degraded", "True", "RecordsResyncPending", &message);

    Ok(())
}

/// Adds a DNS zone to all primary instances.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `dnszone` - The `DNSZone` resource
/// * `zone_manager` - BIND9 manager for adding zone
///
/// # Returns
///
/// * `Ok(ZoneConfigOutcome)` - Per-instance and per-endpoint configuration counts.
///   An instance counts as configured only if ALL of its ready endpoints accepted
///   the zone, so the instance count is directly comparable with the expected
///   PRIMARY instance count when computing readiness.
/// * `Err(_)` - If zone addition failed on every endpoint
///
/// # Errors
///
/// Returns an error if BIND9 zone addition fails or if no instances are assigned.
///
/// # Panics
///
/// Panics if the RNDC key is not loaded by the helper function (should never happen in practice).
#[allow(clippy::too_many_lines)]
/// Builds the `Bind9Manager` to use for one instance's endpoints.
///
/// Zone operations must not reuse the process-wide manager built at startup.
/// That one is constructed before any `Bind9Instance` exists, so it has neither
/// the instance's TLS configuration nor the Kubernetes client needed to read the
/// configured CA bundle; it can only ever speak plaintext. Records already
/// resolve a manager per instance (`reconcilers::records`); zones did not, so a
/// TLS-enabled sidecar was dialled over `http://` and every zone operation
/// failed, with the ServiceAccount token attached to the plaintext request.
///
/// # Arguments
///
/// * `ctx` - Controller context, for the instance stores and the client
/// * `instance_ref` - The instance whose endpoints are about to be addressed
///
/// # Returns
///
/// A manager carrying that instance's TLS configuration, if it has any.
/// The endpoint a zone NOTIFY is sent to, plus the instance that serves it.
///
/// NOTIFY used to record only `<pod-ip>:<port>`, which threw away the one thing
/// needed to dial it correctly: which `Bind9Instance` owns the endpoint, and so
/// whether that instance's sidecar speaks TLS. Without it the call fell back to
/// the shared startup manager, which carries no TLS configuration, and went out
/// over plaintext `http://` against a TLS-only sidecar: refused, then retried
/// until the reconcile ran out of time. Keeping the two together is what lets
/// the notify site resolve a manager through [`zone_manager_for_instance`] like
/// every other bindcar call in this reconciler.
/// The `dnssecPolicy` value that explicitly disables signing for a zone.
const DNSSEC_POLICY_NONE: &str = "none";

/// Decide what `status.dnssec` should say for a zone (ADR-0006).
///
/// - DS records present → `signed: true` with every KSK's DS record; `keyTag`
///   and `algorithm` describe the first KSK.
/// - Policy explicitly `"none"` → no status at all (signing disabled), even
///   if stale DNSKEYs are still being served.
/// - A per-zone policy but no DNSKEYs yet → `signed: false` (keys are still
///   generating; the zone retries with backoff until they appear, ADR-0016).
/// - No policy and no DNSKEYs → no status.
///
/// # Arguments
/// * `dnssec_policy` - The zone's `spec.dnssecPolicy`, if set
/// * `ds_records` - DS records derived from the zone's DNSKEY RRset
/// * `next_key_rollover` - Next scheduled KSK rollover from the sidecar's
///   zone status (bindcar 0.8.1+), if known
fn build_dnssec_status(
    dnssec_policy: Option<&str>,
    ds_records: &[DsRecordInfo],
    next_key_rollover: Option<String>,
) -> Option<crate::crd::DNSSECStatus> {
    if dnssec_policy == Some(DNSSEC_POLICY_NONE) {
        return None;
    }

    if let Some(first) = ds_records.first() {
        return Some(crate::crd::DNSSECStatus {
            signed: true,
            ds_records: ds_records
                .iter()
                .map(|ds| ds.presentation.clone())
                .collect(),
            key_tag: Some(u32::from(first.key_tag)),
            algorithm: Some(first.algorithm.clone()),
            next_key_rollover,
            // No source: bindcar 0.8.x exposes the next scheduled event and
            // current key states, not rollover history.
            last_key_rollover: None,
        });
    }

    // No DNSKEYs. Only promise "signing pending" when this zone explicitly
    // requests a policy; a zone signed solely via the cluster-global policy
    // simply has no DNSSEC status until its keys appear.
    dnssec_policy.map(|_| crate::crd::DNSSECStatus {
        signed: false,
        ds_records: Vec::new(),
        key_tag: None,
        algorithm: None,
        next_key_rollover: None,
        last_key_rollover: None,
    })
}

/// Query one configured endpoint for DNSKEYs and record the zone's DNSSEC
/// status on the updater (ADR-0006).
///
/// A DNS query failure only logs a warning and keeps the previous status:
/// DS reporting must never fail a reconcile of an otherwise healthy zone.
///
/// # Arguments
/// * `status_updater` - Collects the in-memory status change
/// * `zone_name` - The zone that was just configured
/// * `dnssec_policy` - The zone's `spec.dnssecPolicy`, if set
/// * `endpoint` - The first configured primary endpoint, if any
/// * `ctx` - Operator context, for the instance's TLS-aware bindcar client
async fn update_dnssec_status(
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    zone_name: &str,
    dnssec_policy: Option<&str>,
    endpoint: Option<&NotifyTarget>,
    ctx: &crate::context::Context,
) {
    // Explicitly disabled: clear any stale status without querying.
    if dnssec_policy == Some(DNSSEC_POLICY_NONE) {
        status_updater.set_dnssec(None);
        return;
    }

    let Some(target) = endpoint else {
        return;
    };

    let dns_endpoint = dns_query_endpoint(&target.endpoint);
    match extract_ds_records(zone_name, &dns_endpoint).await {
        Ok(ds_records) => {
            let next_rollover = if ds_records.is_empty() {
                None
            } else {
                fetch_next_ksk_rollover(ctx, target, zone_name, status_updater).await
            };
            let status = build_dnssec_status(dnssec_policy, &ds_records, next_rollover);
            if let Some(ref dnssec) = status {
                if dnssec.signed {
                    info!(
                        "Zone {} is DNSSEC-signed; publishing {} DS record(s) to status",
                        zone_name,
                        dnssec.ds_records.len()
                    );
                } else {
                    debug!(
                        "Zone {} has DNSSEC policy {:?} but no DNSKEYs yet (keys generating)",
                        zone_name, dnssec_policy
                    );
                }
            }
            status_updater.set_dnssec(status);
        }
        Err(e) => {
            warn!(
                "Failed to extract DS records for zone {} from {}: {}. Keeping previous DNSSEC status.",
                zone_name, dns_endpoint, e
            );
        }
    }
}

/// Fetch the next scheduled KSK rollover for `zone_name` from the sidecar's
/// zone status (bindcar 0.8.1+, ADR-0006 as amended).
///
/// Best-effort: any failure (older sidecar, transient error, unparsable
/// body) logs at debug and returns the value the update already carries, so
/// a transient status failure never flaps the field.
///
/// # Arguments
/// * `ctx` - Operator context, for the instance's TLS-aware bindcar client
/// * `target` - The endpoint the zone was configured through
/// * `zone_name` - The zone to query
/// * `status_updater` - Source of the previously-known value
async fn fetch_next_ksk_rollover(
    ctx: &crate::context::Context,
    target: &NotifyTarget,
    zone_name: &str,
    status_updater: &bindy_controller_sdk::status::DNSZoneStatusUpdater,
) -> Option<String> {
    let previous = status_updater
        .dnssec()
        .and_then(|d| d.next_key_rollover.clone());

    let manager = zone_manager_for_instance(ctx, &target.instance_name, &target.instance_namespace);
    match manager.zone_status(zone_name, &target.endpoint).await {
        Ok(body) => crate::bind9::zone_ops::parse_zone_status_dnssec(&body)
            .as_ref()
            .and_then(crate::bind9::zone_ops::next_ksk_rollover)
            .or(previous),
        Err(e) => {
            debug!(
                "Could not fetch zone status for {} (keeping previous nextKeyRollover): {}",
                zone_name, e
            );
            previous
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NotifyTarget {
    /// Bare `<host>:<port>` of the endpoint to notify.
    pub endpoint: String,
    /// Name of the `Bind9Instance` serving that endpoint.
    pub instance_name: String,
    /// Namespace of the `Bind9Instance` serving that endpoint.
    pub instance_namespace: String,
}

/// Record `endpoint` as the NOTIFY target if none has been chosen yet.
///
/// Endpoints are configured concurrently, so this is called from every task;
/// only the first caller may win. A later endpoint must not displace it, or
/// NOTIFY would chase a different server on every reconcile.
///
/// # Arguments
/// * `slot` - The shared target, empty until the first endpoint is configured
/// * `endpoint` - Bare `<host>:<port>` of the endpoint just configured
/// * `instance_name` - Name of the `Bind9Instance` serving it
/// * `instance_namespace` - Namespace of that instance
pub(crate) fn remember_first_notify_target(
    slot: &mut Option<NotifyTarget>,
    endpoint: &str,
    instance_name: &str,
    instance_namespace: &str,
) {
    if slot.is_some() {
        return;
    }

    *slot = Some(NotifyTarget {
        endpoint: endpoint.to_string(),
        instance_name: instance_name.to_string(),
        instance_namespace: instance_namespace.to_string(),
    });
}

pub(crate) fn zone_manager_for_instance(
    ctx: &crate::context::Context,
    instance_name: &str,
    instance_namespace: &str,
) -> crate::bind9::Bind9Manager {
    ctx.stores.create_bind9_manager_for_instance_with_client(
        instance_name,
        instance_namespace,
        Some(ctx.client.clone()),
    )
}

/// The `dnssec-policy` to configure `spec`'s zone with, resolved from the
/// reflector caches (no API calls).
///
/// Uses the first primary instance: its own `spec.config` and its cluster's
/// (`Bind9Cluster` in its namespace, else the cluster-scoped
/// `ClusterBind9Provider`) `global` config, the same pair the instance's
/// `named.conf` policies are rendered from. See
/// [`crate::bind9_resources::resolve_zone_dnssec_policy`].
///
/// # Arguments
///
/// * `ctx` - Operator context holding the reflector stores
/// * `spec` - The zone's spec
/// * `primary_instance_refs` - The zone's primary instances
///
/// # Returns
///
/// The policy name, or `None` for an unsigned zone.
fn zone_dnssec_policy(
    ctx: &crate::context::Context,
    spec: &crate::crd::DNSZoneSpec,
    primary_instance_refs: &[crate::crd::InstanceReference],
) -> Option<String> {
    if spec.dnssec_policy.is_some() {
        return crate::bind9_resources::resolve_zone_dnssec_policy(
            spec.dnssec_policy.as_deref(),
            None,
            None,
        );
    }
    let primary = primary_instance_refs.first()?;
    let instance = ctx.stores.bind9_instances.state().into_iter().find(|i| {
        i.name_any() == primary.name && i.namespace().unwrap_or_default() == primary.namespace
    })?;
    let cluster_ref = &instance.spec.cluster_ref;
    let cluster_global = ctx
        .stores
        .bind9_clusters
        .state()
        .into_iter()
        .find(|c| {
            c.name_any() == *cluster_ref && c.namespace().unwrap_or_default() == primary.namespace
        })
        .and_then(|c| c.spec.common.global.clone());
    let global = cluster_global.or_else(|| {
        ctx.stores
            .cluster_bind9_providers
            .state()
            .into_iter()
            .find(|p| p.name_any() == *cluster_ref)
            .and_then(|p| p.spec.common.global.clone())
    });
    crate::bind9_resources::resolve_zone_dnssec_policy(
        None,
        global.as_ref(),
        instance.spec.config.as_ref(),
    )
}

pub async fn add_dnszone(
    ctx: Arc<crate::context::Context>,
    dnszone: DNSZone,
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    instance_refs: &[crate::crd::InstanceReference],
    peers: &crate::crd::ZoneTransferPeers,
) -> Result<types::ZoneConfigOutcome> {
    // One resolver for every BIND9 write this function makes: each primary's
    // RNDC key and endpoints are read once, endpoints from the shared store
    // (ADR-0015, ADR-0016).
    let resolver = bindy_bind9::instances::InstanceResolver::for_kube(&ctx.client, &ctx.stores);
    add_dnszone_with_resolver(
        ctx,
        dnszone,
        status_updater,
        instance_refs,
        peers,
        &resolver,
    )
    .await
}

/// [`add_dnszone`] through a caller-supplied resolver.
///
/// The zones-loaded readiness gate passes a resolver that addresses one pod
/// only, so the zone, its NS and glue records are configured on the pod being
/// admitted exactly as the zone reconcile configures every pod (ADR-0017).
/// `instance_refs` must still be every instance the zone selects: the
/// generated nameservers are derived from them. A zone created here gets the
/// secondaries of `peers` as `allow-transfer` and its NOTIFY targets as
/// `also-notify` (ADR-0019); a zone that already exists is left as it is, its
/// peers are rewritten by `transfer_peers::refresh_primary_peers`.
///
/// # Errors
///
/// Returns an error if BIND9 zone addition fails on every endpoint the
/// resolver returns, or if no PRIMARY instance is assigned.
#[allow(clippy::too_many_lines)]
pub(crate) async fn add_dnszone_with_resolver(
    ctx: Arc<crate::context::Context>,
    dnszone: DNSZone,
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    instance_refs: &[crate::crd::InstanceReference],
    peers: &crate::crd::ZoneTransferPeers,
    resolver: &bindy_bind9::instances::InstanceResolver,
) -> Result<types::ZoneConfigOutcome> {
    let client = ctx.client.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    debug!("Adding DNSZone {}/{}", namespace, name);

    // PHASE 2 OPTIMIZATION: Use the filtered instance list passed by the caller
    // This ensures we only process instances that need reconciliation (lastReconciledAt == None)

    debug!(
        "DNSZone {}/{} will be added to {} instance(s): {:?}",
        namespace,
        name,
        instance_refs.len(),
        instance_refs
            .iter()
            .map(|i| format!("{}/{}", i.namespace, i.name))
            .collect::<Vec<_>>()
    );

    // Filter to only PRIMARY instances, roles from the Bind9Instance store
    // (ADR-0016)
    let primary_instance_refs = primary::filter_primary_instances_cached(
        &client,
        &ctx.stores.bind9_instances,
        instance_refs,
    )
    .await?;

    if primary_instance_refs.is_empty() {
        return Err(anyhow!(
            "DNSZone {}/{} has no PRIMARY instances assigned. Instances: {:?}",
            namespace,
            name,
            instance_refs
                .iter()
                .map(|i| format!("{}/{}", i.namespace, i.name))
                .collect::<Vec<_>>()
        ));
    }

    debug!(
        "Found {} PRIMARY instance(s) for DNSZone {}/{}",
        primary_instance_refs.len(),
        namespace,
        name
    );

    // The secondaries (for nameserver ordering) and their transfer peers:
    // allow-transfer names the secondary pods, also-notify their Services
    // (ADR-0019), both computed by the caller from the stores.
    let secondary_instance_refs =
        secondary::filter_secondary_instances(&client, &ctx.stores.bind9_instances, instance_refs)
            .await?;
    let secondary_ips = peers.secondaries.clone();
    let notify_targets = peers.notify.clone();

    if secondary_ips.is_empty() {
        warn!(
            "No secondary servers found for DNSZone {}/{} - zone transfers will not be configured",
            namespace, name
        );
    } else {
        debug!(
            "Found {} secondary server(s) for DNSZone {}/{} - zone transfers will be configured: {:?}",
            secondary_ips.len(),
            namespace,
            name,
            secondary_ips
        );
    }

    // Get effective nameservers (supports both new `nameServers` and deprecated `nameServerIps`)
    let effective_name_servers = get_effective_name_servers(spec);

    // Generate legacy nameserver IPs format for backward compatibility with bindcar API
    // If user didn't provide either field, auto-generate from instance IPs
    let name_server_ips = if effective_name_servers.is_none() {
        debug!(
            "DNSZone {}/{} has no explicit nameServers - auto-generating from {} instance(s)",
            namespace,
            name,
            instance_refs.len()
        );

        // Build ordered list: primaries first, then secondaries
        let mut ordered_instances = primary_instance_refs.clone();
        ordered_instances.extend(secondary_instance_refs.clone());

        match generate_nameserver_ips(&client, &spec.zone_name, &ordered_instances).await {
            Ok(Some(generated_ips)) => {
                debug!(
                    "Auto-generated {} nameserver(s) for DNSZone {}/{}: {:?}",
                    generated_ips.len(),
                    namespace,
                    name,
                    generated_ips
                );
                Some(generated_ips)
            }
            Ok(None) => {
                warn!(
                    "Failed to auto-generate nameserver IPs for DNSZone {}/{} - no IPs available",
                    namespace, name
                );
                None
            }
            Err(e) => {
                warn!(
                    "Error auto-generating nameserver IPs for DNSZone {}/{}: {}",
                    namespace, name, e
                );
                None
            }
        }
    } else {
        // Convert effective_name_servers to HashMap<String, String> for bindcar API compatibility
        // Only include IPv4 addresses (bindcar doesn't support IPv6 glue records in this field)
        // SAFETY: We know effective_name_servers is Some because we're in the else block
        let name_server_map: HashMap<String, String> =
            if let Some(ref ns_list) = effective_name_servers {
                ns_list
                    .iter()
                    .filter_map(|ns| {
                        ns.ipv4_address
                            .as_ref()
                            .map(|ip| (ns.hostname.clone(), ip.clone()))
                    })
                    .collect()
            } else {
                HashMap::new()
            };

        debug!(
            "Using explicit nameServers for DNSZone {}/{} ({} with IPv4 glue records)",
            namespace,
            name,
            name_server_map.len()
        );

        if name_server_map.is_empty() {
            None
        } else {
            Some(name_server_map)
        }
    };

    // Extract list of ALL nameserver hostnames (primary from SOA + all from nameServers field)
    // This is used by bindcar to generate NS records in the zone file
    let all_nameserver_hostnames: Vec<String> = {
        let mut hostnames = vec![spec.soa_record.primary_ns.clone()];

        if let Some(ref ns_list) = effective_name_servers {
            for ns in ns_list {
                // Avoid duplicates - don't add primary NS again if it's in the list
                if ns.hostname != spec.soa_record.primary_ns {
                    hostnames.push(ns.hostname.clone());
                }
            }
        }

        hostnames
    };

    debug!(
        "Zone {}/{} will be configured with {} nameserver(s): {:?}",
        namespace,
        name,
        all_nameserver_hostnames.len(),
        all_nameserver_hostnames
    );

    // The zone's DNSSEC policy: its own spec.dnssecPolicy, or the signing
    // policy its primary instance renders (instance config over cluster
    // global). Inheriting is what the CRD documents; without it a cluster
    // with signing enabled would sign nothing.
    let resolved_dnssec_policy = zone_dnssec_policy(&ctx, spec, &primary_instance_refs);
    let dnssec_policy = resolved_dnssec_policy.as_deref();
    if let Some(policy) = dnssec_policy {
        debug!(
            "DNSSEC policy '{}' will be applied to zone {}/{}",
            policy, namespace, name
        );
    }

    // Process all primary instances concurrently using async streams
    // Mark each instance as reconciled immediately after first successful endpoint configuration
    let first_endpoint = Arc::new(Mutex::new(None::<NotifyTarget>));
    let total_endpoints = Arc::new(Mutex::new(0_usize));
    // Endpoints where the zone did NOT exist and had to be created. A created
    // zone holds only SOA + NS, so every one of these endpoints is missing all
    // of the zone's record data and needs a replay.
    let zones_created = Arc::new(Mutex::new(0_usize));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let status_updater_shared = Arc::new(Mutex::new(status_updater));

    // Create a stream of futures for all instances.
    // Each instance future resolves to `true` only if EVERY endpoint of that
    // instance accepted the zone (added or already present) - this is the
    // per-INSTANCE success signal used for readiness computation.
    let instance_results = stream::iter(primary_instance_refs.iter())
        .then(|instance_ref| {
            // Per instance, not the shared startup manager: only this carries the
            // instance's TLS configuration. See zone_manager_for_instance.
            let zone_manager =
                zone_manager_for_instance(&ctx, &instance_ref.name, &instance_ref.namespace);
            let zone_name = spec.zone_name.clone();
            let soa_record = spec.soa_record.clone();
            let all_nameserver_hostnames = all_nameserver_hostnames.clone();
            let name_server_ips = name_server_ips.clone();
            let secondary_ips = secondary_ips.clone();
            let notify_targets = notify_targets.clone();
            let first_endpoint = Arc::clone(&first_endpoint);
            let total_endpoints = Arc::clone(&total_endpoints);
            let zones_created = Arc::clone(&zones_created);
            let errors = Arc::clone(&errors);
            let status_updater_shared = Arc::clone(&status_updater_shared);
            let instance_ref = instance_ref.clone();
            let _zone_namespace = namespace.clone();
            let _zone_name_ref = name.clone();

            async move {
                debug!(
                    "Processing endpoints for primary instance {}/{}",
                    instance_ref.namespace, instance_ref.name
                );

                // Load RNDC key for this specific instance
                let key_data = match resolver.rndc_key(&instance_ref.namespace, &instance_ref.name).await {
                    Ok(key) => key,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to load RNDC key: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return false;
                    }
                };

                // Get all endpoints for this instance
                let endpoints = match resolver.endpoints(&instance_ref.namespace, &instance_ref.name, "http").await {
                    Ok(eps) => eps,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to get endpoints: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return false;
                    }
                };

                debug!(
                    "Found {} endpoint(s) for primary instance {}/{}",
                    endpoints.len(),
                    instance_ref.namespace,
                    instance_ref.name
                );

                // Process endpoints concurrently for this instance
                let endpoint_results = stream::iter(endpoints.iter())
                    .then(|endpoint| {
                        let zone_manager = zone_manager.clone();
                        let zone_name = zone_name.clone();
                        let key_data = key_data.clone();
                        let soa_record = soa_record.clone();
                        let all_nameserver_hostnames = all_nameserver_hostnames.clone();
                        let name_server_ips = name_server_ips.clone();
                        let secondary_ips = secondary_ips.clone();
                        let notify_targets = notify_targets.clone();
                        let first_endpoint = Arc::clone(&first_endpoint);
                        let total_endpoints = Arc::clone(&total_endpoints);
                        let zones_created = Arc::clone(&zones_created);
                        let errors = Arc::clone(&errors);
                        let instance_ref = instance_ref.clone();
                        let endpoint = endpoint.clone();

                        async move {
                            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

                            // Save the first endpoint (globally), together with
                            // the instance serving it -- NOTIFY needs that
                            // instance's TLS configuration to dial it.
                            {
                                let mut first = first_endpoint.lock().await;
                                remember_first_notify_target(
                                    &mut first,
                                    &pod_endpoint,
                                    &instance_ref.name,
                                    &instance_ref.namespace,
                                );
                            }

                            // Check if zone already exists before attempting creation
                            let zone_exists = match zone_manager.zone_exists(&zone_name, &pod_endpoint).await {
                                Ok(exists) => exists,
                                Err(e) => {
                                    // An endpoint that cannot say whether it has the
                                    // zone (a pod that is gone, a sidecar restarting)
                                    // is a failed endpoint, retried with the zone's
                                    // backoff. A POST to it would only ride the
                                    // two-minute bindcar retry and hold this zone's
                                    // reconcile, and every other change of the zone,
                                    // behind a dead address (chaos suite).
                                    error!(
                                        "Failed to check if zone {} exists on endpoint {} (instance {}/{}): {:#}",
                                        zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name, e
                                    );
                                    errors.lock().await.push(format!(
                                        "endpoint {pod_endpoint} (instance {}/{}): zone state unknown: {e:#}",
                                        instance_ref.namespace, instance_ref.name
                                    ));
                                    return Err(());
                                }
                            };

                            if zone_exists {
                                debug!(
                                    "Zone {} already exists on endpoint {} (instance {}/{}), skipping creation",
                                    zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name
                                );
                                *total_endpoints.lock().await += 1;
                                // Return false to indicate zone was not newly added
                                return Ok(false);
                            }

                            // Pass secondary IPs for zone transfer configuration
                            let secondary_ips_ref = if secondary_ips.is_empty() {
                                None
                            } else {
                                Some(secondary_ips.as_slice())
                            };
                            let notify_targets_ref = if notify_targets.is_empty() {
                                None
                            } else {
                                Some(notify_targets.as_slice())
                            };

                            match zone_manager
                                .add_zones(
                                    &zone_name,
                                    ZONE_TYPE_PRIMARY,
                                    &pod_endpoint,
                                    &key_data,
                                    Some(&soa_record),
                                    Some(&all_nameserver_hostnames),
                                    name_server_ips.as_ref(),
                                    secondary_ips_ref,
                                    notify_targets_ref,
                                    None, // primary_ips only for secondary zones
                                    dnssec_policy,
                                )
                                .await
                            {
                                Ok(was_added) => {
                                    if was_added {
                                        // The zone was absent from this pod and has just been
                                        // recreated from spec - it currently holds only SOA and
                                        // NS records, so the pod is authoritative for an empty
                                        // zone until the records are replayed.
                                        warn!(
                                            "Zone {} was MISSING on endpoint {} (instance: {}/{}) and has been recreated \
                                             with SOA and NS records only - all records for this zone will be replayed",
                                            zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name
                                        );
                                        *zones_created.lock().await += 1;
                                    }
                                    *total_endpoints.lock().await += 1;
                                    // Return was_added so we can check if zone was actually configured
                                    Ok(was_added)
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to add zone {} to endpoint {} (instance {}/{}): {}",
                                        zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name, e
                                    );
                                    errors.lock().await.push(format!(
                                        "endpoint {pod_endpoint} (instance {}/{}): {e}",
                                        instance_ref.namespace, instance_ref.name
                                    ));
                                    Err(())
                                }
                            }
                        }
                    })
                    .collect::<Vec<Result<bool, ()>>>()
                    .await;

                // Mark this instance as configured if at least one endpoint accepted
                // the zone - freshly added OR already present. has_changes() compares
                // instance lists excluding lastReconciledAt, so re-marking an already
                // recorded instance does not cause a status patch per cycle.
                let zone_was_configured = instance_serves_zone(&endpoint_results);
                if zone_was_configured {
                    status_updater_shared
                        .lock()
                        .await
                        .update_instance_status(
                            &instance_ref.name,
                            &instance_ref.namespace,
                            crate::crd::InstanceStatus::Configured,
                            Some("Zone successfully configured on primary instance".to_string()),
                        );
                    debug!(
                        "Marked primary instance {}/{} as configured for zone {}",
                        instance_ref.namespace, instance_ref.name, zone_name
                    );
                }

                // The instance counts as fully configured only if every one of
                // its ready endpoints accepted the zone (added OR already
                // present). A single failed endpoint means the instance is NOT
                // fully serving the zone and must not count towards readiness.
                !endpoint_results.is_empty() && endpoint_results.iter().all(Result::is_ok)
            }
        })
        .collect::<Vec<bool>>()
        .await;

    let instances_configured = instance_results.iter().filter(|ok| **ok).count();

    let first_endpoint = Arc::try_unwrap(first_endpoint)
        .expect("Failed to unwrap first_endpoint Arc")
        .into_inner();
    let total_endpoints = Arc::try_unwrap(total_endpoints)
        .expect("Failed to unwrap total_endpoints Arc")
        .into_inner();
    let zones_created = Arc::try_unwrap(zones_created)
        .expect("Failed to unwrap zones_created Arc")
        .into_inner();
    let errors = Arc::try_unwrap(errors)
        .expect("Failed to unwrap errors Arc")
        .into_inner();
    let status_updater = Arc::try_unwrap(status_updater_shared)
        .map_err(|_| anyhow!("Failed to unwrap status_updater - multiple references remain"))?
        .into_inner();

    // If ALL operations failed, return an error
    if total_endpoints == 0 && !errors.is_empty() {
        return Err(anyhow!(
            "Failed to add zone {} to all primary instances. Errors: {}",
            spec.zone_name,
            errors.join("; ")
        ));
    }

    debug!(
        "Successfully added zone {} to {} endpoint(s) across {}/{} fully configured primary instance(s)",
        spec.zone_name,
        total_endpoints,
        instances_configured,
        primary_instance_refs.len()
    );

    // Auto-generate NS records and glue records from nameServers field
    if let Some(ref name_servers) = effective_name_servers {
        if !name_servers.is_empty() {
            debug!(
                "Auto-generating NS records for {} nameserver(s) in zone {}",
                name_servers.len(),
                spec.zone_name
            );

            if let Err(e) = auto_generate_ns_records(
                resolver,
                name_servers,
                &spec.zone_name,
                spec.ttl,
                &primary_instance_refs,
            )
            .await
            {
                warn!(
                    "Failed to auto-generate some NS records for zone {}: {}. \
                     Zone is functional but may have incomplete NS records.",
                    spec.zone_name, e
                );
                // Don't fail reconciliation - zone is functional even without all NS records
            }
        }
    }

    // Note: We don't need to reload after addzone because:
    // 1. rndc addzone immediately adds the zone to BIND9's running config
    // 2. The zone file will be created automatically when records are added via dynamic updates
    // 3. Reloading would fail if the zone file doesn't exist yet

    // Publish DNSSEC status (DS records) derived from the zone's DNSKEYs,
    // queried on the first configured endpoint (ADR-0006).
    update_dnssec_status(
        status_updater,
        &spec.zone_name,
        dnssec_policy,
        first_endpoint.as_ref(),
        &ctx,
    )
    .await;

    // Notify secondaries about the new zone via the first endpoint
    // This triggers zone transfer (AXFR) from primary to secondaries
    if let Some(notify_target) = first_endpoint {
        debug!("Notifying secondaries about new zone {}", spec.zone_name);
        // Per instance, not the shared startup manager: only this carries the
        // instance's TLS configuration. See zone_manager_for_instance.
        let notify_manager = zone_manager_for_instance(
            &ctx,
            &notify_target.instance_name,
            &notify_target.instance_namespace,
        );
        if let Err(e) = notify_manager
            .notify_zone(&spec.zone_name, &notify_target.endpoint)
            .await
        {
            // Don't fail if NOTIFY fails - the zone was successfully created
            // Secondaries will sync via SOA refresh timer
            warn!(
                "Failed to notify secondaries for zone {}: {}. Secondaries will sync via SOA refresh timer.",
                spec.zone_name, e
            );
        }
    } else {
        warn!(
            "No endpoints found for zone {}, cannot notify secondaries",
            spec.zone_name
        );
    }

    Ok(types::ZoneConfigOutcome {
        instances_configured,
        endpoints_configured: total_endpoints,
        zones_created,
    })
}

/// What configuring a zone on the secondaries achieved (ADR-0019).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecondaryConfigOutcome {
    /// Per-instance and per-endpoint counts. An instance counts as configured
    /// only when every one of its endpoints has the zone **loaded**.
    pub outcome: types::ZoneConfigOutcome,
    /// Endpoints that have the zone configured but not loaded, as
    /// `namespace/instance (ip:port)`.
    pub not_loaded: Vec<String>,
    /// Endpoints where the zone could not be created or replaced, or whose
    /// state could not be read when a replace was required.
    pub failures: usize,
}

/// Adds a DNS zone to all secondary instances, transferring from
/// `primary_ips`.
///
/// For each secondary endpoint the zone's presence is read (bindcar status,
/// with a SOA probe of `named` when bindcar cannot tell, ADR-0019): an absent
/// zone is created; an existing zone is **replaced** (deleted and created
/// again with exactly `primary_ips`) when `replace_existing` is set, because
/// bindcar 0.9.0 cannot change a secondary's primaries in place; then a
/// `retransfer` is issued. An endpoint whose zone was not loaded before this
/// reconcile, or was just created or replaced, is reported in `not_loaded`.
///
/// # Arguments
///
/// * `ctx` - Controller context
/// * `dnszone` - The `DNSZone` resource
/// * `primary_ips` - The transfer sources (admitted primary pod IPs)
/// * `status_updater` - Status updater for per-instance status
/// * `instance_refs` - Every instance the zone selects
/// * `replace_existing` - Whether the transfer sources changed since they
///   were last pushed
///
/// # Returns
///
/// The [`SecondaryConfigOutcome`].
///
/// # Errors
///
/// Returns an error if BIND9 zone addition fails on every endpoint.
pub async fn add_dnszone_to_secondaries(
    ctx: Arc<crate::context::Context>,
    dnszone: DNSZone,
    primary_ips: &[String],
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    instance_refs: &[crate::crd::InstanceReference],
    replace_existing: bool,
) -> Result<SecondaryConfigOutcome> {
    // One resolver for every secondary's RNDC key and endpoints (ADR-0016)
    let resolver = bindy_bind9::instances::InstanceResolver::for_kube(&ctx.client, &ctx.stores);
    add_dnszone_to_secondaries_with_resolver(
        ctx,
        dnszone,
        primary_ips,
        status_updater,
        instance_refs,
        replace_existing,
        &resolver,
    )
    .await
}

/// The result of one secondary endpoint: whether the zone was created
/// (`Ok(true)`), already there (`Ok(false)`) or failed (`Err`), and whether
/// it is loaded.
type SecondaryEndpointResult = (std::result::Result<bool, ()>, bool);

/// [`add_dnszone_to_secondaries`] through a caller-supplied resolver.
///
/// The zones-loaded readiness gate passes a resolver that addresses one
/// secondary pod only (ADR-0017).
///
/// # Errors
///
/// Returns an error if BIND9 zone addition fails on every endpoint the
/// resolver returns.
#[allow(clippy::too_many_lines)]
pub(crate) async fn add_dnszone_to_secondaries_with_resolver(
    ctx: Arc<crate::context::Context>,
    dnszone: DNSZone,
    primary_ips: &[String],
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
    instance_refs: &[crate::crd::InstanceReference],
    replace_existing: bool,
    resolver: &bindy_bind9::instances::InstanceResolver,
) -> Result<SecondaryConfigOutcome> {
    use crate::bind9::zone_ops::ZonePresence;

    let client = ctx.client.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    if primary_ips.is_empty() {
        warn!(
            "No primary IPs provided for secondary zone {}/{} - skipping secondary configuration",
            namespace, spec.zone_name
        );
        return Ok(SecondaryConfigOutcome::default());
    }

    debug!(
        "Adding DNSZone {}/{} to secondary instances with primaries: {:?} (replace existing: {})",
        namespace, name, primary_ips, replace_existing
    );

    // Filter to only SECONDARY instances, roles from the Bind9Instance store
    let secondary_instance_refs =
        secondary::filter_secondary_instances(&client, &ctx.stores.bind9_instances, instance_refs)
            .await?;

    if secondary_instance_refs.is_empty() {
        info!(
            "No secondary instances found for DNSZone {}/{} - skipping secondary zone configuration",
            namespace, name
        );
        return Ok(SecondaryConfigOutcome::default());
    }

    let total_endpoints = Arc::new(Mutex::new(0_usize));
    // Endpoints where the secondary zone had to be created (see the primary
    // path for why this matters). A replaced zone is not counted: the
    // primaries still hold every record, nothing needs a replay.
    let zones_created = Arc::new(Mutex::new(0_usize));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let not_loaded = Arc::new(Mutex::new(Vec::<String>::new()));
    let status_updater_shared = Arc::new(Mutex::new(status_updater));

    let instance_results = stream::iter(secondary_instance_refs.iter())
        .then(|instance_ref| {
            let zone_manager =
                zone_manager_for_instance(&ctx, &instance_ref.name, &instance_ref.namespace);
            let zone_name = spec.zone_name.clone();
            let primary_ips = primary_ips.to_vec();
            let total_endpoints = Arc::clone(&total_endpoints);
            let zones_created = Arc::clone(&zones_created);
            let errors = Arc::clone(&errors);
            let not_loaded = Arc::clone(&not_loaded);
            let status_updater_shared = Arc::clone(&status_updater_shared);
            let instance_ref = instance_ref.clone();

            async move {
                debug!(
                    "Processing secondary instance {}/{} for zone {}",
                    instance_ref.namespace, instance_ref.name, zone_name
                );

                // Each instance has its own RNDC secret for security isolation
                let key_data = match resolver.rndc_key(&instance_ref.namespace, &instance_ref.name).await {
                    Ok(key) => key,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to load RNDC key: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return false;
                    }
                };

                let endpoints = match resolver.endpoints(&instance_ref.namespace, &instance_ref.name, "http").await {
                    Ok(eps) => eps,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to get endpoints: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return false;
                    }
                };

                let endpoint_results = stream::iter(endpoints.iter())
                    .then(|endpoint| {
                        let zone_manager = zone_manager.clone();
                        let zone_name = zone_name.clone();
                        let key_data = key_data.clone();
                        let primary_ips = primary_ips.clone();
                        let total_endpoints = Arc::clone(&total_endpoints);
                        let zones_created = Arc::clone(&zones_created);
                        let errors = Arc::clone(&errors);
                        let instance_ref = instance_ref.clone();
                        let endpoint = endpoint.clone();

                        async move {
                            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);
                            let label = format!("{}/{} ({pod_endpoint})", instance_ref.namespace, instance_ref.name);

                            // Absent, loaded, or configured with no data
                            // (ADR-0019). An endpoint whose state cannot be read
                            // (a pod that is gone, a sidecar restarting) is a
                            // failed endpoint retried with the zone's backoff,
                            // never a POST that rides the two-minute bindcar
                            // retry against a dead address.
                            let presence = match zone_manager.zone_presence(&zone_name, &pod_endpoint).await {
                                Ok(presence) => Some(presence),
                                Err(e) => {
                                    warn!("Cannot read zone {zone_name} on secondary {label}: {e:#}");
                                    errors.lock().await.push(format!("endpoint {label}: zone state unknown: {e:#}"));
                                    return (Err(()), false);
                                }
                            };
                            let exists = matches!(presence, Some(ZonePresence::Loaded | ZonePresence::NotLoaded));
                            let mut loaded = presence == Some(ZonePresence::Loaded);

                            let created = if exists && replace_existing {
                                match zone_manager
                                    .replace_secondary_zone(&zone_name, &pod_endpoint, &key_data, &primary_ips)
                                    .await
                                {
                                    Ok(()) => {
                                        info!("Replaced secondary zone {zone_name} on {label}: primaries {primary_ips:?}");
                                        loaded = false;
                                        false
                                    }
                                    Err(e) => {
                                        error!("Failed to replace secondary zone {zone_name} on {label}: {e:#}");
                                        errors.lock().await.push(format!("endpoint {label}: {e:#}"));
                                        return (Err(()), false);
                                    }
                                }
                            } else if exists {
                                false
                            } else {
                                match zone_manager
                                    .add_zones(
                                        &zone_name,
                                        ZONE_TYPE_SECONDARY,
                                        &pod_endpoint,
                                        &key_data,
                                        None, // No SOA record for secondary zones
                                        None, // No name_servers for secondary zones
                                        None, // No name_server_ips for secondary zones
                                        None, // No secondary_ips for secondary zones
                                        None, // No notify targets for secondary zones
                                        Some(&primary_ips),
                                        None, // No DNSSEC policy for secondary zones
                                    )
                                    .await
                                {
                                    Ok(added) => {
                                        if added {
                                            info!("Added secondary zone {zone_name} to {label}");
                                            *zones_created.lock().await += 1;
                                        }
                                        loaded = false;
                                        added
                                    }
                                    Err(e) => {
                                        error!("Failed to add secondary zone {zone_name} to {label}: {e:#}");
                                        errors.lock().await.push(format!("endpoint {label}: {e:#}"));
                                        return (Err(()), false);
                                    }
                                }
                            };
                            *total_endpoints.lock().await += 1;

                            // `rndc addzone` only configures the zone; the data
                            // comes from an AXFR. Force one now (also on a zone
                            // that already existed, to bring it up to date).
                            if let Err(e) = zone_manager.retransfer_zone(&zone_name, &pod_endpoint).await {
                                warn!(
                                    "Failed to trigger zone transfer for {zone_name} on {label}: {e:#}. Zone will sync via SOA refresh timer."
                                );
                            }

                            (Ok(created), loaded)
                        }
                    })
                    .collect::<Vec<SecondaryEndpointResult>>()
                    .await;

                let accepted: Vec<std::result::Result<bool, ()>> =
                    endpoint_results.iter().map(|(result, _)| *result).collect();
                let all_loaded = !endpoint_results.is_empty()
                    && endpoint_results.iter().all(|(result, loaded)| result.is_ok() && *loaded);
                let unloaded: Vec<String> = endpoint_results
                    .iter()
                    .zip(endpoints.iter())
                    .filter(|((result, loaded), _)| result.is_ok() && !*loaded)
                    .map(|(_, endpoint)| {
                        format!(
                            "{}/{} ({}:{})",
                            instance_ref.namespace, instance_ref.name, endpoint.ip, endpoint.port
                        )
                    })
                    .collect();

                if instance_serves_zone(&accepted) {
                    let (status, message) = if unloaded.is_empty() {
                        (
                            crate::crd::InstanceStatus::Configured,
                            "Zone successfully configured on secondary instance".to_string(),
                        )
                    } else {
                        (
                            crate::crd::InstanceStatus::Failed,
                            format!(
                                "Zone configured but not loaded on {} endpoint(s): transfer pending or denied",
                                unloaded.len()
                            ),
                        )
                    };
                    status_updater_shared.lock().await.update_instance_status(
                        &instance_ref.name,
                        &instance_ref.namespace,
                        status,
                        Some(message),
                    );
                }
                not_loaded.lock().await.extend(unloaded);

                // Fully configured only if every endpoint has the zone loaded.
                all_loaded
            }
        })
        .collect::<Vec<bool>>()
        .await;

    let instances_configured = instance_results.iter().filter(|ok| **ok).count();

    let total_endpoints = Arc::try_unwrap(total_endpoints)
        .expect("Failed to unwrap total_endpoints Arc")
        .into_inner();
    let zones_created = Arc::try_unwrap(zones_created)
        .expect("Failed to unwrap zones_created Arc")
        .into_inner();
    let errors = Arc::try_unwrap(errors)
        .expect("Failed to unwrap errors Arc")
        .into_inner();
    let not_loaded = Arc::try_unwrap(not_loaded)
        .expect("Failed to unwrap not_loaded Arc")
        .into_inner();

    // If ALL operations failed, return an error
    if total_endpoints == 0 && !errors.is_empty() {
        return Err(anyhow!(
            "Failed to add zone {} to all secondary instances. Errors: {}",
            spec.zone_name,
            errors.join("; ")
        ));
    }

    debug!(
        "Secondary zone {}: {} endpoint(s) configured, {}/{} instance(s) fully loaded, not loaded on {:?}",
        spec.zone_name,
        total_endpoints,
        instances_configured,
        secondary_instance_refs.len(),
        not_loaded
    );

    Ok(SecondaryConfigOutcome {
        outcome: types::ZoneConfigOutcome {
            instances_configured,
            endpoints_configured: total_endpoints,
            zones_created,
        },
        not_loaded,
        failures: errors.len(),
    })
}

/// Deletes a DNS zone and its associated zone files.
///
/// # Arguments
///
/// * `_client` - Kubernetes API client (unused, for future extensions)
/// * `dnszone` - The `DNSZone` resource to delete
/// * `zone_manager` - BIND9 manager for removing zone files
///
/// # Returns
///
/// * `Ok(())` - If zone was deleted successfully
/// * `Err(_)` - If zone deletion failed
///
/// # Errors
///
/// Returns an error if BIND9 zone deletion fails.
pub async fn delete_dnszone(ctx: Arc<crate::context::Context>, dnszone: DNSZone) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    info!("Deleting DNSZone {}/{}", namespace, name);

    // Get instances from new architecture (spec.bind9Instances or status.bind9Instances)
    // If zone has no instances assigned (e.g., orphaned zone), still allow deletion
    let instance_refs = match validation::get_instances_from_zone(&dnszone, bind9_instances_store) {
        Ok(refs) => refs,
        Err(e) => {
            warn!(
                "DNSZone {}/{} has no instances assigned: {}. Allowing deletion anyway.",
                namespace, name, e
            );
            return Ok(());
        }
    };

    // Filter to primary and secondary instances, roles from the
    // Bind9Instance store (ADR-0016)
    let primary_instance_refs =
        primary::filter_primary_instances_cached(&client, bind9_instances_store, &instance_refs)
            .await?;
    let secondary_instance_refs =
        secondary::filter_secondary_instances(&client, bind9_instances_store, &instance_refs)
            .await?;

    // One resolver for every endpoint this deletion addresses (ADR-0015)
    let resolver = bindy_bind9::instances::InstanceResolver::for_kube(&client, &ctx.stores);

    // Namespace per instance name, for the deletion callbacks below.
    let primary_ns_by_name: std::collections::HashMap<String, String> = primary_instance_refs
        .iter()
        .map(|r| (r.name.clone(), r.namespace.clone()))
        .collect();

    // Delete from all primary instances.
    // Deletion cleanup uses SkipUnavailable: an instance whose data is gone
    // (instance deleted, pods gone) must not block finalizer removal. A pod
    // that still holds the zone but cannot be reached right now (a container
    // restarting) fails the deletion, which is retried with backoff: removing
    // the finalizer then left the zone served from the pod's surviving
    // emptyDir (chaos suite). Real API errors still propagate.
    if !primary_instance_refs.is_empty() {
        let (_first_endpoint, total_endpoints) = helpers::for_each_instance_endpoint_with_policy(
            &resolver,
            &primary_instance_refs,
            false,  // with_rndc_key = false for zone deletion
            "http", // Use HTTP API port for zone deletion via bindcar API
            helpers::EndpointFailurePolicy::SkipUnavailable,
            |pod_endpoint, instance_name, _rndc_key| {
                let zone_name = spec.zone_name.clone();
                // The callback only names the instance, so recover its namespace
                // from the refs being iterated to build the per-instance manager.
                let instance_namespace = primary_ns_by_name
                    .get(instance_name.as_str())
                    .cloned()
                    .unwrap_or_else(|| namespace.clone());
                let zone_manager =
                    zone_manager_for_instance(&ctx, &instance_name, &instance_namespace);

                async move {
                    debug!(
                        "Deleting zone {} from endpoint {} (instance: {})",
                        zone_name, pod_endpoint, instance_name
                    );

                    // An absent zone counts as deleted. A failure is reported:
                    // tolerated for a pod that no longer holds the zone,
                    // retried for one that does (coverage check). Each call
                    // gives up within DELETE_RETRY_BUDGET, so an endpoint that
                    // is gone cannot hold this reconcile for minutes.
                    if let Err(e) = zone_manager.delete_zone(&zone_name, &pod_endpoint).await {
                        warn!(
                            "Failed to delete zone {} from endpoint {} (instance: {}): {:#}",
                            zone_name, pod_endpoint, instance_name, e
                        );
                        return Err(e);
                    }
                    debug!(
                        "Successfully deleted zone {} from endpoint {} (instance: {})",
                        zone_name, pod_endpoint, instance_name
                    );
                    Ok(())
                }
            },
        )
        .await?;

        info!(
            "Successfully deleted zone {} from {} primary endpoint(s)",
            spec.zone_name, total_endpoints
        );
    }

    // Delete from all secondary instances, with the same rule: skipped when
    // its data is gone, retried while a pod that holds the zone cannot be
    // reached (a secondary left with the zone would serve it until expiry).
    if !secondary_instance_refs.is_empty() {
        let secondary_ns_by_name: std::collections::HashMap<String, String> =
            secondary_instance_refs
                .iter()
                .map(|r| (r.name.clone(), r.namespace.clone()))
                .collect();
        let (_first_endpoint, secondary_endpoints_deleted) =
            helpers::for_each_instance_endpoint_with_policy(
                &resolver,
                &secondary_instance_refs,
                false, // with_rndc_key = false for zone deletion
                "http",
                helpers::EndpointFailurePolicy::SkipUnavailable,
                |pod_endpoint, instance_name, _rndc_key| {
                    let zone_name = spec.zone_name.clone();
                    let instance_namespace = secondary_ns_by_name
                        .get(instance_name.as_str())
                        .cloned()
                        .unwrap_or_else(|| namespace.clone());
                    // Per instance, not the shared startup manager: only this
                    // carries the instance's TLS configuration.
                    let zone_manager =
                        zone_manager_for_instance(&ctx, &instance_name, &instance_namespace);
                    async move {
                        if let Err(e) = zone_manager.delete_zone(&zone_name, &pod_endpoint).await {
                            warn!(
                                "Failed to delete zone {} from secondary endpoint {} (instance: {}): {:#}",
                                zone_name, pod_endpoint, instance_name, e
                            );
                            return Err(e);
                        }
                        debug!(
                            "Successfully deleted zone {} from secondary endpoint {} (instance: {})",
                            zone_name, pod_endpoint, instance_name
                        );
                        Ok(())
                    }
                },
            )
            .await?;

        info!(
            "Successfully deleted zone {} from {} secondary endpoint(s)",
            spec.zone_name, secondary_endpoints_deleted
        );
    }

    // Note: We don't need to reload after delzone because:
    // 1. rndc delzone immediately removes the zone from BIND9's running config
    // 2. BIND9 will clean up the zone file and journal files automatically

    Ok(())
}

/// Auto-generates NS records for all nameservers in the zone.
///
/// This function is called after zone creation to add NS records for secondary nameservers
/// specified in the `nameServers` field. The primary nameserver NS record is already created
/// by bindcar during zone initialization (from SOA).
///
/// # Arguments
/// * `resolver` - Per-reconcile resolver for instance RNDC keys and endpoints
/// * `effective_name_servers` - List of nameservers from `nameServers` field
/// * `zone_name` - The DNS zone name
/// * `ttl` - TTL for the NS and glue records
/// * `primary_instance_refs` - List of primary instances to update
///
/// # Returns
/// Result indicating success or failure
///
/// # Errors
/// Returns error if NS record or glue record addition fails
#[allow(clippy::too_many_lines)]
async fn auto_generate_ns_records(
    resolver: &bindy_bind9::instances::InstanceResolver,
    effective_name_servers: &[crate::crd::NameServer],
    zone_name: &str,
    ttl: Option<i32>,
    primary_instance_refs: &[crate::crd::InstanceReference],
) -> Result<()> {
    if effective_name_servers.is_empty() {
        return Ok(());
    }

    debug!(
        "Auto-generating {} NS record(s) for zone {}",
        effective_name_servers.len(),
        zone_name
    );

    for nameserver in effective_name_servers {
        // Add NS record at zone apex (@)
        debug!(
            "Adding NS record: {} IN NS {}",
            zone_name, nameserver.hostname
        );

        for instance_ref in primary_instance_refs {
            // Load RNDC key for this instance
            let key_data = match resolver
                .rndc_key(&instance_ref.namespace, &instance_ref.name)
                .await
            {
                Ok(key) => key,
                Err(e) => {
                    warn!(
                        "Failed to load RNDC key for instance {}/{}: {}. Skipping NS record addition.",
                        instance_ref.namespace, instance_ref.name, e
                    );
                    continue;
                }
            };

            // Get endpoints for this instance
            let endpoints = match resolver
                .endpoints(&instance_ref.namespace, &instance_ref.name, "dns-tcp")
                .await
            {
                Ok(eps) => eps,
                Err(e) => {
                    warn!(
                        "Failed to get endpoints for instance {}/{}: {}. Skipping NS record addition.",
                        instance_ref.namespace, instance_ref.name, e
                    );
                    continue;
                }
            };

            // Add NS record to all endpoints of this instance
            for endpoint in &endpoints {
                let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

                if let Err(e) = crate::bind9::records::ns::add_ns_record(
                    zone_name,
                    "@", // Zone apex
                    &nameserver.hostname,
                    ttl,
                    &pod_endpoint,
                    &key_data,
                )
                .await
                {
                    warn!(
                        "Failed to add NS record for {} to endpoint {} (instance {}/{}): {}",
                        nameserver.hostname,
                        pod_endpoint,
                        instance_ref.namespace,
                        instance_ref.name,
                        e
                    );
                    // Continue with other endpoints - partial success is acceptable
                }
            }
        }

        // Add glue records if IPs provided (for in-zone nameservers)
        if let Some(ref ipv4) = nameserver.ipv4_address {
            add_glue_record(
                resolver,
                zone_name,
                &nameserver.hostname,
                ipv4,
                hickory_proto::rr::RecordType::A,
                ttl,
                primary_instance_refs,
            )
            .await?;
        }

        if let Some(ref ipv6) = nameserver.ipv6_address {
            add_glue_record(
                resolver,
                zone_name,
                &nameserver.hostname,
                ipv6,
                hickory_proto::rr::RecordType::AAAA,
                ttl,
                primary_instance_refs,
            )
            .await?;
        }
    }

    debug!(
        "Successfully auto-generated NS records and glue records for zone {}",
        zone_name
    );

    Ok(())
}

/// Adds a glue record (A or AAAA) for an in-zone nameserver.
///
/// Glue records provide IP addresses for nameservers within the zone's own domain.
/// This is necessary to avoid circular dependencies when resolving the nameserver itself.
///
/// # Arguments
/// * `resolver` - Per-reconcile resolver for instance RNDC keys and endpoints
/// * `zone_name` - The DNS zone name
/// * `hostname` - Full nameserver hostname (e.g., "ns2.example.com.")
/// * `ip_address` - IP address (IPv4 or IPv6)
/// * `record_type` - Type of glue record (A or AAAA)
/// * `ttl` - TTL for the glue record
/// * `primary_instance_refs` - List of primary instances to update
///
/// # Returns
/// Result indicating success or failure
///
/// # Errors
/// Returns error if glue record addition fails on all instances
#[allow(clippy::too_many_lines)]
async fn add_glue_record(
    resolver: &bindy_bind9::instances::InstanceResolver,
    zone_name: &str,
    hostname: &str,
    ip_address: &str,
    record_type: hickory_proto::rr::RecordType,
    ttl: Option<i32>,
    primary_instance_refs: &[crate::crd::InstanceReference],
) -> Result<()> {
    // Extract record name from hostname
    // Example: "ns2.example.com." in zone "example.com" → name = "ns2"
    let name = hostname
        .trim_end_matches('.')
        .strip_suffix(&format!(".{}", zone_name.trim_end_matches('.')))
        .unwrap_or_else(|| hostname.trim_end_matches('.'));

    // Check if this is actually an in-zone nameserver
    if name == hostname.trim_end_matches('.') {
        // Hostname doesn't end with zone name - this is an out-of-zone nameserver
        // No glue record needed
        debug!(
            "Skipping glue record for out-of-zone nameserver {} (not in zone {})",
            hostname, zone_name
        );
        return Ok(());
    }

    debug!(
        "Adding {} glue record: {} IN {} {}",
        if record_type == hickory_proto::rr::RecordType::A {
            "A"
        } else {
            "AAAA"
        },
        name,
        if record_type == hickory_proto::rr::RecordType::A {
            "A"
        } else {
            "AAAA"
        },
        ip_address
    );

    let mut success_count = 0;
    let mut errors = Vec::new();

    for instance_ref in primary_instance_refs {
        // Load RNDC key for this instance
        let key_data = match resolver
            .rndc_key(&instance_ref.namespace, &instance_ref.name)
            .await
        {
            Ok(key) => key,
            Err(e) => {
                warn!(
                    "Failed to load RNDC key for instance {}/{}: {}. Skipping glue record addition.",
                    instance_ref.namespace, instance_ref.name, e
                );
                continue;
            }
        };

        // Get endpoints for this instance
        let endpoints = match resolver
            .endpoints(&instance_ref.namespace, &instance_ref.name, "dns-tcp")
            .await
        {
            Ok(eps) => eps,
            Err(e) => {
                warn!(
                    "Failed to get endpoints for instance {}/{}: {}. Skipping glue record addition.",
                    instance_ref.namespace, instance_ref.name, e
                );
                continue;
            }
        };

        // Add glue record to all endpoints of this instance
        for endpoint in &endpoints {
            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

            let result = match record_type {
                hickory_proto::rr::RecordType::A => {
                    crate::bind9::records::a::add_a_record(
                        zone_name,
                        name,
                        &[ip_address.to_string()],
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                }
                hickory_proto::rr::RecordType::AAAA => {
                    crate::bind9::records::a::add_aaaa_record(
                        zone_name,
                        name,
                        &[ip_address.to_string()],
                        ttl,
                        &pod_endpoint,
                        &key_data,
                    )
                    .await
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Invalid record type for glue record: {:?}",
                        record_type
                    ))
                }
            };

            match result {
                Ok(()) => {
                    success_count += 1;
                }
                Err(e) => {
                    warn!(
                        "Failed to add glue record {} to endpoint {} (instance {}/{}): {}",
                        name, pod_endpoint, instance_ref.namespace, instance_ref.name, e
                    );
                    errors.push(format!(
                        "endpoint {} (instance {}/{}): {}",
                        pod_endpoint, instance_ref.namespace, instance_ref.name, e
                    ));
                }
            }
        }
    }

    // Accept partial success - at least one endpoint updated
    if success_count > 0 {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Failed to add glue record {} to all instances. Errors: {}",
            name,
            errors.join("; ")
        ))
    }
}

/// Whether an instance is serving a zone, given per-endpoint results of
/// pushing the zone (`Ok(was_added)` per endpoint, `Err(())` on failure).
///
/// `Ok(false)` means the endpoint already had the zone; the instance is
/// serving it just the same and must be recorded in `status.bind9Instances`
/// with a `lastReconciledAt` timestamp. If only `Ok(true)` counted, a zone
/// whose data predates the CR (a recreated `DNSZone`, an operator restart
/// after partial status loss) would never record its instances and would
/// requeue as "unreconciled" on every cycle.
pub(crate) fn instance_serves_zone(endpoint_results: &[std::result::Result<bool, ()>]) -> bool {
    endpoint_results.iter().any(std::result::Result::is_ok)
}

#[cfg(test)]
#[path = "dnszone_tests.rs"]
mod dnszone_tests;
