// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The `bindy` command line: subcommands, flags, and the dispatch for the
//! subcommands that are not the operator (`bootstrap`, `scout`). `bindy run`
//! is wired in `main.rs`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use tracing::info;

const BANNER: &str = "

        ()           ()
         \\\\    ^    //
       ~~~~\\\\~^^^~//~~~~~~
     /     (  o o  )      \\
    /   ___/  ~~~  \\___    \\
    | /    \\   v   /    \\  |
    |/   __/|=====|\\__   \\ |
    |   /   |=====|   \\   ||
    |  /    |=====|    \\  |/
    \\ /     |=====|     \\ /
            |=====|
             \\===/
              \\=/
               *
";

/// BIND9 DNS Operator for Kubernetes
#[derive(Parser)]
#[command(
    name = "bindy",
    about = "Bindy - BIND9 DNS Operator for Kubernetes",
    version,
    before_help = BANNER,
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum BootstrapCommands {
    /// Bootstrap the BIND9 operator: apply namespace, CRDs, RBAC, and deploy the operator
    Operator {
        /// Namespace to install bindy into
        #[arg(long, default_value = bindy_bootstrap::DEFAULT_NAMESPACE)]
        namespace: String,
        /// Print what would be applied without connecting to a cluster
        #[arg(long)]
        dry_run: bool,
        /// Version (image tag) for the operator Deployment.
        /// Defaults to the binary's own version in release builds (e.g. "v0.5.0"),
        /// or "latest" in debug builds.
        #[arg(long, default_value = bindy_bootstrap::DEFAULT_IMAGE_TAG)]
        version: String,
        /// Override the container registry used for the operator image.
        /// Useful for air-gapped environments where images are mirrored to a private registry.
        /// When set, the image becomes `<registry>/bindy:<version>` instead of
        /// `ghcr.io/firestoned/bindy:<version>`.
        #[arg(long)]
        registry: Option<String>,
    },
    /// Bootstrap the Scout controller: apply RBAC and deploy scout
    Scout {
        /// Namespace to install scout into
        #[arg(long, default_value = bindy_bootstrap::DEFAULT_NAMESPACE)]
        namespace: String,
        /// Print what would be applied without connecting to a cluster
        #[arg(long)]
        dry_run: bool,
        /// Version (image tag) for the Scout Deployment.
        /// Defaults to the binary's own version in release builds (e.g. "v0.5.0"),
        /// or "latest" in debug builds.
        #[arg(long, default_value = bindy_bootstrap::DEFAULT_IMAGE_TAG)]
        version: String,
        /// Override the container registry used for the scout image.
        /// Useful for air-gapped environments where images are mirrored to a private registry.
        /// When set, the image becomes `<registry>/bindy:<version>` instead of
        /// `ghcr.io/firestoned/bindy:<version>`.
        #[arg(long)]
        registry: Option<String>,
        /// Logical name of this cluster stamped on created ARecord labels.
        /// Passed to the scout container as `--cluster-name`.
        /// Overrides the BINDY_SCOUT_CLUSTER_NAME environment variable inside the pod.
        #[arg(long, default_value = bindy_bootstrap::DEFAULT_SCOUT_CLUSTER_NAME)]
        cluster_name: String,
        /// Default IP addresses for all Ingresses when no per-Ingress annotation override
        /// or LoadBalancer status IP is available. Accepts one or more comma-separated values.
        /// Passed to the scout container as `--default-ips`.
        #[arg(long, value_delimiter = ',')]
        default_ips: Vec<String>,
        /// Default DNS zone applied to all Ingresses when no annotation is present.
        /// Passed to the scout container as `--default-zone`.
        #[arg(long)]
        default_zone: Option<String>,
        /// Name of the Secret containing the remote cluster kubeconfig (Phase 2 / multi-cluster).
        /// The Secret must already exist in the same namespace as the scout (created by
        /// `bindy bootstrap multi-cluster`). When set, the scout connects to the remote bindy
        /// cluster to look up DNSZones and write ARecords.
        /// Sets `BINDY_SCOUT_REMOTE_SECRET` in the scout Deployment.
        #[arg(long)]
        remote_secret: Option<String>,
    },
    /// Bootstrap multi-cluster access: create a service account on the queen-ship
    /// and write a `bindy.firestoned.io/remote-kubeconfig` Secret YAML to stdout.
    ///
    /// Run this command against the queen-ship (bindy operator) cluster. Pipe the
    /// output to each child cluster so the scout can connect back to the queen-ship.
    ///
    /// To remove access later: `bindy bootstrap mc --revoke --service-account <name>`
    #[command(alias = "mc")]
    MultiCluster {
        /// Namespace on the queen-ship where the SA, Role, and token Secret are created.
        /// This should be the same namespace the scout is configured to write ARecords into.
        #[arg(long, default_value = bindy_bootstrap::DEFAULT_NAMESPACE)]
        namespace: String,
        /// Name of the service account to create on the queen-ship cluster.
        /// Use one account per child cluster so access can be revoked independently.
        #[arg(long, default_value = bindy_bootstrap::MC_DEFAULT_SERVICE_ACCOUNT_NAME)]
        service_account: String,
        /// Override the API server URL written into the generated kubeconfig Secret.
        /// Required when the KUBECONFIG server address is not reachable from inside
        /// the child cluster (e.g. kind-to-kind: use the queen-ship container's
        /// Docker network address instead of 127.0.0.1).
        /// Example: --server https://172.18.0.3:6443
        #[arg(long)]
        server: Option<String>,
        /// Revoke (delete) all resources previously created for --service-account.
        /// Safe to run multiple times — missing resources are silently skipped.
        /// Alias: --delete
        #[arg(long, alias = "delete", default_value_t = false)]
        revoke: bool,
        /// Emit `insecure-skip-tls-verify: true` in the generated kubeconfig when the
        /// source KUBECONFIG has no `certificate-authority-data`. Without this flag the
        /// command refuses to proceed rather than silently distributing kubeconfigs that
        /// disable TLS verification to every child cluster. Only use for local testing
        /// (e.g. kind) where MITM is not a concern.
        #[arg(long, default_value_t = false)]
        insecure_skip_tls_verify: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Bootstrap bindy components into the cluster
    Bootstrap {
        #[command(subcommand)]
        subcommand: BootstrapCommands,
    },
    /// Run the BIND9 DNS operator
    Run,
    /// Run the ingress scout controller (creates ARecords from annotated Ingresses)
    Scout {
        /// Logical name of this cluster stamped on created ARecord labels.
        /// Overrides the BINDY_SCOUT_CLUSTER_NAME environment variable.
        #[arg(long)]
        cluster_name: Option<String>,
        /// Namespace where ARecords are created.
        /// Overrides the BINDY_SCOUT_NAMESPACE environment variable.
        #[arg(long)]
        namespace: Option<String>,
        /// Default IP addresses used for all Ingresses when no per-Ingress annotation override
        /// or LoadBalancer status IP is available. Accepts one or more comma-separated values.
        /// Overrides the BINDY_SCOUT_DEFAULT_IPS environment variable.
        /// Useful for shared-ingress topologies (e.g. Traefik) where all Ingresses resolve
        /// to the same IP(s).
        #[arg(long, value_delimiter = ',')]
        default_ips: Vec<String>,
        /// Map a gatewayClass to the LoadBalancer Service whose external IP backs it,
        /// as `class=namespace/name` (repeatable). When an HTTPRoute/TLSRoute/TCPRoute has no IP
        /// annotation, scout follows its parentRefs to a Gateway of a configured class
        /// and uses the mapped Service's external IP. Overrides BINDY_SCOUT_GATEWAY_SERVICES.
        /// Example: --gateway-service traefik=traefik/traefik
        #[arg(long = "gateway-service")]
        gateway_service: Vec<String>,
        /// Default DNS zone applied to all Ingresses when no bindy.firestoned.io/zone annotation
        /// is present. Overrides the BINDY_SCOUT_DEFAULT_ZONE environment variable.
        /// When combined with --default-ips, Ingresses only need: bindy.firestoned.io/scout-enabled: "true"
        #[arg(long)]
        default_zone: Option<String>,
        /// Kubernetes label selector (e.g. "bindy.firestoned.io/scout-enabled=true") restricting
        /// which namespaces scout will act in. A source object's per-resource opt-in annotation
        /// is still required in addition to this — the namespace must match the selector AND the
        /// object must carry the annotation. When unset, every namespace is eligible (unchanged
        /// default, for backward compatibility) — production deployments are strongly encouraged
        /// to set this rather than run with cluster-wide scope. Overrides the
        /// BINDY_SCOUT_NAMESPACE_SELECTOR environment variable.
        #[arg(long = "namespace-selector")]
        namespace_selector: Option<String>,
        /// Bindy cluster API URL for the endpoint remote mode (ADR-0008): a
        /// Linkerd-mirrored meshed proxy, a konnectivity endpoint, or the API
        /// server itself. Requires --remote-token-file. Mutually exclusive with
        /// BINDY_SCOUT_REMOTE_SECRET. Overrides BINDY_SCOUT_REMOTE_ENDPOINT.
        #[arg(long = "remote-endpoint")]
        remote_endpoint: Option<String>,
        /// Path to a bearer-token file minted by the bindy cluster, re-read on
        /// rotation. Overrides BINDY_SCOUT_REMOTE_TOKEN_FILE.
        #[arg(long = "remote-token-file")]
        remote_token_file: Option<String>,
        /// Path to the remote endpoint's CA bundle (PEM). When unset, webpki
        /// public roots are used. Overrides BINDY_SCOUT_REMOTE_CA_FILE.
        #[arg(long = "remote-ca-file")]
        remote_ca_file: Option<String>,
    },
    /// Output shell completion code for the specified shell
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Print the binary version (same output as --version)
    Version,
}

impl Commands {
    /// Name for the Tokio worker threads, so logs and thread dumps say which
    /// subcommand is running.
    pub(crate) fn thread_name(&self) -> &'static str {
        match self {
            Self::Bootstrap {
                subcommand: BootstrapCommands::Operator { .. },
            } => "bindy-bootstrap-operator",
            Self::Bootstrap {
                subcommand: BootstrapCommands::Scout { .. },
            } => "bindy-bootstrap-scout",
            Self::Bootstrap {
                subcommand: BootstrapCommands::MultiCluster { .. },
            } => "bindy-bootstrap-mc",
            Self::Run => "bindy-run",
            Self::Scout { .. } => "bindy-scout",
            Self::Completion { .. } | Self::Version => "bindy",
        }
    }
}

