// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The zones-loaded readiness gate (ADR-0017).
//!
//! Every BIND9 pod lists the `bindy.firestoned.io/zones-loaded` condition in
//! `spec.readinessGates`, so Kubernetes keeps it out of its Service until that
//! condition is `True`. This controller sets it. Its primary resource is the
//! Pod (from the shared, label-selected BIND9 Pod watch); for a pod whose
//! containers are ready and whose gate is not yet `True`, it loads every live
//! zone that selects the pod's instance onto that one pod, through the same
//! write paths the `DNSZone` controller uses but with a resolver that
//! addresses only this pod, then sets the condition:
//!
//! - `True` / `NoZones` when no live zone selects the instance;
//! - `True` / `ZonesLoaded` when every zone (and, on a primary, every record
//!   tagged with it) is on the pod;
//! - `True` / `ZonesPartiallyLoaded` when the zones it could not load are
//!   served by no other Ready pod of the instance either (holding the pod
//!   back would protect nothing);
//! - `False` / `ZonesLoadFailed` when a zone it could not load is still
//!   served by another pod of the instance, retried with the per-object
//!   backoff.
//!
//! The gate is a one-way latch per pod: once `True` it is never evaluated
//! again for that pod. A zone created later reaches the running pod through
//! the `DNSZone` controller, as it always did; only a new pod starts gated.
//! The one exception is termination (ADR-0017 decision 6): a pod that gets a
//! `deletionTimestamp` with its gate `True` is set `False` (`PodTerminating`)
//! at once, so it stops being `Ready` and its traffic moves to the remaining
//! Ready pods while `named` drains, instead of when its readiness probe fails
//! after `named` exited.
//!
//! A zone is *live* when its status lists at least one instance as
//! `Configured`: a zone nobody serves yet cannot regress by admitting the pod,
//! and must not hold a shared instance out of Service.
//!
//! Event-driven (ADR-0016): woken by the pod's own events (it appearing, its
//! containers turning ready, its gate condition changing) and by `DNSZone`
//! events that change which live zones select an instance. No timer.

