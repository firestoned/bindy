// Copyright (c) 2025 Erick Bourgeois, firestoned
#![allow(dead_code)]
// SPDX-License-Identifier: MIT

//! DNS zone reconciliation logic.
//!
//! This module handles the creation and management of DNS zones on BIND9 servers.
//! It supports both primary and secondary zone configurations.

use crate::bind9::RndcKeyData;
use crate::constants::{ANNOTATION_ZONE_OWNER, ANNOTATION_ZONE_PREVIOUS_OWNER};
use crate::crd::{Condition, DNSZone, DNSZoneSpec, DNSZoneStatus};
use anyhow::{anyhow, Context, Result};
use bindcar::{ZONE_TYPE_PRIMARY, ZONE_TYPE_SECONDARY};
use chrono::Utc;
use k8s_openapi::api::core::v1::{Endpoints, Pod, Secret};
use kube::{
    api::{ListParams, Patch, PatchParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use std::collections::HashSet;
use tracing::{debug, error, info, warn};

/// Helper function to extract and validate cluster reference from `DNSZoneSpec`.
///
/// Returns the cluster name, whether from clusterRef or clusterProviderRef.
/// Validates that exactly one is specified (mutual exclusivity).
///
/// This function is public so it can be used by other reconcilers (e.g., records reconciler).
///
/// # Errors
///
/// Returns an error if:
/// - Both `clusterRef` and `clusterProviderRef` are specified (mutual exclusivity violation)
/// - Neither `clusterRef` nor `clusterProviderRef` is specified (at least one required)
pub fn get_cluster_ref_from_spec(
    spec: &DNSZoneSpec,
    namespace: &str,
    name: &str,
) -> Result<String> {
    match (&spec.cluster_ref, &spec.cluster_provider_ref) {
        (Some(ref cluster_name), None) => Ok(cluster_name.clone()),
        (None, Some(ref cluster_provider_name)) => Ok(cluster_provider_name.clone()),
        (Some(_), Some(_)) => Err(anyhow!(
            "DNSZone {namespace}/{name} has both clusterRef and clusterProviderRef specified. \
            Only one must be specified."
        )),
        (None, None) => Err(anyhow!(
            "DNSZone {namespace}/{name} has neither clusterRef nor clusterProviderRef specified. \
            Exactly one must be specified."
        )),
    }
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
/// ```rust,no_run
/// use bindy::reconcilers::reconcile_dnszone;
/// use bindy::crd::DNSZone;
/// use bindy::bind9::Bind9Manager;
/// use kube::Client;
///
/// async fn handle_zone(zone: DNSZone) -> anyhow::Result<()> {
///     let client = Client::try_default().await?;
///     let manager = Bind9Manager::new();
///     reconcile_dnszone(client, zone, &manager).await?;
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or BIND9 zone operations fail.
#[allow(clippy::too_many_lines)]
pub async fn reconcile_dnszone(
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();

    info!("Reconciling DNSZone: {}/{}", namespace, name);
    debug!(
        namespace = %namespace,
        name = %name,
        generation = ?dnszone.metadata.generation,
        "Starting DNSZone reconciliation"
    );

    // Create centralized status updater to batch all status changes
    let mut status_updater = crate::reconcilers::status::DNSZoneStatusUpdater::new(&dnszone);

    // Extract spec
    let spec = &dnszone.spec;

    // Guard clause: Validate exactly one cluster reference is provided
    let cluster_ref = get_cluster_ref_from_spec(spec, &namespace, &name)?;
    let is_cluster_provider = spec.cluster_provider_ref.is_some();

    info!(
        "DNSZone {}/{} references cluster '{}' (is_cluster_provider={}, cluster_ref={:?}, cluster_provider_ref={:?})",
        namespace, name, cluster_ref, is_cluster_provider, spec.cluster_ref, spec.cluster_provider_ref
    );

    // Determine if this is the first reconciliation or if spec has changed
    let current_generation = dnszone.metadata.generation;
    let observed_generation = dnszone.status.as_ref().and_then(|s| s.observed_generation);

    let first_reconciliation = observed_generation.is_none();
    let spec_changed =
        crate::reconcilers::should_reconcile(current_generation, observed_generation);

    info!(
        "Reconciling zone {} (first_reconciliation={}, spec_changed={})",
        spec.zone_name, first_reconciliation, spec_changed
    );

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
    let (primary_count, secondary_count) = {
        debug!("Ensuring BIND9 zone exists on all instances (declarative reconciliation)");

        // Set initial Progressing status (in-memory)
        status_updater.set_condition(
            "Progressing",
            "True",
            "PrimaryReconciling",
            "Configuring zone on primary servers",
        );

        // Get current primary IPs for secondary zone configuration
        let primary_ips =
            match find_all_primary_pod_ips(&client, &namespace, &cluster_ref, is_cluster_provider)
                .await
            {
                Ok(ips) if !ips.is_empty() => {
                    info!(
                        "Found {} primary server(s) for cluster {}: {:?}",
                        ips.len(),
                        cluster_ref,
                        ips
                    );
                    ips
                }
                Ok(_) => {
                    status_updater.set_condition(
                        "Degraded",
                        "True",
                        "PrimaryFailed",
                        &format!("No primary servers found for cluster {cluster_ref}"),
                    );
                    // Apply status before returning error
                    status_updater.apply(&client).await?;
                    return Err(anyhow!(
                    "No primary servers found for cluster {cluster_ref} - cannot configure zones"
                ));
                }
                Err(e) => {
                    status_updater.set_condition(
                        "Degraded",
                        "True",
                        "PrimaryFailed",
                        &format!("Failed to find primary servers: {e}"),
                    );
                    // Apply status before returning error
                    status_updater.apply(&client).await?;
                    return Err(e);
                }
            };

        // Add/update zone on all primary instances
        let primary_count = match add_dnszone(client.clone(), dnszone.clone(), zone_manager).await {
            Ok(count) => {
                // Update status after successful primary reconciliation (in-memory)
                status_updater.set_condition(
                    "Progressing",
                    "True",
                    "PrimaryReconciled",
                    &format!(
                        "Zone {} configured on {} primary server(s)",
                        spec.zone_name, count
                    ),
                );
                count
            }
            Err(e) => {
                status_updater.set_condition(
                    "Degraded",
                    "True",
                    "PrimaryFailed",
                    &format!("Failed to configure zone on primary servers: {e}"),
                );
                // Apply status before returning error
                status_updater.apply(&client).await?;
                return Err(e);
            }
        };

        // Update to secondary reconciliation phase (in-memory)
        status_updater.set_condition(
            "Progressing",
            "True",
            "SecondaryReconciling",
            "Configuring zone on secondary servers",
        );

        // Add/update zone on all secondary instances with primaries configured
        let secondary_count = match add_dnszone_to_secondaries(
            client.clone(),
            dnszone.clone(),
            zone_manager,
            &primary_ips,
        )
        .await
        {
            Ok(count) => {
                // Update status after successful secondary reconciliation (in-memory)
                if count > 0 {
                    status_updater.set_condition(
                        "Progressing",
                        "True",
                        "SecondaryReconciled",
                        &format!(
                            "Zone {} configured on {} secondary server(s)",
                            spec.zone_name, count
                        ),
                    );
                }
                count
            }
            Err(e) => {
                // Secondary failure is non-fatal - primaries still work
                warn!(
                    "Failed to configure zone on secondary servers: {}. Primary servers are still operational.",
                    e
                );
                status_updater.set_condition(
                    "Degraded",
                    "True",
                    "SecondaryFailed",
                    &format!(
                        "Zone configured on {primary_count} primary server(s) but secondary configuration failed: {e}"
                    ),
                );
                0
            }
        };

        (primary_count, secondary_count)
    };

    // Discover DNS records that match the zone's label selectors (in-memory status update)
    status_updater.set_condition(
        "Progressing",
        "True",
        "RecordsDiscovering",
        "Discovering DNS records via label selectors",
    );

    let record_refs = match reconcile_zone_records(client.clone(), dnszone.clone()).await {
        Ok(refs) => {
            info!(
                "Discovered {} DNS record(s) for zone {} via label selectors",
                refs.len(),
                spec.zone_name
            );
            refs
        }
        Err(e) => {
            // Record discovery failure is non-fatal - the zone itself is still configured
            warn!(
                "Failed to discover DNS records for zone {}: {}. Zone is configured but record discovery failed.",
                spec.zone_name, e
            );
            status_updater.set_condition(
                "Degraded",
                "True",
                "RecordDiscoveryFailed",
                &format!("Zone configured but record discovery failed: {e}"),
            );
            vec![]
        }
    };

    let record_count = record_refs.len();

    // Update DNSZone status with discovered records (in-memory)
    status_updater.set_records(record_refs.clone());

    // Check if all discovered records are ready and trigger zone transfers if needed
    if record_count > 0 {
        let all_records_ready = check_all_records_ready(&client, &namespace, &record_refs).await?;

        if all_records_ready {
            info!(
                "All {} record(s) for zone {} are ready, triggering zone transfers to secondaries",
                record_count, spec.zone_name
            );

            // Trigger zone transfers to all secondaries
            match trigger_zone_transfers(
                &client,
                &namespace,
                &spec.zone_name,
                &cluster_ref,
                is_cluster_provider,
                zone_manager,
            )
            .await
            {
                Ok(transfer_count) => {
                    info!(
                        "Successfully triggered zone transfer for {} to {} secondary instance(s)",
                        spec.zone_name, transfer_count
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to trigger zone transfers for {}: {}. Zone is configured but secondaries may be out of sync.",
                        spec.zone_name, e
                    );
                    status_updater.set_condition(
                        "Degraded",
                        "True",
                        "TransferFailed",
                        &format!("Zone configured but zone transfer failed: {e}"),
                    );
                }
            }
        } else {
            info!(
                "Not all records for zone {} are ready yet, skipping zone transfer",
                spec.zone_name
            );
        }
    }

    // Re-fetch secondary IPs to store in status (in-memory)
    let secondary_ips =
        find_all_secondary_pod_ips(&client, &namespace, &cluster_ref, is_cluster_provider)
            .await
            .unwrap_or_default();
    status_updater.set_secondary_ips(secondary_ips);

    // Set observed generation (in-memory)
    status_updater.set_observed_generation(current_generation);

    // Set final Ready/Degraded status based on reconciliation outcome
    // Only set Ready=True if there were NO degraded conditions during reconciliation
    if status_updater.has_degraded_condition() {
        // Keep the Degraded condition that was already set, don't overwrite with Ready
        info!(
            "DNSZone {}/{} reconciliation completed with degraded state - will retry faster",
            namespace, name
        );
    } else {
        // All reconciliation steps succeeded - set Ready status and clear any stale Degraded condition
        status_updater.set_condition(
            "Ready",
            "True",
            "ReconcileSucceeded",
            &format!(
                "Zone {} configured on {} primary and {} secondary server(s), discovered {} DNS record(s) for cluster {}",
                spec.zone_name, primary_count, secondary_count, record_count, cluster_ref
            ),
        );
        // Clear any stale Degraded condition from previous failures
        status_updater.clear_degraded_condition();
    }

    // Apply all status changes in a single atomic operation
    status_updater.apply(&client).await?;

    // Trigger record reconciliation: Update all matching records with a "zone-reconciled" annotation
    // This ensures records are re-added to BIND9 after pod restarts or zone recreation
    if !status_updater.has_degraded_condition() {
        if let Err(e) = trigger_record_reconciliation(&client, &namespace, &spec.zone_name).await {
            warn!(
                "Failed to trigger record reconciliation for zone {}: {}",
                spec.zone_name, e
            );
            // Don't fail the entire reconciliation for this - records will eventually reconcile
        }
    }

    Ok(())
}

/// Adds a DNS zone to all primary instances in the cluster.
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
/// Returns an error if BIND9 zone addition fails.
///
/// # Panics
///
/// Panics if the RNDC key is not loaded by the helper function (should never happen in practice).
pub async fn add_dnszone(
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<usize> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    // Extract and validate cluster reference
    let cluster_ref = get_cluster_ref_from_spec(spec, &namespace, &name)?;
    let is_cluster_provider = spec.cluster_provider_ref.is_some();

    info!("Adding DNSZone: {}", name);

    // Find secondary pod IPs for zone transfer configuration
    let secondary_ips =
        find_all_secondary_pod_ips(&client, &namespace, &cluster_ref, is_cluster_provider).await?;

    if secondary_ips.is_empty() {
        warn!(
            "No secondary servers found for cluster {} - zone transfers will not be configured",
            cluster_ref
        );
    } else {
        info!(
            "Found {} secondary server(s) for cluster {} - zone transfers will be configured: {:?}",
            secondary_ips.len(),
            cluster_ref,
            secondary_ips
        );
    }

    // Use the common helper to iterate through all endpoints
    // Load RNDC key (true) since zone addition requires it
    // Use "http" port for HTTP API operations
    let (first_endpoint, total_endpoints) = for_each_primary_endpoint(
        &client,
        &namespace,
        &cluster_ref,
        is_cluster_provider,
        true, // with_rndc_key = true for zone addition
        "http", // Use HTTP API port for zone addition via bindcar API
        |pod_endpoint, instance_name, rndc_key| {
            let zone_name = spec.zone_name.clone();
            let soa_record = spec.soa_record.clone();
            let name_server_ips = spec.name_server_ips.clone();
            let zone_manager = zone_manager.clone();
            let secondary_ips_clone = secondary_ips.clone();

            async move {
                // SAFETY: RNDC key is guaranteed to be Some when with_rndc_key=true
                // The for_each_primary_endpoint helper loads the key when with_rndc_key=true
                let key_data = rndc_key.expect("RNDC key should be loaded when with_rndc_key=true");

                // Pass secondary IPs for zone transfer configuration
                let secondary_ips_ref = if secondary_ips_clone.is_empty() {
                    None
                } else {
                    Some(secondary_ips_clone.as_slice())
                };

                let was_added = zone_manager
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
                    .context(format!(
                        "Failed to add zone {zone_name} to endpoint {pod_endpoint} (instance: {instance_name})"
                    ))?;

                if was_added {
                    info!(
                        "Successfully added zone {} to endpoint {} (instance: {})",
                        zone_name, pod_endpoint, instance_name
                    );
                }

                Ok(())
            }
        },
    )
    .await?;

    info!(
        "Successfully added zone {} to {} endpoint(s) for cluster {}",
        spec.zone_name, total_endpoints, cluster_ref
    );

    // Note: We don't need to reload after addzone because:
    // 1. rndc addzone immediately adds the zone to BIND9's running config
    // 2. The zone file will be created automatically when records are added via dynamic updates
    // 3. Reloading would fail if the zone file doesn't exist yet

    // Notify secondaries about the new zone via the first endpoint
    // This triggers zone transfer (AXFR) from primary to secondaries
    if let Some(first_pod_endpoint) = first_endpoint {
        info!(
            "Notifying secondaries about new zone {} for cluster {}",
            spec.zone_name, cluster_ref
        );
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
#[allow(clippy::too_many_lines)]
pub async fn add_dnszone_to_secondaries(
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
    primary_ips: &[String],
) -> Result<usize> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    // Extract and validate cluster reference
    let cluster_ref = get_cluster_ref_from_spec(spec, &namespace, &name)?;
    let is_cluster_provider = spec.cluster_provider_ref.is_some();

    if primary_ips.is_empty() {
        warn!(
            "No primary IPs provided for secondary zone {} - skipping secondary configuration",
            spec.zone_name
        );
        return Ok(0);
    }

    info!(
        "Adding DNSZone {} to secondary instances with primaries: {:?}",
        name, primary_ips
    );

    // Find all secondary pods
    let secondary_pods =
        find_all_secondary_pods(&client, &namespace, &cluster_ref, is_cluster_provider).await?;

    if secondary_pods.is_empty() {
        info!(
            "No secondary servers found for cluster {} - skipping secondary zone configuration",
            cluster_ref
        );
        return Ok(0);
    }

    info!(
        "Found {} secondary pod(s) for cluster {}",
        secondary_pods.len(),
        cluster_ref
    );

    // Get unique (instance_name, namespace) tuples from secondary pods
    let mut instance_tuples: Vec<(String, String)> = secondary_pods
        .iter()
        .map(|pod| (pod.instance_name.clone(), pod.namespace.clone()))
        .collect();
    instance_tuples.sort();
    instance_tuples.dedup();

    if instance_tuples.is_empty() {
        return Err(anyhow!(
            "No secondary instances found for cluster {cluster_ref}"
        ));
    }

    let mut total_endpoints = 0;

    // Iterate through each secondary instance and add zone to all its endpoints
    for (instance_name, instance_namespace) in &instance_tuples {
        info!(
            "Processing secondary instance {}/{} for zone {}",
            instance_namespace, instance_name, spec.zone_name
        );

        // Load RNDC key for this specific instance
        // Each instance has its own RNDC secret for security isolation
        let key_data = load_rndc_key(&client, instance_namespace, instance_name).await?;

        // Get all endpoints for this secondary instance
        let endpoints = get_endpoint(&client, instance_namespace, instance_name, "http").await?;

        info!(
            "Found {} endpoint(s) for secondary instance {}",
            endpoints.len(),
            instance_name
        );

        for endpoint in &endpoints {
            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

            info!(
                "Adding secondary zone {} to endpoint {} (instance: {}) with primaries: {:?}",
                spec.zone_name, pod_endpoint, instance_name, primary_ips
            );

            let was_added = zone_manager
                .add_zones(
                    &spec.zone_name,
                    ZONE_TYPE_SECONDARY,
                    &pod_endpoint,
                    &key_data,
                    None, // No SOA record for secondary zones
                    None, // No name_server_ips for secondary zones
                    None, // No secondary_ips for secondary zones
                    Some(primary_ips),
                )
                .await
                .context(format!(
                    "Failed to add secondary zone {} to endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
                ))?;

            if was_added {
                info!(
                    "Successfully added secondary zone {} to endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
                );
            } else {
                info!(
                    "Secondary zone {} already exists on endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
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
                spec.zone_name, pod_endpoint
            );
            if let Err(e) = zone_manager
                .retransfer_zone(&spec.zone_name, &pod_endpoint)
                .await
            {
                // Don't fail reconciliation if retransfer fails - zone will sync via SOA refresh
                warn!(
                    "Failed to trigger immediate zone transfer for {} on {}: {}. Zone will sync via SOA refresh timer.",
                    spec.zone_name, pod_endpoint, e
                );
            } else {
                info!(
                    "Successfully triggered zone transfer for {} on {}",
                    spec.zone_name, pod_endpoint
                );
            }

            total_endpoints += 1;
        }
    }

    info!(
        "Successfully configured secondary zone {} on {} endpoint(s) across {} instance(s) for cluster {}",
        spec.zone_name,
        total_endpoints,
        instance_tuples.len(),
        cluster_ref
    );

    Ok(total_endpoints)
}

/// Reconciles DNS records for a zone by discovering records that match the zone's label selectors.
///
/// This function implements the core of the zone/record ownership model:
/// 1. Discovers records matching the zone's label selectors
/// 2. Tags matched records with zone ownership annotation and status.zone field
/// 3. Untags previously matched records that no longer match
/// 4. Returns references to currently matched records for status tracking
///
/// Record reconcilers use the `bindy.firestoned.io/zone` annotation to determine
/// which zone to update in BIND9. When a record loses the annotation, the record
/// reconciler will delete it from BIND9.
///
/// # Arguments
///
/// * `client` - Kubernetes API client for querying DNS records
/// * `dnszone` - The `DNSZone` resource with label selectors
///
/// # Returns
///
/// * `Ok(Vec<RecordReference>)` - List of currently matched DNS records
/// * `Err(_)` - If record discovery or tagging fails
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail.
#[allow(clippy::too_many_lines)]
async fn reconcile_zone_records(
    client: Client,
    dnszone: DNSZone,
) -> Result<Vec<crate::crd::RecordReference>> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let spec = &dnszone.spec;
    let zone_name = &spec.zone_name;

    // Early return if no label selectors are defined
    let Some(ref records_from) = spec.records_from else {
        info!(
            "No label selectors defined for zone {}, skipping record discovery",
            zone_name
        );
        // If no selectors, untag ALL previously matched records
        return Ok(Vec::new());
    };

    info!(
        "Discovering DNS records for zone {} using {} label selector(s)",
        zone_name,
        records_from.len()
    );

    let mut all_record_refs = Vec::new();

    // Query all record types and filter by label selectors
    for record_source in records_from {
        let selector = &record_source.selector;

        // Discover each record type
        all_record_refs.extend(discover_a_records(&client, &namespace, selector).await?);
        all_record_refs.extend(discover_aaaa_records(&client, &namespace, selector).await?);
        all_record_refs.extend(discover_txt_records(&client, &namespace, selector).await?);
        all_record_refs.extend(discover_cname_records(&client, &namespace, selector).await?);
        all_record_refs.extend(discover_mx_records(&client, &namespace, selector).await?);
        all_record_refs.extend(discover_ns_records(&client, &namespace, selector).await?);
        all_record_refs.extend(discover_srv_records(&client, &namespace, selector).await?);
        all_record_refs.extend(discover_caa_records(&client, &namespace, selector).await?);
    }

    info!(
        "Discovered {} DNS record(s) for zone {}",
        all_record_refs.len(),
        zone_name
    );

    // Get previously matched records from current status
    let previous_records: HashSet<String> = dnszone
        .status
        .as_ref()
        .map(|s| {
            s.records
                .iter()
                .map(|r| format!("{}/{}", r.kind, r.name))
                .collect()
        })
        .unwrap_or_default();

    // Create set of currently matched records
    let current_records: HashSet<String> = all_record_refs
        .iter()
        .map(|r| format!("{}/{}", r.kind, r.name))
        .collect();

    // Tag newly matched records (in current but not in previous)
    for record_ref in &all_record_refs {
        let record_key = format!("{}/{}", record_ref.kind, record_ref.name);
        if !previous_records.contains(&record_key) {
            info!(
                "Newly matched record: {} {}/{}",
                record_ref.kind, namespace, record_ref.name
            );
            tag_record_with_zone(
                &client,
                &namespace,
                &record_ref.kind,
                &record_ref.name,
                zone_name,
            )
            .await?;
        }
    }

    // Untag previously matched records that no longer match or were deleted
    // (in previous but not in current)
    for prev_record_key in &previous_records {
        if !current_records.contains(prev_record_key.as_str()) {
            // Parse kind and name from "Kind/name" format
            if let Some((kind, name)) = prev_record_key.split_once('/') {
                warn!(
                    "Record no longer matches zone {} (unmatched or deleted): {} {}/{}",
                    zone_name, kind, namespace, name
                );

                // Try to untag the record, but don't fail if it was deleted
                // If the record was deleted, the API will return NotFound, which is fine
                if let Err(e) =
                    untag_record_from_zone(&client, &namespace, kind, name, zone_name).await
                {
                    // Check if error is because record was deleted (NotFound)
                    if e.to_string().contains("NotFound") || e.to_string().contains("not found") {
                        info!(
                            "Record {} {}/{} was deleted, removing from zone {} status",
                            kind, namespace, name, zone_name
                        );
                    } else {
                        // Other errors should be logged but not fail the reconciliation
                        warn!(
                            "Failed to untag record {} {}/{} from zone {}: {}",
                            kind, namespace, name, zone_name, e
                        );
                    }
                }
                // Continue regardless - the record will be removed from status.records
                // when we return all_record_refs (which doesn't include this record)
            }
        }
    }

    Ok(all_record_refs)
}

/// Tags a DNS record with zone ownership annotation and updates its `status.zone` field.
///
/// This function is called when a `DNSZone`'s label selector matches a record.
/// It sets both the `bindy.firestoned.io/zone` annotation and the `status.zone` field.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the record
/// * `kind` - Record kind (e.g., `ARecord`, `CNAMERecord`)
/// * `name` - Record name
/// * `zone_fqdn` - Fully qualified domain name of the zone (e.g., `"example.com"`)
///
/// # Returns
///
/// * `Ok(())` - If the record was tagged successfully
/// * `Err(_)` - If tagging failed
async fn tag_record_with_zone(
    client: &Client,
    namespace: &str,
    kind: &str,
    name: &str,
    zone_fqdn: &str,
) -> Result<()> {
    debug!(
        "Tagging {} {}/{} with zone {}",
        kind, namespace, name, zone_fqdn
    );

    // Convert kind to plural resource name (e.g., "ARecord" -> "arecords")
    let plural = format!("{}s", kind.to_lowercase());

    // Create GroupVersionKind for the resource
    let gvk = kube::core::GroupVersionKind {
        group: "bindy.firestoned.io".to_string(),
        version: "v1beta1".to_string(),
        kind: kind.to_string(),
    };

    // Use kube's Discovery API to create ApiResource
    let api_resource = kube::api::ApiResource::from_gvk_with_plural(&gvk, &plural);

    // Create a dynamic API client
    let api = kube::api::Api::<kube::api::DynamicObject>::namespaced_with(
        client.clone(),
        namespace,
        &api_resource,
    );

    // Patch metadata to add annotation
    let annotation_patch = json!({
        "metadata": {
            "annotations": {
                ANNOTATION_ZONE_OWNER: zone_fqdn
            }
        }
    });

    api.patch(
        name,
        &PatchParams::default(),
        &Patch::Merge(&annotation_patch),
    )
    .await
    .with_context(|| format!("Failed to add zone annotation to {kind} {namespace}/{name}"))?;

    // Patch status to set zone field
    let status_patch = json!({
        "status": {
            "zone": zone_fqdn
        }
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .with_context(|| format!("Failed to set status.zone on {kind} {namespace}/{name}"))?;

    info!(
        "Successfully tagged {} {}/{} with zone {}",
        kind, namespace, name, zone_fqdn
    );

    Ok(())
}

/// Untags a DNS record that no longer matches a zone's selector.
///
/// This function removes the zone ownership annotation and `status.zone` field,
/// and optionally sets a `"previous-zone"` annotation for tracking.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the record
/// * `kind` - Record kind (e.g., `ARecord`, `CNAMERecord`)
/// * `name` - Record name
/// * `previous_zone_fqdn` - FQDN of the zone that previously owned this record
///
/// # Returns
///
/// * `Ok(())` - If the record was untagged successfully
/// * `Err(_)` - If untagging failed
async fn untag_record_from_zone(
    client: &Client,
    namespace: &str,
    kind: &str,
    name: &str,
    previous_zone_fqdn: &str,
) -> Result<()> {
    debug!(
        "Untagging {} {}/{} from zone {}",
        kind, namespace, name, previous_zone_fqdn
    );

    // Convert kind to plural resource name
    let plural = format!("{}s", kind.to_lowercase());

    // Create GroupVersionKind for the resource
    let gvk = kube::core::GroupVersionKind {
        group: "bindy.firestoned.io".to_string(),
        version: "v1beta1".to_string(),
        kind: kind.to_string(),
    };

    // Use kube's Discovery API to create ApiResource
    let api_resource = kube::api::ApiResource::from_gvk_with_plural(&gvk, &plural);

    // Create a dynamic API client
    let api = kube::api::Api::<kube::api::DynamicObject>::namespaced_with(
        client.clone(),
        namespace,
        &api_resource,
    );

    // Patch metadata to remove zone annotation and add previous-zone annotation
    let annotation_patch = json!({
        "metadata": {
            "annotations": {
                ANNOTATION_ZONE_OWNER: null,
                ANNOTATION_ZONE_PREVIOUS_OWNER: previous_zone_fqdn
            }
        }
    });

    api.patch(
        name,
        &PatchParams::default(),
        &Patch::Merge(&annotation_patch),
    )
    .await
    .with_context(|| format!("Failed to remove zone annotation from {kind} {namespace}/{name}"))?;

    // Patch status to remove zone field
    let status_patch = json!({
        "status": {
            "zone": null
        }
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .with_context(|| format!("Failed to clear status.zone on {kind} {namespace}/{name}"))?;

    info!(
        "Successfully untagged {} {}/{} from zone {}",
        kind, namespace, name, previous_zone_fqdn
    );

    Ok(())
}

/// Helper function to discover A records matching a label selector.
async fn discover_a_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<crate::crd::RecordReference>> {
    use crate::crd::ARecord;
    use std::collections::BTreeMap;

    let api: Api<ARecord> = Api::namespaced(client.clone(), namespace);
    let records = api.list(&ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.metadata.labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!("Discovered A record {}/{}", namespace, record.name_any());

        record_refs.push(crate::crd::RecordReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "ARecord".to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
        });
    }

    Ok(record_refs)
}

/// Helper function to discover AAAA records matching a label selector.
async fn discover_aaaa_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<crate::crd::RecordReference>> {
    use crate::crd::AAAARecord;
    use std::collections::BTreeMap;

    let api: Api<AAAARecord> = Api::namespaced(client.clone(), namespace);
    let records = api.list(&ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.metadata.labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!("Discovered AAAA record {}/{}", namespace, record.name_any());

        record_refs.push(crate::crd::RecordReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "AAAARecord".to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
        });
    }

    Ok(record_refs)
}

/// Helper function to discover TXT records matching a label selector.
async fn discover_txt_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<crate::crd::RecordReference>> {
    use crate::crd::TXTRecord;
    use std::collections::BTreeMap;

    let api: Api<TXTRecord> = Api::namespaced(client.clone(), namespace);
    let records = api.list(&ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.metadata.labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!("Discovered TXT record {}/{}", namespace, record.name_any());

        record_refs.push(crate::crd::RecordReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "TXTRecord".to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
        });
    }

    Ok(record_refs)
}

/// Helper function to discover CNAME records matching a label selector.
async fn discover_cname_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<crate::crd::RecordReference>> {
    use crate::crd::CNAMERecord;
    use std::collections::BTreeMap;

    let api: Api<CNAMERecord> = Api::namespaced(client.clone(), namespace);
    let records = api.list(&ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.metadata.labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!(
            "Discovered CNAME record {}/{}",
            namespace,
            record.name_any()
        );

        record_refs.push(crate::crd::RecordReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "CNAMERecord".to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
        });
    }

    Ok(record_refs)
}

/// Helper function to discover MX records matching a label selector.
async fn discover_mx_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<crate::crd::RecordReference>> {
    use crate::crd::MXRecord;
    use std::collections::BTreeMap;

    let api: Api<MXRecord> = Api::namespaced(client.clone(), namespace);
    let records = api.list(&ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.metadata.labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!("Discovered MX record {}/{}", namespace, record.name_any());

        record_refs.push(crate::crd::RecordReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "MXRecord".to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
        });
    }

    Ok(record_refs)
}

/// Helper function to discover NS records matching a label selector.
async fn discover_ns_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<crate::crd::RecordReference>> {
    use crate::crd::NSRecord;
    use std::collections::BTreeMap;

    let api: Api<NSRecord> = Api::namespaced(client.clone(), namespace);
    let records = api.list(&ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.metadata.labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!("Discovered NS record {}/{}", namespace, record.name_any());

        record_refs.push(crate::crd::RecordReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "NSRecord".to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
        });
    }

    Ok(record_refs)
}

/// Helper function to discover SRV records matching a label selector.
async fn discover_srv_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<crate::crd::RecordReference>> {
    use crate::crd::SRVRecord;
    use std::collections::BTreeMap;

    let api: Api<SRVRecord> = Api::namespaced(client.clone(), namespace);
    let records = api.list(&ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.metadata.labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!("Discovered SRV record {}/{}", namespace, record.name_any());

        record_refs.push(crate::crd::RecordReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "SRVRecord".to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
        });
    }

    Ok(record_refs)
}

/// Helper function to discover CAA records matching a label selector.
async fn discover_caa_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<crate::crd::RecordReference>> {
    use crate::crd::CAARecord;
    use std::collections::BTreeMap;

    let api: Api<CAARecord> = Api::namespaced(client.clone(), namespace);
    let records = api.list(&ListParams::default()).await?;

    let mut record_refs = Vec::new();
    for record in records {
        let labels: BTreeMap<String, String> = record.metadata.labels.clone().unwrap_or_default();

        if !selector.matches(&labels) {
            continue;
        }

        debug!("Discovered CAA record {}/{}", namespace, record.name_any());

        record_refs.push(crate::crd::RecordReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "CAARecord".to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
        });
    }

    Ok(record_refs)
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
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    let spec = &dnszone.spec;

    // Extract and validate cluster reference
    let cluster_ref = get_cluster_ref_from_spec(spec, &namespace, &name)?;
    let is_cluster_provider = spec.cluster_provider_ref.is_some();

    info!("Deleting DNSZone: {}", name);

    // Use the common helper to iterate through all endpoints
    // Don't load RNDC key (false) since zone deletion doesn't require it
    // Use "http" port for HTTP API operations
    let (_first_endpoint, total_endpoints) = for_each_primary_endpoint(
        &client,
        &namespace,
        &cluster_ref,
        is_cluster_provider,
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
                if let Err(e) = zone_manager.delete_zone(&zone_name, &pod_endpoint).await {
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
        "Successfully deleted zone {} from {} primary endpoint(s) for cluster {}",
        spec.zone_name, total_endpoints, cluster_ref
    );

    // Delete from all secondary instances
    let secondary_pods =
        find_all_secondary_pods(&client, &namespace, &cluster_ref, is_cluster_provider).await?;

    if !secondary_pods.is_empty() {
        // Get unique (instance_name, namespace) tuples
        let mut instance_tuples: Vec<(String, String)> = secondary_pods
            .iter()
            .map(|pod| (pod.instance_name.clone(), pod.namespace.clone()))
            .collect();
        instance_tuples.sort();
        instance_tuples.dedup();

        let mut secondary_endpoints_deleted = 0;

        for (instance_name, instance_namespace) in &instance_tuples {
            let endpoints =
                get_endpoint(&client, instance_namespace, instance_name, "http").await?;

            for endpoint in &endpoints {
                let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

                info!(
                    "Deleting zone {} from secondary endpoint {} (instance: {})",
                    spec.zone_name, pod_endpoint, instance_name
                );

                // Attempt to delete zone - if it fails, log a warning but don't fail the deletion
                if let Err(e) = zone_manager
                    .delete_zone(&spec.zone_name, &pod_endpoint)
                    .await
                {
                    warn!(
                        "Failed to delete zone {} from secondary endpoint {} (instance: {}): {}. Continuing with deletion anyway.",
                        spec.zone_name, pod_endpoint, instance_name, e
                    );
                } else {
                    debug!(
                        "Successfully deleted zone {} from secondary endpoint {} (instance: {})",
                        spec.zone_name, pod_endpoint, instance_name
                    );
                    secondary_endpoints_deleted += 1;
                }
            }
        }

        info!(
            "Successfully deleted zone {} from {} secondary endpoint(s) for cluster {}",
            spec.zone_name, secondary_endpoints_deleted, cluster_ref
        );
    }

    // Note: We don't need to reload after delzone because:
    // 1. rndc delzone immediately removes the zone from BIND9's running config
    // 2. BIND9 will clean up the zone file and journal files automatically

    Ok(())
}

/// Find `Bind9Instance` resources matching a label selector
async fn find_matching_instances(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
) -> Result<Vec<String>> {
    use crate::crd::Bind9Instance;

    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    // Build label selector string
    let label_selector = build_label_selector(selector);

    let params = kube::api::ListParams::default();
    let params = if let Some(selector_str) = label_selector {
        params.labels(&selector_str)
    } else {
        params
    };

    let instances = api.list(&params).await?;

    let instance_names: Vec<String> = instances
        .items
        .iter()
        .map(kube::ResourceExt::name_any)
        .collect();

    Ok(instance_names)
}

/// Build a Kubernetes label selector string from our `LabelSelector`
pub(crate) fn build_label_selector(selector: &crate::crd::LabelSelector) -> Option<String> {
    let mut parts = Vec::new();

    // Add match labels
    if let Some(labels) = &selector.match_labels {
        for (key, value) in labels {
            parts.push(format!("{key}={value}"));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Helper struct for pod information
#[derive(Clone)]
pub struct PodInfo {
    pub name: String,
    pub ip: String,
    pub instance_name: String,
    pub namespace: String,
}

/// Find ALL PRIMARY pods for the given `Bind9Cluster` or `ClusterBind9Provider`
///
/// Returns all running pods for PRIMARY instances in the cluster to ensure zone changes
/// are applied to all primary replicas consistently.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search in (ignored if `is_cluster_provider` is true)
/// * `cluster_name` - Name of the `Bind9Cluster` or `ClusterBind9Provider`
/// * `is_cluster_provider` - If true, searches all namespaces; if false, searches only the specified namespace
///
/// # Returns
///
/// A vector of `PodInfo` containing all running PRIMARY pods
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail
pub async fn find_all_primary_pods(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    is_cluster_provider: bool,
) -> Result<Vec<PodInfo>> {
    use crate::crd::{Bind9Instance, ServerRole};

    // First, find all Bind9Instance resources that belong to this cluster and have role=primary
    let instance_api: Api<Bind9Instance> = if is_cluster_provider {
        Api::all(client.clone())
    } else {
        Api::namespaced(client.clone(), namespace)
    };
    let instances = instance_api.list(&ListParams::default()).await?;

    // Store tuples of (instance_name, instance_namespace)
    let mut primary_instances: Vec<(String, String)> = Vec::new();
    for instance in instances.items {
        if instance.spec.cluster_ref == cluster_name && instance.spec.role == ServerRole::Primary {
            if let (Some(name), Some(ns)) = (instance.metadata.name, instance.metadata.namespace) {
                primary_instances.push((name, ns));
            }
        }
    }

    if primary_instances.is_empty() {
        let search_scope = if is_cluster_provider {
            "all namespaces".to_string()
        } else {
            format!("namespace {namespace}")
        };
        return Err(anyhow!(
            "No PRIMARY Bind9Instance resources found for cluster {cluster_name} in {search_scope}"
        ));
    }

    info!(
        "Found {} PRIMARY instance(s) for cluster {}: {:?}",
        primary_instances.len(),
        cluster_name,
        primary_instances
    );

    let mut all_pod_infos = Vec::new();

    for (instance_name, instance_namespace) in &primary_instances {
        // Now find all pods for this primary instance in its namespace
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), instance_namespace);
        // List pods with label selector matching the instance
        let label_selector = format!("app=bind9,instance={instance_name}");
        let lp = ListParams::default().labels(&label_selector);

        let pods = pod_api.list(&lp).await?;

        debug!(
            "Found {} pod(s) for PRIMARY instance {}",
            pods.items.len(),
            instance_name
        );

        for pod in &pods.items {
            let pod_name = pod
                .metadata
                .name
                .as_ref()
                .ok_or_else(|| anyhow!("Pod has no name"))?
                .clone();

            // Get pod IP
            let pod_ip = pod
                .status
                .as_ref()
                .and_then(|s| s.pod_ip.as_ref())
                .ok_or_else(|| anyhow!("Pod {pod_name} has no IP address"))?
                .clone();

            // Check if pod is running
            let phase = pod
                .status
                .as_ref()
                .and_then(|s| s.phase.as_ref())
                .map(String::as_str);

            if phase == Some("Running") {
                all_pod_infos.push(PodInfo {
                    name: pod_name.clone(),
                    ip: pod_ip.clone(),
                    instance_name: instance_name.clone(),
                    namespace: instance_namespace.clone(),
                });
                debug!(
                    "Found running pod {} with IP {} in namespace {}",
                    pod_name, pod_ip, instance_namespace
                );
            } else {
                debug!(
                    "Skipping pod {} (phase: {:?}, not running)",
                    pod_name, phase
                );
            }
        }
    }

    if all_pod_infos.is_empty() {
        return Err(anyhow!(
            "No running PRIMARY pods found for cluster {cluster_name} in namespace {namespace}"
        ));
    }

    info!(
        "Found {} running PRIMARY pod(s) across {} instance(s) for cluster {}",
        all_pod_infos.len(),
        primary_instances.len(),
        cluster_name
    );

    Ok(all_pod_infos)
}

/// Find all SECONDARY pod IPs for a given cluster or global cluster.
///
/// This is a helper function that calls `find_all_secondary_pods` and extracts only the IPs.
///
/// Returns IP addresses of all running secondary pods in the cluster.
/// These IPs are used for configuring also-notify and allow-transfer on primary zones.
async fn find_all_secondary_pod_ips(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    is_cluster_provider: bool,
) -> Result<Vec<String>> {
    info!("Finding SECONDARY pod IPs for cluster {}", cluster_name);

    let secondary_pods =
        find_all_secondary_pods(client, namespace, cluster_name, is_cluster_provider).await?;

    let secondary_ips: Vec<String> = secondary_pods.iter().map(|pod| pod.ip.clone()).collect();

    info!(
        "Found {} running SECONDARY pod IP(s) for cluster {}: {:?}",
        secondary_ips.len(),
        cluster_name,
        secondary_ips
    );

    Ok(secondary_ips)
}

/// Find all PRIMARY pod IPs for a given cluster or global cluster.
///
/// Returns IP addresses of all running primary pods in the cluster.
/// These IPs are used for configuring primaries on secondary zones.
async fn find_all_primary_pod_ips(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    is_cluster_provider: bool,
) -> Result<Vec<String>> {
    info!("Finding PRIMARY pod IPs for cluster {}", cluster_name);

    let primary_pods =
        find_all_primary_pods(client, namespace, cluster_name, is_cluster_provider).await?;

    let primary_ips: Vec<String> = primary_pods.iter().map(|pod| pod.ip.clone()).collect();

    info!(
        "Found {} running PRIMARY pod IP(s) for cluster {}: {:?}",
        primary_ips.len(),
        cluster_name,
        primary_ips
    );

    Ok(primary_ips)
}

/// Find all SECONDARY pods for a given cluster or global cluster.
///
/// Returns structured pod information including IP, name, instance name, and namespace.
/// Similar to `find_all_primary_pods` but for secondary instances.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search in (ignored if `is_cluster_provider` is true)
/// * `cluster_name` - Name of the `Bind9Cluster` or `ClusterBind9Provider`
/// * `is_cluster_provider` - If true, searches all namespaces; if false, searches only the specified namespace
///
/// # Returns
///
/// A vector of `PodInfo` containing all running SECONDARY pods
async fn find_all_secondary_pods(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    is_cluster_provider: bool,
) -> Result<Vec<PodInfo>> {
    use crate::crd::{Bind9Instance, ServerRole};

    // Find all Bind9Instance resources with role=SECONDARY for this cluster
    let instance_api: Api<Bind9Instance> = if is_cluster_provider {
        Api::all(client.clone())
    } else {
        Api::namespaced(client.clone(), namespace)
    };
    let instances = instance_api.list(&ListParams::default()).await?;

    // Store tuples of (instance_name, instance_namespace)
    let mut secondary_instances: Vec<(String, String)> = Vec::new();
    for instance in instances.items {
        if instance.spec.cluster_ref == cluster_name && instance.spec.role == ServerRole::Secondary
        {
            if let (Some(name), Some(ns)) = (instance.metadata.name, instance.metadata.namespace) {
                secondary_instances.push((name, ns));
            }
        }
    }

    if secondary_instances.is_empty() {
        info!("No SECONDARY instances found for cluster {cluster_name}");
        return Ok(Vec::new());
    }

    info!(
        "Found {} SECONDARY instance(s) for cluster {}: {:?}",
        secondary_instances.len(),
        cluster_name,
        secondary_instances
    );

    let mut all_pod_infos = Vec::new();

    for (instance_name, instance_namespace) in &secondary_instances {
        // Find all pods for this secondary instance in its namespace
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), instance_namespace);
        let label_selector = format!("app=bind9,instance={instance_name}");
        let lp = ListParams::default().labels(&label_selector);

        let pods = pod_api.list(&lp).await?;

        debug!(
            "Found {} pod(s) for SECONDARY instance {}",
            pods.items.len(),
            instance_name
        );

        for pod in &pods.items {
            let pod_name = pod
                .metadata
                .name
                .as_ref()
                .ok_or_else(|| anyhow!("Pod has no name"))?
                .clone();

            // Get pod IP
            let pod_ip = pod
                .status
                .as_ref()
                .and_then(|s| s.pod_ip.as_ref())
                .ok_or_else(|| anyhow!("Pod {pod_name} has no IP address"))?
                .clone();

            // Check if pod is running
            let phase = pod
                .status
                .as_ref()
                .and_then(|s| s.phase.as_ref())
                .map(String::as_str);

            if phase == Some("Running") {
                all_pod_infos.push(PodInfo {
                    name: pod_name.clone(),
                    ip: pod_ip.clone(),
                    instance_name: instance_name.clone(),
                    namespace: instance_namespace.clone(),
                });
                debug!(
                    "Found running secondary pod {} with IP {} in namespace {}",
                    pod_name, pod_ip, instance_namespace
                );
            } else {
                debug!(
                    "Skipping secondary pod {} (phase: {:?}, not running)",
                    pod_name, phase
                );
            }
        }
    }

    info!(
        "Found {} running SECONDARY pod(s) across {} instance(s) for cluster {}",
        all_pod_infos.len(),
        secondary_instances.len(),
        cluster_name
    );

    Ok(all_pod_infos)
}

