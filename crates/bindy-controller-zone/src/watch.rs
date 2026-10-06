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
//! - a record its `recordsFrom` selectors match changes;
//! - another zone claiming the same zone name changes or is deleted, which
//!   is what a `DuplicateZone` loser waits on.
//!
//! There is no periodic resync (ADR-0016): a converged or waiting zone is
//! reconciled again only on one of these events, a degraded one retries with
//! the per-object backoff, and a signed zone schedules one wake at its next
//! KSK rollover.
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
use crate::dnszone::types::{ZoneOutcome, REASON_DUPLICATE_ZONE};
use crate::dnszone::{delete_dnszone, discovery::zones_configured_on_instance, reconcile_dnszone};
use crate::labels::FINALIZER_DNS_ZONE;
use bindy_controller_sdk::context::{Context, RecordKind};
use bindy_controller_sdk::error::{converged_action, error_policy, retry_action, ReconcileError};
use bindy_controller_sdk::metrics;
use bindy_controller_sdk::namespace_scope::owned_targets;
use bindy_controller_sdk::reconcile::{finalizer_error, scheduled_action};
use bindy_controller_sdk::watch::{changed_only, primary_predicate};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Endpoints;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::{controller::Action, finalizer, Controller, WatchStreamExt};
use kube::{Api, ResourceExt};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// The `Ready` condition type on a `DNSZone`.
const CONDITION_TYPE_READY: &str = "Ready";

/// The part of a zone the duplicate-zone mapper reads: its zone name and
/// whether it is being deleted. Zone status writes leave it unchanged, so
/// they do not wake the zones in conflict with it.
fn zone_name_key(zone: &DNSZone) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    zone.spec.zone_name.hash(&mut hasher);
    zone.metadata.deletion_timestamp.is_some().hash(&mut hasher);
    hasher.finish()
}

/// Whether a zone reports it lost a zone-name conflict.
fn reports_duplicate(zone: &DNSZone) -> bool {
    zone.status
        .as_ref()
        .and_then(|status| {
            status
                .conditions
                .iter()
                .find(|condition| condition.r#type == CONDITION_TYPE_READY)
        })
        .is_some_and(|condition| condition.reason.as_deref() == Some(REASON_DUPLICATE_ZONE))
}

/// The zones a change or deletion of `changed` must wake: every other zone
/// claiming the same zone name, and every zone reporting `DuplicateZone`.
///
/// This is the event a `DuplicateZone` loser waits on (ADR-0016): when the
/// winning zone is deleted or renamed, the loser re-runs its duplicate check.
/// A zone that changed its own name is only seen with the new name, so the
/// zones it blocked are found by their `DuplicateZone` condition instead.
/// Pure: no I/O (ADR-0009 §5).
///
/// # Arguments
///
/// * `zones` - Every zone in the store
/// * `changed` - The zone that changed or was deleted
///
/// # Returns
///
/// References to the zones to reconcile, `changed` itself excluded.
pub(crate) fn zones_contending_for_name(
    zones: &[Arc<DNSZone>],
    changed: &DNSZone,
) -> Vec<ObjectRef<DNSZone>> {
    let changed_namespace = changed.namespace();
    let changed_name = changed.name_any();
    zones
        .iter()
        .filter(|zone| !(zone.namespace() == changed_namespace && zone.name_any() == changed_name))
        .filter(|zone| zone.spec.zone_name == changed.spec.zone_name || reports_duplicate(zone))
        .filter_map(|zone| {
            let namespace = zone.namespace()?;
            Some(ObjectRef::new(&zone.name_any()).within(&namespace))
        })
        .collect()
}

/// The controller `Action` for a zone reconcile's outcome (ADR-0016).
///
/// # Arguments
///
/// * `zone` - The zone that was reconciled (keys the backoff)
/// * `outcome` - How the reconcile ended
///
/// # Returns
///
/// `await_change` for a converged zone (clearing its backoff) or a waiting
/// one; a capped scheduled wake for a converged zone with a pending KSK
/// rollover; a backing-off requeue for a retry.
#[must_use]
pub(crate) fn action_for_zone_outcome(zone: &DNSZone, outcome: &ZoneOutcome) -> Action {
    match outcome {
        ZoneOutcome::Converged { next_wake: None } => converged_action(zone),
        ZoneOutcome::Converged {
            next_wake: Some(delay),
        } => {
            let _ = converged_action(zone);
            scheduled_action(*delay)
        }
        ZoneOutcome::Waiting { .. } => Action::await_change(),
        ZoneOutcome::Retry { .. } => retry_action(zone),
    }
}

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
    let stores_for_duplicates = ctx.stores.clone();
    let stores_for_duplicates_cached = ctx.stores.clone();

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
        })
        // A DuplicateZone loser waits for the winner to go away or rename
        // (ADR-0016). Filtered on the zone name and deletion, so ordinary
        // status writes do not wake anything.
        .watches_stream(
            changed_only(
                ws.subscribe_all::<DNSZone>(),
                zone_name_key,
                move |zone: &DNSZone| {
                    stores_for_duplicates_cached
                        .get_dnszone(&zone.name_any(), &zone.namespace().unwrap_or_default())
                        .is_some()
                },
            ),
            move |zone| zones_contending_for_name(&stores_for_duplicates.dnszones.state(), &zone),
        );
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
                // The outcome comes from the status this reconcile built; the
                // zone is not re-read to pick the action (ADR-0016).
                let outcome = reconcile_dnszone(ctx.clone(), (*zone).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                match outcome {
                    ZoneOutcome::Converged { next_wake } => {
                        info!("Successfully reconciled DNSZone: {}", zone.name_any());
                        if let Some(delay) = next_wake {
                            debug!(
                                "DNSZone {} wakes at its next KSK rollover in {delay:?}",
                                zone.name_any()
                            );
                        }
                    }
                    ZoneOutcome::Waiting { reason } => {
                        info!(
                            "DNSZone {} is waiting ({reason}); a watch event resumes it",
                            zone.name_any()
                        );
                    }
                    ZoneOutcome::Retry { reason } => {
                        warn!(
                            "DNSZone {} is not converged ({reason}); retrying with backoff",
                            zone.name_any()
                        );
                    }
                }
                Ok(action_for_zone_outcome(zone.as_ref(), &outcome))
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
