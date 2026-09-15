# Namespace-scoped operator RBAC

Least-privilege RBAC for running bindy with `BINDY_WATCH_NAMESPACES` set. Closes
audit findings **C2** (cluster-wide `deployments create/update/patch`, a
privilege-escalation path via `serviceAccountName`) and **H3** (cluster-wide
`secrets get/list/watch`) by construction rather than by compensating control.

**This is opt-in.** The default deployment (`BINDY_WATCH_NAMESPACES` unset) is
unchanged and still uses the cluster-wide `../role.yaml` + `../rolebinding.yaml`.

## What is here

| File | Scope | Purpose |
|---|---|---|
| `clusterrole.yaml` | Cluster | `clusterbind9providers` get/list/watch/update/patch **only** |
| `clusterrolebinding.yaml` | Cluster | Binds the above to the `bindy` SA |
| `role.yaml` | Namespaced | Everything else — apply one copy per watched namespace |
| `rolebinding.yaml` | Namespaced | Binds that Role to the `bindy` SA, per namespace |

### Why a ClusterRole survives at all

`ClusterBind9Provider` is the only bindy kind with `scope: Cluster`. A cluster-scoped
object has no namespace to scope a watch to, so a slim ClusterRole is irreducible.
Every other kind — including `Bind9Cluster`, which earlier analysis wrongly listed as
cluster-scoped — is `Namespaced` and moves into the per-namespace Role.

So M-22's original claim that scoping "eliminates cluster-wide access entirely" is not
achievable. What it *does* eliminate is cluster-wide **Secret read** and cluster-wide
**workload write**, which is what C2 and H3 are actually about.

## Installing

```sh
NAMESPACES="tenant-a tenant-b"

# 1. The irreducible cluster-scoped grant (once).
kubectl apply -f clusterrole.yaml -f clusterrolebinding.yaml

# 2. One Role + RoleBinding per watched namespace.
for ns in $NAMESPACES; do
  sed "s/REPLACE_NAMESPACE/$ns/" role.yaml        | kubectl apply -f -
  sed "s/REPLACE_NAMESPACE/$ns/" rolebinding.yaml | kubectl apply -f -
done

# 3. Point the operator at the same list.
kubectl set env deployment/bindy -n bindy-system \
  BINDY_WATCH_NAMESPACES="$(echo $NAMESPACES | tr ' ' ',')"

# 4. Remove the cluster-wide binding this replaces.
kubectl delete clusterrolebinding bindy-rolebinding
```

> **The namespace list in step 2 must match step 3 exactly.** If the operator watches
> a namespace with no Role, it crash-loops on 403s. If a Role exists for a namespace
> the operator does not watch, that is a silent over-grant.

## Leader election

The lease lives in `BINDY_LEASE_NAMESPACE` (default `bindy-system`), which is
independent of the watched set. If your lease namespace is not in
`BINDY_WATCH_NAMESPACES`, apply `role.yaml`/`rolebinding.yaml` there too — or grant a
dedicated lease-only Role.

## Keeping this in sync

These files are **derived from `../role.yaml`** by splitting its rules on whether the
resource is cluster-scoped. When you change `../role.yaml`, re-split it: the split is
`clusterbind9providers` and `clusterbind9providers/status` into `clusterrole.yaml`,
everything else into `role.yaml`.

`src/bootstrap_tests.rs` asserts the split stays faithful — that the namespaced Role
grants no cluster-scoped kind, and that the two halves together cover every rule in
`../role.yaml`.
