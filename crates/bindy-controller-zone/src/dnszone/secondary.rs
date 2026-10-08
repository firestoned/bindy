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
/// Does not currently fail; an instance that is neither cached nor readable
/// is skipped with a warning.
pub async fn filter_secondary_instances(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    Ok(bindy_bind9::primary::filter_instances_by_role_cached(
        client,
        store,
        instance_refs,
        &crate::crd::ServerRole::Secondary,
    )
    .await)
}

#[cfg(test)]
#[path = "secondary_tests.rs"]
mod secondary_tests;
