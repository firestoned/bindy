// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

use anyhow::Result;
use bindy::{
    bind9::Bind9Manager,
    crd::{
        AAAARecord, ARecord, Bind9Cluster, Bind9Instance, CAARecord, CNAMERecord, DNSZone,
        MXRecord, NSRecord, SRVRecord, TXTRecord,
    },
    reconcilers::{
        reconcile_a_record, reconcile_aaaa_record, reconcile_bind9cluster, reconcile_bind9instance,
        reconcile_caa_record, reconcile_cname_record, reconcile_dnszone, reconcile_mx_record,
        reconcile_ns_record, reconcile_srv_record, reconcile_txt_record,
    },
};
use futures::StreamExt;
use kube::{
    runtime::{controller::Action, watcher::Config, Controller},
    Api, Client, ResourceExt,
};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct ReconcileError(#[from] anyhow::Error);

fn main() -> Result<()> {
    // Build Tokio runtime with custom thread names
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("bindy-controller")
        .enable_all()
        .build()?;

    runtime.block_on(async_main())
}

async fn async_main() -> Result<()> {
    // Initialize logging with custom format
    // Format: timestamp file:line LEVEL message
    // Example: 2025-11-29T23:45:00.123456Z main.rs:49 INFO Starting BIND9 DNS Controller
    //
    // Respects RUST_LOG environment variable if set, otherwise defaults to INFO level
    // Example: RUST_LOG=debug cargo run
    //
    // Respects RUST_LOG_FORMAT environment variable for output format
    // Example: RUST_LOG_FORMAT=json cargo run
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

    // Initialize Kubernetes client
    debug!("Initializing Kubernetes client");
    let client = Client::try_default().await?;
    debug!("Kubernetes client initialized successfully");

    // Create BIND9 manager (no longer needs zones directory - uses rndc protocol)
    debug!("Creating BIND9 manager");
    let bind9_manager = Arc::new(Bind9Manager::new());
    debug!("BIND9 manager created");

    info!("Starting all controllers");

    // Run controllers concurrently
    // Controllers should never exit - if one fails, we log it and exit the main process
    tokio::select! {
        result = run_bind9cluster_controller(client.clone()) => {
            error!("CRITICAL: Bind9Cluster controller exited unexpectedly: {:?}", result);
            result?;
            anyhow::bail!("Bind9Cluster controller exited unexpectedly without error")
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

/// Run the `DNSZone` controller
async fn run_dnszone_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    info!("Starting DNSZone controller");

    let api = Api::<DNSZone>::all(client.clone());

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

    Controller::new(api, Config::default())
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

    Controller::new(api, Config::default())
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

    Controller::new(api, Config::default())
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

    Controller::new(api, Config::default())
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

    Controller::new(api, Config::default())
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

    Controller::new(api, Config::default())
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

    Controller::new(api, Config::default())
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

    Controller::new(api, Config::default())
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
    debug!(
        cluster_name = %cluster.name_any(),
        namespace = ?cluster.namespace(),
        "Reconcile wrapper called for Bind9Cluster"
    );

    match reconcile_bind9cluster((*ctx).clone(), (*cluster).clone()).await {
        Ok(()) => {
            info!(
                "Successfully reconciled Bind9Cluster: {}",
                cluster.name_any()
            );

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
            Err(e.into())
        }
    }
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
    match reconcile_bind9instance((*ctx).clone(), (*instance).clone()).await {
        Ok(()) => {
            info!(
                "Successfully reconciled Bind9Instance: {}",
                instance.name_any()
            );

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
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `DNSZone`
async fn reconcile_dnszone_wrapper(
    dnszone: Arc<DNSZone>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_dnszone(ctx.0.clone(), (*dnszone).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled DNSZone: {}", dnszone.name_any());

            // Check if zone is ready to determine requeue interval
            let is_ready = dnszone
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

            if is_ready {
                // Zone is ready, check less frequently (5 minutes)
                Ok(Action::requeue(Duration::from_secs(300)))
            } else {
                // Zone is not ready, check more frequently (30 seconds)
                Ok(Action::requeue(Duration::from_secs(30)))
            }
        }
        Err(e) => {
            error!("Failed to reconcile DNSZone: {}", e);
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `ARecord`
async fn reconcile_arecord_wrapper(
    record: Arc<ARecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_a_record(ctx.0.clone(), (*record).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled ARecord: {}", record.name_any());

            // Check if record is ready to determine requeue interval
            let is_ready = record
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

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
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `TXTRecord`
async fn reconcile_txtrecord_wrapper(
    record: Arc<TXTRecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_txt_record(ctx.0.clone(), (*record).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled TXTRecord: {}", record.name_any());

            // Check if record is ready to determine requeue interval
            let is_ready = record
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

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
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `AAAARecord`
async fn reconcile_aaaarecord_wrapper(
    record: Arc<AAAARecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_aaaa_record(ctx.0.clone(), (*record).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled AAAARecord: {}", record.name_any());

            // Check if record is ready to determine requeue interval
            let is_ready = record
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

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
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `CNAMERecord`
async fn reconcile_cnamerecord_wrapper(
    record: Arc<CNAMERecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_cname_record(ctx.0.clone(), (*record).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled CNAMERecord: {}", record.name_any());

            // Check if record is ready to determine requeue interval
            let is_ready = record
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

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
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `MXRecord`
async fn reconcile_mxrecord_wrapper(
    record: Arc<MXRecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_mx_record(ctx.0.clone(), (*record).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled MXRecord: {}", record.name_any());

            // Check if record is ready to determine requeue interval
            let is_ready = record
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

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
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `NSRecord`
async fn reconcile_nsrecord_wrapper(
    record: Arc<NSRecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_ns_record(ctx.0.clone(), (*record).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled NSRecord: {}", record.name_any());

            // Check if record is ready to determine requeue interval
            let is_ready = record
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

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
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `SRVRecord`
async fn reconcile_srvrecord_wrapper(
    record: Arc<SRVRecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_srv_record(ctx.0.clone(), (*record).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled SRVRecord: {}", record.name_any());

            // Check if record is ready to determine requeue interval
            let is_ready = record
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

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
            Err(e.into())
        }
    }
}

/// Reconcile wrapper for `CAARecord`
async fn reconcile_caarecord_wrapper(
    record: Arc<CAARecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    match reconcile_caa_record(ctx.0.clone(), (*record).clone(), &ctx.1).await {
        Ok(()) => {
            info!("Successfully reconciled CAARecord: {}", record.name_any());

            // Check if record is ready to determine requeue interval
            let is_ready = record
                .status
                .as_ref()
                .and_then(|status| status.conditions.first())
                .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

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
    Action::requeue(Duration::from_secs(30))
}

/// Error policy for `Bind9Cluster` controller
fn error_policy_cluster(
    _resource: Arc<impl std::fmt::Debug>,
    _err: &ReconcileError,
    _ctx: Arc<Client>,
) -> Action {
    Action::requeue(Duration::from_secs(30))
}

/// Error policy for `Bind9Instance` controller
fn error_policy_instance(
    _resource: Arc<impl std::fmt::Debug>,
    _err: &ReconcileError,
    _ctx: Arc<Client>,
) -> Action {
    Action::requeue(Duration::from_secs(30))
}
