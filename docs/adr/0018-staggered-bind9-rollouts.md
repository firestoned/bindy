# 0018: Staggered BIND9 rollouts

- **Status:** Accepted
- **Date:** 2026-10-07
- **Deciders:** Erick Bourgeois
- **Amended:** 2026-10-07 (Decision 8: a pod-template patch that bumps no generation is not a rollout; the drift check compares semantically; found on v0.8.0-rc.6 as a reconcile hot loop)
- **Amended:** 2026-10-07 (Decision 8 note: the known no-op rule also hid a patch defect. The bindcar `env` list was strategic-merged by name, so a variable removed from `bindcarConfig` was never removed from the pods; the patch changed nothing, was remembered as a no-op and repeated once per operator start. The env list is now replaced whole (`$patch: replace`), so a removal is a real template change and rolls the pods; found by the chaos e2e suite)
- **Related:** Builds on [ADR-0017](0017-zones-loaded-readiness-gate.md) (a new pod is Ready only with its zones; decision 6, the handover at termination), [ADR-0016](0016-event-driven-reconciliation.md) (every wake is a watch event or a backoff retry), [ADR-0013](0013-validate-and-render-bind9-config-with-hornet.md) (a change of the rendered configuration rolls every pod) and [ADR-0009](0009-workspace-crate-split-and-shared-watch-layer.md) §3 and §5 (shared watches, pure mappers)

## Context

