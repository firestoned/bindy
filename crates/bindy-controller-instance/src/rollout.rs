// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Staggered BIND9 rollouts (ADR-0018).
//!
//! A change that rolls a `Bind9Instance`'s pods (anything under its
//! Deployment's `spec.template`) is applied only while no instance in its
//! *conflict set* is mid-rollout, so the nameservers of a zone never take
//! their handover gap at the same time. The conflict set is every instance
//! that serves a zone in common (from `DNSZone` `status.bind9Instances`) or
//! belongs to the same `Bind9Cluster` / `ClusterBind9Provider`.
//!
//! Everything here but [`RolloutQueue`] is pure: the decisions read store
//! snapshots and return values, so they are tested without a cluster. The
//! queue is the one piece of process state: a first-come-first-served list
//! of waiting instances, the *claims* that close the window between two
//! concurrent reconciles reading the same idle store, and the template
//! patches known to change nothing (ADR-0018 decision 8). It holds nothing
//! the cluster does not: after a restart, rollouts in flight are seen in the
//! Deployment store, waiters re-queue on their first reconcile, and a known
//! no-op is re-learnt with one more no-op patch.
//!
//! Event-driven (ADR-0016): a deferred instance awaits a change and is woken
//! by a conflicting instance's Deployment or Pod events
//! ([`waiters_to_wake`]), or by the queue itself when a claim is released or
//! a waiter leaves ([`RolloutQueue::subscribe`]). No timer.

use crate::crd::{Bind9Instance, Condition, DNSZone};
use crate::labels::K8S_INSTANCE;
use crate::status_reasons::{
    CONDITION_TYPE_ROLLOUT, REASON_PROGRESS_DEADLINE_EXCEEDED, REASON_ROLLOUT_PEER_STALLED,
    REASON_ROLLOUT_QUEUED,
};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kube::runtime::reflector::ObjectRef;
use kube::ResourceExt;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// The Deployment condition that reports rollout progress.
const DEPLOYMENT_CONDITION_PROGRESSING: &str = "Progressing";

/// `Progressing` reason the Deployment controller sets when the latest
/// rollout is complete.
const REASON_NEW_REPLICA_SET_AVAILABLE: &str = "NewReplicaSetAvailable";

/// The Pod condition that says the pod is in its Service.
const POD_CONDITION_READY: &str = "Ready";

/// A Kubernetes condition status.
const STATUS_TRUE: &str = "True";

/// A Kubernetes condition status.
const STATUS_FALSE: &str = "False";

/// A `Bind9Instance`, by namespace and name. Ordered by namespace then name,
/// which is the tie-break wherever the order must be deterministic.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct InstanceId {
    /// Namespace of the instance
    pub namespace: String,
    /// Name of the instance
    pub name: String,
}

impl InstanceId {
    /// The id of `namespace`/`name`.
    pub(crate) fn new(namespace: &str, name: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
        }
    }

    /// The id of `instance`.
    pub(crate) fn of(instance: &Bind9Instance) -> Self {
        Self::new(
            &instance.namespace().unwrap_or_default(),
            &instance.name_any(),
        )
    }

    /// The controller reference of this instance.
    pub(crate) fn object_ref(&self) -> ObjectRef<Bind9Instance> {
        ObjectRef::new(&self.name).within(&self.namespace)
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace, self.name)
    }
}

