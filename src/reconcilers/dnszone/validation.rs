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
/// # F-003 mitigation: cross-namespace targeting requires platform-admin opt-in
///
/// A label selector match is *not* sufficient to enrol a `Bind9Instance` in
/// the zone. The instance is included only when **either**:
///
/// 1. The instance lives in the **same namespace** as the `DNSZone`, **or**
/// 2. The instance carries the
///    [`crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES`] annotation
///    whose value contains the zone's namespace (or the wildcard
///    [`crate::constants::ALLOW_ZONE_NAMESPACES_WILDCARD`]).
///
/// The annotation is metadata on the `Bind9Instance`, which is owned by
/// the platform admin (only they have RBAC on the namespace where the
/// instance lives). This preserves the cluster-wide-operator contract:
/// the platform admin keeps full control of who can claim their
/// instances, expressed through a platform-admin-controlled annotation,
/// while still preventing the F-003 hijack — labels on the instance side
/// are not a security boundary (they are discoverable via list/watch and
/// any tenant can write any matchLabels they want), but annotations on
/// the platform-owned instance are.
///
/// # Arguments
///
/// * `dnszone` - The `DNSZone` resource to get instances for
/// * `bind9_instances_store` - Reflector store of `Bind9Instance`
///
/// # Returns
///
/// * `Ok(Vec<InstanceReference>)` - List of instances serving this zone
/// * `Err(_)` - If no instances pass both the selector match and the
///   namespace gate
///
/// # Errors
///
/// Returns an error if no instances pass the selector + namespace gate, or
/// if `spec.bind9_instances_from` is missing or empty.
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

    let mut cross_ns_denied: Vec<(String, String)> = Vec::new();
    let instances_with_zone: Vec<crate::crd::InstanceReference> = bind9_instances_store
        .state()
        .iter()
        .filter_map(|instance| {
            let instance_labels = instance.metadata.labels.as_ref()?;
            let instance_namespace = instance.namespace()?;
            let instance_name = instance.name_any();

            // Selector match (label-based) — necessary but not sufficient.
            let matches = bind9_instances_from
                .iter()
                .any(|source| source.selector.matches(instance_labels));
            if !matches {
                return None;
            }

            // F-003 namespace gate. Same-namespace always allowed; cross-
            // namespace requires the platform-admin annotation on the
            // instance.
            if instance_namespace != namespace
                && !instance_allows_zone_namespace(instance, &namespace)
            {
                cross_ns_denied.push((instance_namespace.clone(), instance_name.clone()));
                return None;
            }

            Some(crate::crd::InstanceReference {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Instance".to_string(),
                name: instance_name,
                namespace: instance_namespace,
                last_reconciled_at: None,
            })
        })
        .collect();

    if !cross_ns_denied.is_empty() {
        warn!(
            "DNSZone {}/{} label selectors matched {} cross-namespace Bind9Instance(s) \
             that were rejected by the F-003 namespace gate: {:?}. \
             To allow cross-namespace targeting, the platform admin must annotate the \
             target Bind9Instance with `{}: <comma-separated namespaces>` (or `*`).",
            namespace,
            name,
            cross_ns_denied.len(),
            cross_ns_denied,
            crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES,
        );
    }

    if !instances_with_zone.is_empty() {
        debug!(
            "DNSZone {}/{} matched {} instances via spec.bind9_instances_from selectors",
            namespace,
            name,
            instances_with_zone.len()
        );
        return Ok(instances_with_zone);
    }

    // No instances found — message distinguishes "no labels matched" from
    // "labels matched but cross-namespace gate denied them".
    if cross_ns_denied.is_empty() {
        Err(anyhow!(
            "DNSZone {namespace}/{name} has no instances matching spec.bind9_instances_from selectors. \
            Verify that Bind9Instance resources exist with matching labels."
        ))
    } else {
        Err(anyhow!(
            "DNSZone {namespace}/{name} matched only cross-namespace Bind9Instance(s) \
             that the F-003 namespace gate denied. Ask the platform admin to annotate \
             the target instance with `{annotation}: {namespace}` (or `{annotation}: *` \
             to allow any namespace).",
            annotation = crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES,
        ))
    }
}

/// Check whether `instance` carries an annotation that grants the named
/// `zone_namespace` permission to target it cross-namespace.
///
/// Returns `true` iff the instance's
/// [`crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES`] annotation is
/// set and its value, when parsed as a comma-separated list, contains
/// either `zone_namespace` or
/// [`crate::constants::ALLOW_ZONE_NAMESPACES_WILDCARD`].
///
/// Same-namespace matching is handled by the caller and does *not*
/// require this annotation.
#[must_use]
pub fn instance_allows_zone_namespace(
    instance: &crate::crd::Bind9Instance,
    zone_namespace: &str,
) -> bool {
    let Some(annotations) = instance.metadata.annotations.as_ref() else {
        return false;
    };
    let Some(value) = annotations.get(crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES) else {
        return false;
    };
    value.split(',').map(str::trim).any(|entry| {
        entry == crate::constants::ALLOW_ZONE_NAMESPACES_WILDCARD || entry == zone_namespace
    })
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
            "Duplicate zone detected: {}/{} already claims {} (older CR or lex-prior); \
             current zone {}/{} will be marked Ready=False with DuplicateZone reason. \
             Instances on the winning zone: {:?}",
            other_namespace, other_name, zone_name, current_namespace, current_name, instance_names
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
