// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Secondary zone instance operations.
//!
//! This module filters instance references to the SECONDARY instances. The
//! secondaries' pod IPs come from the Pod store (`bindy_bind9::peers`,
//! ADR-0019), not from a pod LIST.

use anyhow::Result;
use kube::Client;

/// Filters a list of instance references to only SECONDARY instances.
///
/// Roles come from the `Bind9Instance` reflector store, with a GET only for
/// an instance the store does not hold yet (ADR-0015, ADR-0016).
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// Vector of instance references that have role=Secondary
///
/// # Errors
///
/// Returns an error if an instance the store does not hold cannot be read
/// (any failure other than 404), so a secondary never silently drops out of
/// the zone's secondary set on a transient API error.
pub async fn filter_secondary_instances(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    bindy_bind9::primary::try_filter_instances_by_role_cached(
        client,
        store,
        instance_refs,
        &crate::crd::ServerRole::Secondary,
    )
    .await
}

#[cfg(test)]
#[path = "secondary_tests.rs"]
mod secondary_tests;