/// The instances whose rollouts `me` must not overlap with: every other
/// instance that serves a zone `me` serves, or that belongs to the same
/// cluster (the same `clusterRef` in the same namespace, or the same
/// `clusterRef` naming a `ClusterBind9Provider`). Instances being deleted are
/// left out. Pure: no I/O.
///
/// # Arguments
/// * `me` - The instance about to roll
/// * `instances` - Every instance in the store
/// * `zones` - Every zone in the store
/// * `provider_names` - The names of every `ClusterBind9Provider`
///
/// # Returns
/// The conflicting instances, ordered.
pub(crate) fn conflict_set(
    me: &Bind9Instance,
    instances: &[Arc<Bind9Instance>],
    zones: &[Arc<DNSZone>],
    provider_names: &BTreeSet<String>,
) -> BTreeSet<InstanceId> {
    let my_id = InstanceId::of(me);
    let mut set = BTreeSet::new();

    // Shared zones: both listed in a zone's status.
    for zone in zones {
        let Some(status) = zone.status.as_ref() else {
            continue;
        };
        let served_by: Vec<InstanceId> = status
            .bind9_instances
            .iter()
            .map(|inst| InstanceId::new(&inst.namespace, &inst.name))
            .collect();
        if !served_by.contains(&my_id) {
            continue;
        }
        set.extend(served_by.into_iter().filter(|id| *id != my_id));
    }

    // Same cluster.
    let cluster_ref = me.spec.cluster_ref.as_str();
    if !cluster_ref.is_empty() {
        let is_provider = provider_names.contains(cluster_ref);
        set.extend(
            instances
                .iter()
                .filter(|other| other.spec.cluster_ref == cluster_ref)
                .filter(|other| is_provider || other.namespace() == me.namespace())
                .map(|other| InstanceId::of(other))
                .filter(|id| *id != my_id),
        );
    }

    // An instance being deleted (or unknown to the store) cannot block.
    let live: BTreeSet<InstanceId> = instances
        .iter()
        .filter(|inst| inst.metadata.deletion_timestamp.is_none())
        .map(|inst| InstanceId::of(inst))
        .collect();
    set.retain(|id| live.contains(id));
    set
}

/// Where a conflicting instance's rollout stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PeerRollout {
    /// Not rolling (complete, scaled to zero, degraded outside a rollout, or
    /// no Deployment yet): does not block.
    Idle,
    /// Mid-rollout: blocks. The text says why, for the status message.
    Rolling(String),
    /// Past its `progressDeadlineSeconds` (`ProgressDeadlineExceeded`): does
    /// not block, so a stuck instance cannot hold the others forever.
    Stalled,
}

/// The Deployment's `Progressing` condition as `(status, reason)`.
fn progressing(deployment: &Deployment) -> Option<(&str, &str)> {
    deployment
        .status
        .as_ref()?
        .conditions
        .as_ref()?
        .iter()
        .find(|c| c.type_ == DEPLOYMENT_CONDITION_PROGRESSING)
        .map(|c| (c.status.as_str(), c.reason.as_deref().unwrap_or_default()))
}

/// Whether the pod's `Ready` condition is `True`.
fn pod_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|c| c.type_ == POD_CONDITION_READY && c.status == STATUS_TRUE)
        })
}

/// Where an instance's rollout stands, from its Deployment and its pods.
/// Pure: no I/O.
///
/// The rules (ADR-0018 decision 4), in order: no Deployment is idle;
/// `ProgressDeadlineExceeded` is stalled; an unobserved generation is
/// rolling; zero replicas is idle; a completed rollout
/// (`NewReplicaSetAvailable`, every replica updated, no surge pod) is idle
/// even if a pod was lost since; otherwise a surge pod, fewer updated,
/// available or ready replicas than wanted, or a non-terminating pod that is
/// not `Ready` (its zones-loaded gate still closed) is rolling.
///
/// # Arguments
/// * `deployment` - The instance's Deployment from the store, if any
/// * `pods` - The instance's pods from the store
///
/// # Returns
/// The [`PeerRollout`] of the instance.
pub(crate) fn peer_rollout(deployment: Option<&Deployment>, pods: &[Arc<Pod>]) -> PeerRollout {
    let Some(deployment) = deployment else {
        return PeerRollout::Idle;
    };
    let progress = progressing(deployment);
    if progress.is_some_and(|(status, reason)| {
        status == STATUS_FALSE && reason == REASON_PROGRESS_DEADLINE_EXCEEDED
    }) {
        return PeerRollout::Stalled;
    }
    let generation = deployment.metadata.generation.unwrap_or_default();
    let status = deployment.status.clone().unwrap_or_default();
    let observed = status.observed_generation.unwrap_or_default();
    if observed < generation {
        return PeerRollout::Rolling(format!(
            "its Deployment generation {generation} is not observed yet"
        ));
    }
    let wanted = deployment
        .spec
        .as_ref()
        .and_then(|s| s.replicas)
        .unwrap_or(1);
    if wanted == 0 {
        return PeerRollout::Idle;
    }
    let replicas = status.replicas.unwrap_or_default();
    let updated = status.updated_replicas.unwrap_or_default();
    let complete = progress.is_some_and(|(_, reason)| reason == REASON_NEW_REPLICA_SET_AVAILABLE)
        && updated == wanted
        && replicas == wanted;
    if complete {
        return PeerRollout::Idle;
    }
    let ready = status.ready_replicas.unwrap_or_default();
    let available = status.available_replicas.unwrap_or_default();
    if replicas > wanted || updated < wanted || available < wanted || ready < wanted {
        return PeerRollout::Rolling(format!(
            "rolling out ({updated}/{wanted} updated, {ready}/{wanted} ready, {replicas} pods)"
        ));
    }
    if let Some(gated) = pods
        .iter()
        .filter(|pod| pod.metadata.deletion_timestamp.is_none())
        .find(|pod| !pod_ready(pod))
    {
        return PeerRollout::Rolling(format!(
            "rolling out (pod {} is not Ready yet)",
            gated.name_any()
        ));
    }
    PeerRollout::Idle
}

