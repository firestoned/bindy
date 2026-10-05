// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Bindy binary entry point: CLI, logging, the Kubernetes client, the metrics
//! server, the leader-election handoff, and the controllers from each
//! controller crate (ADR-0009, roadmap 01 Phase F).
//!
//! For full CLI documentation, environment variables, and the Scout guide, see the
//! [Bindy documentation](https://firestoned.github.io/bindy/).

mod cli;

use anyhow::Result;
use axum::{routing::get, Router};
use bindy_api::constants::{
    METRICS_SERVER_BIND_ADDRESS, METRICS_SERVER_PATH, METRICS_SERVER_PORT, TOKIO_WORKER_THREADS,
};
use bindy_controller_sdk::context::Context;
use bindy_controller_sdk::leader::{acquire_leadership, leadership_lost, LeaderElectionConfig};
use bindy_controller_sdk::namespace_scope::NamespaceScope;
use bindy_controller_sdk::shutdown::{self, supervise, ShutdownSignal};
use bindy_controller_sdk::{metrics, rate_limit};
use clap::{CommandFactory, Parser};
use cli::{Cli, Commands};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

fn main() -> Result<()> {
    // Install ring as the default TLS crypto provider. Both ring (via hickory-client/dnssec-ring)
    // and aws-lc-rs (via reqwest) are compiled in as transitive dependencies, so rustls 0.23+
    // requires an explicit provider to be installed before any TLS operation.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install ring CryptoProvider");

    let cli = Cli::parse();

    // Shell completion and version output are synchronous: no Tokio runtime.
    // render_version() derives from CARGO_PKG_VERSION, matching --version exactly.
    match cli.command {
        Commands::Completion { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "bindy", &mut std::io::stdout());
            return Ok(());
        }
        Commands::Version => {
            print!("{}", Cli::command().render_version());
            return Ok(());
        }
        _ => {}
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(TOKIO_WORKER_THREADS)
        .thread_name(cli.command.thread_name())
        .enable_all()
        .build()?;

    runtime.block_on(async {
        initialize_logging();
        match cli.command {
            Commands::Run => run_operator().await,
            command => cli::run(command).await,
        }
    })
}

/// Initialize logging with custom format
///
/// Respects `RUST_LOG` environment variable if set, otherwise defaults to INFO level.
/// Respects `RUST_LOG_FORMAT` environment variable for output format (json or text).
fn initialize_logging() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let log_format = std::env::var("RUST_LOG_FORMAT").unwrap_or_else(|_| "text".to_string());

    let builder = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_file(true)
        .with_line_number(true)
        .with_thread_names(true)
        .with_target(false);
    if log_format.eq_ignore_ascii_case("json") {
        builder.json().init();
    } else {
        builder.with_ansi(true).compact().init();
    }

    debug!("Logging initialized with file and line number tracking");
}

/// Start the Prometheus metrics HTTP server on the configured address and
/// path (default `0.0.0.0:8080/metrics`).
fn start_metrics_server() -> tokio::task::JoinHandle<()> {
    info!(
        bind_address = METRICS_SERVER_BIND_ADDRESS,
        port = METRICS_SERVER_PORT,
        path = METRICS_SERVER_PATH,
        "Starting Prometheus metrics HTTP server"
    );

    tokio::spawn(async move {
        async fn metrics_handler() -> String {
            metrics::gather_metrics().unwrap_or_else(|e| {
                error!("Failed to gather metrics: {}", e);
                String::from("# Error gathering metrics\n")
            })
        }

        let app = Router::new().route(METRICS_SERVER_PATH, get(metrics_handler));
        let bind_addr = format!("{METRICS_SERVER_BIND_ADDRESS}:{METRICS_SERVER_PORT}");
        let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => listener,
            Err(e) => {
                error!("Failed to bind metrics server to {bind_addr}: {e}");
                return;
            }
        };

        info!("Metrics server listening on http://{bind_addr}{METRICS_SERVER_PATH}");
        if let Err(e) = axum::serve(listener, app).await {
            error!("Metrics server error: {e}");
        }
    })
}

