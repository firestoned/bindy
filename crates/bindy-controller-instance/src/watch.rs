// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Watch wiring for the `Bind9Instance` controller.
//!
//! Every mapper here is pure: it returns `ObjectRef`s and does no I/O
//! (ADR-0009 §5). In particular, a `DNSZone` change no longer spawns a task
//! that fetches and patches instances outside the controller: it enqueues
//! the instances the zone selected, and their reconcile refreshes
//! `status.zones` with the controller's retries, backoff and metrics. Only a
//! change to what `status.zones` is built from does so ([`zone_selection_key`]).

use crate::bind9instance::reconcile_bind9instance;
use crate::constants::KIND_BIND9_INSTANCE;
use crate::crd::{Bind9Cluster, Bind9Instance, ClusterBind9Provider, DNSZone};
use bindy_controller_sdk::context::Context;
use bindy_controller_sdk::error::{error_policy, ReconcileError};
use bindy_controller_sdk::namespace_scope::{owned_targets, scoped_namespaced_api};
use bindy_controller_sdk::reconcile::instrumented;
use bindy_controller_sdk::watch::changed_only;
use futures::StreamExt;
use k8s_openapi::api::core::v1::{ConfigMap, Secret, Service, ServiceAccount};
use kube::runtime::reflector::ObjectRef;
use kube::runtime::{controller::Action, watcher, Controller};
use kube::ResourceExt;
use std::sync::Arc;
use tracing::{debug, info};

/// The part of a zone an instance's `status.zones` is built from: the zone's
/// identity and `spec.zoneName`, the set of instances in
/// `status.bind9Instances`, and whether it is being deleted.
///
/// The DNSZone stream is filtered on this key ([`changed_only`]), so the
/// timestamps other controllers stamp into a zone's status (every record
/// reconcile writes `status.records[].lastReconciledAt`) do not fan out into a
/// full reconcile of every instance the zone selected.
pub(crate) fn zone_selection_key(zone: &DNSZone) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut selected: Vec<(&str, &str)> = zone
        .status
        .as_ref()
        .map(|status| {
            status
                .bind9_instances
                .iter()
                .map(|i| (i.namespace.as_str(), i.name.as_str()))
                .collect()
        })
        .unwrap_or_default();
    selected.sort_unstable();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    zone.spec.zone_name.hash(&mut hasher);
    selected.hash(&mut hasher);
    zone.metadata.deletion_timestamp.is_some().hash(&mut hasher);
    hasher.finish()
}

/// The instances a zone selected (`status.bind9Instances`), limited to
/// `target` when this controller watches one namespace.
pub(crate) fn instances_selected_by_zone(
    zone: &DNSZone,
    target: Option<&str>,
) -> Vec<ObjectRef<Bind9Instance>> {
    let Some(status) = zone.status.as_ref() else {
        return vec![];
    };
    status
        .bind9_instances
        .iter()
        .filter(|selected| target.is_none_or(|ns| selected.namespace == ns))
        .map(|selected| ObjectRef::new(&selected.name).within(&selected.namespace))
        .collect()
}

/// The instances that reference `cluster` (same namespace, by `clusterRef`).
///
/// An instance inherits configuration resolved against the live cluster at
/// reconcile time, so a cluster change must reach those instances now rather
/// than on their next requeue.
pub(crate) fn instances_of_cluster(
    instances: &[Arc<Bind9Instance>],
    cluster: &Bind9Cluster,
) -> Vec<ObjectRef<Bind9Instance>> {
    let cluster_name = cluster.name_any();
    let Some(cluster_namespace) = cluster.namespace() else {
        return vec![];
    };
    instances
        .iter()
        .filter(|instance| {
            instance.spec.cluster_ref == cluster_name
                && instance.namespace().as_deref() == Some(cluster_namespace.as_str())
        })
        .map(|instance| ObjectRef::from_obj(instance.as_ref()))
        .collect()
}

