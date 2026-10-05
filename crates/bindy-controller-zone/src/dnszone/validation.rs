// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Validation logic for DNS zones.
//!
//! This module contains functions for validating zone configurations,
//! checking for duplicate zones, and filtering instances.

use kube::ResourceExt;
use tracing::{debug, warn};

use super::types::{ConflictingZone, DuplicateZoneInfo};
use crate::crd::DNSZone;

// Moved to `bindy-bind9` (ADR-0009 §2, amended 2026-10-05).
pub use bindy_bind9::instances::get_instances_from_zone;

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
    zones_store: &crate::context::MultiStore<DNSZone>,
) -> Option<DuplicateZoneInfo> {
    let current_namespace = dnszone.namespace().unwrap_or_default();
    let current_name = dnszone.name_any();
    let zone_name = &dnszone.spec.zone_name;

    debug!(
        "Checking for duplicate zones: current zone {}/{} claims {}",
        current_namespace, current_name, zone_name
    );

    // F-003 mitigation: switch the duplicate check from a status-based gate
    // to a spec-based one. The previous implementation only flagged a
    // conflict if the *other* zone had `status.bind9_instances` non-empty
    // and at least one instance not in `Failed`/`Unclaimed`. That left
    // every race window open: a tenant who created their malicious zone
    // *first*, before the legitimate zone reached `Configured` state,
    // would claim the zoneName uncontested, and the legitimate zone would
    // never reconcile. We now compare on `spec.zoneName` directly and use
    // creation timestamp to break ties: the *older* CR wins.
    let current_creation = dnszone.metadata.creation_timestamp.as_ref();

    let mut conflicting_zones = Vec::new();

    for other_zone in &zones_store.state() {
        let other_namespace = other_zone.namespace().unwrap_or_default();
        let other_name = other_zone.name_any();

        // Skip if this is the same zone (updating itself).
        if other_namespace == current_namespace && other_name == current_name {
            continue;
        }

        // Skip if zone name doesn't match.
        if other_zone.spec.zone_name != *zone_name {
            continue;
        }

        // Tie-break by creation timestamp: the *older* CR keeps the
        // zoneName; the newer one is the conflict. If timestamps are
        // missing or equal, fall back to a stable lexicographic order on
        // (namespace, name) so the result is deterministic.
        let other_creation = other_zone.metadata.creation_timestamp.as_ref();
        let other_is_older = match (other_creation, current_creation) {
            (Some(o), Some(c)) if o.0 != c.0 => o.0 < c.0,
            _ => {
                (other_namespace.as_str(), other_name.as_str())
                    < (current_namespace.as_str(), current_name.as_str())
            }
        };
        if !other_is_older {
            // The current zone is the older / lexicographically first
            // claimant — keep it; the *other* zone is the loser. Don't
            // record this as a conflict here; the other zone's own
            // reconciler will report its loss when it runs.
            continue;
        }

        // Collect any instance names from status for the operator's
        // diagnostics, but do not gate the conflict on them.
        let instance_names: Vec<String> = other_zone
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
            "Duplicate zone detected: {}/{} already claims {} (older CR or lex-prior); \
             current zone {}/{} will be marked Ready=False with DuplicateZone reason. \
             Instances on the winning zone: {:?}",
            other_namespace, other_name, zone_name, current_namespace, current_name, instance_names
        );

        conflicting_zones.push(ConflictingZone {
            name: other_name,
            namespace: other_namespace,
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
