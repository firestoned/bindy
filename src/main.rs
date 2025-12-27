// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

use anyhow::Result;
use axum::{routing::get, Router};
use bindy::{
    bind9::Bind9Manager,
    constants::{
        DEFAULT_LEASE_DURATION_SECS, DEFAULT_LEASE_RENEW_DEADLINE_SECS,
        DEFAULT_LEASE_RETRY_PERIOD_SECS, ERROR_REQUEUE_DURATION_SECS, METRICS_SERVER_BIND_ADDRESS,
        METRICS_SERVER_PATH, METRICS_SERVER_PORT, TOKIO_WORKER_THREADS,
    },
    crd::{
        AAAARecord, ARecord, Bind9Cluster, Bind9Instance, CAARecord, CNAMERecord,
        ClusterBind9Provider, DNSZone, MXRecord, NSRecord, SRVRecord, TXTRecord,
    },
    metrics,
    reconcilers::{
        delete_dnszone, reconcile_a_record, reconcile_aaaa_record, reconcile_bind9cluster,
        reconcile_bind9instance, reconcile_caa_record, reconcile_clusterbind9provider,
        reconcile_cname_record, reconcile_dnszone, reconcile_mx_record, reconcile_ns_record,
        reconcile_srv_record, reconcile_txt_record,
    },
};
use futures::StreamExt;
use kube::{
    runtime::{controller::Action, finalizer, watcher::Config, Controller},
    Api, Client, ResourceExt,
};
use kube_lease_manager::{LeaseManager, LeaseManagerBuilder};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct ReconcileError(#[from] anyhow::Error);

fn main() -> Result<()> {
    // Build Tokio runtime with custom thread names
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(TOKIO_WORKER_THREADS)
        .thread_name("bindy-controller")
        .enable_all()
        .build()?;

    runtime.block_on(async_main())
}

/// Initialize logging with custom format
///
/// Respects `RUST_LOG` environment variable if set, otherwise defaults to INFO level.
/// Respects `RUST_LOG_FORMAT` environment variable for output format (json or text).
fn initialize_logging() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let log_format = std::env::var("RUST_LOG_FORMAT").unwrap_or_else(|_| "text".to_string());

    match log_format.to_lowercase().as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_file(true)
                .with_line_number(true)
                .with_thread_names(true)
                .with_target(false)
                .json()
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_file(true)
                .with_line_number(true)
                .with_thread_names(true)
                .with_target(false)
                .with_ansi(true)
                .compact()
                .init();
        }
    }

    info!("Starting BIND9 DNS Controller");
    debug!("Logging initialized with file and line number tracking");
}

/// Initialize Kubernetes client and BIND9 manager
async fn initialize_services() -> Result<(Client, Arc<Bind9Manager>)> {
    debug!("Initializing Kubernetes client");
    let client = Client::try_default().await?;
    debug!("Kubernetes client initialized successfully");

    debug!("Creating BIND9 manager");
    let bind9_manager = Arc::new(Bind9Manager::new());
    debug!("BIND9 manager created");

    Ok((client, bind9_manager))
}

/// Start the Prometheus metrics HTTP server
///
/// Serves metrics on the configured port and path (default: 0.0.0.0:8080/metrics)
///
/// # Returns
/// A `JoinHandle` that can be used to monitor the server task
fn start_metrics_server() -> tokio::task::JoinHandle<()> {
    info!(
        bind_address = METRICS_SERVER_BIND_ADDRESS,
        port = METRICS_SERVER_PORT,
        path = METRICS_SERVER_PATH,
        "Starting Prometheus metrics HTTP server"
    );

    tokio::spawn(async move {
        // Define the metrics endpoint handler
        async fn metrics_handler() -> String {
            match metrics::gather_metrics() {
                Ok(metrics_text) => metrics_text,
                Err(e) => {
                    error!("Failed to gather metrics: {}", e);
                    String::from("# Error gathering metrics\n")
                }
            }
        }

        // Build the router with the metrics endpoint
        let app = Router::new().route(METRICS_SERVER_PATH, get(metrics_handler));

        // Bind to the configured address and port
        let bind_addr = format!("{METRICS_SERVER_BIND_ADDRESS}:{METRICS_SERVER_PORT}");
        let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => listener,
            Err(e) => {
                error!("Failed to bind metrics server to {bind_addr}: {e}");
                return;
            }
        };

        info!("Metrics server listening on http://{bind_addr}{METRICS_SERVER_PATH}");

        // Run the server
        if let Err(e) = axum::serve(listener, app).await {
            error!("Metrics server error: {e}");
        }
    })
}

