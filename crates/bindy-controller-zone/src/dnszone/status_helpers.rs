// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Status calculation and finalization helpers for DNSZone reconciliation.
//!
//! This module contains functions for calculating expected instance counts
//! and determining the final Ready/Degraded status of a DNSZone.

use anyhow::Result;
use k8s_openapi::jiff::{civil, tz::TimeZone, Timestamp};
use kube::Client;

use super::types::{
    ZoneOutcome, REASON_CLEANUP_PENDING, REASON_DEGRADED, REASON_DNSSEC_KEYS_PENDING,
};
use crate::crd::{DNSSECStatus, InstanceReference};

/// Calculate expected instance counts (primary and secondary).
///
/// This function filters the instance references to determine how many
/// primary and secondary instances should be configured. Roles come from the
/// `Bind9Instance` store, with a GET only for an instance it does not hold
/// (ADR-0016).
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_refs` - List of instance references assigned to the zone
///
/// # Returns
///
/// Tuple of `(expected_primary_count, expected_secondary_count)`
///
/// # Errors
///
/// Does not currently fail; the `Result` keeps the established signature.
pub async fn calculate_expected_instance_counts(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[InstanceReference],
) -> Result<(usize, usize)> {
    let expected_primary_count =
        super::primary::filter_primary_instances_cached(client, store, instance_refs)
            .await
            .map(|refs| refs.len())
            .unwrap_or(0);

    let expected_secondary_count =
        super::secondary::filter_secondary_instances(client, store, instance_refs)
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
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
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
    status_updater: &mut bindy_controller_sdk::status::DNSZoneStatusUpdater,
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

/// Parse the next-rollover instant a bindcar sidecar reports.
///
/// bindcar reports a civil time without an offset (`2027-09-27T00:00:00`),
/// which BIND9 means as UTC; an RFC 3339 instant is accepted too.
///
/// # Arguments
///
/// * `value` - The `nextKeyRollover` string
///
/// # Returns
///
/// The instant, or `None` when it does not parse.
#[must_use]
pub fn parse_rollover_instant(value: &str) -> Option<Timestamp> {
    if let Ok(instant) = value.parse::<Timestamp>() {
        return Some(instant);
    }
    let civil: civil::DateTime = value.parse().ok()?;
    civil
        .to_zoned(TimeZone::UTC)
        .ok()
        .map(|zoned| zoned.timestamp())
}

/// Decide how a finished zone reconcile asks to be continued (ADR-0016).
///
/// - Any `Degraded` condition left set: [`ZoneOutcome::Retry`]. An instance or
///   endpoint rejected the zone, or a record replay is incomplete; the backoff
///   retries it, the Endpoints watch wakes it sooner when a pod comes back.
/// - A cleanup pass left work behind (a deleted record whose DNS data is not
///   confirmed gone, or a failed instance or record cleanup):
///   [`ZoneOutcome::Retry`], because the pass only runs inside a reconcile.
/// - DNSSEC requested but the keys are not there yet: [`ZoneOutcome::Retry`],
///   because key generation inside BIND9 raises no Kubernetes event.
/// - Otherwise [`ZoneOutcome::Converged`], with a wake at the next KSK
///   rollover when the sidecar reported one in the future, so
///   `status.dnssec` follows the new DS record.
///
/// # Arguments
///
/// * `degraded` - Whether the reconcile left a `Degraded` condition set
/// * `cleanup_incomplete` - Whether a cleanup pass left work to retry
/// * `dnssec` - The DNSSEC status the reconcile computed, if any
/// * `now` - The current instant
///
/// # Returns
///
/// The zone's [`ZoneOutcome`].
#[must_use]
pub fn zone_outcome(
    degraded: bool,
    cleanup_incomplete: bool,
    dnssec: Option<&DNSSECStatus>,
    now: Timestamp,
) -> ZoneOutcome {
    if degraded {
        return ZoneOutcome::Retry {
            reason: REASON_DEGRADED,
        };
    }

    if cleanup_incomplete {
        return ZoneOutcome::Retry {
            reason: REASON_CLEANUP_PENDING,
        };
    }

    let Some(dnssec) = dnssec else {
        return ZoneOutcome::Converged { next_wake: None };
    };

    if !dnssec.signed {
        return ZoneOutcome::Retry {
            reason: REASON_DNSSEC_KEYS_PENDING,
        };
    }

    let next_wake = dnssec
        .next_key_rollover
        .as_deref()
        .and_then(parse_rollover_instant)
        .and_then(|rollover| std::time::Duration::try_from(rollover.duration_since(now)).ok())
        .filter(|delay| !delay.is_zero());

    ZoneOutcome::Converged { next_wake }
}

#[cfg(test)]
#[path = "status_helpers_tests.rs"]
mod status_helpers_tests;