/// The instance a pod belongs to (label `app.kubernetes.io/instance`).
pub(crate) fn instance_of_pod(pod: &Pod) -> Option<InstanceId> {
    let name = pod.labels().get(K8S_INSTANCE)?;
    Some(InstanceId::new(&pod.namespace()?, name))
}

/// The instance owning a Deployment (owner reference of kind
/// `Bind9Instance`).
pub(crate) fn instance_of_deployment(deployment: &Deployment) -> Option<InstanceId> {
    let owner = deployment
        .owner_references()
        .iter()
        .find(|owner| owner.kind == crate::constants::KIND_BIND9_INSTANCE)?;
    Some(InstanceId::new(&deployment.namespace()?, &owner.name))
}

/// The rollout state of every instance in `me`'s conflict set, ordered by
/// id. Pure: no I/O.
///
/// # Arguments
/// * `me` - The instance about to roll
/// * `instances`, `zones`, `provider_names` - As for [`conflict_set`]
/// * `deployments` - Every instance Deployment in the store
/// * `pods` - Every BIND9 pod in the store
///
/// # Returns
/// `(peer, state)` for each conflicting instance.
pub(crate) fn peer_states(
    me: &Bind9Instance,
    instances: &[Arc<Bind9Instance>],
    zones: &[Arc<DNSZone>],
    provider_names: &BTreeSet<String>,
    deployments: &[Arc<Deployment>],
    pods: &[Arc<Pod>],
) -> Vec<(InstanceId, PeerRollout)> {
    conflict_set(me, instances, zones, provider_names)
        .into_iter()
        .map(|peer| {
            let deployment = deployments
                .iter()
                .find(|d| instance_of_deployment(d).as_ref() == Some(&peer))
                .map(AsRef::as_ref);
            let own_pods: Vec<Arc<Pod>> = pods
                .iter()
                .filter(|pod| instance_of_pod(pod).as_ref() == Some(&peer))
                .cloned()
                .collect();
            let state = peer_rollout(deployment, &own_pods);
            (peer, state)
        })
        .collect()
}

/// Why an instance waits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WaitReason {
    /// The blocker is mid-rollout (the text says how far).
    Rolling(String),
    /// The blocker has decided to roll and its patch is not in the store yet.
    Claimed,
    /// The blocker has been waiting longer (first come, first served).
    QueuedEarlier,
}

/// Whether an instance may apply its pod-template change now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RolloutDecision {
    /// Roll now. `stalled_peers` are conflicting instances whose rollout is
    /// past its deadline and was not waited for.
    Proceed {
        /// Conflicting instances whose stalled rollout was not waited for
        stalled_peers: Vec<InstanceId>,
    },
    /// Wait for `blocker`.
    Wait {
        /// The instance waited for
        blocker: InstanceId,
        /// Why
        reason: WaitReason,
    },
}

