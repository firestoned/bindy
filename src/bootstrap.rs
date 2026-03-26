// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Bootstrap logic for `bindy bootstrap`.
//!
//! ## `bindy bootstrap operator`
//! Applies all operator prerequisites to a Kubernetes cluster in order:
//! 1. Namespace (`bindy-system` by default, or `--namespace`)
//! 2. CRDs — generated from Rust types, always in sync with the operator
//! 3. ServiceAccount (`bindy`)
//! 4. ClusterRole (`bindy-role`) — operator permissions
//! 5. ClusterRole (`bindy-admin-role`) — admin/destructive permissions
//! 6. ClusterRoleBinding (`bindy-rolebinding`) — binds SA to operator role
//! 7. Deployment (`bindy`) — the operator itself
//!
//! ## `bindy bootstrap scout`
//! Applies all scout prerequisites to a Kubernetes cluster in order:
//! 1. Namespace (`bindy-system` by default, or `--namespace`)
//! 2. CRDs — same 12 CRDs as the operator (shared types)
//! 3. ServiceAccount (`bindy-scout`)
//! 4. ClusterRole (`bindy-scout`) — scout cluster-scoped permissions
//! 5. ClusterRoleBinding (`bindy-scout`) — binds scout SA to scout ClusterRole
//! 6. Role (`bindy-scout-writer`) — namespaced ARecord write permissions
//! 7. RoleBinding (`bindy-scout-writer`) — binds scout SA to writer Role
//! 8. Deployment (`bindy-scout`) — the scout controller itself
//!
//! ## `bindy bootstrap mc`
//! Sets up remote access so a scout running on a child (workload) cluster can write
//! ARecords to the queen-ship (bindy) cluster.  Run this command **against the
//! queen-ship cluster** (`KUBECONFIG` must point at it):
//!
//! 1. ServiceAccount (`bindy-scout-remote` by default, or `--service-account`)
//!    — one SA per child cluster so access can be revoked independently
//! 2. Role (`bindy-scout-remote`) — namespaced ARecord CRUD + DNSZone read permissions
//!    on the queen-ship.  A namespaced Role is sufficient because the scout watches
//!    DNSZones via `Api::namespaced` (not `Api::all`) in the same target namespace.
//! 3. RoleBinding (`bindy-scout-remote`) — binds the SA to the namespaced Role
//! 4. SA token Secret — a long-lived token for the SA
//! 5. Kubeconfig Secret (`bindy-scout-remote-remote-kubeconfig`) — a ready-to-use
//!    kubeconfig for the SA, printed to **stdout** as YAML
//!
//! The stdout output is applied to the **child cluster** where scout runs:
//! ```text
//! bindy bootstrap mc | kubectl --context=<child-cluster> apply -f -
//! ```
//! Then set `BINDY_SCOUT_REMOTE_SECRET=bindy-scout-remote-kubeconfig` on the
//! scout Deployment so it picks up the remote kubeconfig at startup.

use anyhow::{Context as _, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Namespace, Secret, ServiceAccount};
use k8s_openapi::api::rbac::v1::{
    ClusterRole, ClusterRoleBinding, PolicyRule, Role, RoleBinding, RoleRef, Subject,
};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::ByteString;
use kube::{
    api::{DeleteParams, Patch, PatchParams},
    config::Kubeconfig,
    Api, Client, CustomResourceExt,
};
use std::collections::BTreeMap;
use std::time::Duration;

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

/// Default image tag for operator and scout Deployments.
///
/// Always matches the binary's own version (e.g. `"v0.5.0"`) so that
/// `bindy bootstrap` installs exactly the image that was shipped with this binary.
pub const DEFAULT_IMAGE_TAG: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// Embedded RBAC YAML files — compiled into the binary so bootstrap is self-contained.
pub const BINDY_ROLE_YAML: &str = include_str!("../deploy/operator/rbac/role.yaml");
pub const BINDY_ADMIN_ROLE_YAML: &str = include_str!("../deploy/operator/rbac/role-admin.yaml");

// ---------------------------------------------------------------------------
// Scout constants
// ---------------------------------------------------------------------------

/// Scout ServiceAccount name.
pub const SCOUT_SERVICE_ACCOUNT_NAME: &str = "bindy-scout";

/// Scout ClusterRole name.
pub const SCOUT_CLUSTER_ROLE_NAME: &str = "bindy-scout";

/// Scout ClusterRoleBinding name.
pub const SCOUT_CLUSTER_ROLE_BINDING_NAME: &str = "bindy-scout";

/// Scout namespaced Role name (ARecord write permissions).
pub const SCOUT_WRITER_ROLE_NAME: &str = "bindy-scout-writer";

/// Scout namespaced RoleBinding name.
pub const SCOUT_WRITER_ROLE_BINDING_NAME: &str = "bindy-scout-writer";

/// Scout Deployment name.
pub const SCOUT_DEPLOYMENT_NAME: &str = "bindy-scout";

/// Default ServiceAccount name created by `bootstrap mc` on the queen-ship cluster.
///
/// Each child cluster gets its own SA so access can be revoked independently.
/// The local in-cluster scout SA is named [`SCOUT_SERVICE_ACCOUNT_NAME`] (`bindy-scout`);
/// the remote SA uses this distinct name to avoid confusion.
pub const MC_DEFAULT_SERVICE_ACCOUNT_NAME: &str = "bindy-scout-remote";

/// Default logical cluster name stamped on ARecord labels by the scout controller.
pub const DEFAULT_SCOUT_CLUSTER_NAME: &str = "default";

/// Field manager name used for scout server-side apply.
const SCOUT_FIELD_MANAGER: &str = "bindy-bootstrap-scout";

// ---------------------------------------------------------------------------
// Scout deployment configuration
// ---------------------------------------------------------------------------