/// Leader election configuration
struct LeaderElectionConfig {
    enabled: bool,
    lease_name: String,
    lease_namespace: String,
    identity: String,
    lease_duration: u64,
    renew_deadline: u64,
    retry_period: u64,
}

/// Load leader election configuration from environment variables
fn load_leader_election_config() -> LeaderElectionConfig {
    let enabled = std::env::var("BINDY_ENABLE_LEADER_ELECTION")
        .unwrap_or_else(|_| "true".to_string())
        .parse::<bool>()
        .unwrap_or(true);

    let lease_name =
        std::env::var("BINDY_LEASE_NAME").unwrap_or_else(|_| "bindy-leader".to_string());

    let lease_namespace = std::env::var("BINDY_LEASE_NAMESPACE")
        .or_else(|_| std::env::var("POD_NAMESPACE"))
        .unwrap_or_else(|_| "dns-system".to_string());

    let lease_duration = std::env::var("BINDY_LEASE_DURATION_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LEASE_DURATION_SECS);

    let renew_deadline = std::env::var("BINDY_LEASE_RENEW_DEADLINE_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LEASE_RENEW_DEADLINE_SECS);

    let retry_period = std::env::var("BINDY_LEASE_RETRY_PERIOD_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LEASE_RETRY_PERIOD_SECS);

    let identity = std::env::var("POD_NAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| format!("bindy-{}", rand::random::<u32>()));

    LeaderElectionConfig {
        enabled,
        lease_name,
        lease_namespace,
        identity,
        lease_duration,
        renew_deadline,
        retry_period,
    }
}

/// Run all controllers without leader election, with signal handling
async fn run_controllers_without_leader_election(
    client: Client,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    warn!("Leader election DISABLED - running without high availability");
    info!("Starting all controllers with signal handling");

    // Run controllers concurrently with signal handling
    // Controllers should never exit - if one fails, we log it and exit the main process
    let shutdown_result: Result<()> = tokio::select! {
        // Monitor for SIGINT (Ctrl+C)
        result = tokio::signal::ctrl_c() => {
            info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
            info!("Stopping all controllers...");
            result.map_err(anyhow::Error::from)
        }

        // Monitor for SIGTERM (Kubernetes sends this when deleting pods)
        result = async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate())?;
                sigterm.recv().await;
                Ok::<(), anyhow::Error>(())
            }
            #[cfg(not(unix))]
            {
                // On non-Unix platforms, just wait forever
                std::future::pending::<()>().await;
                Ok::<(), anyhow::Error>(())
            }
        } => {
            info!("Received SIGTERM (pod termination), initiating graceful shutdown...");
            info!("Stopping all controllers...");
            result
        }

        result = run_bind9cluster_controller(client.clone()) => {
            error!("CRITICAL: Bind9Cluster controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("Bind9Cluster controller exited unexpectedly without error")
        }
        result = run_clusterbind9provider_controller(client.clone()) => {
            error!("CRITICAL: ClusterBind9Provider controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("ClusterBind9Provider controller exited unexpectedly without error")
        }
        result = run_bind9instance_controller(client.clone()) => {
            error!("CRITICAL: Bind9Instance controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("Bind9Instance controller exited unexpectedly without error")
        }
        result = run_dnszone_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: DNSZone controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("DNSZone controller exited unexpectedly without error")
        }
        result = run_arecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: ARecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("ARecord controller exited unexpectedly without error")
        }
        result = run_aaaarecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: AAAARecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("AAAARecord controller exited unexpectedly without error")
        }
        result = run_txtrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: TXTRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("TXTRecord controller exited unexpectedly without error")
        }
        result = run_cnamerecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: CNAMERecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("CNAMERecord controller exited unexpectedly without error")
        }
        result = run_mxrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: MXRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("MXRecord controller exited unexpectedly without error")
        }
        result = run_nsrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: NSRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("NSRecord controller exited unexpectedly without error")
        }
        result = run_srvrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: SRVRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("SRVRecord controller exited unexpectedly without error")
        }
        result = run_caarecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: CAARecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("CAARecord controller exited unexpectedly without error")
        }
    };

    // Handle shutdown result
    shutdown_result?;
    info!("Graceful shutdown completed successfully");

    Ok(())
}