/// Decide whether an instance with queue position `my_seq` may roll. Pure:
/// no I/O.
///
/// It waits for, in this order: the first conflicting instance (by id) that
/// is rolling; the first that holds a claim; the conflicting instance that
/// has waited longest, if it was queued before `my_seq`. Otherwise it
/// proceeds. Two candidates never wait for each other: between waiters only
/// the lower sequence number waits for nothing.
///
/// # Arguments
/// * `my_seq` - The instance's position in the queue
/// * `peers` - Its conflicting instances and their rollout state
/// * `claimed` - Instances holding a claim (the instance itself excluded)
/// * `waiting` - Every waiting instance and its position
///
/// # Returns
/// The [`RolloutDecision`].
pub(crate) fn decide_rollout(
    my_seq: u64,
    peers: &[(InstanceId, PeerRollout)],
    claimed: &BTreeSet<InstanceId>,
    waiting: &BTreeMap<InstanceId, u64>,
) -> RolloutDecision {
    let mut sorted: Vec<&(InstanceId, PeerRollout)> = peers.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    if let Some((blocker, why)) = sorted.iter().find_map(|(id, state)| match state {
        PeerRollout::Rolling(why) => Some((id, why)),
        _ => None,
    }) {
        return RolloutDecision::Wait {
            blocker: blocker.clone(),
            reason: WaitReason::Rolling(why.clone()),
        };
    }
    if let Some((blocker, _)) = sorted.iter().find(|(id, _)| claimed.contains(id)) {
        return RolloutDecision::Wait {
            blocker: blocker.clone(),
            reason: WaitReason::Claimed,
        };
    }
    if let Some((_, blocker)) = sorted
        .iter()
        .filter_map(|(id, _)| waiting.get(id).map(|seq| (*seq, id)))
        .filter(|(seq, _)| *seq < my_seq)
        .min()
    {
        return RolloutDecision::Wait {
            blocker: blocker.clone(),
            reason: WaitReason::QueuedEarlier,
        };
    }
    RolloutDecision::Proceed {
        stalled_peers: sorted
            .iter()
            .filter(|(_, state)| *state == PeerRollout::Stalled)
            .map(|(id, _)| id.clone())
            .collect(),
    }
}

/// The queue's state, behind its lock.
#[derive(Default)]
struct QueueState {
    /// The next sequence number to hand out
    next_seq: u64,
    /// Waiting instances and their position
    waiting: BTreeMap<InstanceId, u64>,
    /// Instances that decided to roll, with the Deployment generation they
    /// decided on; a claim lasts until the store shows a later generation
    claims: BTreeMap<InstanceId, i64>,
    /// Template patches known to change nothing, per instance
    known_noops: BTreeMap<InstanceId, KnownNoop>,
}

/// A pod-template patch that bumped no Deployment generation: the drift
/// check saw a difference the API server's defaulting erases. Sending the
/// same patch to the same generation again would change nothing again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KnownNoop {
    /// The Deployment generation the patch was sent to
    generation: i64,
    /// The fingerprint of the patch
    fingerprint: u64,
}

/// How a pod-template change starts ([`RolloutQueue::begin_template_change`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TemplateStart {
    /// The same patch already changed nothing at this generation: treat the
    /// template as up to date. The instance is off the waiting list.
    KnownNoop,
    /// The queue decided.
    Decided(RolloutDecision),
}

/// What a sent pod-template patch did ([`RolloutQueue::finish_template_patch`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PatchOutcome {
    /// It bumped the Deployment generation: the pods roll. The status to
    /// report.
    Rolled(RolloutStatus),
    /// It bumped no generation: nothing rolls, and the instance reports no
    /// rollout.
    NoOp,
}

/// The process-wide rollout queue (ADR-0018 decision 5). Only the leader
/// reconciles, so one queue orders every rollout the operator makes.
pub(crate) struct RolloutQueue {
    state: Mutex<QueueState>,
    wakers: Mutex<Vec<UnboundedSender<InstanceId>>>,
}

