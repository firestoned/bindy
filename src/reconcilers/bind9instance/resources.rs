// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Kubernetes resource lifecycle management for `Bind9Instance` resources.
//!
//! This module handles creating, updating, and deleting all Kubernetes resources
//! needed to run a BIND9 DNS server (`ConfigMap`, Deployment, Service, etc.).

#[allow(clippy::wildcard_imports)]
use super::types::*;

use crate::bind9::Bind9Manager;
use crate::bind9_resources::{
    build_configmap, build_deployment, build_service, build_service_account,
};
use crate::constants::{API_GROUP_VERSION, KIND_BIND9_INSTANCE};
use crate::reconcilers::resources::{create_or_apply, create_or_replace};

pub(super) async fn create_or_update_resources(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
) -> Result<(
    Option<Bind9Cluster>,
    Option<crate::crd::ClusterBind9Provider>,
)> {
    debug!(
        namespace = %namespace,
        name = %name,
        "Creating or updating Kubernetes resources"
    );

    // Fetch the Bind9Cluster (namespace-scoped) if referenced
    let cluster = if instance.spec.cluster_ref.is_empty() {
        debug!("No cluster reference, proceeding with standalone instance");
        None
    } else {
        debug!(cluster_ref = %instance.spec.cluster_ref, "Fetching Bind9Cluster");
        let cluster_api: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
        match cluster_api.get(&instance.spec.cluster_ref).await {
            Ok(cluster) => {
                debug!(
                    cluster_name = %instance.spec.cluster_ref,
                    "Successfully fetched Bind9Cluster"
                );
                info!(
                    "Found Bind9Cluster: {}/{}",
                    namespace, instance.spec.cluster_ref
                );
                Some(cluster)
            }
            Err(e) => {
                warn!(
                    "Failed to fetch Bind9Cluster {}/{}: {}. Proceeding with instance-only config.",
                    namespace, instance.spec.cluster_ref, e
                );
                None
            }
        }
    };

    // Fetch the ClusterBind9Provider (cluster-scoped) if no namespace-scoped cluster was found
    let cluster_provider = if cluster.is_none() && !instance.spec.cluster_ref.is_empty() {
        debug!(cluster_ref = %instance.spec.cluster_ref, "Fetching ClusterBind9Provider");
        let cluster_provider_api: Api<crate::crd::ClusterBind9Provider> = Api::all(client.clone());
        match cluster_provider_api.get(&instance.spec.cluster_ref).await {
            Ok(gc) => {
                debug!(
                    cluster_name = %instance.spec.cluster_ref,
                    "Successfully fetched ClusterBind9Provider"
                );
                info!("Found ClusterBind9Provider: {}", instance.spec.cluster_ref);
                Some(gc)
            }
            Err(e) => {
                warn!(
                    "Failed to fetch ClusterBind9Provider {}: {}. Proceeding with instance-only config.",
                    instance.spec.cluster_ref, e
                );
                None
            }
        }
    } else {
        None
    };

    // 1. Create/update ServiceAccount (must be first, as deployment will reference it)
    debug!("Step 1: Creating/updating ServiceAccount");
    create_or_update_service_account(client, namespace, instance).await?;

    // 2. Create/update RNDC Secret (must be before deployment, as it will be mounted)
    debug!("Step 2: Creating/updating RNDC Secret");
    create_or_update_rndc_secret(client, namespace, name, instance).await?;

    // 3. Create/update ConfigMap
    debug!("Step 3: Creating/updating ConfigMap");
    create_or_update_configmap(
        client,
        namespace,
        name,
        instance,
        cluster.as_ref(),
        cluster_provider.as_ref(),
    )
    .await?;

    // 4. Create/update Deployment
    debug!("Step 4: Creating/updating Deployment");
    create_or_update_deployment(
        client,
        namespace,
        name,
        instance,
        cluster.as_ref(),
        cluster_provider.as_ref(),
    )
    .await?;

    // 5. Create/update Service
    debug!("Step 5: Creating/updating Service");
    create_or_update_service(
        client,
        namespace,
        name,
        instance,
        cluster.as_ref(),
        cluster_provider.as_ref(),
    )
    .await?;

    debug!("Successfully created/updated all resources");
    Ok((cluster, cluster_provider))
}

/// Create or update the `ServiceAccount` for BIND9 pods
async fn create_or_update_service_account(
    client: &Client,
    namespace: &str,
    instance: &Bind9Instance,
) -> Result<()> {
    let service_account = build_service_account(namespace, instance);
    create_or_apply(client, namespace, &service_account, "bindy-controller").await
}

