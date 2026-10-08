# Failure Behaviour and Resilience

This page describes what bindy does when something fails, what you will see
while it recovers, how long recovery takes, and what bindy deliberately does
not handle. It is written for the people who run bindy. The design decisions
behind it are in [ADR-0015](https://github.com/firestoned/bindy/blob/main/docs/adr/0015-bounded-api-cost-of-dns-writes.md)
to [ADR-0019](https://github.com/firestoned/bindy/blob/main/docs/adr/0019-zone-transfer-peers-follow-pods.md);
the test that holds bindy to this behaviour is the
[chaos suite](../development/chaos-testing.md).

## At a glance

| What fails | What bindy does | What clients see (one replica per instance) |
|---|---|---|
| The operator's leader pod dies | The other replica takes the lease and carries on; every object is reconciled from the watch caches | Nothing: DNS is served by BIND9, not by the operator |
| Every operator pod is down | BIND9 keeps serving the last configuration it was given. Changes to CRs wait. On start, the operator repairs everything that drifted while it was away (deleted Services and ConfigMaps included) | Nothing, unless a BIND9 pod is also lost while the operator is down (see below) |
| A BIND9 pod is deleted or rescheduled | The replacement starts empty, the operator loads its zones and records onto it, and only then does Kubernetes route traffic to it | That instance's Service: a few seconds (measured up to 8 s) |
| `named` or the bindcar sidecar crashes (container restart) | The pod keeps its IP and its zone data; the container comes back and the pod is routed again once ready | That instance's Service: measured up to 14 s; up to 28 s when the container is killed twice in a row |
| A secondary's or primary's pod IP changes | Every primary's `allow-transfer` / `also-notify` and every secondary's `primaries` list are rewritten to the current pods | The secondary may answer `SERVFAIL` for one zone transfer while its zone is replaced |
| Every BIND9 pod of a zone is deleted at once | All are reloaded from the CRs | The zone is unanswered until the first pod is loaded (measured up to 16 s) |
| A BIND9 Service is deleted | The operator recreates it at once | That Service gets a new ClusterIP: clients using the old one fail until they learn the new one (measured up to 84 s through cluster DNS caching) |
| A change rolls every instance (upgrade, config change) | Instances that share a zone or a cluster roll one at a time; each new pod takes traffic only once loaded | At most one nameserver of a zone is changing at a time; measured at most 2 s on the rolling Service |
| A record or zone is changed or deleted while a pod is down | The change is applied to that pod once it is reachable again; a deletion is not considered done until it is confirmed on every pod that holds the zone | The status says `Degraded` until it is (see below); the old answer is never left behind silently |
| Someone edits BIND9's data directly (`nsupdate`, `rndc`, a bindcar call) | Nothing, until the owning CR's next event | The edit stays (see [Limits](#limits)) |

With **two or more replicas per instance**, losing one pod or container costs
no answers on that Service: the other replica keeps serving while the
replacement loads.

## How it works

### Everything is driven by events

Since [ADR-0016](https://github.com/firestoned/bindy/blob/main/docs/adr/0016-event-driven-reconciliation.md)
no controller re-runs on a timer. A reconcile happens when something changes:
a CR, a pod, a Service, an `Endpoints` object, a ConfigMap, or a related
object (a record's zone, a zone's instances). A failure is retried with a
per-object exponential backoff (2 s, doubling, up to 60 s), never with a
fixed resync. When nothing changes, the operator does nothing: an idle
cluster costs no reconciles and no API calls.

To force a reconcile of one object, change an annotation on it:

```bash
kubectl annotate dnszone example-com -n dns-system \
  bindy.firestoned.io/reconcile-trigger="$(date +%s)" --overwrite
```

### A new BIND9 pod takes traffic only once its zones are loaded

Zone data lives in the pod (an `emptyDir`), so a replacement pod starts with
none. Every BIND9 pod carries a readiness gate,
`bindy.firestoned.io/zones-loaded`
([ADR-0017](https://github.com/firestoned/bindy/blob/main/docs/adr/0017-zones-loaded-readiness-gate.md)).
Kubernetes does not route a Service to the pod until the operator sets that
condition to `True`, which it does only after loading every zone the pod's
instance serves (and, on a primary, replaying every record). During a
rollout the old pod keeps serving until the new one is loaded.

- A pod of an instance that serves no zone yet (a new install) is admitted at
  once (`reason: NoZones`).
- Once a pod is admitted it stays admitted: a zone created later is loaded
  onto the running pod without taking it out of service.
- A container restart inside a pod does not re-gate it: the zone data
  survives a container restart.
- A zone that cannot be loaded anywhere does not hold a new pod out forever:
  the pod is admitted as `ZonesPartiallyLoaded`, naming the missing zones.
- When a pod starts terminating, the operator closes its gate at once
  (`reason: PodTerminating`). The pod's `Ready` condition follows on the
  kubelet's next status sync (measured about 18 s later), so traffic moves to
  the new pod mainly because the new pod is `Ready` first.

If the operator is down, new BIND9 pods stay out of their Service: this is
deliberate, since an empty pod would answer `REFUSED` for every zone.

### Rollouts are staggered

When a change would roll several instances at once (a bindy upgrade that
changes the rendered configuration, a shared cluster ConfigMap, a bindcar
setting), instances that serve a common zone, or belong to the same
`Bind9Cluster` / `ClusterBind9Provider`, roll one at a time
([ADR-0018](https://github.com/firestoned/bindy/blob/main/docs/adr/0018-staggered-bind9-rollouts.md)).
A waiting instance shows `Rollout=False`, `reason: RolloutQueued`, naming the
instance it waits for; its current pods keep serving. A rollout stuck past
its Deployment's `progressDeadlineSeconds` (600 s by default) stops blocking
the others. Creating an instance, changing its replica count, and rotating
its RNDC key are never queued.

### Zone transfers follow the pods

A primary allows transfers to, and notifies, its secondaries; a secondary
transfers from its primaries. Pods get new IPs whenever they are replaced, so
the operator computes these lists from the live pods on every zone reconcile
and rewrites them on every server whenever they change
([ADR-0019](https://github.com/firestoned/bindy/blob/main/docs/adr/0019-zone-transfer-peers-follow-pods.md)).
The lists last applied are recorded in `DNSZone.status.transferPeers`. A new
secondary pod is allowed to transfer before it is loaded.

### Writes and deletions are confirmed on every pod

A record write or deletion is complete only when every pod that holds the
zone has applied it
([ADR-0015](https://github.com/firestoned/bindy/blob/main/docs/adr/0015-bounded-api-cost-of-dns-writes.md),
decision 7). A pod whose container is restarting still holds its zone data,
so it is not skipped: the operator retries until the pod answers. Only a pod
that is gone (and its data with it) is skipped. A deleted record stays
tracked by its zone until its data is confirmed gone everywhere.

### Status tells the truth

`Ready=True` on a `DNSZone` means every instance actually serves the zone,
including every secondary having **loaded** it, not merely having it
configured. Otherwise the zone is `Degraded` with one of these reasons:

| Reason | Meaning | Usually resolves when |
|---|---|---|
| `SecondaryNotLoaded` | A secondary has the zone configured but no data (the message names the instance and endpoint) | Its transfer completes |
| `TransferPeersNotUpdated` | A primary's `allow-transfer` / `also-notify` could not be rewritten | The primary's bindcar is reachable again |
| `NoTransferSource` | No primary pod has its zones loaded yet | A primary pod is admitted |
| `RecordDeletionPending` | A deleted or unselected record is not yet confirmed gone from every pod | The pod holding it is reachable again |

Related conditions elsewhere: `Rollout=False/RolloutQueued` on a
`Bind9Instance` (see above), and the `bindy.firestoned.io/zones-loaded`
condition on each BIND9 pod (`ZonesLoading`, `ZonesLoaded`, `NoZones`,
`ZonesPartiallyLoaded`, `PodTerminating`). See
[Status Conditions](status.md) and [Troubleshooting](troubleshooting.md).

## Measured behaviour

The chaos suite runs on a 3-node kind cluster with one `Bind9Cluster` (two
primaries and one secondary, one replica each), a standalone primary, three
zones, and two operator replicas. It attacks the operator, every BIND9 pod
and both containers, rolls the cluster, and changes records and zones while
pods are down; after every attack it checks that every pod serves exactly the
expected data, the transfer lists name exactly the live pods, every status is
truthful, and the operator goes quiet. Measured across three full runs
(each a fixed-order and a randomized pass over fifteen steps, 31 checked
steps per run, all passing):

| | Measured |
|---|---|
| Convergence after a single pod or container failure | about 30 s (27 to 43 s) |
| Convergence after every BIND9 pod is deleted | 42 to 114 s |
| Operator reconciles in the 60 s after convergence | 0 to 3 |
| A zone with no answering nameserver | never, except when every pod is deleted on purpose (up to 16 s) |
| Longest gap on a Service the attack did not touch | 2 s |
| Longest gap on a rolling Service during a staggered rollout | 2 s |

On a production cluster (two primaries behind MetalLB layer-2
`LoadBalancer`s, one secondary):

- Before rollouts were staggered and the gate existed, an upgrade that rolled
  every BIND9 pod left the zone unanswered on both nameservers for 9 to 12 s.
- With both, the same kind of rollout lost 2 of 136 probe queries (isolated
  single timeouts at each primary's handover), and at least one nameserver
  answered throughout.

## Recommendations

- **Run two or more replicas per instance** for any zone you cannot afford to
  drop for seconds. Every single-replica gap above disappears for pod and
  container failures.
- **Run the operator with two replicas.** Leadership moves in seconds and no
  change is lost.
- **Name more than one nameserver in your zones' NS records,** on different
  instances. Resolvers then retry the other while one is rolling.
- **`externalTrafficPolicy` on `LoadBalancer` Services:** `Cluster` has no
  handover gap. `Local` preserves client source IPs (useful for ACLs and rate
  limiting) but, with MetalLB in layer-2 mode, the address is announced from
  the node running the pod, so replacing that pod moves the announcement and
  costs a short gap. More replicas make it less frequent, not shorter.
- **Do not edit BIND9's data by hand.** Make the change in the CRs.

## Limits

These are accepted, documented trade-offs (see the
[threat model](../security/threat-model.md), accepted risks 16 and 17):

- **Out-of-band changes inside BIND9 are not reverted automatically.** There is
  no Kubernetes event for them. They stay until the owning CR's next event, a
  pod restart, an operator restart, or a forced reconcile (annotation above).
  The RNDC/TSIG keys and bindcar's TokenReview check are what prevent them.
- **NOTIFY goes to the secondary Service's ClusterIP, not to each pod.** With
  several secondary replicas, a NOTIFY reaches one of them; the others pick
  the change up on their next refresh. No NOTIFY is sent to a headless
  Service.
- **A secondary's zone is replaced when its primaries move,** so that
  secondary answers `SERVFAIL` for that zone for the duration of one zone
  transfer, about twice per primary rollout.
- **A single-replica instance has the gaps listed above** when its pod or a
  container fails, bounded by the time to reload one pod.
- **The order of queued rollouts is held in memory.** After an operator
  restart or leader change, waiting instances re-queue in a new order.

## Upgrading

Upgrading to the release that contains these changes:

- every BIND9 pod rolls once (the readiness gate is added to the pod
  template), staggered as described above;
- the first reconcile of each zone rewrites every primary's transfer lists
  and replaces every secondary's zone once, which also repairs any secondary
  that had lost its copy of a zone;
- zones created before this release keep their old `also-notify` until their
  pod is next replaced; their transfers are fixed at once. One
  `kubectl rollout restart` of the BIND9 Deployments brings every zone fully
  up to date.

See the [Migration Guide](migration-guide.md) for the full upgrade notes.
