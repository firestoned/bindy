# Namespace-scoped operator (WATCH_NAMESPACES)

Status: **in progress** — config foundation landed; core watch-loop rewrite pending.

## Motivation

The operator currently watches every namespace (`Api::all` throughout
`src/main.rs`) and is bound with a **ClusterRoleBinding**, so its RBAC is
cluster-wide. Two audit findings stem directly from this:

- **C2 (CRITICAL):** the operator holds `deployments create/update/patch`
  cluster-wide; a Deployment template may name any `serviceAccountName`, so a
  compromised operator token can run a Pod as a privileged SA → cluster
  takeover. (A compensating admission policy —
  `deploy/admission-policies/11-bindy-operator-workload-sa-policy.yaml` — now
  blocks the operator SA from creating workloads that run as anything other
  than `bind9`. This roadmap is the durable fix.)
- **H3 (HIGH):** the operator holds `secrets get/list/watch` cluster-wide to
  back the owned-Secret watch (`Api::<Secret>::all`, `src/main.rs`), so it can
  read every Secret in every namespace.

Running the operator scoped to a known set of namespaces lets it use
`Api::namespaced` and be bound with per-namespace **RoleBindings**, removing
both cluster-wide grants by construction (true least privilege).

## Landed in this pass

- `src/namespace_scope.rs` — `NamespaceScope` (`All` | `Namespaces(Vec<String>)`)
  and `NamespaceScope::parse` / `from_env`, driven by `BINDY_WATCH_NAMESPACES`.
  Default (unset/empty) is `All` → fully backward compatible. Unit-tested in
  `src/namespace_scope_tests.rs`.

## Remaining work

### 1. Watch-loop rewrite (`src/main.rs`)

Every reflector and controller is built from `Api::all(client)`. For
`NamespaceScope::Namespaces(ns_list)` these must become **one watch per
namespace**, because kube-rs `Controller` / `watcher` operate on a single
`Api` (there is no native multi-namespace `Api`). Approaches:

- **Multi-controller:** spawn one `Controller` per (resource, namespace) pair,
  all sharing the same reconciler + context. Simplest mental model; N×M tasks.
- **Merged streams:** for the reflector *stores*, merge N per-namespace
  `watcher` streams (`futures::stream::select_all`) into a single store writer
  so the reflector `Store` semantics are preserved. Note: a `reflector::store()`
  has a single writer — a naive per-namespace reflector into the same writer is
  incorrect; use `select_all` over the namespaced watch streams feeding one
  reflector.

Keep `Api::all` when `scope.is_all()` so the cluster-wide default is unchanged.
Add a helper such as `fn watch_targets<K>(client, &scope) -> Vec<Api<K>>`.

The owned-**Secret** watch (`Api::<Secret>::all`) is the H3-critical one and
should be scoped first; it is also lower-risk than the CRD watchers because it
feeds a narrower reconcile path.

### 2. RBAC restructure (`deploy/operator/rbac/`, `src/bootstrap.rs`)

- Replace the cluster-wide `ClusterRoleBinding` (`rolebinding.yaml`) with a
  **RoleBinding per watched namespace** referencing the same rules (as a Role,
  or a ClusterRole referenced by RoleBindings).
- Drop `secrets get/list/watch` and `deployments`/`serviceaccounts`/etc. from
  the cluster scope once every watch is namespaced.
- Keep `src/bootstrap.rs` and the static YAML in sync (see the RBAC sync rule
  in `.claude/CLAUDE.md`); update `deploy/operator/rbac/README.md` and the
  KSV-0056 risk-acceptance comments (also fixes audit finding RBAC F-6, whose
  "namespace-scoped via RoleBinding" comment is currently untrue).

### 3. Leader-election / lease

Lease already uses a fixed namespace (`BINDY_LEASE_NAMESPACE`) — unaffected,
but confirm the lease namespace is within the watched set or grant a dedicated
lease Role there.

### 4. Docs & migration

- Document `BINDY_WATCH_NAMESPACES` in the deployment guide.
- Migration note: the cluster-wide default is unchanged; opting into scoping is
  a deliberate switch that also requires applying the namespaced RBAC and
  removing the ClusterRoleBinding. Multi-namespace requires the
  RoleBinding-per-namespace set.

## Acceptance criteria

- With `BINDY_WATCH_NAMESPACES` unset: behaviour identical to today (all tests
  green, cluster-wide).
- With it set to a list: operator functions with **only** namespaced RBAC in
  those namespaces (no cluster-wide `secrets`/`deployments` grant), verified by
  `deploy/operator/rbac/verify-rbac.sh` and an integration test.
