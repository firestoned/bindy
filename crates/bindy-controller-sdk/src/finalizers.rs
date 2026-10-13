// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Generic finalizer management for Kubernetes resources.
//!
//! This module provides reusable functions for adding, removing, and handling
//! finalizers on Kubernetes custom resources. One set of functions serves
//! both namespaced and cluster-scoped kinds: the caller passes the `Api` the
//! resource lives behind (`Api::namespaced` or `Api::all`), and that is the
//! only thing that differs between the two scopes.
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy_controller_sdk::finalizers::{ensure_finalizer, handle_deletion};
//! use bindy_api::crd::Bind9Cluster;
//! use kube::{Api, Client, ResourceExt};
//! use anyhow::Result;
//!
//! const FINALIZER: &str = "bind9cluster.dns.firestoned.io/finalizer";
//!
//! async fn delete_children(client: &Client, cluster: &Bind9Cluster) -> Result<()> {
//!     // Perform cleanup operations
//!     Ok(())
//! }
//!
//! async fn reconcile(client: Client, cluster: Bind9Cluster) -> Result<()> {
//!     let api: Api<Bind9Cluster> =
//!         Api::namespaced(client.clone(), &cluster.namespace().unwrap_or_default());
//!
//!     // Handle deletion if resource is being deleted
//!     if cluster.metadata.deletion_timestamp.is_some() {
//!         return handle_deletion(&api, &cluster, FINALIZER, || {
//!             delete_children(&client, &cluster)
//!         })
//!         .await;
//!     }
//!
//!     // Ensure finalizer is present
//!     ensure_finalizer(&api, &cluster, FINALIZER).await?;
//!
//!     // Normal reconciliation logic...
//!     Ok(())
//! }
//! ```
//!
//! The cleanup is a closure rather than a trait the resource implements: a
//! trait defined here and implemented for a `bindy-api` type in a controller
//! crate would break Rust's orphan rule (ADR-0009 §2, amended 2026-10-05).

use anyhow::{anyhow, Context as AnyhowContext, Result};
use kube::api::{Patch, PatchParams};
use kube::{Api, Resource, ResourceExt};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::fmt::Debug;
use std::future::Future;
use tracing::info;

/// Builds the JSON Patch that atomically adds `finalizer` to a resource.
///
/// Mirrors the pattern used by kube-runtime's own finalizer helper:
/// - When the resource has no finalizers, the patch `test`s that
///   `/metadata/finalizers` is absent (null) before creating the array. This
///   guarantees a racing writer that added a finalizer first is not clobbered.
/// - When finalizers exist, JSON Patch has no test-for-absence, so the patch
///   `test`s `/metadata/resourceVersion` instead and appends with the `-`
///   (end-of-array) pointer, never rewriting existing entries.
///
/// If either `test` fails on the server (concurrent modification), the API
/// call fails and the caller must requeue; the next reconciliation retries
/// with fresh state.
///
/// # Arguments
///
/// * `existing_finalizers` - Finalizers currently on the (possibly stale) object
/// * `resource_version` - The object's `metadata.resourceVersion`
/// * `finalizer` - The finalizer string to add
///
/// # Errors
///
/// Returns an error if the resource has existing finalizers but no
/// `resourceVersion` (cannot build a safe guarded patch), or if patch
/// serialization fails.
pub(crate) fn build_add_finalizer_patch(
    existing_finalizers: &[String],
    resource_version: Option<&str>,
    finalizer: &str,
) -> Result<json_patch::Patch> {
    let operations = if existing_finalizers.is_empty() {
        json!([
            {"op": "test", "path": "/metadata/finalizers", "value": null},
            {"op": "add", "path": "/metadata/finalizers", "value": [finalizer]},
        ])
    } else {
        let resource_version = resource_version.ok_or_else(|| {
            anyhow!("resource has no resourceVersion; cannot safely add finalizer {finalizer}")
        })?;
        json!([
            {"op": "test", "path": "/metadata/resourceVersion", "value": resource_version},
            {"op": "add", "path": "/metadata/finalizers/-", "value": finalizer},
        ])
    };

    serde_json::from_value(operations).context("failed to build add-finalizer JSON Patch")
}