use crate::constants::{
    CONDITION_STATUS_FALSE, CONDITION_STATUS_TRUE, ZONES_LOADED_CONDITION_TYPE,
    ZONES_LOADED_REASON_FAILED, ZONES_LOADED_REASON_INSTANCE_UNKNOWN, ZONES_LOADED_REASON_LOADED,
    ZONES_LOADED_REASON_LOADING, ZONES_LOADED_REASON_NO_ZONES, ZONES_LOADED_REASON_PARTIAL,
    ZONES_LOADED_REASON_TERMINATING,
};
use crate::crd::{Bind9Instance, DNSZone, InstanceStatus, ServerRole};
use crate::labels::K8S_INSTANCE;
use crate::watch::reports_duplicate;
use anyhow::{anyhow, ensure};
use bindy_bind9::bind9::zone_ops::ZonePresence;
use bindy_bind9::instances::{
    cached_endpoints, get_instances_from_zone, instance_allows_zone_namespace,
    pod_containers_ready, EndpointAddress, InstanceResolver,
};
use bindy_controller_sdk::context::Context;
use bindy_controller_sdk::error::{converged_action, error_policy, retry_action, ReconcileError};
use bindy_controller_sdk::metrics;
use bindy_controller_sdk::namespace_scope::owned_targets;
use bindy_controller_sdk::watch::changed_only;
use futures::StreamExt;
use k8s_openapi::api::core::v1::{Pod, PodCondition};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::{Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::Controller;
use kube::{Api, ResourceExt};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// The metrics label of this controller's reconciles.
const KIND_ZONES_GATE: &str = "ZonesLoadedGate";

/// The Service port name of the bindcar API, through which zones are written
/// and their presence checked.
const BINDCAR_PORT_NAME: &str = "http";

/// The longest condition message the gate writes. A pod's status is part of
/// an etcd object; a list of failures across many zones is cut here.
pub(crate) const MAX_GATE_MESSAGE_CHARS: usize = 1024;

/// What the gate controller does with a pod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GateStep {
    /// Nothing to do, ever (no gate, gate already `True`, terminating, or not
    /// one of bindy's instance pods). The reason is for the debug log.
    Done(&'static str),
    /// The pod is terminating and its gate is `True`: set it `False` now, so
    /// the pod stops being `Ready` (and its endpoint stops `serving`) at the
    /// start of its termination rather than when its readiness probe fails
    /// after `named` exited (ADR-0017 decision 6). The one exception to the
    /// one-way latch.
    CloseForTermination,
    /// The pod's containers are not ready yet: it cannot take writes. Its
    /// next Pod event (containers turning ready) wakes the controller.
    WaitForContainers,
    /// Load the instance's zones onto the pod and set the gate.
    Evaluate {
        /// Namespace of the pod and its instance
        instance_namespace: String,
        /// Name of the pod's `Bind9Instance`
        instance_name: String,
        /// The pod's IP, the one endpoint the loads address
        pod_ip: String,
    },
}

/// The pod's zones-loaded condition, if it has one.
fn gate_condition(pod: &Pod) -> Option<&PodCondition> {
    pod.status
        .as_ref()?
        .conditions
        .as_ref()?
        .iter()
        .find(|condition| condition.type_ == ZONES_LOADED_CONDITION_TYPE)
}

/// Whether the pod's spec lists the zones-loaded readiness gate. Pods created
/// before the gate existed do not, and are never touched.
fn has_gate(pod: &Pod) -> bool {
    pod.spec
        .as_ref()
        .and_then(|spec| spec.readiness_gates.as_ref())
        .is_some_and(|gates| {
            gates
                .iter()
                .any(|gate| gate.condition_type == ZONES_LOADED_CONDITION_TYPE)
        })
}

/// Whether the pod's gate condition is `True`.
fn gate_is_open(pod: &Pod) -> bool {
    gate_condition(pod).is_some_and(|condition| condition.status == CONDITION_STATUS_TRUE)
}

/// Decide what to do with a pod. Pure: no I/O.
///
/// A terminating pod whose gate is `True` is closed
/// ([`GateStep::CloseForTermination`]); any other terminating pod is left
/// alone. A pod that is not terminating is evaluated until its gate is
/// `True`, and never again after (the one-way latch).
///
/// # Arguments
/// * `pod` - The pod from the BIND9 Pod store
///
/// # Returns
/// The [`GateStep`] for the pod.
pub(crate) fn gate_step(pod: &Pod) -> GateStep {
    if !has_gate(pod) {
        return GateStep::Done("pod has no zones-loaded readiness gate");
    }
    if pod.metadata.deletion_timestamp.is_some() {
        if gate_is_open(pod) {
            return GateStep::CloseForTermination;
        }
        return GateStep::Done("pod is terminating and its gate is not open");
    }
    if gate_is_open(pod) {
        return GateStep::Done("zones already loaded");
    }
    let Some(instance_name) = pod
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(K8S_INSTANCE))
    else {
        return GateStep::Done("pod has no instance label");
    };
    if !pod_containers_ready(pod) {
        return GateStep::WaitForContainers;
    }
    let Some(pod_ip) = pod.status.as_ref().and_then(|s| s.pod_ip.clone()) else {
        return GateStep::WaitForContainers;
    };
    GateStep::Evaluate {
        instance_namespace: pod.namespace().unwrap_or_default(),
        instance_name: instance_name.clone(),
        pod_ip,
    }
}

