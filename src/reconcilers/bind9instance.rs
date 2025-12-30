// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 instance reconciliation logic.
//!
//! This module handles the lifecycle of BIND9 DNS server deployments in Kubernetes.
//! It creates and manages Deployments, `ConfigMaps`, and Services for each `Bind9Instance`.

use crate::bind9::Bind9Manager;
use crate::bind9_resources::{
    build_configmap, build_deployment, build_service, build_service_account,
};
use crate::constants::{API_GROUP_VERSION, DEFAULT_BIND9_VERSION, KIND_BIND9_INSTANCE};
use crate::crd::{Bind9Cluster, Bind9Instance, Bind9InstanceStatus, Condition};
use crate::labels::{BINDY_MANAGED_BY_LABEL, FINALIZER_BIND9_INSTANCE};
use crate::reconcilers::finalizers::{ensure_finalizer, handle_deletion, FinalizerCleanup};
use crate::reconcilers::resources::{create_or_apply, create_or_replace};
use crate::status_reasons::{
    pod_condition_type, CONDITION_TYPE_READY, REASON_ALL_READY, REASON_NOT_READY,
    REASON_PARTIALLY_READY, REASON_READY,
};
use anyhow::Result;
use chrono::Utc;
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{ConfigMap, Pod, Secret, Service, ServiceAccount},
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::{
    api::{ListParams, Patch, PatchParams, PostParams},
    client::Client,
    Api, ResourceExt,
};
use serde_json::json;
use tracing::{debug, error, info, warn};

/// Implement cleanup trait for `Bind9Instance` finalizer management
#[async_trait::async_trait]
impl FinalizerCleanup for Bind9Instance {
    async fn cleanup(&self, client: &Client) -> Result<()> {
        let namespace = self.namespace().unwrap_or_default();
        let name = self.name_any();

        // Check if this instance is managed by a Bind9Cluster
        let is_managed: bool = self
            .metadata
            .labels
            .as_ref()
            .and_then(|labels| labels.get(BINDY_MANAGED_BY_LABEL))
            .is_some();

        if is_managed {
            info!(
                "Bind9Instance {}/{} is managed by a Bind9Cluster, skipping resource cleanup (cluster will handle it)",
                namespace, name
            );
            Ok(())
        } else {
            info!(
                "Running cleanup for standalone Bind9Instance {}/{}",
                namespace, name
            );
            delete_resources(client, &namespace, &name).await
        }
    }
}