/// Run `bindy bootstrap …` or `bindy scout …`.
///
/// # Errors
/// Returns the subcommand's error.
pub(crate) async fn run(command: Commands) -> Result<()> {
    match command {
        Commands::Bootstrap { subcommand } => run_bootstrap(subcommand).await,
        Commands::Scout {
            cluster_name,
            namespace,
            default_ips,
            gateway_service,
            default_zone,
            namespace_selector,
            remote_endpoint,
            remote_token_file,
            remote_ca_file,
        } => {
            // Phase 1 (same-cluster) and Phase 2 (remote cluster) are tracked in
            // `.github/community/12-scout-ingress-controller.md`.
            info!("Starting Scout controller");
            bindy_scout::run_scout(
                cluster_name,
                namespace,
                default_ips,
                gateway_service,
                default_zone,
                namespace_selector,
                bindy_scout::ScoutRemoteOverrides {
                    endpoint: remote_endpoint,
                    token_file: remote_token_file,
                    ca_file: remote_ca_file,
                },
            )
            .await
        }
        Commands::Run | Commands::Completion { .. } | Commands::Version => {
            unreachable!("handled in main")
        }
    }
}

async fn run_bootstrap(subcommand: BootstrapCommands) -> Result<()> {
    match subcommand {
        // Applies namespace, CRDs, RBAC, and the operator Deployment.
        BootstrapCommands::Operator {
            namespace,
            dry_run,
            version,
            registry,
        } => {
            bindy_bootstrap::run_bootstrap_operator(
                &namespace,
                dry_run,
                &version,
                registry.as_deref(),
            )
            .await
        }
        // Applies scout RBAC and the scout Deployment.
        BootstrapCommands::Scout {
            namespace,
            dry_run,
            version,
            registry,
            cluster_name,
            default_ips,
            default_zone,
            remote_secret,
        } => {
            let opts = bindy_bootstrap::ScoutDeploymentOptions {
                image_tag: &version,
                registry: registry.as_deref(),
                cluster_name: &cluster_name,
                default_ips: &default_ips,
                default_zone: default_zone.as_deref(),
                remote_secret: remote_secret.as_deref(),
            };
            bindy_bootstrap::run_bootstrap_scout(&namespace, dry_run, &opts).await
        }
        // Creates or revokes the service account and RBAC on the queen-ship.
        // Without --revoke, writes a `bindy.firestoned.io/remote-kubeconfig`
        // Secret manifest to stdout.
        BootstrapCommands::MultiCluster {
            namespace,
            service_account,
            server,
            revoke,
            insecure_skip_tls_verify,
        } => {
            if revoke {
                bindy_bootstrap::run_revoke_multi_cluster(&namespace, &service_account).await
            } else {
                bindy_bootstrap::run_bootstrap_multi_cluster(
                    &namespace,
                    &service_account,
                    server.as_deref(),
                    insecure_skip_tls_verify,
                )
                .await
            }
        }
    }
}