/// Whether `zone` selects `instance`: its `bind9InstancesFrom` selector
/// matches the instance's labels and the F-003 namespace gate allows it (same
/// namespace, or the instance's allow-zone-namespaces annotation names the
/// zone's namespace). Mirrors `get_instances_from_zone`.
fn zone_selects_instance(zone: &DNSZone, instance: &Bind9Instance) -> bool {
    let Some(labels) = instance.metadata.labels.as_ref() else {
        return false;
    };
    let matches = zone
        .spec
        .bind9_instances_from
        .iter()
        .flatten()
        .any(|source| source.selector.matches(labels));
    if !matches {
        return false;
    }
    let zone_namespace = zone.namespace().unwrap_or_default();
    instance.namespace().as_deref() == Some(zone_namespace.as_str())
        || instance_allows_zone_namespace(instance, &zone_namespace)
}

/// Whether some instance serves the zone: its status lists at least one
/// instance as `Configured`.
fn zone_is_live(zone: &DNSZone) -> bool {
    zone.status.as_ref().is_some_and(|status| {
        status
            .bind9_instances
            .iter()
            .any(|inst| inst.status == InstanceStatus::Configured)
    })
}

/// The zones that must be on a pod of `instance` before it is Ready: every
/// zone that selects the instance, is live, is not being deleted and is not a
/// `DuplicateZone` loser. Pure: no I/O.
///
/// # Arguments
/// * `zones` - Every zone in the store
/// * `instance` - The pod's `Bind9Instance`
///
/// # Returns
/// The required zones; empty means the pod can be admitted at once.
pub(crate) fn required_zones(
    zones: &[Arc<DNSZone>],
    instance: &Bind9Instance,
) -> Vec<Arc<DNSZone>> {
    zones
        .iter()
        .filter(|zone| zone.metadata.deletion_timestamp.is_none())
        .filter(|zone| !reports_duplicate(zone))
        .filter(|zone| zone_is_live(zone))
        .filter(|zone| zone_selects_instance(zone, instance))
        .cloned()
        .collect()
}

/// Whether the pod's gate condition already says exactly this.
pub(crate) fn condition_matches(pod: &Pod, status: &str, reason: &str, message: &str) -> bool {
    gate_condition(pod).is_some_and(|condition| {
        condition.status == status
            && condition.reason.as_deref() == Some(reason)
            && condition.message.as_deref() == Some(message)
    })
}

/// The `lastTransitionTime` to write: the current one when the status does
/// not change, `now` otherwise.
pub(crate) fn transition_time(pod: &Pod, status: &str, now: &Time) -> Time {
    gate_condition(pod)
        .filter(|condition| condition.status == status)
        .and_then(|condition| condition.last_transition_time.clone())
        .unwrap_or_else(|| now.clone())
}

/// The strategic merge patch of `pods/status` that sets the gate condition.
///
/// `status.conditions` merges by `type`, so the patch replaces this one
/// condition and leaves `Ready`, `ContainersReady` and the rest alone.
pub(crate) fn gate_patch(
    status: &str,
    reason: &str,
    message: &str,
    last_transition_time: &Time,
) -> serde_json::Value {
    serde_json::json!({
        "status": {
            "conditions": [{
                "type": ZONES_LOADED_CONDITION_TYPE,
                "status": status,
                "reason": reason,
                "message": message,
                "lastTransitionTime": last_transition_time,
            }]
        }
    })
}

/// The gate condition (`status`, `reason`, `message`) written on a pod that
/// started terminating (ADR-0017 decision 6). Pure: no I/O.
pub(crate) fn termination_condition() -> (&'static str, &'static str, &'static str) {
    (
        CONDITION_STATUS_FALSE,
        ZONES_LOADED_REASON_TERMINATING,
        "Pod is terminating: removed from its Service at once so traffic moves to the remaining Ready pods while named drains",
    )
}

/// Cut a condition message to [`MAX_GATE_MESSAGE_CHARS`] characters.
pub(crate) fn truncate_message(message: &str) -> String {
    message.chars().take(MAX_GATE_MESSAGE_CHARS).collect()
}

