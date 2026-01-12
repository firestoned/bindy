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

/// Determine final zone status and apply conditions.
///
/// This function calculates the final Ready or Degraded status based on:
/// - Whether any degraded conditions were set during reconciliation
/// - Whether all expected instances were successfully configured
/// - Number of records discovered
///
/// The function then applies all accumulated status changes to the API server
/// in a single atomic operation.
///
/// # Arguments
///
/// * `status_updater` - Status updater with accumulated changes
/// * `client` - Kubernetes API client
/// * `zone_name` - DNS zone name (e.g., "example.com")
/// * `namespace` - Kubernetes namespace of the DNSZone resource
/// * `name` - Name of the DNSZone resource
/// * `primary_count` - Number of primary instances successfully configured
/// * `secondary_count` - Number of secondary instances successfully configured
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
    primary_count: usize,
    secondary_count: usize,
    expected_primary_count: usize,
    expected_secondary_count: usize,
    records_count: usize,
    generation: Option<i64>,
) -> Result<()> {
    // Set observed generation
    status_updater.set_observed_generation(generation);

    // Set final Ready/Degraded status based on reconciliation outcome
    // Only set Ready=True if there were NO degraded conditions during reconciliation
    // AND all expected instances were successfully configured
    if status_updater.has_degraded_condition() {
        // Keep the Degraded condition that was already set, don't overwrite with Ready
        tracing::info!(
            "DNSZone {}/{} reconciliation completed with degraded state - will retry faster",
            namespace,
            name
        );
    } else if primary_count < expected_primary_count || secondary_count < expected_secondary_count {
        // Not all instances were configured - set Degraded condition
        status_updater.set_condition(
            "Degraded",
            "True",
            "PartialReconciliation",
            &format!(
                "Zone {} configured on {}/{} primary and {}/{} secondary instance(s) - {} instance(s) pending",
                zone_name,
                primary_count,
                expected_primary_count,
                secondary_count,
                expected_secondary_count,
                (expected_primary_count - primary_count)
                    + (expected_secondary_count - secondary_count)
            ),
        );
        tracing::info!(
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
                zone_name, primary_count, secondary_count, records_count
            ),
        );
        // Clear any stale Degraded condition from previous failures
        status_updater.clear_degraded_condition();
    }

    // Apply all status changes in a single atomic operation
    status_updater.apply(client).await?;

    Ok(())
}
