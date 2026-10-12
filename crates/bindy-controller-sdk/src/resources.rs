// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Generic resource creation and update helpers for Kubernetes resources.
//!
//! [`create_or_apply`] creates a namespaced resource when it is missing and
//! updates it with server-side apply (SSA) when it exists.
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy_controller_sdk::resources::create_or_apply;
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
/// # use bindy_controller_sdk::resources::create_or_apply;
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

#[cfg(test)]
#[path = "resources_tests.rs"]
mod resources_tests;
