// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Validation logic for DNS zones.
//!
//! This module contains functions for validating zone configurations,
//! checking for duplicate zones, and filtering instances.

use anyhow::{anyhow, Result};
use kube::ResourceExt;
use tracing::{debug, warn};

use super::types::{ConflictingZone, DuplicateZoneInfo};
use crate::crd::DNSZone;

/// Get instances from a DNSZone based on `bind9_instances_from` selectors.
///
/// This function:
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

/// Checks if another zone has already claimed the same zone name across any BIND9 instances.
///
/// This function prevents multiple teams from creating conflicting zones with the same
/// fully qualified domain name (FQDN). A conflict exists if:
/// 1. Another DNSZone CR has the same `spec.zoneName`
/// 2. That zone is NOT the same resource (different namespace/name)
/// 3. That zone has at least one instance configured (status.bind9Instances is non-empty)
/// 4. Those instances have status != "Failed"
///
/// # Arguments
///
/// * `dnszone` - The DNSZone resource to check for duplicates
/// * `zones_store` - The reflector store containing all DNSZone resources
///
/// # Returns
///
/// * `Some(DuplicateZoneInfo)` - If a duplicate zone is detected, with details about conflicts
/// * `None` - If no duplicate exists (safe to proceed)
///
/// # Examples
///
/// ```rust,ignore
/// use tracing::warn;
/// use bindy::reconcilers::dnszone::check_for_duplicate_zones;
///
/// if let Some(duplicate_info) = check_for_duplicate_zones(&dnszone, &zones_store) {
///     warn!("Zone {} conflicts with existing zones: {:?}",
///           duplicate_info.zone_name, duplicate_info.conflicting_zones);
///     // Set status condition to DuplicateZone and stop processing
/// }
/// ```
pub fn check_for_duplicate_zones(
    dnszone: &DNSZone,
    zones_store: &kube::runtime::reflector::Store<DNSZone>,
) -> Option<DuplicateZoneInfo> {
    let current_namespace = dnszone.namespace().unwrap_or_default();
    let current_name = dnszone.name_any();
    let zone_name = &dnszone.spec.zone_name;

    debug!(
        "Checking for duplicate zones: current zone {}/{} claims {}",
        current_namespace, current_name, zone_name
    );

    let mut conflicting_zones = Vec::new();

    // Query all zones from the reflector store
    for other_zone in &zones_store.state() {
        let other_namespace = other_zone.namespace().unwrap_or_default();
        let other_name = other_zone.name_any();

        // Skip if this is the same zone (updating itself)
        if other_namespace == current_namespace && other_name == current_name {
            continue;
        }

        // Skip if zone name doesn't match
        if other_zone.spec.zone_name != *zone_name {
            continue;
        }

        // Check if other zone has instances configured
        let has_configured_instances = other_zone.status.as_ref().is_some_and(|status| {
            !status.bind9_instances.is_empty()
                && status.bind9_instances.iter().any(|inst| {
                    inst.status != crate::crd::InstanceStatus::Failed
                        && inst.status != crate::crd::InstanceStatus::Unclaimed
                })
        });

        if !has_configured_instances {
            debug!(
                "Zone {}/{} also uses {} but has no configured instances - not a conflict",
                other_namespace, other_name, zone_name
            );
            continue;
        }

        // This is a conflict - collect instance names
        let instance_names = other_zone
            .status
            .as_ref()
            .map(|status| {
                status
                    .bind9_instances
                    .iter()
                    .filter(|inst| {
                        inst.status != crate::crd::InstanceStatus::Failed
                            && inst.status != crate::crd::InstanceStatus::Unclaimed
                    })
                    .map(|inst| format!("{}/{}", inst.namespace, inst.name))
                    .collect()
            })
            .unwrap_or_default();

        warn!(
            "Duplicate zone detected: {}/{} already claims {} on instances: {:?}",
            other_namespace, other_name, zone_name, instance_names
        );

        conflicting_zones.push(ConflictingZone {
            name: other_name,
            namespace: other_namespace,
            instance_names,
        });
    }

    if conflicting_zones.is_empty() {
        None
    } else {
        Some(DuplicateZoneInfo {
            zone_name: zone_name.clone(),
            conflicting_zones,
        })
    }
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
#[must_use]
pub fn filter_instances_needing_reconciliation(
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

#[cfg(test)]
#[path = "validation_tests.rs"]
mod validation_tests;