/// The gated, not yet admitted pods of every instance `zone` selects: the
/// pods a change of the zone must wake. Pure: no I/O (ADR-0009 §5).
///
/// # Arguments
/// * `pods` - Every pod in the BIND9 Pod store
/// * `instances` - Every `Bind9Instance` in the store
/// * `zone` - The zone that changed
///
/// # Returns
/// References to the pods to reconcile.
pub(crate) fn gated_pods_for_zone(
    pods: &[Arc<Pod>],
    instances: &[Arc<Bind9Instance>],
    zone: &DNSZone,
) -> Vec<ObjectRef<Pod>> {
    let selected: Vec<(String, String)> = instances
        .iter()
        .filter(|instance| zone_selects_instance(zone, instance))
        .map(|instance| {
            (
                instance.namespace().unwrap_or_default(),
                instance.name_any(),
            )
        })
        .collect();
    if selected.is_empty() {
        return vec![];
    }
    pods.iter()
        .filter(|pod| has_gate(pod) && !gate_is_open(pod))
        .filter(|pod| {
            let namespace = pod.namespace().unwrap_or_default();
            pod.metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get(K8S_INSTANCE))
                .is_some_and(|name| {
                    selected
                        .iter()
                        .any(|(ns, inst)| *ns == namespace && inst == name)
                })
        })
        .filter_map(|pod| {
            let namespace = pod.namespace()?;
            Some(ObjectRef::new(&pod.name_any()).within(&namespace))
        })
        .collect()
}

/// What the gate reads from a pod: whether it has the gate, its containers'
/// readiness, its IP, deletion, and the gate's status (not its reason or
/// message, so the controller's own `False` rewrites do not wake it ahead of
/// its backoff).
pub(crate) fn pod_gate_key(pod: &Pod) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    has_gate(pod).hash(&mut hasher);
    pod_containers_ready(pod).hash(&mut hasher);
    pod.status
        .as_ref()
        .and_then(|s| s.pod_ip.as_deref())
        .hash(&mut hasher);
    pod.metadata.deletion_timestamp.is_some().hash(&mut hasher);
    gate_condition(pod)
        .map(|condition| condition.status.as_str())
        .hash(&mut hasher);
    hasher.finish()
}

/// What the gate reads from a zone: its instance selectors, the instances it
/// is configured on, deletion and whether it lost a zone-name conflict.
/// Record counts and timestamps in its status are ignored.
pub(crate) fn zone_gate_key(zone: &DNSZone) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&zone.spec.bind9_instances_from)
        .unwrap_or_default()
        .hash(&mut hasher);
    let mut configured: Vec<(String, String)> = zone
        .status
        .iter()
        .flat_map(|status| status.bind9_instances.iter())
        .filter(|inst| inst.status == InstanceStatus::Configured)
        .map(|inst| (inst.namespace.clone(), inst.name.clone()))
        .collect();
    configured.sort();
    configured.hash(&mut hasher);
    zone.metadata.deletion_timestamp.is_some().hash(&mut hasher);
    reports_duplicate(zone).hash(&mut hasher);
    hasher.finish()
}

/// What one zone's load attempt on the pod means for the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ZoneLoad {
    /// The zone (and its records) is on the pod.
    Loaded,
    /// The zone could not be loaded, and another Ready pod of the instance
    /// serves it: admitting this pod would lose it. Blocks the gate.
    Blocking(String),
    /// The zone could not be loaded, and no other Ready pod of the instance
    /// serves it either: holding this pod back would protect nothing and
    /// would keep every other zone of the instance out of service. Does not
    /// block; the `DNSZone` controller keeps retrying it.
    NotServedElsewhere(String),
}

