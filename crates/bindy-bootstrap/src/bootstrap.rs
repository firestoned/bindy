// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Bootstrap logic for `bindy bootstrap`.
//!
//! ## `bindy bootstrap operator`
//! Applies all operator prerequisites to a Kubernetes cluster in order:
//! 1. Namespace (`bindy-system` by default, or `--namespace`)
//! 2. CRDs — generated from Rust types, always in sync with the operator
//! 3. ServiceAccount (`bindy`)
//! 4. ClusterRole (`bindy-role`) — operator permissions
//! 5. ClusterRole (`bindy-admin-role`) — admin/destructive permissions
//! 6. ClusterRole (`bindcar-tokenreview`) — `create tokenreviews` for the bindcar sidecar
//! 7. ClusterRoleBinding (`bindy-rolebinding`) — binds SA to operator role
//! 8. ClusterRoleBinding (`bindcar-tokenreview`) — binds the operand `bind9` SA
//!    (in the requested `--namespace`) to the TokenReview ClusterRole
//! 9. Role + RoleBinding (`bindy-secrets-writer`) — namespaced Secret write access
//! 10. Deployment (`bindy`) — the operator itself, with a projected SA token
//!     (`audience: bindcar`) and `POD_NAMESPACE` from the downward API so it can
//!     authenticate to bindcar 0.7.0 sidecars
//!
//! ## `bindy bootstrap scout`
//! Applies all scout prerequisites to a Kubernetes cluster in order:
//! 1. Namespace (`bindy-system` by default, or `--namespace`)
//! 2. CRDs: the same 13 CRDs as the operator (shared types)
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
//! 5. Kubeconfig Secret (`bindy-scout-remote-kubeconfig`): a ready-to-use
//!    kubeconfig for the SA, written as YAML to a file readable only by its owner
//!    (mode 0600; `--output`, default `<service-account>-kubeconfig.yaml`).
//!    It carries the SA token, so it is never printed.
//!
//! The file is applied to the **child cluster** where scout runs, then deleted:
//! ```text
//! bindy bootstrap mc
//! kubectl --context=<child-cluster> apply -f bindy-scout-remote-kubeconfig.yaml
//! rm bindy-scout-remote-kubeconfig.yaml
//! ```
//! Then set `BINDY_SCOUT_REMOTE_SECRET=bindy-scout-remote-kubeconfig` on the
//! scout Deployment so it picks up the remote kubeconfig at startup.

use anyhow::{anyhow, Context as _, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bindy_controller_sdk::resources::delete_if_present;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Namespace, Secret, ServiceAccount};
use k8s_openapi::api::rbac::v1::{
    ClusterRole, ClusterRoleBinding, PolicyRule, Role, RoleBinding, RoleRef, Subject,
};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::{ByteString, NamespaceResourceScope};
use kube::{
    api::{Patch, PatchParams},
    config::Kubeconfig,
    Api, Client, CustomResourceExt, Resource,
};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::time::Duration;

use crate::crd::{Bind9Cluster, Bind9Instance, ClusterBind9Provider, DNSZone};

/// Default namespace for the bindy operator deployment.
pub const DEFAULT_NAMESPACE: &str = "bindy-system";

/// Field manager name used for server-side apply.
const FIELD_MANAGER: &str = "bindy-bootstrap";

/// ServiceAccount name created for the operator.
pub(crate) const SERVICE_ACCOUNT_NAME: &str = "bindy";

/// ClusterRoleBinding name.
pub(crate) const CLUSTER_ROLE_BINDING_NAME: &str = "bindy-rolebinding";

/// Operator ClusterRole name.
pub(crate) const OPERATOR_ROLE_NAME: &str = "bindy-role";

/// Namespaced Role granting the operator the mutating verbs on Secrets (B-5 hardening).
///
/// The operator ClusterRole grants only read-only (get/list/watch) on Secrets
/// cluster-wide; create/update/patch/delete are confined to this namespaced Role,
/// bound only in the operator namespace.
pub(crate) const SECRETS_WRITER_ROLE_NAME: &str = "bindy-secrets-writer";

/// Namespaced RoleBinding name for the secrets-writer Role.
pub(crate) const SECRETS_WRITER_ROLE_BINDING_NAME: &str = "bindy-secrets-writer";

/// Operator Deployment name.
pub(crate) const OPERATOR_DEPLOYMENT_NAME: &str = "bindy";

/// Container image registry and repository (without tag).
pub(crate) const OPERATOR_IMAGE_BASE: &str = "ghcr.io/firestoned/bindy";

/// Default image tag for operator and scout Deployments.
///
/// Always matches the binary's own version (e.g. `"v0.5.0"`) so that
/// `bindy bootstrap` installs exactly the image that was shipped with this binary.
pub const DEFAULT_IMAGE_TAG: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// Embedded RBAC YAML files — compiled into the binary so bootstrap is self-contained.
pub(crate) const BINDY_ROLE_YAML: &str = include_str!("../../../deploy/operator/rbac/role.yaml");
pub(crate) const BINDY_ADMIN_ROLE_YAML: &str =
    include_str!("../../../deploy/operator/rbac/role-admin.yaml");

/// Embedded TokenReview ClusterRole (bindcar `0.7.0` Mode B).
///
/// Grants `create tokenreviews` so the bindcar sidecar (running as the operand
/// `bind9` ServiceAccount) can validate the operator's bearer token against the
/// API server. Mirrors `deploy/operator/rbac/tokenreview-clusterrole.yaml`.
pub(crate) const BINDCAR_TOKENREVIEW_CLUSTER_ROLE_YAML: &str =
    include_str!("../../../deploy/operator/rbac/tokenreview-clusterrole.yaml");

/// Embedded TokenReview ClusterRoleBinding (bindcar `0.7.0` Mode B).
///
/// The static manifest binds the operand `bind9` ServiceAccount in
/// `bindy-system`; the bootstrap path rewrites the subject namespace to the
/// requested `--namespace` via [`build_tokenreview_cluster_role_binding`].
pub(crate) const BINDCAR_TOKENREVIEW_CLUSTER_ROLE_BINDING_YAML: &str =
    include_str!("../../../deploy/operator/rbac/tokenreview-clusterrolebinding.yaml");

/// Name shared by the TokenReview ClusterRole and ClusterRoleBinding.
pub(crate) const BINDCAR_TOKENREVIEW_NAME: &str = "bindcar-tokenreview";

/// Embedded ClusterRole manifests applied by `bindy bootstrap operator`, in apply order.
pub(crate) const OPERATOR_CLUSTER_ROLE_YAMLS: &[&str] = &[
    BINDY_ROLE_YAML,
    BINDY_ADMIN_ROLE_YAML,
    BINDCAR_TOKENREVIEW_CLUSTER_ROLE_YAML,
];

/// Volume name for the projected ServiceAccount token carrying the `bindcar` audience.
///
/// Mirrors the `bindcar-token` volume in `deploy/operator/deployment.yaml`.
pub(crate) const BINDCAR_TOKEN_VOLUME_NAME: &str = "bindcar-token";

/// Mount path of the projected bindcar token volume inside the operator container.
///
/// Together with [`BINDCAR_TOKEN_FILENAME`] this composes
/// `bindy_bind9::bind9::BINDCAR_TOKEN_PATH` (`/var/run/secrets/bindcar/token`),
/// the path the operator reads the bearer token from.
pub(crate) const BINDCAR_TOKEN_MOUNT_PATH: &str = "/var/run/secrets/bindcar";

/// File name of the projected token inside [`BINDCAR_TOKEN_MOUNT_PATH`].
pub(crate) const BINDCAR_TOKEN_FILENAME: &str = "token";

