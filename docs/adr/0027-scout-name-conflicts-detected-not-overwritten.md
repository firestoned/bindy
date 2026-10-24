# 0027: Scout records that claim one name from two clusters are a conflict, not a race

- **Status:** Accepted
- **Date:** 2026-10-23
- **Proposed:** 2026-10-21 (revised 2026-10-23 after review)
- **Deciders:** Erick Bourgeois
- **Related:** Closes roadmap 27 item 2; builds on [ADR-0026](0026-scout-one-record-per-dns-name.md) (one record per DNS name per cluster), [ADR-0025](0025-scout-records-bound-to-cluster-and-zones.md) (records bound to their cluster and zones; the `bindy-scout-sources` registry; accepted risk 23 (a)) and [ADR-0022](0022-scout-sources-share-one-pipeline.md) Decisions 4 and 5 (renames, and a shared name surviving a deletion); keeps [ADR-0021](0021-rrset-replaced-in-one-update.md) (an RRset replaced in one UPDATE)

## Context

Since ADR-0026 every source of one Scout that publishes a DNS name contributes
to one `ARecord` for that name. Other Scout records can still claim the same
name, kind and zone. Some of them are legitimate:

- **A handover inside one cluster** (ADR-0026 Decision 5): a per-source record
  from before ADR-0026, or a legacy record from before ADR-0022, next to the
  per-name record replacing it, until the new one is served. Both carry the
  same `source-cluster` label.
- **A cluster rename** (ADR-0022 Decision 4): records written under the old
  `--cluster-name` next to the new ones, until the new ones are served. ADR-0025
  tells operators to register both names on the edge cluster's account for
  the duration (`bindy bootstrap mc --cluster-name new,old`).

The rest are a real conflict: **a second edge cluster** whose account is
registered for the same zone publishes the same name. ADR-0025 allows an
administrator to register one zone for several clusters. Today the records
controller lets each record replace the RRset with its own values (ADR-0021),
so the last write wins and the winner changes on every event and every
operator restart. ADR-0025 records this as accepted risk 23 (a): either
cluster can take over the other's name. The guardrails e2e suite reproduces it
(G7).

Records alone cannot tell a rename from two clusters: Scout's contributor
labels hash a source's kind, namespace and name without the cluster (so that
a rename finds its old records), and the same manifests deployed to two
clusters produce the same labels. The registry does tell them apart: a rename
is two cluster names registered to one account; two clusters are two
accounts.

The rule for who keeps a contested name also has to survive what records go
through: a failed reconcile sets `Ready=False` without changing what BIND9
serves; a record may never manage to publish at all; an operator restart must
reach the same answer. And the records controller is not the only writer: the
zone controller replays every selected record onto a wiped or newly admitted
primary (ADR-0015, ADR-0017), and a record's finalizer re-pushes another
claimant of its name (ADR-0022 Decision 5).

## Decision

The records controller detects Scout records that claim one name from
different edge clusters, keeps the name with the record that has already
published it, holds the others back with a status condition, and every path
that writes a record to BIND9 applies the same rule.

### 1. What a conflict is

Two records conflict when all of the following hold:

1. both carry `bindy.firestoned.io/managed-by: scout`;
2. both are of the same kind and bound to the same zone (`status.zoneRef`);
3. both declare the same DNS name (`spec.name`, compared ignoring case);
4. neither is being deleted;
5. their `source-cluster` labels differ;
6. the two cluster names are not registered to the same account in the
   records namespace's `bindy-scout-sources` ConfigMap (ADR-0025). Two names
   on one account are one edge cluster being renamed.

Condition 5 exempts every handover inside one cluster; condition 6 exempts a
rename done the documented way. A rename done without registering both names
is treated as a conflict: the old record keeps serving, the new one is held
back with a message naming the old cluster, and registering both names (or
deleting the old record) resolves it. Records without the Scout label are never
in conflict: two hand-written records sharing a name keep ADR-0022 Decision 5
behavior, and a hand-written record next to a Scout record is left to a
follow-up.

The registry is read when two records from different clusters claim one name,
which is rare; a missing ConfigMap means no names are registered together.

### 2. The record that has published the name keeps it