Each `Bind9Instance` is a Deployment, rolled by Kubernetes with
`maxSurge` 1 / `maxUnavailable` 0. ADR-0017 made each rollout safe on its own:
the new pod is admitted only with its zones, and (decision 6) the old pod
leaves its Service at the start of its termination. What remains at every
handover is a short gap that bindy does not control: with
`externalTrafficPolicy: Local` behind a MetalLB layer-2 `LoadBalancer`, the
announcement moves to the new pod's node, and until clients learn the new MAC
(a gratuitous ARP, "a few seconds" per MetalLB's documentation) packets to the
old node are dropped.

One instance taking that gap is what a zone's other nameservers are for. But
several changes roll **every instance of a cluster at once**:

- a change of the shared cluster ConfigMap (`<cluster>-config`), which every
  managed instance mounts and whose hash is stamped on each pod template;
- a bindy upgrade that changes the rendered configuration (ADR-0013 stage 3)
  or the pod template (a new volume, the readiness gate of ADR-0017);
- an image or bindcar change at the cluster or provider level.

The instance controller applies each of these the moment it reconciles, and
at operator start every instance reconciles at once. On 2026-10-07, rolling
v0.8.0-rc.5, all three nameservers of a live zone (two primaries behind
MetalLB with `Local`, one secondary) rolled at the same instant, and every
query to both load balancer IPs timed out for 9 to 12 s (ADR-0017 decision 6
has the timeline). Decision 6 shrinks each gap; it does not stop all the gaps
of a zone from landing together.

## Decision

1. **An instance applies a rollout-triggering Deployment change only while no
   instance in its conflict set is mid-rollout.** The decision is taken in
   `create_or_update_deployment`
   (`crates/bindy-controller-instance/src/bind9instance/resources.rs`) by the
   pure functions of `crates/bindy-controller-instance/src/rollout.rs`.

2. **What is staggered.** `deployment_change` splits the existing drift check
   (`deployment_needs_update`) in two:
   - **template changes** (anything under `spec.template`: the config hash,
     volumes and mounts, init containers, the bindcar image, env, pull policy
     and resources, topology spread constraints, readiness gates, pod
     template labels) roll the pods and are staggered;
   - **scale changes** (`spec.replicas` only) are applied at once. A queued
     template change still lets a replica change through: the patch then
     carries `spec.replicas` alone.

   Not staggered at all: creating a Deployment (first install, a new
   instance, a Deployment deleted by hand), and the RNDC key rotation's
   rollout (`trigger_deployment_rollout`), because the operator starts using
   the new key the moment it is written; holding the pods on the old key
   would fail every RNDC call to them.

3. **The conflict set** of instance X is every other instance that:
   - **serves a zone X serves**: some `DNSZone`'s `status.bind9Instances`
     lists both. This is the relationship that matters: two nameservers of
     the same zone must not take their handover gap together; or
   - **belongs to the same cluster**: the same `spec.clusterRef` in the same
     namespace (a `Bind9Cluster`), or the same `spec.clusterRef` naming a
     `ClusterBind9Provider` in any namespace.

   The cluster rule is the safety net. A zone's `status.bind9Instances` is
   written by the zone controller and can lag (a zone just created, a
   selector just changed, a status write that failed), and the instances of
   one cluster are the ones a shared ConfigMap change rolls together. It
   costs little: within one cluster, rollouts become sequential, which is
   what the zone rule would ask for anyway in the usual layout (every zone of
   a cluster on its primaries and secondaries). Instances that share neither
   a zone nor a cluster roll in parallel. The relation is symmetric and not
   transitive: if A and B share a zone and B and C share another, A and C
   may roll together, and each zone still loses at most one nameserver at a
   time.

   An instance being deleted is not in anyone's conflict set. In
   namespace-restricted mode, an instance in a namespace the operator does not
   watch is invisible and cannot block.

4. **Mid-rollout** is read from the shared Deployment and Pod stores, never
   from the API (`peer_rollout`):
   - no Deployment: idle (creation is not a rollout);
   - `Progressing=False`, reason `ProgressDeadlineExceeded`: **stalled**,
     which does **not** block (decision 5);
   - `status.observedGeneration` < `metadata.generation`: rolling;
   - `spec.replicas` = 0: idle;
   - `Progressing` reason `NewReplicaSetAvailable` with
     `status.updatedReplicas` and `status.replicas` equal to `spec.replicas`:
     idle. This is the Deployment controller's own "rollout complete"; an
     instance that lost a pod after its rollout completed (crash loop, node
     loss) is degraded, not rolling, and does not block: the Deployment
     controller never times such a state out, so blocking on it could last
     forever;
   - otherwise, rolling when `status.replicas` > `spec.replicas` (a surge
     pod), or `updatedReplicas`, `availableReplicas` or `readyReplicas` <
     `spec.replicas`, or a non-terminating pod of the instance is not `Ready`
     (its zones-loaded gate is still closed).

   A terminating old pod does not count: by then its replacement is Ready
   (`maxUnavailable` 0 and the zones-loaded gate), and the next instance's
   own handover is at least that instance's pod start plus its zone load
   away.

5. **Ordering, with no deadlock and no starvation.** A process-wide
   `RolloutQueue` (one per operator; only the leader reconciles) holds two
   things, both rebuilt from the cluster after a restart:
   - **waiting** instances, each with a sequence number taken the first time
     it is deferred (first come, first served);
   - **claims**: an instance that has decided to roll holds a claim from the
     decision until the Deployment store shows its patch (store generation
     above the generation it decided on), or the patch fails (released at
     once) or changes nothing (decision 8). The claim closes the window in which two
     instances reconciling at the same moment would both read an idle store
     and both roll.

   Under one lock, X proceeds unless (`decide_rollout`):
   - a conflicting instance is rolling (decision 4), or holds a claim; or
   - a conflicting instance has been waiting longer (lower sequence number).

   The first blocker in a fixed order (rolling, then claimed, then the
   earliest waiter; ties by `namespace/name`) is named in status.

   *No deadlock.* "Rolling" is progress Kubernetes makes on its own: it ends
   in a completed rollout or in `ProgressDeadlineExceeded`, and a stalled
   rollout no longer blocks. A claim ends with its patch. Between waiters,
   "waits for" follows strictly increasing sequence numbers, so it has no
   cycle: two instances can never defer on each other, and the earliest
   waiter of any conflict set only ever waits for a rolling or claimed
   instance. *No starvation.* First come, first served: a waiter is only
   ever overtaken by instances queued before it, and a stuck instance blocks
   its peers for at most its Deployment's `progressDeadlineSeconds` (600 s by
   default). A waiter whose change goes away (spec reverted, instance
   deleted) leaves the queue and wakes the others.

6. **Event-driven (ADR-0016), no timer.** A deferred instance returns
   `await_change`. It is woken by:
   - the Deployment stream, through a mapper filtered (`changed_only`) on
     what decision 4 reads (generation, observed generation, replica counts,
     the `Progressing` condition): a conflicting instance's Deployment
     progressing, completing or stalling wakes the waiting instances in its
     conflict set (`waiters_to_wake`);
   - the BIND9 Pod stream, filtered on pod readiness and deletion, for the
     same reason (a gated pod turning Ready);
   - the queue itself: an instance that releases a claim without rolling, or
     leaves the queue, wakes the waiters through a channel merged into each
     controller's watches, so no wait depends on an event that will not
     come;
   - its own events, as before.

   **A queued instance never takes the reconcile short-cut.** It does not
   advance `status.observedGeneration` or `status.observedParentGeneration`,
   and while its status carries the queued condition the next reconcile
   always runs the resource step, so the deferred change is applied when it
   is woken whatever caused it.