/// Configuration options for the Scout Deployment and bootstrap process.
///
/// Groups all deployment-specific parameters to avoid functions exceeding the
/// recommended argument count.
pub struct ScoutDeploymentOptions<'a> {
    /// Image tag for the scout container (e.g. `"v0.5.0"` or `"latest"`).
    pub image_tag: &'a str,
    /// Optional registry override (e.g. `"my.registry.io/org"`).
    pub registry: Option<&'a str>,
    /// Logical cluster name stamped on ARecord labels (`--cluster-name`).
    pub cluster_name: &'a str,
    /// Default IP addresses for Ingresses with no per-Ingress annotation or LB status.
    pub default_ips: &'a [String],
    /// Default DNS zone for Ingresses with no zone annotation.
    pub default_zone: Option<&'a str>,
    /// Name of the Secret containing the remote cluster kubeconfig.
    /// When set, `BINDY_SCOUT_REMOTE_SECRET` is injected into the Deployment env.
    pub remote_secret: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Multi-cluster (MC) constants
// ---------------------------------------------------------------------------

/// Field manager for multi-cluster bootstrap.
const MC_FIELD_MANAGER: &str = "bindy-bootstrap-mc";

/// Secret type for the kubeconfig Secret placed on a child (workload) cluster.
///
/// Secrets of this type hold a kubeconfig that the scout controller uses to connect
/// back to the queen-ship (bindy operator) cluster to create ARecords and read DNSZones.
pub const REMOTE_KUBECONFIG_SECRET_TYPE: &str = "bindy.firestoned.io/remote-kubeconfig";

/// Suffix appended to the service account name when naming the SA token Secret.
///
/// For example, SA `scout` produces token Secret `scout-token`.
pub const SA_TOKEN_SECRET_SUFFIX: &str = "-token";

/// Suffix appended to the service account name when naming the remote kubeconfig Secret.
///
/// For example, SA `bindy-scout` produces kubeconfig Secret `bindy-scout-remote-kubeconfig`.
pub const REMOTE_KUBECONFIG_SECRET_SUFFIX: &str = "-remote-kubeconfig";

/// `app.kubernetes.io/component` label value for all resources created by `bootstrap mc`.
const MC_COMPONENT_LABEL: &str = "scout-remote";

/// HTTP 404 Not Found — used to detect missing resources during revoke so they can be
/// skipped rather than treated as errors.
const HTTP_NOT_FOUND: u16 = 404;

/// Maximum polling attempts while waiting for the SA token Secret to be populated.
const SA_TOKEN_WAIT_MAX_ATTEMPTS: usize = 20;

/// Milliseconds between SA token Secret polling attempts.
const SA_TOKEN_WAIT_INTERVAL_MS: u64 = 500;

// ---------------------------------------------------------------------------
// Image resolution
// ---------------------------------------------------------------------------

/// Resolve the full container image reference for the bindy image.
///
/// The image name is always `bindy`; only the registry/org prefix and tag vary:
///
/// | `registry`              | `tag`    | result                              |
/// |-------------------------|----------|-------------------------------------|
/// | `None`                  | `latest` | `ghcr.io/firestoned/bindy:latest`   |
/// | `Some("my.reg.io/org")` | `v0.5.0` | `my.reg.io/org/bindy:v0.5.0`        |
///
/// Trailing slashes on `registry` are stripped before composing the reference.
pub fn resolve_image(registry: Option<&str>, tag: &str) -> String {
    match registry {
        None => format!("{OPERATOR_IMAGE_BASE}:{tag}"),
        Some(reg) => format!("{}/bindy:{}", reg.trim_end_matches('/'), tag),
    }
}

