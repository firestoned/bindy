# 0022: Scout's source kinds share one reconcile pipeline

- **Status:** Accepted
- **Date:** 2026-10-13
- **Proposed:** 2026-10-13
- **Deciders:** Erick Bourgeois
- **Related:** Builds on [ADR-0002](0002-scout-deployment-topology.md) (where Scout runs) and [ADR-0008](0008-scout-remote-endpoint-override.md) (how it reaches the bindy cluster); closes threat-model accepted risk 20

## Context

Scout watches five source kinds (`Ingress`, `Service`, `HTTPRoute`,
`TLSRoute`, `TCPRoute`) and publishes an `ARecord` for each opted-in
hostname. Each kind has its own reconcile function (`scout.rs`, about 4,900
lines in all), and all five follow the same pipeline: excluded namespace,
deletion with a grace period, opt-out cleanup, finalizer, zone resolution
and authorization, IP resolution, TTL, per-host apply, pruning of hosts no
longer present, stale-cluster cleanup.

The copies are hand-kept in step and have drifted:

- **B1 (fixed in PR #544):** the TCPRoute path skipped the P2-7 validation
  of the `record-name` override that the other four applied.
- **B11 (fixed in PR #544):** Service alone had no stale-cluster cleanup and
  no release when it stopped being a `LoadBalancer`; no kind pruned a host
  that left the source.
- `reconcile_httproute` and `reconcile_tlsroute` are identical once the kind
  name is normalised.

Two defects remain in what the copies share (threat-model accepted risk 20):

- **ARecords carry no source-kind label.** `Ingress`, `HTTPRoute`,
  `TLSRoute` and `TCPRoute` objects with the same name in one namespace get
  the same record names and the same source labels, so they overwrite each
  other's records, pruning can delete another kind's records, and releasing
  a same-named `Service` deletes them all.
- **Record names are ambiguous across namespaces.** A record is named
  `scout-{cluster}-{namespace}-{name}-{index}`, joined with hyphens, so
  `team-a`/`web` and `team`/`a-web` map to the same `ARecord` and overwrite
  each other.

Renaming a record exposes one more defect, on the operator side and already
on `main`. The record finalizer deletes the record's whole RRset from BIND9
(`delete_record_from_primaries` by zone, name and type) without checking
whether another live record in the zone still declares that name and type.
When two records publish the same name and one is deleted, the survivor's
data disappears, and since ADR-0016 nothing re-pushes it until the survivor
itself changes. The zone's stale-record cleanup already guards against this
(`claimed_by_live_record`, bug-211); the finalizer does not.

The first review also found smaller fail-fast gaps that every copy shares:
IPs read from status fields are not checked to be IPv4 before going into an
A record; a malformed TTL annotation is silently dropped (and a negative one
accepted); malformed startup configuration (`--gateway-service`, empty
environment values) is silently ignored; a failed Gateway or Service lookup
is logged at debug level only; and the namespace-eligibility LIST runs
before the cheap opt-in check, for every object Scout sees.

## Decision

1. **One pipeline.** One `reconcile_source<K: ScoutSource>` implements every
   step above, once. A `ScoutSource` supplies only what differs between
   kinds: its kind name, its hostnames (Ingress rules, route `hostnames`, the
   Service's derived name, TCPRoute's required `record-name` override), and
   where its IPs come from (LoadBalancer status, Gateway status or the
   configured gateway Service). The five reconcile functions, their
   finalizer, selector, naming and cleanup helpers collapse into it.
2. **A source-kind label.** Every `ARecord` Scout writes carries
   `bindy.firestoned.io/source-kind: <Kind>`, and every selector Scout uses
   to find its own records (pruning, opt-out and deletion cleanup,
   stale-cluster cleanup) includes it. A record is owned by exactly one
   (cluster, kind, namespace, name) source.
3. **Unambiguous names.** A record's name keeps a readable prefix and adds a
   short hash of the unambiguous source identity:
   `scout-{cluster}-{namespace}-{name}-{hash}-{index}`, where `{hash}` is the
   first 8 hex characters of the SHA-256 of
   `cluster \0 kind \0 namespace \0 name`. The readable part is sanitized
   and truncated as today, so a name stays within the Kubernetes limit; the
   hash keeps it unique.
4. **Migration in place.** Existing records (legacy names, no source-kind
   label) are replaced by handover: a source applies its new-named records,
   and deletes the records that match its cluster, namespace and name labels
   but carry no source-kind label only once every new record is served (its
   `status.zoneRef` is set and its `Ready` condition is `True`). Until then
   the source rechecks every 10 seconds and keeps the old records, so a name
   never stops resolving. Records a previous `--cluster-name` left in the
   same zone are handed over the same way. When the source itself goes away
   (deletion, opt-out, release) there is nothing to hand over to, and its
   legacy records are deleted with its own. In the rare case where two kinds
   shared a name, whichever reconciles first replaces the shared legacy
   record and the other kind publishes its own on its next reconcile (every
   source reconciles at Scout startup).
5. **A shared name survives a deletion.** The record finalizer does not
   delete an RRset that another live record in the same zone still declares
   (same name and type, the `claimed_by_live_record` rule the zone cleanup
   uses). The RRset may still hold the deleted record's values if it wrote
   last, so the finalizer re-pushes the surviving record instead of
   deleting; that write compares by value first (ADR-0021) and sends nothing
   when BIND9 already serves the survivor, and a failed re-push keeps the
   finalizer so the deletion is retried. A rename (`spec.name` changed) does
   the same for the old name: it re-pushes the claimant instead of deleting,
   and a failed re-push keeps `status.publishedName` so the rename is
   retried. Only a claimant bound to the zone (`status.zoneRef`) counts. This
   backs up the handover in (4), and it fixes the same loss for any two
   records that share a name.
6. **Fail fast on input.** In the shared pipeline:
   - an IP from a status field that is not IPv4 is skipped with a warning;
   - a TTL annotation that does not parse as a positive integer is treated
     like an invalid `record-name` override: a warning, no record published
     for that source, and a requeue;
   - malformed `--gateway-service` entries and empty values in Scout's
     environment configuration fail Scout at startup;
   - a Gateway or Service lookup error other than 404 fails the reconcile,
     which retries;
   - the opt-in check runs before the namespace-eligibility LIST, so objects
     Scout ignores cost no API call.

## Consequences

- One place to change Scout's behaviour; the B1 and B11 class of drift cannot
  recur. `scout.rs` is expected to shrink by more than half.
- Closes accepted risk 20: kinds no longer overwrite or prune each other's
  records, and namespaces no longer collide.
- **Visible change:** every Scout `ARecord` is renamed once, on the first
  reconcile after the upgrade, and gains the `source-kind` label. Tools that
  select Scout records by name pattern must use the labels instead. The
  migration guide records it.
- An invalid TTL annotation now stops publication for that source instead of
  being ignored, and a misconfigured Scout fails at startup instead of
  running degraded. Both surface earlier; neither can corrupt published data.
- The record finalizer and the rename path gain a store lookup (the record
  stores are already cached, ADR-0015); a deletion claimed by another record
  sends nothing to BIND9 when BIND9 already serves the claimant.
- The upgrade depends on the operator: a legacy record is kept until the
  operator serves its replacement, so a Scout upgraded against an operator
  that never marks the new record `Ready` keeps the old record (still served)
  and rechecks every 10 seconds. The operator is upgraded first, since an
  older one has no Decision 5 guard.
- Name collisions are now a 32-bit hash collision within one cluster and
  target namespace; at that point the two sources' records overwrite each
  other as before. Accepted: the readable prefix must also match for the
  names to collide.
- The published DNS names do not change: only the `ARecord` object names do.
- No CRD, RBAC or CALM change: Scout creates, updates and deletes the same
  `ARecord` objects with the same permissions; only their names and labels
  change.
