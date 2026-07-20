# ADR-0002: Scout Deployment Topology in CAPI/k0rdent Fleets

## Status

Accepted

## Context

Bindy Scout watches Ingress, Service, and Gateway API route objects in a
*source* cluster and manages `ARecord`/`DNSZone` resources on a *Bindy* (DNS)
cluster. In Phase 2 (multi-cluster) mode the write side uses a kubeconfig
stored in a Kubernetes Secret (`BINDY_SCOUT_REMOTE_SECRET`); the watch side
always uses the pod's local client.

In a k0rdent / Cluster API (CAPI) fleet there is one management cluster — the
**queenship** (also called the mothership) — hosting the CAPI machinery and,
with hosted control planes, the child clusters' API servers. Each tenant
namespace on the queenship can own N child **drone** clusters, whose
cluster-admin kubeconfigs CAPI stores as Secrets (`<cluster>-kubeconfig`) in
that tenant namespace.

Two topologies were considered for running Scout across such a fleet:

- **Option A — Scout per drone**: one Scout Deployment inside every drone
  cluster (plus one on the queenship for the queenship's own ingresses). Each
  Scout watches its own cluster with its local ServiceAccount and holds one
  narrowly-scoped kubeconfig credential pointing *at* the Bindy cluster.
- **Option B — Scouts centralized on the queenship**: N Scout Deployments per
  tenant namespace on the queenship, one per drone. Because Scout watches with
  its *local* client, each such Scout must mount the drone's CAPI kubeconfig
  Secret as its primary client — a **cluster-admin** credential for the drone.

The decisive security question is which credential a compromised Scout pod
yields:

| | Option A (per drone) | Option B (queenship) |
|---|---|---|
| Credential held by Scout | Bindy-cluster kubeconfig, scopable to ARecord CRUD + DNSZone read in one tenant namespace | Drone cluster-admin kubeconfig (CAPI-issued, not meaningfully scopable) |
| Blast radius of one pod compromise | One tenant's DNS records | Full admin of one drone cluster |
| Aggravated by cluster-wide `secrets: get` (threat model I4) | That drone's Secrets only (and the I4 fix removes even that) | **Every drone kubeconfig of every tenant on the queenship** |
| Tenant isolation boundary | Cluster boundary (hard) | Namespace boundary on a shared cluster (soft) |
| Scaling shape | 1 pod per drone, distributed | N×tenants pods concentrated on the queenship |
| Network requirement | Drone → Bindy API egress | None new (queenship already reaches drones) |

Option B's single advantage (no new network path) does not offset
concentrating cluster-admin credentials for the entire fleet in one place. It
also inverts the credential direction: Option A holds low-privilege
credentials pointing *at* the DNS cluster; Option B holds high-privilege
credentials pointing *from* a shared cluster into every tenant's workload
cluster.

## Decision

**Option A.** Scout runs inside every drone cluster, plus exactly one Scout on
the queenship for the queenship's own ingresses. No Scout instance ever mounts
a drone's CAPI kubeconfig.

Required hardening that makes Option A sound:

1. **No cluster-wide Secret read** (threat model I4): Scout's default RBAC
   carries no Secret rule; Phase 2 deployments grant a namespaced,
   `resourceNames`-restricted Role for the single remote-kubeconfig Secret.
2. **Per-drone identities on the Bindy cluster**: each drone's Scout
   authenticates with its own ServiceAccount, RBAC-boxed to that tenant's
   namespace (ARecord create/update/delete, DNSZone get/list). Audit logs
   attribute every record write to a specific drone; one drone can be revoked
   without touching the fleet.
3. **Admission policy on the Bindy cluster** enforcing that a tenant's Scout
   identity can only write records in that tenant's zones, containing the
   residual "compromised Scout hijacks DNS" scenario.
4. **Fleet rollout via k0rdent**: Scout ships to drones as a
   ServiceTemplate/MultiClusterService add-on, so every new drone receives it
   automatically with the scoped credentials above.

## Consequences

- A compromised Scout yields at most one tenant's DNS-record surface — never
  cluster admin of any drone and never other tenants' credentials.
- Drone clusters need API egress to the Bindy cluster endpoint (the only new
  network path).
- Credential lifecycle work shifts to the Bindy side: one SA + RBAC + zone
  admission entry per drone, automated by the fleet rollout template.
- The queenship Scout is an ordinary same-cluster or Phase 2 deployment with
  no special privileges; nothing on the queenship holds drone credentials on
  Scout's behalf.
- Documented for users in `docs/src/guide/scout-topology.md`; threat model I4
  remains the tracking item for the RBAC scoping that this decision depends
  on.