/// Load RNDC key from the instance's secret
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

/// Check if the operator is running inside a Kubernetes cluster
///
/// Detects the environment by checking for the presence of the Kubernetes service account token,
/// which is automatically mounted in all pods running in the cluster.
///
/// Returns `true` if running in-cluster, `false` if running locally (e.g., via kubectl proxy)
fn is_running_in_cluster() -> bool {
    std::path::Path::new("/var/run/secrets/kubernetes.io/serviceaccount/token").exists()
}

/// Update a single condition on the `DNSZone` status
///
/// This is a lightweight status update that only modifies the conditions field.
/// Use this for intermediate status updates during reconciliation.
async fn update_condition(
    client: &Client,
    dnszone: &DNSZone,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
) -> Result<()> {
    let api: Api<DNSZone> =
        Api::namespaced(client.clone(), &dnszone.namespace().unwrap_or_default());

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        last_transition_time: Some(Utc::now().to_rfc3339()),
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
    };

    // Preserve existing status fields, only update conditions
    let current_status = dnszone.status.as_ref();
    let new_status = DNSZoneStatus {
        conditions: vec![condition],
        observed_generation: current_status
            .and_then(|s| s.observed_generation)
            .or(dnszone.metadata.generation),
        record_count: current_status.and_then(|s| s.record_count),
        secondary_ips: current_status.and_then(|s| s.secondary_ips.clone()),
        records: current_status
            .map(|s| s.records.clone())
            .unwrap_or_default(),
    };

    let patch = json!({
        "status": new_status
    });

    api.patch_status(
        &dnszone.name_any(),
        &PatchParams::default(),
        &Patch::Merge(&patch),
    )
    .await?;

    debug!(
        "Updated DNSZone {}/{} condition: type={}, status={}, reason={}",
        dnszone.namespace().unwrap_or_default(),
        dnszone.name_any(),
        condition_type,
        status,
        reason
    );

    Ok(())
}