/// Create or update the RNDC Secret for BIND9 remote control
async fn create_or_update_rndc_secret(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
) -> Result<()> {
    let secret_name = format!("{name}-rndc-key");
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);

    // Check if secret already exists
    match secret_api.get(&secret_name).await {
        Ok(existing_secret) => {
            // Secret exists, don't regenerate the key
            info!(
                "RNDC Secret {}/{} already exists, skipping",
                namespace, secret_name
            );
            // Verify it has the required keys
            if let Some(ref data) = existing_secret.data {
                if !data.contains_key("key-name")
                    || !data.contains_key("algorithm")
                    || !data.contains_key("secret")
                {
                    warn!(
                        "RNDC Secret {}/{} is missing required keys, will recreate",
                        namespace, secret_name
                    );
                    // Delete and recreate
                    secret_api
                        .delete(&secret_name, &kube::api::DeleteParams::default())
                        .await?;
                } else {
                    return Ok(());
                }
            } else {
                warn!(
                    "RNDC Secret {}/{} has no data, will recreate",
                    namespace, secret_name
                );
                secret_api
                    .delete(&secret_name, &kube::api::DeleteParams::default())
                    .await?;
            }
        }
        Err(_) => {
            info!(
                "RNDC Secret {}/{} does not exist, creating",
                namespace, secret_name
            );
        }
    }

    // Generate new RNDC key
    let mut key_data = Bind9Manager::generate_rndc_key();
    key_data.name = "bindy-operator".to_string();

    // Create Secret data
    let secret_data = Bind9Manager::create_rndc_secret_data(&key_data);

    // Create owner reference to the Bind9Instance
    let owner_ref = OwnerReference {
        api_version: API_GROUP_VERSION.to_string(),
        kind: KIND_BIND9_INSTANCE.to_string(),
        name: name.to_string(),
        uid: instance.metadata.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    };

    // Build Secret object
    let secret = Secret {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(secret_name.clone()),
            namespace: Some(namespace.to_string()),
            owner_references: Some(vec![owner_ref]),
            ..Default::default()
        },
        string_data: Some(secret_data),
        ..Default::default()
    };

    // Create the secret
    info!("Creating RNDC Secret {}/{}", namespace, secret_name);
    secret_api.create(&PostParams::default(), &secret).await?;

    Ok(())
}

