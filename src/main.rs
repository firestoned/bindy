// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

use anyhow::Result;
use bindy::{
    bind9::Bind9Manager,
    crd::{
        AAAARecord, ARecord, Bind9Instance, CAARecord, CNAMERecord, DNSZone, MXRecord, NSRecord,
        SRVRecord, TXTRecord,
    },
    reconcilers::{
        reconcile_a_record, reconcile_aaaa_record, reconcile_bind9instance, reconcile_caa_record,
        reconcile_cname_record, reconcile_dnszone, reconcile_mx_record, reconcile_ns_record,
        reconcile_srv_record, reconcile_txt_record,
    },
};
use futures::StreamExt;
use kube::{
    runtime::{controller::Action, watcher::Config, Controller},
    Api, Client, ResourceExt,
};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct ReconcileError(#[from] anyhow::Error);

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Initialize Kubernetes client
    let client = Client::try_default().await?;

    // Create BIND9 manager (no longer needs zones directory - uses rndc protocol)
    let bind9_manager = Arc::new(Bind9Manager::new());

    info!("Starting BIND9 DNS Controller");

    // Run controllers concurrently
    tokio::select! {
        result = run_bind9instance_controller(client.clone()) => {
            error!("Bind9Instance controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_dnszone_controller(client.clone(), bind9_manager.clone()) => {
            error!("DNSZone controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_arecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("ARecord controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_aaaarecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("AAAARecord controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_txtrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("TXTRecord controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_cnamerecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CNAMERecord controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_mxrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("MXRecord controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_nsrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("NSRecord controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_srvrecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("SRVRecord controller exited: {:?}", result);
            Err(result.unwrap_err())
        }
        result = run_caarecord_controller(client.clone(), bind9_manager.clone()) => {
            error!("CAARecord controller exited: {:?}", result);
            Err(result.unwrap_err())
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

    let api = Api::<ARecord>::all(client.clone());

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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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
            Ok(Action::requeue(Duration::from_secs(300)))
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

/// Error policy for `Bind9Instance` controller
fn error_policy_instance(
    _resource: Arc<impl std::fmt::Debug>,
    _err: &ReconcileError,
    _ctx: Arc<Client>,
) -> Action {
    Action::requeue(Duration::from_secs(30))
}
