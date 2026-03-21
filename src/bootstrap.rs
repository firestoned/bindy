// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Bootstrap logic for `bindy bootstrap`.
//!
//! Applies all prerequisites to a Kubernetes cluster in order:
//! 1. Namespace (`bindy-system` by default, or `--namespace`)
//! 2. CRDs — generated from Rust types, always in sync with the operator
//! 3. ServiceAccount (`bindy`)
//! 4. ClusterRole (`bindy-role`) — operator permissions
//! 5. ClusterRole (`bindy-admin-role`) — admin/destructive permissions
//! 6. ClusterRoleBinding (`bindy-rolebinding`) — binds SA to operator role

use anyhow::{Context as _, Result};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Namespace, ServiceAccount};
use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding, RoleRef, Subject};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{
    api::{Patch, PatchParams},
    Api, Client, CustomResourceExt,
};

use crate::crd::{
    AAAARecord, ARecord, Bind9Cluster, Bind9Instance, CAARecord, CNAMERecord, ClusterBind9Provider,
    DNSZone, MXRecord, NSRecord, SRVRecord, TXTRecord,
};

/// Default namespace for the bindy operator deployment.
pub const DEFAULT_NAMESPACE: &str = "bindy-system";

/// Field manager name used for server-side apply.
const FIELD_MANAGER: &str = "bindy-bootstrap";

/// ServiceAccount name created for the operator.
pub const SERVICE_ACCOUNT_NAME: &str = "bindy";

/// ClusterRoleBinding name.
pub const CLUSTER_ROLE_BINDING_NAME: &str = "bindy-rolebinding";

/// Operator ClusterRole name.
pub const OPERATOR_ROLE_NAME: &str = "bindy-role";

/// Operator Deployment name.
pub const OPERATOR_DEPLOYMENT_NAME: &str = "bindy";

/// Container image registry and repository (without tag).
pub const OPERATOR_IMAGE_BASE: &str = "ghcr.io/firestoned/bindy";

/// Default image tag for the operator Deployment.
///
/// In debug builds (`cargo build`) this is `"latest"` so local development always
/// pulls the most recent image without needing a version bump.
/// In release builds (`cargo build --release`) this is `"v{CARGO_PKG_VERSION}"`
/// (e.g. `"v0.5.0"`) so `bindy bootstrap` installs exactly the same version
/// that was released alongside the binary.
pub const DEFAULT_IMAGE_TAG: &str = if cfg!(debug_assertions) {
    "latest"
} else {
    concat!("v", env!("CARGO_PKG_VERSION"))
};

/// Embedded RBAC YAML files — compiled into the binary so bootstrap is self-contained.
pub const BINDY_ROLE_YAML: &str = include_str!("../deploy/operator/rbac/role.yaml");
pub const BINDY_ADMIN_ROLE_YAML: &str = include_str!("../deploy/operator/rbac/role-admin.yaml");

/// Run the bootstrap process.
///
/// When `dry_run` is `true`, prints the resources that would be applied to stdout (as YAML)
/// without connecting to a cluster. When `false`, applies each resource via server-side apply
/// (idempotent — safe to run multiple times).
///
/// # Arguments
/// * `namespace` - Namespace to install bindy into (default: `bindy-system`)
/// * `dry_run` - If true, print what would be applied without applying
/// * `image_tag` - Image tag for the operator Deployment (e.g. `"v0.5.0"` or `"latest"`)
///
/// # Errors
/// Returns error if Kubernetes API calls fail (in non-dry-run mode).
pub async fn run_bootstrap(namespace: &str, dry_run: bool, image_tag: &str) -> Result<()> {
    if dry_run {
        return run_dry_run(namespace, image_tag);
    }

    let client = Client::try_default()
        .await
        .context("Failed to connect to Kubernetes cluster — is KUBECONFIG set?")?;

    apply_namespace(&client, namespace).await?;
    apply_crds(&client).await?;
    apply_service_account(&client, namespace).await?;
    apply_cluster_role(&client, BINDY_ROLE_YAML).await?;
    apply_cluster_role(&client, BINDY_ADMIN_ROLE_YAML).await?;
    apply_cluster_role_binding(&client, namespace).await?;
    apply_deployment(&client, namespace, image_tag).await?;

    println!("\nBootstrap complete! The operator is running in namespace {namespace}.");

    Ok(())
}

// ---------------------------------------------------------------------------
// Dry-run path — no cluster connection needed
// ---------------------------------------------------------------------------