/// Builds the JSON Patch that atomically removes `finalizer` from a resource.
///
/// Mirrors the pattern used by kube-runtime's own finalizer helper: the patch
/// `test`s that the exact array index still contains our finalizer before
/// removing that index. If a racing writer added or removed a finalizer in the
/// meantime (shifting indices), the `test` fails server-side and nothing is
/// removed - so a foreign finalizer can never be dropped by accident.
///
/// # Arguments
///
/// * `existing_finalizers` - Finalizers currently on the (possibly stale) object
/// * `finalizer` - The finalizer string to remove
///
/// # Returns
///
/// `Ok(None)` when the finalizer is not present (nothing to do).
///
/// # Errors
///
/// Returns an error if patch serialization fails.
pub(crate) fn build_remove_finalizer_patch(
    existing_finalizers: &[String],
    finalizer: &str,
) -> Result<Option<json_patch::Patch>> {
    let Some(index) = existing_finalizers.iter().position(|f| f == finalizer) else {
        return Ok(None);
    };

    let finalizer_path = format!("/metadata/finalizers/{index}");
    let operations = json!([
        {"op": "test", "path": finalizer_path, "value": finalizer},
        {"op": "remove", "path": finalizer_path},
    ]);

    serde_json::from_value(operations)
        .map(Some)
        .context("failed to build remove-finalizer JSON Patch")
}

/// The object's name for logs and errors: `namespace/name` for a namespaced
/// object, `name` for a cluster-scoped one.
fn display_name<K: Resource>(resource: &K) -> String {
    let name = resource.name_any();
    match resource.namespace() {
        Some(namespace) => format!("{namespace}/{name}"),
        None => name,
    }
}

/// Whether `resource` carries `finalizer`.
///
/// # Arguments
///
/// * `resource` - The resource to inspect
/// * `finalizer` - The finalizer string to look for (exact match)
///
/// # Returns
///
/// `true` when `metadata.finalizers` contains `finalizer`; `false` when it
/// does not, is empty, or is absent.
#[must_use]
pub fn has_finalizer<K: Resource>(resource: &K, finalizer: &str) -> bool {
    resource
        .meta()
        .finalizers
        .as_ref()
        .is_some_and(|f| f.iter().any(|existing| existing == finalizer))
}

/// Add a finalizer to a resource if not already present.
///
/// This function checks if the specified finalizer is present on the resource,
/// and adds it if missing. The operation is idempotent - calling it multiple
/// times has no effect if the finalizer is already present.
///
/// Works for both scopes: pass `Api::namespaced(client, namespace)` for a
/// namespaced kind and `Api::all(client)` for a cluster-scoped one.
///
/// # Arguments
///
/// * `api` - The API the resource lives behind
/// * `resource` - The resource to add the finalizer to
/// * `finalizer` - The finalizer string to add
///
/// # Returns
///
/// Returns `Ok(())` if the finalizer was added or already present.
///
/// # Concurrency
///
/// The patch is a JSON Patch guarded by a `test` operation (mirroring
/// kube-runtime's finalizer helper), so a racing writer's finalizer edits are
/// never clobbered. On a conflict the API call fails and the returned error
/// triggers a requeue; the next reconciliation retries with fresh state.
///
/// # Errors
///
/// Returns an error if:
/// - The resource has other finalizers but no `resourceVersion` (no safe
///   guarded patch can be built)
/// - The API patch operation fails, including when a concurrent writer
///   modified `metadata.finalizers` and the guarded patch was rejected
///
/// # Example
///
/// ```rust,no_run
/// # use bindy_controller_sdk::finalizers::ensure_finalizer;
/// # use bindy_api::crd::ClusterBind9Provider;
/// # use kube::{Api, Client};
/// # async fn example(client: Client, provider: ClusterBind9Provider) {
/// const FINALIZER: &str = "bind9globalcluster.dns.firestoned.io/finalizer";
/// let api: Api<ClusterBind9Provider> = Api::all(client);
/// ensure_finalizer(&api, &provider, FINALIZER).await.unwrap();
/// # }
/// ```
pub async fn ensure_finalizer<K>(api: &Api<K>, resource: &K, finalizer: &str) -> Result<()>
where
    K: Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned,
{
    // Early return: finalizer already present
    if has_finalizer(resource, finalizer) {
        return Ok(());
    }

    let kind = K::kind(&());
    let object = display_name(resource);
    info!("Adding finalizer {finalizer} to {kind} {object}");

    let finalizers = resource.meta().finalizers.clone().unwrap_or_default();
    let patch = build_add_finalizer_patch(
        &finalizers,
        resource.meta().resource_version.as_deref(),
        finalizer,
    )?;

    api.patch(
        &resource.name_any(),
        &PatchParams::default(),
        &Patch::Json::<()>(patch),
    )
    .await
    .with_context(|| {
        format!(
            "failed to add finalizer {finalizer} to {object} \
             (possibly a concurrent finalizer edit; will retry on next reconciliation)"
        )
    })?;

    info!("Successfully added finalizer {finalizer} to {kind} {object}");
    Ok(())
}

