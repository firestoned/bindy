// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Generic resource helpers for Kubernetes resources.
//!
//! - [`create_or_apply`] creates a namespaced resource when it is missing and
//!   updates it with server-side apply (SSA) when it exists.
//! - [`create_or_force_apply`] POSTs a resource and, when it already exists,
//!   force-applies it with SSA.
//! - [`delete_if_present`] deletes a resource, treating "already absent" as
//!   success.
//! - [`controller_owner_reference`] builds the owner reference every bindy
//!   child carries.
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

use crate::kube_errors::{is_conflict, is_not_found};
use anyhow::{anyhow, Context as _, Result};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{DeleteParams, Patch, PatchParams, PostParams};
use kube::core::NamespaceResourceScope;
use kube::{Api, Client, Resource, ResourceExt};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::fmt::Debug;
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

/// Builds the controller owner reference that makes `owner` garbage-collect
/// the object carrying it.
///
/// Uses kube's [`Resource::controller_owner_ref`], so `apiVersion`, `kind`
/// and `name` come from the owner itself, and keeps the shape every bindy
/// child has always carried: `controller: true` and
/// `blockOwnerDeletion: true`.
///
/// # Arguments
///
/// * `owner` - The owning object, as read from the API server
///
/// # Returns
///
/// The owner reference pointing to `owner`.
///
/// # Errors
///
/// Returns an error when `owner` has no name or no (or an empty) UID. The API
/// server rejects or mis-resolves such a reference, so nothing is written
/// with one: an object that has not been read from the API server cannot own
/// anything.
pub fn controller_owner_reference<K>(owner: &K) -> Result<OwnerReference>
where
    K: Resource<DynamicType = ()>,
{
    let mut owner_ref = owner
        .controller_owner_ref(&())
        .filter(|r| !r.uid.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "cannot build an owner reference to {} {:?}: it has no name or UID",
                K::kind(&()),
                owner.meta().name
            )
        })?;
    owner_ref.block_owner_deletion = Some(true);
    Ok(owner_ref)
}

/// The object's name for logs and errors: `namespace/name`, or `name` for a
/// cluster-scoped object or a cluster-wide `Api`.
fn display_name(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        Some(namespace) => format!("{namespace}/{name}"),
        None => name.to_string(),
    }
}

/// Delete an object, treating "already absent" as success.
///
/// # Arguments
///
/// * `api` - The API the object lives behind (`Api::namespaced` or `Api::all`)
/// * `name` - The object's name
///
/// # Returns
///
/// `Ok(true)` when the delete was accepted (the object is gone or is being
/// deleted), `Ok(false)` when the API server reported it absent (HTTP 404).
///
/// # Errors
///
/// Returns every other error (403, 409, 429, 5xx, transport), with context
/// naming the kind and the object. The `kube::Error` stays in the chain, so
/// [`crate::kube_errors`] can still classify it.
pub async fn delete_if_present<K>(api: &Api<K>, name: &str) -> Result<bool>
where
    K: Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned,
{
    let kind = K::kind(&());
    match api.delete(name, &DeleteParams::default()).await {
        Ok(_) => {
            debug!("Deleted {kind} {name}");
            Ok(true)
        }
        Err(e) if is_not_found(&e) => {
            debug!("{kind} {name} already absent");
            Ok(false)
        }
        Err(e) => Err(anyhow::Error::from(e).context(format!("failed to delete {kind} {name}"))),
    }
}