/// Resolve on SIGINT (Ctrl+C) or SIGTERM (Kubernetes deleting the pod).
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!("Cannot listen for SIGINT: {e}");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(e) => {
                error!("Cannot listen for SIGTERM: {e}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("Received SIGINT (Ctrl+C), draining controllers"),
        () = terminate => info!("Received SIGTERM (pod termination), draining controllers"),
    }
}

/// `bindy run`: the operator.
async fn run_operator() -> Result<()> {
    info!("Starting BIND9 DNS Operator");

    // Client-side rate limiting via tower middleware (ADR-0005): kube-rs has
    // no QPS/burst fields on Config, so the limiter lives in the client stack.
    let limits = rate_limit::RateLimitConfig::from_env();
    let client = rate_limit::build_rate_limited_client(kube::Config::infer().await?, &limits)?;

    // The shared context: one watch per cached kind (ADR-0009 §3) and the
    // shutdown signal every controller drains on (§5).
    let (trigger, shutdown) = shutdown::channel();
    let ctx = Arc::new(Context::new(
        client.clone(),
        NamespaceScope::from_env(),
        shutdown.clone(),
    )?);
    let _metrics_server = start_metrics_server();

    let leader_election = LeaderElectionConfig::from_env();
    let lease_lost = Arc::new(AtomicBool::new(false));
    let _leadership = if leader_election.enabled {
        info!(
            lease_name = %leader_election.lease_name,
            lease_namespace = %leader_election.lease_namespace,
            identity = %leader_election.identity,
            lease_duration_secs = leader_election.lease_duration,
            renew_deadline_secs = leader_election.renew_deadline,
            "Leader election enabled, waiting to acquire leadership..."
        );
        let leadership = tokio::select! {
            leadership = acquire_leadership(client, &leader_election) => leadership?,
            () = shutdown_signal() => return Ok(()),
        };
        info!("🎉 Leadership acquired! Starting controllers...");

        // Losing the lease drains the controllers, then the process exits
        // non-zero so Kubernetes restarts it as a follower.
        let leader_rx = leadership.leader_rx.clone();
        let (trigger, lost) = (trigger.clone(), lease_lost.clone());
        tokio::spawn(async move {
            match leadership_lost(leader_rx).await {
                Ok(()) => warn!("Leadership lost! Draining controllers..."),
                Err(e) => error!("Leadership monitor error: {e:?}"),
            }
            lost.store(true, Ordering::SeqCst);
            trigger.fire();
        });
        Some(leadership)
    } else {
        warn!("Leader election DISABLED - running without high availability");
        None
    };

    tokio::spawn(async move {
        shutdown_signal().await;
        trigger.fire();
    });

    run_controllers(ctx, shutdown).await?;

    if lease_lost.load(Ordering::SeqCst) {
        anyhow::bail!("Leadership lost - stepping down");
    }
    info!("Graceful shutdown completed successfully");
    Ok(())
}

/// Run every controller crate's entry point until the shutdown signal fires
/// and they have all drained. A controller that stops on its own, or fails,
/// fails the whole operator (see [`supervise`]).
async fn run_controllers(ctx: Arc<Context>, shutdown: ShutdownSignal) -> Result<()> {
    futures::try_join!(
        supervise(
            "Bind9Cluster/ClusterBind9Provider",
            bindy_controller_cluster::controller(ctx.clone()),
            shutdown.clone(),
        ),
        supervise(
            "Bind9Instance",
            bindy_controller_instance::controller(ctx.clone()),
            shutdown.clone(),
        ),
        supervise(
            "DNSZone",
            bindy_controller_zone::controller(ctx.clone()),
            shutdown.clone(),
        ),
        supervise(
            "DNS record",
            bindy_controller_records::controller(ctx),
            shutdown,
        ),
    )?;
    Ok(())
}

// Tests are in main_tests.rs
#[cfg(test)]
mod main_tests;