/// Remove a finalizer from a resource.
///
/// This function removes the specified finalizer from the resource if present.
/// The operation is idempotent - calling it multiple times has no effect if
/// the finalizer is already absent.
///
/// **Note:** Typically you should use [`handle_deletion`] instead of calling
/// this function directly, as it performs cleanup before removing the finalizer.
///
/// # Arguments
///
/// * `api` - The API the resource lives behind (`Api::namespaced` or `Api::all`)
/// * `resource` - The resource to remove the finalizer from
/// * `finalizer` - The finalizer string to remove
///
/// # Returns
///
/// Returns `Ok(())` if the finalizer was removed or already absent.
///
/// # Concurrency
///
/// The patch is a JSON Patch that `test`s the exact array index before
/// removing it (mirroring kube-runtime's finalizer helper), so a racing
/// writer's finalizer edits are never clobbered. On a conflict the API call
/// fails and the returned error triggers a requeue; the next reconciliation
/// retries with fresh state.
///
/// # Errors
///
/// Returns an error if the API patch operation fails, including when a
/// concurrent writer modified `metadata.finalizers` and the guarded patch was
/// rejected.
pub async fn remove_finalizer<K>(api: &Api<K>, resource: &K, finalizer: &str) -> Result<()>
where
    K: Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned,
{
    let finalizers = resource.meta().finalizers.clone().unwrap_or_default();

    // Early return: finalizer already absent
    let Some(patch) = build_remove_finalizer_patch(&finalizers, finalizer)? else {
        return Ok(());
    };

    let kind = K::kind(&());
    let object = display_name(resource);
    info!("Removing finalizer {finalizer} from {kind} {object}");

    api.patch(
        &resource.name_any(),
        &PatchParams::default(),
        &Patch::Json::<()>(patch),
    )
    .await
    .with_context(|| {
        format!(
            "failed to remove finalizer {finalizer} from {object} \
             (possibly a concurrent finalizer edit; will retry on next reconciliation)"
        )
    })?;

    info!("Successfully removed finalizer {finalizer} from {kind} {object}");
    Ok(())
}

/// Handle resource deletion with cleanup and finalizer removal.
///
/// This function orchestrates the complete deletion process:
/// 1. Logs that the resource is being deleted
/// 2. Runs the `cleanup` closure, only when the finalizer is present
/// 3. Removes the finalizer to allow Kubernetes to delete the resource
///
/// This function should be called when a resource has a deletion timestamp.
/// It works for both scopes, through the `Api` the caller passes.
///
/// # Arguments
///
/// * `api` - The API the resource lives behind (`Api::namespaced` or `Api::all`)
/// * `resource` - The resource being deleted
/// * `finalizer` - The finalizer string to check and remove
/// * `cleanup` - Async closure that removes what the resource owns; runs only
///   when the finalizer is present, and the finalizer is removed only if it succeeds
///
/// # Returns
///
/// Returns `Ok(())` if cleanup and finalizer removal succeeded, or if the
/// finalizer was not present (nothing to do).
///
/// # Errors
///
/// Returns an error if:
/// - The cleanup operation fails
/// - The finalizer removal fails
///
/// If an error occurs, the finalizer will remain on the resource and deletion
/// will be blocked until the operation succeeds on a subsequent reconciliation.
///
/// # Example
///
/// ```text
/// use bindy_controller_sdk::finalizers::handle_deletion;
/// use bindy_api::crd::ClusterBind9Provider;
/// use kube::{Api, Client};
/// use anyhow::Result;
///
/// const FINALIZER: &str = "bind9globalcluster.dns.firestoned.io/finalizer";
///
/// async fn reconcile(client: Client, provider: ClusterBind9Provider) -> Result<()> {
///     let api: Api<ClusterBind9Provider> = Api::all(client);
///     if provider.metadata.deletion_timestamp.is_some() {
///         return handle_deletion(&api, &provider, FINALIZER, || async { Ok(()) }).await;
///     }
///     // Normal reconciliation...
///     Ok(())
/// }
/// ```
pub async fn handle_deletion<K, F, Fut>(
    api: &Api<K>,
    resource: &K,
    finalizer: &str,
    cleanup: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<()>>,
    K: Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned,
{
    let kind = K::kind(&());
    let object = display_name(resource);

    info!("{kind} {object} is being deleted");

    // Early return: without our finalizer there is nothing to clean up
    if !has_finalizer(resource, finalizer) {
        return Ok(());
    }

    info!("Running cleanup for {kind} {object}");

    // Perform cleanup operations
    cleanup().await?;

    // Remove the finalizer
    remove_finalizer(api, resource, finalizer).await
}

#[cfg(test)]
#[path = "finalizers_tests.rs"]
mod finalizers_tests;
