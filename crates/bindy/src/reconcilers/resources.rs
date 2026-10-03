// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Generic resource creation and update helpers for Kubernetes resources.
//!
//! This module provides reusable functions for creating and updating Kubernetes
//! resources with different strategies (Apply, Replace, or JSON Patch). It eliminates
//! duplicate create/update code across reconcilers.
//!
//! # Strategies
//!
//! - **Apply**: Uses server-side apply (SSA) for idempotent updates
//! - **Replace**: Uses replace operation (suitable for resources like Deployments)
//! - **Create with JSON Patch**: Try create, fallback to JSON patch on `AlreadyExists`
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy::reconcilers::resources::{create_or_apply, create_or_replace};
//! use k8s_openapi::api::core::v1::ServiceAccount;
//! use kube::Client;
//! use anyhow::Result;
//!
//! async fn example(client: &Client, namespace: &str, sa: ServiceAccount) -> Result<()> {
//!     // Using Apply strategy (server-side apply)
//!     create_or_apply(
//!         client,
//!         namespace,
//!         &sa,
//!         "bindy-controller"
//!     ).await?;
//!
//!     Ok(())
//! }
//! ```

use anyhow::Result;
use kube::api::{Patch, PatchParams, PostParams};
use kube::core::NamespaceResourceScope;
use kube::{Api, Client, Resource, ResourceExt};
use tracing::{debug, info};

/// Create or update a resource using server-side apply strategy.
///
/// This function checks if the resource exists. If it does, it patches using
/// server-side apply (SSA). Otherwise, it creates the resource.
///
/// Server-side apply is the recommended approach for managing resources in modern
/// Kubernetes as it provides better conflict resolution and field ownership tracking.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace where the resource should be created/updated
/// * `resource` - The resource to create or update
/// * `field_manager` - Field manager name for server-side apply (e.g., "bindy-controller")
///
/// # Returns
///
/// Returns `Ok(())` if the operation succeeded.
///
/// # Errors
///
/// Returns an error if:
/// - The resource has no name in its metadata
/// - API operations fail
///
/// # Example
///
/// ```rust,no_run
/// # use bindy::reconcilers::resources::create_or_apply;
/// # use k8s_openapi::api::core::v1::ServiceAccount;
/// # use kube::Client;
/// # async fn example(client: &Client, namespace: &str, sa: ServiceAccount) {
/// create_or_apply(client, namespace, &sa, "bindy-controller").await.unwrap();
/// # }
/// ```
pub async fn create_or_apply<T>(
    client: &Client,
    namespace: &str,
    resource: &T,
    field_manager: &str,
) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let name = resource
        .meta()
        .name
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Resource must have a name"))?;

    let api: Api<T> = Api::namespaced(client.clone(), namespace);

    debug!(
        namespace = %namespace,
        name = %name,
        kind = %T::kind(&()),
        "Creating or updating resource with Apply strategy"
    );

    if api.get(name).await.is_ok() {
        debug!(
            "{} {}/{} already exists, applying update",
            T::kind(&()),
            namespace,
            name
        );
        api.patch(
            name,
            &PatchParams::apply(field_manager),
            &Patch::Apply(resource),
        )
        .await?;
        info!("Updated {} {}/{}", T::kind(&()), namespace, name);
    } else {
        debug!(
            "{} {}/{} does not exist, creating",
            T::kind(&()),
            namespace,
            name
        );
        api.create(&PostParams::default(), resource).await?;
        info!("Created {} {}/{}", T::kind(&()), namespace, name);
    }

    Ok(())
}