fn run_dry_run(namespace: &str, image_tag: &str) -> Result<()> {
    println!("# Dry-run mode — no resources will be applied\n");

    print_resource("Namespace", &build_namespace(namespace))?;

    for crd in build_all_crds()? {
        let name = crd.metadata.name.as_deref().unwrap_or("unknown");
        print_resource(&format!("CustomResourceDefinition/{name}"), &crd)?;
    }

    print_resource("ServiceAccount", &build_service_account(namespace))?;
    print_resource(
        "ClusterRole (operator)",
        &parse_cluster_role(BINDY_ROLE_YAML)?,
    )?;
    print_resource(
        "ClusterRole (admin)",
        &parse_cluster_role(BINDY_ADMIN_ROLE_YAML)?,
    )?;
    print_resource("ClusterRoleBinding", &build_cluster_role_binding(namespace))?;
    print_resource("Deployment", &build_deployment(namespace, image_tag)?)?;

    println!("# Dry-run complete — no resources were applied");
    Ok(())
}

fn print_resource<T: serde::Serialize>(label: &str, resource: &T) -> Result<()> {
    let yaml =
        serde_yaml::to_string(resource).with_context(|| format!("Failed to serialize {label}"))?;
    println!("---\n# {label}");
    print!("{yaml}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Apply helpers
// ---------------------------------------------------------------------------

async fn apply_namespace(client: &Client, name: &str) -> Result<()> {
    let api: Api<Namespace> = Api::all(client.clone());
    let ns = build_namespace(name);
    api.patch(
        name,
        &PatchParams::apply(FIELD_MANAGER).force(),
        &Patch::Apply(&ns),
    )
    .await
    .with_context(|| format!("Failed to apply Namespace/{name}"))?;
    println!("✓ Namespace: {name}");
    Ok(())
}

async fn apply_crds(client: &Client) -> Result<()> {
    let api: Api<CustomResourceDefinition> = Api::all(client.clone());
    for crd in build_all_crds()? {
        let name = crd.metadata.name.clone().unwrap_or_default();
        api.patch(
            &name,
            &PatchParams::apply(FIELD_MANAGER).force(),
            &Patch::Apply(&crd),
        )
        .await
        .with_context(|| format!("Failed to apply CRD/{name}"))?;
        println!("✓ CRD: {name}");
    }
    Ok(())
}

async fn apply_service_account(client: &Client, namespace: &str) -> Result<()> {
    let api: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    let sa = build_service_account(namespace);
    api.patch(
        SERVICE_ACCOUNT_NAME,
        &PatchParams::apply(FIELD_MANAGER).force(),
        &Patch::Apply(&sa),
    )
    .await
    .context("Failed to apply ServiceAccount/bindy")?;
    println!("✓ ServiceAccount: {SERVICE_ACCOUNT_NAME} (namespace: {namespace})");
    Ok(())
}

async fn apply_cluster_role(client: &Client, yaml: &str) -> Result<()> {
    let role = parse_cluster_role(yaml)?;
    let name = role.metadata.name.clone().unwrap_or_default();
    let api: Api<ClusterRole> = Api::all(client.clone());
    api.patch(
        &name,
        &PatchParams::apply(FIELD_MANAGER).force(),
        &Patch::Apply(&role),
    )
    .await
    .with_context(|| format!("Failed to apply ClusterRole/{name}"))?;
    println!("✓ ClusterRole: {name}");
    Ok(())
}

async fn apply_cluster_role_binding(client: &Client, namespace: &str) -> Result<()> {
    let api: Api<ClusterRoleBinding> = Api::all(client.clone());
    let crb = build_cluster_role_binding(namespace);
    api.patch(
        CLUSTER_ROLE_BINDING_NAME,
        &PatchParams::apply(FIELD_MANAGER).force(),
        &Patch::Apply(&crb),
    )
    .await
    .context("Failed to apply ClusterRoleBinding/bindy-rolebinding")?;
    println!("✓ ClusterRoleBinding: {CLUSTER_ROLE_BINDING_NAME}");
    Ok(())
}

async fn apply_deployment(client: &Client, namespace: &str, image_tag: &str) -> Result<()> {
    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment = build_deployment(namespace, image_tag)?;
    api.patch(
        OPERATOR_DEPLOYMENT_NAME,
        &PatchParams::apply(FIELD_MANAGER).force(),
        &Patch::Apply(&deployment),
    )
    .await
    .context("Failed to apply operator Deployment")?;
    let image = format!("{OPERATOR_IMAGE_BASE}:{image_tag}");
    println!("✓ Deployment: {OPERATOR_DEPLOYMENT_NAME} (image: {image})");
    Ok(())
}

// ---------------------------------------------------------------------------
// Resource builders (pub so tests can access them)
// ---------------------------------------------------------------------------

/// Build the operator namespace object.
pub fn build_namespace(name: &str) -> Namespace {
    Namespace {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(
                [("kubernetes.io/metadata.name".to_string(), name.to_string())]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Build the bindy ServiceAccount in the given namespace.
pub fn build_service_account(namespace: &str) -> ServiceAccount {
    ServiceAccount {
        metadata: ObjectMeta {
            name: Some(SERVICE_ACCOUNT_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        "rbac".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Build the ClusterRoleBinding that binds the bindy ServiceAccount to `bindy-role`.
///
/// The subject namespace is set to `namespace` so bootstrap works for custom namespaces.
pub fn build_cluster_role_binding(namespace: &str) -> ClusterRoleBinding {
    ClusterRoleBinding {
        metadata: ObjectMeta {
            name: Some(CLUSTER_ROLE_BINDING_NAME.to_string()),
            ..Default::default()
        },
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".to_string(),
            kind: "ClusterRole".to_string(),
            name: OPERATOR_ROLE_NAME.to_string(),
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".to_string(),
            name: SERVICE_ACCOUNT_NAME.to_string(),
            namespace: Some(namespace.to_string()),
            api_group: Some(String::new()),
        }]),
    }
}

/// Build the operator Deployment manifest.
///
/// The container image is `ghcr.io/firestoned/bindy:<image_tag>`.
pub fn build_deployment(namespace: &str, image_tag: &str) -> Result<Deployment> {
    let image = format!("{OPERATOR_IMAGE_BASE}:{image_tag}");
    let value = serde_json::json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": OPERATOR_DEPLOYMENT_NAME,
            "namespace": namespace,
            "labels": {"app": "bindy"}
        },
        "spec": {
            "replicas": 1,
            "selector": {"matchLabels": {"app": "bindy"}},
            "template": {
                "metadata": {"labels": {"app": "bindy"}},
                "spec": {
                    "serviceAccountName": SERVICE_ACCOUNT_NAME,
                    "securityContext": {"runAsNonRoot": true, "fsGroup": 65_534_i64},
                    "containers": [{
                        "name": "bindy",
                        "image": image,
                        "imagePullPolicy": "IfNotPresent",
                        "args": ["run"],
                        "env": [
                            {"name": "RUST_LOG", "value": "info"},
                            {"name": "RUST_LOG_FORMAT", "value": "text"},
                            {"name": "BINDY_ENABLE_LEADER_ELECTION", "value": "true"},
                            {"name": "BINDY_LEASE_NAME", "value": "bindy-leader"}
                        ],
                        "securityContext": {
                            "allowPrivilegeEscalation": false,
                            "capabilities": {"drop": ["ALL"]},
                            "readOnlyRootFilesystem": true,
                            "runAsNonRoot": true,
                            "runAsUser": 65_534_i64
                        },
                        "resources": {
                            "limits": {"cpu": "500m", "memory": "512Mi"},
                            "requests": {"cpu": "100m", "memory": "128Mi"}
                        },
                        "volumeMounts": [{"name": "tmp", "mountPath": "/tmp"}]
                    }],
                    "volumes": [{"name": "tmp", "emptyDir": {}}]
                }
            }
        }
    });
    serde_json::from_value(value).context("Failed to build operator Deployment")
}

/// Parse a ClusterRole from embedded YAML.
pub fn parse_cluster_role(yaml: &str) -> Result<ClusterRole> {
    serde_yaml::from_str(yaml).context("Failed to parse ClusterRole YAML")
}

/// Build a single CRD from a Rust type, ensuring `storage: true` and `served: true`.
///
/// Mirrors the logic in `src/bin/crdgen.rs` so bootstrap and crdgen stay in sync.
pub fn build_crd<T: CustomResourceExt>() -> Result<CustomResourceDefinition> {
    let crd = T::crd();
    let mut crd_json = serde_json::to_value(&crd).context("Failed to serialize CRD to JSON")?;

    if let Some(versions) = crd_json["spec"]["versions"].as_array_mut() {
        if let Some(first) = versions.first_mut() {
            first["storage"] = serde_json::Value::Bool(true);
            first["served"] = serde_json::Value::Bool(true);
        }
    }

    serde_json::from_value(crd_json).context("Failed to deserialize CRD from JSON")
}

/// Build all 12 CRDs in the same order as `crdgen`.
pub fn build_all_crds() -> Result<Vec<CustomResourceDefinition>> {
    Ok(vec![
        build_crd::<ARecord>()?,
        build_crd::<AAAARecord>()?,
        build_crd::<CNAMERecord>()?,
        build_crd::<MXRecord>()?,
        build_crd::<NSRecord>()?,
        build_crd::<TXTRecord>()?,
        build_crd::<SRVRecord>()?,
        build_crd::<CAARecord>()?,
        build_crd::<DNSZone>()?,
        build_crd::<Bind9Cluster>()?,
        build_crd::<ClusterBind9Provider>()?,
        build_crd::<Bind9Instance>()?,
    ])
}
