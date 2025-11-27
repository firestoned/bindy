//! BIND9 instance reconciliation logic.
//!
//! This module handles the lifecycle of BIND9 DNS server deployments in Kubernetes.
//! It creates and manages Deployments, ConfigMaps, and Services for each Bind9Instance.

use crate::bind9_resources::{build_configmap, build_deployment, build_service};
use crate::crd::{Bind9Instance, Bind9InstanceStatus, Condition};
use anyhow::Result;
use chrono::Utc;
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{ConfigMap, Service},
};
use kube::{
    api::{Patch, PatchParams, PostParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use tracing::{error, info, warn};

/// Reconciles a Bind9Instance resource.
///
/// Creates or updates all Kubernetes resources needed to run a BIND9 DNS server:
/// - ConfigMap with BIND9 configuration files
/// - Deployment with BIND9 container pods
/// - Service for DNS traffic (TCP/UDP port 53)
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance` - The Bind9Instance resource to reconcile
///
/// # Returns
///
/// * `Ok(())` - If reconciliation succeeded
/// * `Err(_)` - If resource creation/update failed
///
/// # Example
///
/// ```rust,no_run
/// use bindy::reconcilers::reconcile_bind9instance;
/// use bindy::crd::Bind9Instance;
/// use kube::Client;
///
/// async fn handle_instance(instance: Bind9Instance) -> anyhow::Result<()> {
///     let client = Client::try_default().await?;
///     reconcile_bind9instance(client, instance).await?;
///     Ok(())
/// }
/// ```
pub async fn reconcile_bind9instance(client: Client, instance: Bind9Instance) -> Result<()> {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    info!("Reconciling Bind9Instance: {}/{}", namespace, name);

    let spec = &instance.spec;
    let replicas = spec.replicas.unwrap_or(1);
    let version = spec.version.as_deref().unwrap_or("9.18");

    info!(
        "Bind9Instance {} configured with {} replicas, version {}",
        name, replicas, version
    );

    // Create or update resources
    match create_or_update_resources(&client, &namespace, &name, &instance).await {
        Ok(_) => {
            info!(
                "Successfully created/updated resources for {}/{}",
                namespace, name
            );

            // Update status to show it's ready
            update_status(
                &client,
                &instance,
                "Ready",
                "True",
                &format!("Bind9Instance configured with {} replicas", replicas),
                replicas,
                replicas,
            )
            .await?;
        }
        Err(e) => {
            error!(
                "Failed to create/update resources for {}/{}: {}",
                namespace, name, e
            );

            // Update status to show error
            update_status(
                &client,
                &instance,
                "Ready",
                "False",
                &format!("Failed to create resources: {}", e),
                replicas,
                0,
            )
            .await?;

            return Err(e);
        }
    }

    Ok(())
}

/// Create or update all Kubernetes resources for a Bind9Instance
async fn create_or_update_resources(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
) -> Result<()> {
    // 1. Create/update ConfigMap
    create_or_update_configmap(client, namespace, name, instance).await?;

    // 2. Create/update Deployment
    create_or_update_deployment(client, namespace, name, instance).await?;

    // 3. Create/update Service
    create_or_update_service(client, namespace, name).await?;

    Ok(())
}

/// Create or update the ConfigMap for BIND9 configuration
async fn create_or_update_configmap(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
) -> Result<()> {
    let configmap = build_configmap(name, namespace, instance);
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let cm_name = format!("{}-config", name);

    match cm_api.get(&cm_name).await {
        Ok(_) => {
            // ConfigMap exists, update it
            info!("Updating ConfigMap {}/{}", namespace, cm_name);
            cm_api
                .replace(&cm_name, &PostParams::default(), &configmap)
                .await?;
        }
        Err(_) => {
            // ConfigMap doesn't exist, create it
            info!("Creating ConfigMap {}/{}", namespace, cm_name);
            cm_api.create(&PostParams::default(), &configmap).await?;
        }
    }

    Ok(())
}

/// Create or update the Deployment for BIND9
async fn create_or_update_deployment(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
) -> Result<()> {
    let deployment = build_deployment(name, namespace, instance);
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    match deploy_api.get(name).await {
        Ok(_) => {
            // Deployment exists, update it
            info!("Updating Deployment {}/{}", namespace, name);
            deploy_api
                .replace(name, &PostParams::default(), &deployment)
                .await?;
        }
        Err(_) => {
            // Deployment doesn't exist, create it
            info!("Creating Deployment {}/{}", namespace, name);
            deploy_api
                .create(&PostParams::default(), &deployment)
                .await?;
        }
    }

    Ok(())
}

/// Create or update the Service for BIND9
async fn create_or_update_service(client: &Client, namespace: &str, name: &str) -> Result<()> {
    let service = build_service(name, namespace);
    let svc_api: Api<Service> = Api::namespaced(client.clone(), namespace);

    match svc_api.get(name).await {
        Ok(existing) => {
            // Service exists, update it (preserve clusterIP)
            info!("Updating Service {}/{}", namespace, name);
            let mut updated_service = service;
            if let Some(ref mut spec) = updated_service.spec {
                if let Some(ref existing_spec) = existing.spec {
                    spec.cluster_ip = existing_spec.cluster_ip.clone();
                    spec.cluster_ips = existing_spec.cluster_ips.clone();
                }
            }
            svc_api
                .replace(name, &PostParams::default(), &updated_service)
                .await?;
        }
        Err(_) => {
            // Service doesn't exist, create it
            info!("Creating Service {}/{}", namespace, name);
            svc_api.create(&PostParams::default(), &service).await?;
        }
    }

    Ok(())
}

/// Deletes all resources associated with a Bind9Instance.
///
/// Cleans up Kubernetes resources in reverse order:
/// 1. Service
/// 2. Deployment
/// 3. ConfigMap
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance` - The Bind9Instance resource to delete
///
/// # Returns
///
/// * `Ok(())` - If deletion succeeded or resources didn't exist
/// * `Err(_)` - If a critical error occurred during deletion
pub async fn delete_bind9instance(client: Client, instance: Bind9Instance) -> Result<()> {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    info!("Deleting Bind9Instance: {}/{}", namespace, name);

    // Delete resources in reverse order (Service, Deployment, ConfigMap)
    delete_resources(&client, &namespace, &name).await?;

    info!("Successfully deleted resources for {}/{}", namespace, name);

    Ok(())
}

/// Delete all Kubernetes resources for a Bind9Instance
async fn delete_resources(client: &Client, namespace: &str, name: &str) -> Result<()> {
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
    let cm_name = format!("{}-config", name);
    match cm_api.delete(&cm_name, &delete_params).await {
        Ok(_) => info!("Deleted ConfigMap {}/{}", namespace, cm_name),
        Err(e) => warn!(
            "Failed to delete ConfigMap {}/{}: {}",
            namespace, cm_name, e
        ),
    }

    Ok(())
}

/// Update the status of a Bind9Instance
async fn update_status(
    client: &Client,
    instance: &Bind9Instance,
    condition_type: &str,
    status: &str,
    message: &str,
    replicas: i32,
    ready_replicas: i32,
) -> Result<()> {
    let api: Api<Bind9Instance> =
        Api::namespaced(client.clone(), &instance.namespace().unwrap_or_default());

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(condition_type.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(Utc::now().to_rfc3339()),
    };

    let status = Bind9InstanceStatus {
        conditions: vec![condition],
        observed_generation: instance.metadata.generation,
        replicas: Some(replicas),
        ready_replicas: Some(ready_replicas),
    };

    let patch = json!({ "status": status });
    api.patch_status(
        &instance.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(())
}
