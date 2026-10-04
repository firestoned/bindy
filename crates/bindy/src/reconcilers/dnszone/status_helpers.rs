// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Status calculation and finalization helpers for DNSZone reconciliation.
//!
//! This module contains functions for calculating expected instance counts
//! and determining the final Ready/Degraded status of a DNSZone.

use anyhow::Result;
use kube::Client;

use crate::crd::InstanceReference;

/// Calculate expected instance counts (primary and secondary).
///
/// This function filters the instance references to determine how many
/// primary and secondary instances should be configured.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - List of instance references assigned to the zone
///
/// # Returns
///
/// Tuple of `(expected_primary_count, expected_secondary_count)`
///
/// # Errors
///
/// Returns an error if Kubernetes API calls fail
pub async fn calculate_expected_instance_counts(
    client: &Client,
    instance_refs: &[InstanceReference],
) -> Result<(usize, usize)> {
    let expected_primary_count = super::primary::filter_primary_instances(client, instance_refs)
        .await
        .map(|refs| refs.len())
        .unwrap_or(0);

    let expected_secondary_count =
        super::secondary::filter_secondary_instances(client, instance_refs)
            .await
            .map(|refs| refs.len())
            .unwrap_or(0);

    Ok((expected_primary_count, expected_secondary_count))
}

/// Set the final zone conditions in memory (no API call).
///
/// This function calculates the final Ready/Degraded/Progressing status based on:
/// - Whether any degraded conditions were set during reconciliation
/// - Whether all expected INSTANCES were successfully configured (comparing
///   instance counts with instance counts - never endpoint counts, which would
///   mask partial pod failures)
/// - Number of records discovered
///
/// The conditions always converge to a consistent triple:
/// - Success: `Ready=True`, `Degraded=False`, `Progressing=False`
/// - Failure/partial: `Ready=False`, `Degraded=True`, `Progressing=False`
///
/// # Arguments
///
/// * `status_updater` - Status updater with accumulated changes
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `namespace` - Kubernetes namespace of the DNSZone resource
/// * `name` - Name of the DNSZone resource
/// * `primary` - Primary configuration outcome (instance + endpoint counts)
/// * `secondary` - Secondary configuration outcome (instance + endpoint counts)
/// * `expected_primary_count` - Expected number of primary instances
/// * `expected_secondary_count` - Expected number of secondary instances
/// * `records_count` - Number of DNS records discovered
/// * `generation` - Metadata generation to set as observed
#[allow(clippy::too_many_arguments)]
pub fn set_final_zone_conditions(
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
    zone_name: &str,
    namespace: &str,
    name: &str,
    primary: super::types::ZoneConfigOutcome,
    secondary: super::types::ZoneConfigOutcome,
    expected_primary_count: usize,
    expected_secondary_count: usize,
    records_count: usize,
    generation: Option<i64>,
) {
    // Set observed generation
    status_updater.set_observed_generation(generation);

    // Set final Ready/Degraded status based on reconciliation outcome
    // Only set Ready=True if there were NO degraded conditions during reconciliation
    // AND all expected instances were successfully configured
    if status_updater.has_degraded_condition() {
        // Keep the Degraded condition that was already set, but make the
        // condition triple consistent: a stale Ready=True from a previous
        // successful reconciliation must not survive a failure.
        status_updater.set_condition(
            "Ready",
            "False",
            "ReconcileDegraded",
            &format!("Zone {zone_name} reconciliation completed with degraded state - see Degraded condition for details"),
        );
        tracing::info!(
            "DNSZone {}/{} reconciliation completed with degraded state - will retry faster",
            namespace,
            name
        );
    } else if primary.instances_configured < expected_primary_count
        || secondary.instances_configured < expected_secondary_count
    {
        // Not all INSTANCES were configured - set Degraded and Ready=False.
        // Comparing instance counts (not endpoint counts) ensures an instance
        // that received the zone on none of its pods is not masked by another
        // instance with multiple successful pod endpoints.
        let message = format!(
            "Zone {} configured on {}/{} primary and {}/{} secondary instance(s) - {} instance(s) pending",
            zone_name,
            primary.instances_configured,
            expected_primary_count,
            secondary.instances_configured,
            expected_secondary_count,
            expected_primary_count.saturating_sub(primary.instances_configured)
                + expected_secondary_count.saturating_sub(secondary.instances_configured)
        );
        status_updater.set_condition("Degraded", "True", "PartialReconciliation", &message);
        status_updater.set_condition("Ready", "False", "PartialReconciliation", &message);
        tracing::info!(
            "DNSZone {}/{} partially configured: {}/{} primaries, {}/{} secondaries",
            namespace,
            name,
            primary.instances_configured,
            expected_primary_count,
            secondary.instances_configured,
            expected_secondary_count
        );
    } else {
        // All reconciliation steps succeeded - set Ready status and clear any stale Degraded condition
        status_updater.set_condition(
            "Ready",
            "True",
            "ReconcileSucceeded",
            &format!(
                "Zone {} configured on {} primary and {} secondary instance(s) ({} endpoint(s)), discovered {} DNS record(s)",
                zone_name,
                primary.instances_configured,
                secondary.instances_configured,
                primary.endpoints_configured + secondary.endpoints_configured,
                records_count
            ),
        );
        // Clear any stale Degraded condition from previous failures
        status_updater.clear_degraded_condition();
    }

    // The reconciliation attempt has finished either way - resolve the
    // Progressing condition set at the start of BIND9 configuration so it
    // does not stay True forever.
    status_updater.set_condition(
        "Progressing",
        "False",
        "ReconcileComplete",
        "Reconciliation attempt finished",
    );
}

/// Determine final zone status and apply conditions.
///
/// Sets the final condition triple via [`set_final_zone_conditions`] and then
/// applies all accumulated status changes to the API server in a single
/// atomic operation.
///
/// # Arguments
///
/// * `status_updater` - Status updater with accumulated changes
/// * `client` - Kubernetes API client
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `namespace` - Kubernetes namespace of the DNSZone resource
/// * `name` - Name of the DNSZone resource
/// * `primary` - Primary configuration outcome (instance + endpoint counts)
/// * `secondary` - Secondary configuration outcome (instance + endpoint counts)
/// * `expected_primary_count` - Expected number of primary instances
/// * `expected_secondary_count` - Expected number of secondary instances
/// * `records_count` - Number of DNS records discovered
/// * `generation` - Metadata generation to set as observed
///
/// # Errors
///
/// Returns an error if status update fails to apply
#[allow(clippy::too_many_arguments)]
pub async fn finalize_zone_status(
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
    client: &Client,
    zone_name: &str,
    namespace: &str,
    name: &str,
    primary: super::types::ZoneConfigOutcome,
    secondary: super::types::ZoneConfigOutcome,
    expected_primary_count: usize,
    expected_secondary_count: usize,
    records_count: usize,
    generation: Option<i64>,
) -> Result<()> {
    set_final_zone_conditions(
        status_updater,
        zone_name,
        namespace,
        name,
        primary,
        secondary,
        expected_primary_count,
        expected_secondary_count,
        records_count,
        generation,
    );

    // Apply all status changes in a single atomic operation
    status_updater.apply(client).await?;

    Ok(())
}

#[cfg(test)]
#[path = "status_helpers_tests.rs"]
mod status_helpers_tests;