/// The instances that reference the cluster-scoped `provider`. A provider's
/// instances can live in any namespace, so only the name is matched.
pub(crate) fn instances_of_provider(
    instances: &[Arc<Bind9Instance>],
    provider: &ClusterBind9Provider,
) -> Vec<ObjectRef<Bind9Instance>> {
    let provider_name = provider.name_any();
    instances
        .iter()
        .filter(|instance| instance.spec.cluster_ref == provider_name)
        .map(|instance| ObjectRef::from_obj(instance.as_ref()))
        .collect()
}

/// Run one `Bind9Instance` controller per namespace target. Cluster-wide mode
/// yields exactly one.
pub(crate) async fn run_bind9instance_controllers(ctx: Arc<Context>) -> anyhow::Result<()> {
    info!("Starting Bind9Instance controller");
    let targets = owned_targets(&ctx.namespace_scope);
    futures::future::join_all(
        targets
            .into_iter()
            .map(|target| run_bind9instance_controller(ctx.clone(), target)),
    )
    .await;
    Ok(())
}

async fn run_bind9instance_controller(ctx: Arc<Context>, target: Option<String>) {
    debug!(
        namespace = target.as_deref().unwrap_or("<all>"),
        "Starting Bind9Instance controller"
    );

    let client = ctx.client.clone();
    let ws = ctx.watch.clone();
    let target_ns = target.clone();
    let stores_for_cluster_watch = ctx.stores.clone();
    let stores_for_provider_watch = ctx.stores.clone();

    // An instance's status.zones lists the zones in its own namespace that
    // selected it, so only this namespace's zones matter, and only when the
    // part status.zones is built from changes (or the zone is deleted).
    let zone_store = ctx.stores.clone();
    let zone_selection_changes = changed_only(
        ws.subscribe::<DNSZone>(target.as_deref()),
        zone_selection_key,
        move |zone: &DNSZone| {
            zone_store
                .get_dnszone(&zone.name_any(), &zone.namespace().unwrap_or_default())
                .is_some()
        },
    );

    // Bind9Instance, Deployment, DNSZone, Bind9Cluster and ClusterBind9Provider
    // come from the shared WatchSet; the other owned kinds are watched only
    // here and never cached, so they keep their own watches (ADR-0009 §3).
    // Owning the Deployment already triggers a reconcile when pod status
    // changes, without a chatty pod watch.
    Controller::for_stream(
        ws.subscribe::<Bind9Instance>(target.as_deref()),
        ws.store::<Bind9Instance>(target.as_deref()),
    )
    .owns(
        scoped_namespaced_api::<ServiceAccount>(&client, target.as_deref()),
        watcher::Config::default(),
    )
    .owns(
        scoped_namespaced_api::<Secret>(&client, target.as_deref()),
        watcher::Config::default(),
    )
    .owns(
        scoped_namespaced_api::<ConfigMap>(&client, target.as_deref()),
        watcher::Config::default(),
    )
    .owns_stream(ws.subscribe::<k8s_openapi::api::apps::v1::Deployment>(target.as_deref()))
    .owns(
        scoped_namespaced_api::<Service>(&client, target.as_deref()),
        watcher::Config::default(),
    )
    .watches_stream(zone_selection_changes, move |zone| {
        instances_selected_by_zone(&zone, target_ns.as_deref())
    })
    .watches_stream(
        ws.subscribe::<Bind9Cluster>(target.as_deref()),
        move |cluster| {
            instances_of_cluster(&stores_for_cluster_watch.bind9_instances.state(), &cluster)
        },
    )
    .watches_stream(
        ws.subscribe::<ClusterBind9Provider>(None),
        move |provider| {
            instances_of_provider(
                &stores_for_provider_watch.bind9_instances.state(),
                &provider,
            )
        },
    )
    .graceful_shutdown_on(ctx.shutdown.wait())
    .run(reconcile_bind9instance_wrapper, error_policy, ctx)
    .for_each(|_| futures::future::ready(()))
    .await;
}

async fn reconcile_bind9instance_wrapper(
    instance: Arc<Bind9Instance>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcileError> {
    let name = instance.name_any();
    info!("Reconciling instance {name}");
    instrumented(
        KIND_BIND9_INSTANCE,
        &name,
        Box::pin(reconcile_bind9instance(ctx, (*instance).clone())),
    )
    .await
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod watch_tests;
