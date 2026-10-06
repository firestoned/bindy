# 0016: Event-driven reconciliation, no periodic resync

- **Status:** Accepted
- **Date:** 2026-10-06
- **Deciders:** Erick Bourgeois
- **Related:** Builds on [ADR-0009](0009-workspace-crate-split-and-shared-watch-layer.md) §3 (one shared watch per kind) and §4 (self-trigger policy), and on [ADR-0015](0015-bounded-api-cost-of-dns-writes.md) (resolver, cached lookups)

## Context

The v0.8.0-rc.3 load test (300 `ARecord`s, one zone, 3 primary
`Bind9Instance`s) still showed the leader spending most of its Kubernetes API
budget on work that changed nothing. Over the leader's lifetime: 2,668
`ARecord` reconciles (about 9 per record), 3,595 GETs of `arecords`, 1,681 of
`dnszones`, 1,190 of `bind9instances`, 1,116 of `secrets` and 650 of
`endpoints`. Traced in code:

1. **Timed requeues.** Every controller requeued itself after a successful
   reconcile. Records and `DNSZone` requeued after 300 s when Ready and after
   30 s when not (`requeue_based_on_readiness`, the zone wrapper);
   `Bind9Instance`, `Bind9Cluster` and `ClusterBind9Provider` requeued after
   300 s through the SDK's `instrumented`. At rest 300 records cost about one
   full reconcile per second forever, each re-pushing the record to every
   primary.
2. **Waiting on a timer.** A new record the zone had not tagged yet ended
   `NotSelected` and requeued every 30 s: 969 such reconciles in the run. The
   record controller already wakes on a change of `status.zoneRef`, which is
   exactly the event that ends that wait.
3. **Re-reading what the reconcile already had.** Each record reconcile made
   about 3 GETs when nothing changed: `update_record_status` fetched the record
   before deciding whether to patch, the wrapper fetched it again only to
   log Ready or NotReady, and `get_zone_from_ref` fetched the `DNSZone`
   instead of reading the store. The zone wrapper fetched the zone again
   after every reconcile to pick the requeue interval.
4. **Uncached lookups left in the `DNSZone` controller.** Instance roles,
   RNDC keys and endpoints were still read with a GET per instance per
   reconcile at about nine sites, although ADR-0015 gave the operator a
   resolver and stores for exactly this.

The timers existed as a drift backstop: if something changed and no event
arrived, the next timed reconcile would find it. The watches already cover
the changes that matter:

- The zone controller watches the `Endpoints` of every instance a zone is
  configured on. A killed or restarted BIND9 pod changes its Endpoints, the
  zone reconcile re-creates the zone and replays its records, and the intent
  survives a crash in `status.recordsResyncPending` (ADR-0015).
- The instance controller owns its ServiceAccount, Secret, ConfigMap,
  Deployment and Service, and maps zone, cluster and provider changes to the
  instances they affect. Pod readiness reaches it through the Deployment's
  status. The cluster and provider controllers own their instances and
  clusters.
