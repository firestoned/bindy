# 0017: BIND9 pods are Ready only once their zones are loaded

- **Status:** Accepted
- **Date:** 2026-10-06
- **Deciders:** Erick Bourgeois
- **Amended:** 2026-10-07 (Decision 6: a terminating pod's gate is closed at the start of its termination, the one exception to the one-way latch; found rolling v0.8.0-rc.5)
- **Amended:** 2026-10-07 (Decision 6 corrected against v0.8.0-rc.6: the gate condition flips at once, but the pod's `Ready` follows only on the kubelet's next status sync, about 18 s later; the flip does not shorten the handover, the new pod being Ready first and ADR-0018's staggering do)
- **Amended:** 2026-10-07 by [ADR-0019](0019-zone-transfer-peers-follow-pods.md) (Decision 2: for a secondary pod the gate first rewrites every primary's `allow-transfer` / `also-notify` to the zone's current peers, then treats a zone not transferred yet as a load failure, so a pod whose sibling still serves the zone waits for its transfer; transfer sources are the admitted primary pods only)
- **Related:** Builds on [ADR-0016](0016-event-driven-reconciliation.md) (every wake is a watch event or a backoff retry), [ADR-0015](0015-bounded-api-cost-of-dns-writes.md) (`InstanceResolver`, endpoints from the store, record replay), [ADR-0009](0009-workspace-crate-split-and-shared-watch-layer.md) §3 (one shared watch per kind) and [ADR-0013](0013-validate-and-render-bind9-config-with-hornet.md) (a config change rolls every pod)

## Context

Each `Bind9Instance` is a one-replica Deployment. Its pods keep zone data on
an `emptyDir`, so a replacement pod starts with no zones at all. The pod's
readiness, until now, only asked whether its containers were up:

- `bind9`: a TCP probe on 5353, which `named` answers with or without zones;
- `api` (bindcar): `GET /api/v1/ready`, which does not look at zones either.

The Deployment rolls with the default `RollingUpdate` (`maxSurge` 25%, which
rounds up to 1; `maxUnavailable` 25%, which rounds down to 0). So during a
rollout the new pod goes Ready within seconds, the Service switches to it, the
old pod is terminated, and only then does the `DNSZone` controller notice the
`Endpoints` change and replay the zones (`zones_for_endpoints`,
`replay_records_if_zone_was_recreated`). Until it finishes, the new pod
answers `REFUSED` for every zone it should serve. A change that rolls every
primary at once (an operator upgrade that changes the rendered `named.conf`,
ADR-0013 stage 3) empties every nameserver of a zone at the same moment.

Two facts constrain the fix:

1. **Kubernetes already has the mechanism.** A pod's `Ready` condition is
   `True` only when every container is ready *and* every condition listed in
   `spec.readinessGates` is `True`. The `ContainersReady` condition keeps
   reporting the containers on their own. Kubernetes leaves the gate
   condition to whoever owns it, written through the `pods/status`
   subresource; the kubelet preserves conditions it does not manage.
2. **A gated pod is not in the Service's ready endpoints, and the operator
   writes only to ready endpoints.** Zone configuration and record writes
   resolve an instance's pods from `Endpoints.subsets[].addresses`
   (`ready_endpoint_addresses`). A pod held back by a gate is listed under
   `notReadyAddresses`, so a gate alone deadlocks: the pod waits for zones,
   and zones are never written to a pod that is not Ready. EndpointSlice does
   not help: its `serving` condition "maps to the Pod's `Ready` condition"
   (Kubernetes docs, EndpointSlice conditions), which includes the readiness
   gates; `serving` differs from `ready` only for terminating endpoints. The
   one signal that says "the containers are up but the pod is held back" is
   the pod's own `ContainersReady` condition.

## Decision

1. **Every BIND9 pod carries a readiness gate.** `build_pod_spec`
   (`crates/bindy-bind9/src/bind9_resources.rs`) adds
   `readinessGates: [{conditionType: bindy.firestoned.io/zones-loaded}]`
   (`bindy_api::constants::ZONES_LOADED_CONDITION_TYPE`). The instance
   controller treats a missing or different gate as Deployment drift and
   patches it in (`deployment_needs_update`), which rolls the pods once.

2. **A zones-loaded gate controller sets the condition.** It lives in the
   `bindy-controller-zone` crate (`zones_gate.rs`), next to the zone logic it
   reuses, and runs one controller per namespace target beside the `DNSZone`
   controller. Its primary resource is the Pod, from a new label-selected
   Pod watch in the shared `WatchSet`
   (`app.kubernetes.io/part-of=bindy,app.kubernetes.io/component=dns-server`,
   `bindy_api::labels::BIND9_POD_SELECTOR`), so its cache holds only bindy's
   BIND9 pods.

   For a pod that has the gate, is not terminating, has an IP and reports
   `ContainersReady=True`, and whose gate condition is not already `True`, it:

   1. resolves the pod's `Bind9Instance` (pod label
      `app.kubernetes.io/instance`) from the store;
   2. computes the **required zones**: every `DNSZone` that selects the
      instance (its `bind9InstancesFrom` selector matches and the F-003
      namespace gate allows it), that is not being deleted, that is not a
      `DuplicateZone` loser, and that is **live**, meaning its
      `status.bind9Instances` lists at least one instance as `Configured`;
   3. with no required zone, sets the condition `True`
      (`reason: NoZones`) at once;
   4. otherwise sets it `False` (`reason: ZonesLoading`) and loads every
      required zone **onto this one pod**, through an `InstanceResolver`
      whose lookup only returns this pod's address (`SinglePodLookup`):
      - primary: the same `add_dnszone` path the zone controller runs
        (zone, also-notify and allow-transfer for the secondaries, NS and
        glue records, DNSSEC policy), then a replay of every record the store
        holds tagged with the zone (`status.zoneRef`), not terminating;
      - secondary: the primaries' `allow-transfer` / `also-notify` are
        rewritten to the zone's current peers first (ADR-0019), then the same
        `add_dnszone_to_secondaries` path (zone with its primaries, the
        admitted primary pods, then `retransfer`). A zone not transferred yet
        counts as not loaded (amended by ADR-0019; it was not awaited);
   5. decides from the result of every load (`gate_outcome`):
      - every zone loaded and every record replayed: `True`
        (`reason: ZonesLoaded`);
      - a zone failed to load and **another Ready pod of the instance serves
        it** (bindcar on a sibling address from the Service's ready endpoints
        reports the zone; a sibling that cannot be asked counts as serving):
        `False` (`reason: ZonesLoadFailed`, the failures in the message), and
        a retry with the per-object backoff. Admitting the pod would lose a
        zone that is served today; the old pod keeps serving meanwhile;
      - a zone failed to load and **no other Ready pod of the instance serves
        it**: that zone does not block. The pod is admitted `True`
        (`reason: ZonesPartiallyLoaded`, the zones not loaded in the message)
        and the `DNSZone` controller keeps retrying the zone as it does for
        any degraded zone. This is the case of a zone that is invalid
        everywhere, or of the only replica of an instance being replaced:
        holding the pod back would protect nothing and would keep every
        other zone of the instance out of service.

   The condition is written with a strategic merge patch of
   `status.conditions` on `pods/status`, which merges by condition `type` and
   leaves every other condition alone. An unchanged condition is not
   re-written.

   **The gate is a one-way latch per pod** (with one exception, a pod that
   is terminating, decision 6). Once `True` it is never evaluated
   again for that pod (`gate_step` returns `Done`, and the zone mapper skips
   admitted pods). A zone created, or newly selecting the instance, after the
   pod went Ready never pulls the pod back out of its Service, which would
   drop every zone it already serves: the zone is configured on the running
   pod by the `DNSZone` controller as before, and is simply not served until
   that is done, which is how a brand-new zone has always behaved. Only a new
   pod (rollout, restart, rescheduling, scale-up) starts with the gate
   closed.

   **The required set is taken when the pod is evaluated.** A zone that
   becomes live, or starts selecting the instance, while the pod is still
   gated wakes the pod again (decision 4) and is included in that
   evaluation. A zone that appears in the instant between the last
   evaluation and the gate opening is not waited for, but it still reaches
   the pod: the `DNSZone` controller writes to the gated pod too
   (decision 3).

   **A container restart does not re-gate the pod.** The pod object, its
   condition and its `emptyDir` volumes survive a container restart. `named`'s
   working directory (`/var/cache/bind`, where `rndc addzone` keeps its
   new-zone database and the zone files and journals live) and the zones
   directory are both `emptyDir` volumes, which last as long as the pod, so
   the restarted `named` loads the same zones. The pod drops out of the
   Service while its containers are not ready (`ContainersReady=False`) and
   returns when they are, with the gate still `True`.

   **Why only live zones.** The guarantee that matters is "a replacement pod
   serves at least what its predecessors served". A zone that no instance has
   ever served (brand new, or broken from birth) cannot regress by admitting
   the pod, and making it block would let one tenant's broken `DNSZone` hold
   a shared instance out of Service on every rollout. A zone that is live
   elsewhere but fails on this pod does block it, which is the point.

   **Why the gate controller loads the zones itself** rather than waiting for
   the `DNSZone` controller to report them. A per-pod "zone Z is fully loaded
   on pod P" fact exists nowhere durable: `DNSZone` status is per instance,
   and "the zone exists on the pod" is not enough for a primary, whose
   records may still be replaying. The reconcile that admits the pod is the
   reconcile that made it complete, so the decision needs no cross-controller
   state, survives an operator restart (the Pod watch's initial list
   re-runs every unfinished pod), and costs what the existing post-rollout
   replay cost: the zone controller, woken by the `Endpoints` change after
   the pod turns Ready, finds every zone already present and replays nothing.

3. **Writers reach container-ready pods, not only Ready ones.** The
   production `InstanceLookup` (`KubeInstanceLookup`) returns
   `writable_endpoint_addresses`: the ready addresses plus each
   `notReadyAddresses` entry whose Pod (by `targetRef`, from the Pod store)
   is `ContainersReady=True` and not terminating. Every BIND9 write the
   operator makes (zone add and delete, NS records, record writes, record
   replay, deletion cleanup) therefore also reaches a gated pod. A pod whose
   containers are not ready is still skipped, as before.

   This is what keeps a write made during the window from being lost on the
   new pod. A record created or changed while the pod is gated is written by
   the record controller to the gated pod as well. If that write lands
   before the gate controller created the zone on the pod it fails there,
   but the record controller acts only after the record (with its
   `status.zoneRef`) is in the store, and the gate controller reads the store
   after creating the zone, so the record is in its replay. A zone created
   or deleted during the window reaches the gated pod through the zone
   controller.

4. **Event-driven, no timer (ADR-0016).** The gate controller is woken by:
   - the Pod's own events, filtered (`changed_only`) on what it reads:
     `ContainersReady`, the gate condition, the IP and deletion. The pod
     appearing, and its containers turning ready, are both Pod events;
   - a `DNSZone` mapper (`gated_pods_for_zone`), filtered on the zone's live
     instance set, selectors and deletion: a zone becoming live, changing
     selectors or being deleted wakes the gated pods of every instance it
     selects;
   - its own backoff retry after a failed load.

5. **RBAC.** The operator gains `get` and `patch` on `pods/status`, in the
   cluster-wide role and the per-namespace Role of namespace-restricted
   mode. It keeps read-only access to `pods`: it cannot change a pod's spec,
   labels or delete it.

6. **A terminating pod's gate is closed at the start of its termination**
   (amended 2026-10-07, corrected the same day against v0.8.0-rc.6). When a
   pod that carries the gate, with the condition `True`, gets a
   `deletionTimestamp`, the gate controller sets the condition `False`
   (`reason: PodTerminating`) at once (`gate_step` returns
   `CloseForTermination`). A pod without the gate, a pod whose gate is
   already `False` or was never set, and a pod that is not terminating are
   left alone.

   *What the flip does, and what it does not.* The condition itself is
   `False` from the first second, for anything that reads it. The pod's
   `Ready` condition is not: the kubelet recomputes it on its own pod
   status sync (or the next probe result), not on the gate patch. On rc.6
   the gate went `False` / `PodTerminating` at the deletion event and the
   pod's `Ready` turned `False` about 18 s later, after the 10 s preStop
   drain and after `named` had exited. The EndpointSlice `serving` flag
   follows `Ready`, so the flip does not move traffic earlier and does not
   shorten the handover. It is kept: it is harmless (the pod is leaving its
   Service anyway) and immediate for anything that reads the condition.
   What made the rc.6 handovers smooth is that the new pod
   was Ready before the old one was deleted (`maxSurge` 1 /
   `maxUnavailable` 0 and the zones-loaded gate), MetalLB moved the
   announcement to the new pod's node, and ADR-0018 kept the zone's other
   nameservers up while each one handed over.

   *Evidence (v0.8.0-rc.5, 2026-10-07).* One zone on three instances (two
   primaries behind MetalLB layer-2 `LoadBalancer` Services with
   `externalTrafficPolicy: Local`, one secondary on a `ClusterIP`), one
   replica each, `maxSurge` 1 / `maxUnavailable` 0, preStop drain 10 s,
   readiness probe 5 s x 3. The upgrade changed the rendered configuration,
   so all three Deployments rolled at the same instant. The gate did its job
   (no `REFUSED` at any time). But from the moment the old `named` exited at
   the end of its drain until the kubelet's readiness probe failed (up to
   15 s), the old pod was still `Ready`, so its endpoint stayed
   `serving: true`. MetalLB, which picks the announcing node from `serving`
   endpoints (issue #2074, below), kept announcing from the old node, and kube-proxy
   there, with no ready local endpoint left, fell back to the local
   serving-terminating one (ProxyTerminatingEndpoints) whose `named` had
   exited. Every query to both load balancer IPs timed out for 9 to 12 s.

   *Evidence (v0.8.0-rc.6, 2026-10-07).* The same cluster, a configuration
   change rolled with ADR-0018's staggering, one instance at a time. The
   gate condition went `False` (`PodTerminating`) at each old pod's
   deletion; the pod's `Ready` turned `False` about 18 s later. A DNS probe
   against both `LoadBalancer` IPs lost 2 of 136 queries over the whole
   rollout, isolated single timeouts, one at each primary's handover,
   against 9 to 12 s of total outage on both IPs during rc.5's simultaneous
   rollout.

   *Why the latch exception is safe.* The latch exists so a serving pod is
   never pulled out of its Service by a later zone (decision 2). A
   terminating pod is leaving its Service anyway, and Kubernetes already
   counts it as not `ready`; the only thing the latch preserved was
   `serving: true` for the drain (which, as rc.6 showed, the pod keeps
   until the kubelet's next status sync anyway). The rule is narrow: it
   fires only on
   `deletionTimestamp`, only from `True` to `False`, and a terminating pod is
   never evaluated for admission again.

   *Mechanics.* The Pod watch already wakes on deletion (`pod_gate_key`
   hashes `deletionTimestamp`). The patch is issued in that one reconcile,
   with no wait or backoff in front of it; a failed patch (other than the
   pod being gone) returns an error and is retried with the controller's
   per-object backoff. A pod that is already gone (`404`) is done. An
   instance scaled to zero or deleted goes through the same path: its pods
   get the flip and then go away, which changes nothing for traffic since no
   pod of that instance remains.

   *What Kubernetes guarantees, and what it does not.* Sources relied on:
   - Kubernetes, [Pod readiness](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#pod-readiness-gate):
     a pod with readiness gates is Ready only when every container is ready
     *and* every gate condition is `True`; operators write the condition with
     a `PATCH` of the status. The kubelet folds the gate into `Ready` when it
     next syncs the pod's status; rc.6 measured that at about 18 s after the
     gate flip on a terminating pod, so `Ready` is not immediate.
   - Kubernetes, [EndpointSlice conditions](https://kubernetes.io/docs/concepts/services-networking/endpoint-slices/#conditions):
     `serving` "maps to the Pod's `Ready` condition"; `terminating` is set
     "when the Pod is first deleted"; `ready` is "`serving` and not
     `terminating`"; service proxies "may route traffic to endpoints that are
     both `serving` and `terminating` if all available endpoints are
     `terminating`".
   - Kubernetes, [Pod termination](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#pod-termination)
     and KEP-1669 (ProxyTerminatingEndpoints, GA in v1.28): the kubelet keeps
     running readiness probes during termination, and kube-proxy falls back
     to serving terminating endpoints only when no ready endpoint is left
     (per node, for `externalTrafficPolicy: Local`).
   - MetalLB, [layer 2 concepts](https://metallb.universe.tf/concepts/layer2/)
     and [traffic policies](https://metallb.universe.tf/usage/#traffic-policies):
     one node announces the IP; with `Local` only a node with a usable local
     endpoint announces, and kube-proxy there sends traffic only to the
     node's own pods; failover "happens within a few seconds" for clients
     that honour gratuitous ARP, longer for those that do not. MetalLB issue
     [#2074](https://github.com/metallb/metallb/issues/2074) (PR #2088)
     switched the layer-2 speaker from `ready` to `serving`, which is why the
     old node kept the announcement while the old pod was terminating but
     still Ready (the rc.5 evidence matches; the MetalLB release that carries
     the change was not pinned down).

   *`externalTrafficPolicy: Local` versus `Cluster`.* With `Cluster`, every
   node forwards to every ready endpoint, so traffic moves to the new pod as
   soon as the old one is not `serving`: no gap. With `Local`, MetalLB moves
   the announcement to the new pod's node once that node has a usable
   endpoint and the old one stops being eligible; between that move and
   clients learning the new MAC (a gratuitous ARP, normally within a few
   seconds, MetalLB's own reaction time), packets that still reach the old
   node can be dropped. That residual gap is a property of layer-2 failover
   with `Local`, not of bindy; on rc.6 it cost one query timeout per primary
   handover. `externalTrafficPolicy: Cluster`, where the
   client source IP is not needed, removes it; more than one replica per
   instance, spread over nodes, makes it rarer (only replacing the pod on the
   announcing node moves the announcement) without removing it.

   *The preStop drain stays at 10 s* (`BIND9_PRESTOP_DRAIN_SECS`). It was
   sized as the sum of the kubelet re-evaluating `Ready` (assumed well under
   a second), kube-proxy's sync (about a second), MetalLB re-announcing and
   the gratuitous ARP reaching clients ("a few seconds"), and one full
   stub-resolver retry interval (5 s by default in glibc's `resolv.conf`).
   rc.6 showed the first term is wrong (`Ready` lags the flip by about
   18 s), so the drain does not cover the old pod's whole time as a
   `serving` endpoint. It is kept because the measured client impact with
   the new pod Ready first and staggering in place was two isolated
   timeouts in a full rollout; a longer drain would only delay each
   rollout. The termination grace period
   (`BIND9_TERMINATION_GRACE_PERIOD_SECS`, 45 s) is unchanged.

## Consequences

- **A rollout no longer empties a nameserver.** The old pod keeps serving
  (`maxUnavailable` 0) until the new one has every live zone and record;
  `kubectl rollout status` completes when the gate turns `True`.
- **Upgrade.** Adding the gate changes the pod template, so every BIND9 pod
  rolls once (an ADR-0013 stage 3 upgrade rolls them anyway). Pods created
  before the upgrade have no gate and are not touched. The new RBAC must be
  applied with, or before, the new operator; without it the gate cannot be
  set and new pods stay out of Service while the old ones keep serving.
- **Fail-safe, not fail-open.** If the operator is down, or older than the
  pod template (it does not know the gate), a new pod stays not Ready. Under
  `maxUnavailable` 0 the old pod keeps serving, so a rollout stalls rather
  than serving empty zones. The Deployment's `progressDeadlineSeconds`
  reports the stall; the pod's condition message says why.
- **First install, scale-up, zero zones.** No deadlock: a pod whose
  instance no live zone selects (a new setup, or an instance with zero
  zones) is admitted (`NoZones`) as soon as its containers are ready, without
  waiting for any zone to exist; a pod of a new instance (scale-up) gets
  every live zone that selects it loaded before it is admitted, and a zone it
  cannot load does not block it (no other pod of the new instance serves it);
  zones are loaded on a gated pod directly, so nothing waits on the pod being
  Ready. On a first install no zone is live yet, so the first pods are
  admitted before their zones load, which is today's behaviour.
- **A zone that fails only on the new pod delays a rollout.** If a zone the
  old pod serves cannot be loaded on the new pod, the new pod stays not Ready
  with the failure in `ZonesLoadFailed`, retried with backoff. Availability
  is kept by the old pod; the rollout waits for the fix. A zone that fails on
  every pod of the instance never blocks (`ZonesPartiallyLoaded`), so no
  zone can hold an instance out of service on its own.
- **`publishNotReadyAddresses` bypasses the gate.** It is a user-settable
  field of the instance's Service spec; a Service that sets it routes to
  pods regardless of readiness, gate included.
- **New write paths.** The operator now writes `pods/status`. A holder of the
  operator's ServiceAccount token can mark a BIND9 pod Ready before its zones
  load (re-opening the empty-pod window) or hold one out of Service; it
  cannot change what the pod runs. Recorded in the threat model (M-49).
- **More cached objects.** One more watch per namespace target, holding only
  bindy's BIND9 pods.
- **Endpoints v1.** The operator still reads `Endpoints`, which Kubernetes
  deprecated in favour of EndpointSlice (v1.33). EndpointSlice would not
  remove the Pod watch, because its `serving` condition includes the gate;
  moving to it is separate work.
- **A terminating pod's gate reads `False` from its first second**
  (decision 6). The pod's `Ready` follows on the kubelet's next status sync
  (about 18 s on rc.6), so `kubectl get pods` shows the old pod `2/2` for
  part of its drain. The flip does not move traffic earlier. The handover is
  kept short by the new pod being Ready before the old one is deleted and,
  with `externalTrafficPolicy: Local` and one replica, MetalLB's layer-2
  re-announcement is the remaining gap; ADR-0018 keeps two nameservers of a
  zone from taking that gap at the same time (rc.6: 2 of 136 probe queries
  lost over a full staggered rollout).
- **Not done here.** Nothing else in bindy reads pod readiness as "has
  zones": the `Bind9Instance` status reports pod `Ready`, which now also
  means "zones loaded", and zone placement and selection never looked at pod
  readiness.