/// Create or update a resource using replace strategy.
///
/// This function checks if the resource exists. If it does, it replaces the entire
/// resource. Otherwise, it creates the resource.
///
/// The replace strategy is suitable for resources like Deployments where you want
/// to completely replace the specification rather than merge changes.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace where the resource should be created/updated
/// * `resource` - The resource to create or update
///
/// # Returns
///
/// Returns `Ok(())` if the operation succeeded.
///
/// # Errors
///
/// Returns an error if:
/// - The resource has no name in its metadata
/// - API operations fail
///
/// # Example
///
/// ```rust,no_run
/// # use bindy::reconcilers::resources::create_or_replace;
/// # use k8s_openapi::api::apps::v1::Deployment;
/// # use kube::Client;
/// # async fn example(client: &Client, namespace: &str, deploy: Deployment) {
/// create_or_replace(client, namespace, &deploy).await.unwrap();
/// # }
/// ```
pub async fn create_or_replace<T>(client: &Client, namespace: &str, resource: &T) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let name = resource
        .meta()
        .name
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Resource must have a name"))?;

    let api: Api<T> = Api::namespaced(client.clone(), namespace);

    debug!(
        namespace = %namespace,
        name = %name,
        kind = %T::kind(&()),
        "Creating or replacing resource"
    );

    if api.get(name).await.is_ok() {
        info!("Replacing {} {}/{}", T::kind(&()), namespace, name);
        api.replace(name, &PostParams::default(), resource).await?;
    } else {
        info!("Creating {} {}/{}", T::kind(&()), namespace, name);
        api.create(&PostParams::default(), resource).await?;
    }

    Ok(())
}

/// Create or update a resource using JSON patch on conflict.
///
/// This function attempts to create the resource. If it already exists (`AlreadyExists` error),
/// it patches the resource using server-side apply with a provided JSON patch object.
///
/// This strategy is useful when you need full control over the patch structure, such as
/// when updating complex resources with owner references, labels, and annotations.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `namespace` - Namespace where the resource should be created/updated
/// * `resource` - The resource to create
/// * `patch_json` - JSON value containing the patch to apply if resource exists
/// * `field_manager` - Field manager name for server-side apply (e.g., "bindy-controller")
///
/// # Returns
///
/// Returns `Ok(())` if the operation succeeded.
///
/// # Errors
///
/// Returns an error if:
/// - The resource has no name in its metadata
/// - Create or patch operations fail (other than `AlreadyExists` on create)
///
/// # Example
///
/// ```rust,no_run
/// # use bindy::reconcilers::resources::create_or_patch_json;
/// # use bindy::crd::Bind9Instance;
/// # use kube::Client;
/// # use serde_json::json;
/// # async fn example(client: &Client, namespace: &str, instance: Bind9Instance) {
/// let patch = json!({
///     "apiVersion": "dns.firestoned.io/v1beta1",
///     "kind": "Bind9Instance",
///     "metadata": {
///         "name": "my-instance",
///         "namespace": namespace,
///     },
///     "spec": instance.spec,
/// });
///
/// create_or_patch_json(
///     client,
///     namespace,
///     &instance,
///     &patch,
///     "bindy-controller"
/// ).await.unwrap();
/// # }
/// ```
pub async fn create_or_patch_json<T>(
    client: &Client,
    namespace: &str,
    resource: &T,
    patch_json: &serde_json::Value,
    field_manager: &str,
) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let name = resource
        .meta()
        .name
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Resource must have a name"))?;

    let api: Api<T> = Api::namespaced(client.clone(), namespace);

    debug!(
        namespace = %namespace,
        name = %name,
        kind = %T::kind(&()),
        "Creating or patching resource with JSON strategy"
    );

    match api.create(&PostParams::default(), resource).await {
        Ok(_) => {
            info!("Created {} {}/{}", T::kind(&()), namespace, name);
            Ok(())
        }
        Err(e) => {
            // If already exists, patch it to ensure spec is up to date
            if e.to_string().contains("AlreadyExists") {
                debug!(
                    "{} {}/{} already exists, patching with updated spec",
                    T::kind(&()),
                    namespace,
                    name
                );

                api.patch(
                    name,
                    &PatchParams::apply(field_manager).force(),
                    &Patch::Apply(patch_json),
                )
                .await?;

                info!(
                    "Patched {} {}/{} with updated spec",
                    T::kind(&()),
                    namespace,
                    name
                );
                Ok(())
            } else {
                Err(e.into())
            }
        }
    }
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod resources_tests;