async fn async_main() -> Result<()> {
    initialize_logging();

    let (client, bind9_manager) = initialize_services().await?;

    // Start the metrics HTTP server
    let _metrics_handle = start_metrics_server();

    let config = load_leader_election_config();

    if config.enabled {
        info!(
            lease_name = %config.lease_name,
            lease_namespace = %config.lease_namespace,
            identity = %config.identity,
            lease_duration_secs = config.lease_duration,
            renew_deadline_secs = config.renew_deadline,
            "Leader election enabled"
        );

        // Create and start lease manager for leader election
        // The manager returns a watch receiver (to monitor leadership status)
        // and a join handle (to monitor the lease renewal task)
        info!("Starting leader election, waiting to acquire leadership...");

        let lease_manager = LeaseManagerBuilder::new(client.clone(), &config.lease_name)
            .with_namespace(&config.lease_namespace)
            .with_identity(&config.identity)
            .with_duration(config.lease_duration)
            .with_grace(config.retry_period)
            .build()
            .await?;

        let (leader_rx, lease_handle) = lease_manager.watch().await;

        // Wait until we become leader
        let mut rx = leader_rx.clone();
        while !*rx.borrow_and_update() {
            rx.changed().await?;
        }

        info!("🎉 Leadership acquired! Starting controllers...");

        // Run controllers with leader election monitoring and signal handling
        run_controllers_with_leader_election(client, bind9_manager, leader_rx, lease_handle)
            .await?;
    } else {
        run_controllers_without_leader_election(client, bind9_manager).await?;
    }

    Ok(())
}

/// Monitor leadership status - returns when leadership is lost or an error occurs
async fn monitor_leadership(
    mut leader_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), anyhow::Error> {
    loop {
        leader_rx.changed().await?;
        if !*leader_rx.borrow() {
            // Leadership lost
            return Ok(());
        }
    }
}

/// Run all DNS record controllers
async fn run_all_controllers(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    tokio::select! {
        result = run_bind9cluster_controller(client.clone()) => {
            error!("CRITICAL: Bind9Cluster controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("Bind9Cluster controller exited unexpectedly without error")
        }
        result = run_clusterbind9provider_controller(client.clone()) => {
            error!("CRITICAL: ClusterBind9Provider controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("ClusterBind9Provider controller exited unexpectedly without error")
        }
        result = run_bind9instance_controller(client.clone()) => {
            error!("CRITICAL: Bind9Instance controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("Bind9Instance controller exited unexpectedly without error")
        }
        result = run_dnszone_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: DNSZone controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("DNSZone controller exited unexpectedly without error")
        }
        result = run_arecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: ARecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("ARecord controller exited unexpectedly without error")
        }
        result = run_aaaarecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: AAAARecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("AAAARecord controller exited unexpectedly without error")
        }
        result = run_txtrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: TXTRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("TXTRecord controller exited unexpectedly without error")
        }
        result = run_cnamerecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: CNAMERecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("CNAMERecord controller exited unexpectedly without error")
        }
        result = run_mxrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: MXRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("MXRecord controller exited unexpectedly without error")
        }
        result = run_nsrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: NSRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("NSRecord controller exited unexpectedly without error")
        }
        result = run_srvrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: SRVRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("SRVRecord controller exited unexpectedly without error")
        }
        result = run_caarecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: CAARecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("CAARecord controller exited unexpectedly without error")
        }
    }
}

