// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Generic finalizer management for Kubernetes resources.
//!
//! This module provides reusable functions for adding, removing, and handling
//! finalizers on Kubernetes custom resources. It eliminates duplicate finalizer
//! management code across reconcilers.
//!
//! # Example
//!
//! ```rust,ignore
//! use bindy::reconcilers::finalizers::{ensure_finalizer, handle_deletion, FinalizerCleanup};
//! use bindy::crd::Bind9Cluster;
//! use kube::Client;
//! use anyhow::Result;
//!
//! const FINALIZER: &str = "bind9cluster.dns.firestoned.io/finalizer";
//!
//! #[async_trait::async_trait]
//! impl FinalizerCleanup for Bind9Cluster {
//!     async fn cleanup(&self, client: &Client) -> Result<()> {
//!         // Perform cleanup operations
//!         Ok(())
//!     }
//! }
//!
//! async fn reconcile(client: Client, cluster: Bind9Cluster) -> Result<()> {
//!     // Ensure finalizer is present
//!     ensure_finalizer(&client, &cluster, FINALIZER).await?;
//!
//!     // Handle deletion if resource is being deleted
//!     if cluster.metadata.deletion_timestamp.is_some() {
//!         return handle_deletion(&client, &cluster, FINALIZER).await;
//!     }
//!
//!     // Normal reconciliation logic...
//!     Ok(())
//! }
//! ```

use anyhow::{anyhow, Context as AnyhowContext, Result};
use kube::api::{Patch, PatchParams};
use kube::core::{ClusterResourceScope, NamespaceResourceScope};
use kube::{Api, Client, Resource, ResourceExt};
use serde_json::json;
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

/// Trait for resources that require cleanup operations when being deleted.
///
/// Implement this trait to define custom cleanup logic that should run
/// before a finalizer is removed from a resource.
#[async_trait::async_trait]
pub trait FinalizerCleanup: Resource + ResourceExt + Clone {
    /// Perform cleanup operations before the finalizer is removed.
    ///
    /// This method is called when a resource with a deletion timestamp
    /// still has the finalizer present. Implement any cleanup logic needed
    /// before the resource is fully deleted.
    ///
    /// # Arguments
    ///
    /// * `client` - Kubernetes client for accessing the API
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if cleanup succeeded, or an error if cleanup failed.
    /// If this method returns an error, the finalizer will NOT be removed and
    /// deletion will be blocked until cleanup succeeds.
    ///
    /// # Errors
    ///
    /// Should return an error if:
    /// - Child resources cannot be deleted
    /// - External systems cannot be cleaned up
    /// - Any other cleanup operation fails
    async fn cleanup(&self, client: &Client) -> Result<()>;
}

/// Add a finalizer to a resource if not already present.
///
/// This function checks if the specified finalizer is present on the resource,
/// and adds it if missing. The operation is idempotent - calling it multiple
/// times has no effect if the finalizer is already present.
///
/// # Arguments
///
/// * `client` - Kubernetes client for accessing the API
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
/// - The resource has no namespace (for namespaced resources)
/// - The API patch operation fails, including when a concurrent writer
///   modified `metadata.finalizers` and the guarded patch was rejected
///
/// # Example
///
/// ```rust,no_run
/// # use bindy::reconcilers::finalizers::ensure_finalizer;
/// # use bindy::crd::Bind9Cluster;
/// # use kube::Client;
/// # async fn example(client: Client, cluster: Bind9Cluster) {
/// const FINALIZER: &str = "bind9cluster.dns.firestoned.io/finalizer";
/// ensure_finalizer(&client, &cluster, FINALIZER).await.unwrap();
/// # }
/// ```
pub async fn ensure_finalizer<T>(client: &Client, resource: &T, finalizer: &str) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let namespace = resource.namespace().unwrap_or_default();
    let name = resource.name_any();

    let finalizers = resource.meta().finalizers.clone().unwrap_or_default();

    // Early return: finalizer already present
    if finalizers.iter().any(|f| f == finalizer) {
        return Ok(());
    }

    info!(
        "Adding finalizer {} to {}/{} {}",
        finalizer,
        namespace,
        name,
        T::kind(&())
    );

    let patch = build_add_finalizer_patch(
        &finalizers,
        resource.meta().resource_version.as_deref(),
        finalizer,
    )?;

    let api: Api<T> = Api::namespaced(client.clone(), &namespace);
    api.patch(&name, &PatchParams::default(), &Patch::Json::<()>(patch))
        .await
        .with_context(|| {
            format!(
                "failed to add finalizer {finalizer} to {namespace}/{name} \
                 (possibly a concurrent finalizer edit; will retry on next reconciliation)"
            )
        })?;

    info!(
        "Successfully added finalizer {} to {}/{} {}",
        finalizer,
        namespace,
        name,
        T::kind(&())
    );

    Ok(())
}

