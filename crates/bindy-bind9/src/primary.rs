// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Primary zone instance operations.
//!
//! This module handles all operations specific to PRIMARY BIND9 instances,
//! including:
//! - Reading an instance's role, from the reflector store or the API server
//! - Filtering instance references to only primary instances, or by role
//! - Partitioning a zone's instances by role in one pass ([`ZoneInstances`])

use anyhow::Result;
use kube::{Api, Client};
use tracing::{debug, warn};

use bindy_controller_sdk::kube_errors::is_not_found;

/// Turn the result of a `Bind9Instance` GET into that instance's role.
///
/// Only a 404 means the instance is gone and may be skipped (`Ok(None)`, with
/// a warning). Every other failure (timeout, 429, 5xx, transport) propagates:
/// skipping it would silently drop a live instance from a write's target set,
/// and the write would report success without touching it.
///
/// # Arguments
///
/// * `result` - The outcome of `Api::<Bind9Instance>::get`
/// * `instance_ref` - The instance that was read, for logs and errors
///
/// # Returns
///
/// `Ok(Some(role))` for a readable instance, `Ok(None)` for a deleted one.
///
/// # Errors
///
/// Returns an error for any GET failure other than 404 Not Found.
pub fn role_from_get_result(
    result: std::result::Result<crate::crd::Bind9Instance, kube::Error>,
    instance_ref: &crate::crd::InstanceReference,
) -> Result<Option<crate::crd::ServerRole>> {
    match result {
        Ok(instance) => Ok(Some(instance.spec.role)),
        Err(e) if is_not_found(&e) => {
            warn!(
                "Instance {}/{} not found (deleted). Skipping.",
                instance_ref.namespace, instance_ref.name
            );
            Ok(None)
        }
        Err(e) => Err(anyhow::Error::new(e).context(format!(
            "Failed to get instance {}/{}",
            instance_ref.namespace, instance_ref.name
        ))),
    }
}

/// Read one instance's role from the API server.
async fn get_instance_role(
    client: &Client,
    instance_ref: &crate::crd::InstanceReference,
) -> Result<Option<crate::crd::ServerRole>> {
    let api: Api<crate::crd::Bind9Instance> =
        Api::namespaced(client.clone(), &instance_ref.namespace);
    role_from_get_result(api.get(&instance_ref.name).await, instance_ref)
}

/// Filters a list of instance references to only PRIMARY instances.
///
/// An instance that no longer exists (404) is skipped with a warning.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// Vector of instance references that have role=Primary
///
/// # Errors
///
/// Returns an error if reading any instance fails for a reason other than
/// 404 Not Found (timeout, 429, 5xx), so a live primary never silently drops
/// out of the target set.
pub async fn filter_primary_instances(
    client: &Client,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    use crate::crd::ServerRole;

    let mut primary_refs = Vec::new();

    for instance_ref in instance_refs {
        if get_instance_role(client, instance_ref).await? == Some(ServerRole::Primary) {
            primary_refs.push(instance_ref.clone());
        }
    }

    Ok(primary_refs)
}

/// The role of the instance behind `instance_ref`, according to the
/// `Bind9Instance` reflector store.
///
/// # Arguments
///
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_ref` - The instance to look up
///
/// # Returns
///
/// The instance's `spec.role`, or `None` when the store does not hold the
/// instance (the caller decides whether to fall back to the API server).
#[must_use]
pub fn instance_role_in_store(
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_ref: &crate::crd::InstanceReference,
) -> Option<crate::crd::ServerRole> {
    use kube::ResourceExt;
    store
        .state()
        .iter()
        .find(|instance| {
            instance.name_any() == instance_ref.name
                && instance.namespace().as_deref() == Some(instance_ref.namespace.as_str())
        })
        .map(|instance| instance.spec.role)
}

/// The role of an instance: from the reflector store, or with a GET for an
/// instance the store does not hold yet.
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_ref` - The instance to look up
///
/// # Returns
///
/// The instance's role, or `None` (with a warning) when it is not cached and
/// the API server says it does not exist (404).
///
/// # Errors
///
/// Returns an error when the fallback GET fails for any reason other than
/// 404 Not Found.
pub async fn instance_role_cached(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_ref: &crate::crd::InstanceReference,
) -> Result<Option<crate::crd::ServerRole>> {
    if let Some(role) = instance_role_in_store(store, instance_ref) {
        return Ok(Some(role));
    }
    get_instance_role(client, instance_ref).await
}

