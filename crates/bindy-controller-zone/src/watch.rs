// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Watch wiring for the `DNSZone` controller.
//!
//! Event-driven, zone-centric selection: a zone reconciles when
//! - its own spec, finalizers, labels or annotations change (the primary
//!   stream drops its status writes, ADR-0009 §4);
//! - a `Bind9Instance` its `bind9InstancesFrom` selectors match changes;
//! - the `Endpoints` of an instance it is configured on change, which is the
//!   earliest signal that a BIND9 pod was replaced and came back empty;
//! - a record its `recordsFrom` selectors match changes.
//!
//! `Bind9Instance` and `Endpoints` are subscribed across every namespace
//! target: a zone can be served by an instance in another namespace. Refs
//! the mappers resolve to zones in other namespaces are dropped by this
//! controller, whose store holds only its own namespace's zones.

use crate::constants::KIND_DNS_ZONE;
use crate::crd::{
    AAAARecord, ARecord, Bind9Instance, CAARecord, CNAMERecord, DNSZone, MXRecord, NSRecord,
    PTRRecord, SRVRecord, TXTRecord,
};
use crate::dnszone::{delete_dnszone, discovery::zones_configured_on_instance, reconcile_dnszone};
use crate::labels::FINALIZER_DNS_ZONE;
use bindy_controller_sdk::context::{Context, RecordKind};
use bindy_controller_sdk::error::{error_policy, ReconcileError};
use bindy_controller_sdk::metrics;
use bindy_controller_sdk::namespace_scope::owned_targets;
use bindy_controller_sdk::reconcile::finalizer_error;
use bindy_controller_sdk::requeue::{REQUEUE_WHEN_NOT_READY_SECS, REQUEUE_WHEN_READY_SECS};
use bindy_controller_sdk::watch::primary_predicate;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Endpoints;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::{controller::Action, finalizer, Controller, WatchStreamExt};
use kube::{Api, ResourceExt};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// The zones whose `bind9InstancesFrom` selectors match `instance`'s labels.
pub(crate) fn zones_selecting_instance(
    zones: &[Arc<DNSZone>],
    instance: &Bind9Instance,
) -> Vec<ObjectRef<DNSZone>> {
    let Some(instance_labels) = instance.metadata.labels.as_ref() else {
        return vec![];
    };
    zones
        .iter()
        .filter(|zone| {
            zone.spec
                .bind9_instances_from
                .as_ref()
                .is_some_and(|sources| {
                    sources
                        .iter()
                        .any(|source| source.selector.matches(instance_labels))
                })
        })
        .filter_map(|zone| {
            let zone_namespace = zone.namespace()?;
            Some(ObjectRef::new(&zone.name_any()).within(&zone_namespace))
        })
        .collect()
}

/// The zones configured on the instance behind `endpoints`. The Endpoints
/// object shares its name with the instance's Service, which shares its
/// name with the instance.
fn zones_for_endpoints(zones: &[Arc<DNSZone>], endpoints: &Endpoints) -> Vec<ObjectRef<DNSZone>> {
    let Some(instance_namespace) = endpoints.namespace() else {
        return vec![];
    };
    let instance_name = endpoints.name_any();
    let zones: Vec<DNSZone> = zones.iter().map(|zone| (**zone).clone()).collect();
    zones_configured_on_instance(&zones, &instance_namespace, &instance_name)
        .into_iter()
        .map(|(zone_namespace, zone_name)| ObjectRef::new(&zone_name).within(&zone_namespace))
        .collect()
}

/// Reconcile the zones whose `recordsFrom` selectors match a record of kind
/// `T` when it changes. One call per record kind replaces the nine identical
/// closures this controller used to carry.
fn watch_records<T: RecordKind>(
    controller: Controller<DNSZone>,
    ctx: &Arc<Context>,
    target: Option<&str>,
) -> Controller<DNSZone> {
    let stores = ctx.stores.clone();
    controller.watches_stream(ctx.watch.subscribe::<T>(target), move |record| {
        let Some(namespace) = record.namespace() else {
            return vec![];
        };
        stores
            .dnszones_selecting_record(record.labels(), &namespace)
            .into_iter()
            .map(|(name, ns)| ObjectRef::new(&name).within(&ns))
            .collect()
    })
}

