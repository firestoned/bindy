# 0025: Scout records are bound to their cluster and zones

- **Status:** Accepted
- **Date:** 2026-10-19
- **Proposed:** 2026-10-19
- **Deciders:** Erick Bourgeois
- **Related:** Builds on [ADR-0002](0002-scout-deployment-topology.md) (Scout on a workload cluster writing to the bindy cluster), [ADR-0008](0008-scout-remote-endpoint-override.md) (how it authenticates there) and [ADR-0022](0022-scout-sources-share-one-pipeline.md) (record ownership by source); extends audit finding H1 (namespace-scoped zone grants); implements roadmap 31; leaves roadmap 27 item 2 (conflict detection in a shared zone) open

## Context

Scout runs on an edge (workload) cluster, for example `foo`, and writes
`ARecord`s into one namespace of the bindy cluster with a ServiceAccount
token from `bindy bootstrap mc`. Several edge clusters can feed one bindy
cluster. Before this ADR, what a Scout could publish was limited by checks
that all ran inside Scout:

- the source object must opt in;
- the zone named by `bindy.firestoned.io/zone` (or `--default-zone`) must
  be a `DNSZone` in Scout's target namespace that lives in, or grants via
  `bindy.firestoned.io/allow-zone-namespaces`, the source's namespace (H1);
- each host must sit inside that zone.

Nothing tied a Scout to a cluster or to a set of zones:

1. **Namespace grants are not cluster-qualified.** The grant compares the
   source's namespace on the edge cluster with a namespace name on the
   bindy cluster. A `DNSZone` for `bar.example.com` that grants `team-a`
   for cluster `bar` grants every `team-a` namespace on every edge cluster.
2. **The cluster name is self-declared.** `source-cluster` comes from
   Scout's own `BINDY_SCOUT_CLUSTER_NAME`. Nothing on the bindy cluster
   checks it.
3. **The checks run on the client.** The token grants create, update and
   delete on every `ARecord` in the namespace. A compromised Scout, or
   anyone holding its token, can write any record, with any labels, and
   delete any other cluster's records (the stale-cluster cleanup does
   delete records of other cluster names on purpose).
4. **The zone label is advisory.** A record joins a zone when a `DNSZone`'s
   `recordsFrom` selector matches its labels. The documented selector
   matches `managed-by: scout` and `zone: <zone>`, but a selector that
   omits the zone label hands every Scout record to that zone.
5. **Addresses are unrestricted.** The `bindy.firestoned.io/ip` annotation
   lets a tenant point a name at any address, even inside a zone it may
   use.

The result is that a tenant on `foo`, or a compromised `foo` Scout, can
publish into `bar.example.com`. Where `foo` and `bar` publish the same name
in one zone, the records controller replaces the RRset with whichever
record wrote last (ADR-0021), so the name can be taken over, not only added.

## Decision

Bind each Scout's records to its cluster and zones in three places: in
Scout itself (cheap, close to the tenant, the clearest error), in admission
on the bindy cluster (the trust boundary, independent of the edge cluster's
honesty), and in the DNSZone controller (so the zone label cannot be
bypassed by a loose selector).

### 1. Scout refuses zones outside an allowlist

New setting `--allowed-zones` / `BINDY_SCOUT_ALLOWED_ZONES`, a
comma-separated list of zone names. An entry allows that zone and its
subzones (`example.com` allows `example.com` and `api.example.com`).
Entries are lowercased and a trailing dot is dropped; a `*` entry or an
entry that is not a valid DNS name fails Scout at startup.

- A source whose resolved zone is not allowed publishes nothing. Scout logs
  a warning and waits for the source to change. Records it published
  earlier stay: a typo in the allowlist must not take names off the air;
  the operator removes them deliberately (as with an H1 refusal today).
- `--default-zone` outside the allowlist fails Scout at startup.
- Unset means unrestricted, as before, with a startup warning.
  `bindy bootstrap scout` gains `--allowed-zones` and templates it.

The setting is separate from `--cluster-name`. The cluster name is part of
every record's name and labels, and changing it renames records (ADR-0022);
it should not also carry a security meaning.

### 2. Scout refuses addresses outside an allowlist

New setting `--allowed-ip-ranges` / `BINDY_SCOUT_ALLOWED_IP_RANGES`, a
comma-separated list of IPv4 CIDRs (a bare address means `/32`). Every
address a source would publish, whether from the annotation, the
discovered load-balancer or Gateway status, or `--default-ips`, must fall
inside a range; otherwise the source publishes nothing and Scout logs a
warning and retries (a load balancer's address can change without the
source changing). A default IP outside the ranges, or a malformed range,
fails Scout at startup. Unset means unrestricted, with a startup warning.
`bindy bootstrap scout` gains `--allowed-ip-ranges`.