7. **Status.** A waiting instance carries
   `type: Rollout, status: "False", reason: RolloutQueued` with a message
   naming the instance it waits for and why (rolling, claimed, queued
   earlier). Its `Ready` condition keeps reporting its pods, which keep
   serving the previous configuration. An instance that rolled while a
   conflicting peer's rollout was stalled carries
   `type: Rollout, status: "True", reason: RolloutPeerStalled` naming that
   peer, until its next status write.

8. **A template patch that changes nothing is not a rollout** (amended
   2026-10-07). Two parts:
   - **The drift check compares semantically.** The API server defaults and
     canonicalises what it stores: an absent `resources` comes back as `{}`,
     `0.5` CPU as `500m`, an env `value: ""` as absent, a `fieldRef` gains
     `apiVersion: v1`, an absent `imagePullPolicy` becomes the image's
     default. Every pod-template field bindy owns is compared with those
     differences absorbed (`bind9instance/template_drift.rs`), so a
     Deployment bindy rendered compares equal to itself as stored. A
     regression test builds the desired Deployment from a sanitized capture
     of a live instance, cluster and ConfigMap and requires no difference
     from the captured live Deployment.
   - **Defence in depth: a no-op patch is remembered.** If a template patch
     still bumps no `metadata.generation`, the drift check was wrong. The
     instance drops its claim, leaves the waiting list, reports no
     `Rollout` condition, and the queue remembers the patch (its
     fingerprint and the generation it was sent to) as a known no-op
     (`RolloutQueue::finish_template_patch`). Its next reconciles treat the
     template as up to date while both are unchanged
     (`RolloutQueue::begin_template_change`), applying only a replica
     change if there is one. The waiters are woken once (one may wait for
     the dropped claim), which cannot repeat: an instance claims, and
     wakes, at most once per known no-op. It is logged once, at `WARN`,
     naming the field the drift check saw. Like the rest of the queue, the
     memory is per process: after a restart or a leader change each
     affected instance re-learns it with one more no-op patch.

   *Why.* On rc.6 the bindcar container's `resources: {}` never equalled
   the `None` bindy renders. Before the queue that cost one no-op PATCH per
   reconcile; with it it became a hot loop once three instances
   waited at the same time: each in turn classified a template change,
   took the claim, sent a PATCH that bumped nothing, released and woke the
   others, which did the same. About two instance reconciles and two no-op
   Deployment PATCHes per second, about 40 `INFO` lines a minute, and
   instances left with `RolloutQueued` naming peers after every rollout had
   finished. The "queued behind" line is now logged at `INFO` only when an
   instance joins the queue.

## Consequences

- **A cluster-wide change rolls one nameserver at a time per zone.** For a
  cluster of N instances the rollout takes about N times one instance's
  rollout (pod start, zone load, handover) instead of one. With one replica
  and the defaults this is roughly 20 to 30 s per instance.
- **The residual gap is MetalLB's, once per instance.** With
  `externalTrafficPolicy: Local` and one replica, each handover still drops
  traffic to that instance's IP until clients take the gratuitous ARP; the
  zone's other nameservers answer meanwhile. `externalTrafficPolicy:
  Cluster`, where source IPs are not needed, removes it; more than one
  replica per instance, spread over nodes, makes it rarer.
- **Stalled rollouts are bounded, not ignored.** A rollout stuck behind a
  pod that never becomes Ready blocks its peers until
  `progressDeadlineSeconds`, then stops blocking; the stuck instance's own
  status and Deployment condition report it.
- **A queued change is visible on the mounted ConfigMap first.** A managed
  instance shares its cluster's ConfigMap, which is updated before any
  Deployment, so a queued instance's pods already have the new files on
  disk; `named` reads them only when the pod is replaced. A container
  restart inside a queued pod starts `named` on the new configuration early.
  Accepted: the configuration was validated before it was written
  (ADR-0013), and the restart would happen regardless of the queue.
- **In-memory ordering.** The queue is per process and holds no state that
  the cluster does not: after a restart or a leader change, Deployments in
  flight are seen in the store as rolling, and waiting instances re-queue on
  their first reconcile (in the order they reconcile). A deposed leader's
  last in-flight reconcile (accepted risk 10 of the threat model) could roll
  one instance while the new leader rolls another.
- **A no-op patch costs one PATCH and one `WARN`.** A comparison bug that
  the semantic check misses can no longer loop; it shows as one `WARN` per
  instance and Deployment generation, and is fixed in the comparison.
- **Not done here.** Staggering across operators (two bindy installations
  serving the same zone) and a configurable concurrency (more than one
  instance of a conflict set at a time) are out of scope.