/// Run one `DNSZone` controller per namespace target. Cluster-wide mode
/// yields exactly one.
pub(crate) async fn run_dnszone_controllers(ctx: Arc<Context>) -> anyhow::Result<()> {
    info!("Starting DNSZone controller");
    let targets = owned_targets(&ctx.namespace_scope);
    futures::future::join_all(
        targets
            .into_iter()
            .map(|target| run_dnszone_controller(ctx.clone(), target)),
    )
    .await;
    Ok(())
}

async fn run_dnszone_controller(ctx: Arc<Context>, target: Option<String>) {
    debug!(
        namespace = target.as_deref().unwrap_or("<all>"),
        "Starting DNSZone controller"
    );

    let ws = ctx.watch.clone();
    let target = target.as_deref();
    let stores_for_endpoints = ctx.stores.clone();
    let stores_for_instances = ctx.stores.clone();

    // The controller's own status writes do not retrigger it: the primary
    // stream passes generation, finalizer, label and annotation changes only.
    // That is what the old 2-second rate limiter in the reconcile wrapper was
    // standing in for, so it is gone.
    let primary = ws
        .subscribe::<DNSZone>(target)
        .predicate_filter(primary_predicate(), Default::default());

    let controller = Controller::for_stream(primary, ws.store::<DNSZone>(target))
        .watches_stream(ws.subscribe_all::<Endpoints>(), move |endpoints| {
            zones_for_endpoints(&stores_for_endpoints.dnszones.state(), &endpoints)
        })
        .watches_stream(ws.subscribe_all::<Bind9Instance>(), move |instance| {
            zones_selecting_instance(&stores_for_instances.dnszones.state(), &instance)
        });
    let controller = watch_records::<ARecord>(controller, &ctx, target);
    let controller = watch_records::<AAAARecord>(controller, &ctx, target);
    let controller = watch_records::<TXTRecord>(controller, &ctx, target);
    let controller = watch_records::<CNAMERecord>(controller, &ctx, target);
    let controller = watch_records::<MXRecord>(controller, &ctx, target);
    let controller = watch_records::<NSRecord>(controller, &ctx, target);
    let controller = watch_records::<SRVRecord>(controller, &ctx, target);
    let controller = watch_records::<CAARecord>(controller, &ctx, target);
    let controller = watch_records::<PTRRecord>(controller, &ctx, target);

    controller
        .graceful_shutdown_on(ctx.shutdown.wait())
        .run(reconcile_dnszone_wrapper, error_policy, ctx.clone())
        .for_each(|_| futures::future::ready(()))
        .await;
}

/// Whether a zone's status says it is Ready and not Degraded.
fn zone_is_ready(zone: &DNSZone) -> bool {
    let condition_true = |kind: &str| {
        zone.status
            .as_ref()
            .and_then(|status| status.conditions.iter().find(|c| c.r#type == kind))
            .is_some_and(|condition| condition.status == "True")
    };
    condition_true("Ready") && !condition_true("Degraded")
}

async fn reconcile_dnszone_wrapper(
    dnszone: Arc<DNSZone>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcileError> {
    let start = std::time::Instant::now();
    // No shared Bind9Manager here on purpose: every bindcar call the DNSZone
    // reconciler makes resolves a manager for the specific instance it is
    // addressing, so it picks up that instance's TLS configuration.
    let namespace = dnszone.namespace().unwrap_or_default();
    let api: Api<DNSZone> = Api::namespaced(ctx.client.clone(), &namespace);

    let result = finalizer(&api, FINALIZER_DNS_ZONE, dnszone, |event| async {
        match event {
            finalizer::Event::Apply(zone) => {
                reconcile_dnszone(ctx.clone(), (*zone).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled DNSZone: {}", zone.name_any());

                // Re-fetch for the status reconcile_dnszone just wrote: a zone
                // that is degraded or not ready yet is checked again sooner.
                let updated_zone = api
                    .get(&zone.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;
                let requeue_secs = if zone_is_ready(&updated_zone) {
                    REQUEUE_WHEN_READY_SECS
                } else {
                    REQUEUE_WHEN_NOT_READY_SECS
                };
                debug!(
                    "DNSZone {} requeues in {requeue_secs}s",
                    updated_zone.name_any()
                );
                Ok(Action::requeue(Duration::from_secs(requeue_secs)))
            }
            finalizer::Event::Cleanup(zone) => {
                delete_dnszone(ctx.clone(), (*zone).clone())
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

    result.map_err(|e| finalizer_error(KIND_DNS_ZONE, e))
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod watch_tests;