/// Remove a finalizer from a resource.
///
/// This function removes the specified finalizer from the resource if present.
/// The operation is idempotent - calling it multiple times has no effect if
/// the finalizer is already absent.
///
/// **Note:** Typically you should use `handle_deletion()` instead of calling
/// this function directly, as it performs cleanup before removing the finalizer.
///
/// # Arguments
///
/// * `client` - Kubernetes client for accessing the API
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
/// Returns an error if:
/// - The resource has no namespace (for namespaced resources)
/// - The API patch operation fails, including when a concurrent writer
///   modified `metadata.finalizers` and the guarded patch was rejected
pub async fn remove_finalizer<T>(client: &Client, resource: &T, finalizer: &str) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let namespace = resource.namespace().unwrap_or_default();
    let name = resource.name_any();

    let finalizers = resource.meta().finalizers.clone().unwrap_or_default();

    // Early return: finalizer already absent
    let Some(patch) = build_remove_finalizer_patch(&finalizers, finalizer)? else {
        return Ok(());
    };

    info!(
        "Removing finalizer {} from {}/{} {}",
        finalizer,
        namespace,
        name,
        T::kind(&())
    );

    let api: Api<T> = Api::namespaced(client.clone(), &namespace);
    api.patch(&name, &PatchParams::default(), &Patch::Json::<()>(patch))
        .await
        .with_context(|| {
            format!(
                "failed to remove finalizer {finalizer} from {namespace}/{name} \
                 (possibly a concurrent finalizer edit; will retry on next reconciliation)"
            )
        })?;

    info!(
        "Successfully removed finalizer {} from {}/{} {}",
        finalizer,
        namespace,
        name,
        T::kind(&())
    );

    Ok(())
}

/// Handle resource deletion with cleanup and finalizer removal.
///
/// This function orchestrates the complete deletion process:
/// 1. Logs that the resource is being deleted
/// 2. Calls the resource's `cleanup()` method to perform cleanup operations
/// 3. Removes the finalizer to allow Kubernetes to delete the resource
///
/// This function should be called when a resource has a deletion timestamp
/// and the finalizer is still present.
///
/// # Arguments
///
/// * `client` - Kubernetes client for accessing the API
/// * `resource` - The resource being deleted
/// * `finalizer` - The finalizer string to check and remove
///
/// # Returns
///
/// Returns `Ok(())` if cleanup and finalizer removal succeeded.
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
/// use bindy::reconcilers::finalizers::{handle_deletion, FinalizerCleanup};
/// use bindy::crd::Bind9Cluster;
/// use kube::Client;
/// use anyhow::Result;
///
/// const FINALIZER: &str = "bind9cluster.dns.firestoned.io/finalizer";
///
/// async fn reconcile(client: Client, cluster: Bind9Cluster) -> Result<()> {
///     if cluster.metadata.deletion_timestamp.is_some() {
///         return handle_deletion(&client, &cluster, FINALIZER).await;
///     }
///     // Normal reconciliation...
///     Ok(())
/// }
/// ```
pub async fn handle_deletion<T>(client: &Client, resource: &T, finalizer: &str) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + ResourceExt
        + FinalizerCleanup
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let namespace = resource.namespace().unwrap_or_default();
    let name = resource.name_any();

    info!("{} {}/{} is being deleted", T::kind(&()), namespace, name);

    // Only proceed if the finalizer is present
    if resource
        .meta()
        .finalizers
        .as_ref()
        .is_some_and(|f| f.contains(&finalizer.to_string()))
    {
        info!(
            "Running cleanup for {} {}/{}",
            T::kind(&()),
            namespace,
            name
        );

        // Perform cleanup operations
        resource.cleanup(client).await?;

        // Remove the finalizer
        remove_finalizer(client, resource, finalizer).await?;
    }

    Ok(())
}

/// Add a finalizer to a cluster-scoped resource if not already present.
///
/// This function is similar to `ensure_finalizer()` but works with cluster-scoped
/// resources that don't have a namespace. It checks if the specified finalizer is
/// present on the resource, and adds it if missing.
///
/// # Arguments
///
/// * `client` - Kubernetes client for accessing the API
/// * `resource` - The cluster-scoped resource to add the finalizer to
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
/// Returns an error if the API patch operation fails, including when a
/// concurrent writer modified `metadata.finalizers` and the guarded patch
/// was rejected.
///
/// # Example
///
/// ```rust,no_run
/// # use bindy::reconcilers::finalizers::ensure_cluster_finalizer;
/// # use bindy::crd::ClusterBind9Provider;
/// # use kube::Client;
/// # async fn example(client: Client, cluster: ClusterBind9Provider) {
/// const FINALIZER: &str = "bind9globalcluster.dns.firestoned.io/finalizer";
/// ensure_cluster_finalizer(&client, &cluster, FINALIZER).await.unwrap();
/// # }
/// ```
pub async fn ensure_cluster_finalizer<T>(
    client: &Client,
    resource: &T,
    finalizer: &str,
) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = ClusterResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let name = resource.name_any();

    let finalizers = resource.meta().finalizers.clone().unwrap_or_default();

    // Early return: finalizer already present
    if finalizers.iter().any(|f| f == finalizer) {
        return Ok(());
    }

    info!(
        "Adding finalizer {} to {} {}",
        finalizer,
        T::kind(&()),
        name
    );

    let patch = build_add_finalizer_patch(
        &finalizers,
        resource.meta().resource_version.as_deref(),
        finalizer,
    )?;

    let api: Api<T> = Api::all(client.clone());
    api.patch(&name, &PatchParams::default(), &Patch::Json::<()>(patch))
        .await
        .with_context(|| {
            format!(
                "failed to add finalizer {finalizer} to {name} \
                 (possibly a concurrent finalizer edit; will retry on next reconciliation)"
            )
        })?;

    info!(
        "Successfully added finalizer {} to {} {}",
        finalizer,
        T::kind(&()),
        name
    );

    Ok(())
}