/// The gate condition (`status`, `reason`, `message`) for a pod after one
/// round of loads. Pure: no I/O.
///
/// The rule (ADR-0017): the pod is admitted unless a zone it could not load
/// is served by another Ready pod of its instance. A zone failing on every
/// pod of the instance does not block, since no pod serves it today.
///
/// # Arguments
/// * `loads` - One entry per required zone
///
/// # Returns
/// The condition to write. `status` is `"True"` when the pod may be admitted.
pub(crate) fn gate_outcome(loads: &[ZoneLoad]) -> (&'static str, &'static str, String) {
    let total = loads.len();
    let blocking: Vec<&str> = loads
        .iter()
        .filter_map(|load| match load {
            ZoneLoad::Blocking(failure) => Some(failure.as_str()),
            _ => None,
        })
        .collect();
    if !blocking.is_empty() {
        return (
            CONDITION_STATUS_FALSE,
            ZONES_LOADED_REASON_FAILED,
            format!(
                "{}/{total} zone(s) not loaded and still served by another pod of the instance: {}",
                blocking.len(),
                blocking.join("; ")
            ),
        );
    }
    let skipped: Vec<&str> = loads
        .iter()
        .filter_map(|load| match load {
            ZoneLoad::NotServedElsewhere(failure) => Some(failure.as_str()),
            _ => None,
        })
        .collect();
    if !skipped.is_empty() {
        return (
            CONDITION_STATUS_TRUE,
            ZONES_LOADED_REASON_PARTIAL,
            format!(
                "{}/{total} zone(s) loaded; not loaded and served by no other pod of the instance: {}",
                total - skipped.len(),
                skipped.join("; ")
            ),
        );
    }
    (
        CONDITION_STATUS_TRUE,
        ZONES_LOADED_REASON_LOADED,
        format!("{total} zone(s) loaded"),
    )
}

/// The instance's other Ready pods: its Service's ready addresses except the
/// pod being evaluated. Pure: no I/O.
pub(crate) fn sibling_addresses(ready: &[EndpointAddress], pod_ip: &str) -> Vec<String> {
    ready
        .iter()
        .filter(|address| address.ip != pod_ip)
        .map(|address| format!("{}:{}", address.ip, address.port))
        .collect()
}

/// Whether another Ready pod of `instance` serves `zone`. A sibling that
/// cannot be asked counts as serving it: when in doubt, keep the pod out.
async fn served_by_sibling(
    ctx: &Context,
    zone: &DNSZone,
    instance: &Bind9Instance,
    pod_ip: &str,
) -> bool {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();
    let ready = cached_endpoints(&ctx.stores.endpoints, &namespace, &name, BINDCAR_PORT_NAME)
        .unwrap_or_default();
    let siblings = sibling_addresses(&ready, pod_ip);
    if siblings.is_empty() {
        return false;
    }
    let manager = crate::dnszone::zone_manager_for_instance(ctx, &name, &namespace);
    for sibling in &siblings {
        // Serving means loaded: a sibling with the zone configured but no data
        // (a secondary whose transfer failed) serves nothing (ADR-0019).
        match manager.zone_presence(&zone.spec.zone_name, sibling).await {
            Ok(ZonePresence::Absent | ZonePresence::NotLoaded) => {}
            Ok(ZonePresence::Loaded) => return true,
            Err(e) => {
                warn!(
                    "Zones-loaded gate: cannot ask {sibling} whether it serves {}: {e:#}; assuming it does",
                    zone.spec.zone_name
                );
                return true;
            }
        }
    }
    false
}

/// HTTP status of a missing object.
const HTTP_NOT_FOUND: u16 = 404;

/// Whether `error` is the API server reporting the object as gone.
fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<kube::Error>()
        .is_some_and(|e| matches!(e, kube::Error::Api(status) if status.code == HTTP_NOT_FOUND))
}

/// Write the gate condition unless the pod already carries exactly it.
async fn set_gate(
    ctx: &Context,
    pod: &Pod,
    status: &str,
    reason: &str,
    message: &str,
) -> anyhow::Result<()> {
    let message = truncate_message(message);
    if condition_matches(pod, status, reason, &message) {
        return Ok(());
    }
    let namespace = pod
        .namespace()
        .ok_or_else(|| anyhow!("pod {} has no namespace", pod.name_any()))?;
    let now = Time(k8s_openapi::jiff::Timestamp::now());
    let patch = gate_patch(
        status,
        reason,
        &message,
        &transition_time(pod, status, &now),
    );
    let api: Api<Pod> = Api::namespaced(ctx.client.clone(), &namespace);
    api.patch_status(
        &pod.name_any(),
        &PatchParams::default(),
        &Patch::Strategic(&patch),
    )
    .await?;
    Ok(())
}