- Every controller reconciles every object it holds when it starts (the
  watcher's initial list), which is how drift made while no operator ran is
  repaired (ADR-0009 §5, the restart e2e suite).

What no watch sees is a change made to BIND9 itself while the pod keeps
running: an `nsupdate`, `rndc` or bindcar call issued by hand. The timer was
the only thing that reverted those, and only within five minutes.

## Decision

1. **A successful reconcile returns `Action::await_change()`** in every
   controller: the nine record kinds, `DNSZone`, `Bind9Instance`,
   `Bind9Cluster` and `ClusterBind9Provider`. The next reconcile comes from
   a watch event. `REQUEUE_WHEN_READY_SECS`, `REQUEUE_WHEN_NOT_READY_SECS`,
   the SDK `requeue` module and `requeue_based_on_readiness` are deleted.
   Scout is out of scope and keeps its own policy.

2. **A wait on another object is not a timer.** A not-Ready outcome that
   waits on another object also returns `await_change()`, and the event of
   the awaited object wakes it:

   | Controller | Outcome | Woken by |
   |---|---|---|
   | Record | `NotSelected` (no `status.zoneRef`) | The record's primary stream passes a change of `status.zoneRef` (`zone_ref_hash`) |
   | Record | `ZoneNotFound` (the zone does not exist), `ZoneNotConfigured`, `NoPrimaryInstances` | The record controller's `DNSZone` mapper (`records_to_wake_for_zone`): every record the zone lists that is unstamped or whose cached status is not Ready. A zone that is created or gains (primary) instances writes its status, which wakes them |
   | `DNSZone` | No `bind9InstancesFrom`, or no instance matches | The `Bind9Instance` mapper (`zones_selecting_instance`) and the zone's own spec |
   | `DNSZone` | `DuplicateZone` | A new `DNSZone` mapper (`zones_contending_for_name`): a change or delete of any zone wakes the zones claiming the same name and the zones reporting `DuplicateZone` |
   | `Bind9Instance` | Parent cluster or provider missing | The `Bind9Cluster` and `ClusterBind9Provider` mappers (`instances_of_cluster`, `instances_of_provider`) |
   | `Bind9Instance` | Pods not ready | The owned Deployment's status |
   | `Bind9Instance` | Shared cluster ConfigMap deleted or edited | A new mapper on the ConfigMap watch the controller already runs (`instances_for_configmap`): a cluster-level ConfigMap maps to the instances of its cluster |
   | `Bind9Cluster`, `ClusterBind9Provider` | Instances or clusters not ready | Owned `Bind9Instance` / `Bind9Cluster` status |

   The `DNSZone` mapper is filtered (`changed_only`) on the zone name and
   deletion, and only zones in conflict are returned, so ordinary zone status
   writes do not fan out. The record mapper reads the record store and does
   no I/O (ADR-0009 §5).

3. **A failure is a retry, not a resync.** A Kubernetes API error returns
   `Err` and goes through `error_policy`'s per-object exponential backoff
   (2 s doubling to 60 s). An outcome that is `Ok` but failed against BIND9
   or bindcar retries through the same backoff, from the reconcile, with
   `sdk::error::retry_action`: a `Degraded` zone (some instance or endpoint
   rejected the zone, or a record replay is incomplete), a zone whose cleanup
   pass left work behind (a deleted or unselected record whose DNS data is
   not confirmed gone, which ADR-0015 retried "every reconcile"), a record
   whose write BIND9 rejected or could not reach, and a record whose zone or
   primaries could not be read (`InstanceFilterError`, or `ZoneNotFound` from
   an API error rather than a missing zone). A rejected record write keeps the
   existing cooldown (`retry::write_in_cooldown`): its retry is never sooner
   than `REJECTED_WRITE_COOLDOWN` (30 s), and a reconcile woken inside the
   cooldown requeues for the remaining time instead of writing. A converged
   reconcile clears the object's backoff counter.

4. **Two scheduled wakes, for instants the API cannot announce.** A
   `Bind9Instance` with RNDC auto-rotation requeues for the moment its key
   becomes due (`rotate_at`, no earlier than the minimum interval since the
   last rotation). A signed `DNSZone` whose sidecar reports the next KSK
   rollover requeues for that instant, so `status.dnssec` follows the new DS
   record. A zone whose policy is set but whose keys are still generating
   retries with the backoff of decision 3. A scheduled wake is capped at
   `MAX_SCHEDULED_WAKE` (30 days) so a far-off instant cannot overflow the
   controller's delay queue. Neither is a fixed interval.

5. **Records stop re-reading what the reconcile has.**
   `update_record_status` decides from the object the reconcile was handed
   (the watch cache) and sends a merge patch that never includes `zone` or
   `zoneRef`, so it cannot overwrite what the `DNSZone` controller wrote, and
   includes `addresses` and `publishedName` only when the reconcile sets
   them. The record reconcile returns its outcome, so the wrapper does not
   re-GET to log it. `get_zone_from_ref` reads the `DNSZone` store, with a
   GET fallback for a zone the store does not hold yet.

6. **The `DNSZone` controller uses the ADR-0015 lookups everywhere.**
   Instance roles come from the `Bind9Instance` store (primaries and
   secondaries, GET fallback), and every RNDC key and endpoint read goes
   through one `InstanceResolver` per reconcile. `for_each_primary_endpoint`
   takes a resolver. The wrapper decides its action from the reconcile's
   in-memory outcome, not from a re-GET.

7. **Out-of-band changes to BIND9 are not reverted by a timer.** A change
   made directly in BIND9 while its pod keeps running produces no
   Kubernetes event, so it stands until the next event for the object that
   owns that data: a spec, label, annotation or finalizer change, a pod
   restart (Endpoints), or an operator restart. To force a repair, change
   any annotation on the `DNSZone` or the record; the primary predicate
   passes annotation changes. The documented annotation is
   `bindy.firestoned.io/reconcile-trigger` (`BINDY_RECONCILE_TRIGGER_ANNOTATION`),
   set to a fresh value such as a timestamp. A record reconcile re-pushes the
   record; a zone reconcile re-creates a missing zone and replays its
   records.

Per record reconcile in steady state (nothing changed, record Ready):

| | Before | After |
|---|---|---|
| GETs | 3 (zone, record before the status decision, record after) | 0 (zone from store, status from cache, outcome returned) |
| PATCHes | 0 (status unchanged) | 0 |
| Timed reconciles per record at rest | 1 per 300 s, each re-pushing to every primary | 0 |
| `NotSelected` reconciles | 1 per 30 s until the zone tags the record | 1, then 1 when `status.zoneRef` arrives |

## Consequences

- At rest the operator makes no reconciles and no API calls beyond its
  watches. Load scales with change, not with object count.
- Drift inside BIND9 with the pod running is no longer reverted within five
  minutes. This is a deliberate trade, recorded as an accepted risk in the
  threat model: tampering inside BIND9 already requires the RNDC/TSIG key or
  an operator ServiceAccount token accepted by bindcar's TokenReview, and
  the same actor could re-do the change after a timed revert. The force
  annotation, a pod restart and an operator restart all repair it.
- A hand-edited `DNSZone` status (ADR-0009 §4, accepted risk 11) now stands
  until the zone's next event rather than at most five minutes.
- A record or zone that keeps failing against BIND9 is retried at the
  backoff ceiling (60 s, never sooner than 30 s for a rejected record write)
  for as long as it fails. That is a retry of a known failure, bounded per
  object, not a resync of healthy objects.
- Objects no controller ever checked for drift on its timer (the shared
  ServiceAccount, a cluster's PodDisruptionBudgets) are unchanged by this
  ADR: neither the timer nor an event repaired them before, and they are
  rebuilt on the next spec change or operator start.
- The record controller's `DNSZone` mapper now reads the record store. A
  record that stays not-Ready (a permanently rejected write) is woken by
  each status write of its zone; the rejected-write cooldown absorbs that,
  as it did when the timer and the zone both woke it.
- `DnsRecordType::reconcile_record` returns a `RecordOutcome`,
  `reconcile_dnszone` returns a `ZoneOutcome`, `reconcile_bind9instance`
  returns the scheduled rotation wake, and the SDK gains `instrumented_scheduled`,
  `retry_action`, `converged_action` and `scheduled_action`. The SDK
  `requeue` module is removed (contract change inside the workspace).
  `for_each_primary_endpoint` takes a resolver; `find_primary_ips_from_instances`,
  `filter_secondary_instances`, `find_secondary_pod_ips_from_instances` and
  `calculate_expected_instance_counts` take the `Bind9Instance` store;
  `reconcile_zone_records` and `discover_and_update_records` also report
  whether a cleanup must be retried.
- The ConfigMap watch the instance controller already ran (via `.owns`) is
  kept, with a mapper that adds cluster-level ConfigMaps. No new watch, no
  new kind, no RBAC change.
- CALM: the operator node's description records that reconciliation is
  event-driven with no periodic resync; no node, relationship, interface or
  protocol changes.
- Threat model v1.16: M-48 (D2), accepted risk 13 (T1: out-of-band BIND9
  changes not reverted by a timer), accepted risk 11 revised.