/// Run the operator bootstrap process (`bindy bootstrap operator`).
///
/// When `dry_run` is `true`, prints the resources that would be applied to stdout (as YAML)
/// without connecting to a cluster. When `false`, applies each resource via server-side apply
/// (idempotent — safe to run multiple times).
///
/// # Arguments
/// * `namespace` - Namespace to install bindy into (default: `bindy-system`)
/// * `dry_run` - If true, print what would be applied without applying
/// * `image_tag` - Image tag for the operator Deployment (e.g. `"v0.5.0"` or `"latest"`)
/// * `registry` - Optional registry override for air-gapped environments
///
/// # Errors
/// Returns error if Kubernetes API calls fail (in non-dry-run mode).
pub async fn run_bootstrap_operator(
    namespace: &str,
    dry_run: bool,
    image_tag: &str,
    registry: Option<&str>,
) -> Result<()> {
    if dry_run {
        return run_operator_dry_run(namespace, image_tag, registry);
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
    apply_deployment(&client, namespace, image_tag, registry).await?;

    println!("\nBootstrap complete! The operator is running in namespace {namespace}.");

    Ok(())
}

/// Run the scout bootstrap process (`bindy bootstrap scout`).
///
/// Applies the namespace, all CRDs (shared with the operator), and all scout-specific
/// RBAC resources and the scout Deployment.
///
/// When `dry_run` is `true`, prints the resources that would be applied to stdout (as YAML)
/// without connecting to a cluster. When `false`, applies each resource via server-side apply
/// (idempotent — safe to run multiple times).
///
/// # Arguments
/// * `namespace` - Namespace to install scout into (default: `bindy-system`)
/// * `dry_run` - If true, print what would be applied without applying
/// * `opts` - Deployment configuration (image, registry, cluster name, IPs, zone, remote secret)
///
/// # Errors
/// Returns error if Kubernetes API calls fail (in non-dry-run mode).
pub async fn run_bootstrap_scout(
    namespace: &str,
    dry_run: bool,
    opts: &ScoutDeploymentOptions<'_>,
) -> Result<()> {
    if dry_run {
        return run_scout_dry_run(namespace, opts);
    }

    let client = Client::try_default()
        .await
        .context("Failed to connect to Kubernetes cluster — is KUBECONFIG set?")?;

    apply_namespace(&client, namespace).await?;
    apply_crds(&client).await?;
    apply_scout_service_account(&client, namespace).await?;
    apply_scout_cluster_role(&client).await?;
    apply_scout_cluster_role_binding(&client, namespace).await?;
    apply_scout_writer_role(&client, namespace).await?;
    apply_scout_writer_role_binding(&client, namespace).await?;
    apply_scout_deployment(&client, namespace, opts).await?;

    println!("\nBootstrap complete! Scout is running in namespace {namespace}.");

    Ok(())
}

// ---------------------------------------------------------------------------
// Dry-run paths — no cluster connection needed
// ---------------------------------------------------------------------------

fn run_operator_dry_run(namespace: &str, image_tag: &str, registry: Option<&str>) -> Result<()> {
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
    print_resource(
        "Deployment",
        &build_deployment(namespace, image_tag, registry)?,
    )?;

    println!("# Dry-run complete — no resources were applied");
    Ok(())
}

fn run_scout_dry_run(namespace: &str, opts: &ScoutDeploymentOptions<'_>) -> Result<()> {
    println!("# Dry-run mode (scout) — no resources will be applied\n");

    print_resource("Namespace", &build_namespace(namespace))?;

    for crd in build_all_crds()? {
        let name = crd.metadata.name.as_deref().unwrap_or("unknown");
        print_resource(&format!("CustomResourceDefinition/{name}"), &crd)?;
    }

    print_resource(
        "ServiceAccount (scout)",
        &build_scout_service_account(namespace),
    )?;
    print_resource("ClusterRole (scout)", &build_scout_cluster_role())?;
    print_resource(
        "ClusterRoleBinding (scout)",
        &build_scout_cluster_role_binding(namespace),
    )?;
    print_resource("Role (scout-writer)", &build_scout_writer_role(namespace))?;
    print_resource(
        "RoleBinding (scout-writer)",
        &build_scout_writer_role_binding(namespace),
    )?;
    print_resource(
        "Deployment (scout)",
        &build_scout_deployment(namespace, opts)?,
    )?;

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

async fn apply_deployment(
    client: &Client,
    namespace: &str,
    image_tag: &str,
    registry: Option<&str>,
) -> Result<()> {
    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment = build_deployment(namespace, image_tag, registry)?;
    api.patch(
        OPERATOR_DEPLOYMENT_NAME,
        &PatchParams::apply(FIELD_MANAGER).force(),
        &Patch::Apply(&deployment),
    )
    .await
    .context("Failed to apply operator Deployment")?;
    let image = resolve_image(registry, image_tag);
    println!("✓ Deployment: {OPERATOR_DEPLOYMENT_NAME} (image: {image})");
    Ok(())
}

// ---------------------------------------------------------------------------
// Scout apply helpers
// ---------------------------------------------------------------------------

async fn apply_scout_service_account(client: &Client, namespace: &str) -> Result<()> {
    let api: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    let sa = build_scout_service_account(namespace);
    api.patch(
        SCOUT_SERVICE_ACCOUNT_NAME,
        &PatchParams::apply(SCOUT_FIELD_MANAGER).force(),
        &Patch::Apply(&sa),
    )
    .await
    .context("Failed to apply ServiceAccount/bindy-scout")?;
    println!("✓ ServiceAccount: {SCOUT_SERVICE_ACCOUNT_NAME} (namespace: {namespace})");
    Ok(())
}

async fn apply_scout_cluster_role(client: &Client) -> Result<()> {
    let api: Api<ClusterRole> = Api::all(client.clone());
    let role = build_scout_cluster_role();
    api.patch(
        SCOUT_CLUSTER_ROLE_NAME,
        &PatchParams::apply(SCOUT_FIELD_MANAGER).force(),
        &Patch::Apply(&role),
    )
    .await
    .with_context(|| format!("Failed to apply ClusterRole/{SCOUT_CLUSTER_ROLE_NAME}"))?;
    println!("✓ ClusterRole: {SCOUT_CLUSTER_ROLE_NAME}");
    Ok(())
}

async fn apply_scout_cluster_role_binding(client: &Client, namespace: &str) -> Result<()> {
    let api: Api<ClusterRoleBinding> = Api::all(client.clone());
    let crb = build_scout_cluster_role_binding(namespace);
    api.patch(
        SCOUT_CLUSTER_ROLE_BINDING_NAME,
        &PatchParams::apply(SCOUT_FIELD_MANAGER).force(),
        &Patch::Apply(&crb),
    )
    .await
    .context("Failed to apply ClusterRoleBinding/bindy-scout")?;
    println!("✓ ClusterRoleBinding: {SCOUT_CLUSTER_ROLE_BINDING_NAME}");
    Ok(())
}

async fn apply_scout_writer_role(client: &Client, namespace: &str) -> Result<()> {
    let api: Api<Role> = Api::namespaced(client.clone(), namespace);
    let role = build_scout_writer_role(namespace);
    api.patch(
        SCOUT_WRITER_ROLE_NAME,
        &PatchParams::apply(SCOUT_FIELD_MANAGER).force(),
        &Patch::Apply(&role),
    )
    .await
    .with_context(|| format!("Failed to apply Role/{SCOUT_WRITER_ROLE_NAME}"))?;
    println!("✓ Role: {SCOUT_WRITER_ROLE_NAME} (namespace: {namespace})");
    Ok(())
}

async fn apply_scout_writer_role_binding(client: &Client, namespace: &str) -> Result<()> {
    let api: Api<RoleBinding> = Api::namespaced(client.clone(), namespace);
    let rb = build_scout_writer_role_binding(namespace);
    api.patch(
        SCOUT_WRITER_ROLE_BINDING_NAME,
        &PatchParams::apply(SCOUT_FIELD_MANAGER).force(),
        &Patch::Apply(&rb),
    )
    .await
    .with_context(|| format!("Failed to apply RoleBinding/{SCOUT_WRITER_ROLE_BINDING_NAME}"))?;
    println!("✓ RoleBinding: {SCOUT_WRITER_ROLE_BINDING_NAME} (namespace: {namespace})");
    Ok(())
}

async fn apply_scout_deployment(
    client: &Client,
    namespace: &str,
    opts: &ScoutDeploymentOptions<'_>,
) -> Result<()> {
    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment = build_scout_deployment(namespace, opts)?;
    api.patch(
        SCOUT_DEPLOYMENT_NAME,
        &PatchParams::apply(SCOUT_FIELD_MANAGER).force(),
        &Patch::Apply(&deployment),
    )
    .await
    .context("Failed to apply scout Deployment")?;
    let image = resolve_image(opts.registry, opts.image_tag);
    println!("✓ Deployment: {SCOUT_DEPLOYMENT_NAME} (image: {image})");
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
/// The container image defaults to `ghcr.io/firestoned/bindy:<image_tag>`.
/// Pass `registry` to override the registry/org prefix for air-gapped environments.
pub fn build_deployment(
    namespace: &str,
    image_tag: &str,
    registry: Option<&str>,
) -> Result<Deployment> {
    let image = resolve_image(registry, image_tag);
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

// ---------------------------------------------------------------------------
// Scout resource builders (pub so tests can access them)
// ---------------------------------------------------------------------------

/// Build the scout ServiceAccount in the given namespace.
pub fn build_scout_service_account(namespace: &str) -> ServiceAccount {
    ServiceAccount {
        metadata: ObjectMeta {
            name: Some(SCOUT_SERVICE_ACCOUNT_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        "scout".to_string(),
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

/// Build the scout ClusterRole with cluster-scoped permissions.
///
/// Grants watch/patch/update on Ingresses (kube-rs finalizer patches the main resource),
/// read on DNSZones, and read on Secrets (for remote kubeconfig).
pub fn build_scout_cluster_role() -> ClusterRole {
    ClusterRole {
        metadata: ObjectMeta {
            name: Some(SCOUT_CLUSTER_ROLE_NAME.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        "scout".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        rules: Some(vec![
            // Watch and mutate Ingresses across all namespaces.
            // kube-rs finalizer::finalizer() patches the main resource metadata to
            // add/remove finalizers, so patch+update on ingresses (not just the
            // ingresses/finalizers subresource) is required.
            PolicyRule {
                api_groups: Some(vec!["networking.k8s.io".to_string()]),
                resources: Some(vec!["ingresses".to_string()]),
                verbs: vec![
                    "get".to_string(),
                    "list".to_string(),
                    "watch".to_string(),
                    "patch".to_string(),
                    "update".to_string(),
                ],
                ..Default::default()
            },
            // Also grant the finalizers subresource for forward-compatibility.
            PolicyRule {
                api_groups: Some(vec!["networking.k8s.io".to_string()]),
                resources: Some(vec!["ingresses/finalizers".to_string()]),
                verbs: vec!["update".to_string()],
                ..Default::default()
            },
            // Read DNSZones for zone validation
            PolicyRule {
                api_groups: Some(vec!["bindy.firestoned.io".to_string()]),
                resources: Some(vec!["dnszones".to_string()]),
                verbs: vec!["get".to_string(), "list".to_string(), "watch".to_string()],
                ..Default::default()
            },
            // Read kubeconfig Secret for remote cluster connection
            PolicyRule {
                api_groups: Some(vec![String::new()]),
                resources: Some(vec!["secrets".to_string()]),
                verbs: vec!["get".to_string()],
                ..Default::default()
            },
        ]),
        ..Default::default()
    }
}

/// Build the ClusterRoleBinding that binds the scout ServiceAccount to the scout ClusterRole.
///
/// The subject namespace is set to `namespace` so bootstrap works for custom namespaces.
pub fn build_scout_cluster_role_binding(namespace: &str) -> ClusterRoleBinding {
    ClusterRoleBinding {
        metadata: ObjectMeta {
            name: Some(SCOUT_CLUSTER_ROLE_BINDING_NAME.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        "scout".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".to_string(),
            kind: "ClusterRole".to_string(),
            name: SCOUT_CLUSTER_ROLE_NAME.to_string(),
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".to_string(),
            name: SCOUT_SERVICE_ACCOUNT_NAME.to_string(),
            namespace: Some(namespace.to_string()),
            api_group: Some(String::new()),
        }]),
    }
}

/// Build the scout writer Role (namespaced ARecord write permissions).
pub fn build_scout_writer_role(namespace: &str) -> Role {
    Role {
        metadata: ObjectMeta {
            name: Some(SCOUT_WRITER_ROLE_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        "scout".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        rules: Some(vec![PolicyRule {
            api_groups: Some(vec!["bindy.firestoned.io".to_string()]),
            resources: Some(vec!["arecords".to_string()]),
            verbs: vec![
                "get".to_string(),
                "list".to_string(),
                "watch".to_string(),
                "create".to_string(),
                "update".to_string(),
                "patch".to_string(),
                "delete".to_string(),
            ],
            ..Default::default()
        }]),
    }
}

/// Build the scout writer RoleBinding (binds scout SA to writer Role).
pub fn build_scout_writer_role_binding(namespace: &str) -> RoleBinding {
    RoleBinding {
        metadata: ObjectMeta {
            name: Some(SCOUT_WRITER_ROLE_BINDING_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        "scout".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".to_string(),
            kind: "Role".to_string(),
            name: SCOUT_WRITER_ROLE_NAME.to_string(),
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".to_string(),
            name: SCOUT_SERVICE_ACCOUNT_NAME.to_string(),
            namespace: Some(namespace.to_string()),
            api_group: Some(String::new()),
        }]),
    }
}

// ---------------------------------------------------------------------------
// Multi-cluster kubeconfig serialization helpers (private)
// ---------------------------------------------------------------------------

/// Top-level kubeconfig structure for serialization.
#[derive(serde::Serialize)]
struct BootstrapKubeconfig {
    #[serde(rename = "apiVersion")]
    api_version: String,
    kind: String,
    clusters: Vec<BootstrapNamedCluster>,
    contexts: Vec<BootstrapNamedContext>,
    #[serde(rename = "current-context")]
    current_context: String,
    users: Vec<BootstrapNamedUser>,
}

#[derive(serde::Serialize)]
struct BootstrapNamedCluster {
    name: String,
    cluster: BootstrapCluster,
}

#[derive(serde::Serialize)]
struct BootstrapCluster {
    server: String,
    #[serde(
        rename = "certificate-authority-data",
        skip_serializing_if = "Option::is_none"
    )]
    certificate_authority_data: Option<String>,
    #[serde(
        rename = "insecure-skip-tls-verify",
        skip_serializing_if = "Option::is_none"
    )]
    insecure_skip_tls_verify: Option<bool>,
}

#[derive(serde::Serialize)]
struct BootstrapNamedContext {
    name: String,
    context: BootstrapContext,
}

#[derive(serde::Serialize)]
struct BootstrapContext {
    cluster: String,
    user: String,
}

#[derive(serde::Serialize)]
struct BootstrapNamedUser {
    name: String,
    user: BootstrapUser,
}

#[derive(serde::Serialize)]
struct BootstrapUser {
    token: String,
}

// ---------------------------------------------------------------------------
// Multi-cluster public API
// ---------------------------------------------------------------------------

/// Run the multi-cluster bootstrap process (`bindy bootstrap multi-cluster`).
///
/// Run this command against the **queen-ship** (bindy operator) cluster. It creates a
/// `ServiceAccount`, namespaced `Role` (ARecord CRUD + DNSZone read), and `RoleBinding`
/// on the queen-ship, generates a kubeconfig for that service account, and writes a
/// `bindy.firestoned.io/remote-kubeconfig` Secret manifest to **stdout**.
///
/// Apply the stdout output to the child (workload) cluster where scout runs:
///
/// ```text
/// bindy bootstrap mc | kubectl --context=<child-cluster> apply -f -
/// ```
///
/// Then configure the scout Deployment with:
/// ```text
/// BINDY_SCOUT_REMOTE_SECRET=<service-account>-remote-kubeconfig
/// ```
///
/// # Arguments
/// * `namespace` - Namespace on the queen-ship where the SA and Role are created
/// * `service_account` - Name of the ServiceAccount to create
/// * `server_override` - Optional API server URL to use in the kubeconfig instead of the
///   address from KUBECONFIG. Required when the KUBECONFIG address is not reachable from
///   inside the child cluster (e.g. `https://172.18.0.3:6443` for kind-to-kind).
///
/// # Errors
/// Returns error if KUBECONFIG is unreadable or if Kubernetes API calls fail.
pub async fn run_bootstrap_multi_cluster(
    namespace: &str,
    service_account: &str,
    server_override: Option<&str>,
) -> Result<()> {
    let client = Client::try_default()
        .await
        .context("Failed to connect to Kubernetes cluster — is KUBECONFIG set?")?;

    let (kubeconfig_server, ca_data_b64, cluster_name) = read_cluster_info()?;
    let server = server_override.unwrap_or(&kubeconfig_server);
    if server_override.is_some() {
        eprintln!("ℹ Using server override: {server} (KUBECONFIG had: {kubeconfig_server})");
    }

    apply_mc_service_account(&client, namespace, service_account).await?;
    apply_mc_writer_role(&client, namespace, service_account).await?;
    apply_mc_writer_role_binding(&client, namespace, service_account).await?;

    let token_secret_name = format!("{service_account}{SA_TOKEN_SECRET_SUFFIX}");
    apply_mc_sa_token_secret(&client, namespace, service_account).await?;
    eprintln!("⏳ Waiting for SA token to be populated...");
    let token = wait_for_sa_token(&client, namespace, &token_secret_name).await?;

    let kubeconfig_yaml = build_kubeconfig_yaml(
        &cluster_name,
        server,
        ca_data_b64.as_deref(),
        service_account,
        &token,
    )?;

    let secret = build_mc_kubeconfig_secret(namespace, service_account, &kubeconfig_yaml);
    let secret_name = format!("{service_account}{REMOTE_KUBECONFIG_SECRET_SUFFIX}");
    let secret_yaml =
        serde_yaml::to_string(&secret).context("Failed to serialize kubeconfig Secret")?;

    println!("---");
    print!("{secret_yaml}");

    eprintln!("\n✓ Apply the above Secret to each child cluster:");
    eprintln!("    bindy bootstrap mc | kubectl --context=<child-cluster> apply -f -");
    eprintln!("Then set BINDY_SCOUT_REMOTE_SECRET={secret_name} on the scout Deployment.");

    Ok(())
}

/// Build a kubeconfig YAML string for the given service account token.
///
/// When `ca_data_b64` is `None`, the generated kubeconfig sets
/// `insecure-skip-tls-verify: true`. Otherwise it embeds the CA data.
///
/// # Arguments
/// * `cluster_name` - Name of the cluster entry in the kubeconfig
/// * `server` - Kubernetes API server URL (e.g. `https://192.0.2.1:6443`)
/// * `ca_data_b64` - Base64-encoded PEM CA certificate, or `None` to skip TLS verify
/// * `sa_name` - Name of the service account / kubeconfig user entry
/// * `token` - Bearer token for the service account
///
/// # Errors
/// Returns error if YAML serialization fails.
pub fn build_kubeconfig_yaml(
    cluster_name: &str,
    server: &str,
    ca_data_b64: Option<&str>,
    sa_name: &str,
    token: &str,
) -> Result<String> {
    let cfg = BootstrapKubeconfig {
        api_version: "v1".to_string(),
        kind: "Config".to_string(),
        clusters: vec![BootstrapNamedCluster {
            name: cluster_name.to_string(),
            cluster: BootstrapCluster {
                server: server.to_string(),
                certificate_authority_data: ca_data_b64.map(str::to_string),
                insecure_skip_tls_verify: ca_data_b64.is_none().then_some(true),
            },
        }],
        contexts: vec![BootstrapNamedContext {
            name: "default".to_string(),
            context: BootstrapContext {
                cluster: cluster_name.to_string(),
                user: sa_name.to_string(),
            },
        }],
        current_context: "default".to_string(),
        users: vec![BootstrapNamedUser {
            name: sa_name.to_string(),
            user: BootstrapUser {
                token: token.to_string(),
            },
        }],
    };
    serde_yaml::to_string(&cfg).context("Failed to serialize kubeconfig YAML")
}

/// Build the multi-cluster ServiceAccount on the queen-ship cluster.
pub fn build_mc_service_account(namespace: &str, sa_name: &str) -> ServiceAccount {
    ServiceAccount {
        metadata: ObjectMeta {
            name: Some(sa_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        MC_COMPONENT_LABEL.to_string(),
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

/// Build the namespaced Role for the multi-cluster service account on the queen-ship.
///
/// Grants:
/// - Full CRUD on `arecords` — scout creates/deletes ARecords via the remote client
/// - Read-only on `dnszones` — scout validates zones before creating ARecords
///
/// Both resources live in the same target namespace on the queen-ship cluster, so a
/// namespaced `Role` is sufficient.  The scout watches DNSZones via
/// `Api::namespaced(remote_client, target_namespace)` (not `Api::all`), which means no
/// cluster-scoped `ClusterRole` is required.
///
/// The Role name matches the service account name, mirroring the convention in
/// `deploy/scout/remote-cluster-rbac.yaml`.
pub fn build_mc_writer_role(namespace: &str, sa_name: &str) -> Role {
    Role {
        metadata: ObjectMeta {
            name: Some(sa_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        MC_COMPONENT_LABEL.to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        rules: Some(vec![
            PolicyRule {
                api_groups: Some(vec!["bindy.firestoned.io".to_string()]),
                resources: Some(vec!["arecords".to_string()]),
                verbs: vec![
                    "get".to_string(),
                    "list".to_string(),
                    "watch".to_string(),
                    "create".to_string(),
                    "update".to_string(),
                    "patch".to_string(),
                    "delete".to_string(),
                ],
                ..Default::default()
            },
            PolicyRule {
                api_groups: Some(vec!["bindy.firestoned.io".to_string()]),
                resources: Some(vec!["dnszones".to_string()]),
                verbs: vec!["get".to_string(), "list".to_string(), "watch".to_string()],
                ..Default::default()
            },
        ]),
    }
}

/// Build the RoleBinding that binds the multi-cluster SA to its Role on the queen-ship.
///
/// The RoleBinding name matches the service account name, mirroring the convention in
/// `deploy/scout/remote-cluster-rbac.yaml`.
pub fn build_mc_writer_role_binding(namespace: &str, sa_name: &str) -> RoleBinding {
    RoleBinding {
        metadata: ObjectMeta {
            name: Some(sa_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        MC_COMPONENT_LABEL.to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".to_string(),
            kind: "Role".to_string(),
            name: sa_name.to_string(),
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".to_string(),
            name: sa_name.to_string(),
            namespace: Some(namespace.to_string()),
            api_group: Some(String::new()),
        }]),
    }
}

/// Build the `kubernetes.io/service-account-token` Secret that triggers token generation.
///
/// After this Secret is applied, the Kubernetes token controller populates `data.token`
/// with a long-lived bearer token for the specified service account.
pub fn build_mc_sa_token_secret(namespace: &str, sa_name: &str) -> Secret {
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "kubernetes.io/service-account.name".to_string(),
        sa_name.to_string(),
    );

    Secret {
        metadata: ObjectMeta {
            name: Some(format!("{sa_name}{SA_TOKEN_SECRET_SUFFIX}")),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        MC_COMPONENT_LABEL.to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            annotations: Some(annotations),
            ..Default::default()
        },
        type_: Some("kubernetes.io/service-account-token".to_string()),
        ..Default::default()
    }
}

/// Build the `bindy.firestoned.io/remote-kubeconfig` Secret containing the kubeconfig YAML.
///
/// The kubeconfig is stored under the `kubeconfig` key in `data`. Copy this Secret to the
/// queen-ship cluster to grant the operator access to the remote child cluster.
pub fn build_mc_kubeconfig_secret(namespace: &str, sa_name: &str, kubeconfig_yaml: &str) -> Secret {
    let mut data = BTreeMap::new();
    data.insert(
        "kubeconfig".to_string(),
        ByteString(kubeconfig_yaml.as_bytes().to_vec()),
    );

    Secret {
        metadata: ObjectMeta {
            name: Some(format!("{sa_name}{REMOTE_KUBECONFIG_SECRET_SUFFIX}")),
            namespace: Some(namespace.to_string()),
            labels: Some(
                [
                    ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                    (
                        "app.kubernetes.io/component".to_string(),
                        MC_COMPONENT_LABEL.to_string(),
                    ),
                    (
                        "bindy.firestoned.io/service-account".to_string(),
                        sa_name.to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        type_: Some(REMOTE_KUBECONFIG_SECRET_TYPE.to_string()),
        data: Some(data),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Multi-cluster apply helpers
// ---------------------------------------------------------------------------

async fn apply_mc_service_account(client: &Client, namespace: &str, sa_name: &str) -> Result<()> {
    let api: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    let sa = build_mc_service_account(namespace, sa_name);
    api.patch(
        sa_name,
        &PatchParams::apply(MC_FIELD_MANAGER).force(),
        &Patch::Apply(&sa),
    )
    .await
    .with_context(|| format!("Failed to apply ServiceAccount/{sa_name}"))?;
    eprintln!("✓ ServiceAccount: {sa_name} (namespace: {namespace})");
    Ok(())
}

async fn apply_mc_writer_role(client: &Client, namespace: &str, sa_name: &str) -> Result<()> {
    let api: Api<Role> = Api::namespaced(client.clone(), namespace);
    let role = build_mc_writer_role(namespace, sa_name);
    api.patch(
        sa_name,
        &PatchParams::apply(MC_FIELD_MANAGER).force(),
        &Patch::Apply(&role),
    )
    .await
    .with_context(|| format!("Failed to apply Role/{sa_name}"))?;
    eprintln!("✓ Role: {sa_name} (namespace: {namespace})");
    Ok(())
}

async fn apply_mc_writer_role_binding(
    client: &Client,
    namespace: &str,
    sa_name: &str,
) -> Result<()> {
    let api: Api<RoleBinding> = Api::namespaced(client.clone(), namespace);
    let rb = build_mc_writer_role_binding(namespace, sa_name);
    api.patch(
        sa_name,
        &PatchParams::apply(MC_FIELD_MANAGER).force(),
        &Patch::Apply(&rb),
    )
    .await
    .with_context(|| format!("Failed to apply RoleBinding/{sa_name}"))?;
    eprintln!("✓ RoleBinding: {sa_name} (namespace: {namespace})");
    Ok(())
}

async fn apply_mc_sa_token_secret(client: &Client, namespace: &str, sa_name: &str) -> Result<()> {
    let secret_name = format!("{sa_name}{SA_TOKEN_SECRET_SUFFIX}");
    let api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret = build_mc_sa_token_secret(namespace, sa_name);
    api.patch(
        &secret_name,
        &PatchParams::apply(MC_FIELD_MANAGER).force(),
        &Patch::Apply(&secret),
    )
    .await
    .with_context(|| format!("Failed to apply Secret/{secret_name}"))?;
    eprintln!("✓ Secret: {secret_name} (namespace: {namespace})");
    Ok(())
}

/// Poll the SA token Secret until the `token` key is populated, up to a bounded timeout.
///
/// The Kubernetes token controller populates the token typically within milliseconds.
/// This function retries up to `SA_TOKEN_WAIT_MAX_ATTEMPTS` times with
/// `SA_TOKEN_WAIT_INTERVAL_MS` ms between attempts (max ~10 seconds total).
async fn wait_for_sa_token(client: &Client, namespace: &str, secret_name: &str) -> Result<String> {
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);

    for _ in 0..SA_TOKEN_WAIT_MAX_ATTEMPTS {
        let secret = secret_api
            .get(secret_name)
            .await
            .with_context(|| format!("Failed to read Secret/{secret_name}"))?;

        if let Some(data) = &secret.data {
            if let Some(token_bytes) = data.get("token") {
                return String::from_utf8(token_bytes.0.clone())
                    .context("SA token bytes are not valid UTF-8");
            }
        }

        tokio::time::sleep(Duration::from_millis(SA_TOKEN_WAIT_INTERVAL_MS)).await;
    }

    Err(anyhow::anyhow!(
        "Timed out waiting for Secret/{secret_name} to be populated with a token"
    ))
}

/// Read the current KUBECONFIG context's cluster server URL and CA certificate.
///
/// Returns `(server_url, ca_data_base64, cluster_name)`.
/// `ca_data_base64` is `None` when neither inline data nor a CA file is configured,
/// in which case `build_kubeconfig_yaml` sets `insecure-skip-tls-verify: true`.
fn read_cluster_info() -> Result<(String, Option<String>, String)> {
    let raw = Kubeconfig::read().context(
        "Failed to read KUBECONFIG — ensure KUBECONFIG env var is set or ~/.kube/config exists",
    )?;

    let current_context = raw.current_context.as_deref().unwrap_or_default();

    let named_context = raw
        .contexts
        .iter()
        .find(|c| c.name == current_context)
        .ok_or_else(|| {
            anyhow::anyhow!("Current context '{current_context}' not found in KUBECONFIG")
        })?;

    let ctx = named_context
        .context
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Context '{current_context}' has no data in KUBECONFIG"))?;

    let cluster_name = ctx.cluster.clone();

    let named_cluster = raw
        .clusters
        .iter()
        .find(|c| c.name == cluster_name)
        .ok_or_else(|| anyhow::anyhow!("Cluster '{cluster_name}' not found in KUBECONFIG"))?;

    let cluster = named_cluster
        .cluster
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Cluster '{cluster_name}' has no data in KUBECONFIG"))?;

    let server = cluster
        .server
        .clone()
        .unwrap_or_else(|| "https://kubernetes.default.svc".to_string());

    // Prefer inline base64-encoded CA; fall back to reading from a file path.
    let ca_data = if let Some(ca_b64) = &cluster.certificate_authority_data {
        Some(ca_b64.clone())
    } else if let Some(ca_path) = &cluster.certificate_authority {
        let bytes = std::fs::read(ca_path)
            .with_context(|| format!("Failed to read CA certificate file: {ca_path}"))?;
        Some(STANDARD.encode(bytes))
    } else {
        None
    };

    Ok((server, ca_data, cluster_name))
}

/// Build the scout Deployment manifest.
///
/// The container image defaults to `ghcr.io/firestoned/bindy:<image_tag>`.
/// Pass `registry` to override the registry/org prefix for air-gapped environments.
///
/// Scout CLI args (`--cluster-name`, `--default-ips`, `--default-zone`) are passed
/// directly to the container command so the scout behaves consistently with `bindy scout`.
pub fn build_scout_deployment(
    namespace: &str,
    opts: &ScoutDeploymentOptions<'_>,
) -> Result<Deployment> {
    let image = resolve_image(opts.registry, opts.image_tag);

    let mut args: Vec<serde_json::Value> = vec![
        serde_json::json!("scout"),
        serde_json::json!("--cluster-name"),
        serde_json::json!(opts.cluster_name),
    ];
    if !opts.default_ips.is_empty() {
        args.push(serde_json::json!("--default-ips"));
        args.push(serde_json::json!(opts.default_ips.join(",")));
    }
    if let Some(zone) = opts.default_zone {
        args.push(serde_json::json!("--default-zone"));
        args.push(serde_json::json!(zone));
    }

    let mut env: Vec<serde_json::Value> = vec![
        serde_json::json!({
            "name": "POD_NAMESPACE",
            "valueFrom": {"fieldRef": {"fieldPath": "metadata.namespace"}}
        }),
        serde_json::json!({"name": "RUST_LOG", "value": "info"}),
        serde_json::json!({"name": "RUST_LOG_FORMAT", "value": "text"}),
    ];
    if let Some(secret) = opts.remote_secret {
        env.push(serde_json::json!({"name": "BINDY_SCOUT_REMOTE_SECRET", "value": secret}));
    }

    let value = serde_json::json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": SCOUT_DEPLOYMENT_NAME,
            "namespace": namespace,
            "labels": {
                "app.kubernetes.io/name": "bindy",
                "app.kubernetes.io/component": "scout"
            }
        },
        "spec": {
            "replicas": 1,
            "selector": {
                "matchLabels": {
                    "app.kubernetes.io/name": "bindy",
                    "app.kubernetes.io/component": "scout"
                }
            },
            "template": {
                "metadata": {
                    "labels": {
                        "app.kubernetes.io/name": "bindy",
                        "app.kubernetes.io/component": "scout"
                    }
                },
                "spec": {
                    "serviceAccountName": SCOUT_SERVICE_ACCOUNT_NAME,
                    "securityContext": {"runAsNonRoot": true, "fsGroup": 65_534_i64},
                    "containers": [{
                        "name": "scout",
                        "image": image,
                        "imagePullPolicy": "IfNotPresent",
                        "args": args,
                        "env": env,
                        "securityContext": {
                            "allowPrivilegeEscalation": false,
                            "capabilities": {"drop": ["ALL"]},
                            "readOnlyRootFilesystem": true,
                            "runAsNonRoot": true,
                            "runAsUser": 65_534_i64
                        },
                        "resources": {
                            "limits": {"cpu": "200m", "memory": "128Mi"},
                            "requests": {"cpu": "50m", "memory": "64Mi"}
                        },
                        "volumeMounts": [{"name": "tmp", "mountPath": "/tmp"}]
                    }],
                    "volumes": [{"name": "tmp", "emptyDir": {}}]
                }
            }
        }
    });
    serde_json::from_value(value).context("Failed to build scout Deployment")
}

// ---------------------------------------------------------------------------
// Multi-cluster revoke
// ---------------------------------------------------------------------------

/// Revoke all resources that `bootstrap mc` created for a given service account.
///
/// Deletes in reverse creation order (bindings before roles, roles before SA) so
/// that access is cut off at the earliest possible step. Missing resources are
/// silently skipped — it is safe to call this function more than once.
///
/// Run this command **against the queen-ship cluster** (the same context used
/// when the resources were originally created).
///
/// # Arguments
/// * `namespace` - Namespace the resources were created in
/// * `service_account` - Name of the ServiceAccount that was created by `bootstrap mc`
///
/// # Errors
/// Returns an error if the Kubernetes API call fails for any reason other than 404.
pub async fn run_revoke_multi_cluster(namespace: &str, service_account: &str) -> Result<()> {
    let client = Client::try_default()
        .await
        .context("Failed to connect to Kubernetes cluster — is KUBECONFIG set?")?;

    // Revoke in reverse creation order: bindings → roles → secrets → SA
    delete_mc_role_binding(&client, namespace, service_account).await?;
    delete_mc_role(&client, namespace, service_account).await?;

    let kubeconfig_secret = format!("{service_account}{REMOTE_KUBECONFIG_SECRET_SUFFIX}");
    delete_mc_secret(&client, namespace, &kubeconfig_secret).await?;

    let token_secret = format!("{service_account}{SA_TOKEN_SECRET_SUFFIX}");
    delete_mc_secret(&client, namespace, &token_secret).await?;

    delete_mc_service_account(&client, namespace, service_account).await?;

    eprintln!("\n✓ Revoked multi-cluster access for: {service_account} (namespace: {namespace})");
    Ok(())
}

async fn delete_mc_role_binding(client: &Client, namespace: &str, sa_name: &str) -> Result<()> {
    let api: Api<RoleBinding> = Api::namespaced(client.clone(), namespace);
    match api.delete(sa_name, &DeleteParams::default()).await {
        Ok(_) => eprintln!("✓ Deleted RoleBinding: {sa_name} (namespace: {namespace})"),
        Err(kube::Error::Api(ref s)) if s.code == HTTP_NOT_FOUND => {
            eprintln!("  RoleBinding/{sa_name} not found, skipping");
        }
        Err(e) => {
            return Err(anyhow::Error::from(e))
                .with_context(|| format!("Failed to delete RoleBinding/{sa_name}"));
        }
    }
    Ok(())
}

async fn delete_mc_role(client: &Client, namespace: &str, sa_name: &str) -> Result<()> {
    let api: Api<Role> = Api::namespaced(client.clone(), namespace);
    match api.delete(sa_name, &DeleteParams::default()).await {
        Ok(_) => eprintln!("✓ Deleted Role: {sa_name} (namespace: {namespace})"),
        Err(kube::Error::Api(ref s)) if s.code == HTTP_NOT_FOUND => {
            eprintln!("  Role/{sa_name} not found, skipping");
        }
        Err(e) => {
            return Err(anyhow::Error::from(e))
                .with_context(|| format!("Failed to delete Role/{sa_name}"));
        }
    }
    Ok(())
}

async fn delete_mc_secret(client: &Client, namespace: &str, secret_name: &str) -> Result<()> {
    let api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    match api.delete(secret_name, &DeleteParams::default()).await {
        Ok(_) => eprintln!("✓ Deleted Secret: {secret_name} (namespace: {namespace})"),
        Err(kube::Error::Api(ref s)) if s.code == HTTP_NOT_FOUND => {
            eprintln!("  Secret/{secret_name} not found, skipping");
        }
        Err(e) => {
            return Err(anyhow::Error::from(e))
                .with_context(|| format!("Failed to delete Secret/{secret_name}"));
        }
    }
    Ok(())
}

async fn delete_mc_service_account(client: &Client, namespace: &str, sa_name: &str) -> Result<()> {
    let api: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    match api.delete(sa_name, &DeleteParams::default()).await {
        Ok(_) => eprintln!("✓ Deleted ServiceAccount: {sa_name} (namespace: {namespace})"),
        Err(kube::Error::Api(ref s)) if s.code == HTTP_NOT_FOUND => {
            eprintln!("  ServiceAccount/{sa_name} not found, skipping");
        }
        Err(e) => {
            return Err(anyhow::Error::from(e))
                .with_context(|| format!("Failed to delete ServiceAccount/{sa_name}"));
        }
    }
    Ok(())
}
