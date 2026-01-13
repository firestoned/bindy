// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 configuration orchestration for DNS zones.
//!
//! This module coordinates zone configuration on primary and secondary BIND9 instances,
//! managing status updates and error handling throughout the configuration process.

use anyhow::{anyhow, Result};
use kube::ResourceExt;
use std::sync::Arc;
use tracing::info;

use crate::crd::{DNSZone, InstanceReference};

/// Configure zone on all BIND9 instances (primary and secondary).
///
/// This function orchestrates the complete BIND9 configuration workflow:
/// 1. Sets initial "Progressing" status
/// 2. Finds primary server IPs for secondary configuration
/// 3. Configures zone on all primary instances
/// 4. Configures zone on all secondary instances
/// 5. Updates status conditions based on success/failure
///
/// # Arguments
///
/// * `ctx` - Application context with Kubernetes client
/// * `dnszone` - The DNSZone resource being reconciled
/// * `zone_manager` - BIND9 manager for zone operations
/// * `status_updater` - Status updater for condition updates
/// * `instance_refs` - All instance references assigned to the zone
/// * `unreconciled_instances` - Instances that need reconciliation (Phase 2 optimization)
///
/// # Returns
///
/// Tuple of `(primary_count, secondary_count)` - number of successfully configured instances
///
/// # Errors
///
/// Returns an error if:
/// - No primary servers are found (cannot configure secondary zones)
/// - Primary configuration fails completely
/// - Kubernetes API operations fail
///
/// Note: Secondary configuration failure is non-fatal and logged as a warning
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
pub async fn configure_zone_on_instances(
    ctx: Arc<crate::context::Context>,
    dnszone: &DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
    status_updater: &mut crate::reconcilers::status::DNSZoneStatusUpdater,
    instance_refs: &[InstanceReference],
    unreconciled_instances: &[InstanceReference],
) -> Result<(usize, usize)> {
    let client = ctx.client.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let spec = &dnszone.spec;

    tracing::debug!("Ensuring BIND9 zone exists on all instances (declarative reconciliation)");

    // Set initial Progressing status (in-memory)
    status_updater.set_condition(
        "Progressing",
        "True",
        "PrimaryReconciling",
        "Configuring zone on primary servers",
    );

    // Get current primary IPs for secondary zone configuration
    // Find all primary instances from our instance refs and get their pod IPs
    let primary_ips =
        match super::primary::find_primary_ips_from_instances(&client, instance_refs).await {
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
    let primary_count = match super::add_dnszone(
        ctx.clone(),
        dnszone.clone(),
        zone_manager,
        status_updater,
        unreconciled_instances,
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
    let secondary_count = match super::add_dnszone_to_secondaries(
        ctx.clone(),
        dnszone.clone(),
        zone_manager,
        &primary_ips,
        status_updater,
        unreconciled_instances,
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
            tracing::warn!(
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

    Ok((primary_count, secondary_count))
}

#[cfg(test)]
#[path = "bind9_config_tests.rs"]
mod bind9_config_tests;