impl Default for RolloutQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl RolloutQueue {
    /// An empty queue.
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(QueueState::default()),
            wakers: Mutex::new(Vec::new()),
        }
    }

    /// A stream of instances to wake: waiters whose blocker released its
    /// claim without rolling or left the queue. Each controller (one per
    /// namespace target) subscribes once and reconciles the ids it owns.
    pub(crate) fn subscribe(&self) -> UnboundedReceiver<InstanceId> {
        let (tx, rx) = unbounded_channel();
        self.wakers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tx);
        rx
    }

    /// Decide, atomically with every other decision, whether `me` may apply
    /// its pod-template change now. On `Proceed` it takes a claim and leaves
    /// the waiting list; on `Wait` it keeps (or takes) its place.
    ///
    /// # Arguments
    /// * `me` - The instance
    /// * `my_generation` - Its Deployment's generation before the patch
    /// * `peers` - Its conflicting instances and their rollout state
    /// * `store_generation` - An instance's Deployment generation in the
    ///   store, which ends a claim once it moves past the claimed one
    ///
    /// # Returns
    /// The [`RolloutDecision`].
    pub(crate) fn try_start(
        &self,
        me: &InstanceId,
        my_generation: i64,
        peers: &[(InstanceId, PeerRollout)],
        store_generation: impl Fn(&InstanceId) -> Option<i64>,
    ) -> RolloutDecision {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .claims
            .retain(|id, claimed| store_generation(id).is_some_and(|current| current <= *claimed));
        let my_seq = if let Some(seq) = state.waiting.get(me) {
            *seq
        } else {
            let seq = state.next_seq;
            state.next_seq += 1;
            seq
        };
        let claimed: BTreeSet<InstanceId> = state
            .claims
            .keys()
            .filter(|id| *id != me)
            .cloned()
            .collect();
        let decision = decide_rollout(my_seq, peers, &claimed, &state.waiting);
        if matches!(decision, RolloutDecision::Proceed { .. }) {
            state.waiting.remove(me);
            state.claims.insert(me.clone(), my_generation);
        } else {
            state.waiting.insert(me.clone(), my_seq);
        }
        decision
    }

    /// Drop `me`'s claim: its patch failed, so nothing is rolling and no
    /// Deployment event will wake its waiters. Wakes them. (A patch that
    /// changed nothing goes through [`RolloutQueue::finish_template_patch`].)
    pub(crate) fn release(&self, me: &InstanceId) {
        let waiting = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.claims.remove(me).is_none() {
                return;
            }
            state.waiting.keys().cloned().collect::<Vec<_>>()
        };
        self.wake(&waiting);
    }

    /// `me` has no pod-template change pending (or is being deleted): leave
    /// the waiting list, and wake the remaining waiters if it was on it. A
    /// claim is kept until the store shows the patch.
    pub(crate) fn leave(&self, me: &InstanceId) {
        let waiting = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.waiting.remove(me).is_none() {
                return;
            }
            state.waiting.keys().cloned().collect::<Vec<_>>()
        };
        self.wake(&waiting);
    }

    /// Start a pod-template change: [`TemplateStart::KnownNoop`] (and off
    /// the waiting list) when the same patch already changed nothing at
    /// `my_generation`, else [`RolloutQueue::try_start`]'s decision.
    ///
    /// This is what stops a perpetual false difference from looping: an
    /// instance sends a given patch to a given generation at most once, so
    /// it claims, and wakes the waiters, at most once for it.
    ///
    /// # Arguments
    /// * `me` - The instance
    /// * `my_generation` - Its Deployment's generation before the patch
    /// * `fingerprint` - The fingerprint of the patch it would send
    /// * `peers`, `store_generation` - As for [`RolloutQueue::try_start`]
    ///
    /// # Returns
    /// The [`TemplateStart`].
    pub(crate) fn begin_template_change(
        &self,
        me: &InstanceId,
        my_generation: i64,
        fingerprint: u64,
        peers: &[(InstanceId, PeerRollout)],
        store_generation: impl Fn(&InstanceId) -> Option<i64>,
    ) -> TemplateStart {
        let known = KnownNoop {
            generation: my_generation,
            fingerprint,
        };
        let is_known = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .known_noops
            .get(me)
            == Some(&known);
        if is_known {
            self.leave(me);
            return TemplateStart::KnownNoop;
        }
        TemplateStart::Decided(self.try_start(me, my_generation, peers, store_generation))
    }

    /// Record what a sent pod-template patch did.
    ///
    /// A patch that bumped the generation rolls the pods: `me` keeps its
    /// claim until the store shows the patch, and reports `decision`. One
    /// that bumped nothing rolled nothing: `me` drops its claim, leaves the
    /// waiting list, and the patch is remembered as a known no-op for
    /// [`RolloutQueue::begin_template_change`]. The waiters are woken (one
    /// may wait for this claim and no Deployment event will come), which
    /// happens at most once per known no-op.
    ///
    /// # Arguments
    /// * `me` - The instance
    /// * `decision` - The decision it patched on
    /// * `generation_before` - Its Deployment's generation before the patch
    /// * `generation_after` - The generation the patch returned
    /// * `fingerprint` - The fingerprint of the patch
    ///
    /// # Returns
    /// The [`PatchOutcome`].
    pub(crate) fn finish_template_patch(
        &self,
        me: &InstanceId,
        decision: &RolloutDecision,
        generation_before: i64,
        generation_after: i64,
        fingerprint: u64,
    ) -> PatchOutcome {
        if generation_after > generation_before {
            return PatchOutcome::Rolled(RolloutStatus::from_decision(decision));
        }
        let waiting = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.known_noops.insert(
                me.clone(),
                KnownNoop {
                    generation: generation_before,
                    fingerprint,
                },
            );
            let held_claim = state.claims.remove(me).is_some();
            let was_waiting = state.waiting.remove(me).is_some();
            if !held_claim && !was_waiting {
                return PatchOutcome::NoOp;
            }
            state.waiting.keys().cloned().collect::<Vec<_>>()
        };
        self.wake(&waiting);
        PatchOutcome::NoOp
    }

    /// `me` is gone (deleted) or was just created: forget its known no-op
    /// patch and leave the waiting list ([`RolloutQueue::leave`]).
    pub(crate) fn forget(&self, me: &InstanceId) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .known_noops
            .remove(me);
        self.leave(me);
    }

    /// The waiting instances, ordered by id.
    pub(crate) fn waiting(&self) -> Vec<InstanceId> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .waiting
            .keys()
            .cloned()
            .collect()
    }

    fn wake(&self, ids: &[InstanceId]) {
        if ids.is_empty() {
            return;
        }
        let mut wakers = self.wakers.lock().unwrap_or_else(PoisonError::into_inner);
        // A controller that has shut down dropped its receiver: forget it.
        wakers.retain(|tx| ids.iter().all(|id| tx.send(id.clone()).is_ok()));
    }
}