/// Load one zone onto one pod: the zone itself, and on a primary every record
/// tagged with it.
///
/// Uses the `DNSZone` controller's own write paths through a resolver that
/// addresses only this pod, so the pod gets the zone exactly as every other
/// pod got it. The zone's status is not written: the updater the write paths
/// need is discarded.
///
/// On a secondary (ADR-0019, amending ADR-0017 decision 2): every primary's
/// `allow-transfer` / `also-notify` is first rewritten to the zone's current
/// peers, which include this pod, so its transfer is not denied; after the
/// zone is created and a retransfer issued, a zone not loaded yet is a
/// failure, so a pod whose sibling still serves the zone waits for its
/// transfer.
///
/// # Errors
/// Returns an error when the zone or any record could not be written to the
/// pod, when the primaries could not be told to allow it, or when a secondary
/// has not transferred the zone yet.
async fn load_zone_on_pod(
    ctx: &Arc<Context>,
    zone: &DNSZone,
    instance: &Bind9Instance,
    pod_ip: &str,
) -> anyhow::Result<()> {
    let instance_namespace = instance.namespace().unwrap_or_default();
    let instance_name = instance.name_any();
    let zone_namespace = zone.namespace().unwrap_or_default();
    let instance_refs = get_instances_from_zone(zone, &ctx.stores.bind9_instances)?;
    let resolver = InstanceResolver::for_single_pod(
        &ctx.client,
        &ctx.stores,
        &instance_namespace,
        &instance_name,
        pod_ip,
    );
    let mut discarded = bindy_controller_sdk::status::DNSZoneStatusUpdater::new(zone);
    let (primary_refs, _secondary_refs, peers) =
        crate::dnszone::transfer_peers::zone_transfer_peers(ctx, &instance_refs).await?;

    if instance.spec.role == ServerRole::Secondary {
        ensure!(
            !peers.primaries.is_empty(),
            "no primary pod with its zones loaded to transfer the zone from"
        );
        // The primaries must allow this pod before it asks for the zone.
        let full_resolver = InstanceResolver::for_kube(&ctx.client, &ctx.stores);
        let failures = crate::dnszone::transfer_peers::refresh_primary_peers(
            ctx,
            &zone.spec.zone_name,
            &primary_refs,
            &peers,
            &full_resolver,
        )
        .await;
        ensure!(
            failures.is_empty(),
            "the primaries could not be told to allow this pod: {}",
            failures.join("; ")
        );
        let secondary = crate::dnszone::add_dnszone_to_secondaries_with_resolver(
            ctx.clone(),
            zone.clone(),
            &peers.primaries,
            &mut discarded,
            &instance_refs,
            false,
            &resolver,
        )
        .await?;
        ensure!(
            secondary.outcome.endpoints_configured > 0,
            "the secondary zone was not configured on the pod"
        );
        ensure!(
            secondary.not_loaded.is_empty(),
            "the zone is configured on the pod but not transferred yet"
        );
        return Ok(());
    }

    let outcome = crate::dnszone::add_dnszone_with_resolver(
        ctx.clone(),
        zone.clone(),
        &mut discarded,
        &instance_refs,
        &peers,
        &resolver,
    )
    .await?;
    ensure!(
        outcome.endpoints_configured > 0,
        "the zone was not configured on the pod"
    );

    // Every record the record controller writes into this zone. Read from the
    // store after the zone exists on the pod: a record written to the pod
    // before then (and rejected there) is already in the store (ADR-0017).
    let records = ctx
        .stores
        .records_tagged_with_zone(&zone_namespace, &zone.name_any());
    let own_instance: Vec<_> = instance_refs
        .into_iter()
        .filter(|r| r.namespace == instance_namespace && r.name == instance_name)
        .collect();
    let replay = bindy_bind9::record_push::replay_zone_records_with(
        &ctx.client,
        &ctx.stores,
        &resolver,
        &zone.spec.zone_name,
        &records,
        &own_instance,
    )
    .await;
    ensure!(
        replay.is_complete(),
        "{}",
        replay.summary(&zone.spec.zone_name)
    );
    Ok(())
}

