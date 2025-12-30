// Copyright (c) 2025 Erick Bourgeois, firestoned
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::doc_markdown)]
// SPDX-License-Identifier: MIT

//! DNS zone reconciliation logic.
//!
//! This module handles the creation and management of DNS zones on BIND9 servers.
//! It supports both primary and secondary zone configurations.

use crate::bind9::RndcKeyData;
// Bind9Instance and InstanceReferenceWithStatus are used by dead_code marked functions (Phase 2 cleanup)
#[allow(unused_imports)]
use crate::crd::{Condition, DNSZone, DNSZoneStatus};
use anyhow::{anyhow, Context as AnyhowContext, Result};
use bindcar::{ZONE_TYPE_PRIMARY, ZONE_TYPE_SECONDARY};
use futures::stream::{self, StreamExt};
use k8s_openapi::api::core::v1::{Endpoints, Pod, Secret, Service};
use kube::{
    api::{ListParams, Patch, PatchParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// - Uses the reflector store for O(1) lookups without API calls
/// - Single source of truth: `DNSZone` owns the zone-instance relationship
///
/// # Arguments
///
/// * `dnszone` - The `DNSZone` resource to get instances for
/// * `bind9_instances_store` - The reflector store for querying `Bind9Instance` resources
///
/// # Returns
///
/// * `Ok(Vec<InstanceReference>)` - List of instances serving this zone
/// * `Err(_)` - If no instances match the `bind9_instances_from` selectors
///
/// # Errors
///
/// Returns an error if no instances are found matching the label selectors.
pub fn get_instances_from_zone(
    dnszone: &DNSZone,
    bind9_instances_store: &kube::runtime::reflector::Store<crate::crd::Bind9Instance>,
) -> Result<Vec<crate::crd::InstanceReference>> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();

    // Get bind9_instances_from selectors from zone spec
    let bind9_instances_from = match &dnszone.spec.bind9_instances_from {
        Some(sources) if !sources.is_empty() => sources,
        _ => {
            return Err(anyhow!(
                "DNSZone {namespace}/{name} has no bind9_instances_from selectors configured. \
                Add spec.bind9_instances_from[] with label selectors to target Bind9Instance resources."
            ));
        }
    };

    // Query all instances from the reflector store and filter by label selectors
    let instances_with_zone: Vec<crate::crd::InstanceReference> = bind9_instances_store
        .state()
        .iter()
        .filter_map(|instance| {
            let instance_labels = instance.metadata.labels.as_ref()?;
            let instance_namespace = instance.namespace()?;
            let instance_name = instance.name_any();

            // Check if instance matches ANY of the bind9_instances_from selectors (OR logic)
            let matches = bind9_instances_from
                .iter()
                .any(|source| source.selector.matches(instance_labels));

            if matches {
                Some(crate::crd::InstanceReference {
                    api_version: "bindy.firestoned.io/v1beta1".to_string(),
                    kind: "Bind9Instance".to_string(),
                    name: instance_name,
                    namespace: instance_namespace,
                    last_reconciled_at: None,
                })
            } else {
                None
            }
        })
        .collect();

    if !instances_with_zone.is_empty() {
        debug!(
            "DNSZone {}/{} matched {} instances via spec.bind9_instances_from selectors",
            namespace,
            name,
            instances_with_zone.len()
        );
        return Ok(instances_with_zone);
    }

    // No instances found
    Err(anyhow!(
        "DNSZone {namespace}/{name} has no instances matching spec.bind9_instances_from selectors. \
        Verify that Bind9Instance resources exist with matching labels."
    ))
}

/// Filters instances that need reconciliation based on their `last_reconciled_at` timestamp.
///
/// Returns instances where:
/// - `last_reconciled_at` is `None` (never reconciled)
/// - `last_reconciled_at` exists but we need to verify pod IPs haven't changed
///
/// # Arguments
///
/// * `instances` - All instances assigned to the zone
///
/// # Returns
///
/// List of instances that need reconciliation (zone configuration)
fn filter_instances_needing_reconciliation(
    instances: &[crate::crd::InstanceReference],
) -> Vec<crate::crd::InstanceReference> {
    instances
        .iter()
        .filter(|instance| {
            // If never reconciled, needs reconciliation
            instance.last_reconciled_at.is_none()
        })
        .cloned()
        .collect()
}
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// Vector of instance references that have role=Primary
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail.
pub async fn filter_primary_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    use crate::crd::{Bind9Instance, ServerRole};

    let mut primary_refs = Vec::new();

    for instance_ref in instance_refs {
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        match instance_api.get(&instance_ref.name).await {
            Ok(instance) => {
                if instance.spec.role == ServerRole::Primary {
                    primary_refs.push(instance_ref.clone());
                }
            }
            Err(e) => {
                warn!(
                    "Failed to get instance {}/{}: {}. Skipping.",
                    instance_ref.namespace, instance_ref.name, e
                );
            }
        }
    }

    Ok(primary_refs)
}

/// Filters a list of instance references to only SECONDARY instances.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// Vector of instance references that have role=Secondary
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail.
pub async fn filter_secondary_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    use crate::crd::{Bind9Instance, ServerRole};

    let mut secondary_refs = Vec::new();

    for instance_ref in instance_refs {
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        match instance_api.get(&instance_ref.name).await {
            Ok(instance) => {
                if instance.spec.role == ServerRole::Secondary {
                    secondary_refs.push(instance_ref.clone());
                }
            }
            Err(e) => {
                warn!(
                    "Failed to get instance {}/{}: {}. Skipping.",
                    instance_ref.namespace, instance_ref.name, e
                );
            }
        }
    }

    Ok(secondary_refs)
}