/// What `create_or_update_deployment` needs to stagger a pod-template change:
/// the shared stores (read-only snapshots) and the process-wide queue.
pub(crate) struct RolloutGate<'a> {
    /// The shared reflector stores
    pub stores: &'a bindy_controller_sdk::context::Stores,
    /// The process-wide rollout queue
    pub queue: &'a RolloutQueue,
}

impl RolloutGate<'_> {
    /// Start `instance`'s pod-template change
    /// ([`RolloutQueue::begin_template_change`]), reading every input from
    /// the stores (no API call).
    ///
    /// # Arguments
    /// * `instance` - The instance about to roll
    /// * `my_generation` - Its Deployment's generation before the patch
    /// * `fingerprint` - The fingerprint of the patch it would send
    ///
    /// # Returns
    /// The [`TemplateStart`]. On `Proceed` the instance holds a claim: the
    /// caller must [`RolloutQueue::release`] it if the patch fails, and pass
    /// a sent patch's result to [`RolloutQueue::finish_template_patch`].
    pub(crate) fn begin(
        &self,
        instance: &Bind9Instance,
        my_generation: i64,
        fingerprint: u64,
    ) -> TemplateStart {
        let instances = self.stores.bind9_instances.state();
        let zones = self.stores.dnszones.state();
        let provider_names: BTreeSet<String> = self
            .stores
            .cluster_bind9_providers
            .state()
            .iter()
            .map(|provider| provider.name_any())
            .collect();
        let deployments = self.stores.bind9_deployments.state();
        let pods = self.stores.bind9_pods.state();
        let peers = peer_states(
            instance,
            &instances,
            &zones,
            &provider_names,
            &deployments,
            &pods,
        );
        let generations: BTreeMap<InstanceId, i64> = deployments
            .iter()
            .filter_map(|d| Some((instance_of_deployment(d)?, d.metadata.generation?)))
            .collect();
        self.queue.begin_template_change(
            &InstanceId::of(instance),
            my_generation,
            fingerprint,
            &peers,
            |id| generations.get(id).copied(),
        )
    }
}

/// The waiting instances a change to `changed` (its Deployment or a pod)
/// must wake: those in `changed`'s conflict set, or every waiter when
/// `changed` is no longer in the store. `changed` itself is left out: its
/// own events reach it already. Pure: no I/O (ADR-0009 §5).
///
/// # Arguments
/// * `changed` - The instance whose Deployment or pod changed
/// * `waiting` - The waiting instances
/// * `instances`, `zones`, `provider_names` - As for [`conflict_set`]
///
/// # Returns
/// References to the instances to reconcile.
pub(crate) fn waiters_to_wake(
    changed: &InstanceId,
    waiting: &[InstanceId],
    instances: &[Arc<Bind9Instance>],
    zones: &[Arc<DNSZone>],
    provider_names: &BTreeSet<String>,
) -> Vec<ObjectRef<Bind9Instance>> {
    if waiting.is_empty() {
        return vec![];
    }
    let others = waiting.iter().filter(|id| *id != changed);
    let Some(changed_instance) = instances.iter().find(|i| InstanceId::of(i) == *changed) else {
        return others.map(InstanceId::object_ref).collect();
    };
    let conflicts = conflict_set(changed_instance, instances, zones, provider_names);
    others
        .filter(|id| conflicts.contains(id))
        .map(InstanceId::object_ref)
        .collect()
}