/// Reconcile one pod's zones-loaded gate.
async fn reconcile_pod(pod: Arc<Pod>, ctx: Arc<Context>) -> anyhow::Result<Action> {
    let (instance_namespace, instance_name, pod_ip) = match gate_step(&pod) {
        GateStep::Done(why) => {
            debug!("Zones-loaded gate: pod {} skipped: {why}", pod.name_any());
            return Ok(Action::await_change());
        }
        GateStep::CloseForTermination => {
            // No wait in front of the patch: every second the gate stays open
            // is a second the endpoint stays `serving` for a pod about to stop
            // answering. A failed patch errors out and is retried with the
            // controller's backoff; a pod already gone needs nothing.
            let (status, reason, message) = termination_condition();
            match set_gate(&ctx, &pod, status, reason, message).await {
                Ok(()) => {
                    info!(
                        "Zones-loaded gate: pod {} is terminating, closed its gate so traffic moves before named exits",
                        pod.name_any()
                    );
                    return Ok(converged_action(pod.as_ref()));
                }
                Err(e) if is_not_found(&e) => {
                    debug!(
                        "Zones-loaded gate: terminating pod {} is already gone",
                        pod.name_any()
                    );
                    return Ok(converged_action(pod.as_ref()));
                }
                Err(e) => return Err(e),
            }
        }
        GateStep::WaitForContainers => {
            debug!(
                "Zones-loaded gate: pod {} waits for its containers",
                pod.name_any()
            );
            return Ok(Action::await_change());
        }
        GateStep::Evaluate {
            instance_namespace,
            instance_name,
            pod_ip,
        } => (instance_namespace, instance_name, pod_ip),
    };

    let Some(instance) = ctx
        .stores
        .get_bind9instance(&instance_name, &instance_namespace)
    else {
        set_gate(
            &ctx,
            &pod,
            CONDITION_STATUS_FALSE,
            ZONES_LOADED_REASON_INSTANCE_UNKNOWN,
            &format!("Bind9Instance {instance_namespace}/{instance_name} is not known to the operator yet"),
        )
        .await?;
        return Ok(retry_action(pod.as_ref()));
    };

    let zones = required_zones(&ctx.stores.dnszones.state(), &instance);
    if zones.is_empty() {
        info!(
            "Zones-loaded gate: no live zone selects {instance_namespace}/{instance_name}; admitting pod {}",
            pod.name_any()
        );
        set_gate(
            &ctx,
            &pod,
            CONDITION_STATUS_TRUE,
            ZONES_LOADED_REASON_NO_ZONES,
            &format!("No live DNSZone selects Bind9Instance {instance_namespace}/{instance_name}"),
        )
        .await?;
        return Ok(converged_action(pod.as_ref()));
    }

    // Show the pod is being worked on, once: a retry keeps the last failure
    // visible instead of flapping between two messages.
    if gate_condition(&pod).is_none() {
        set_gate(
            &ctx,
            &pod,
            CONDITION_STATUS_FALSE,
            ZONES_LOADED_REASON_LOADING,
            &format!("Loading {} zone(s) onto the pod", zones.len()),
        )
        .await?;
    }

    let mut loads = Vec::with_capacity(zones.len());
    for zone in &zones {
        let Err(e) = load_zone_on_pod(&ctx, zone, &instance, &pod_ip).await else {
            loads.push(ZoneLoad::Loaded);
            continue;
        };
        let failure = format!("{}: {e:#}", zone.spec.zone_name);
        if served_by_sibling(&ctx, zone, &instance, &pod_ip).await {
            warn!(
                "Zones-loaded gate: pod {} cannot be admitted, zone {} is not loaded on it: {e:#}",
                pod.name_any(),
                zone.spec.zone_name
            );
            loads.push(ZoneLoad::Blocking(failure));
        } else {
            warn!(
                "Zones-loaded gate: zone {} not loaded on pod {} and served by no other pod of the instance; not holding the pod back: {e:#}",
                zone.spec.zone_name,
                pod.name_any()
            );
            loads.push(ZoneLoad::NotServedElsewhere(failure));
        }
    }

    let (status, reason, message) = gate_outcome(&loads);
    set_gate(&ctx, &pod, status, reason, &message).await?;
    if status == CONDITION_STATUS_TRUE {
        info!(
            "Zones-loaded gate: pod {} admitted ({reason}): {message}",
            pod.name_any()
        );
        return Ok(converged_action(pod.as_ref()));
    }
    Ok(retry_action(pod.as_ref()))
}

