// Copyright (c) 2025 Erick Bourgeois, firestoned
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::doc_markdown)]
// SPDX-License-Identifier: MIT

//! DNS zone reconciliation logic.
//!
//! This module handles the creation and management of DNS zones on BIND9 servers.
//! It supports both primary and secondary zone configurations.

// Module imports
pub mod bind9_config;
pub mod cleanup;
pub mod constants;
pub mod discovery;
pub mod helpers;
pub mod primary;
pub mod secondary;
pub mod status_helpers;
pub mod types;
pub mod validation;

#[cfg(test)]
#[path = "dnszone/helpers_tests.rs"]
mod helpers_tests;

// Bind9Instance and InstanceReferenceWithStatus are used by dead_code marked functions (Phase 2 cleanup)
use self::types::DuplicateZoneInfo;
#[allow(unused_imports)]
use crate::crd::{Condition, DNSZone, DNSZoneStatus};
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

            match pod_api.list(&lp).await {
                Ok(pods) => {
                    // Find first running pod
                    pods.items
                        .iter()
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
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
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
        crate::reconcilers::should_reconcile(current_generation, observed_generation);

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
/// * `Ok(())` - If zone was created/updated successfully
/// * `Err(_)` - If zone creation failed or configuration is invalid
///
/// # Example
///
/// ```rust,no_run,ignore
/// use bindy::reconcilers::reconcile_dnszone;
/// use bindy::crd::DNSZone;
/// use bindy::bind9::Bind9Manager;
/// use bindy::context::Context;
/// use std::sync::Arc;
///
/// async fn handle_zone(ctx: Arc<Context>, zone: DNSZone) -> anyhow::Result<()> {
///     let manager = Bind9Manager::new();
///     reconcile_dnszone(ctx, zone, &manager).await?;
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
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let client = ctx.client.clone();
    let bind9_instances_store = &ctx.stores.bind9_instances;

    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();

    info!("Reconciling DNSZone: {}/{}", namespace, name);
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
    let mut status_updater = crate::reconcilers::status::DNSZoneStatusUpdater::new(&dnszone);

    // Extract spec
    let spec = &dnszone.spec;

    // Validate that zone has instances assigned (via spec.bind9Instances or status.bind9Instances)
    // This will fail early if zone is not selected by any instance
    let instance_refs = validation::get_instances_from_zone(&dnszone, bind9_instances_store)?;

    info!(
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
        return Ok(());
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
            // Don't fail reconciliation for cleanup errors
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

    info!(
        "Reconciling zone {} (first_reconciliation={}, spec_changed={})",
        spec.zone_name, first_reconciliation, spec_changed
    );

    // Cleanup stale records from status.records[] before main reconciliation
    // This ensures status stays in sync with actual Kubernetes resources
    match cleanup::cleanup_stale_records(
        &client,
        &dnszone,
        &mut status_updater,
        bind9_instances_store,
    )
    .await
    {
        Ok(stale_count) if stale_count > 0 => {
            info!(
                "Cleaned up {} stale record(s) from zone {}/{} status",
                stale_count, namespace, name
            );
        }
        Ok(_) => {
            debug!(
                "No stale records found in zone {}/{} status",
                namespace, name
            );
        }
        Err(e) => {
            warn!(
                "Failed to cleanup stale records for zone {}/{}: {} (continuing with reconciliation)",
                namespace, name, e
            );
            // Don't fail reconciliation for cleanup errors
        }
    }

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
    let (primary_count, secondary_count) = bind9_config::configure_zone_on_instances(
        ctx.clone(),
        &dnszone,
        zone_manager,
        &mut status_updater,
        &instance_refs,
        &unreconciled_instances,
    )
    .await?;

    // Discover DNS records and update status
    let (record_refs, records_count) =
        discovery::discover_and_update_records(&client, &dnszone, &mut status_updater).await?;

    // Check if all discovered records are ready and trigger zone transfers if needed
    if records_count > 0 {
        let all_records_ready =
            discovery::check_all_records_ready(&client, &namespace, &record_refs).await?;

        if all_records_ready {
            info!(
                "All {} record(s) for zone {} are ready, triggering zone transfers to secondaries",
                records_count, spec.zone_name
            );

            // Trigger zone transfers to all secondaries
            // Zone transfers are triggered automatically by BIND9 via NOTIFY messages
            // No manual trigger needed in the new architecture
            info!(
                "Zone {} configured on instances - BIND9 will handle zone transfers via NOTIFY",
                spec.zone_name
            );
        } else {
            info!("Not all records for zone {} are ready yet", spec.zone_name);
        }
    }
    // Calculate expected counts and finalize status
    let (expected_primary_count, expected_secondary_count) =
        status_helpers::calculate_expected_instance_counts(&client, &instance_refs).await?;

    status_helpers::finalize_zone_status(
        &mut status_updater,
        &client,
        &spec.zone_name,
        &namespace,
        &name,
        primary_count,
        secondary_count,
        expected_primary_count,
        expected_secondary_count,
        records_count,
        dnszone.metadata.generation,
    )
    .await?;

    // Trigger record reconciliation: Update all matching records with a "zone-reconciled" annotation
    // This ensures records are re-added to BIND9 after pod restarts or zone recreation
    if !status_updater.has_degraded_condition() {
        if let Err(e) =
            discovery::trigger_record_reconciliation(&client, &namespace, &spec.zone_name).await
        {
            warn!(
                "Failed to trigger record reconciliation for zone {}: {}",
                spec.zone_name, e
            );
            // Don't fail the entire reconciliation for this - records will eventually reconcile
        }
    }

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
/// * `Ok(usize)` - Number of primary endpoints successfully configured
/// * `Err(_)` - If zone addition failed
///
/// # Errors
///
/// Returns an error if BIND9 zone addition fails or if no instances are assigned.
///
/// # Panics
///
/// Panics if the RNDC key is not loaded by the helper function (should never happen in practice).
#[allow(clippy::too_many_lines)]
pub async fn add_dnszone(
    ctx: Arc<crate::context::Context>,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<usize> {
    let client = ctx.client.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    info!("Adding DNSZone {}/{}", namespace, name);

    // PHASE 2 OPTIMIZATION: Use the filtered instance list passed by the caller
    // This ensures we only process instances that need reconciliation (lastReconciledAt == None)

    info!(
        "DNSZone {}/{} will be added to {} instance(s): {:?}",
        namespace,
        name,
        instance_refs.len(),
        instance_refs
            .iter()
            .map(|i| format!("{}/{}", i.namespace, i.name))
            .collect::<Vec<_>>()
    );

    // Filter to only PRIMARY instances
    let primary_instance_refs = primary::filter_primary_instances(&client, instance_refs).await?;

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

    info!(
        "Found {} PRIMARY instance(s) for DNSZone {}/{}",
        primary_instance_refs.len(),
        namespace,
        name
    );

    // Find all secondary instances for zone transfer configuration
    let secondary_instance_refs =
        secondary::filter_secondary_instances(&client, instance_refs).await?;
    let secondary_ips =
        secondary::find_secondary_pod_ips_from_instances(&client, &secondary_instance_refs).await?;

    if secondary_ips.is_empty() {
        warn!(
            "No secondary servers found for DNSZone {}/{} - zone transfers will not be configured",
            namespace, name
        );
    } else {
        info!(
            "Found {} secondary server(s) for DNSZone {}/{} - zone transfers will be configured: {:?}",
            secondary_ips.len(),
            namespace,
            name,
            secondary_ips
        );
    }

    // Generate nameserver IPs if not explicitly provided
    // Nameservers are generated from ALL instances (primaries first, then secondaries)
    let name_server_ips = if spec.name_server_ips.is_none() {
        info!(
            "DNSZone {}/{} has no explicit nameServerIps - auto-generating from {} instance(s)",
            namespace,
            name,
            instance_refs.len()
        );

        // Build ordered list: primaries first, then secondaries
        let mut ordered_instances = primary_instance_refs.clone();
        ordered_instances.extend(secondary_instance_refs.clone());

        match generate_nameserver_ips(&client, &spec.zone_name, &ordered_instances).await {
            Ok(Some(generated_ips)) => {
                info!(
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
        info!(
            "Using explicit nameServerIps for DNSZone {}/{}",
            namespace, name
        );
        spec.name_server_ips.clone()
    };

    // Process all primary instances concurrently using async streams
    // Mark each instance as reconciled immediately after first successful endpoint configuration
    let first_endpoint = Arc::new(Mutex::new(None::<String>));
    let total_endpoints = Arc::new(Mutex::new(0_usize));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let status_updater_shared = Arc::new(Mutex::new(status_updater));

    // Create a stream of futures for all instances
    let _instance_results = stream::iter(primary_instance_refs.iter())
        .then(|instance_ref| {
            let client = client.clone();
            let zone_manager = zone_manager.clone();
            let zone_name = spec.zone_name.clone();
            let soa_record = spec.soa_record.clone();
            let name_server_ips = name_server_ips.clone();
            let secondary_ips = secondary_ips.clone();
            let first_endpoint = Arc::clone(&first_endpoint);
            let total_endpoints = Arc::clone(&total_endpoints);
            let errors = Arc::clone(&errors);
            let status_updater_shared = Arc::clone(&status_updater_shared);
            let instance_ref = instance_ref.clone();
            let _zone_namespace = namespace.clone();
            let _zone_name_ref = name.clone();

            async move {
                info!(
                    "Processing endpoints for primary instance {}/{}",
                    instance_ref.namespace, instance_ref.name
                );

                // Load RNDC key for this specific instance
                let key_data = match helpers::load_rndc_key(&client, &instance_ref.namespace, &instance_ref.name).await {
                    Ok(key) => key,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to load RNDC key: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return;
                    }
                };

                // Get all endpoints for this instance
                let endpoints = match helpers::get_endpoint(&client, &instance_ref.namespace, &instance_ref.name, "http").await {
                    Ok(eps) => eps,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to get endpoints: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return;
                    }
                };

                info!(
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
                        let name_server_ips = name_server_ips.clone();
                        let secondary_ips = secondary_ips.clone();
                        let first_endpoint = Arc::clone(&first_endpoint);
                        let total_endpoints = Arc::clone(&total_endpoints);
                        let errors = Arc::clone(&errors);
                        let instance_ref = instance_ref.clone();
                        let endpoint = endpoint.clone();

                        async move {
                            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

                            // Save the first endpoint (globally)
                            {
                                let mut first = first_endpoint.lock().await;
                                if first.is_none() {
                                    *first = Some(pod_endpoint.clone());
                                }
                            }

                            // Pass secondary IPs for zone transfer configuration
                            let secondary_ips_ref = if secondary_ips.is_empty() {
                                None
                            } else {
                                Some(secondary_ips.as_slice())
                            };

                            match zone_manager
                                .add_zones(
                                    &zone_name,
                                    ZONE_TYPE_PRIMARY,
                                    &pod_endpoint,
                                    &key_data,
                                    Some(&soa_record),
                                    name_server_ips.as_ref(),
                                    secondary_ips_ref,
                                    None, // primary_ips only for secondary zones
                                )
                                .await
                            {
                                Ok(was_added) => {
                                    if was_added {
                                        info!(
                                            "Successfully added zone {} to endpoint {} (instance: {}/{})",
                                            zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name
                                        );
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

                // Mark this instance as configured ONLY if at least one endpoint actually added the zone
                // This prevents updating lastReconciledAt when zone already exists (avoids tight loop)
                let zone_was_configured = endpoint_results.iter().any(|r| r.is_ok() && *r.as_ref().unwrap());
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
                    info!(
                        "Marked primary instance {}/{} as configured for zone {}",
                        instance_ref.namespace, instance_ref.name, zone_name
                    );

                    // PHASE 2 COMPLETION: Update Bind9Instance.status.selectedZones[].lastReconciledAt
                    // This signals successful zone configuration and prevents infinite reconciliation loops
                    // STUB: No longer needed - function is a no-op
                    // update_zone_reconciled_timestamp(
                    //     &client,
                    //     &instance_ref.name,
                    //     &instance_ref.namespace,
                    //     &zone_name_ref,
                    //     &zone_namespace,
                    // );
                }
            }
        })
        .collect::<Vec<()>>()
        .await;

    let first_endpoint = Arc::try_unwrap(first_endpoint)
        .expect("Failed to unwrap first_endpoint Arc")
        .into_inner();
    let total_endpoints = Arc::try_unwrap(total_endpoints)
        .expect("Failed to unwrap total_endpoints Arc")
        .into_inner();
    let errors = Arc::try_unwrap(errors)
        .expect("Failed to unwrap errors Arc")
        .into_inner();
    let _status_updater = Arc::try_unwrap(status_updater_shared)
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

    info!(
        "Successfully added zone {} to {} endpoint(s) across {} primary instance(s)",
        spec.zone_name,
        total_endpoints,
        primary_instance_refs.len()
    );

    // Note: We don't need to reload after addzone because:
    // 1. rndc addzone immediately adds the zone to BIND9's running config
    // 2. The zone file will be created automatically when records are added via dynamic updates
    // 3. Reloading would fail if the zone file doesn't exist yet

    // Notify secondaries about the new zone via the first endpoint
    // This triggers zone transfer (AXFR) from primary to secondaries
    if let Some(first_pod_endpoint) = first_endpoint {
        info!("Notifying secondaries about new zone {}", spec.zone_name);
        if let Err(e) = zone_manager
            .notify_zone(&spec.zone_name, &first_pod_endpoint)
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

    Ok(total_endpoints)
}

/// Adds a DNS zone to all secondary instances in the cluster with primaries configured.
///
/// Creates secondary zones on all secondary instances, configuring them to transfer
/// from the provided primary server IPs. If a zone already exists on a secondary,
/// it checks if the primaries list matches and updates it if necessary.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `dnszone` - The `DNSZone` resource
/// * `zone_manager` - BIND9 manager for adding zone
/// * `primary_ips` - List of primary server IPs to configure in the primaries field
///
/// # Returns
///
/// * `Ok(usize)` - Number of secondary endpoints successfully configured
/// * `Err(_)` - If zone addition failed
///
/// # Errors
///
/// Returns an error if BIND9 zone addition fails on any secondary instance.
///
/// # Panics
///
/// Panics if internal Arc unwrapping fails (should not happen in normal operation).
#[allow(clippy::too_many_lines)]
pub async fn add_dnszone_to_secondaries(
    ctx: Arc<crate::context::Context>,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
    primary_ips: &[String],
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<usize> {
    let client = ctx.client.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    if primary_ips.is_empty() {
        warn!(
            "No primary IPs provided for secondary zone {}/{} - skipping secondary configuration",
            namespace, spec.zone_name
        );
        return Ok(0);
    }

    info!(
        "Adding DNSZone {}/{} to secondary instances with primaries: {:?}",
        namespace, name, primary_ips
    );

    // PHASE 2 OPTIMIZATION: Use the filtered instance list passed by the caller
    // This ensures we only process instances that need reconciliation (lastReconciledAt == None)

    // Filter to only SECONDARY instances
    let secondary_instance_refs =
        secondary::filter_secondary_instances(&client, instance_refs).await?;

    if secondary_instance_refs.is_empty() {
        info!(
            "No secondary instances found for DNSZone {}/{} - skipping secondary zone configuration",
            namespace, name
        );
        return Ok(0);
    }

    info!(
        "Found {} secondary instance(s) for DNSZone {}/{}",
        secondary_instance_refs.len(),
        namespace,
        name
    );

    // Process all secondary instances concurrently using async streams
    // Mark each instance as reconciled immediately after first successful endpoint configuration
    let total_endpoints = Arc::new(Mutex::new(0_usize));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let status_updater_shared = Arc::new(Mutex::new(status_updater));

    // Create a stream of futures for all secondary instances
    let _instance_results = stream::iter(secondary_instance_refs.iter())
        .then(|instance_ref| {
            let client = client.clone();
            let zone_manager = zone_manager.clone();
            let zone_name = spec.zone_name.clone();
            let primary_ips = primary_ips.to_vec();
            let total_endpoints = Arc::clone(&total_endpoints);
            let errors = Arc::clone(&errors);
            let status_updater_shared = Arc::clone(&status_updater_shared);
            let instance_ref = instance_ref.clone();
            let _zone_namespace = namespace.clone();
            let _zone_name_ref = name.clone();

            async move {
                info!(
                    "Processing secondary instance {}/{} for zone {}",
                    instance_ref.namespace, instance_ref.name, zone_name
                );

                // Load RNDC key for this specific instance
                // Each instance has its own RNDC secret for security isolation
                let key_data = match helpers::load_rndc_key(&client, &instance_ref.namespace, &instance_ref.name).await {
                    Ok(key) => key,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to load RNDC key: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return;
                    }
                };

                // Get all endpoints for this secondary instance
                let endpoints = match helpers::get_endpoint(&client, &instance_ref.namespace, &instance_ref.name, "http").await {
                    Ok(eps) => eps,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to get endpoints: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return;
                    }
                };

                info!(
                    "Found {} endpoint(s) for secondary instance {}/{}",
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
                        let primary_ips = primary_ips.clone();
                        let total_endpoints = Arc::clone(&total_endpoints);
                        let errors = Arc::clone(&errors);
                        let instance_ref = instance_ref.clone();
                        let endpoint = endpoint.clone();

                        async move {
                            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

                            info!(
                                "Adding secondary zone {} to endpoint {} (instance: {}/{}) with primaries: {:?}",
                                zone_name,
                                pod_endpoint,
                                instance_ref.namespace,
                                instance_ref.name,
                                primary_ips
                            );

                            match zone_manager
                                .add_zones(
                                    &zone_name,
                                    ZONE_TYPE_SECONDARY,
                                    &pod_endpoint,
                                    &key_data,
                                    None, // No SOA record for secondary zones
                                    None, // No name_server_ips for secondary zones
                                    None, // No secondary_ips for secondary zones
                                    Some(&primary_ips),
                                )
                                .await
                            {
                                Ok(was_added) => {
                                    if was_added {
                                        info!(
                                            "Successfully added secondary zone {} to endpoint {} (instance: {}/{})",
                                            zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name
                                        );
                                    } else {
                                        info!(
                                            "Secondary zone {} already exists on endpoint {} (instance: {}/{})",
                                            zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name
                                        );
                                    }

                                    // CRITICAL: Immediately trigger zone transfer to load the zone data
                                    // This is necessary because:
                                    // 1. `rndc addzone` only adds the zone to BIND9's config (in-memory)
                                    // 2. The zone file doesn't exist yet on the secondary
                                    // 3. Queries will return SERVFAIL until data is transferred from primary
                                    // 4. `rndc retransfer` forces an immediate AXFR from primary to secondary
                                    //
                                    // This ensures the zone is LOADED and SERVING queries immediately after
                                    // secondary pod restart or zone creation.
                                    info!(
                                        "Triggering immediate zone transfer for {} on secondary {} to load zone data",
                                        zone_name, pod_endpoint
                                    );
                                    if let Err(e) = zone_manager
                                        .retransfer_zone(&zone_name, &pod_endpoint)
                                        .await
                                    {
                                        // Don't fail reconciliation if retransfer fails - zone will sync via SOA refresh
                                        warn!(
                                            "Failed to trigger immediate zone transfer for {} on {}: {}. Zone will sync via SOA refresh timer.",
                                            zone_name, pod_endpoint, e
                                        );
                                    } else {
                                        info!(
                                            "Successfully triggered zone transfer for {} on {}",
                                            zone_name, pod_endpoint
                                        );
                                    }

                                    *total_endpoints.lock().await += 1;
                                    // Return was_added so we can check if zone was actually configured
                                    Ok(was_added)
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to add secondary zone {} to endpoint {} (instance {}/{}): {}",
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

                // Mark this instance as configured ONLY if at least one endpoint actually added the zone
                // This prevents updating lastReconciledAt when zone already exists (avoids tight loop)
                let zone_was_configured = endpoint_results.iter().any(|r| r.is_ok() && *r.as_ref().unwrap());
                if zone_was_configured {
                    status_updater_shared
                        .lock()
                        .await
                        .update_instance_status(
                            &instance_ref.name,
                            &instance_ref.namespace,
                            crate::crd::InstanceStatus::Configured,
                            Some("Zone successfully configured on secondary instance".to_string()),
                        );
                    info!(
                        "Marked secondary instance {}/{} as configured for zone {}",
                        instance_ref.namespace, instance_ref.name, zone_name
                    );

                    // PHASE 2 COMPLETION: Update Bind9Instance.status.selectedZones[].lastReconciledAt
                    // This signals successful zone configuration and prevents infinite reconciliation loops
                    // STUB: No longer needed - function is a no-op
                    // update_zone_reconciled_timestamp(
                    //     &client,
                    //     &instance_ref.name,
                    //     &instance_ref.namespace,
                    //     &zone_name_ref,
                    //     &zone_namespace,
                    // );
                }
            }
        })
        .collect::<Vec<()>>()
        .await;

    let total_endpoints = Arc::try_unwrap(total_endpoints).unwrap().into_inner();
    let errors = Arc::try_unwrap(errors).unwrap().into_inner();

    // If ALL operations failed, return an error
    if total_endpoints == 0 && !errors.is_empty() {
        return Err(anyhow!(
            "Failed to add zone {} to all secondary instances. Errors: {}",
            spec.zone_name,
            errors.join("; ")
        ));
    }

    info!(
        "Successfully configured secondary zone {} on {} endpoint(s) across {} secondary instance(s)",
        spec.zone_name,
        total_endpoints,
        secondary_instance_refs.len()
    );

    Ok(total_endpoints)
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
pub async fn delete_dnszone(
    ctx: Arc<crate::context::Context>,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
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

    // Filter to primary and secondary instances
    let primary_instance_refs = primary::filter_primary_instances(&client, &instance_refs).await?;
    let secondary_instance_refs =
        secondary::filter_secondary_instances(&client, &instance_refs).await?;

    // Delete from all primary instances
    if !primary_instance_refs.is_empty() {
        let (_first_endpoint, total_endpoints) = helpers::for_each_instance_endpoint(
            &client,
            &primary_instance_refs,
            false, // with_rndc_key = false for zone deletion
            "http", // Use HTTP API port for zone deletion via bindcar API
            |pod_endpoint, instance_name, _rndc_key| {
                let zone_name = spec.zone_name.clone();
                let zone_manager = zone_manager.clone();

                async move {
                    info!(
                        "Deleting zone {} from endpoint {} (instance: {})",
                        zone_name, pod_endpoint, instance_name
                    );

                    // Attempt to delete zone - if it fails (zone not found, endpoint unreachable, etc.),
                    // log a warning but don't fail the deletion. This ensures DNSZones can be deleted
                    // even if BIND9 instances are unavailable or the zone was already removed.
                    // Pass freeze_before_delete=true for primary zones to prevent updates during deletion
                    if let Err(e) = zone_manager.delete_zone(&zone_name, &pod_endpoint, true).await {
                        warn!(
                            "Failed to delete zone {} from endpoint {} (instance: {}): {}. Continuing with deletion anyway.",
                            zone_name, pod_endpoint, instance_name, e
                        );
                    } else {
                        debug!(
                            "Successfully deleted zone {} from endpoint {} (instance: {})",
                            zone_name, pod_endpoint, instance_name
                        );
                    }

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

    // Delete from all secondary instances
    if !secondary_instance_refs.is_empty() {
        let mut secondary_endpoints_deleted = 0;

        for instance_ref in &secondary_instance_refs {
            let endpoints =
                helpers::get_endpoint(&client, &instance_ref.namespace, &instance_ref.name, "http")
                    .await?;

            for endpoint in &endpoints {
                let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

                info!(
                    "Deleting zone {} from secondary endpoint {} (instance: {}/{})",
                    spec.zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name
                );

                // Attempt to delete zone - if it fails, log a warning but don't fail the deletion
                // Pass freeze_before_delete=false for secondary zones (they are read-only, no need to freeze)
                if let Err(e) = zone_manager
                    .delete_zone(&spec.zone_name, &pod_endpoint, false)
                    .await
                {
                    warn!(
                        "Failed to delete zone {} from secondary endpoint {} (instance: {}/{}): {}. Continuing with deletion anyway.",
                        spec.zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name, e
                    );
                } else {
                    debug!(
                        "Successfully deleted zone {} from secondary endpoint {} (instance: {}/{})",
                        spec.zone_name, pod_endpoint, instance_ref.namespace, instance_ref.name
                    );
                    secondary_endpoints_deleted += 1;
                }
            }
        }

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
