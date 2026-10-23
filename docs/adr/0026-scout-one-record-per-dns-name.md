# 0026: Scout publishes one record per DNS name

- **Status:** Accepted
- **Date:** 2026-10-20
- **Proposed:** 2026-10-19
- **Deciders:** Erick Bourgeois
- **Amended:** 2026-10-23 (Decision 7: a label-selected `--gateway-service` target publishes the union of every matching LoadBalancer Service, issue #569)
- **Related:** Amends [ADR-0022](0022-scout-sources-share-one-pipeline.md) Decision 3 (one record per source host); relies on [ADR-0021](0021-rrset-replaced-in-one-update.md) (an RRset replaced in one DNS UPDATE) and [ADR-0022](0022-scout-sources-share-one-pipeline.md) Decision 5 (a shared name survives a deletion); keeps [ADR-0025](0025-scout-records-bound-to-cluster-and-zones.md) (records bound to their cluster and zones); resolves issue #556; leaves roadmap 27 item 2 (two owners of one name) to its own ADR

## Context

Scout writes one `ARecord` per source host (ADR-0022 Decision 3). The record
is named after the source (`scout-{cluster}-{namespace}-{name}-{hash}-{index}`,
the hash taken over cluster, kind, namespace and name) and labelled with it
(`source-kind`, `source-namespace`, `source-name`). Two sources that resolve
to the same DNS name therefore produce two records with the same `spec.name`.

The records controller treats each record as the owner of its whole RRset:
it compares the live RRset with the record's own addresses and, when they
differ, replaces the RRset in one UPDATE (ADR-0021). Two records for one name
overwrite each other. Since there is no periodic resync (ADR-0016), the
winner is whichever record reconciled last: it changes on any event and on
every operator restart.

Issue #556 is the common case. Kube-API VIPs, one `LoadBalancer` Service per
availability zone or pool, all published at the cluster apex
(`bindy.firestoned.io/record-name: "@"`):

| Service | Address      | record-name |
|---------|--------------|-------------|
| `svc-a` | `10.0.0.246` | `@`         |
| `svc-b` | `10.0.0.241` | `@`         |
| `svc-c` | `10.0.0.240` | `@`         |

The wanted result is one RRset, `<zone> A 10.0.0.246 10.0.0.241 10.0.0.240`.
BIND9 serves one of the three addresses.

What already holds:

- Deleting one of the records no longer removes the name: the finalizer
  re-pushes a live claimant instead (ADR-0022 Decision 5). The re-push
  writes that claimant's single address, so the name survives but the set
  is still wrong.
- One source with several addresses works. Since the issue #556 part 1 fix,
  every IPv4 address of a load balancer status is published, not only the
  first.

Two places could build the full set:

1. **The records controller**, writing the union of every record that
   claims the name. This changes what a record means (from "owns the RRset"
   to "adds to it") for every kind, needs a rule for kinds that cannot be
   merged (CNAME) and for TTLs that differ, and merges records from
   different owners: a second edge cluster registered for the same zone
   could add addresses to another cluster's name without either side
   noticing. ADR-0025 deliberately leaves that case to conflict detection,
   not merging.
2. **Scout**, which already knows every source it publishes for a name,
   writing one record with the complete set. The controller is unchanged,
   the record still owns its RRset, and only sources of one Scout (one
   cluster) are ever merged.

## Decision

Scout builds each DNS name's complete address set and publishes it as one
`ARecord`. Every write replaces the record's whole address list; nothing is
added to or removed from a set Scout did not just compute in full.

### 1. One record per cluster, zone and DNS name

A Scout record identifies a DNS name, not a source. Its name is
`scout-{cluster}-{zone}-{dns-name}-{hash}`, the hash taken over cluster,
zone and DNS name (`@` for the apex), truncated and sanitized as today.
Two sources of one Scout that resolve to the same zone and name write the
same record. Two Scouts (two cluster names) never share a record.

### 2. Contributions are recorded on the record

Each source host that resolves to the name is a *contributor*. The record
carries, in the annotation `bindy.firestoned.io/scout-contributors`, a JSON
map from contributor (`kind/namespace/name#index`) to that contributor's
resolved IPv4 addresses and TTL. `spec.ipv4Addresses` is always derived
from the map: the union of every contributor's addresses, sorted, each
address once. `spec.ttl` is the lowest TTL any contributor sets, or unset
(the zone's TTL) when none does. Contributors that set different TTLs do not
block the record: it is published with the lowest, and Scout logs the
disagreement once per change (the record is not held back until they agree).

The map lives on the record, so Scout stays stateless: a restart rebuilds
nothing from memory, and a contributor's addresses are never re-resolved by
another source's reconcile (re-resolving a route's addresses is an API call
to its Gateway).

Each contributor also sets a label `contributor.scout.bindy.firestoned.io/{hash}`
(the ADR-0022 source identity hash), so a source finds every record it
contributes to with one label selector. The `source-kind`, `source-namespace`
and `source-name` labels go away; `managed-by`, `source-cluster` and `zone`
stay, so admission policy 21 (ADR-0025) applies unchanged.

### 3. Every write is a complete, conditional replace

A source's reconcile, for each of its hosts:

1. reads the record (a 404 means "no contributors yet");
2. sets or replaces its own entry in the contributor map;
3. derives `spec.ipv4Addresses` and `spec.ttl` from the whole map;
4. writes the record with the `resourceVersion` it read as a precondition
   (a create when the record did not exist);
5. on a 409 conflict, goes back to step 1.

Two sources reconciling at once cannot lose each other's entry: the second
write fails its precondition and is rebuilt from the first one's result. The
operator then replaces the RRset with the complete set in one UPDATE
(ADR-0021). The controller does nothing new.

A source checks its zone, the allowlists (ADR-0025 Decision 1) and the zone's
namespace authorization before it writes its entry, exactly as it checks
them before writing its record today. A source that fails a check writes no
entry and leaves the other contributors as they are.

### 4. Leaving a name removes only the source's own entry

When a source is deleted, opted out, stops being publishable, or stops
resolving to a name (a new `record-name`, a host removed from an Ingress),
it removes its entry with the same conditional read-modify-write. The record
is updated with the remaining set. When the map becomes empty the record is
deleted, with the `resourceVersion` as a delete precondition, so a
contributor that joined in between is not lost. The name leaves DNS only
when its last contributor does.

The source finds the records it must leave by its contributor label, so a
host renamed while Scout was down is still cleaned up on the next reconcile.

### 5. Records written before this ADR are handed over

A source that still has a per-source record (selected by its
`source-kind`, `source-namespace` and `source-name` labels) writes its entry
into the per-name record first, waits until that record is served
(`handover_step`, ADR-0022 Decision 4), then deletes its old record. While
both exist they claim the same name; the per-name record holds a superset,
and deleting the old record re-pushes the per-name one (ADR-0022 Decision
5).

### 6. Name collisions between namespaces are merged, and logged

Two sources in different namespaces of one cluster that resolve to the same
name are merged, as two Services in one namespace are. Both namespaces are
authorized for the zone (Decision 3), so either could already publish the
name on its own; merging replaces today's last-writer-wins. Scout logs, at
info level, each source that joins a name another source already
contributes to, and the contributor map is kept on the record for audit.
Merging needs no opt-in annotation: zone authorization already decides who
may publish a name, and an opt-in would keep the flip-flop for any
contributor that missed it.

### 7. A label-selected gateway Service target publishes every match

*Added 2026-10-23 (issue #569).* A route whose Gateway has no
`status.addresses` resolves through the `--gateway-service` target mapped to
its class. A `namespace/name` target is one Service and already publishes
all of its addresses (issue #556). A `namespace/<label-selector>` target
previously stopped at the first matching `LoadBalancer` Service with an
address, so with the common one-Service-per-VIP pattern each route published
one VIP of several.

A label-selected target now resolves to the de-duplicated union of the
external IPv4 addresses of **every** matching `LoadBalancer` Service:
non-`LoadBalancer` Services are ignored, a Service with no address yet
contributes nothing, and Services are taken in name order (each one's
addresses in status order), so the source's contributor entry does not
change with the API server's list order and an unchanged record is still
not rewritten. Name targets, Gateways with `status.addresses`, and the
annotation → gateway → `--default-ips` precedence are unchanged.
`--allowed-ip-ranges` (ADR-0025) still applies to the whole set.

Not decided here: Scout still re-resolves a route only when the route
itself is reconciled (its own change, a watch restart, or an error
requeue). A matching Service being created or deleted, or its
`status.loadBalancer` changing, does not by itself update the record. Mapping
Service and Gateway changes to the routes that resolve through them is left
to its own ADR.

## Consequences

- Issue #556 is fixed: the three Services publish one record with three
  addresses, and deleting one Service updates the record instead of
  removing or flipping the name.
- Issue #569 is fixed (Decision 7): a label-selected gateway Service target
  publishes every matching VIP. A selector that matches several
  `LoadBalancer` Services now publishes more addresses than before, so
  selectors must be as narrow as the set of VIPs meant to serve the
  gateway; a route still picks up a Service change only on its next
  reconcile.
- The records controller, the CRD and the admission policies do not change.
  `ARecord` keeps "one record owns one RRset", so ADR-0021 and ADR-0022
  Decision 5 keep their meaning.
- Scout writes more carefully: a read before every write, a precondition, a
  retry on conflict. A busy shared name (many contributors changing at once)
  costs retries, not lost updates.
- A record is no longer traceable to one source by its name or labels; the
  contributor map and labels replace that. `docs/src/guide/scout.md`
  (record names, labels, troubleshooting) and the migration guide change.
- The annotation is bounded by Kubernetes' 256 KiB annotation limit, a few
  thousand contributors per name, far beyond any real name.
- Upgrading Scout creates a per-name record for each name and deletes the
  per-source ones once the new records are served. Rolling Scout back
  leaves per-name records the old Scout does not manage; the migration guide
  needs a step for that.
- Two owners of one name are still last-writer-wins: two Scouts registered
  for one zone, or a hand-written `ARecord` next to a Scout one. With this
  ADR, a name shared within one cluster is one record, so roadmap 27 item 2
  can treat any second record for a name as a conflict (a condition on the
  records, not a merge). That needs its own ADR, and it must not flag the
  overlap in Decision 5 while a handover is in progress.