/// Finds all pod IPs from a list of instance references, filtering by role.
///
/// Queries each `Bind9Instance` resource to determine its role, then collects
/// pod IPs only from secondary instances. This is event-driven as it reacts
/// to the current state of `Bind9Instance` resources rather than caching.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Instance references to query
///
/// # Returns
///
/// Vector of pod IP addresses from secondary instances only
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail.
pub async fn find_secondary_pod_ips_from_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<String>> {
    use crate::crd::{Bind9Instance, ServerRole};
    use k8s_openapi::api::core::v1::Pod;

    let mut secondary_ips = Vec::new();

    for instance_ref in instance_refs {
        // Query the Bind9Instance resource to check its role
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        let instance = match instance_api.get(&instance_ref.name).await {
            Ok(inst) => inst,
            Err(e) => {
                warn!(
                    "Failed to get Bind9Instance {}/{}: {}. Skipping.",
                    instance_ref.namespace, instance_ref.name, e
                );
                continue;
            }
        };

        // Only collect IPs from secondary instances
        if instance.spec.role != ServerRole::Secondary {
            debug!(
                "Skipping instance {}/{} - role is {:?}, not Secondary",
                instance_ref.namespace, instance_ref.name, instance.spec.role
            );
            continue;
        }

        // Find pods for this secondary instance
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), &instance_ref.namespace);
        let label_selector = format!("app=bind9,instance={}", instance_ref.name);
        let lp = ListParams::default().labels(&label_selector);

        match pod_api.list(&lp).await {
            Ok(pods) => {
                for pod in pods.items {
                    if let Some(pod_ip) = pod.status.as_ref().and_then(|s| s.pod_ip.as_ref()) {
                        // Check if pod is running
                        let phase = pod
                            .status
                            .as_ref()
                            .and_then(|s| s.phase.as_ref())
                            .map_or("Unknown", std::string::String::as_str);

                        if phase == "Running" {
                            secondary_ips.push(pod_ip.clone());
                        } else {
                            debug!(
                                "Skipping pod {} in phase {} for instance {}/{}",
                                pod.metadata.name.as_ref().unwrap_or(&"unknown".to_string()),
                                phase,
                                instance_ref.namespace,
                                instance_ref.name
                            );
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to list pods for instance {}/{}: {}. Skipping.",
                    instance_ref.namespace, instance_ref.name, e
                );
            }
        }
    }

    Ok(secondary_ips)
}

/// Generates nameserver IPs for a DNS zone from bind9 instances.
///
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

