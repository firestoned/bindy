# 0002: Scout Deployment Topology in CAPI/k0rdent Fleets

- **Status:** Accepted
- **Date:** 2026-07-19
- **Amended:** 2026-10-23 (addendum "One Scout pod at a time": the Scout
  Deployment uses the `Recreate` strategy, `bindy bootstrap scout` migrates an
  existing Deployment to it, and Scout drains on SIGTERM)

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

### Addendum (2026-10-23): one Scout pod at a time

Scout runs one replica and has no leader election: it is stateless, every
source reconciles when it starts, and its writes are idempotent. Under the
Deployment's default `RollingUpdate` strategy, though, an upgrade runs the old
and the new pod side by side until the old one has terminated, and both
reconcile. Seen on kind (2026-10-20): during an upgrade across ADR-0026, a
Service created in that window was published by both versions, one per-source
record and one per-name record, and the newer Scout never revisits a source
that does not change again.

The Scout Deployment, built by `bindy bootstrap scout` and shipped as
`deploy/scout/deployment.yaml`, uses `strategy: {type: Recreate}`: the old pod
is gone before the new one starts. Leader election would also prevent the
overlap but needs a Lease and RBAC on every edge cluster and code in Scout,
for a component that only ever needs one replica. `replicas` stays 1; more
than one replica would bring the overlap back and is not supported.

Two things make `Recreate` work in practice:

- **Scout drains on SIGTERM.** Scout used to ignore SIGTERM, so the old pod
  sat out the whole termination grace period (30 seconds) before the new one
  could start. Its source controllers now drain on SIGTERM or SIGINT like the
  operator's (the SDK shutdown signal and kube's `graceful_shutdown_on`):
  no new reconcile starts, running ones finish, the process exits. Measured
  on kind: under a second with no Scout running between two such versions,
  against about 30 seconds without it. DNS keeps serving everything already
  published during the gap, and changes made to sources in it are picked up
  when the new pod reconciles every source at startup.
- **`bindy bootstrap scout` migrates an existing Deployment.** The API server
  refuses a server-side apply that sets `type: Recreate` on a Deployment
  whose `spec.strategy.rollingUpdate` it defaulted ("may not be specified
  when strategy type is 'Recreate'"; verified on kind, with or without an
  explicit `rollingUpdate: null`). Bootstrap therefore merge-patches an
  existing Deployment that wants `Recreate` and does not have it
  (`type: Recreate`, `rollingUpdate: null`) before the apply. A client-side
  `kubectl apply` of `deploy/scout/deployment.yaml` handles the switch on its
  own.

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
- (Addendum) A Scout upgrade or restart pauses publishing for about a second;
  nothing already published is affected. The first upgrade to a Scout that
  drains on SIGTERM still waits out the old pod's grace period once. Running
  more than one Scout replica per cluster is unsupported, and a Scout
  Deployment managed outside bootstrap (Helm, Kustomize, GitOps) must set
  `strategy.type: Recreate` itself.