### 3. One registered ServiceAccount per edge cluster

Each edge cluster gets its own ServiceAccount on the bindy cluster (already
the documented practice for `bindy bootstrap mc`), and registers it in a
ConfigMap named `bindy-scout-sources` in the namespace the records are
written to. For ServiceAccount `<sa>`:

| Key | Value |
|---|---|
| `<sa>.clusters` | comma-separated cluster names the account may stamp in `source-cluster` |
| `<sa>.zones` | comma-separated zones the account may publish into, same matching as decision 1 |

`bindy bootstrap mc` gains `--cluster-name` and `--allowed-zones`. With
both, it writes the account's two keys with server-side apply under a field
manager of its own (`bindy-bootstrap-mc.<sa>`), so registering one cluster
never rewrites another's keys; `--revoke` removes them. A list of clusters
rather than one name lets a cluster be renamed: add the new name, restart
Scout with it, let it clean up its old records, then drop the old name.

### 4. Admission binds a registered account to its clusters and zones

A new ValidatingAdmissionPolicy, `bindy-scout-source-binding`
(`deploy/admission-policies/21-*` and `22-*`), matches `ARecord` CREATE,
UPDATE and DELETE requests made by a ServiceAccount of the record's own
namespace. Its parameter is the `bindy-scout-sources` ConfigMap of the
record's namespace (`paramRef` with no namespace,
`parameterNotFoundAction: Allow`). When the requesting account has a
`<sa>.clusters` key, the request is denied unless:

- the new object (CREATE, UPDATE) carries `bindy.firestoned.io/managed-by:
  scout`, a `source-cluster` in `<sa>.clusters`, and a `zone` allowed by
  `<sa>.zones` (a missing `.zones` key allows nothing);
- the old object (UPDATE, DELETE) carries a `source-cluster` in
  `<sa>.clusters` and an allowed `zone`, so an account can neither rewrite
  nor delete another cluster's record.

Requests from unregistered principals, including the operator and people,
are not affected. Addresses are not checked in admission: the policy stays
free of CEL's IP and CIDR library, and the address check of decision 2 runs
where the address is chosen.

### 5. A DNSZone takes a Scout record only for its own zone

The DNSZone controller, when its `recordsFrom` selector matches a record
labelled `bindy.firestoned.io/managed-by: scout`, takes the record only if
the record's `bindy.firestoned.io/zone` label equals the zone's
`spec.zoneName` (ignoring case and a trailing dot). Otherwise the record is
skipped with a warning, as if unselected. Scout already stamps exactly the
zone name it authorized against, so no legitimate record changes zone; the
zone label of decision 4 becomes the record's zone whatever the selector
says.

### 6. Scout skips a refused delete of another cluster's record

The stale-cluster cleanup (on publish handover and on source removal)
deletes records left by other cluster names in the same zone. Under
decision 4 a registered account may only delete records of its own
clusters, so the API server refuses the rest (HTTP 403 or 422). Scout logs
the refusal and moves on instead of failing the reconcile: retrying cannot
succeed, and on source removal a failure would hold the source's finalizer
until the grace period. Deletes of the source's own records still fail the
reconcile on any error.

### Not decided here

Two clusters that may both publish into one zone can still publish the same
name, and the last writer wins. Rejecting or flagging the second claimant
needs the records controller to compare records, which is roadmap 27 item 2.
Decisions 1, 3 and 4 make it a configuration choice: zones are only shared
by clusters an administrator registered for them.

## Consequences

- An edge cluster cannot publish into, rewrite, or delete records of zones
  or clusters it is not registered for, even with a stolen token, once its
  account is registered. An account registered before this change is
  unconstrained until `bindy bootstrap mc` is re-run with `--cluster-name`
  and `--allowed-zones`; the upgrade notes say so.
- The admission check relies on per-cluster ServiceAccounts. Two edge
  clusters sharing one account share one registration.
- The policy is only as strong as its installation: the admission policies
  are opt-in (`make admission-policies-install`, the release's
  `admission-policies.yaml`), and the parameter ConfigMap is writable by
  whoever can write ConfigMaps in the namespace. Scout's own role does not
  grant that.
- A `DNSZone` whose selector matched Scout records labelled for another zone
  stops serving them. Scout never writes such records, so only hand-made or
  forged records are affected.
- After a cluster rename on a registered account, the old cluster's records
  cannot be cleaned up by Scout until the old name is in `<sa>.clusters`;
  the refusal is logged, not retried.
- Kubernetes 1.30 remains the floor for the admission policies.