/// The reconcile entry point: metrics around [`reconcile_pod`].
async fn reconcile_pod_wrapper(pod: Arc<Pod>, ctx: Arc<Context>) -> Result<Action, ReconcileError> {
    let start = std::time::Instant::now();
    let result = reconcile_pod(pod, ctx).await;
    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_ZONES_GATE, duration);
    } else {
        metrics::record_reconciliation_error(KIND_ZONES_GATE, duration);
        metrics::record_error(KIND_ZONES_GATE, "reconcile_error");
    }
    result.map_err(ReconcileError::from)
}

/// Run one zones-loaded gate controller per namespace target. Cluster-wide
/// mode yields exactly one.
pub(crate) async fn run_zones_gate_controllers(ctx: Arc<Context>) -> anyhow::Result<()> {
    info!("Starting zones-loaded readiness gate controller");
    let targets = owned_targets(&ctx.namespace_scope);
    futures::future::join_all(
        targets
            .into_iter()
            .map(|target| run_zones_gate_controller(ctx.clone(), target)),
    )
    .await;
    Ok(())
}

async fn run_zones_gate_controller(ctx: Arc<Context>, target: Option<String>) {
    debug!(
        namespace = target.as_deref().unwrap_or("<all>"),
        "Starting zones-loaded readiness gate controller"
    );
    let ws = ctx.watch.clone();
    let target = target.as_deref();
    let pod_store = ws.store::<Pod>(target);
    let pod_store_for_filter = pod_store.clone();
    let stores_for_zones = ctx.stores.clone();
    let stores_for_zone_filter = ctx.stores.clone();

    // The pod's own events, only when something the gate reads changed: the
    // pod appearing, its containers turning ready, its gate condition.
    let primary = changed_only(
        ws.subscribe::<Pod>(target),
        pod_gate_key,
        move |pod: &Pod| {
            pod_store_for_filter
                .get(&ObjectRef::from_obj(pod))
                .is_some()
        },
    );

    Controller::for_stream(primary, pod_store)
        // A zone becoming live, changing selectors or being deleted changes
        // which zones a gated pod needs (ADR-0017). Zones of any namespace can
        // select an instance here, so every target's zones are subscribed.
        .watches_stream(
            changed_only(
                ws.subscribe_all::<DNSZone>(),
                zone_gate_key,
                move |zone: &DNSZone| {
                    stores_for_zone_filter
                        .get_dnszone(&zone.name_any(), &zone.namespace().unwrap_or_default())
                        .is_some()
                },
            ),
            move |zone| {
                gated_pods_for_zone(
                    &stores_for_zones.bind9_pods.state(),
                    &stores_for_zones.bind9_instances.state(),
                    &zone,
                )
            },
        )
        .graceful_shutdown_on(ctx.shutdown.wait())
        .run(reconcile_pod_wrapper, error_policy, ctx.clone())
        .for_each(|_| futures::future::ready(()))
        .await;
}

#[cfg(test)]
#[path = "zones_gate_tests.rs"]
mod zones_gate_tests;