/// Expiration (seconds) for the projected bindcar token.
///
/// The kubelet automatically rotates the token before expiry; mirrors
/// `expirationSeconds: 3600` in `deploy/operator/deployment.yaml`.
pub(crate) const BINDCAR_TOKEN_EXPIRATION_SECONDS: i64 = 3600;

/// Downward-API environment variable carrying the operator pod's namespace.
///
/// `src/bind9_resources.rs` reads this to compose the
/// `BIND_ALLOWED_SERVICE_ACCOUNTS` value for operand bindcar sidecars; without
/// it the operator falls back to `bindy-system` and bindcar rejects tokens
/// from operators installed in any other namespace.
pub(crate) const POD_NAMESPACE_ENV: &str = "POD_NAMESPACE";

/// Environment variable naming the namespace Scout writes `ARecord`s into.
///
/// `bindy scout` reads it when `--namespace` is not passed and otherwise falls
/// back to `bindy-system`. Bootstrap creates the scout writer Role/RoleBinding
/// in the `--namespace` it was given, so the Deployment must point the scout at
/// that same namespace or it writes where it has no RBAC.
pub(crate) const SCOUT_TARGET_NAMESPACE_ENV: &str = "BINDY_SCOUT_NAMESPACE";

// ---------------------------------------------------------------------------
// Scout constants
// ---------------------------------------------------------------------------

/// Scout ServiceAccount name.
pub(crate) const SCOUT_SERVICE_ACCOUNT_NAME: &str = "bindy-scout";

/// Scout ClusterRole name.
pub(crate) const SCOUT_CLUSTER_ROLE_NAME: &str = "bindy-scout";

/// Scout ClusterRoleBinding name.
pub(crate) const SCOUT_CLUSTER_ROLE_BINDING_NAME: &str = "bindy-scout";

/// Scout namespaced Role name (ARecord write permissions).
pub(crate) const SCOUT_WRITER_ROLE_NAME: &str = "bindy-scout-writer";

/// Scout namespaced RoleBinding name.
pub(crate) const SCOUT_WRITER_ROLE_BINDING_NAME: &str = "bindy-scout-writer";

/// Scout namespaced Role name (remote-cluster kubeconfig Secret read, Phase 2 only).
///
/// Namespaced and `resourceNames`-restricted to the single configured Secret —
/// deliberately NOT part of the cluster-wide `bindy-scout` ClusterRole, which
/// previously granted unscoped `secrets: get` across every namespace in the
/// cluster (closed 2026-07-19).
pub(crate) const SCOUT_SECRETS_READER_ROLE_NAME: &str = "bindy-scout-secrets-reader";

/// Scout namespaced RoleBinding name for [`SCOUT_SECRETS_READER_ROLE_NAME`].
pub(crate) const SCOUT_SECRETS_READER_ROLE_BINDING_NAME: &str = "bindy-scout-secrets-reader";

/// Scout Deployment name.
pub(crate) const SCOUT_DEPLOYMENT_NAME: &str = "bindy-scout";

/// Default ServiceAccount name created by `bootstrap mc` on the queen-ship cluster.
///
/// Each child cluster gets its own SA so access can be revoked independently.
/// The local in-cluster scout SA is named `SCOUT_SERVICE_ACCOUNT_NAME` (`bindy-scout`);
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
pub(crate) const REMOTE_KUBECONFIG_SECRET_TYPE: &str = "bindy.firestoned.io/remote-kubeconfig";

/// Suffix appended to the service account name when naming the SA token Secret.
///
/// For example, SA `scout` produces token Secret `scout-token`.
pub(crate) const SA_TOKEN_SECRET_SUFFIX: &str = "-token";

/// Suffix appended to the service account name when naming the remote kubeconfig Secret.
///
/// The default SA `bindy-scout-remote` produces `bindy-scout-remote-kubeconfig`, the
/// Secret the static `deploy/scout/secrets-reader-rbac.yaml` lets the scout read.
pub(crate) const REMOTE_KUBECONFIG_SECRET_SUFFIX: &str = "-kubeconfig";

/// `app.kubernetes.io/component` label value for all resources created by `bootstrap mc`.
const MC_COMPONENT_LABEL: &str = "scout-remote";

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
pub(crate) fn resolve_image(registry: Option<&str>, tag: &str) -> String {
    match registry {
        None => format!("{OPERATOR_IMAGE_BASE}:{tag}"),
        Some(reg) => format!("{}/bindy:{}", reg.trim_end_matches('/'), tag),
    }
}

