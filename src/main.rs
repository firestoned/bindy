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
    context::{Context, Metrics, Stores},
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
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Secret, Service, ServiceAccount};
use kube::{
    runtime::{controller::Action, finalizer, reflector, watcher, watcher::Config, Controller},
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

/// Initialize reflectors for all CRD types and create shared context.
///
/// This function creates reflector tasks for all custom resources, populating
/// in-memory stores that enable O(1) label-based lookups without API queries.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
///
/// # Returns
///
/// * `Arc<Context>` - Shared context with client, stores, and metrics
///
/// # Architecture
///
/// Each reflector spawns a background task that watches its resource type
/// and updates the corresponding store. The stores are then made available
/// to all controllers through the shared context.
#[allow(clippy::too_many_lines, clippy::unused_async)]
async fn initialize_shared_context(client: Client) -> Result<Arc<Context>> {
    info!("Initializing reflectors for all CRD types");

    // Create APIs for all CRD types
    let cluster_bind9_providers_api = Api::<ClusterBind9Provider>::all(client.clone());
    let bind9_clusters_api = Api::<Bind9Cluster>::all(client.clone());
    let bind9_instances_api = Api::<Bind9Instance>::all(client.clone());
    let bind9_deployments_api = Api::<Deployment>::all(client.clone());
    let dnszones_api = Api::<DNSZone>::all(client.clone());
    let a_records_api = Api::<ARecord>::all(client.clone());
    let aaaa_records_api = Api::<AAAARecord>::all(client.clone());
    let cname_records_api = Api::<CNAMERecord>::all(client.clone());
    let txt_records_api = Api::<TXTRecord>::all(client.clone());
    let mx_records_api = Api::<MXRecord>::all(client.clone());
    let ns_records_api = Api::<NSRecord>::all(client.clone());
    let srv_records_api = Api::<SRVRecord>::all(client.clone());
    let caa_records_api = Api::<CAARecord>::all(client.clone());

    // Create stores (will be populated by reflectors)
    let (cluster_bind9_providers_store, cluster_bind9_providers_writer) = reflector::store();
    let (bind9_clusters_store, bind9_clusters_writer) = reflector::store();
    let (bind9_instances_store, bind9_instances_writer) = reflector::store();
    let (bind9_deployments_store, bind9_deployments_writer) = reflector::store();
    let (dnszones_store, dnszones_writer) = reflector::store();
    let (a_records_store, a_records_writer) = reflector::store();
    let (aaaa_records_store, aaaa_records_writer) = reflector::store();
    let (cname_records_store, cname_records_writer) = reflector::store();
    let (txt_records_store, txt_records_writer) = reflector::store();
    let (mx_records_store, mx_records_writer) = reflector::store();
    let (ns_records_store, ns_records_writer) = reflector::store();
    let (srv_records_store, srv_records_writer) = reflector::store();
    let (caa_records_store, caa_records_writer) = reflector::store();

    // Start reflector tasks (one per CRD type)
    // These run in the background and continuously update the stores
    tokio::spawn(async move {
        let stream = watcher(cluster_bind9_providers_api, watcher::Config::default());
        reflector(cluster_bind9_providers_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("ClusterBind9Provider reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(bind9_clusters_api, watcher::Config::default());
        reflector(bind9_clusters_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("Bind9Cluster reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(bind9_instances_api, watcher::Config::default());
        reflector(bind9_instances_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("Bind9Instance reflector stream ended");
    });

    tokio::spawn(async move {
        // Filter deployments to only include those owned by Bind9Instance
        // We'll use a streaming filter to check ownerReferences
        let stream =
            watcher(bind9_deployments_api, watcher::Config::default()).filter_map(
                |event| async move {
                    match event {
                        Ok(watcher::Event::Apply(deployment)) => {
                            // Check if this deployment is owned by a Bind9Instance
                            let is_bind9_deployment =
                                deployment.metadata.owner_references.as_ref().is_some_and(
                                    |owners| {
                                        owners.iter().any(|owner| owner.kind == "Bind9Instance")
                                    },
                                );

                            if is_bind9_deployment {
                                Some(Ok(watcher::Event::Apply(deployment)))
                            } else {
                                None
                            }
                        }
                        Ok(watcher::Event::Delete(deployment)) => {
                            // Also filter deleted events
                            let is_bind9_deployment =
                                deployment.metadata.owner_references.as_ref().is_some_and(
                                    |owners| {
                                        owners.iter().any(|owner| owner.kind == "Bind9Instance")
                                    },
                                );

                            if is_bind9_deployment {
                                Some(Ok(watcher::Event::Delete(deployment)))
                            } else {
                                None
                            }
                        }
                        Ok(watcher::Event::InitApply(deployment)) => {
                            // Also filter init events
                            let is_bind9_deployment =
                                deployment.metadata.owner_references.as_ref().is_some_and(
                                    |owners| {
                                        owners.iter().any(|owner| owner.kind == "Bind9Instance")
                                    },
                                );

                            if is_bind9_deployment {
                                Some(Ok(watcher::Event::InitApply(deployment)))
                            } else {
                                None
                            }
                        }
                        Ok(watcher::Event::Init) => Some(Ok(watcher::Event::Init)),
                        Ok(watcher::Event::InitDone) => Some(Ok(watcher::Event::InitDone)),
                        Err(e) => Some(Err(e)),
                    }
                },
            );

        reflector(bind9_deployments_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("Deployment reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(dnszones_api, watcher::Config::default());
        reflector(dnszones_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("DNSZone reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(a_records_api, watcher::Config::default());
        reflector(a_records_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("ARecord reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(aaaa_records_api, watcher::Config::default());
        reflector(aaaa_records_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("AAAARecord reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(cname_records_api, watcher::Config::default());
        reflector(cname_records_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("CNAMERecord reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(txt_records_api, watcher::Config::default());
        reflector(txt_records_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("TXTRecord reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(mx_records_api, watcher::Config::default());
        reflector(mx_records_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("MXRecord reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(ns_records_api, watcher::Config::default());
        reflector(ns_records_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("NSRecord reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(srv_records_api, watcher::Config::default());
        reflector(srv_records_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("SRVRecord reflector stream ended");
    });

    tokio::spawn(async move {
        let stream = watcher(caa_records_api, watcher::Config::default());
        reflector(caa_records_writer, stream)
            .for_each(|_| futures::future::ready(()))
            .await;
        warn!("CAARecord reflector stream ended");
    });

    // Create the stores structure
    let stores = Stores {
        cluster_bind9_providers: cluster_bind9_providers_store,
        bind9_clusters: bind9_clusters_store,
        bind9_instances: bind9_instances_store,
        bind9_deployments: bind9_deployments_store,
        dnszones: dnszones_store,
        a_records: a_records_store,
        aaaa_records: aaaa_records_store,
        cname_records: cname_records_store,
        txt_records: txt_records_store,
        mx_records: mx_records_store,
        ns_records: ns_records_store,
        srv_records: srv_records_store,
        caa_records: caa_records_store,
    };

    // Create HTTP client for bindcar API calls
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    // Create the shared context
    let context = Arc::new(Context {
        client,
        stores,
        http_client,
        metrics: Metrics::default(),
    });

    info!("Shared context initialized with reflectors for all CRD types");

    Ok(context)
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

/// Create a default watcher configuration.
///
/// Returns a basic watcher configuration without semantic filtering.
/// Used for controllers that need to watch all changes including status updates.
///
/// # Returns
///
/// A `Config` instance with default settings.
#[inline]
fn default_watcher_config() -> Config {
    Config::default()
}

/// Create a semantic watcher configuration.
///
/// Returns a watcher configuration that only triggers on semantic changes
/// (spec modifications), ignoring status-only updates. This prevents
/// reconciliation loops when controllers update status fields.
///
/// # Returns
///
/// A `Config` instance configured with semantic filtering.
#[inline]
fn semantic_watcher_config() -> Config {
    Config::default().any_semantic()
}

/// Run all controllers without leader election, with signal handling
async fn run_controllers_without_leader_election(
    context: Arc<Context>,
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

        // Run all controllers - delegate to shared function
        result = run_all_controllers(context.clone(), bind9_manager.clone()) => {
            result
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

    // Initialize shared context with reflectors for all CRD types
    let context = initialize_shared_context(client.clone()).await?;

    // Start the metrics HTTP server
    let _metrics_handle = start_metrics_server();

    let leader_election_config = load_leader_election_config();

    if leader_election_config.enabled {
        info!(
            lease_name = %leader_election_config.lease_name,
            lease_namespace = %leader_election_config.lease_namespace,
            identity = %leader_election_config.identity,
            lease_duration_secs = leader_election_config.lease_duration,
            renew_deadline_secs = leader_election_config.renew_deadline,
            "Leader election enabled"
        );

        // Create and start lease manager for leader election
        // The manager returns a watch receiver (to monitor leadership status)
        // and a join handle (to monitor the lease renewal task)
        info!("Starting leader election, waiting to acquire leadership...");

        let lease_manager =
            LeaseManagerBuilder::new(client.clone(), &leader_election_config.lease_name)
                .with_namespace(&leader_election_config.lease_namespace)
                .with_identity(&leader_election_config.identity)
                .with_duration(leader_election_config.lease_duration)
                .with_grace(leader_election_config.retry_period)
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
        run_controllers_with_leader_election(context, bind9_manager, leader_rx, lease_handle)
            .await?;
    } else {
        run_controllers_without_leader_election(context, bind9_manager).await?;
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
async fn run_all_controllers(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    tokio::select! {
        result = run_bind9cluster_controller(context.clone()) => {
            error!("CRITICAL: Bind9Cluster controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("Bind9Cluster controller exited unexpectedly without error")
        }
        result = run_clusterbind9provider_controller(context.clone()) => {
            error!("CRITICAL: ClusterBind9Provider controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("ClusterBind9Provider controller exited unexpectedly without error")
        }
        result = run_bind9instance_controller(context.clone()) => {
            error!("CRITICAL: Bind9Instance controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("Bind9Instance controller exited unexpectedly without error")
        }
        result = run_dnszone_controller(context.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: DNSZone controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("DNSZone controller exited unexpectedly without error")
        }
        result = run_arecord_controller(context.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: ARecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("ARecord controller exited unexpectedly without error")
        }
        result = run_aaaarecord_controller(context.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: AAAARecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("AAAARecord controller exited unexpectedly without error")
        }
        result = run_txtrecord_controller(context.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: TXTRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("TXTRecord controller exited unexpectedly without error")
        }
        result = run_cnamerecord_controller(context.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: CNAMERecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("CNAMERecord controller exited unexpectedly without error")
        }
        result = run_mxrecord_controller(context.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: MXRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("MXRecord controller exited unexpectedly without error")
        }
        result = run_nsrecord_controller(context.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: NSRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("NSRecord controller exited unexpectedly without error")
        }
        result = run_srvrecord_controller(context.clone(), bind9_manager.clone()) => {
            error!("CRITICAL: SRVRecord controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("SRVRecord controller exited unexpectedly without error")
        }
        result = run_caarecord_controller(context.clone(), bind9_manager.clone()) => {
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
    context: Arc<Context>,
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
        result = run_all_controllers(context, bind9_manager) => {
            result
        }
    };

    // Handle shutdown result
    shutdown_result?;
    info!("Graceful shutdown completed successfully, leader election lease released");
    Ok(())
}

/// Run the `ClusterBind9Provider` controller
async fn run_clusterbind9provider_controller(context: Arc<Context>) -> Result<()> {
    info!("Starting ClusterBind9Provider controller");

    let client = context.client.clone();
    let api = Api::<ClusterBind9Provider>::all(client.clone());
    let bind9_cluster_api = Api::<Bind9Cluster>::all(client.clone());

    Controller::new(api, default_watcher_config())
        .owns(bind9_cluster_api, semantic_watcher_config())
        .run(
            reconcile_clusterbind9provider_wrapper,
            error_policy,
            context,
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Reconcile wrapper for `ClusterBind9Provider`
async fn reconcile_clusterbind9provider_wrapper(
    cluster: Arc<ClusterBind9Provider>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_CLUSTER_BIND9_PROVIDER;
    let start = std::time::Instant::now();

    debug!(
        cluster_name = %cluster.name_any(),
        "Reconcile wrapper called for ClusterBind9Provider"
    );

    let result = reconcile_clusterbind9provider(ctx.clone(), (*cluster).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!(
                "Successfully reconciled ClusterBind9Provider: {}",
                cluster.name_any()
            );
            metrics::record_reconciliation_success(KIND_CLUSTER_BIND9_PROVIDER, duration);

            // Event-Driven: Use consistent requeue interval regardless of readiness.
            // Changes to owned Bind9Cluster resources trigger immediate reconciliation
            // via watch events, so we don't need shorter polling intervals.
            debug!("Cluster provider reconciled, requeueing in 5 minutes");
            Ok(Action::requeue(Duration::from_secs(
                bindy::record_wrappers::REQUEUE_WHEN_READY_SECS,
            )))
        }
        Err(e) => {
            error!("Failed to reconcile ClusterBind9Provider: {}", e);
            metrics::record_reconciliation_error(KIND_CLUSTER_BIND9_PROVIDER, duration);
            metrics::record_error(KIND_CLUSTER_BIND9_PROVIDER, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Run the `Bind9Cluster` controller
async fn run_bind9cluster_controller(context: Arc<Context>) -> Result<()> {
    info!("Starting Bind9Cluster controller");

    let client = context.client.clone();
    let api = Api::<Bind9Cluster>::all(client.clone());
    let instance_api = Api::<Bind9Instance>::all(client.clone());

    Controller::new(api, default_watcher_config())
        .owns(instance_api, semantic_watcher_config())
        .run(reconcile_bind9cluster_wrapper, error_policy, context)
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Reconcile wrapper for `Bind9Cluster`
async fn reconcile_bind9cluster_wrapper(
    cluster: Arc<Bind9Cluster>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_BIND9_CLUSTER;
    let start = std::time::Instant::now();

    debug!(
        cluster_name = %cluster.name_any(),
        namespace = ?cluster.namespace(),
        "Reconcile wrapper called for Bind9Cluster"
    );

    let result = reconcile_bind9cluster(ctx.clone(), (*cluster).clone()).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!(
                "Successfully reconciled Bind9Cluster: {}",
                cluster.name_any()
            );
            metrics::record_reconciliation_success(KIND_BIND9_CLUSTER, duration);

            // Event-Driven: Use consistent requeue interval regardless of readiness.
            // Changes to owned Bind9Instance resources trigger immediate reconciliation
            // via watch events, so we don't need shorter polling intervals.
            debug!("Cluster reconciled, requeueing in 5 minutes");
            Ok(Action::requeue(Duration::from_secs(
                bindy::record_wrappers::REQUEUE_WHEN_READY_SECS,
            )))
        }
        Err(e) => {
            error!("Failed to reconcile Bind9Cluster: {}", e);
            metrics::record_reconciliation_error(KIND_BIND9_CLUSTER, duration);
            metrics::record_error(KIND_BIND9_CLUSTER, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Run the `Bind9Instance` controller
#[allow(clippy::too_many_lines)]
async fn run_bind9instance_controller(context: Arc<Context>) -> Result<()> {
    info!("Starting Bind9Instance controller");

    let client = context.client.clone();
    let api = Api::<Bind9Instance>::all(client.clone());
    let deployment_api = Api::<Deployment>::all(client.clone());
    let service_account_api = Api::<ServiceAccount>::all(client.clone());
    let secret_api = Api::<Secret>::all(client.clone());
    let configmap_api = Api::<ConfigMap>::all(client.clone());
    let service_api = Api::<Service>::all(client.clone());
    let _dnszone_api = Api::<DNSZone>::all(client.clone());

    // Clone client and stores for the watch mapper closure
    let client_for_watch = client.clone();
    let stores_for_watch = context.stores.clone();

    // DNSZone API for status-only watcher
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Build the controller
    Controller::new(api.clone(), semantic_watcher_config())
        .owns(service_account_api, default_watcher_config())
        .owns(secret_api, default_watcher_config())
        .owns(configmap_api, default_watcher_config())
        .owns(deployment_api, default_watcher_config())
        .owns(service_api, default_watcher_config())
        .watches(dnszone_api, default_watcher_config(), move |zone| {
            // Event-driven watcher: When DNSZone.status.bind9Instances changes,
            // update the corresponding Bind9Instance.status.zones.
            //
            // This provides immediate zone reconciliation when zone selections change.
            //
            // CRITICAL: Returns empty vec to avoid triggering full reconciliation.
            // The status update is done directly in the mapper via a background task.

            // Extract instances that should have this zone
            let selected_instances = zone
                .status
                .as_ref()
                .map(|s| s.bind9_instances.clone())
                .unwrap_or_default();

            // Clone for the spawned task
            let client = client_for_watch.clone();
            let stores = stores_for_watch.clone();

            // Spawn background task to update instances
            tokio::spawn(async move {
                // Call reconcile_instance_zones() for each instance in the zone's selection
                for instance_ref in &selected_instances {
                    let instance_api =
                        Api::<Bind9Instance>::namespaced(client.clone(), &instance_ref.namespace);

                    // Fetch current instance
                    let instance = match instance_api.get(&instance_ref.name).await {
                        Ok(inst) => inst,
                        Err(e) => {
                            warn!(
                                "Failed to fetch Bind9Instance {}/{} for zone reconciliation: {}",
                                instance_ref.namespace, instance_ref.name, e
                            );
                            continue;
                        }
                    };

                    // Reconcile zones for this instance (status-only update)
                    if let Err(e) = bindy::reconcilers::bind9instance::reconcile_instance_zones(
                        &client, &stores, &instance,
                    )
                    .await
                    {
                        warn!(
                            "Failed to reconcile zones for Bind9Instance {}/{}: {}",
                            instance_ref.namespace, instance_ref.name, e
                        );
                    }
                }
            });

            // Return empty vec to avoid triggering full reconciliation
            vec![]
        })
        .run(reconcile_bind9instance_wrapper, error_policy, context)
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Reconcile wrapper for `Bind9Instance`
async fn reconcile_bind9instance_wrapper(
    instance: Arc<Bind9Instance>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_BIND9_INSTANCE;

    let start = std::time::Instant::now();

    info!("Reconciling instance {}", instance.name_any());
    let result = Box::pin(reconcile_bind9instance(ctx.clone(), (*instance).clone())).await;
    let duration = start.elapsed();

    match result {
        Ok(()) => {
            info!(
                "Successfully reconciled Bind9Instance: {}",
                instance.name_any()
            );
            metrics::record_reconciliation_success(KIND_BIND9_INSTANCE, duration);

            // Event-Driven: Use consistent requeue interval regardless of readiness.
            // Changes to owned resources (Deployment, Service, etc.) trigger immediate
            // reconciliation via watch events, so we don't need shorter polling intervals
            // to monitor pod startup progress.
            debug!("Instance reconciled, requeueing in 5 minutes");
            Ok(Action::requeue(Duration::from_secs(
                bindy::record_wrappers::REQUEUE_WHEN_READY_SECS,
            )))
        }
        Err(e) => {
            error!("Failed to reconcile Bind9Instance: {}", e);
            metrics::record_reconciliation_error(KIND_BIND9_INSTANCE, duration);
            metrics::record_error(KIND_BIND9_INSTANCE, "reconcile_error");
            Err(e.into())
        }
    }
}

/// Run the `DNSZone` controller
#[allow(clippy::too_many_lines)]
async fn run_dnszone_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting DNSZone controller");

    let client = context.client.clone();
    let api = Api::<DNSZone>::all(client.clone());

    // Create API clients for Bind9Instance and all record types
    let bind9instance_api = Api::<Bind9Instance>::all(client.clone());
    let arecord_api = Api::<ARecord>::all(client.clone());
    let aaaarecord_api = Api::<AAAARecord>::all(client.clone());
    let txtrecord_api = Api::<TXTRecord>::all(client.clone());
    let cnamerecord_api = Api::<CNAMERecord>::all(client.clone());
    let mxrecord_api = Api::<MXRecord>::all(client.clone());
    let nsrecord_api = Api::<NSRecord>::all(client.clone());
    let srvrecord_api = Api::<SRVRecord>::all(client.clone());
    let caarecord_api = Api::<CAARecord>::all(client.clone());

    // Clone context for watch closures
    let ctx_for_a = context.clone();
    let ctx_for_aaaa = context.clone();
    let ctx_for_txt = context.clone();
    let ctx_for_cname = context.clone();
    let ctx_for_mx = context.clone();
    let ctx_for_ns = context.clone();
    let ctx_for_srv = context.clone();
    let ctx_for_caa = context.clone();
    let ctx_for_instance_watch = context.clone();

    // Event-Driven Architecture for DNSZone (Zone-Centric Selection):
    // 1. Watches Bind9Instance label changes - trigger zones with matching bind9_instances_from selectors
    // 2. Watches Records: Record changes → zones check selectors → update status.zoneRef
    //
    // CRITICAL: Zone-to-Instance Selection
    // - Zones select instances via spec.bind9_instances_from label selectors
    // - When instance labels change, all zones with matching selectors must reconcile
    // - Uses reflector store for efficient lookups without API calls
    Controller::new(api.clone(), semantic_watcher_config())
        .watches(
            bind9instance_api,
            default_watcher_config(),
            move |instance| {
                // When a Bind9Instance changes (labels/status/etc), find all DNSZones
                // that might select this instance via their bind9_instances_from selectors

                let Some(instance_namespace) = instance.namespace() else {
                    return vec![];
                };
                let instance_name = instance.name_any();
                let instance_labels = instance.metadata.labels.as_ref();

                // Get all DNSZones and check which ones have bind9_instances_from selectors
                // that match this instance's labels
                let zones_to_reconcile: Vec<kube::runtime::reflector::ObjectRef<DNSZone>> =
                    ctx_for_instance_watch
                        .stores
                        .dnszones
                        .state()
                        .iter()
                        .filter_map(|zone| {
                            let zone_namespace = zone.namespace()?;
                            let zone_name = zone.name_any();

                            // Check if zone has bind9_instances_from selectors
                            let bind9_instances_from = zone.spec.bind9_instances_from.as_ref()?;
                            if bind9_instances_from.is_empty() {
                                return None;
                            }

                            // Check if ANY of the bind9_instances_from selectors match this instance
                            let instance_labels = instance_labels?;
                            let matches = bind9_instances_from.iter().any(|source| {
                                source.selector.matches(instance_labels)
                            });

                            if matches {
                                debug!(
                                    "Bind9Instance {}/{} label change triggers DNSZone {}/{} reconciliation",
                                    instance_namespace, instance_name, zone_namespace, zone_name
                                );
                                Some(
                                    kube::runtime::reflector::ObjectRef::new(&zone_name)
                                        .within(&zone_namespace),
                                )
                            } else {
                                None
                            }
                        })
                        .collect();

                zones_to_reconcile
            },
        )
        .watches(arecord_api, default_watcher_config(), move |record| {
            // Use shared reflector store to find zones with recordsFrom matching record labels
            let Some(namespace) = record.namespace() else {
                return vec![];
            };
            let record_labels = record.labels();

            ctx_for_a
                .stores
                .dnszones_selecting_record(record_labels, &namespace)
                .into_iter()
                .map(|(name, ns)| kube::runtime::reflector::ObjectRef::new(&name).within(&ns))
                .collect::<Vec<_>>()
        })
        .watches(aaaarecord_api, default_watcher_config(), move |record| {
            let Some(namespace) = record.namespace() else {
                return vec![];
            };
            let record_labels = record.labels();

            ctx_for_aaaa
                .stores
                .dnszones_selecting_record(record_labels, &namespace)
                .into_iter()
                .map(|(name, ns)| kube::runtime::reflector::ObjectRef::new(&name).within(&ns))
                .collect::<Vec<_>>()
        })
        .watches(txtrecord_api, default_watcher_config(), move |record| {
            let Some(namespace) = record.namespace() else {
                return vec![];
            };
            let record_labels = record.labels();

            ctx_for_txt
                .stores
                .dnszones_selecting_record(record_labels, &namespace)
                .into_iter()
                .map(|(name, ns)| kube::runtime::reflector::ObjectRef::new(&name).within(&ns))
                .collect::<Vec<_>>()
        })
        .watches(cnamerecord_api, default_watcher_config(), move |record| {
            let Some(namespace) = record.namespace() else {
                return vec![];
            };
            let record_labels = record.labels();

            ctx_for_cname
                .stores
                .dnszones_selecting_record(record_labels, &namespace)
                .into_iter()
                .map(|(name, ns)| kube::runtime::reflector::ObjectRef::new(&name).within(&ns))
                .collect::<Vec<_>>()
        })
        .watches(mxrecord_api, default_watcher_config(), move |record| {
            let Some(namespace) = record.namespace() else {
                return vec![];
            };
            let record_labels = record.labels();

            ctx_for_mx
                .stores
                .dnszones_selecting_record(record_labels, &namespace)
                .into_iter()
                .map(|(name, ns)| kube::runtime::reflector::ObjectRef::new(&name).within(&ns))
                .collect::<Vec<_>>()
        })
        .watches(nsrecord_api, default_watcher_config(), move |record| {
            let Some(namespace) = record.namespace() else {
                return vec![];
            };
            let record_labels = record.labels();

            ctx_for_ns
                .stores
                .dnszones_selecting_record(record_labels, &namespace)
                .into_iter()
                .map(|(name, ns)| kube::runtime::reflector::ObjectRef::new(&name).within(&ns))
                .collect::<Vec<_>>()
        })
        .watches(srvrecord_api, default_watcher_config(), move |record| {
            let Some(namespace) = record.namespace() else {
                return vec![];
            };
            let record_labels = record.labels();

            ctx_for_srv
                .stores
                .dnszones_selecting_record(record_labels, &namespace)
                .into_iter()
                .map(|(name, ns)| kube::runtime::reflector::ObjectRef::new(&name).within(&ns))
                .collect::<Vec<_>>()
        })
        .watches(caarecord_api, default_watcher_config(), move |record| {
            let Some(namespace) = record.namespace() else {
                return vec![];
            };
            let record_labels = record.labels();

            ctx_for_caa
                .stores
                .dnszones_selecting_record(record_labels, &namespace)
                .into_iter()
                .map(|(name, ns)| kube::runtime::reflector::ObjectRef::new(&name).within(&ns))
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_dnszone_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Reconcile wrapper for `DNSZone`
async fn reconcile_dnszone_wrapper(
    dnszone: Arc<DNSZone>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_DNS_ZONE;
    use bindy::labels::FINALIZER_DNS_ZONE;
    const FINALIZER_NAME: &str = FINALIZER_DNS_ZONE;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let bind9_manager = ctx.1.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let api: Api<DNSZone> = Api::namespaced(client.clone(), &namespace);

    // Skip reconciliation if only status changed (not spec)
    // This prevents tight loops when status updates trigger watch events
    if let Some(status) = &dnszone.status {
        if let (Some(observed_gen), Some(current_gen)) =
            (status.observed_generation, dnszone.metadata.generation)
        {
            if observed_gen == current_gen {
                debug!(
                    "Skipping reconciliation for DNSZone {}/{} - status-only update (generation {} already reconciled)",
                    namespace,
                    dnszone.name_any(),
                    current_gen
                );
                // Re-check after 5 minutes for health monitoring
                return Ok(Action::requeue(Duration::from_secs(
                    bindy::record_wrappers::REQUEUE_WHEN_READY_SECS,
                )));
            }
        }
    }

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, dnszone.clone(), |event| async {
        match event {
            finalizer::Event::Apply(zone) => {
                // Create or update the zone
                reconcile_dnszone(context.clone(), (*zone).clone(), &bind9_manager)
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled DNSZone: {}", zone.name_any());

                // Re-fetch the zone to get updated status (reconcile_dnszone updates it)
                let updated_zone = api
                    .get(&zone.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;
                debug!("Updated DNSZone: {}", updated_zone.name_any());

                // Check if zone has degraded conditions (secondaries failed, etc.)
                // Degraded zones should requeue faster to retry operations
                let has_degraded = updated_zone
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.iter().find(|c| c.r#type == "Degraded"))
                    .is_some_and(|condition| condition.status == "True");
                debug!(
                    "DNSZone {} degraded status: {}",
                    updated_zone.name_any(),
                    has_degraded
                );

                // Check if zone is fully ready (no degradation)
                let is_ready = updated_zone
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.iter().find(|c| c.r#type == "Ready"))
                    .is_some_and(|condition| condition.status == "True")
                    && !has_degraded;

                if is_ready {
                    // Zone is fully ready with no degradation, check less frequently (5 minutes)
                    Ok(Action::requeue(Duration::from_secs(
                        bindy::record_wrappers::REQUEUE_WHEN_READY_SECS,
                    )))
                } else {
                    // Zone is degraded or not ready, check more frequently (30 seconds) to retry
                    Ok(Action::requeue(Duration::from_secs(
                        bindy::record_wrappers::REQUEUE_WHEN_NOT_READY_SECS,
                    )))
                }
            }
            finalizer::Event::Cleanup(zone) => {
                // Delete the zone
                delete_dnszone(context.clone(), (*zone).clone(), &bind9_manager)
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

// ============================================================================
// Record Reconciliation Wrappers (With Finalizer Support)
// ============================================================================

/// Run the `ARecord` controller
async fn run_arecord_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting ARecord controller");
    debug!("Initializing ARecord controller with cluster-wide watch");

    let client = context.client.clone();
    let api = Api::<ARecord>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());
    debug!("ARecord API client created");

    // Configure controller to watch for ALL changes including status updates
    // This allows reacting to status.zoneRef changes set by DNSZone controller
    let watcher_config = default_watcher_config();

    Controller::new(api, watcher_config)
        .watches(dnszone_api, default_watcher_config(), |zone| {
            // When DNSZone.status.selectedRecords[] changes, trigger reconciliation
            // for ARecords that have lastReconciledAt == None (need configuration).
            //
            // Event-Driven Pattern (same as Phase 2 zones → instances):
            // - DNSZone owns the relationship in status.selectedRecords[]
            // - lastReconciledAt == None signals "record needs configuration"
            // - Record reconciles and updates lastReconciledAt after successful BIND9 update
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            // Get records from zone.status.selectedRecords[] that need reconciliation
            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    // Only reconcile ARecords with lastReconciledAt == None
                    record_ref.kind == "ARecord"
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_arecord_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `TXTRecord` controller
async fn run_txtrecord_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting TXTRecord controller");

    let client = context.client.clone();
    let api = Api::<TXTRecord>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Configure controller to watch for ALL changes including status updates
    // This allows reacting to status.zoneRef changes set by DNSZone controller
    let watcher_config = default_watcher_config();

    Controller::new(api, watcher_config)
        .watches(dnszone_api, default_watcher_config(), |zone| {
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    record_ref.kind == "TXTRecord"
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_txtrecord_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `AAAARecord` controller
async fn run_aaaarecord_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting AAAARecord controller");

    let client = context.client.clone();
    let api = Api::<AAAARecord>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Configure controller to watch for ALL changes including status updates
    // This allows reacting to status.zoneRef changes set by DNSZone controller
    let watcher_config = default_watcher_config();

    Controller::new(api, watcher_config)
        .watches(dnszone_api, default_watcher_config(), |zone| {
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    record_ref.kind == "AAAARecord"
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_aaaarecord_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `CNAMERecord` controller
async fn run_cnamerecord_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting CNAMERecord controller");

    let client = context.client.clone();
    let api = Api::<CNAMERecord>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Configure controller to watch for ALL changes including status updates
    // This allows reacting to status.zoneRef changes set by DNSZone controller
    let watcher_config = default_watcher_config();

    Controller::new(api, watcher_config)
        .watches(dnszone_api, default_watcher_config(), |zone| {
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    record_ref.kind == "CNAMERecord"
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_cnamerecord_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `MXRecord` controller
async fn run_mxrecord_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting MXRecord controller");

    let client = context.client.clone();
    let api = Api::<MXRecord>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Configure controller to watch for ALL changes including status updates
    // This allows reacting to status.zoneRef changes set by DNSZone controller
    let watcher_config = default_watcher_config();

    Controller::new(api, watcher_config)
        .watches(dnszone_api, default_watcher_config(), |zone| {
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    record_ref.kind == "MXRecord"
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_mxrecord_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `NSRecord` controller
async fn run_nsrecord_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting NSRecord controller");

    let client = context.client.clone();
    let api = Api::<NSRecord>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Configure controller to watch for ALL changes including status updates
    // This allows reacting to status.zoneRef changes set by DNSZone controller
    let watcher_config = default_watcher_config();

    Controller::new(api, watcher_config)
        .watches(dnszone_api, default_watcher_config(), |zone| {
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    record_ref.kind == "NSRecord"
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_nsrecord_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `SRVRecord` controller
async fn run_srvrecord_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting SRVRecord controller");

    let client = context.client.clone();
    let api = Api::<SRVRecord>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Configure controller to watch for ALL changes including status updates
    // This allows reacting to status.zoneRef changes set by DNSZone controller
    let watcher_config = default_watcher_config();

    Controller::new(api, watcher_config)
        .watches(dnszone_api, default_watcher_config(), |zone| {
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    record_ref.kind == "SRVRecord"
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_srvrecord_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Run the `CAARecord` controller
async fn run_caarecord_controller(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()> {
    info!("Starting CAARecord controller");

    let client = context.client.clone();
    let api = Api::<CAARecord>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Configure controller to watch for ALL changes including status updates
    // This allows reacting to status.zoneRef changes set by DNSZone controller
    let watcher_config = default_watcher_config();

    Controller::new(api, watcher_config)
        .watches(dnszone_api, default_watcher_config(), |zone| {
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    record_ref.kind == "CAARecord"
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .run(
            reconcile_caarecord_wrapper,
            error_policy,
            Arc::new((context.clone(), bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Reconcile wrapper for `ARecord` with finalizer support
async fn reconcile_arecord_wrapper(
    record: Arc<ARecord>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_A_RECORD;
    use bindy::labels::FINALIZER_A_RECORD;
    use hickory_client::rr::RecordType;

    const FINALIZER_NAME: &str = FINALIZER_A_RECORD;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<ARecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_a_record(context.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled ARecord: {}", rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = bindy::record_wrappers::is_resource_ready(&updated_record.status);

                Ok(bindy::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use bindy::reconcilers::delete_record;

                delete_record(&client, &*rec, "A", RecordType::A, &context.stores)
                    .await
                    .map_err(ReconcileError::from)?;

                info!(
                    "Successfully deleted ARecord from BIND9: {}",
                    rec.name_any()
                );
                metrics::record_resource_deleted(KIND_A_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_A_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_A_RECORD, duration);
        metrics::record_error(KIND_A_RECORD, bindy::record_wrappers::ERROR_TYPE_RECONCILE);
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("ARecord has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer for ARecord"))
        }
    })
}

/// Reconcile wrapper for `TXTRecord` with finalizer support
async fn reconcile_txtrecord_wrapper(
    record: Arc<TXTRecord>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_TXT_RECORD;
    use bindy::labels::FINALIZER_TXT_RECORD;
    use hickory_client::rr::RecordType;

    const FINALIZER_NAME: &str = FINALIZER_TXT_RECORD;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<TXTRecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_txt_record(context.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled TXTRecord: {}", rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = bindy::record_wrappers::is_resource_ready(&updated_record.status);

                Ok(bindy::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use bindy::reconcilers::delete_record;

                delete_record(&client, &*rec, "TXT", RecordType::TXT, &context.stores)
                    .await
                    .map_err(ReconcileError::from)?;

                info!(
                    "Successfully deleted TXTRecord from BIND9: {}",
                    rec.name_any()
                );
                metrics::record_resource_deleted(KIND_TXT_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_TXT_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_TXT_RECORD, duration);
        metrics::record_error(
            KIND_TXT_RECORD,
            bindy::record_wrappers::ERROR_TYPE_RECONCILE,
        );
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("TXTRecord has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer for TXTRecord"))
        }
    })
}

/// Reconcile wrapper for `AAAARecord` with finalizer support
async fn reconcile_aaaarecord_wrapper(
    record: Arc<AAAARecord>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_AAAA_RECORD;
    use bindy::labels::FINALIZER_AAAA_RECORD;
    use hickory_client::rr::RecordType;

    const FINALIZER_NAME: &str = FINALIZER_AAAA_RECORD;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<AAAARecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_aaaa_record(context.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled AAAARecord: {}", rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = bindy::record_wrappers::is_resource_ready(&updated_record.status);

                Ok(bindy::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use bindy::reconcilers::delete_record;

                delete_record(&client, &*rec, "AAAA", RecordType::AAAA, &context.stores)
                    .await
                    .map_err(ReconcileError::from)?;

                info!(
                    "Successfully deleted AAAARecord from BIND9: {}",
                    rec.name_any()
                );
                metrics::record_resource_deleted(KIND_AAAA_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_AAAA_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_AAAA_RECORD, duration);
        metrics::record_error(
            KIND_AAAA_RECORD,
            bindy::record_wrappers::ERROR_TYPE_RECONCILE,
        );
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("AAAARecord has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer for AAAARecord"))
        }
    })
}

/// Reconcile wrapper for `CNAMERecord` with finalizer support
async fn reconcile_cnamerecord_wrapper(
    record: Arc<CNAMERecord>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_CNAME_RECORD;
    use bindy::labels::FINALIZER_CNAME_RECORD;
    use hickory_client::rr::RecordType;

    const FINALIZER_NAME: &str = FINALIZER_CNAME_RECORD;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<CNAMERecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_cname_record(context.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled CNAMERecord: {}", rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = bindy::record_wrappers::is_resource_ready(&updated_record.status);

                Ok(bindy::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use bindy::reconcilers::delete_record;

                delete_record(&client, &*rec, "CNAME", RecordType::CNAME, &context.stores)
                    .await
                    .map_err(ReconcileError::from)?;

                info!(
                    "Successfully deleted CNAMERecord from BIND9: {}",
                    rec.name_any()
                );
                metrics::record_resource_deleted(KIND_CNAME_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_CNAME_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_CNAME_RECORD, duration);
        metrics::record_error(
            KIND_CNAME_RECORD,
            bindy::record_wrappers::ERROR_TYPE_RECONCILE,
        );
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("CNAMERecord has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer for CNAMERecord"))
        }
    })
}

/// Reconcile wrapper for `MXRecord` with finalizer support
async fn reconcile_mxrecord_wrapper(
    record: Arc<MXRecord>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_MX_RECORD;
    use bindy::labels::FINALIZER_MX_RECORD;
    use hickory_client::rr::RecordType;

    const FINALIZER_NAME: &str = FINALIZER_MX_RECORD;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<MXRecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_mx_record(context.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled MXRecord: {}", rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = bindy::record_wrappers::is_resource_ready(&updated_record.status);

                Ok(bindy::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use bindy::reconcilers::delete_record;

                delete_record(&client, &*rec, "MX", RecordType::MX, &context.stores)
                    .await
                    .map_err(ReconcileError::from)?;

                info!(
                    "Successfully deleted MXRecord from BIND9: {}",
                    rec.name_any()
                );
                metrics::record_resource_deleted(KIND_MX_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_MX_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_MX_RECORD, duration);
        metrics::record_error(KIND_MX_RECORD, bindy::record_wrappers::ERROR_TYPE_RECONCILE);
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("MXRecord has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer for MXRecord"))
        }
    })
}

/// Reconcile wrapper for `NSRecord` with finalizer support
async fn reconcile_nsrecord_wrapper(
    record: Arc<NSRecord>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_NS_RECORD;
    use bindy::labels::FINALIZER_NS_RECORD;
    use hickory_client::rr::RecordType;

    const FINALIZER_NAME: &str = FINALIZER_NS_RECORD;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<NSRecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_ns_record(context.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled NSRecord: {}", rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = bindy::record_wrappers::is_resource_ready(&updated_record.status);

                Ok(bindy::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use bindy::reconcilers::delete_record;

                delete_record(&client, &*rec, "NS", RecordType::NS, &context.stores)
                    .await
                    .map_err(ReconcileError::from)?;

                info!(
                    "Successfully deleted NSRecord from BIND9: {}",
                    rec.name_any()
                );
                metrics::record_resource_deleted(KIND_NS_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_NS_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_NS_RECORD, duration);
        metrics::record_error(KIND_NS_RECORD, bindy::record_wrappers::ERROR_TYPE_RECONCILE);
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("NSRecord has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer for NSRecord"))
        }
    })
}

/// Reconcile wrapper for `SRVRecord` with finalizer support
async fn reconcile_srvrecord_wrapper(
    record: Arc<SRVRecord>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_SRV_RECORD;
    use bindy::labels::FINALIZER_SRV_RECORD;
    use hickory_client::rr::RecordType;

    const FINALIZER_NAME: &str = FINALIZER_SRV_RECORD;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<SRVRecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_srv_record(context.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled SRVRecord: {}", rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = bindy::record_wrappers::is_resource_ready(&updated_record.status);

                Ok(bindy::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use bindy::reconcilers::delete_record;

                delete_record(&client, &*rec, "SRV", RecordType::SRV, &context.stores)
                    .await
                    .map_err(ReconcileError::from)?;

                info!(
                    "Successfully deleted SRVRecord from BIND9: {}",
                    rec.name_any()
                );
                metrics::record_resource_deleted(KIND_SRV_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_SRV_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_SRV_RECORD, duration);
        metrics::record_error(
            KIND_SRV_RECORD,
            bindy::record_wrappers::ERROR_TYPE_RECONCILE,
        );
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("SRVRecord has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer for SRVRecord"))
        }
    })
}

/// Reconcile wrapper for `CAARecord` with finalizer support
async fn reconcile_caarecord_wrapper(
    record: Arc<CAARecord>,
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_CAA_RECORD;
    use bindy::labels::FINALIZER_CAA_RECORD;
    use hickory_client::rr::RecordType;

    const FINALIZER_NAME: &str = FINALIZER_CAA_RECORD;
    let start = std::time::Instant::now();

    let context = ctx.0.clone();
    let client = context.client.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<CAARecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_caa_record(context.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled CAARecord: {}", rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = bindy::record_wrappers::is_resource_ready(&updated_record.status);

                Ok(bindy::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use bindy::reconcilers::delete_record;

                delete_record(&client, &*rec, "CAA", RecordType::CAA, &context.stores)
                    .await
                    .map_err(ReconcileError::from)?;

                info!(
                    "Successfully deleted CAARecord from BIND9: {}",
                    rec.name_any()
                );
                metrics::record_resource_deleted(KIND_CAA_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_CAA_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_CAA_RECORD, duration);
        metrics::record_error(
            KIND_CAA_RECORD,
            bindy::record_wrappers::ERROR_TYPE_RECONCILE,
        );
    }

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow::anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => {
            ReconcileError::from(anyhow::anyhow!("CAARecord has no name"))
        }
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow::anyhow!("Invalid finalizer for CAARecord"))
        }
    })
}

/// Generic error policy for all controllers.
///
/// This function handles reconciliation errors by requeuing the resource
/// after a fixed delay. It's generic over both the resource type and
/// context type, allowing it to be used across all controller types.
///
/// # Arguments
///
/// * `resource` - The resource being reconciled
/// * `err` - The reconciliation error that occurred
/// * `_ctx` - The controller context (unused)
///
/// # Returns
///
/// An `Action` to requeue the resource after `ERROR_REQUEUE_DURATION_SECS` seconds.
#[allow(clippy::needless_pass_by_value)]
fn error_policy<T, C>(resource: Arc<T>, err: &ReconcileError, _ctx: Arc<C>) -> Action
where
    T: std::fmt::Debug,
{
    error!(
        error = %err,
        resource = ?resource,
        "Reconciliation error - will retry in {}s",
        ERROR_REQUEUE_DURATION_SECS
    );
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_DURATION_SECS))
}

// Tests are in main_tests.rs
#[cfg(test)]
mod main_tests;