/// What the rollout decision reads from a Deployment: its generation, the
/// observed generation, the replica counts and the `Progressing` condition.
/// The Deployment stream is filtered on it.
pub(crate) fn deployment_rollout_key(deployment: &Deployment) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    deployment.metadata.generation.hash(&mut hasher);
    deployment
        .spec
        .as_ref()
        .and_then(|s| s.replicas)
        .hash(&mut hasher);
    if let Some(status) = deployment.status.as_ref() {
        status.observed_generation.hash(&mut hasher);
        status.replicas.hash(&mut hasher);
        status.updated_replicas.hash(&mut hasher);
        status.ready_replicas.hash(&mut hasher);
        status.available_replicas.hash(&mut hasher);
    }
    progressing(deployment).hash(&mut hasher);
    hasher.finish()
}

/// What the rollout decision reads from a pod: its instance, readiness and
/// deletion. The Pod stream is filtered on it.
pub(crate) fn pod_rollout_key(pod: &Pod) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    instance_of_pod(pod).hash(&mut hasher);
    pod_ready(pod).hash(&mut hasher);
    pod.metadata.deletion_timestamp.is_some().hash(&mut hasher);
    hasher.finish()
}

/// What an instance reports about its rollout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RolloutStatus {
    /// Nothing to report.
    None,
    /// A pod-template change waits (the message names the blocker).
    Queued(String),
    /// Rolled out without waiting for a stalled conflicting rollout.
    ProceededPastStalled(String),
}

impl RolloutStatus {
    /// The status that reports `decision`.
    pub(crate) fn from_decision(decision: &RolloutDecision) -> Self {
        match decision {
            RolloutDecision::Proceed { stalled_peers } if stalled_peers.is_empty() => Self::None,
            RolloutDecision::Proceed { stalled_peers } => Self::ProceededPastStalled(format!(
                "Rolled out without waiting for {}: rollout exceeded its progressDeadlineSeconds",
                stalled_peers
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            RolloutDecision::Wait { blocker, reason } => {
                let why = match reason {
                    WaitReason::Rolling(progress) => format!("is {progress}"),
                    WaitReason::Claimed => "is starting its rollout".to_string(),
                    WaitReason::QueuedEarlier => "was queued earlier".to_string(),
                };
                Self::Queued(format!(
                    "Pod template change waits for Bind9Instance {blocker}, which {why}; the current pods keep serving"
                ))
            }
        }
    }
}

/// The `Rollout` condition for `status`, if there is one to show. Pure.
pub(crate) fn rollout_condition(status: &RolloutStatus) -> Option<Condition> {
    let (condition_status, reason, message) = match status {
        RolloutStatus::None => return None,
        RolloutStatus::Queued(message) => (STATUS_FALSE, REASON_ROLLOUT_QUEUED, message),
        RolloutStatus::ProceededPastStalled(message) => {
            (STATUS_TRUE, REASON_ROLLOUT_PEER_STALLED, message)
        }
    };
    Some(Condition {
        r#type: CONDITION_TYPE_ROLLOUT.to_string(),
        status: condition_status.to_string(),
        reason: Some(reason.to_string()),
        message: Some(message.clone()),
        last_transition_time: Some(chrono::Utc::now().to_rfc3339()),
    })
}

/// Whether the instance's status says a pod-template change is queued. Such
/// an instance never takes the reconcile short-cut, so the change is applied
/// when it is woken (ADR-0018 decision 6).
pub(crate) fn rollout_queued(instance: &Bind9Instance) -> bool {
    instance.status.as_ref().is_some_and(|status| {
        status.conditions.iter().any(|c| {
            c.r#type == CONDITION_TYPE_ROLLOUT && c.reason.as_deref() == Some(REASON_ROLLOUT_QUEUED)
        })
    })
}

#[cfg(test)]
#[path = "rollout_tests.rs"]
mod rollout_tests;