/// Run the operator bootstrap process (`bindy bootstrap operator`).
///
/// When `dry_run` is `true`, prints the resources that would be applied to stdout (as YAML)
/// without connecting to a cluster. When `false`, applies each resource via server-side apply
/// (idempotent, safe to run multiple times). Both modes walk the same `operator_steps`
/// list, so what `--dry-run` shows is exactly what is applied.
///
/// # Arguments
/// * `namespace` - Namespace to install bindy into (default: `bindy-system`)
/// * `dry_run` - If true, print what would be applied without applying
/// * `image_tag` - Image tag for the operator Deployment (e.g. `"v0.5.0"` or `"latest"`)
/// * `registry` - Optional registry override for air-gapped environments
///
/// # Errors
/// Returns error if a resource cannot be built or serialized, or (in non-dry-run mode)
/// if Kubernetes API calls fail.
pub async fn run_bootstrap_operator(
    namespace: &str,
    dry_run: bool,
    image_tag: &str,
    registry: Option<&str>,
) -> Result<()> {
    let steps = operator_steps(namespace, image_tag, registry)?;
    if dry_run {
        print!("{}", render_dry_run(OPERATOR_DRY_RUN_BANNER, &steps)?);
        return Ok(());
    }

    let client = Client::try_default()
        .await
        .context("Failed to connect to Kubernetes cluster — is KUBECONFIG set?")?;
    apply_steps(&client, namespace, &steps, FIELD_MANAGER).await?;

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
/// (idempotent, safe to run multiple times). Both modes walk the same `scout_steps` list.
///
/// # Arguments
/// * `namespace` - Namespace to install scout into (default: `bindy-system`)
/// * `dry_run` - If true, print what would be applied without applying
/// * `opts` - Deployment configuration (image, registry, cluster name, IPs, zone, remote secret)
///
/// # Errors
/// Returns error if a resource cannot be built or serialized, or (in non-dry-run mode)
/// if Kubernetes API calls fail.
pub async fn run_bootstrap_scout(
    namespace: &str,
    dry_run: bool,
    opts: &ScoutDeploymentOptions<'_>,
) -> Result<()> {
    let steps = scout_steps(namespace, opts)?;
    if dry_run {
        print!("{}", render_dry_run(SCOUT_DRY_RUN_BANNER, &steps)?);
        return Ok(());
    }

    let client = Client::try_default()
        .await
        .context("Failed to connect to Kubernetes cluster — is KUBECONFIG set?")?;
    apply_steps(&client, namespace, &steps, SCOUT_FIELD_MANAGER).await?;

    println!("\nBootstrap complete! Scout is running in namespace {namespace}.");

    Ok(())
}

// ---------------------------------------------------------------------------
// Steps: the one object list each command both prints (--dry-run) and applies
// ---------------------------------------------------------------------------

/// First line of `bindy bootstrap operator --dry-run`.
pub(crate) const OPERATOR_DRY_RUN_BANNER: &str =
    "# Dry-run mode \u{2014} no resources will be applied";

/// First line of `bindy bootstrap scout --dry-run`.
pub(crate) const SCOUT_DRY_RUN_BANNER: &str =
    "# Dry-run mode (scout) \u{2014} no resources will be applied";

/// Last line of every `--dry-run`.
const DRY_RUN_FOOTER: &str = "# Dry-run complete \u{2014} no resources were applied";

/// Declares [`BootstrapObject`], one boxed variant per kind (named after the
/// kind), and a `From<Kind>` impl for each.
macro_rules! bootstrap_objects {
    ($($kind:ident),* $(,)?) => {
        /// An object a bootstrap command applies.
        ///
        /// Serializes as the wrapped object itself (`untagged`), so `--dry-run`
        /// prints exactly the manifest that is applied. Variants are boxed: a
        /// Deployment is several times the size of the other kinds.
        #[derive(serde::Serialize)]
        #[serde(untagged)]
        pub(crate) enum BootstrapObject {
            $($kind(Box<$kind>),)*
        }

        $(impl From<$kind> for BootstrapObject {
            fn from(object: $kind) -> Self {
                Self::$kind(Box::new(object))
            }
        })*
    };
}

bootstrap_objects!(
    Namespace,
    CustomResourceDefinition,
    ServiceAccount,
    ClusterRole,
    ClusterRoleBinding,
    Role,
    RoleBinding,
    Deployment,
);

/// One entry of a command's object list.
pub(crate) struct Step {
    /// The object to apply.
    pub(crate) object: BootstrapObject,
    /// The `# <heading>` line above the object in `--dry-run` output.
    pub(crate) heading: String,
    /// The `✓ ...` line printed once the object is applied.
    pub(crate) applied: String,
}

/// Build a [`Step`].
fn step(object: impl Into<BootstrapObject>, heading: impl Into<String>, applied: String) -> Step {
    Step {
        object: object.into(),
        heading: heading.into(),
        applied,
    }
}

/// The Namespace and CRD steps every install command starts with.
fn namespace_and_crd_steps(namespace: &str) -> Result<Vec<Step>> {
    let mut steps = vec![step(
        build_namespace(namespace),
        "Namespace",
        format!("✓ Namespace: {namespace}"),
    )];
    for crd in build_all_crds()? {
        let name = crd.metadata.name.clone().unwrap_or_default();
        let heading = format!("CustomResourceDefinition/{name}");
        steps.push(step(crd, heading, format!("✓ CRD: {name}")));
    }
    Ok(steps)
}

/// The objects `bindy bootstrap operator` applies, in apply order.
///
/// # Arguments
/// * `namespace` - Namespace to install bindy into
/// * `image_tag` - Image tag for the operator Deployment
/// * `registry` - Optional registry override
///
/// # Errors
/// Returns an error if a CRD, embedded RBAC manifest or the Deployment fails to build.
pub(crate) fn operator_steps(
    namespace: &str,
    image_tag: &str,
    registry: Option<&str>,
) -> Result<Vec<Step>> {
    let ns = namespace;
    let mut steps = namespace_and_crd_steps(ns)?;
    steps.push(step(
        build_service_account(ns),
        "ServiceAccount",
        format!("✓ ServiceAccount: {SERVICE_ACCOUNT_NAME} (namespace: {ns})"),
    ));
    for yaml in OPERATOR_CLUSTER_ROLE_YAMLS {
        let role = parse_cluster_role(yaml)?;
        let name = role.metadata.name.clone().unwrap_or_default();
        let heading = format!("ClusterRole ({name})");
        steps.push(step(role, heading, format!("✓ ClusterRole: {name}")));
    }
    let image = resolve_image(registry, image_tag);
    steps.extend([
        step(
            build_cluster_role_binding(ns),
            "ClusterRoleBinding",
            format!("✓ ClusterRoleBinding: {CLUSTER_ROLE_BINDING_NAME}"),
        ),
        step(
            build_tokenreview_cluster_role_binding(ns)?,
            "ClusterRoleBinding (bindcar-tokenreview)",
            format!("✓ ClusterRoleBinding: {BINDCAR_TOKENREVIEW_NAME} (subject namespace: {ns})"),
        ),
        step(
            build_secrets_writer_role(ns),
            "Role (secrets-writer)",
            format!("✓ Role: {SECRETS_WRITER_ROLE_NAME} (namespace: {ns})"),
        ),
        step(
            build_secrets_writer_role_binding(ns),
            "RoleBinding (secrets-writer)",
            format!("✓ RoleBinding: {SECRETS_WRITER_ROLE_BINDING_NAME} (namespace: {ns})"),
        ),
        step(
            build_deployment(ns, image_tag, registry)?,
            "Deployment",
            format!("✓ Deployment: {OPERATOR_DEPLOYMENT_NAME} (image: {image})"),
        ),
    ]);
    Ok(steps)
}

/// The objects `bindy bootstrap scout` applies, in apply order.
///
/// The namespaced, `resourceNames`-restricted secrets-reader Role and RoleBinding
/// are included only when `opts.remote_secret` is set (Phase 2 mode); same-cluster
/// deployments get no Secret access at all.
///
/// # Arguments
/// * `namespace` - Namespace to install scout into
/// * `opts` - Deployment configuration
///
/// # Errors
/// Returns an error if a CRD or the Deployment fails to build.
pub(crate) fn scout_steps(namespace: &str, opts: &ScoutDeploymentOptions<'_>) -> Result<Vec<Step>> {
    let ns = namespace;
    let mut steps = namespace_and_crd_steps(ns)?;
    steps.extend([
        step(
            build_scout_service_account(ns),
            "ServiceAccount (scout)",
            format!("✓ ServiceAccount: {SCOUT_SERVICE_ACCOUNT_NAME} (namespace: {ns})"),
        ),
        step(
            build_scout_cluster_role(),
            "ClusterRole (scout)",
            format!("✓ ClusterRole: {SCOUT_CLUSTER_ROLE_NAME}"),
        ),
        step(
            build_scout_cluster_role_binding(ns),
            "ClusterRoleBinding (scout)",
            format!("✓ ClusterRoleBinding: {SCOUT_CLUSTER_ROLE_BINDING_NAME}"),
        ),
        step(
            build_scout_writer_role(ns),
            "Role (scout-writer)",
            format!("✓ Role: {SCOUT_WRITER_ROLE_NAME} (namespace: {ns})"),
        ),
        step(
            build_scout_writer_role_binding(ns),
            "RoleBinding (scout-writer)",
            format!("✓ RoleBinding: {SCOUT_WRITER_ROLE_BINDING_NAME} (namespace: {ns})"),
        ),
    ]);
    if let Some(secret) = opts.remote_secret {
        steps.extend([
            step(
                build_scout_secrets_reader_role(ns, secret),
                "Role (scout-secrets-reader)",
                format!(
                    "✓ Role: {SCOUT_SECRETS_READER_ROLE_NAME} (namespace: {ns}, secret: {secret})"
                ),
            ),
            step(
                build_scout_secrets_reader_role_binding(ns),
                "RoleBinding (scout-secrets-reader)",
                format!(
                    "✓ RoleBinding: {SCOUT_SECRETS_READER_ROLE_BINDING_NAME} (namespace: {ns})"
                ),
            ),
        ]);
    }
    let image = resolve_image(opts.registry, opts.image_tag);
    steps.push(step(
        build_scout_deployment(ns, opts)?,
        "Deployment (scout)",
        format!("✓ Deployment: {SCOUT_DEPLOYMENT_NAME} (image: {image})"),
    ));
    Ok(steps)
}

/// Render the `--dry-run` output for `steps`: the banner, a blank line, then a
/// `---` / `# <heading>` / YAML block per object, then the footer.
///
/// # Errors
/// Returns `Failed to serialize <heading>` if an object cannot be rendered as YAML.
pub(crate) fn render_dry_run(banner: &str, steps: &[Step]) -> Result<String> {
    let mut out = format!("{banner}\n\n");
    for step in steps {
        let yaml = bindy_api::yaml::to_string(&step.object)
            .with_context(|| format!("Failed to serialize {}", step.heading))?;
        out.push_str(&format!("---\n# {}\n{yaml}", step.heading));
    }
    out.push_str(DRY_RUN_FOOTER);
    out.push('\n');
    Ok(out)
}

/// Apply `steps` in order, printing each step's `✓` line to stdout once applied.
///
/// Cluster-scoped objects go through `Api::all`; namespaced ones through
/// `Api::namespaced(namespace)`.
///
/// # Errors
/// Stops at, and returns, the first failed apply.
async fn apply_steps(
    client: &Client,
    namespace: &str,
    steps: &[Step],
    field_manager: &str,
) -> Result<()> {
    use BootstrapObject as O;

    let fm = field_manager;
    for step in steps {
        let c = client.clone();
        let ns = namespace;
        match &step.object {
            O::Namespace(o) => apply(&Api::all(c), &**o, fm).await?,
            O::CustomResourceDefinition(o) => apply(&Api::all(c), &**o, fm).await?,
            O::ClusterRole(o) => apply(&Api::all(c), &**o, fm).await?,
            O::ClusterRoleBinding(o) => apply(&Api::all(c), &**o, fm).await?,
            O::ServiceAccount(o) => apply(&Api::namespaced(c, ns), &**o, fm).await?,
            O::Role(o) => apply(&Api::namespaced(c, ns), &**o, fm).await?,
            O::RoleBinding(o) => apply(&Api::namespaced(c, ns), &**o, fm).await?,
            O::Deployment(o) => apply(&Api::namespaced(c, ns), &**o, fm).await?,
        }
        println!("{}", step.applied);
    }
    Ok(())
}

/// Server-side apply one object (`force: true`) under `field_manager`.
///
/// The object is addressed by its own `metadata.name`; `api` decides the scope
/// (cluster or namespace).
///
/// # Arguments
/// * `api` - The API for the object's kind and scope
/// * `obj` - The object to apply
/// * `field_manager` - The server-side-apply field manager
///
/// # Errors
/// Returns `Cannot apply <Kind>: metadata.name is not set` (without calling the
/// API) when the object has no name, and `Failed to apply <Kind>/<name>` with the
/// `kube::Error` in the chain when the API call fails.
pub(crate) async fn apply<K>(api: &Api<K>, obj: &K, field_manager: &str) -> Result<()>
where
    K: Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + serde::Serialize,
{
    let kind = K::kind(&());
    let Some(name) = obj.meta().name.as_deref() else {
        return Err(anyhow!("Cannot apply {kind}: metadata.name is not set"));
    };
    api.patch(
        name,
        &PatchParams::apply(field_manager).force(),
        &Patch::Apply(obj),
    )
    .await
    .with_context(|| format!("Failed to apply {kind}/{name}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Resource builders
// ---------------------------------------------------------------------------

/// API group of `Role`, `ClusterRole` and their bindings.
const RBAC_API_GROUP: &str = "rbac.authorization.k8s.io";

/// The core (`""`) API group, as it appears in a `PolicyRule`.
const CORE_API_GROUP: &str = "";

/// API group of the bindy CRDs.
const BINDY_API_GROUP: &str = "bindy.firestoned.io";

/// `app.kubernetes.io/name` label key.
const APP_NAME_LABEL: &str = "app.kubernetes.io/name";

/// `app.kubernetes.io/component` label key.
const APP_COMPONENT_LABEL: &str = "app.kubernetes.io/component";

/// `app.kubernetes.io/name` value on everything bootstrap creates.
const APP_NAME: &str = "bindy";

/// `app.kubernetes.io/component` value for the operator's RBAC objects.
const RBAC_COMPONENT: &str = "rbac";

/// `app.kubernetes.io/component` value for Scout's objects.
const SCOUT_COMPONENT: &str = "scout";

/// The read-only verbs.
const READ_VERBS: &[&str] = &["get", "list", "watch"];

/// The verbs Scout needs on a watched source kind: read, plus patch/update so the
/// kube-rs finalizer can add and remove itself on the object's metadata.
const WATCH_AND_FINALIZE_VERBS: &[&str] = &["get", "list", "watch", "patch", "update"];

/// Full CRUD.
const CRUD_VERBS: &[&str] = &[
    "get", "list", "watch", "create", "update", "patch", "delete",
];

/// The `app.kubernetes.io/name: bindy` + `app.kubernetes.io/component` label pair.
pub(crate) fn labels(component: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (APP_NAME_LABEL.to_string(), APP_NAME.to_string()),
        (APP_COMPONENT_LABEL.to_string(), component.to_string()),
    ])
}

/// A binding subject for ServiceAccount `name` in `namespace`.
pub(crate) fn sa_subject(name: &str, namespace: &str) -> Subject {
    Subject {
        kind: <ServiceAccount as k8s_openapi::Resource>::KIND.to_string(),
        name: name.to_string(),
        namespace: Some(namespace.to_string()),
        api_group: Some(CORE_API_GROUP.to_string()),
    }
}

/// A `roleRef` to the `R` (`Role` or `ClusterRole`) named `name`.
pub(crate) fn role_ref<R: k8s_openapi::Resource>(name: &str) -> RoleRef {
    RoleRef {
        api_group: RBAC_API_GROUP.to_string(),
        kind: R::KIND.to_string(),
        name: name.to_string(),
    }
}

/// A `PolicyRule` granting `verbs` on `resources` in `groups`.
pub(crate) fn rule(groups: &[&str], resources: &[&str], verbs: &[&str]) -> PolicyRule {
    PolicyRule {
        api_groups: Some(strings(groups)),
        resources: Some(strings(resources)),
        verbs: strings(verbs),
        ..Default::default()
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

/// Build the operator namespace object.
pub(crate) fn build_namespace(name: &str) -> Namespace {
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
pub(crate) fn build_service_account(namespace: &str) -> ServiceAccount {
    ServiceAccount {
        metadata: ObjectMeta {
            name: Some(SERVICE_ACCOUNT_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(RBAC_COMPONENT)),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Build the ClusterRoleBinding that binds the bindy ServiceAccount to `bindy-role`.
///
/// The subject namespace is set to `namespace` so bootstrap works for custom namespaces.
pub(crate) fn build_cluster_role_binding(namespace: &str) -> ClusterRoleBinding {
    ClusterRoleBinding {
        metadata: ObjectMeta {
            name: Some(CLUSTER_ROLE_BINDING_NAME.to_string()),
            ..Default::default()
        },
        role_ref: role_ref::<ClusterRole>(OPERATOR_ROLE_NAME),
        subjects: Some(vec![sa_subject(SERVICE_ACCOUNT_NAME, namespace)]),
    }
}

/// Build the namespaced Role granting the operator the mutating verbs on Secrets.
///
/// B-5 hardening: cluster-wide Secret access in the operator ClusterRole is
/// read-only; `create`/`update`/`patch`/`delete` are confined to this namespaced
/// Role, bound only in the operator namespace. This prevents a compromised
/// operator from creating, modifying, or deleting Secrets in other namespaces.
pub(crate) fn build_secrets_writer_role(namespace: &str) -> Role {
    Role {
        metadata: ObjectMeta {
            name: Some(SECRETS_WRITER_ROLE_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(RBAC_COMPONENT)),
            ..Default::default()
        },
        rules: Some(vec![rule(
            &[CORE_API_GROUP],
            &["secrets"],
            &["create", "update", "patch", "delete"],
        )]),
    }
}

/// Build the RoleBinding that binds the operator ServiceAccount to the namespaced
/// `bindy-secrets-writer` Role.
pub(crate) fn build_secrets_writer_role_binding(namespace: &str) -> RoleBinding {
    RoleBinding {
        metadata: ObjectMeta {
            name: Some(SECRETS_WRITER_ROLE_BINDING_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(RBAC_COMPONENT)),
            ..Default::default()
        },
        role_ref: role_ref::<Role>(SECRETS_WRITER_ROLE_NAME),
        subjects: Some(vec![sa_subject(SERVICE_ACCOUNT_NAME, namespace)]),
    }
}

/// Build the operator Deployment manifest.
///
/// The container image defaults to `ghcr.io/firestoned/bindy:<image_tag>`.
/// Pass `registry` to override the registry/org prefix for air-gapped environments.
pub(crate) fn build_deployment(
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
                            // Downward API: the operator derives BIND_ALLOWED_SERVICE_ACCOUNTS
                            // for operand bindcar sidecars from its own namespace.
                            {"name": POD_NAMESPACE_ENV, "valueFrom": {"fieldRef": {"fieldPath": "metadata.namespace"}}},
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
                        "volumeMounts": [
                            {"name": "tmp", "mountPath": "/tmp"},
                            // bindcar 0.7.0 (Mode B / TokenReview) bearer token,
                            // read from /var/run/secrets/bindcar/token.
                            {
                                "name": BINDCAR_TOKEN_VOLUME_NAME,
                                "mountPath": BINDCAR_TOKEN_MOUNT_PATH,
                                "readOnly": true
                            }
                        ]
                    }],
                    "volumes": [
                        {"name": "tmp", "emptyDir": {}},
                        // Short-lived SA token minted with the `bindcar` audience so
                        // bindcar 0.7.0 accepts it (default BIND_TOKEN_AUDIENCES is `bindcar`).
                        {
                            "name": BINDCAR_TOKEN_VOLUME_NAME,
                            "projected": {
                                "sources": [{
                                    "serviceAccountToken": {
                                        "audience": crate::constants::BINDCAR_TOKEN_AUDIENCE,
                                        "expirationSeconds": BINDCAR_TOKEN_EXPIRATION_SECONDS,
                                        "path": BINDCAR_TOKEN_FILENAME
                                    }
                                }]
                            }
                        }
                    ]
                }
            }
        }
    });
    serde_json::from_value(value).context("Failed to build operator Deployment")
}

/// Parse a ClusterRole from embedded YAML.
pub(crate) fn parse_cluster_role(yaml: &str) -> Result<ClusterRole> {
    serde_saphyr::from_str(yaml).context("Failed to parse ClusterRole YAML")
}

/// Parse a ClusterRoleBinding from embedded YAML.
pub(crate) fn parse_cluster_role_binding(yaml: &str) -> Result<ClusterRoleBinding> {
    serde_saphyr::from_str(yaml).context("Failed to parse ClusterRoleBinding YAML")
}

/// Build the TokenReview ClusterRoleBinding for the requested install namespace.
///
/// Parses the embedded manifest ([`BINDCAR_TOKENREVIEW_CLUSTER_ROLE_BINDING_YAML`])
/// and rewrites every subject's namespace to `namespace`. The static manifest
/// hardcodes `bindy-system`, but `bindy bootstrap operator --namespace foo`
/// creates the operand `bind9` ServiceAccount in `foo`, so the binding must
/// follow the requested namespace or bindcar's TokenReview calls are denied.
///
/// # Arguments
/// * `namespace` - Namespace the operator (and operand `bind9` SA) is installed into
///
/// # Errors
/// Returns an error if the embedded YAML fails to parse or has no subjects.
pub(crate) fn build_tokenreview_cluster_role_binding(
    namespace: &str,
) -> Result<ClusterRoleBinding> {
    let mut crb = parse_cluster_role_binding(BINDCAR_TOKENREVIEW_CLUSTER_ROLE_BINDING_YAML)?;

    let Some(subjects) = crb.subjects.as_mut() else {
        return Err(anyhow!(
            "Embedded TokenReview ClusterRoleBinding has no subjects"
        ));
    };

    for subject in subjects {
        subject.namespace = Some(namespace.to_string());
    }

    Ok(crb)
}

/// Build a single CRD from a Rust type, ensuring `storage: true` and `served: true`.
///
/// Mirrors the logic in `src/bin/crdgen.rs` so bootstrap and crdgen stay in sync.
///
/// # Errors
/// As [`mark_storage_version`].
pub(crate) fn build_crd<T: CustomResourceExt>() -> Result<CustomResourceDefinition> {
    mark_storage_version(T::crd())
}

/// Mark the first version of `crd` as `served: true` and `storage: true`.
///
/// # Errors
/// Returns an error if `spec.versions` is empty: a CRD with no version cannot be
/// served, so applying it would only fail later on the API server.
pub(crate) fn mark_storage_version(
    mut crd: CustomResourceDefinition,
) -> Result<CustomResourceDefinition> {
    let Some(first) = crd.spec.versions.first_mut() else {
        let name = crd.metadata.name.as_deref().unwrap_or_default();
        return Err(anyhow!(
            "CRD {name} has no spec.versions to mark as served and stored"
        ));
    };
    first.storage = true;
    first.served = true;
    Ok(crd)
}

/// Build every CRD in the same order as `crdgen`: each record kind of the
/// record-kind table (ADR-0020), then the zone and infrastructure CRDs.
pub(crate) fn build_all_crds() -> Result<Vec<CustomResourceDefinition>> {
    let mut crds = Vec::new();
    bindy_api::each_record_kind!(K => crds.push(build_crd::<K>()?));
    crds.push(build_crd::<DNSZone>()?);
    crds.push(build_crd::<Bind9Cluster>()?);
    crds.push(build_crd::<ClusterBind9Provider>()?);
    crds.push(build_crd::<Bind9Instance>()?);
    Ok(crds)
}

// ---------------------------------------------------------------------------
// Scout resource builders
// ---------------------------------------------------------------------------

/// Build the scout ServiceAccount in the given namespace.
pub(crate) fn build_scout_service_account(namespace: &str) -> ServiceAccount {
    ServiceAccount {
        metadata: ObjectMeta {
            name: Some(SCOUT_SERVICE_ACCOUNT_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(SCOUT_COMPONENT)),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Build the scout ClusterRole with cluster-scoped permissions.
///
/// Grants watch/patch/update on Ingresses, Services and Gateway API routes (the
/// kube-rs finalizer patches the main resource metadata to add/remove finalizers),
/// read on Gateways and DNSZones, and get/list on Namespaces (for the namespace
/// selector). There is no Secret rule: the remote-kubeconfig Secret is read through
/// the namespaced, `resourceNames`-restricted Role from
/// [`build_scout_secrets_reader_role`].
pub(crate) fn build_scout_cluster_role() -> ClusterRole {
    const NETWORKING: &str = "networking.k8s.io";
    const GATEWAY: &str = "gateway.networking.k8s.io";
    const FINALIZER_VERBS: &[&str] = &["update"];

    ClusterRole {
        metadata: ObjectMeta {
            name: Some(SCOUT_CLUSTER_ROLE_NAME.to_string()),
            labels: Some(labels(SCOUT_COMPONENT)),
            ..Default::default()
        },
        rules: Some(vec![
            // Watch and mutate Ingresses across all namespaces.
            // kube-rs finalizer::finalizer() patches the main resource metadata to
            // add/remove finalizers, so patch+update on ingresses (not just the
            // ingresses/finalizers subresource) is required.
            rule(&[NETWORKING], &["ingresses"], WATCH_AND_FINALIZE_VERBS),
            // Also grant the finalizers subresource for forward-compatibility.
            rule(&[NETWORKING], &["ingresses/finalizers"], FINALIZER_VERBS),
            // Watch LoadBalancer Services for external IP to ARecord automation.
            // patch+update required to add/remove the Scout finalizer on the Service metadata.
            rule(&[CORE_API_GROUP], &["services"], WATCH_AND_FINALIZE_VERBS),
            // services/finalizers subresource for forward-compatibility.
            rule(&[CORE_API_GROUP], &["services/finalizers"], FINALIZER_VERBS),
            // Watch HTTPRoutes, TLSRoutes, and TCPRoutes from the Gateway API
            // (gateway.networking.k8s.io) to automate A record creation for Gateway
            // routes with opt-in annotations. Gateways are read to follow a route's
            // parentRefs back to the serving gateway and discover its external IP.
            // patch+update required to add/remove the Scout finalizer on route metadata.
            rule(
                &[GATEWAY],
                &["httproutes", "tlsroutes", "tcproutes"],
                WATCH_AND_FINALIZE_VERBS,
            ),
            // route/finalizers subresource for forward-compatibility.
            rule(
                &[GATEWAY],
                &[
                    "httproutes/finalizers",
                    "tlsroutes/finalizers",
                    "tcproutes/finalizers",
                ],
                FINALIZER_VERBS,
            ),
            // Gateways are read-only: Scout reads status.addresses to discover external IPs.
            rule(&[GATEWAY], &["gateways"], READ_VERBS),
            // Read DNSZones for zone validation
            rule(&[BINDY_API_GROUP], &["dnszones"], READ_VERBS),
            // NOTE: there is deliberately no cluster-wide Secret rule here. Reading the
            // remote-cluster kubeconfig Secret (Phase 2 only) is granted by a namespaced,
            // resourceNames-restricted Role/RoleBinding instead (see
            // `build_scout_secrets_reader_role`), applied only when `--remote-secret` is
            // configured. This closes the previously unscoped `secrets: get` grant that
            // applied to every Secret in every namespace.
            // Read Namespace labels for --namespace-selector / BINDY_SCOUT_NAMESPACE_SELECTOR:
            // scout checks each source object's namespace against the configured selector
            // before acting, via a List scoped to that single namespace by name. Namespace
            // metadata (name/labels) is low-sensitivity, unlike Secret contents.
            rule(&[CORE_API_GROUP], &["namespaces"], &["get", "list"]),
        ]),
        ..Default::default()
    }
}

/// Build the ClusterRoleBinding that binds the scout ServiceAccount to the scout ClusterRole.
///
/// The subject namespace is set to `namespace` so bootstrap works for custom namespaces.
pub(crate) fn build_scout_cluster_role_binding(namespace: &str) -> ClusterRoleBinding {
    ClusterRoleBinding {
        metadata: ObjectMeta {
            name: Some(SCOUT_CLUSTER_ROLE_BINDING_NAME.to_string()),
            labels: Some(labels(SCOUT_COMPONENT)),
            ..Default::default()
        },
        role_ref: role_ref::<ClusterRole>(SCOUT_CLUSTER_ROLE_NAME),
        subjects: Some(vec![sa_subject(SCOUT_SERVICE_ACCOUNT_NAME, namespace)]),
    }
}

/// Build the scout writer Role (namespaced ARecord write permissions).
pub(crate) fn build_scout_writer_role(namespace: &str) -> Role {
    Role {
        metadata: ObjectMeta {
            name: Some(SCOUT_WRITER_ROLE_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(SCOUT_COMPONENT)),
            ..Default::default()
        },
        rules: Some(vec![rule(&[BINDY_API_GROUP], &["arecords"], CRUD_VERBS)]),
    }
}

/// Build the scout writer RoleBinding (binds scout SA to writer Role).
pub(crate) fn build_scout_writer_role_binding(namespace: &str) -> RoleBinding {
    RoleBinding {
        metadata: ObjectMeta {
            name: Some(SCOUT_WRITER_ROLE_BINDING_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(SCOUT_COMPONENT)),
            ..Default::default()
        },
        role_ref: role_ref::<Role>(SCOUT_WRITER_ROLE_NAME),
        subjects: Some(vec![sa_subject(SCOUT_SERVICE_ACCOUNT_NAME, namespace)]),
    }
}

/// Build the namespaced Role granting Scout read access to the single remote-cluster
/// kubeconfig Secret (Phase 2 / multi-cluster mode only).
///
/// Scoped two ways: namespaced (not a ClusterRole) and `resourceNames`-restricted to
/// `secret_name`, so this grants `get` on exactly one Secret object, not "every Secret
/// in this namespace" and certainly not "every Secret in the cluster" (the previous,
/// now-removed cluster-wide `bindy-scout` ClusterRole behavior).
pub(crate) fn build_scout_secrets_reader_role(namespace: &str, secret_name: &str) -> Role {
    Role {
        metadata: ObjectMeta {
            name: Some(SCOUT_SECRETS_READER_ROLE_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(SCOUT_COMPONENT)),
            ..Default::default()
        },
        rules: Some(vec![PolicyRule {
            resource_names: Some(vec![secret_name.to_string()]),
            ..rule(&[CORE_API_GROUP], &["secrets"], &["get"])
        }]),
    }
}

/// Build the RoleBinding pairing [`build_scout_secrets_reader_role`] with the Scout
/// ServiceAccount.
pub(crate) fn build_scout_secrets_reader_role_binding(namespace: &str) -> RoleBinding {
    RoleBinding {
        metadata: ObjectMeta {
            name: Some(SCOUT_SECRETS_READER_ROLE_BINDING_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(SCOUT_COMPONENT)),
            ..Default::default()
        },
        role_ref: role_ref::<Role>(SCOUT_SECRETS_READER_ROLE_NAME),
        subjects: Some(vec![sa_subject(SCOUT_SERVICE_ACCOUNT_NAME, namespace)]),
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
/// `bindy.firestoned.io/remote-kubeconfig` Secret manifest to `output`: by default a
/// file readable only by its owner (mode 0600), because the Secret carries the
/// ServiceAccount token. Stdout is used only when the caller asks for it (`-`).
///
/// Apply the file to the child (workload) cluster where scout runs, then delete it:
///
/// ```text
/// kubectl --context=<child-cluster> apply -f <service-account>-kubeconfig.yaml
/// ```
///
/// Then configure the scout Deployment with:
/// ```text
/// BINDY_SCOUT_REMOTE_SECRET=<service-account>-kubeconfig
/// ```
///
/// # Arguments
/// * `namespace` - Namespace on the queen-ship where the SA and Role are created
/// * `service_account` - Name of the ServiceAccount to create
/// * `server_override` - Optional API server URL to use in the kubeconfig instead of the
///   address from KUBECONFIG. Required when the KUBECONFIG address is not reachable from
///   inside the child cluster (e.g. `https://172.18.0.3:6443` for kind-to-kind).
/// * `allow_insecure` - Opt in to emitting `insecure-skip-tls-verify: true` when the
///   KUBECONFIG lacks `certificate-authority-data`. Defaults to `false`; the command
///   refuses rather than silently distributing MITM-susceptible kubeconfigs.
/// * `output` - The file the Secret manifest is written to ([`mc_output_path`]).
///
/// # Errors
/// Returns error if KUBECONFIG is unreadable, the Kubernetes API calls fail, a
/// CA bundle is missing and `allow_insecure` is `false`, or the output file
/// cannot be written.
pub async fn run_bootstrap_multi_cluster(
    namespace: &str,
    service_account: &str,
    server_override: Option<&str>,
    allow_insecure: bool,
    output: &std::path::Path,
) -> Result<()> {
    let client = Client::try_default()
        .await
        .context("Failed to connect to Kubernetes cluster — is KUBECONFIG set?")?;

    let cluster = read_cluster_info()?;
    let server = resolve_server(server_override, &cluster)?;
    if server_override.is_some() {
        let had = cluster.server.as_deref().unwrap_or("no server");
        eprintln!("ℹ Using server override: {server} (KUBECONFIG had: {had})");
    }

    let sa = service_account;
    let fm = MC_FIELD_MANAGER;
    let service_account_obj = build_mc_service_account(namespace, sa);
    apply(
        &Api::namespaced(client.clone(), namespace),
        &service_account_obj,
        fm,
    )
    .await?;
    eprintln!("✓ ServiceAccount: {sa} (namespace: {namespace})");
    let role = build_mc_writer_role(namespace, sa);
    apply(&Api::namespaced(client.clone(), namespace), &role, fm).await?;
    eprintln!("✓ Role: {sa} (namespace: {namespace})");
    let rb = build_mc_writer_role_binding(namespace, sa);
    apply(&Api::namespaced(client.clone(), namespace), &rb, fm).await?;
    eprintln!("✓ RoleBinding: {sa} (namespace: {namespace})");

    let token_secret_name = format!("{sa}{SA_TOKEN_SECRET_SUFFIX}");
    let token_secret = build_mc_sa_token_secret(namespace, sa);
    apply(
        &Api::namespaced(client.clone(), namespace),
        &token_secret,
        fm,
    )
    .await?;
    eprintln!("✓ Secret: {token_secret_name} (namespace: {namespace})");
    eprintln!("⏳ Waiting for SA token to be populated...");
    let token = wait_for_sa_token(&client, namespace, &token_secret_name).await?;

    let kubeconfig_yaml = build_kubeconfig_yaml(
        &cluster.name,
        &server,
        cluster.ca_data_b64.as_deref(),
        service_account,
        &token,
        allow_insecure,
    )?;

    let secret = build_mc_kubeconfig_secret(namespace, service_account, &kubeconfig_yaml);
    let secret_name = format!("{service_account}{REMOTE_KUBECONFIG_SECRET_SUFFIX}");
    let secret_yaml =
        bindy_api::yaml::to_string(&secret).context("Failed to serialize kubeconfig Secret")?;

    write_private_file(output, &format!("---\n{secret_yaml}"))?;
    let shown = output.display();
    eprintln!("\n✓ Wrote the kubeconfig Secret to {shown} (mode 0600; it holds the token)");
    eprintln!("Apply it to each child cluster, then delete it:");
    eprintln!("    kubectl --context=<child-cluster> apply -f {shown}");
    eprintln!("    rm {shown}");
    eprintln!("Then set BINDY_SCOUT_REMOTE_SECRET={secret_name} on the scout Deployment.");

    Ok(())
}

/// The `--output` value that conventionally means standard output, refused here.
const MC_OUTPUT_STDOUT: &str = "-";

/// Owner read/write only: the kubeconfig Secret file carries a token.
const PRIVATE_FILE_MODE: u32 = 0o600;

/// Resolve `--output` for `bindy bootstrap mc` to the file to write.
///
/// # Arguments
/// * `output` - The `--output` value, if given
/// * `service_account` - The ServiceAccount the kubeconfig is for
///
/// # Returns
/// The given path, or by default `<service-account>-kubeconfig.yaml` in the
/// current directory.
///
/// # Errors
/// Returns an error for `-`: the Secret carries the ServiceAccount token and
/// is never printed, so it cannot end up in scrollback, a CI log or a session
/// recording.
pub fn mc_output_path(output: Option<&str>, service_account: &str) -> Result<std::path::PathBuf> {
    match output {
        Some(MC_OUTPUT_STDOUT) => Err(anyhow!(
            "refusing to print the kubeconfig Secret: it carries the ServiceAccount token; \
             pass --output <file> (written with mode 0600)"
        )),
        Some(path) => Ok(std::path::PathBuf::from(path)),
        None => Ok(std::path::PathBuf::from(format!(
            "{service_account}{REMOTE_KUBECONFIG_SECRET_SUFFIX}.yaml"
        ))),
    }
}

/// Write `contents` to `path` so only its owner can read it (mode 0600).
///
/// A new file is created 0600. An existing file is first restricted to 0600
/// and then truncated, so a token is never written into a file that others
/// can read.
///
/// # Errors
/// Returns an error if the file cannot be opened, restricted or written.
pub(crate) fn write_private_file(path: &std::path::Path, contents: &str) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(PRIVATE_FILE_MODE)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    file.set_permissions(std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .with_context(|| format!("Failed to restrict {} to mode 0600", path.display()))?;
    file.set_len(0)
        .with_context(|| format!("Failed to truncate {}", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("Failed to write {}", path.display()))
}

/// Build a kubeconfig YAML string for the given service account token.
///
/// When `ca_data_b64` is `Some(...)` the CA data is embedded and TLS
/// verification is enforced. When `ca_data_b64` is `None` the function
/// refuses to produce output unless `allow_insecure` is `true`; in that case
/// it sets `insecure-skip-tls-verify: true` on the cluster entry.
///
/// Refusing insecure output by default prevents a bootstrap run against a
/// KUBECONFIG that lacks CA data from silently distributing kubeconfigs that
/// skip TLS verification (MITM risk against the child-cluster scout).
///
/// # Arguments
/// * `cluster_name` - Name of the cluster entry in the kubeconfig
/// * `server` - Kubernetes API server URL (e.g. `https://192.0.2.1:6443`)
/// * `ca_data_b64` - Base64-encoded PEM CA certificate, or `None` to skip TLS verify
/// * `sa_name` - Name of the service account / kubeconfig user entry
/// * `token` - Bearer token for the service account
/// * `allow_insecure` - Must be `true` to allow emitting `insecure-skip-tls-verify`
///   when `ca_data_b64` is `None`. Intended to be wired to an explicit CLI
///   opt-out flag (e.g. `--insecure-skip-tls-verify`).
///
/// # Errors
/// - Returns an error if `ca_data_b64` is `None` and `allow_insecure` is `false`.
/// - Returns an error if YAML serialization fails.
pub(crate) fn build_kubeconfig_yaml(
    cluster_name: &str,
    server: &str,
    ca_data_b64: Option<&str>,
    sa_name: &str,
    token: &str,
    allow_insecure: bool,
) -> Result<String> {
    if ca_data_b64.is_none() && !allow_insecure {
        return Err(anyhow!(
            "refusing to build kubeconfig for {server}: KUBECONFIG has no \
             certificate-authority-data and --insecure-skip-tls-verify was not set. \
             Provide a CA bundle or re-run with the explicit insecure opt-out."
        ));
    }

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
    bindy_api::yaml::to_string(&cfg).context("Failed to serialize kubeconfig YAML")
}

/// Build the multi-cluster ServiceAccount on the queen-ship cluster.
pub(crate) fn build_mc_service_account(namespace: &str, sa_name: &str) -> ServiceAccount {
    ServiceAccount {
        metadata: ObjectMeta {
            name: Some(sa_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(MC_COMPONENT_LABEL)),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Build the namespaced Role for the multi-cluster service account on the queen-ship.
///
/// Grants:
/// - Full CRUD on `arecords`: scout creates/deletes ARecords via the remote client
/// - Read-only on `dnszones`: scout validates zones before creating ARecords
///
/// Both resources live in the same target namespace on the queen-ship cluster, so a
/// namespaced `Role` is sufficient.  The scout watches DNSZones via
/// `Api::namespaced(remote_client, target_namespace)` (not `Api::all`), which means no
/// cluster-scoped `ClusterRole` is required.
///
/// The Role name matches the service account name, mirroring the convention in
/// `deploy/scout/remote-cluster-rbac.yaml`.
pub(crate) fn build_mc_writer_role(namespace: &str, sa_name: &str) -> Role {
    Role {
        metadata: ObjectMeta {
            name: Some(sa_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(MC_COMPONENT_LABEL)),
            ..Default::default()
        },
        rules: Some(vec![
            rule(&[BINDY_API_GROUP], &["arecords"], CRUD_VERBS),
            rule(&[BINDY_API_GROUP], &["dnszones"], READ_VERBS),
        ]),
    }
}

/// Build the RoleBinding that binds the multi-cluster SA to its Role on the queen-ship.
///
/// The RoleBinding name matches the service account name, mirroring the convention in
/// `deploy/scout/remote-cluster-rbac.yaml`.
pub(crate) fn build_mc_writer_role_binding(namespace: &str, sa_name: &str) -> RoleBinding {
    RoleBinding {
        metadata: ObjectMeta {
            name: Some(sa_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(MC_COMPONENT_LABEL)),
            ..Default::default()
        },
        role_ref: role_ref::<Role>(sa_name),
        subjects: Some(vec![sa_subject(sa_name, namespace)]),
    }
}

/// Build the `kubernetes.io/service-account-token` Secret that triggers token generation.
///
/// After this Secret is applied, the Kubernetes token controller populates `data.token`
/// with a long-lived bearer token for the specified service account.
pub(crate) fn build_mc_sa_token_secret(namespace: &str, sa_name: &str) -> Secret {
    let annotations = BTreeMap::from([(
        "kubernetes.io/service-account.name".to_string(),
        sa_name.to_string(),
    )]);

    Secret {
        metadata: ObjectMeta {
            name: Some(format!("{sa_name}{SA_TOKEN_SECRET_SUFFIX}")),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(MC_COMPONENT_LABEL)),
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
pub(crate) fn build_mc_kubeconfig_secret(
    namespace: &str,
    sa_name: &str,
    kubeconfig_yaml: &str,
) -> Secret {
    let data = BTreeMap::from([(
        "kubeconfig".to_string(),
        ByteString(kubeconfig_yaml.as_bytes().to_vec()),
    )]);
    let mut secret_labels = labels(MC_COMPONENT_LABEL);
    secret_labels.insert(
        "bindy.firestoned.io/service-account".to_string(),
        sa_name.to_string(),
    );

    Secret {
        metadata: ObjectMeta {
            name: Some(format!("{sa_name}{REMOTE_KUBECONFIG_SECRET_SUFFIX}")),
            namespace: Some(namespace.to_string()),
            labels: Some(secret_labels),
            ..Default::default()
        },
        type_: Some(REMOTE_KUBECONFIG_SECRET_TYPE.to_string()),
        data: Some(data),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Multi-cluster token wait and KUBECONFIG reading
// ---------------------------------------------------------------------------

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

/// The cluster entry of the current KUBECONFIG context.
pub(crate) struct KubeconfigCluster {
    /// The cluster entry's name.
    pub(crate) name: String,
    /// The API server URL, if the entry has one.
    pub(crate) server: Option<String>,
    /// Base64-encoded PEM CA bundle: the inline data, or the CA file's content.
    /// `None` when neither is configured.
    pub(crate) ca_data_b64: Option<String>,
}

/// Read the current KUBECONFIG context's cluster entry ([`cluster_info`]).
///
/// # Errors
/// Returns an error if KUBECONFIG cannot be read, or as [`cluster_info`].
fn read_cluster_info() -> Result<KubeconfigCluster> {
    let raw = Kubeconfig::read().context(
        "Failed to read KUBECONFIG — ensure KUBECONFIG env var is set or ~/.kube/config exists",
    )?;
    cluster_info(&raw)
}

/// The cluster entry of `raw`'s current context: its name, server URL and CA bundle.
///
/// An inline `certificate-authority-data` wins over a `certificate-authority` file.
///
/// # Errors
/// Returns an error if the current context or its cluster is missing or empty, or
/// the CA file cannot be read.
pub(crate) fn cluster_info(raw: &Kubeconfig) -> Result<KubeconfigCluster> {
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

    Ok(KubeconfigCluster {
        name: cluster_name,
        server: cluster.server.clone(),
        ca_data_b64: ca_data,
    })
}

/// The API server URL to write into the child cluster's kubeconfig.
///
/// `--server` wins; otherwise the KUBECONFIG cluster's own server URL.
///
/// # Errors
/// Returns an error when there is neither: there is no in-cluster default that a
/// child cluster could reach.
pub(crate) fn resolve_server(
    server_override: Option<&str>,
    cluster: &KubeconfigCluster,
) -> Result<String> {
    if let Some(server) = server_override {
        return Ok(server.to_string());
    }
    cluster.server.clone().ok_or_else(|| {
        anyhow!(
            "KUBECONFIG cluster '{}' has no server URL; pass --server <url> with an API \
             server address reachable from the child cluster",
            cluster.name
        )
    })
}

/// Build the scout Deployment manifest.
///
/// The container image defaults to `ghcr.io/firestoned/bindy:<image_tag>`.
/// Pass `registry` to override the registry/org prefix for air-gapped environments.
///
/// Scout CLI args (`--cluster-name`, `--default-ips`, `--default-zone`) are passed
/// directly to the container command so the scout behaves consistently with `bindy scout`.
/// [`SCOUT_TARGET_NAMESPACE_ENV`] is set to `namespace`, so the scout writes
/// `ARecord`s into the namespace where bootstrap created its writer Role.
pub(crate) fn build_scout_deployment(
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
        // ARecords go where the writer Role was created (see run_bootstrap_scout).
        serde_json::json!({"name": SCOUT_TARGET_NAMESPACE_ENV, "value": namespace}),
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
    delete_mc_resource::<RoleBinding>(&client, namespace, service_account).await?;
    delete_mc_resource::<Role>(&client, namespace, service_account).await?;

    let kubeconfig_secret = format!("{service_account}{REMOTE_KUBECONFIG_SECRET_SUFFIX}");
    delete_mc_resource::<Secret>(&client, namespace, &kubeconfig_secret).await?;

    let token_secret = format!("{service_account}{SA_TOKEN_SECRET_SUFFIX}");
    delete_mc_resource::<Secret>(&client, namespace, &token_secret).await?;

    delete_mc_resource::<ServiceAccount>(&client, namespace, service_account).await?;

    eprintln!("\n✓ Revoked multi-cluster access for: {service_account} (namespace: {namespace})");
    Ok(())
}

/// Delete one namespaced object that `bootstrap mc` created, for revoke.
///
/// Reports the outcome on stderr: `✓ Deleted <Kind>: <name> (namespace: <ns>)`
/// when the delete was accepted, `  <Kind>/<name> not found, skipping` when the
/// object is already gone (HTTP 404).
///
/// # Arguments
/// * `client` - Client for the queen-ship cluster
/// * `namespace` - Namespace the object lives in
/// * `name` - The object's name
///
/// # Errors
/// Returns `Failed to delete <Kind>/<name>` for any API failure other than
/// 404; the `kube::Error` stays in the chain.
pub(crate) async fn delete_mc_resource<K>(
    client: &Client,
    namespace: &str,
    name: &str,
) -> Result<()>
where
    K: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + Clone
        + Debug
        + DeserializeOwned,
{
    let kind = K::kind(&());
    let api: Api<K> = Api::namespaced(client.clone(), namespace);
    let deleted = delete_if_present(&api, name)
        .await
        .with_context(|| format!("Failed to delete {kind}/{name}"))?;
    if !deleted {
        eprintln!("  {kind}/{name} not found, skipping");
        return Ok(());
    }
    eprintln!("✓ Deleted {kind}: {name} (namespace: {namespace})");
    Ok(())
}