/// Iterates through all endpoints for the given instances and executes an operation on each.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Instance references to process
/// * `with_rndc_key` - Whether to load RNDC keys for each instance
/// * `port_name` - Port name to use ("http", "dns", etc.)
/// * `operation` - Async function to execute on each endpoint
///
/// # Returns
///
/// Tuple of (`first_endpoint`, `total_successful_endpoints`)
///
/// # Errors
///
/// Returns an error if all operations fail or if critical API calls fail.
pub async fn for_each_instance_endpoint<F, Fut>(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
    with_rndc_key: bool,
    port_name: &str,
    operation: F,
) -> Result<(Option<String>, usize)>
where
    F: Fn(String, String, Option<RndcKeyData>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut first_endpoint: Option<String> = None;
    let mut total_endpoints = 0;
    let mut errors: Vec<String> = Vec::new();

    for instance_ref in instance_refs {
        info!(
            "Processing endpoints for instance {}/{}",
            instance_ref.namespace, instance_ref.name
        );

        // Load RNDC key for this specific instance if requested
        let key_data = if with_rndc_key {
            Some(load_rndc_key(client, &instance_ref.namespace, &instance_ref.name).await?)
        } else {
            None
        };

        // Get all endpoints for this instance's service
        let endpoints = get_endpoint(
            client,
            &instance_ref.namespace,
            &instance_ref.name,
            port_name,
        )
        .await?;

        info!(
            "Found {} endpoint(s) for instance {}/{}",
            endpoints.len(),
            instance_ref.namespace,
            instance_ref.name
        );

        for endpoint in &endpoints {
            let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);

            // Save the first endpoint
            if first_endpoint.is_none() {
                first_endpoint = Some(pod_endpoint.clone());
            }

            // Execute the operation on this endpoint
            if let Err(e) = operation(
                pod_endpoint.clone(),
                instance_ref.name.clone(),
                key_data.clone(),
            )
            .await
            {
                error!(
                    "Failed operation on endpoint {} (instance {}/{}): {}",
                    pod_endpoint, instance_ref.namespace, instance_ref.name, e
                );
                errors.push(format!(
                    "endpoint {pod_endpoint} (instance {}/{}): {e}",
                    instance_ref.namespace, instance_ref.name
                ));
            } else {
                total_endpoints += 1;
            }
        }
    }

    // If ALL operations failed, return an error
    if total_endpoints == 0 && !errors.is_empty() {
        return Err(anyhow!(
            "All operations failed. Errors: {}",
            errors.join("; ")
        ));
    }

    Ok((first_endpoint, total_endpoints))
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
    let watch_event_instances = get_instances_from_zone(&dnszone, bind9_instances_store).ok();

    // CRITICAL: Re-fetch the zone to get the latest status
    // The `dnszone` parameter might have stale status from the cache/watch event
    // We need the latest status.bind9Instances which may have been updated by Bind9Instance reconciler
    let zones_api: Api<DNSZone> = Api::namespaced(client.clone(), &namespace);
    let dnszone = zones_api.get(&name).await?;

    // Create centralized status updater to batch all status changes
    let mut status_updater = crate::reconcilers::status::DNSZoneStatusUpdater::new(&dnszone);

    // Extract spec
    let spec = &dnszone.spec;

    // Validate that zone has instances assigned (via spec.bind9Instances or status.bind9Instances)
    // This will fail early if zone is not selected by any instance
    let instance_refs = get_instances_from_zone(&dnszone, bind9_instances_store)?;

    info!(
        "DNSZone {}/{} is assigned to {} instance(s): {:?}",
        namespace,
        name,
        instance_refs.len(),
        instance_refs.iter().map(|r| &r.name).collect::<Vec<_>>()
    );

    // Determine if this is the first reconciliation or if spec has changed
    let current_generation = dnszone.metadata.generation;
    let observed_generation = dnszone.status.as_ref().and_then(|s| s.observed_generation);

    let first_reconciliation = observed_generation.is_none();
    let spec_changed =
        crate::reconcilers::should_reconcile(current_generation, observed_generation);

    // Check if the instance list or lastReconciledAt timestamps changed between watch event and re-fetch
    // This is critical for detecting when:
    // 1. New instances are added to status.bind9Instances (via bind9InstancesFrom selectors)
    // 2. Instance lastReconciledAt timestamps are cleared (e.g., instance deleted, needs reconfiguration)
    //
    // NOTE: InstanceReference PartialEq ignores lastReconciledAt, so we must check timestamps separately!
    let instances_changed = if let Some(watch_instances) = &watch_event_instances {
        // Get the instance names from the watch event (what triggered us)
        let watch_instance_names: std::collections::HashSet<_> =
            watch_instances.iter().map(|r| &r.name).collect();

        // Get the instance names after re-fetching (current state)
        let current_instance_names: std::collections::HashSet<_> =
            instance_refs.iter().map(|r| &r.name).collect();

        // Check if instance list changed (added/removed instances)
        let list_changed = watch_instance_names != current_instance_names;

        if list_changed {
            info!(
                "Instance list changed during reconciliation for zone {}/{}: watch_event={:?}, current={:?}",
                namespace,
                name,
                watch_instance_names,
                current_instance_names
            );
            true
        } else {
            // List is the same, but check if any lastReconciledAt timestamps changed
            // Use InstanceReference as HashMap key (uses its Hash impl which hashes identity fields)
            let watch_timestamps: HashMap<&crate::crd::InstanceReference, Option<&str>> =
                watch_instances
                    .iter()
                    .map(|inst| (inst, inst.last_reconciled_at.as_deref()))
                    .collect();

            let current_timestamps: HashMap<&crate::crd::InstanceReference, Option<&str>> =
                instance_refs
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
    } else {
        // No instances in watch event, first reconciliation or error
        true
    };

    // Check if any instances need reconciliation (never reconciled or reconciliation failed)
    let unreconciled_instances = filter_instances_needing_reconciliation(&instance_refs);
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
    match cleanup_deleted_instances(&client, &dnszone, &mut status_updater).await {
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
    match cleanup_stale_records(
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
        // Find all primary instances from our instance refs and get their pod IPs
        let primary_ips = match find_primary_ips_from_instances(&client, &instance_refs).await {
            Ok(ips) if !ips.is_empty() => {
                info!(
                    "Found {} primary server IP(s) for zone {}/{}: {:?}",
                    ips.len(),
                    namespace,
                    spec.zone_name,
                    ips
                );
                ips
            }
            Ok(_) => {
                status_updater.set_condition(
                    "Degraded",
                    "True",
                    "PrimaryFailed",
                    "No primary servers found - cannot configure secondary zones",
                );
                // Apply status before returning error
                status_updater.apply(&client).await?;
                return Err(anyhow!(
                    "No primary servers found for zone {}/{} - cannot configure secondary zones",
                    namespace,
                    spec.zone_name
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
        // Primary instances are marked as reconciled inside add_dnszone() immediately after success
        // PHASE 2 OPTIMIZATION: Only process instances that need reconciliation (lastReconciledAt == None)
        let primary_count = match add_dnszone(
            ctx.clone(),
            dnszone.clone(),
            zone_manager,
            &mut status_updater,
            &unreconciled_instances,
        )
        .await
        {
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
        // Secondary instances are marked as reconciled inside add_dnszone_to_secondaries() immediately after success
        // PHASE 2 OPTIMIZATION: Only process instances that need reconciliation (lastReconciledAt == None)
        let secondary_count = match add_dnszone_to_secondaries(
            ctx.clone(),
            dnszone.clone(),
            zone_manager,
            &primary_ips,
            &mut status_updater,
            &unreconciled_instances,
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

    let records_count = record_refs.len();

    // Update DNSZone status with discovered records (in-memory)
    status_updater.set_records(&record_refs);

    // Check if all discovered records are ready and trigger zone transfers if needed
    if records_count > 0 {
        let all_records_ready = check_all_records_ready(&client, &namespace, &record_refs).await?;

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

    // Set observed generation (in-memory)
    status_updater.set_observed_generation(current_generation);

    // Calculate expected counts to validate all instances were configured
    let expected_primary_count = filter_primary_instances(&client, &instance_refs)
        .await
        .map(|refs| refs.len())
        .unwrap_or(0);
    let expected_secondary_count = filter_secondary_instances(&client, &instance_refs)
        .await
        .map(|refs| refs.len())
        .unwrap_or(0);

    // Set final Ready/Degraded status based on reconciliation outcome
    // Only set Ready=True if there were NO degraded conditions during reconciliation
    // AND all expected instances were successfully configured
    if status_updater.has_degraded_condition() {
        // Keep the Degraded condition that was already set, don't overwrite with Ready
        info!(
            "DNSZone {}/{} reconciliation completed with degraded state - will retry faster",
            namespace, name
        );
    } else if primary_count < expected_primary_count || secondary_count < expected_secondary_count {
        // Not all instances were configured - set Degraded condition
        status_updater.set_condition(
            "Degraded",
            "True",
            "PartialReconciliation",
            &format!(
                "Zone {} configured on {}/{} primary and {}/{} secondary instance(s) - {} instance(s) pending",
                spec.zone_name,
                primary_count,
                expected_primary_count,
                secondary_count,
                expected_secondary_count,
                (expected_primary_count - primary_count) + (expected_secondary_count - secondary_count)
            ),
        );
        info!(
            "DNSZone {}/{} partially configured: {}/{} primaries, {}/{} secondaries",
            namespace,
            name,
            primary_count,
            expected_primary_count,
            secondary_count,
            expected_secondary_count
        );
    } else {
        // All reconciliation steps succeeded - set Ready status and clear any stale Degraded condition
        status_updater.set_condition(
            "Ready",
            "True",
            "ReconcileSucceeded",
            &format!(
                "Zone {} configured on {} primary and {} secondary instance(s), discovered {} DNS record(s)",
                spec.zone_name, primary_count, secondary_count, records_count
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
    let primary_instance_refs = filter_primary_instances(&client, instance_refs).await?;

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
    let secondary_instance_refs = filter_secondary_instances(&client, instance_refs).await?;
    let secondary_ips =
        find_secondary_pod_ips_from_instances(&client, &secondary_instance_refs).await?;

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
            let zone_namespace = namespace.clone();
            let zone_name_ref = name.clone();

            async move {
                info!(
                    "Processing endpoints for primary instance {}/{}",
                    instance_ref.namespace, instance_ref.name
                );

                // Load RNDC key for this specific instance
                let key_data = match load_rndc_key(&client, &instance_ref.namespace, &instance_ref.name).await {
                    Ok(key) => key,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to load RNDC key: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return;
                    }
                };

                // Get all endpoints for this instance
                let endpoints = match get_endpoint(&client, &instance_ref.namespace, &instance_ref.name, "http").await {
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
                                    Ok(())
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
                    .collect::<Vec<Result<(), ()>>>()
                    .await;

                // Mark this instance as configured if at least one endpoint succeeded
                if endpoint_results.iter().any(Result::is_ok) {
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
                    update_zone_reconciled_timestamp(
                        &client,
                        &instance_ref.name,
                        &instance_ref.namespace,
                        &zone_name_ref,
                        &zone_namespace,
                    );
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
    let secondary_instance_refs = filter_secondary_instances(&client, instance_refs).await?;

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
            let zone_namespace = namespace.clone();
            let zone_name_ref = name.clone();

            async move {
                info!(
                    "Processing secondary instance {}/{} for zone {}",
                    instance_ref.namespace, instance_ref.name, zone_name
                );

                // Load RNDC key for this specific instance
                // Each instance has its own RNDC secret for security isolation
                let key_data = match load_rndc_key(&client, &instance_ref.namespace, &instance_ref.name).await {
                    Ok(key) => key,
                    Err(e) => {
                        let err_msg = format!("instance {}/{}: failed to load RNDC key: {e}", instance_ref.namespace, instance_ref.name);
                        errors.lock().await.push(err_msg);
                        return;
                    }
                };

                // Get all endpoints for this secondary instance
                let endpoints = match get_endpoint(&client, &instance_ref.namespace, &instance_ref.name, "http").await {
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
                                    Ok(())
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
                    .collect::<Vec<Result<(), ()>>>()
                    .await;

                // Mark this instance as configured if at least one endpoint succeeded
                if endpoint_results.iter().any(Result::is_ok) {
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
                    update_zone_reconciled_timestamp(
                        &client,
                        &instance_ref.name,
                        &instance_ref.namespace,
                        &zone_name_ref,
                        &zone_namespace,
                    );
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

/// Reconciles DNS records for a zone by discovering records that match the zone's label selectors.
///
/// **Event-Driven Architecture**: This function implements the core of the zone/record ownership model:
/// 1. Discovers records matching the zone's `recordsFrom` label selectors
/// 2. Tags matched records by setting `status.zoneRef` (triggers record reconciliation via watches)
/// 3. Untags previously matched records by clearing `status.zoneRef` (stops record reconciliation)
/// 4. Returns references to currently matched records for `DNSZone.status.records` tracking
///
/// Record reconcilers watch `status.zoneRef` to determine which zone they belong to.
/// When `status.zoneRef` is set, the record is reconciled to BIND9.
/// When `status.zoneRef` is cleared, the record reconciler marks it as `"NotSelected"`.
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
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
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
        all_record_refs.extend(discover_a_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_aaaa_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_txt_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_cname_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_mx_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_ns_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_srv_records(&client, &namespace, selector, zone_name).await?);
        all_record_refs
            .extend(discover_caa_records(&client, &namespace, selector, zone_name).await?);
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

    // Tag all matched records to ensure status.zoneRef is set
    // Previously we only tagged "newly matched" records, but records can exist in
    // status.records without having status.zoneRef set (e.g., from a previous
    // implementation or migration). Always tag to ensure consistency.
    for record_ref in &all_record_refs {
        let record_key = format!("{}/{}", record_ref.kind, record_ref.name);
        let is_new = !previous_records.contains(&record_key);

        if is_new {
            info!(
                "Newly matched record: {} {}/{}",
                record_ref.kind, namespace, record_ref.name
            );
        } else {
            debug!(
                "Re-tagging existing record to ensure status.zoneRef: {} {}/{}",
                record_ref.kind, namespace, record_ref.name
            );
        }

        tag_record_with_zone(
            &client,
            &namespace,
            &record_ref.kind,
            &record_ref.name,
            zone_name,
            &dnszone,
        )
        .await?;
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

    // CRITICAL: Preserve existing timestamps for records that haven't changed
    // This prevents status updates from triggering unnecessary reconciliation loops
    if let Some(status) = &dnszone.status {
        let existing_timestamps: std::collections::HashMap<String, _> = status
            .records
            .iter()
            .filter_map(|r| {
                r.last_reconciled_at
                    .as_ref()
                    .map(|timestamp| (format!("{}/{}", r.kind, r.name), timestamp.clone()))
            })
            .collect();

        // Update timestamps for records that already existed
        for record_ref in &mut all_record_refs {
            let key = format!("{}/{}", record_ref.kind, record_ref.name);
            if let Some(existing_timestamp) = existing_timestamps.get(&key) {
                record_ref.last_reconciled_at = Some(existing_timestamp.clone());
            }
        }
    }

    Ok(all_record_refs)
}

/// Tags a DNS record with zone ownership by setting `status.zoneRef`.
///
/// **Event-Driven Architecture**: This function is called when a `DNSZone`'s label selector
/// matches a record. It sets `status.zoneRef` with a structured reference to the zone,
/// which triggers the record controller via Kubernetes watch to reconcile the record to BIND9.
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
    dnszone: &DNSZone,
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

    // Create ZoneReference for status.zoneRef (event-driven architecture)
    let zone_ref = crate::crd::ZoneReference {
        api_version: crate::constants::API_GROUP_VERSION.to_string(),
        kind: crate::constants::KIND_DNS_ZONE.to_string(),
        name: dnszone.name_any(),
        namespace: dnszone.namespace().unwrap_or_default(),
        zone_name: zone_fqdn.to_string(),
        last_reconciled_at: None, // Not used in DNSZone status
    };

    // Patch status to set zone field (backward compatibility) AND zoneRef (new event-driven field)
    let status_patch = json!({
        "status": {
            "zone": zone_fqdn,
            "zoneRef": zone_ref
        }
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .with_context(|| {
            format!("Failed to set status.zone and status.zoneRef on {kind} {namespace}/{name}")
        })?;

    info!(
        "Successfully tagged {} {}/{} with zone {} (set status.zoneRef)",
        kind, namespace, name, zone_fqdn
    );

    Ok(())
}

/// Untags a DNS record that no longer matches a zone's selector.
///
/// This function clears the `status.zoneRef` field (event-driven architecture)
/// and the deprecated `status.zone` field for backward compatibility.
///
/// **Event-Driven Architecture**: Records use `status.zoneRef` (not annotations) to track
/// which zone they belong to. When a record no longer matches a zone's selector, this
/// function clears the status fields so the record reconciler knows it's no longer selected.
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
        "Untagging {} {}/{} from zone {} (clearing status.zoneRef)",
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

    // Patch status to remove zoneRef (event-driven architecture uses status.zoneRef, not annotations)
    let status_patch = json!({
        "status": {
            "zoneRef": null,
            "zone": null  // Also clear deprecated zone field for backward compatibility
        }
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .with_context(|| format!("Failed to clear status.zoneRef on {kind} {namespace}/{name}"))?;

    info!(
        "Successfully untagged {} {}/{} from zone {} (cleared status.zoneRef)",
        kind, namespace, name, previous_zone_fqdn
    );

    Ok(())
}

/// Helper function to discover A records matching a label selector.
async fn discover_a_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    use crate::crd::{ARecord, DNSRecordKind};
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

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .status
            .as_ref()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                // Parse ISO8601 timestamp string into k8s Time
                chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                        dt.with_timezone(&chrono::Utc),
                    )
                })
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: DNSRecordKind::A.as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec.name.clone()),
            last_reconciled_at,
        });
    }

    Ok(record_refs)
}

/// Helper function to discover AAAA records matching a label selector.
async fn discover_aaaa_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    use crate::crd::{AAAARecord, DNSRecordKind};
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

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .status
            .as_ref()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                        dt.with_timezone(&chrono::Utc),
                    )
                })
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: DNSRecordKind::AAAA.as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec.name.clone()),
            last_reconciled_at,
        });
    }

    Ok(record_refs)
}

/// Helper function to discover TXT records matching a label selector.
async fn discover_txt_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    use crate::crd::{DNSRecordKind, TXTRecord};
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

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .status
            .as_ref()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                        dt.with_timezone(&chrono::Utc),
                    )
                })
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: DNSRecordKind::TXT.as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec.name.clone()),
            last_reconciled_at,
        });
    }

    Ok(record_refs)
}

