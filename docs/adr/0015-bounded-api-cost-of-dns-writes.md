# 0015: Bounded Kubernetes API cost of DNS writes

- **Status:** Accepted
- **Date:** 2026-10-05
- **Deciders:** Erick Bourgeois
- **Related:** Extends [ADR-0005](0005-client-side-kube-api-rate-limiting.md) (client-side rate limit) and [ADR-0009](0009-workspace-crate-split-and-shared-watch-layer.md) §3/§4 (shared watch layer, no self-triggering)

## Context

The v0.8.0-rc.2 load test created 300 `ARecord`s at once in one zone served
by 3 primary `Bind9Instance`s. After 600 s only 152 records were served;
after the 300 were deleted, 30 were still served long afterwards. The leader
made 25,582 Kubernetes API requests, running at about 17 req/s against the
20 QPS client limit of ADR-0005, and ran 6,935 `ARecord` reconciles (average
18.5 s) and 11 `DNSZone` reconciles (average 238 s). Records sat
`NotSelected` while the zone reconcile ran.

Tracing the call graph with R records and I primary instances:

- **Every record write read the instance's RNDC key Secret and its
  `Endpoints` with a GET, per instance, per write.** A record reconcile also
  fetched each `Bind9Instance` to learn its role. A record reconcile cost about
  `7 + 3I` requests (GET zone, I instance GETs, I Secret GETs, I Endpoints
  GETs, GET and PATCH of the zone status, GET and PATCH of the record status
  plus an Event, a final GET). A zone replay cost `R x (1 + 2I)`.
- **Record reconciles fanned out.** Each successful record reconcile
  read-modify-wrote the whole `DNSZone.status.records` array to stamp its
  `lastReconciledAt`, with a merge patch and no resourceVersion. Concurrent
  reconciles overwrote each other's stamps, and every write woke, through
  the record controller's `DNSZone` watch mapper, every record still
  unstamped. The record controller also woke on its own condition writes.
- **A zone reconcile cost O(R) even when nothing changed.** It PATCHed
  `status.zoneRef` on every selected record, fetched every record in
  `status.records` to check it still existed, and fetched every record again
  only to log whether all were Ready.
- **Deleted records could stay served.** The record finalizer is best-effort
  by design: under API pressure its primary-instance GETs failed (the
  instance was skipped) and failed DNS deletes were logged and swallowed. The
  zone's self-healing pass that backs it up dropped the record from
  `status.records` even when its own lookups or DNS deletes had failed, so
  nothing ever retried. A zone replay could also re-publish a record whose
  finalizer was running.

The operator has no Secret watch, and must not get one: a cluster-wide Secret
list/watch is the grant M-25 removed. RBAC must not widen.

## Decision

1. **Per-reconcile resolution.** Every BIND9 write goes through an
   `InstanceResolver` (`crates/bindy-bind9/src/instances.rs`) built once per
   reconcile and shared by every write the reconcile makes. It memoizes each
   instance's RNDC key and each `(instance, port)`'s endpoints (successes
   only). `for_each_instance_endpoint*`, `add_record_to_instances_generic`,
   `delete_record_from_primaries` and the zone replay take it as a parameter.
2. **Existing reflector stores.** Endpoints come from the `Endpoints` watch
   the shared `WatchSet` already ran (label-selected to bindy's Services), now
   kept as `Stores::endpoints`; a Service the store does not hold falls back
   to a GET. Instance roles come from the `Bind9Instance` store
   (`filter_primary_instances_cached`), with a GET fallback for an instance
   not cached yet.
3. **A short process-wide RNDC key cache.** `RndcKeyCache` keeps a parsed key
   for `RNDC_KEY_CACHE_TTL` (60 s) per instance. An entry is invalidated when
   the operator rotates the key and when any write with it fails, so a
   rotated key is re-read on the next write. No Secret list or watch, no RBAC
   change: the operator reads the same Secrets with the same `get`, less
   often.
4. **The record controller stops writing its zone's status.** The zone
   controller copies `lastReconciledAt` from each record's
   `status.lastUpdated` during discovery, triggered by the record's status
   write it already watches. The record controller's primary stream passes
   spec, finalizer, label and annotation changes plus a change of
   `status.zoneRef`, and drops its own condition writes.
5. **A zone reconcile costs O(new records), not O(R).** It PATCHes
   `status.zoneRef` only on records not already tagged with it, checks
   existence with one LIST per record kind and namespace, and no longer polls
   record readiness.
6. **Deleted records stay tracked until their data is confirmed gone.** The
   stale-record pass keeps a deleted record in `status.records` when any
   primary endpoint could not be checked or cleaned, and discovery keeps it
   there, so the cleanup is retried every reconcile. A replay skips a record
   that is gone or has a `deletionTimestamp`.

Per write, with R records and I primaries:

| | Before | After |
|---|---|---|
| Record reconcile | `7 + 3I` | `5`, plus at most `I` Secret GETs per 60 s process-wide |
| Zone reconcile (steady state) | `3R + O(I)` plus 9 LISTs | `O(I)` plus at most 18 LISTs |
| Zone replay | `R x (1 + 2I)` | `R + 2I` |

## Consequences

- The record write path's API cost no longer depends on the number of
  instances, and a zone reconcile's no longer depends on the number of
  records, so a burst of records drains instead of queueing behind the
  client limit.
- A key changed outside the operator can be used for up to 60 s after the
  change; the first write it breaks invalidates it. The operator's own
  rotation invalidates at once.
- A resolver memoizes endpoints for one reconcile. A pod replaced during a
  long replay makes that replay's writes to the old IP fail, and the replay is
  retried (the zone stays `Degraded` until it completes), as before.
- A deleted record whose DNS data cannot be confirmed gone stays in its
  zone's `status.records`, and the zone logs a warning each reconcile, until
  the primaries are reachable again. An instance that is deleted leaves the
  zone's primaries and stops blocking the retry.
- `Stores` gained an `endpoints` field (SDK contract). `discover_and_update_records`,
  `reconcile_zone_records` and `cleanup_stale_records` changed signature; the
  record crate's `update_record_reconciled_timestamp` is gone.
- No CALM change: no node, relationship, interface or protocol changes; the
  operator reads the same Secrets and Endpoints it read before.
- Threat model v1.14: M-46 (D2, T1, I1).