Among conflicting records, the **holder** is chosen from those that have
**published** the name: `status.publishedName` equal to `spec.name` (ignoring
case) and `status.lastUpdated` set, both written only after a successful
write to BIND9. `Ready` is not consulted: a failed reconcile does not change
what BIND9 serves, so a transient failure of the holder does not hand the name
over.

- One published record: it is the holder.
- Several (two records that both published before this change, or a
  takeover in flight): the oldest `creationTimestamp`, ties broken by
  namespace and name.
- None: there is no holder yet, and nobody is held back. Each record writes,
  as today, until one has published; from then on the rule above applies.
  A record that never manages to publish can therefore never hold a name.

The rule reads only persisted fields, so every operator restart and replica
reaches the same holder, and a newcomer can never take over a name another
cluster has published. One function computes it, shared by every path below.

### 3. Everything else is held back

A record in conflict that is not the holder:

- writes nothing to BIND9;
- gets `Ready=False` with reason `RRsetConflict` and a message naming the
  holder and its cluster (`ARecord bindy-system/<name> of cluster 'foo' has
  published 'api' in shared.test`);
- re-pushes the holder once, when it first finds the conflict, so a name it
  overwrote before this change goes back to the holder's values (the write
  compares by value first and sends nothing when BIND9 already serves the
  holder, ADR-0021);
- logs one warning per conflict it finds, not one per reconcile.

### 4. Every writer applies the rule

- **Zone replay** (a wiped primary, the zones-loaded gate): records held back
  are skipped, so a replay cannot make the last record in `status.records[]`
  the one served.
- **The finalizer re-push** (ADR-0022 Decision 5): when a record that claims a
  name is deleted or renamed and other records still claim it, the record
  re-pushed is the holder the rule picks among the remaining claimants, not
  the first one the store returns.

### 5. A held-back record takes over without a timer

When the holder leaves (deleted, renamed, unbound from the zone), the next
holder must notice. Each record kind's controller gains a second trigger: a
change to, or deletion of, a Scout record of that kind wakes the other Scout
records of the kind that declare the same name in the same zone (read from the
kind's reflector store, no API call). This trigger must see **status-only
changes and deletions** (`status.zoneRef` cleared, `status.publishedName`
changed), which the controller's primary stream filters out
(`primary_predicate`); it subscribes to the kind's watch without that filter.
The woken record reconciles, finds itself the holder, writes, and goes
`Ready=True`.

### 6. Scout is unchanged

Scout keeps writing its own record. A held-back record is not served
(`Ready=False`), so a Scout handover waiting for its new record to be served
keeps the old one, as it does for any unserved record (ADR-0022 Decision 4).

## Consequences

- Accepted risk 23 (a) narrows from "either cluster can take over the
  other's name" to "the first cluster to publish a name keeps it; the second
  is held back visibly". A zone registered for two clusters still lets each
  claim names the other has not published.
- The conflict is visible where operators look: `kubectl get arecords` shows
  the held-back record not Ready with reason `RRsetConflict` and the holder in
  the message.
- The guardrails e2e suite's G7 becomes a hard assertion: one cluster's
  address served on every primary and secondary, the other record
  `Ready=False, RRsetConflict`, deleting the holder hands the name to the
  other, and a primary wipe (replay) keeps the holder.
- Renames that skip the documented registration step now show a conflict
  instead of completing silently. The migration guide's rename steps already
  register both names; the troubleshooting guide gains the message.
- The trigger in Decision 5 scans the kind's store for records with the same
  zone and name on every Scout record event: linear in the records of that
  kind, from memory.
- A hand-written record next to a Scout record stays last-writer-wins;
  extending conflicts to it would change behavior for names shared on purpose
  today and needs its own decision.
- A Scout handover whose new record is held back keeps its old record and
  rechecks every 10 seconds (`HANDOVER_RECHECK_SECS`) while the conflict
  lasts. Bounded; a follow-up can have Scout stop waiting on a record whose
  reason is `RRsetConflict`.
- No CRD, RBAC or admission change: the condition uses existing status
  fields, the reason is one new constant, and the operator already reads
  ConfigMaps in the records namespace.