/// Run controllers with leader election
///
/// This function runs all controllers while monitoring leadership status and handling signals.
/// If leadership is lost or SIGTERM/SIGINT is received, all controllers are stopped and the process exits gracefully.
async fn run_controllers_with_leader_election(
    client: Client,
    bind9_manager: Arc<Bind9Manager>,
    leader_rx: tokio::sync::watch::Receiver<bool>,
    _lease_handle: tokio::task::JoinHandle<
        Result<LeaseManager, kube_lease_manager::LeaseManagerError>,
    >,
) -> Result<()> {
    info!("Running controllers with leader election and signal handling");

    // Run controllers concurrently with leadership monitoring and signal handling
    let shutdown_result: Result<()> = tokio::select! {
        // Monitor for SIGINT (Ctrl+C)
        result = tokio::signal::ctrl_c() => {
            info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
            info!("Stopping all controllers and releasing leader election lease...");
            result.map_err(anyhow::Error::from)
        }

        // Monitor for SIGTERM (Kubernetes sends this when deleting pods)
        result = async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate())?;
                sigterm.recv().await;
                Ok::<(), anyhow::Error>(())
            }
            #[cfg(not(unix))]
            {
                // On non-Unix platforms, just wait forever
                std::future::pending::<()>().await;
                Ok::<(), anyhow::Error>(())
            }
        } => {
            info!("Received SIGTERM (pod termination), initiating graceful shutdown...");
            info!("Stopping all controllers and releasing leader election lease...");
            result
        }

        // Monitor leadership - if lost, stop all controllers
        result = monitor_leadership(leader_rx) => {
            match result {
                Ok(()) => {
                    warn!("Leadership lost! Stopping all controllers...");
                    anyhow::bail!("Leadership lost - stepping down")
                }
                Err(e) => {
                    error!("Leadership monitor error: {:?}", e);
                    anyhow::bail!("Leadership monitoring failed: {e}")
                }
            }
        }

        // Run all controllers
        result = run_all_controllers(client, bind9_manager) => {
            result
        }
    };

    // Handle shutdown result
    shutdown_result?;
    info!("Graceful shutdown completed successfully, leader election lease released");
    Ok(())
}