/// Remove a finalizer from a cluster-scoped resource.
///
/// This function removes the specified finalizer from the cluster-scoped resource
/// if present. The operation is idempotent - calling it multiple times has no effect
/// if the finalizer is already absent.
///
/// **Note:** Typically you should use `handle_cluster_deletion()` instead of calling
/// this function directly, as it performs cleanup before removing the finalizer.
///
/// # Arguments
///
/// * `client` - Kubernetes client for accessing the API
/// * `resource` - The cluster-scoped resource to remove the finalizer from
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
/// concurrent writer modified `metadata.finalizers` and the guarded patch
/// was rejected.
pub async fn remove_cluster_finalizer<T>(
    client: &Client,
    resource: &T,
    finalizer: &str,
) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = ClusterResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let name = resource.name_any();

    let finalizers = resource.meta().finalizers.clone().unwrap_or_default();

    // Early return: finalizer already absent
    let Some(patch) = build_remove_finalizer_patch(&finalizers, finalizer)? else {
        return Ok(());
    };

    info!(
        "Removing finalizer {} from {} {}",
        finalizer,
        T::kind(&()),
        name
    );

    let api: Api<T> = Api::all(client.clone());
    api.patch(&name, &PatchParams::default(), &Patch::Json::<()>(patch))
        .await
        .with_context(|| {
            format!(
                "failed to remove finalizer {finalizer} from {name} \
                 (possibly a concurrent finalizer edit; will retry on next reconciliation)"
            )
        })?;

    info!(
        "Successfully removed finalizer {} from {} {}",
        finalizer,
        T::kind(&()),
        name
    );

    Ok(())
}

/// Handle cluster-scoped resource deletion with cleanup and finalizer removal.
///
/// This function orchestrates the complete deletion process for cluster-scoped resources:
/// 1. Logs that the resource is being deleted
/// 2. Calls the resource's `cleanup()` method to perform cleanup operations
/// 3. Removes the finalizer to allow Kubernetes to delete the resource
///
/// This function should be called when a cluster-scoped resource has a deletion
/// timestamp and the finalizer is still present.
///
/// # Arguments
///
/// * `client` - Kubernetes client for accessing the API
/// * `resource` - The cluster-scoped resource being deleted
/// * `finalizer` - The finalizer string to check and remove
///
/// # Returns
///
/// Returns `Ok(())` if cleanup and finalizer removal succeeded.
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
/// use bindy::reconcilers::finalizers::{handle_cluster_deletion, FinalizerCleanup};
/// use bindy::crd::ClusterBind9Provider;
/// use kube::Client;
/// use anyhow::Result;
///
/// const FINALIZER: &str = "bind9globalcluster.dns.firestoned.io/finalizer";
///
/// async fn reconcile(client: Client, cluster: ClusterBind9Provider) -> Result<()> {
///     if cluster.metadata.deletion_timestamp.is_some() {
///         return handle_cluster_deletion(&client, &cluster, FINALIZER).await;
///     }
///     // Normal reconciliation...
///     Ok(())
/// }
/// ```
pub async fn handle_cluster_deletion<T>(
    client: &Client,
    resource: &T,
    finalizer: &str,
) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = ClusterResourceScope>
        + ResourceExt
        + FinalizerCleanup
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let name = resource.name_any();

    info!("{} {} is being deleted", T::kind(&()), name);

    // Only proceed if the finalizer is present
    if resource
        .meta()
        .finalizers
        .as_ref()
        .is_some_and(|f| f.contains(&finalizer.to_string()))
    {
        info!("Running cleanup for {} {}", T::kind(&()), name);

        // Perform cleanup operations
        resource.cleanup(client).await?;

        // Remove the finalizer
        remove_cluster_finalizer(client, resource, finalizer).await?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "finalizers_tests.rs"]
mod finalizers_tests;