/// Create or update the `ConfigMap` for BIND9 configuration
///
/// **Note:** If the instance belongs to a cluster (has `spec.clusterRef`), this function
/// does NOT create an instance-specific `ConfigMap`. Instead, the instance will use the
/// cluster-level shared `ConfigMap` created by the `Bind9Cluster` reconciler.
async fn create_or_update_configmap(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    _cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Result<()> {
    // If instance belongs to a cluster, skip ConfigMap creation
    // The cluster creates a shared ConfigMap that all instances use
    if !instance.spec.cluster_ref.is_empty() {
        debug!(
            "Instance {}/{} belongs to cluster '{}', using cluster ConfigMap",
            namespace, name, instance.spec.cluster_ref
        );
        return Ok(());
    }

    // Instance is standalone (no clusterRef), create instance-specific ConfigMap
    info!(
        "Instance {}/{} is standalone, creating instance-specific ConfigMap",
        namespace, name
    );

    // Get role-specific allow-transfer override from cluster config
    // Note: We only reach this code for standalone instances (no clusterRef),
    // so we should only have a namespace-scoped cluster here, not a global cluster
    let role_allow_transfer = cluster.and_then(|c| match instance.spec.role {
        crate::crd::ServerRole::Primary => c
            .spec
            .common
            .primary
            .as_ref()
            .and_then(|p| p.allow_transfer.as_ref()),
        crate::crd::ServerRole::Secondary => c
            .spec
            .common
            .secondary
            .as_ref()
            .and_then(|s| s.allow_transfer.as_ref()),
    });

    // build_configmap returns None if custom ConfigMaps are referenced
    if let Some(configmap) =
        build_configmap(name, namespace, instance, cluster, role_allow_transfer)
    {
        let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
        let cm_name = format!("{name}-config");

        if (cm_api.get(&cm_name).await).is_ok() {
            // ConfigMap exists, update it
            info!("Updating ConfigMap {}/{}", namespace, cm_name);
            cm_api
                .replace(&cm_name, &PostParams::default(), &configmap)
                .await?;
        } else {
            // ConfigMap doesn't exist, create it
            info!("Creating ConfigMap {}/{}", namespace, cm_name);
            cm_api.create(&PostParams::default(), &configmap).await?;
        }
    } else {
        info!(
            "Using custom ConfigMaps for {}/{}, skipping ConfigMap creation",
            namespace, name
        );
    }

    Ok(())
}

/// Create or update the Deployment for BIND9
async fn create_or_update_deployment(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Result<()> {
    let deployment = build_deployment(name, namespace, instance, cluster, cluster_provider);
    create_or_replace(client, namespace, &deployment).await
}

/// Create or update the Service for BIND9
async fn create_or_update_service(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Result<()> {
    // Get custom service spec based on instance role from cluster (namespace-scoped or global)
    let custom_spec = cluster
        .and_then(|c| match instance.spec.role {
            crate::crd::ServerRole::Primary => c
                .spec
                .common
                .primary
                .as_ref()
                .and_then(|p| p.service.as_ref()),
            crate::crd::ServerRole::Secondary => c
                .spec
                .common
                .secondary
                .as_ref()
                .and_then(|s| s.service.as_ref()),
        })
        .or_else(|| {
            // Fall back to global cluster if no namespace-scoped cluster
            cluster_provider.and_then(|gc| match instance.spec.role {
                crate::crd::ServerRole::Primary => gc
                    .spec
                    .common
                    .primary
                    .as_ref()
                    .and_then(|p| p.service.as_ref()),
                crate::crd::ServerRole::Secondary => gc
                    .spec
                    .common
                    .secondary
                    .as_ref()
                    .and_then(|s| s.service.as_ref()),
            })
        });

    let service = build_service(name, namespace, instance, custom_spec);
    let svc_api: Api<Service> = Api::namespaced(client.clone(), namespace);

    if let Ok(existing) = svc_api.get(name).await {
        // Service exists, update it (preserve clusterIP)
        info!("Updating Service {}/{}", namespace, name);
        let mut updated_service = service;
        if let Some(ref mut spec) = updated_service.spec {
            if let Some(ref existing_spec) = existing.spec {
                spec.cluster_ip.clone_from(&existing_spec.cluster_ip);
                spec.cluster_ips.clone_from(&existing_spec.cluster_ips);
            }
        }
        svc_api
            .replace(name, &PostParams::default(), &updated_service)
            .await?;
    } else {
        // Service doesn't exist, create it
        info!("Creating Service {}/{}", namespace, name);
        svc_api.create(&PostParams::default(), &service).await?;
    }

    Ok(())
}

/// Deletes all resources associated with a `Bind9Instance`.
///
/// Cleans up Kubernetes resources in reverse order:
/// 1. Service
/// 2. Deployment
/// 3. `ConfigMap`
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance` - The `Bind9Instance` resource to delete
///
/// # Returns
///
/// * `Ok(())` - If deletion succeeded or resources didn't exist
/// * `Err(_)` - If a critical error occurred during deletion
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail during resource deletion.
pub async fn delete_bind9instance(ctx: Arc<Context>, instance: Bind9Instance) -> Result<()> {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    info!("Deleting Bind9Instance: {}/{}", namespace, name);

    // Delete resources in reverse order (Service, Deployment, ConfigMap)
    delete_resources(&ctx.client, &namespace, &name).await?;

    info!("Successfully deleted resources for {}/{}", namespace, name);

    Ok(())
}

/// Delete all Kubernetes resources for a `Bind9Instance`
pub(super) async fn delete_resources(client: &Client, namespace: &str, name: &str) -> Result<()> {
    let delete_params = kube::api::DeleteParams::default();

    // 1. Delete Service (if it exists)
    let svc_api: Api<Service> = Api::namespaced(client.clone(), namespace);
    match svc_api.delete(name, &delete_params).await {
        Ok(_) => info!("Deleted Service {}/{}", namespace, name),
        Err(e) => warn!("Failed to delete Service {}/{}: {}", namespace, name, e),
    }

    // 2. Delete Deployment (if it exists)
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    match deploy_api.delete(name, &delete_params).await {
        Ok(_) => info!("Deleted Deployment {}/{}", namespace, name),
        Err(e) => warn!("Failed to delete Deployment {}/{}: {}", namespace, name, e),
    }

    // 3. Delete ConfigMap (if it exists)
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let cm_name = format!("{name}-config");
    match cm_api.delete(&cm_name, &delete_params).await {
        Ok(_) => info!("Deleted ConfigMap {}/{}", namespace, cm_name),
        Err(e) => warn!(
            "Failed to delete ConfigMap {}/{}: {}",
            namespace, cm_name, e
        ),
    }

    // 4. Delete RNDC Secret (if it exists)
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret_name = format!("{name}-rndc-key");
    match secret_api.delete(&secret_name, &delete_params).await {
        Ok(_) => info!("Deleted Secret {}/{}", namespace, secret_name),
        Err(e) => warn!(
            "Failed to delete Secret {}/{}: {}",
            namespace, secret_name, e
        ),
    }

    // 5. Delete ServiceAccount (if it exists and is owned by this instance)
    let sa_api: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    let sa_name = crate::constants::BIND9_SERVICE_ACCOUNT;
    match sa_api.get(sa_name).await {
        Ok(sa) => {
            // Check if this instance owns the ServiceAccount
            let is_owner = sa
                .metadata
                .owner_references
                .as_ref()
                .is_some_and(|owners| owners.iter().any(|owner| owner.name == name));

            if is_owner {
                match sa_api.delete(sa_name, &delete_params).await {
                    Ok(_) => info!("Deleted ServiceAccount {}/{}", namespace, sa_name),
                    Err(e) => warn!(
                        "Failed to delete ServiceAccount {}/{}: {}",
                        namespace, sa_name, e
                    ),
                }
            } else {
                debug!(
                    "ServiceAccount {}/{} is not owned by this instance, skipping deletion",
                    namespace, sa_name
                );
            }
        }
        Err(e) => {
            debug!(
                "ServiceAccount {}/{} does not exist or cannot be retrieved: {}",
                namespace, sa_name, e
            );
        }
    }

    Ok(())
}