/// Helper function to discover CNAME records matching a label selector.
async fn discover_cname_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    use crate::crd::{CNAMERecord, DNSRecordKind};
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

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .status
            .as_ref()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                        dt.with_timezone(&chrono::Utc),
                    )
                })
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: DNSRecordKind::CNAME.as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec.name.clone()),
            last_reconciled_at,
        });
    }

    Ok(record_refs)
}

/// Helper function to discover MX records matching a label selector.
async fn discover_mx_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    use crate::crd::{DNSRecordKind, MXRecord};
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

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .status
            .as_ref()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                        dt.with_timezone(&chrono::Utc),
                    )
                })
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: DNSRecordKind::MX.as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec.name.clone()),
            last_reconciled_at,
        });
    }

    Ok(record_refs)
}

/// Helper function to discover NS records matching a label selector.
async fn discover_ns_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    use crate::crd::{DNSRecordKind, NSRecord};
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

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .status
            .as_ref()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                        dt.with_timezone(&chrono::Utc),
                    )
                })
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: DNSRecordKind::NS.as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec.name.clone()),
            last_reconciled_at,
        });
    }

    Ok(record_refs)
}

/// Helper function to discover SRV records matching a label selector.
async fn discover_srv_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    use crate::crd::{DNSRecordKind, SRVRecord};
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

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .status
            .as_ref()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                        dt.with_timezone(&chrono::Utc),
                    )
                })
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: DNSRecordKind::SRV.as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec.name.clone()),
            last_reconciled_at,
        });
    }

    Ok(record_refs)
}