/// Filters instance references to those with `role`, reading each role from
/// the `Bind9Instance` reflector store (ADR-0015, ADR-0016).
///
/// An instance the store holds costs no API call; one it does not hold yet
/// is read with a GET, and skipped only if the API server says it does not
/// exist (404).
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_refs` - Instance references to filter
/// * `role` - The role to keep
///
/// # Returns
///
/// The references whose instance has `role`, in input order.
///
/// # Errors
///
/// Returns an error when a fallback GET fails for any reason other than
/// 404 Not Found, so a live instance never silently drops out.
pub async fn try_filter_instances_by_role_cached(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[crate::crd::InstanceReference],
    role: &crate::crd::ServerRole,
) -> Result<Vec<crate::crd::InstanceReference>> {
    let mut matching = Vec::new();
    for instance_ref in instance_refs {
        if instance_role_cached(client, store, instance_ref)
            .await?
            .as_ref()
            == Some(role)
        {
            matching.push(instance_ref.clone());
        }
    }
    Ok(matching)
}

/// Filters instance references to PRIMARY instances, reading each role from
/// the `Bind9Instance` reflector store (ADR-0015).
///
/// Same result as [`filter_primary_instances`], but an instance the store
/// holds costs no API call, so a record write no longer spends one GET per
/// instance. An instance the store does not hold yet (a watch that has not
/// caught up) falls back to a GET, and is skipped only if the API server
/// says it does not exist (404), exactly as [`filter_primary_instances`] does.
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `instance_refs` - Instance references to filter
///
/// # Returns
///
/// The references whose instance has role=Primary.
///
/// # Errors
///
/// Returns an error when the fallback GET for an uncached instance fails for
/// any reason other than 404 Not Found, so a live primary never silently
/// drops out of a record write or zone delete.
pub async fn filter_primary_instances_cached(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    instance_refs: &[crate::crd::InstanceReference],
) -> Result<Vec<crate::crd::InstanceReference>> {
    let mut primary_refs = Vec::new();
    let mut uncached = Vec::new();

    for instance_ref in instance_refs {
        match instance_role_in_store(store, instance_ref) {
            Some(crate::crd::ServerRole::Primary) => primary_refs.push(instance_ref.clone()),
            Some(_) => {}
            None => uncached.push(instance_ref.clone()),
        }
    }

    if !uncached.is_empty() {
        debug!(
            "{} instance(s) not in the Bind9Instance store yet, reading their role from the API",
            uncached.len()
        );
        primary_refs.extend(filter_primary_instances(client, &uncached).await?);
    }

    Ok(primary_refs)
}

/// A zone's instances, with each instance's role read once and the result
/// kept, so a reconcile filters by role without asking again.
///
/// Every list keeps the order of the input references.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZoneInstances {
    /// Every instance whose role could be read: primaries and secondaries
    /// interleaved in input order. An instance the API server reports as
    /// deleted (404) is in no list.
    pub all: Vec<crate::crd::InstanceReference>,
    /// The instances with role=Primary, in input order.
    pub primaries: Vec<crate::crd::InstanceReference>,
    /// The instances with role=Secondary, in input order.
    pub secondaries: Vec<crate::crd::InstanceReference>,
}

/// Partitions a zone's instance references by role, reading each role once.
///
/// Uses the same lookup as [`instance_role_cached`]: the `Bind9Instance`
/// reflector store first (no API call), a GET for an instance the store does
/// not hold yet. An instance the GET reports as deleted (404) is skipped with
/// a warning; any other GET failure fails the whole partition, so a live
/// instance never silently drops out of a zone write's target set.
///
/// # Arguments
///
/// * `client` - Kubernetes API client, for the fallback GET
/// * `store` - The shared `Bind9Instance` reflector store
/// * `refs` - The zone's instance references
///
/// # Returns
///
/// The live instances, all together and split into primaries and
/// secondaries, each list in input order.
///
/// # Errors
///
/// Returns an error when the fallback GET for an uncached instance fails for
/// any reason other than 404 Not Found.
pub async fn partition_zone_instances(
    client: &Client,
    store: &crate::context::MultiStore<crate::crd::Bind9Instance>,
    refs: Vec<crate::crd::InstanceReference>,
) -> Result<ZoneInstances> {
    use crate::crd::ServerRole;

    let mut partition = ZoneInstances::default();
    for instance_ref in refs {
        let Some(role) = instance_role_cached(client, store, &instance_ref).await? else {
            continue;
        };
        match role {
            ServerRole::Primary => partition.primaries.push(instance_ref.clone()),
            ServerRole::Secondary => partition.secondaries.push(instance_ref.clone()),
        }
        partition.all.push(instance_ref);
    }
    Ok(partition)
}

#[cfg(test)]
#[path = "primary_tests.rs"]
mod primary_tests;
