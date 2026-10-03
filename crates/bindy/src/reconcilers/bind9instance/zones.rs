// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Zone reconciliation logic for `Bind9Instance` resources.
//!
//! This module handles updating the instance status with zone references
//! when zones select this instance.

#[allow(clippy::wildcard_imports)]
use super::types::*;

use crate::constants::API_GROUP_VERSION;

/// Reconciles the zones list for a `Bind9Instance` based on current `DNSZone` state.
///
/// This function performs status-only updates by:
/// 1. Querying all `DNSZones` from the reflector store (in-memory, no API call)
/// 2. Filtering to zones that have this instance in their `status.bind9Instances`
/// 3. Patching the instance's `status.zones` field if changed
///
/// # Arguments
///
/// * `client` - Kubernetes API client for status patching
/// * `stores` - Reflector stores containing all `DNSZones`
/// * `instance` - The `Bind9Instance` to reconcile zones for
///
/// # Returns
///
/// * `Ok(())` - If zone reconciliation succeeded (or no change needed)
///
/// # Errors
///
/// Returns an error if the Kubernetes API status patch fails.
pub async fn reconcile_instance_zones(
    client: &Client,
    stores: &crate::context::Stores,
    instance: &Bind9Instance,
) -> Result<()> {
    let namespace = instance.namespace().unwrap_or_default();
    let instance_name = instance.name_any();

    // Get all DNSZones from reflector store (no API call)
    let all_zones = stores.dnszones.state();

    let mut new_zones = Vec::new();

    // Filter zones that have this instance in their status.bind9Instances
    for zone in &all_zones {
        let zone_namespace = zone.namespace().unwrap_or_default();

        // Only consider zones in the same namespace
        if zone_namespace != namespace {
            continue;
        }

        // Check if this instance is in the zone's status.bind9instances list
        if let Some(status) = &zone.status {
            let instance_found = status
                .bind9_instances
                .iter()
                .any(|inst_ref| inst_ref.name == instance_name && inst_ref.namespace == namespace);

            if instance_found {
                new_zones.push(ZoneReference {
                    api_version: API_GROUP_VERSION.to_string(),
                    kind: crate::constants::KIND_DNS_ZONE.to_string(),
                    name: zone.name_any(),
                    namespace: zone_namespace,
                    zone_name: zone.spec.zone_name.clone(),
                    last_reconciled_at: None, // Populated by DNSZone reconciler
                });
            }
        }
    }

    // Check if zones changed (avoid unnecessary patches)
    let current_zones = instance
        .status
        .as_ref()
        .map(|s| s.zones.clone())
        .unwrap_or_default();

    if zones_equal(&current_zones, &new_zones) {
        debug!(
            "Zones unchanged for Bind9Instance {}/{}, skipping status patch",
            namespace, instance_name
        );
        return Ok(());
    }

    // Patch status with new zones list and zones_count
    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), &namespace);

    let zones_count = i32::try_from(new_zones.len()).ok();

    let status_patch = serde_json::json!({
        "status": {
            "zones": new_zones,
            "zonesCount": zones_count
        }
    });

    api.patch_status(
        &instance_name,
        &PatchParams::default(),
        &Patch::Merge(&status_patch),
    )
    .await?;

    info!(
        "Updated zones for Bind9Instance {}/{}: {} zone(s)",
        namespace,
        instance_name,
        new_zones.len()
    );

    Ok(())
}

/// Compare two zone lists for equality (order-independent).
///
/// This helper function compares zone lists by content, not order.
/// Two lists are equal if they contain the same zones (by name and namespace).
pub(super) fn zones_equal(a: &[ZoneReference], b: &[ZoneReference]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    // Create sets of (name, namespace) tuples for comparison
    let set_a: std::collections::HashSet<_> = a.iter().map(|z| (&z.name, &z.namespace)).collect();
    let set_b: std::collections::HashSet<_> = b.iter().map(|z| (&z.name, &z.namespace)).collect();

    set_a == set_b
}

#[cfg(test)]
#[path = "zones_tests.rs"]
mod zones_tests;