/// Helper function to discover CAA records matching a label selector.
async fn discover_caa_records(
    client: &Client,
    namespace: &str,
    selector: &crate::crd::LabelSelector,
    _zone_name: &str,
) -> Result<Vec<crate::crd::RecordReferenceWithTimestamp>> {
    use crate::crd::{CAARecord, DNSRecordKind};
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

        // Preserve existing last_updated timestamp if record was previously reconciled
        let last_reconciled_at = record
            .status
            .as_ref()
            .and_then(|s| s.last_updated.as_ref())
            .and_then(|ts| {
                chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                        dt.with_timezone(&chrono::Utc),
                    )
                })
            });

        record_refs.push(crate::crd::RecordReferenceWithTimestamp {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: DNSRecordKind::CAA.as_str().to_string(),
            name: record.name_any(),
            namespace: namespace.to_string(),
            record_name: Some(record.spec.name.clone()),
            last_reconciled_at,
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
    let instance_refs = match get_instances_from_zone(&dnszone, bind9_instances_store) {
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
    let primary_instance_refs = filter_primary_instances(&client, &instance_refs).await?;
    let secondary_instance_refs = filter_secondary_instances(&client, &instance_refs).await?;

    // Delete from all primary instances
    if !primary_instance_refs.is_empty() {
        let (_first_endpoint, total_endpoints) = for_each_instance_endpoint(
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
                get_endpoint(&client, &instance_ref.namespace, &instance_ref.name, "http").await?;

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
/// Helper struct for tracking pod information during reconciliation.
/// Used by pod discovery operations.
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
/// Find all PRIMARY pod IPs for a given cluster or global cluster.
///
/// Returns IP addresses of all running primary pods in the cluster.
/// Find primary server IPs from a list of instance references.
///
/// This is the NEW instance-based approach that replaces cluster-based lookup.
/// It filters the instance refs to only PRIMARY instances, then gets their pod IPs.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - List of instance references to search
///
/// # Returns
///
/// A vector of IP addresses for all running PRIMARY pods across all primary instances
async fn find_primary_ips_from_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<String>> {
    use crate::crd::{Bind9Instance, ServerRole};
    use k8s_openapi::api::core::v1::Pod;

    info!(
        "Finding PRIMARY pod IPs from {} instance reference(s)",
        instance_refs.len()
    );

    let mut primary_ips = Vec::new();

    for instance_ref in instance_refs {
        // Get the Bind9Instance to check its role
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        let instance = match instance_api.get(&instance_ref.name).await {
            Ok(inst) => inst,
            Err(e) => {
                warn!(
                    "Failed to get instance {}/{}: {}",
                    instance_ref.namespace, instance_ref.name, e
                );
                continue;
            }
        };

        // Skip if not a PRIMARY instance
        if instance.spec.role != ServerRole::Primary {
            continue;
        }

        // Get running pod IPs for this primary instance
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), &instance_ref.namespace);
        let label_selector = format!("app=bind9,instance={}", instance_ref.name);
        let lp = ListParams::default().labels(&label_selector);

        match pod_api.list(&lp).await {
            Ok(pods) => {
                for pod in pods.items {
                    if let Some(pod_ip) = pod.status.as_ref().and_then(|s| s.pod_ip.as_ref()) {
                        // Check if pod is running
                        let phase = pod
                            .status
                            .as_ref()
                            .and_then(|s| s.phase.as_ref())
                            .map_or("Unknown", std::string::String::as_str);

                        if phase == "Running" {
                            primary_ips.push(pod_ip.clone());
                            debug!(
                                "Added IP {} from running PRIMARY pod {} (instance {}/{})",
                                pod_ip,
                                pod.metadata.name.as_ref().unwrap_or(&"unknown".to_string()),
                                instance_ref.namespace,
                                instance_ref.name
                            );
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to list pods for PRIMARY instance {}/{}: {}",
                    instance_ref.namespace, instance_ref.name, e
                );
            }
        }
    }

    info!(
        "Found total of {} PRIMARY pod IP(s) across all instances: {:?}",
        primary_ips.len(),
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

/// Update `lastReconciledAt` timestamp for a zone in `Bind9Instance.status.selectedZones[]`.
///
/// This function implements the critical Phase 2 completion step: after successfully
/// configuring a zone on an instance, we update the instance's status to signal that
/// the zone is now reconciled and doesn't need reconfiguration on future reconciliations.
///
/// This prevents infinite reconciliation loops by ensuring the `DNSZone` watch mapper
/// only triggers reconciliation when `lastReconciledAt == None`.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_name` - Name of the `Bind9Instance`
/// * `instance_namespace` - Namespace of the `Bind9Instance`
/// * `zone_name` - Name of the `DNSZone` resource
/// * `zone_namespace` - Namespace of the `DNSZone` resource
///
/// # Returns
///
/// * `Ok(())` - If timestamp update succeeded
/// * `Err(_)` - If instance fetch or status patching failed
///
/// # Errors
///
/// Returns an error if:
/// - Instance cannot be fetched from Kubernetes API
/// - Zone is not found in instance's `selectedZones[]` array
/// - Status patch operation fails
fn update_zone_reconciled_timestamp(
    _client: &Client,
    instance_name: &str,
    instance_namespace: &str,
    zone_name: &str,
    zone_namespace: &str,
) {
    // REMOVED: Instances no longer track selected_zones in status
    // TODO: Implement new timestamp tracking mechanism in DNSZone.status.bind9Instances
    // For now, this function is a no-op
    debug!(
        "STUB: update_zone_reconciled_timestamp for zone {}/{} on instance {}/{} - logic removed",
        zone_namespace, zone_name, instance_namespace, instance_name
    );
}

/// Get all secondary pod IPs for a primary instance from a list of zone instances.
///
/// Filters the provided instances to find all secondary instances (excluding the current instance),
/// then queries the Kubernetes API to get their running pod IPs.
///
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

/// Helper struct for tracking endpoint information (pod IP and port).
/// Used by endpoint management operations.
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
    record_refs: &[crate::crd::RecordReferenceWithTimestamp],
) -> Result<bool> {
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, DNSRecordKind, MXRecord, NSRecord, SRVRecord,
        TXTRecord,
    };

    for record_ref in record_refs {
        let kind = DNSRecordKind::from(record_ref.kind.as_str());
        let is_ready = match kind {
            DNSRecordKind::A => {
                let api: Api<ARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::AAAA => {
                let api: Api<AAAARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::TXT => {
                let api: Api<TXTRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::CNAME => {
                let api: Api<CNAMERecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::MX => {
                let api: Api<MXRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::NS => {
                let api: Api<NSRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::SRV => {
                let api: Api<SRVRecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
            }
            DNSRecordKind::CAA => {
                let api: Api<CAARecord> = Api::namespaced(client.clone(), namespace);
                check_record_ready(&api, &record_ref.name).await?
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
/// Counts DNS records matching a zone for logging purposes.
///
/// **Event-Driven Architecture**: This function only counts and logs records that have
/// `status.zoneRef.zoneName` matching the zone. The actual reconciliation is triggered
/// automatically by Kubernetes watches - when the `DNSZone` status changes, record controllers
/// are notified via watch events and reconcile automatically.
///
/// This function is called after zone recreation (e.g., pod restarts) to log how many
/// records will be automatically reconciled via the event-driven architecture.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace to search for records
/// * `zone_name` - Zone FQDN to match
///
/// # Errors
///
/// Returns an error if listing records fails. Errors are logged but don't fail
/// the parent `DNSZone` reconciliation.
async fn trigger_record_reconciliation(
    client: &Client,
    namespace: &str,
    zone_name: &str,
) -> Result<()> {
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, TXTRecord,
    };

    debug!(
        "Triggering record reconciliation for zone {} in namespace {}",
        zone_name, namespace
    );

    // Helper macro to count records by status.zoneRef
    // Note: We don't need to patch anything - the event-driven architecture (watches)
    // will automatically trigger reconciliation when records see zone status changes
    macro_rules! count_records {
        ($record_type:ty, $type_name:expr) => {{
            let api: Api<$record_type> = Api::namespaced(client.clone(), namespace);
            let lp = ListParams::default();

            match api.list(&lp).await {
                Ok(records) => {
                    let matching_count = records
                        .items
                        .iter()
                        .filter(|r| {
                            // Check if status.zoneRef.zoneName matches
                            r.status
                                .as_ref()
                                .and_then(|s| s.zone_ref.as_ref())
                                .map(|zr| zr.zone_name == zone_name)
                                .unwrap_or(false)
                        })
                        .count();

                    debug!(
                        "Found {} {} record(s) for zone {} (event-driven watches will trigger reconciliation)",
                        matching_count,
                        $type_name,
                        zone_name
                    );
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

    // Count records for each type (event-driven watches will trigger reconciliation automatically)
    count_records!(ARecord, "A");
    count_records!(AAAARecord, "AAAA");
    count_records!(TXTRecord, "TXT");
    count_records!(CNAMERecord, "CNAME");
    count_records!(MXRecord, "MX");
    count_records!(NSRecord, "NS");
    count_records!(SRVRecord, "SRV");
    count_records!(CAARecord, "CAA");

    Ok(())
}

/// Clean up stale records from `DNSZone` status with BIND9 self-healing.
///
/// This function provides full self-healing for DNS records by:
/// 1. Checking if each record in `status.records[]` still exists in Kubernetes API
/// 2. If record doesn't exist in K8s: queries BIND9 to verify DNS record was deleted
/// 3. If DNS record still exists in BIND9: deletes it (self-healing from race conditions/bugs)
/// 4. Removes stale entry from `status.records[]`
///
/// This catches edge cases where the record finalizer failed to delete from BIND9 due to:
/// - Race conditions during controller restart
/// - Controller crashes during deletion
/// - Finalizer bugs that skipped deletion
/// - Manual changes to BIND9
///
/// # Important Note
///
/// The DNS record name is stored in `RecordReference.record_name` (from `spec.name`) and
/// the zone name in `RecordReference.zone_name`. These fields are populated when records are
/// discovered. If a record reference is missing these fields (old status format), the BIND9
/// cleanup is skipped for that record, but it's still removed from status.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `dnszone` - The `DNSZone` resource
/// * `status_updater` - Status updater for modifying `status.records[]`
///
/// # Returns
///
/// Returns the number of stale records removed from status.
/// Cleans up deleted instances from `status.bind9Instances`.
///
/// Checks each instance in `status.bind9Instances` to see if the `Bind9Instance` still exists.
/// If an instance has been deleted, clears its `lastReconciledAt` timestamp to indicate
/// the zone is no longer configured on that instance.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `dnszone` - The `DNSZone` resource
/// * `status_updater` - Status updater to apply changes
///
/// # Returns
///
/// * `Ok(usize)` - Number of deleted instances found and cleared
/// * `Err(_)` - If API calls fail critically
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail (except `NotFound` errors, which are expected).
pub async fn cleanup_deleted_instances(
    client: &Client,
    dnszone: &DNSZone,
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
) -> Result<usize> {
    use crate::crd::Bind9Instance;

    let namespace = dnszone.namespace().unwrap_or_default();
    let zone_name = &dnszone.spec.zone_name;

    // Get current instances from status
    let current_instances = dnszone
        .status
        .as_ref()
        .map(|s| s.bind9_instances.clone())
        .unwrap_or_default();

    if current_instances.is_empty() {
        debug!(
            "No instances in status for zone {}/{} - skipping cleanup",
            namespace, zone_name
        );
        return Ok(0);
    }

    info!(
        "Cleaning up deleted instances for zone {}/{}: checking {} instance(s)",
        namespace,
        zone_name,
        current_instances.len()
    );

    let mut deleted_count = 0;

    // Check each instance to see if it still exists
    for instance_ref in current_instances {
        let instance_api: Api<Bind9Instance> =
            Api::namespaced(client.clone(), &instance_ref.namespace);

        let instance_exists = instance_api.get(&instance_ref.name).await.is_ok();

        if !instance_exists {
            info!(
                "Instance {}/{} no longer exists - removing from zone {}/{}",
                instance_ref.namespace, instance_ref.name, namespace, zone_name
            );
            status_updater.remove_instance(&instance_ref.name, &instance_ref.namespace);
            deleted_count += 1;
        }
    }

    Ok(deleted_count)
}

///
/// # Errors
///
/// Returns an error if API calls fail critically (non-NotFound errors).
#[allow(clippy::too_many_lines)]
pub async fn cleanup_stale_records(
    client: &Client,
    dnszone: &DNSZone,
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
    bind9_instances_store: &kube::runtime::reflector::Store<crate::crd::Bind9Instance>,
) -> Result<usize> {
    use crate::bind9::records::query_dns_record;
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, DNSRecordKind, MXRecord, NSRecord,
        RecordReferenceWithTimestamp, SRVRecord, TXTRecord,
    };

    let namespace = dnszone.namespace().unwrap_or_default();
    let zone_name = &dnszone.spec.zone_name;

    // Get current records from status
    let current_records = dnszone
        .status
        .as_ref()
        .map(|s| s.records.clone())
        .unwrap_or_default();

    if current_records.is_empty() {
        debug!(
            "No records in status for zone {}/{} - skipping cleanup",
            namespace, zone_name
        );
        return Ok(0);
    }

    info!(
        "Cleaning up stale records for zone {}/{}: checking {} record(s)",
        namespace,
        zone_name,
        current_records.len()
    );

    // Get instances to query DNS and delete if needed
    let instance_refs = get_instances_from_zone(dnszone, bind9_instances_store)?;
    let primary_refs = filter_primary_instances(client, &instance_refs).await?;

    let mut records_to_keep: Vec<RecordReferenceWithTimestamp> = Vec::new();
    let mut stale_count = 0;

    // Check each record to see if it still exists
    for record_ref in current_records {
        let kind = DNSRecordKind::from(record_ref.kind.as_str());
        let record_exists = match kind {
            DNSRecordKind::A => {
                let api: Api<ARecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::AAAA => {
                let api: Api<AAAARecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::TXT => {
                let api: Api<TXTRecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::CNAME => {
                let api: Api<CNAMERecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::MX => {
                let api: Api<MXRecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::NS => {
                let api: Api<NSRecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::SRV => {
                let api: Api<SRVRecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
            DNSRecordKind::CAA => {
                let api: Api<CAARecord> = Api::namespaced(client.clone(), &record_ref.namespace);
                api.get(&record_ref.name).await.is_ok()
            }
        };

        if record_exists {
            // Record still exists in Kubernetes - keep it in status
            // The record reconciler will handle updating BIND9
            debug!(
                "Record {} {}/{} still exists - keeping in status",
                record_ref.kind, record_ref.namespace, record_ref.name
            );
            records_to_keep.push(record_ref);
        } else {
            // Record doesn't exist in Kubernetes - need to clean up
            info!(
                "Record {} {}/{} no longer exists in Kubernetes",
                record_ref.kind, record_ref.namespace, record_ref.name
            );

            // Self-healing: Check if record still exists in BIND9 and delete if found
            // This catches cases where the finalizer failed to delete
            let kind = DNSRecordKind::from(record_ref.kind.as_str());
            let record_type = kind.to_hickory_record_type();

            // Extract DNS record name and zone from RecordReference
            // These fields are populated from spec.name when the record is discovered
            let dns_record_name = if let Some(name) = &record_ref.record_name {
                name.as_str()
            } else {
                warn!(
                    "Record {} {}/{} has no recordName in status - skipping BIND9 cleanup",
                    record_ref.kind, record_ref.namespace, record_ref.name
                );
                stale_count += 1;
                continue;
            };

            // Check BIND9 on all primary instances and delete if found
            // Use for_each_instance_endpoint to iterate over all primary endpoints
            let dns_record_name_clone = dns_record_name.to_string();
            let dns_zone_name_clone = zone_name.clone();
            let record_kind = record_ref.kind.clone();
            let record_namespace = record_ref.namespace.clone();
            let record_name = record_ref.name.clone();

            // Query and potentially delete from each primary instance
            let _ = for_each_instance_endpoint(
                client,
                &primary_refs,
                true,      // with_rndc_key (needed for deletion)
                "dns-tcp", // Use DNS TCP port for queries and updates
                |pod_endpoint, _instance_name, rndc_key| {
                    let server = pod_endpoint.clone();
                    let zone = dns_zone_name_clone.clone();
                    let dns_name = dns_record_name_clone.clone();
                    let r_type = record_type;
                    let r_kind = record_kind.clone();
                    let r_namespace = record_namespace.clone();
                    let r_name = record_name.clone();

                    async move {
                        // Query DNS to check if record exists
                        match query_dns_record(&zone, &dns_name, r_type, &server).await {
                            Ok(records) if !records.is_empty() => {
                                warn!(
                                    "SELF-HEALING: Record {} {}/{} deleted from K8s but still exists in BIND9 on {}",
                                    r_kind, r_namespace, r_name, server
                                );

                                // Delete from BIND9 using the RNDC key
                                if let Some(key_data) = rndc_key {
                                    match crate::bind9::records::delete_dns_record(
                                        &zone,
                                        &dns_name,
                                        r_type,
                                        &server,
                                        &key_data,
                                    )
                                    .await
                                    {
                                        Ok(()) => {
                                            info!(
                                                "SELF-HEALING: Successfully deleted orphaned {} record {} from BIND9 on {}",
                                                r_kind, dns_name, server
                                            );
                                        }
                                        Err(e) => {
                                            warn!(
                                                "SELF-HEALING: Failed to delete orphaned record from BIND9 on {}: {}",
                                                server, e
                                            );
                                        }
                                    }
                                } else {
                                    warn!(
                                        "No RNDC key available for {} - cannot delete orphaned record",
                                        server
                                    );
                                }
                            }
                            Ok(_) => {
                                // Record doesn't exist in BIND9 - good, finalizer worked
                                debug!(
                                    "Record {} not found in BIND9 on {} - already cleaned up",
                                    dns_name, server
                                );
                            }
                            Err(e) => {
                                debug!(
                                    "Failed to query DNS on {} for {} (may not exist): {}",
                                    server, dns_name, e
                                );
                            }
                        }

                        Ok(())
                    }
                },
            )
            .await;

            // Remove from status regardless of whether we found it in BIND9
            stale_count += 1;
        }
    }

    // Update status with cleaned records list
    if stale_count > 0 {
        status_updater.set_records(&records_to_keep);
        info!(
            "Removed {} stale record(s) from zone {}/{} status",
            stale_count, namespace, zone_name
        );
    }

    Ok(stale_count)
}
