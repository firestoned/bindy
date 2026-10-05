// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Watch wiring for the cluster controllers: every stream comes from the
//! shared `WatchSet` (ADR-0009 §3), and every controller drains on the
//! context's shutdown signal (§5).

use crate::bind9cluster::reconcile_bind9cluster;
use crate::clusterbind9provider::reconcile_clusterbind9provider;
use crate::constants::{KIND_BIND9_CLUSTER, KIND_CLUSTER_BIND9_PROVIDER};
use crate::crd::{Bind9Cluster, Bind9Instance, ClusterBind9Provider};
use bindy_controller_sdk::context::Context;
use bindy_controller_sdk::error::{error_policy, ReconcileError};
use bindy_controller_sdk::namespace_scope::owned_targets;
use bindy_controller_sdk::reconcile::instrumented;
use futures::StreamExt;
use kube::runtime::{controller::Action, Controller};
use kube::ResourceExt;
use std::sync::Arc;
use tracing::{debug, info};

/// Run the `ClusterBind9Provider` controller.
///
/// The provider is cluster-scoped, so its primary stream is the one
/// cluster-wide shard; the `Bind9Cluster`s it owns are watched per namespace
/// target.
pub(crate) async fn run_clusterbind9provider_controller(ctx: Arc<Context>) -> anyhow::Result<()> {
    info!("Starting ClusterBind9Provider controller");

    let ws = ctx.watch.clone();
    let mut controller = Controller::for_stream(
        ws.subscribe::<ClusterBind9Provider>(None),
        ws.store::<ClusterBind9Provider>(None),
    );
    for target in owned_targets(&ctx.namespace_scope) {
        controller = controller.owns_stream(ws.subscribe::<Bind9Cluster>(target.as_deref()));
    }

    controller
        .graceful_shutdown_on(ctx.shutdown.wait())
        .run(reconcile_clusterbind9provider_wrapper, error_policy, ctx)
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

async fn reconcile_clusterbind9provider_wrapper(
    provider: Arc<ClusterBind9Provider>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcileError> {
    let name = provider.name_any();
    debug!(cluster_name = %name, "Reconcile wrapper called for ClusterBind9Provider");
    instrumented(
        KIND_CLUSTER_BIND9_PROVIDER,
        &name,
        Box::pin(reconcile_clusterbind9provider(ctx, (*provider).clone())),
    )
    .await
}

/// Run one `Bind9Cluster` controller per namespace target.
///
/// A `Controller` watches one namespace or all of them; cluster-wide mode
/// yields exactly one controller.
pub(crate) async fn run_bind9cluster_controllers(ctx: Arc<Context>) -> anyhow::Result<()> {
    info!("Starting Bind9Cluster controller");
    let targets = owned_targets(&ctx.namespace_scope);
    futures::future::join_all(
        targets
            .into_iter()
            .map(|target| run_bind9cluster_controller(ctx.clone(), target)),
    )
    .await;
    Ok(())
}

async fn run_bind9cluster_controller(ctx: Arc<Context>, target: Option<String>) {
    let ws = ctx.watch.clone();
    let target = target.as_deref();

    Controller::for_stream(
        ws.subscribe::<Bind9Cluster>(target),
        ws.store::<Bind9Cluster>(target),
    )
    .owns_stream(ws.subscribe::<Bind9Instance>(target))
    .graceful_shutdown_on(ctx.shutdown.wait())
    .run(reconcile_bind9cluster_wrapper, error_policy, ctx)
    .for_each(|_| futures::future::ready(()))
    .await;
}

async fn reconcile_bind9cluster_wrapper(
    cluster: Arc<Bind9Cluster>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcileError> {
    let name = cluster.name_any();
    debug!(
        cluster_name = %name,
        namespace = ?cluster.namespace(),
        "Reconcile wrapper called for Bind9Cluster"
    );
    instrumented(
        KIND_BIND9_CLUSTER,
        &name,
        Box::pin(reconcile_bind9cluster(ctx, (*cluster).clone())),
    )
    .await
}