/// Update `DNSZone` status including secondary IPs
async fn update_status_with_secondaries(
    client: &Client,
    dnszone: &DNSZone,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
    secondary_ips: Vec<String>,
) -> Result<()> {
    let api: Api<DNSZone> =
        Api::namespaced(client.clone(), &dnszone.namespace().unwrap_or_default());

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        last_transition_time: Some(Utc::now().to_rfc3339()),
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
    };

    let new_status = DNSZoneStatus {
        conditions: vec![condition],
        observed_generation: dnszone.metadata.generation,
        record_count: dnszone.status.as_ref().and_then(|s| s.record_count),
        secondary_ips: if secondary_ips.is_empty() {
            None
        } else {
            Some(secondary_ips)
        },
        records: dnszone
            .status
            .as_ref()
            .map(|s| s.records.clone())
            .unwrap_or_default(),
    };

    let patch = json!({
        "status": new_status
    });

    api.patch_status(
        &dnszone.name_any(),
        &PatchParams::default(),
        &Patch::Merge(&patch),
    )
    .await?;

    info!(
        "Updated DNSZone {}/{} status: {}={}",
        dnszone.namespace().unwrap_or_default(),
        dnszone.name_any(),
        condition_type,
        status
    );

    Ok(())
}

async fn update_status(
    client: &Client,
    dnszone: &DNSZone,
    condition_type: &str,
    status: &str,
    message: &str,
) -> Result<()> {
    let api: Api<DNSZone> =
        Api::namespaced(client.clone(), &dnszone.namespace().unwrap_or_default());

    // Check if status has actually changed
    let current_status = &dnszone.status;
    let status_changed = if let Some(current) = current_status {
        if let Some(current_condition) = current.conditions.first() {
            // Check if condition changed
            current_condition.r#type != condition_type
                || current_condition.status != status
                || current_condition.message.as_deref() != Some(message)
        } else {
            // No conditions exist, need to update
            true
        }
    } else {
        // No status exists, need to update
        true
    };

    // Only update if status has changed
    if !status_changed {
        debug!(
            namespace = %dnszone.namespace().unwrap_or_default(),
            name = %dnszone.name_any(),
            "Status unchanged, skipping update"
        );
        info!(
            "DNSZone {}/{} status unchanged, skipping update",
            dnszone.namespace().unwrap_or_default(),
            dnszone.name_any()
        );
        return Ok(());
    }

    debug!(
        condition_type = %condition_type,
        status = %status,
        message = %message,
        "Preparing status update"
    );

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(condition_type.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(Utc::now().to_rfc3339()),
    };

    let new_status = DNSZoneStatus {
        conditions: vec![condition],
        observed_generation: dnszone.metadata.generation,
        record_count: None,
        secondary_ips: dnszone
            .status
            .as_ref()
            .and_then(|s| s.secondary_ips.clone()),
        records: dnszone
            .status
            .as_ref()
            .map(|s| s.records.clone())
            .unwrap_or_default(),
    };

    info!(
        "Updating DNSZone {}/{} status",
        dnszone.namespace().unwrap_or_default(),
        dnszone.name_any()
    );

    let patch = json!({ "status": new_status });
    api.patch_status(
        &dnszone.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(())
}

/// Structure representing an endpoint (pod IP and port)
#[derive(Debug, Clone)]
pub struct EndpointAddress {
    /// IP address of the pod
    pub ip: String,
    /// Container port number
    pub port: i32,
}

/// Execute an operation on all endpoints of all primary instances in a cluster.
///
/// This helper function handles the common pattern of:
/// 1. Finding all primary pods for a cluster
/// 2. Collecting unique instance names
/// 3. Optionally loading RNDC key from the first instance
/// 4. Getting endpoints for each instance
/// 5. Executing a provided operation on each endpoint
///
/// # Arguments
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the cluster
/// * `cluster_ref` - Name of the `Bind9Cluster`
/// * `with_rndc_key` - Whether to load RNDC key from first instance
/// * `operation` - Async closure to execute for each endpoint
///   - Arguments: `(pod_endpoint: String, instance_name: String, rndc_key: Option<RndcKeyData>)`
///   - Returns: `Result<()>`
///
/// # Returns
/// Returns `Ok((first_endpoint, total_count))` where:
/// - `first_endpoint` - Optional first endpoint encountered (useful for NOTIFY operations)
/// - `total_count` - Total number of endpoints processed
///
/// # Errors
/// Returns error if:
/// - No primary pods found for the cluster
/// - Failed to load RNDC key (if requested)
/// - Failed to get endpoints for any instance
/// - The operation closure returns an error for any endpoint
pub async fn for_each_primary_endpoint<F, Fut>(
    client: &Client,
    namespace: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    with_rndc_key: bool,
    port_name: &str,
    operation: F,
) -> Result<(Option<String>, usize)>
where
    F: Fn(String, String, Option<RndcKeyData>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    // Find all PRIMARY pods to get the unique instance names
    let primary_pods =
        find_all_primary_pods(client, namespace, cluster_ref, is_cluster_provider).await?;

    info!(
        "Found {} PRIMARY pod(s) for cluster {}",
        primary_pods.len(),
        cluster_ref
    );

    // Collect unique (instance_name, namespace) tuples from the primary pods
    // Each instance may have multiple pods (replicas)
    let mut instance_tuples: Vec<(String, String)> = primary_pods
        .iter()
        .map(|pod| (pod.instance_name.clone(), pod.namespace.clone()))
        .collect();
    instance_tuples.sort();
    instance_tuples.dedup();

    info!(
        "Found {} primary instance(s) for cluster {}: {:?}",
        instance_tuples.len(),
        cluster_ref,
        instance_tuples
    );

    let mut first_endpoint: Option<String> = None;
    let mut total_endpoints = 0;
    let mut errors: Vec<String> = Vec::new();

    // Loop through each primary instance and get its endpoints
    // Important: With EmptyDir storage (per-pod, non-shared), each primary pod maintains its own
    // zone files. We need to process ALL pods across ALL instances.
    for (instance_name, instance_namespace) in &instance_tuples {
        info!(
            "Getting endpoints for instance {}/{} in cluster {}",
            instance_namespace, instance_name, cluster_ref
        );

        // Load RNDC key for this specific instance if requested
        // Each instance has its own RNDC secret for security isolation
        let key_data = if with_rndc_key {
            Some(load_rndc_key(client, instance_namespace, instance_name).await?)
        } else {
            None
        };

        // Get all endpoints for this instance's service
        // The Endpoints API gives us pod IPs with their container ports (not service ports)
        let endpoints = get_endpoint(client, instance_namespace, instance_name, port_name).await?;

        info!(
            "Found {} endpoint(s) for instance {}",
            endpoints.len(),
            instance_name
        );

        for endpoint in &endpoints {
            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

            // Save the first endpoint
            if first_endpoint.is_none() {
                first_endpoint = Some(pod_endpoint.clone());
            }

            // Execute the operation on this endpoint with this instance's RNDC key
            // Continue processing remaining endpoints even if this one fails
            if let Err(e) = operation(
                pod_endpoint.clone(),
                instance_name.clone(),
                key_data.clone(),
            )
            .await
            {
                error!(
                    "Failed operation on endpoint {} (instance {}): {}",
                    pod_endpoint, instance_name, e
                );
                errors.push(format!(
                    "endpoint {pod_endpoint} (instance {instance_name}): {e}"
                ));
            } else {
                total_endpoints += 1;
            }
        }
    }

    // If any operations failed, return an error with all failures listed
    if !errors.is_empty() {
        return Err(anyhow::anyhow!(
            "Failed to process {} endpoint(s): {}",
            errors.len(),
            errors.join("; ")
        ));
    }

    Ok((first_endpoint, total_endpoints))
}

/// Execute an operation on all SECONDARY endpoints for a cluster.
///
/// Similar to `for_each_primary_endpoint`, but operates on SECONDARY instances.
/// Useful for triggering zone transfers or other secondary-specific operations.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for instances
/// * `cluster_ref` - Cluster reference name
/// * `is_cluster_provider` - Whether this is a cluster provider (cluster-scoped)
/// * `with_rndc_key` - Whether to load and pass RNDC keys for each instance
/// * `port_name` - Port name to use for endpoints (e.g., "rndc-api", "dns-tcp")
/// * `operation` - Async closure to execute for each endpoint
///
/// # Returns
///
/// * `Ok((first_endpoint, total_endpoints))` - First endpoint found and total count
///
/// # Errors
///
/// Returns an error if:
/// - Failed to find secondary pods
/// - Failed to load RNDC keys
/// - Failed to get service endpoints
/// - The operation closure returns an error for any endpoint
pub async fn for_each_secondary_endpoint<F, Fut>(
    client: &Client,
    namespace: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    with_rndc_key: bool,
    port_name: &str,
    operation: F,
) -> Result<(Option<String>, usize)>
where
    F: Fn(String, String, Option<RndcKeyData>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    // Find all SECONDARY pods to get the unique instance names
    let secondary_pods =
        find_all_secondary_pods(client, namespace, cluster_ref, is_cluster_provider).await?;

    info!(
        "Found {} SECONDARY pod(s) for cluster {}",
        secondary_pods.len(),
        cluster_ref
    );

    // Collect unique (instance_name, namespace) tuples from the secondary pods
    // Each instance may have multiple pods (replicas)
    let mut instance_tuples: Vec<(String, String)> = secondary_pods
        .iter()
        .map(|pod| (pod.instance_name.clone(), pod.namespace.clone()))
        .collect();
    instance_tuples.sort();
    instance_tuples.dedup();

    info!(
        "Found {} secondary instance(s) for cluster {}: {:?}",
        instance_tuples.len(),
        cluster_ref,
        instance_tuples
    );

    let mut first_endpoint: Option<String> = None;
    let mut total_endpoints = 0;
    let mut errors: Vec<String> = Vec::new();

    // Loop through each secondary instance and get its endpoints
    for (instance_name, instance_namespace) in &instance_tuples {
        info!(
            "Getting endpoints for secondary instance {}/{} in cluster {}",
            instance_namespace, instance_name, cluster_ref
        );

        // Load RNDC key for this specific instance if requested
        // Each instance has its own RNDC secret for security isolation
        let key_data = if with_rndc_key {
            Some(load_rndc_key(client, instance_namespace, instance_name).await?)
        } else {
            None
        };

        // Get all endpoints for this instance's service
        // The Endpoints API gives us pod IPs with their container ports (not service ports)
        let endpoints = get_endpoint(client, instance_namespace, instance_name, port_name).await?;

        info!(
            "Found {} endpoint(s) for secondary instance {}",
            endpoints.len(),
            instance_name
        );

        for endpoint in &endpoints {
            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

            // Save the first endpoint
            if first_endpoint.is_none() {
                first_endpoint = Some(pod_endpoint.clone());
            }

            // Execute the operation on this endpoint with this instance's RNDC key
            // Continue processing remaining endpoints even if this one fails
            if let Err(e) = operation(
                pod_endpoint.clone(),
                instance_name.clone(),
                key_data.clone(),
            )
            .await
            {
                error!(
                    "Failed operation on secondary endpoint {} (instance {}): {}",
                    pod_endpoint, instance_name, e
                );
                errors.push(format!(
                    "endpoint {pod_endpoint} (instance {instance_name}): {e}"
                ));
            } else {
                total_endpoints += 1;
            }
        }
    }

    // If any operations failed, return an error with all failures listed
    if !errors.is_empty() {
        return Err(anyhow::anyhow!(
            "Failed to process {} secondary endpoint(s): {}",
            errors.len(),
            errors.join("; ")
        ));
    }

    Ok((first_endpoint, total_endpoints))
}

/// Get all endpoints for a service with a specific port name
///
/// Looks up the Kubernetes Endpoints object associated with a service and returns
/// all pod IP addresses with their corresponding container ports.
///
/// When connecting directly to pod IPs, you must use the container port from the endpoints,
/// not the service port.
///
/// # Arguments
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace of the service/endpoints
/// * `service_name` - Name of the service (endpoints have the same name)
/// * `port_name` - Name of the port to lookup (e.g., "http", "dns-tcp")
///
/// # Returns
/// Vector of `EndpointAddress` containing pod IP and container port pairs
///
/// # Errors
/// Returns error if:
/// - Endpoints object doesn't exist
/// - Port name not found in any endpoint subset
/// - No ready addresses found
pub async fn get_endpoint(
    client: &Client,
    namespace: &str,
    service_name: &str,
    port_name: &str,
) -> Result<Vec<EndpointAddress>> {
    let endpoints_api: Api<Endpoints> = Api::namespaced(client.clone(), namespace);
    let endpoints = endpoints_api.get(service_name).await.context(format!(
        "Failed to get endpoints for service {service_name}"
    ))?;

    let mut result = Vec::new();

    // Endpoints are organized into subsets. Each subset has:
    // - addresses: List of ready pod IPs
    // - ports: List of container ports
    if let Some(subsets) = endpoints.subsets {
        for subset in subsets {
            // Find the port in this subset
            if let Some(ports) = subset.ports {
                if let Some(endpoint_port) = ports
                    .iter()
                    .find(|p| p.name.as_ref().is_some_and(|name| name == port_name))
                {
                    let port = endpoint_port.port;

                    // Get all ready addresses for this subset
                    if let Some(addresses) = subset.addresses {
                        for addr in addresses {
                            result.push(EndpointAddress {
                                ip: addr.ip.clone(),
                                port,
                            });
                        }
                    }
                }
            }
        }
    }

    if result.is_empty() {
        return Err(anyhow!(
            "No ready endpoints found for service {service_name} with port '{port_name}'"
        ));
    }

    Ok(result)
}

/// Check if all discovered records are ready.
///
/// Queries each record in the list and checks if it has a "Ready" condition with status="True".
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace containing the records
/// * `record_refs` - List of record references from `DNSZone.status.records`
///
/// # Returns
///
/// * `Ok(true)` - All records are ready
/// * `Ok(false)` - Some records are not ready
/// * `Err(_)` - API error
async fn check_all_records_ready(
    client: &Client,
    namespace: &str,
    record_refs: &[crate::crd::RecordReference],
) -> Result<bool> {
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, TXTRecord,
    };

    for record_ref in record_refs {
        let is_ready = match record_ref.kind.as_str() {
            "ARecord" => {
                let api: Api<ARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            "AAAARecord" => {
                let api: Api<AAAARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            "TXTRecord" => {
                let api: Api<TXTRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            "CNAMERecord" => {
                let api: Api<CNAMERecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            "MXRecord" => {
                let api: Api<MXRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            "NSRecord" => {
                let api: Api<NSRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            "SRVRecord" => {
                let api: Api<SRVRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            "CAARecord" => {
                let api: Api<CAARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            _ => {
                warn!(
                    "Unknown record kind: {}, skipping readiness check",
                    record_ref.kind
                );
                false
            }
        };

        if !is_ready {
            debug!(
                "Record {}/{} (kind: {}) is not ready yet",
                namespace, record_ref.name, record_ref.kind
            );
            return Ok(false);
        }
    }

    Ok(true)
}

/// Check if a specific record is ready by examining its status conditions.
async fn check_record_ready<T>(api: &Api<T>, name: &str) -> Result<bool>
where
    T: kube::Resource<DynamicType = ()>
        + Clone
        + serde::de::DeserializeOwned
        + serde::Serialize
        + std::fmt::Debug
        + Send
        + Sync,
    <T as kube::Resource>::DynamicType: Default,
{
    let record = match api.get(name).await {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to get record {}: {}", name, e);
            return Ok(false);
        }
    };

    // Use serde_json to access the status field dynamically
    let record_json = serde_json::to_value(&record)?;
    let status = record_json.get("status");

    if let Some(status_obj) = status {
        if let Some(conditions) = status_obj.get("conditions").and_then(|c| c.as_array()) {
            for condition in conditions {
                if let (Some(type_val), Some(status_val)) = (
                    condition.get("type").and_then(|t| t.as_str()),
                    condition.get("status").and_then(|s| s.as_str()),
                ) {
                    if type_val == "Ready" && status_val == "True" {
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(false)
}

/// Find all `DNSZones` that have selected a given record via label selectors.
///
/// This function is used by the watch mapper to determine which `DNSZones` should be
/// reconciled when a DNS record changes. It checks each `DNSZone`'s `status.records` list
/// to see if the record is present.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record_namespace` - Namespace of the record
/// * `record_kind` - Kind of the record (e.g., `"ARecord"`, `"TXTRecord"`)
/// * `record_name` - Name of the record resource
///
/// # Returns
///
/// A vector of tuples containing `(zone_name, zone_namespace)` for all `DNSZones` that have
/// selected this record.
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail.
pub async fn find_zones_selecting_record(
    client: &Client,
    record_namespace: &str,
    record_kind: &str,
    record_name: &str,
) -> Result<Vec<(String, String)>> {
    let api: Api<DNSZone> = Api::namespaced(client.clone(), record_namespace);
    let zones = api.list(&ListParams::default()).await?;

    let mut selecting_zones = vec![];

    for zone in zones {
        let Some(ref status) = zone.status else {
            continue;
        };

        // Check if this record is in the zone's status.records list
        let is_selected = status
            .records
            .iter()
            .any(|r| r.kind == record_kind && r.name == record_name);

        if is_selected {
            let zone_name = zone.name_any();
            let zone_namespace = zone.namespace().unwrap_or_default();
            selecting_zones.push((zone_name, zone_namespace));
        }
    }

    Ok(selecting_zones)
}

/// Trigger zone transfers to all secondary instances.
///
/// Uses the `rndc retransfer` command to initiate zone transfers from primaries to secondaries.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace containing the BIND9 instances
/// * `zone_name` - Name of the zone to transfer
/// * `cluster_ref` - Cluster reference
/// * `is_cluster_provider` - Whether this is a cluster provider reference
/// * `zone_manager` - BIND9 manager for zone operations
///
/// # Returns
///
/// * `Ok(usize)` - Number of secondaries that successfully initiated transfer
/// * `Err(_)` - If no secondaries found or all transfers failed
async fn trigger_zone_transfers(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    cluster_ref: &str,
    is_cluster_provider: bool,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<usize> {
    let (_first_endpoint, total_endpoints) = for_each_secondary_endpoint(
        client,
        namespace,
        cluster_ref,
        is_cluster_provider,
        false,  // with_rndc_key = false (not needed for retransfer)
        "http", // Use bindcar HTTP API port for zone operations
        |secondary_endpoint, instance_name, _rndc_key| {
            let zone_name = zone_name.to_string();
            let zone_manager = zone_manager.clone();

            async move {
                zone_manager
                    .retransfer_zone(&zone_name, &secondary_endpoint)
                    .await
                    .with_context(|| format!(
                        "Failed to trigger zone transfer for {zone_name} on secondary {secondary_endpoint} (instance: {instance_name})"
                    ))?;

                info!(
                    "Triggered zone transfer for {zone_name} on secondary {secondary_endpoint} (instance: {instance_name})"
                );

                Ok(())
            }
        },
    )
    .await?;

    if total_endpoints == 0 {
        warn!(
            "No secondary instances found for zone {} in cluster {}",
            zone_name, cluster_ref
        );
    }

    Ok(total_endpoints)
}

/// Trigger reconciliation of all DNS records matching a zone.
///
/// Updates an annotation on all record types (`ARecord`, `TXTRecord`, etc.) that have
/// the `bindy.firestoned.io/zone` annotation matching the zone name. This causes
/// the record controllers to re-reconcile and re-add the records to BIND9.
///
/// This is critical after zone recreation (e.g., pod restarts) to ensure records
/// are re-added to the newly created zones.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for records
/// * `zone_name` - Zone FQDN to match
///
/// # Errors
///
/// Returns an error if patching records fails. Errors are logged but don't fail
/// the parent `DNSZone` reconciliation.
async fn trigger_record_reconciliation(
    client: &Client,
    namespace: &str,
    zone_name: &str,
) -> Result<()> {
    use crate::constants::ANNOTATION_ZONE_OWNER;
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, TXTRecord,
    };
    use chrono::Utc;

    debug!(
        "Triggering record reconciliation for zone {} in namespace {}",
        zone_name, namespace
    );

    // Annotation to update - timestamp triggers reconciliation without changing spec
    let timestamp = Utc::now().to_rfc3339();
    let patch_annotation = "bindy.firestoned.io/zone-reconciled-at";

    // Helper macro to patch all records of a given type
    macro_rules! trigger_records {
        ($record_type:ty, $type_name:expr) => {{
            let api: Api<$record_type> = Api::namespaced(client.clone(), namespace);
            let lp = ListParams::default();

            match api.list(&lp).await {
                Ok(records) => {
                    let matching: Vec<_> = records
                        .items
                        .iter()
                        .filter(|r| {
                            r.metadata
                                .annotations
                                .as_ref()
                                .and_then(|a| a.get(ANNOTATION_ZONE_OWNER))
                                == Some(&zone_name.to_string())
                        })
                        .collect();

                    debug!(
                        "Found {} {} record(s) for zone {}",
                        matching.len(),
                        $type_name,
                        zone_name
                    );

                    for record in matching {
                        let name = record.name_any();
                        let patch = json!({
                            "metadata": {
                                "annotations": {
                                    patch_annotation: timestamp.clone()
                                }
                            }
                        });

                        if let Err(e) = api
                            .patch(
                                &name,
                                &PatchParams::default(),
                                &Patch::Merge(&patch),
                            )
                            .await
                        {
                            warn!(
                                "Failed to trigger reconciliation for {} {}/{}: {}",
                                $type_name, namespace, name, e
                            );
                        } else {
                            debug!(
                                "Triggered reconciliation for {} {}/{}",
                                $type_name, namespace, name
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to list {} records in namespace {}: {}",
                        $type_name, namespace, e
                    );
                }
            }
        }};
    }

    // Trigger all record types
    trigger_records!(ARecord, "A");
    trigger_records!(AAAARecord, "AAAA");
    trigger_records!(TXTRecord, "TXT");
    trigger_records!(CNAMERecord, "CNAME");
    trigger_records!(MXRecord, "MX");
    trigger_records!(NSRecord, "NS");
    trigger_records!(SRVRecord, "SRV");
    trigger_records!(CAARecord, "CAA");

    Ok(())
}