/// The server-side apply body [`create_or_force_apply`] sends for `obj`.
///
/// Carries what the hand-rolled create-or-apply sites in the cluster
/// controllers sent: `apiVersion` and `kind` of `K`; in `metadata`, `name`,
/// `namespace` (when set), `labels` and `annotations` (each only when set)
/// and `ownerReferences` (always, `null` when unset); then every top-level
/// field of the serialized object other than `apiVersion`, `kind`,
/// `metadata` and `status` (for a custom resource, its `spec`).
///
/// # Arguments
///
/// * `obj` - The desired object
///
/// # Returns
///
/// The JSON body for a `Patch::Apply`.
///
/// # Errors
///
/// Returns an error when `obj` has no name or does not serialize to a JSON
/// object.
pub fn force_apply_body<K>(obj: &K) -> Result<serde_json::Value>
where
    K: Resource<DynamicType = ()> + serde::Serialize,
{
    let meta = obj.meta();
    let name = meta
        .name
        .as_deref()
        .ok_or_else(|| anyhow!("{} must have a name to be applied", K::kind(&())))?;

    let mut metadata = serde_json::Map::new();
    metadata.insert("name".to_string(), json!(name));
    if let Some(namespace) = &meta.namespace {
        metadata.insert("namespace".to_string(), json!(namespace));
    }
    if let Some(labels) = &meta.labels {
        metadata.insert("labels".to_string(), json!(labels));
    }
    if let Some(annotations) = &meta.annotations {
        metadata.insert("annotations".to_string(), json!(annotations));
    }
    metadata.insert("ownerReferences".to_string(), json!(meta.owner_references));

    let serde_json::Value::Object(fields) = serde_json::to_value(obj)? else {
        return Err(anyhow!(
            "{} {name} does not serialize to an object",
            K::kind(&())
        ));
    };

    let mut body = serde_json::Map::new();
    body.insert("apiVersion".to_string(), json!(K::api_version(&())));
    body.insert("kind".to_string(), json!(K::kind(&())));
    body.insert("metadata".to_string(), serde_json::Value::Object(metadata));
    for (key, value) in fields {
        if APPLY_BODY_EXCLUDED_FIELDS.contains(&key.as_str()) {
            continue;
        }
        body.insert(key, value);
    }
    Ok(serde_json::Value::Object(body))
}

/// Top-level fields [`force_apply_body`] does not copy from the object: the
/// type and metadata are rebuilt, and `status` belongs to the status
/// subresource.
const APPLY_BODY_EXCLUDED_FIELDS: [&str; 4] = ["apiVersion", "kind", "metadata", "status"];

/// Create an object; if it already exists, force server-side apply it.
///
/// POSTs `obj`. On a conflict (HTTP 409, `AlreadyExists`) it sends
/// [`force_apply_body`] as a server-side apply with `field_manager` and
/// `force: true`, which is what `ClusterBind9Provider` (managed
/// `Bind9Cluster`s) and `Bind9Cluster` (managed `Bind9Instance`s) did by
/// hand. Prefer [`create_or_apply`] for new code that has no reason to POST
/// first.
///
/// # Arguments
///
/// * `api` - The API the object lives behind (`Api::namespaced` or `Api::all`)
/// * `obj` - The desired object; its `metadata.name` names it
/// * `field_manager` - The server-side apply field manager
///
/// # Returns
///
/// The object as the API server stored it, from the create or the apply.
///
/// # Errors
///
/// Returns an error when `obj` has no name (nothing is sent), when the create
/// fails with anything but a conflict (nothing is applied), or when the apply
/// fails. Each error names the kind and the object and keeps the
/// `kube::Error` in its chain.
pub async fn create_or_force_apply<K>(api: &Api<K>, obj: &K, field_manager: &str) -> Result<K>
where
    K: Resource<DynamicType = ()> + Clone + Debug + serde::Serialize + DeserializeOwned,
{
    let kind = K::kind(&());
    // Built first: a nameless object fails before any request is sent.
    let body = force_apply_body(obj)?;
    let name = obj.meta().name.clone().unwrap_or_default();
    let object = display_name(obj.meta().namespace.as_deref(), &name);

    let create_error = match api.create(&PostParams::default(), obj).await {
        Ok(created) => {
            info!("Created {kind} {object}");
            return Ok(created);
        }
        Err(e) if is_conflict(&e) => e,
        Err(e) => {
            return Err(anyhow::Error::from(e).context(format!("failed to create {kind} {object}")))
        }
    };

    debug!("{kind} {object} already exists ({create_error}), applying the desired state");
    let applied = api
        .patch(
            &name,
            &PatchParams::apply(field_manager).force(),
            &Patch::Apply(&body),
        )
        .await
        .with_context(|| format!("failed to apply {kind} {object}"))?;
    info!("Applied {kind} {object}");
    Ok(applied)
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod resources_tests;