/// Run the `DNSZone` controller
async fn run_dnszone_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting DNSZone controller with watch mappings for all record types");

    let api = Api::<DNSZone>::all(client.clone());

    // Canonical Kubernetes Controller Pattern (with kube-rs constraints):
    // The DNSZone controller watches all DNS record types. When a record changes,
    // we need to trigger reconciliation of DNSZones that have selected that record
    // via label selectors.
    //
    // Challenge: kube-rs `.watches()` requires synchronous mappers, but looking up
    // which zones selected a record requires an async API call to check zone status.
    //
    // Solution: We maintain the existing periodic reconciliation for discovery,
    // and rely on the DNSZone reconciler's built-in record discovery logic. The
    // individual record controllers still update BIND9 directly for immediate
    // propagation of record changes. DNSZone reconciliation updates zone transfer
    // and secondary synchronization.
    //
    // This hybrid approach provides:
    // - Immediate record updates via record controllers
    // - Periodic zone-level reconciliation for consistency
    // - Event-driven reconciliation when records change (via separate controllers)
    //
    // Note: A future enhancement could use kube-rs reflector/store to build an
    // in-memory cache of DNSZones for synchronous lookup in watch mappers, enabling
    // true parent-watches-child pattern.
    Controller::new(api, Config::default())
        .run(
            reconcile_dnszone_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `ARecord` controller
async fn run_arecord_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting ARecord controller");
    debug!("Initializing ARecord controller with cluster-wide watch");

    let api = Api::<ARecord>::all(client.clone());
    debug!("ARecord API client created");

    // Configure controller to only watch for spec changes, not status updates
    // This prevents reconciliation loops when we update status
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            reconcile_arecord_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `TXTRecord` controller
async fn run_txtrecord_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting TXTRecord controller");

    let api = Api::<TXTRecord>::all(client.clone());

    // Configure controller to only watch for spec changes, not status updates
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            reconcile_txtrecord_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `AAAARecord` controller
async fn run_aaaarecord_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting AAAARecord controller");

    let api = Api::<AAAARecord>::all(client.clone());

    // Configure controller to only watch for spec changes, not status updates
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            reconcile_aaaarecord_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `CNAMERecord` controller
async fn run_cnamerecord_controller(
    client: Client,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting CNAMERecord controller");

    let api = Api::<CNAMERecord>::all(client.clone());

    // Configure controller to only watch for spec changes, not status updates
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            reconcile_cnamerecord_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `MXRecord` controller
async fn run_mxrecord_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting MXRecord controller");

    let api = Api::<MXRecord>::all(client.clone());

    // Configure controller to only watch for spec changes, not status updates
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            reconcile_mxrecord_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `NSRecord` controller
async fn run_nsrecord_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting NSRecord controller");

    let api = Api::<NSRecord>::all(client.clone());

    // Configure controller to only watch for spec changes, not status updates
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            reconcile_nsrecord_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `SRVRecord` controller
async fn run_srvrecord_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting SRVRecord controller");

    let api = Api::<SRVRecord>::all(client.clone());

    // Configure controller to only watch for spec changes, not status updates
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            reconcile_srvrecord_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `CAARecord` controller
async fn run_caarecord_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting CAARecord controller");

    let api = Api::<CAARecord>::all(client.clone());

    // Configure controller to only watch for spec changes, not status updates
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            reconcile_caarecord_wrapper,
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `Bind9Cluster` controller
async fn run_bind9cluster_controller(client: Client) -> Result<()> {
    info!("Starting Bind9Cluster controller");

    let api = Api::<Bind9Cluster>::all(client.clone());

    Controller::new(api, Config::default())
        .run(
            reconcile_bind9cluster_wrapper,
            error_policy_cluster,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Reconcile wrapper for `Bind9Cluster`
async fn reconcile_bind9cluster_wrapper(
    cluster: Arc<Bind9Cluster>,
    ctx: Arc<Client>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_BIND9_CLUSTER;
    let start = std::time::Instant::now();

    debug!(
        cluster_name = %cluster.name_any(),
        namespace = ?cluster.namespace(),
        "Reconcile wrapper called for Bind9Cluster"
    );

    let result = reconcile_bind9cluster((*ctx).clone(), (*cluster).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!(
                "Successfully reconciled Bind9Cluster: {}",
                cluster.name_any()
            );
            metrics::record_reconciliation_success(KIND_BIND9_CLUSTER, duration);

            // Check if cluster is ready to determine requeue interval
            let is_ready = cluster
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

            if is_ready {
                // Cluster is ready, check less frequently (5 minutes)
                debug!("Cluster ready, requeueing in 5 minutes");
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Cluster is not ready, check more frequently (30 seconds)
                // to monitor instance status changes
                debug!("Cluster not ready, requeueing in 30 seconds");
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile Bind9Cluster: {}", e);
            metrics::record_reconciliation_error(KIND_BIND9_CLUSTER, duration);
            metrics::record_error(KIND_BIND9_CLUSTER, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `ClusterBind9Provider`
async fn reconcile_clusterbind9provider_wrapper(
    cluster: Arc<ClusterBind9Provider>,
    ctx: Arc<Client>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_CLUSTER_BIND9_PROVIDER;
    let start = std::time::Instant::now();

    debug!(
        cluster_name = %cluster.name_any(),
        "Reconcile wrapper called for ClusterBind9Provider"
    );

    let result = reconcile_clusterbind9provider((*ctx).clone(), (*cluster).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!(
                "Successfully reconciled ClusterBind9Provider: {}",
                cluster.name_any()
            );
            metrics::record_reconciliation_success(KIND_CLUSTER_BIND9_PROVIDER, duration);

            // Check if cluster is ready to determine requeue interval
            let is_ready = cluster
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

            if is_ready {
                // Cluster is ready, check less frequently (5 minutes)
                debug!("Cluster provider ready, requeueing in 5 minutes");
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Cluster is not ready, check more frequently (30 seconds)
                // to monitor instance status changes
                debug!("Cluster provider not ready, requeueing in 30 seconds");
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile ClusterBind9Provider: {}", e);
            metrics::record_reconciliation_error(KIND_CLUSTER_BIND9_PROVIDER, duration);
            metrics::record_error(KIND_CLUSTER_BIND9_PROVIDER, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Run the `ClusterBind9Provider` controller
async fn run_clusterbind9provider_controller(client: Client) -> Result<()> {
    info!("Starting ClusterBind9Provider controller");

    let api = Api::<ClusterBind9Provider>::all(client.clone());

    Controller::new(api, Config::default())
        .run(
            reconcile_clusterbind9provider_wrapper,
            error_policy_clusterprovider,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `Bind9Instance` controller
async fn run_bind9instance_controller(client: Client) -> Result<()> {
    info!("Starting Bind9Instance controller");

    let api = Api::<Bind9Instance>::all(client.clone());

    Controller::new(api, Config::default())
        .run(
            reconcile_bind9instance_wrapper,
            error_policy_instance,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Reconcile wrapper for `Bind9Instance`
async fn reconcile_bind9instance_wrapper(
    instance: Arc<Bind9Instance>,
    ctx: Arc<Client>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_BIND9_INSTANCE;
    let start = std::time::Instant::now();

    let result = reconcile_bind9instance((*ctx).clone(), (*instance).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!(
                "Successfully reconciled Bind9Instance: {}",
                instance.name_any()
            );
            metrics::record_reconciliation_success(KIND_BIND9_INSTANCE, duration);

            // Check if instance is ready to determine requeue interval
            let is_ready = instance
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

            if is_ready {
                // Instance is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Instance is not ready, check more frequently (30 seconds)
                // to monitor pod startup progress
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile Bind9Instance: {}", e);
            metrics::record_reconciliation_error(KIND_BIND9_INSTANCE, duration);
            metrics::record_error(KIND_BIND9_INSTANCE, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `DNSZone`
async fn reconcile_dnszone_wrapper(
    dnszone: Arc<DNSZone>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_DNS_ZONE;
    use bindy::labels::FINALIZER_DNS_ZONE;
    const FINALIZER_NAME: &str = FINALIZER_DNS_ZONE;
    let start = std::time::Instant::now();

    let client = ctx.0.clone();
    let bind9_manager = ctx.1.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let api: Api<DNSZone> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, dnszone.clone(), |event| async {
        match event {
            finalizer::Event::Apply(zone) => {
                // Create or update the zone
                reconcile_dnszone(client.clone(), (*zone).clone(), &bind9_manager)
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled DNSZone: {}", zone.name_any());

                // Check if zone is ready to determine requeue interval
                let is_ready = zone
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    });

                if is_ready {
                    // Zone is ready, check less frequently (5 minutes)
                    Ok(Action::requeue(Duration::from_secs(300)))
                } else {
                    // Zone is not ready, check more frequently (30 seconds)
                    Ok(Action::requeue(Duration::from_secs(30)))
                }
            }
            finalizer::Event::Cleanup(zone) => {
                // Delete the zone
                delete_dnszone(client.clone(), (*zone).clone(), &bind9_manager)
                    .await
                    .map_err(ReconcileError::from)?;
                info!(
                    "Successfully deleted DNSZone from bindcar: {}",
                    zone.name_any()
                );
                metrics::record_resource_deleted(KIND_DNS_ZONE);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_DNS_ZONE, duration);
    } else {
        metrics::record_reconciliation_error(KIND_DNS_ZONE, duration);
        metrics::record_error(KIND_DNS_ZONE, "reconcile_error");
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("DNSZone has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer name"))
        }
    })
}

/// Reconcile wrapper for `ARecord`
async fn reconcile_arecord_wrapper(
    record: Arc<ARecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_A_RECORD;
    let start = std::time::Instant::now();

    let result = reconcile_a_record(ctx.0.clone(), (*record).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled ARecord: {}", record.name_any());
            metrics::record_reconciliation_success(KIND_A_RECORD, duration);

            // Fetch the latest status to check if record is ready
            // (status was updated inside reconcile_a_record, but not in our Arc<ARecord>)
            let namespace = record.namespace().unwrap_or_default();
            let name = record.name_any();
            let api: Api<ARecord> = Api::namespaced(ctx.0.clone(), &namespace);

            let is_ready = if let Ok(updated_record) = api.get(&name).await {
                updated_record
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    })
            } else {
                false
            };

            if is_ready {
                // Record is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Record is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile ARecord: {}", e);
            metrics::record_reconciliation_error(KIND_A_RECORD, duration);
            metrics::record_error(KIND_A_RECORD, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `TXTRecord`
async fn reconcile_txtrecord_wrapper(
    record: Arc<TXTRecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_TXT_RECORD;
    let start = std::time::Instant::now();

    let result = reconcile_txt_record(ctx.0.clone(), (*record).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled TXTRecord: {}", record.name_any());
            metrics::record_reconciliation_success(KIND_TXT_RECORD, duration);

            // Fetch the latest status to check if record is ready
            // (status was updated inside reconcile_txt_record, but not in our Arc<TXTRecord>)
            let namespace = record.namespace().unwrap_or_default();
            let name = record.name_any();
            let api: Api<TXTRecord> = Api::namespaced(ctx.0.clone(), &namespace);

            let is_ready = if let Ok(updated_record) = api.get(&name).await {
                updated_record
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    })
            } else {
                false
            };

            if is_ready {
                // Record is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Record is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile TXTRecord: {}", e);
            metrics::record_reconciliation_error(KIND_TXT_RECORD, duration);
            metrics::record_error(KIND_TXT_RECORD, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `AAAARecord`
async fn reconcile_aaaarecord_wrapper(
    record: Arc<AAAARecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_AAAA_RECORD;
    let start = std::time::Instant::now();

    let result = reconcile_aaaa_record(ctx.0.clone(), (*record).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled AAAARecord: {}", record.name_any());
            metrics::record_reconciliation_success(KIND_AAAA_RECORD, duration);

            // Fetch the latest status to check if record is ready
            // (status was updated inside reconcile_aaaa_record, but not in our Arc<AAAARecord>)
            let namespace = record.namespace().unwrap_or_default();
            let name = record.name_any();
            let api: Api<AAAARecord> = Api::namespaced(ctx.0.clone(), &namespace);

            let is_ready = if let Ok(updated_record) = api.get(&name).await {
                updated_record
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    })
            } else {
                false
            };

            if is_ready {
                // Record is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Record is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile AAAARecord: {}", e);
            metrics::record_reconciliation_error(KIND_AAAA_RECORD, duration);
            metrics::record_error(KIND_AAAA_RECORD, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `CNAMERecord`
async fn reconcile_cnamerecord_wrapper(
    record: Arc<CNAMERecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_CNAME_RECORD;
    let start = std::time::Instant::now();

    let result = reconcile_cname_record(ctx.0.clone(), (*record).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled CNAMERecord: {}", record.name_any());
            metrics::record_reconciliation_success(KIND_CNAME_RECORD, duration);

            // Fetch the latest status to check if record is ready
            // (status was updated inside reconcile_cname_record, but not in our Arc<CNAMERecord>)
            let namespace = record.namespace().unwrap_or_default();
            let name = record.name_any();
            let api: Api<CNAMERecord> = Api::namespaced(ctx.0.clone(), &namespace);

            let is_ready = if let Ok(updated_record) = api.get(&name).await {
                updated_record
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    })
            } else {
                false
            };

            if is_ready {
                // Record is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Record is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile CNAMERecord: {}", e);
            metrics::record_reconciliation_error(KIND_CNAME_RECORD, duration);
            metrics::record_error(KIND_CNAME_RECORD, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `MXRecord`
async fn reconcile_mxrecord_wrapper(
    record: Arc<MXRecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_MX_RECORD;
    let start = std::time::Instant::now();

    let result = reconcile_mx_record(ctx.0.clone(), (*record).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled MXRecord: {}", record.name_any());
            metrics::record_reconciliation_success(KIND_MX_RECORD, duration);

            // Fetch the latest status to check if record is ready
            // (status was updated inside reconcile_mx_record, but not in our Arc<MXRecord>)
            let namespace = record.namespace().unwrap_or_default();
            let name = record.name_any();
            let api: Api<MXRecord> = Api::namespaced(ctx.0.clone(), &namespace);

            let is_ready = if let Ok(updated_record) = api.get(&name).await {
                updated_record
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    })
            } else {
                false
            };

            if is_ready {
                // Record is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Record is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile MXRecord: {}", e);
            metrics::record_reconciliation_error(KIND_MX_RECORD, duration);
            metrics::record_error(KIND_MX_RECORD, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `NSRecord`
async fn reconcile_nsrecord_wrapper(
    record: Arc<NSRecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_NS_RECORD;
    let start = std::time::Instant::now();

    let result = reconcile_ns_record(ctx.0.clone(), (*record).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled NSRecord: {}", record.name_any());
            metrics::record_reconciliation_success(KIND_NS_RECORD, duration);

            // Fetch the latest status to check if record is ready
            // (status was updated inside reconcile_ns_record, but not in our Arc<NSRecord>)
            let namespace = record.namespace().unwrap_or_default();
            let name = record.name_any();
            let api: Api<NSRecord> = Api::namespaced(ctx.0.clone(), &namespace);

            let is_ready = if let Ok(updated_record) = api.get(&name).await {
                updated_record
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    })
            } else {
                false
            };

            if is_ready {
                // Record is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Record is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile NSRecord: {}", e);
            metrics::record_reconciliation_error(KIND_NS_RECORD, duration);
            metrics::record_error(KIND_NS_RECORD, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `SRVRecord`
async fn reconcile_srvrecord_wrapper(
    record: Arc<SRVRecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_SRV_RECORD;
    let start = std::time::Instant::now();

    let result = reconcile_srv_record(ctx.0.clone(), (*record).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled SRVRecord: {}", record.name_any());
            metrics::record_reconciliation_success(KIND_SRV_RECORD, duration);

            // Fetch the latest status to check if record is ready
            // (status was updated inside reconcile_srv_record, but not in our Arc<SRVRecord>)
            let namespace = record.namespace().unwrap_or_default();
            let name = record.name_any();
            let api: Api<SRVRecord> = Api::namespaced(ctx.0.clone(), &namespace);

            let is_ready = if let Ok(updated_record) = api.get(&name).await {
                updated_record
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    })
            } else {
                false
            };

            if is_ready {
                // Record is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Record is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile SRVRecord: {}", e);
            metrics::record_reconciliation_error(KIND_SRV_RECORD, duration);
            metrics::record_error(KIND_SRV_RECORD, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `CAARecord`
async fn reconcile_caarecord_wrapper(
    record: Arc<CAARecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_CAA_RECORD;
    let start = std::time::Instant::now();

    let result = reconcile_caa_record(ctx.0.clone(), (*record).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!("Successfully reconciled CAARecord: {}", record.name_any());
            metrics::record_reconciliation_success(KIND_CAA_RECORD, duration);

            // Fetch the latest status to check if record is ready
            // (status was updated inside reconcile_caa_record, but not in our Arc<CAARecord>)
            let namespace = record.namespace().unwrap_or_default();
            let name = record.name_any();
            let api: Api<CAARecord> = Api::namespaced(ctx.0.clone(), &namespace);

            let is_ready = if let Ok(updated_record) = api.get(&name).await {
                updated_record
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.first())
                    .is_some_and(|condition| {
                        condition.r#type == "Ready" && condition.status == "True"
                    })
            } else {
                false
            };

            if is_ready {
                // Record is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Record is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile CAARecord: {}", e);
            metrics::record_reconciliation_error(KIND_CAA_RECORD, duration);
            metrics::record_error(KIND_CAA_RECORD, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Error policy for controller
fn error_policy(
    _resource: Arc<impl std::fmt::Debug>,
    _err: &ReconcileError,
    _ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Action {
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_DURATION_SECS))
}

/// Error policy for `Bind9Cluster` controller
fn error_policy_cluster(
    _resource: Arc<impl std::fmt::Debug>,
    _err: &ReconcileError,
    _ctx: Arc<Client>,
) -> Action {
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_DURATION_SECS))
}

/// Error policy for `ClusterBind9Provider` controller
fn error_policy_clusterprovider(
    _resource: Arc<impl std::fmt::Debug>,
    _err: &ReconcileError,
    _ctx: Arc<Client>,
) -> Action {
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_DURATION_SECS))
}

/// Error policy for `Bind9Instance` controller
fn error_policy_instance(
    _resource: Arc<impl std::fmt::Debug>,
    _err: &ReconcileError,
    _ctx: Arc<Client>,
) -> Action {
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_DURATION_SECS))
}

// Tests are in main_tests.rs
#[cfg(test)]
mod main_tests;