/// Reconciles a `Bind9Instance` resource.
///
/// Creates or updates all Kubernetes resources needed to run a BIND9 DNS server:
/// - `ConfigMap` with BIND9 configuration files
/// - Deployment with BIND9 container pods
/// - Service for DNS traffic (TCP/UDP port 53)
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance` - The `Bind9Instance` resource to reconcile
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
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or resource creation/update fails.
///
/// Reconcile a `Bind9Instance` custom resource
///
/// Creates or updates all Kubernetes resources needed to run a BIND9 DNS server:
/// - `ConfigMap` with BIND9 configuration files
/// - Deployment with BIND9 container pods
/// - Service for DNS traffic (TCP/UDP port 53)
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `instance` - The `Bind9Instance` resource to reconcile
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail or resource creation/update fails.
pub async fn reconcile_bind9instance(client: Client, instance: Bind9Instance) -> Result<()> {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    info!("Reconciling Bind9Instance: {}/{}", namespace, name);
    debug!(
        namespace = %namespace,
        name = %name,
        generation = ?instance.metadata.generation,
        "Starting Bind9Instance reconciliation"
    );

    // Check if the instance is being deleted
    if instance.metadata.deletion_timestamp.is_some() {
        return handle_deletion(&client, &instance, FINALIZER_BIND9_INSTANCE).await;
    }

    // Add finalizer if not present
    ensure_finalizer(&client, &instance, FINALIZER_BIND9_INSTANCE).await?;

    let spec = &instance.spec;
    let replicas = spec.replicas.unwrap_or(1);
    let version = spec.version.as_deref().unwrap_or(DEFAULT_BIND9_VERSION);

    debug!(
        cluster_ref = %spec.cluster_ref,
        replicas,
        version = %version,
        role = ?spec.role,
        "Instance configuration"
    );

    info!(
        "Bind9Instance {} configured with {} replicas, version {}",
        name, replicas, version
    );

    // Check if spec has changed using the standard generation check
    let current_generation = instance.metadata.generation;
    let observed_generation = instance.status.as_ref().and_then(|s| s.observed_generation);

    // Check if resources actually exist (drift detection)
    let deployment_exists = {
        let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
        deployment_api.get(&name).await.is_ok()
    };

    // Only reconcile resources if:
    // 1. Spec changed (generation mismatch), OR
    // 2. We haven't processed this resource yet (no observed_generation), OR
    // 3. Resources are missing (drift detected)
    let should_reconcile =
        crate::reconcilers::should_reconcile(current_generation, observed_generation);

    if !should_reconcile && deployment_exists {
        debug!(
            "Spec unchanged (generation={:?}) and resources exist, skipping resource reconciliation",
            current_generation
        );
        // Update status from current deployment state (only patches if status changed)
        update_status_from_deployment(&client, &namespace, &name, &instance, replicas).await?;
        return Ok(());
    }

    if !should_reconcile && !deployment_exists {
        info!(
            "Drift detected for Bind9Instance {}/{}: Deployment missing, will recreate",
            namespace, name
        );
    }

    debug!(
        "Reconciliation needed: current_generation={:?}, observed_generation={:?}",
        current_generation, observed_generation
    );

    // Create or update resources
    match create_or_update_resources(&client, &namespace, &name, &instance).await {
        Ok(()) => {
            info!(
                "Successfully created/updated resources for {}/{}",
                namespace, name
            );

            // Update status based on actual deployment state
            update_status_from_deployment(&client, &namespace, &name, &instance, replicas).await?;
        }
        Err(e) => {
            error!(
                "Failed to create/update resources for {}/{}: {}",
                namespace, name, e
            );

            // Update status to show error
            let error_condition = Condition {
                r#type: CONDITION_TYPE_READY.to_string(),
                status: "False".to_string(),
                reason: Some(REASON_NOT_READY.to_string()),
                message: Some(format!("Failed to create resources: {e}")),
                last_transition_time: Some(Utc::now().to_rfc3339()),
            };
            update_status(&client, &instance, vec![error_condition], replicas, 0).await?;

            return Err(e);
        }
    }

    Ok(())
}

/// Create or update all Kubernetes resources for a `Bind9Instance`
async fn create_or_update_resources(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
) -> Result<()> {
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
    Ok(())
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
pub async fn delete_bind9instance(client: Client, instance: Bind9Instance) -> Result<()> {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    info!("Deleting Bind9Instance: {}/{}", namespace, name);

    // Delete resources in reverse order (Service, Deployment, ConfigMap)
    delete_resources(&client, &namespace, &name).await?;

    info!("Successfully deleted resources for {}/{}", namespace, name);

    Ok(())
}

/// Delete all Kubernetes resources for a `Bind9Instance`
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

/// Update status based on actual Deployment and Pod readiness
#[allow(clippy::too_many_lines)]
async fn update_status_from_deployment(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    expected_replicas: i32,
) -> Result<()> {
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);

    match deploy_api.get(name).await {
        Ok(deployment) => {
            let actual_replicas = deployment
                .spec
                .as_ref()
                .and_then(|spec| spec.replicas)
                .unwrap_or(0);

            let ready_replicas = deployment
                .status
                .as_ref()
                .and_then(|status| status.ready_replicas)
                .unwrap_or(0);

            // List pods for this deployment using label selector
            // Use the standard Kubernetes label for instance matching
            let label_selector = format!("{}={}", crate::labels::K8S_INSTANCE, name);
            let list_params = ListParams::default().labels(&label_selector);
            let pods = pod_api.list(&list_params).await?;

            // Create pod-level conditions
            let mut pod_conditions = Vec::new();
            let mut ready_pod_count = 0;

            for (index, pod) in pods.items.iter().enumerate() {
                let pod_name = pod.metadata.name.as_deref().unwrap_or("unknown");
                // Using map_or for explicit false default on None - more readable than is_some_and
                #[allow(clippy::unnecessary_map_or)]
                let is_pod_ready = pod
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.as_ref())
                    .map_or(false, |conditions| {
                        conditions
                            .iter()
                            .any(|c| c.type_ == "Ready" && c.status == "True")
                    });

                if is_pod_ready {
                    ready_pod_count += 1;
                }

                let (status, reason, message) = if is_pod_ready {
                    ("True", REASON_READY, format!("Pod {pod_name} is ready"))
                } else {
                    (
                        "False",
                        REASON_NOT_READY,
                        format!("Pod {pod_name} is not ready"),
                    )
                };

                pod_conditions.push(Condition {
                    r#type: pod_condition_type(index),
                    status: status.to_string(),
                    reason: Some(reason.to_string()),
                    message: Some(message),
                    last_transition_time: Some(Utc::now().to_rfc3339()),
                });
            }

            // Create encompassing Ready condition
            let (encompassing_status, encompassing_reason, encompassing_message) =
                if ready_pod_count == 0 && actual_replicas > 0 {
                    (
                        "False",
                        REASON_NOT_READY,
                        "Waiting for pods to become ready".to_string(),
                    )
                } else if ready_pod_count == actual_replicas && actual_replicas > 0 {
                    (
                        "True",
                        REASON_ALL_READY,
                        format!("All {ready_pod_count} pods are ready"),
                    )
                } else if ready_pod_count > 0 {
                    (
                        "False",
                        REASON_PARTIALLY_READY,
                        format!("{ready_pod_count}/{actual_replicas} pods are ready"),
                    )
                } else {
                    ("False", REASON_NOT_READY, "No pods are ready".to_string())
                };

            let encompassing_condition = Condition {
                r#type: CONDITION_TYPE_READY.to_string(),
                status: encompassing_status.to_string(),
                reason: Some(encompassing_reason.to_string()),
                message: Some(encompassing_message),
                last_transition_time: Some(Utc::now().to_rfc3339()),
            };

            // Combine encompassing condition + pod-level conditions
            let mut all_conditions = vec![encompassing_condition];
            all_conditions.extend(pod_conditions);

            // Update status with all conditions
            update_status(
                client,
                instance,
                all_conditions,
                actual_replicas,
                ready_replicas,
            )
            .await?;
        }
        Err(e) => {
            warn!(
                "Failed to get Deployment status for {}/{}: {}",
                namespace, name, e
            );
            // Set status as unknown if we can't check deployment
            let unknown_condition = Condition {
                r#type: CONDITION_TYPE_READY.to_string(),
                status: "Unknown".to_string(),
                reason: Some(REASON_NOT_READY.to_string()),
                message: Some("Unable to determine deployment status".to_string()),
                last_transition_time: Some(Utc::now().to_rfc3339()),
            };
            update_status(
                client,
                instance,
                vec![unknown_condition],
                expected_replicas,
                0,
            )
            .await?;
        }
    }

    Ok(())
}

/// Update the status of a `Bind9Instance` with multiple conditions
async fn update_status(
    client: &Client,
    instance: &Bind9Instance,
    conditions: Vec<Condition>,
    replicas: i32,
    ready_replicas: i32,
) -> Result<()> {
    let api: Api<Bind9Instance> =
        Api::namespaced(client.clone(), &instance.namespace().unwrap_or_default());

    // Check if status has actually changed
    let current_status = &instance.status;
    let status_changed = if let Some(current) = current_status {
        // Check if replicas changed
        if current.replicas != Some(replicas) || current.ready_replicas != Some(ready_replicas) {
            true
        } else {
            // Check if any condition changed
            if current.conditions.len() == conditions.len() {
                // Compare each condition
                current
                    .conditions
                    .iter()
                    .zip(conditions.iter())
                    .any(|(current_cond, new_cond)| {
                        current_cond.r#type != new_cond.r#type
                            || current_cond.status != new_cond.status
                            || current_cond.message != new_cond.message
                            || current_cond.reason != new_cond.reason
                    })
            } else {
                true
            }
        }
    } else {
        // No status exists, need to update
        true
    };

    // Only update if status has changed
    if !status_changed {
        return Ok(());
    }

    let new_status = Bind9InstanceStatus {
        conditions,
        observed_generation: instance.metadata.generation,
        replicas: Some(replicas),
        ready_replicas: Some(ready_replicas),
        service_address: None, // Will be populated when service is ready
        selected_zones: instance
            .status
            .as_ref()
            .map(|s| s.selected_zones.clone())
            .unwrap_or_default(), // Preserve existing selected zones
    };

    let patch = json!({ "status": new_status });
    api.patch_status(
        &instance.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(())
}
