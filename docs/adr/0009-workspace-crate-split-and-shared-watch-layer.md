# 0009: Workspace crate split and shared watch layer

- **Status:** Accepted
- **Date:** 2026-10-03
- **Proposed:** 2026-10-03
- **Deciders:** Erick Bourgeois
- **Related:** Plan in roadmap 01
  (`.github/community/01-controller-crate-split.md`); keeps
  [ADR-0005](0005-client-side-kube-api-rate-limiting.md)'s client-side rate
  limiting; follows the workspace layout of the sibling `banlieue` repo

## Context

bindy is one crate. Measured on `main` @ `d422055`:

| Metric | Value |
|---|---|
| Non-test Rust source | 44,362 lines |
| Test source (`*_tests.rs`) | 30,065 lines |
| Crates | 1 (`bindy`), plus the `crdgen` and `crddoc` bins |
| `pub mod` in `src/lib.rs` | 24 |
| `src/crd.rs` | 4,332 lines |
| `src/scout.rs` | 4,876 lines |
| `src/main.rs` | 2,182 lines |

Two problems follow from that, one about the build and one about the runtime.

**No enforced boundaries.** Every module can reach every other. One
back-edge already exists: `src/bind9/zone_ops.rs` imports
`crate::reconcilers::retry`, so the BIND9 client layer depends on the
controller layer above it. Any edit to `src/crd.rs` recompiles Scout,
bootstrap, every reconciler and every test module, although `crd.rs` itself
has no intra-crate imports: the layering is already there, it is just not
expressed as crates.

**`main.rs` is a hand-written controller framework, and its watch layer
opens many more connections than it needs.** `initialize_shared_context`
spawns one reflector per kind (14 kinds) to fill the shared `Stores`. Each of
the 13 controllers (`Bind9Cluster`, `ClusterBind9Provider`, `Bind9Instance`,
`DNSZone`, and 9 instances of `run_generic_record_operator`) then opens its
*own* primary watch, and the 23 `.watches()` / `.owns()` call sites in
`main.rs` plus the `DNSZone` watch in every record controller open more. In
cluster-wide mode that is about 59 watch streams for 14 kinds, holding two
independent caches of the same objects that can disagree mid-reconcile. In
namespace-restricted mode (`NamespaceScope::Namespaces`) every namespaced
stream is repeated per namespace target, so the count scales as roughly
59 × N.

Four defects live in that layer, all recorded in roadmap 01 §1.3 and all
still present:

1. The `DNSZone` mapper in `run_bind9instance_operator` `tokio::spawn`s a
   task that `get()`s instances and calls `reconcile_instance_zones()`, then
   returns no `ObjectRef`s. That work runs outside any controller: no
   backoff, no retry, no concurrency limit, no metrics, errors are `warn!`ed
   and dropped.
2. `reconcile_dnszone_wrapper` skips a reconcile when an RFC 3339 string in
   `status.bind9Instances[0]` is under `MIN_RECONCILE_INTERVAL_SECS` (2 s)
   old. It is a guard against the zone controller retriggering itself on its
   own status writes, which `any_semantic()` on its primary stream allows.
3. `run_all_operators` is a 13-arm `tokio::select!`: the first controller to
   return drops the other twelve mid-reconcile. Nothing drains on SIGTERM or
   on loss of the leader lease.
4. `perform_startup_drift_detection` lists and reconciles every provider,
   cluster and instance before any controller starts, which the controllers'
   own `InitApply` events then repeat. It lists with `Api::all`, regardless
   of namespace scope.

`ReconcileError` and `error_policy` are also defined twice, identically, in
`src/main.rs` and `src/record_operator.rs`.

kube-runtime 4.2.0, which the lockfile already pins, has the APIs for one
shared cache per kind: `reflector::store_shared()`,
`Controller::for_shared_stream()`, `.owns_shared_stream()` and
`.watches_shared_stream()`. All four are compiled only with kube's
`unstable-runtime-subscribe` feature, which bindy does not enable today.
`Controller::graceful_shutdown_on()` and `predicates::generation` are stable.

## Decision

### 1. One Cargo workspace, one binary

The root `Cargo.toml` becomes a virtual `[workspace]` with
`[workspace.package]` (version, edition, `rust-version`, license, authors)
and `[workspace.dependencies]`, so each dependency version is pinned once.
Crates live in `crates/bindy-*`, as in `banlieue`.

The build output does not change: one `bindy` binary, the same subcommands
(`run`, `scout`, `bootstrap`, ...), flags and environment variables, the
same container images. Edition stays 2021 and MSRV stays 1.94; moving to
edition 2024 is a separate change. Every crate is `publish = false`.

### 2. The crates and their direction

```
bindy (bin) ──→ bindy-bootstrap ───────────────────────────→ bindy-api
            ──→ bindy-scout ──────→ bindy-controller-sdk ──→ bindy-api
            ──→ bindy-controller-{cluster,instance,zone,records}
                    ├──→ bindy-controller-sdk ──→ bindy-api
                    └──→ bindy-bind9 ───────────→ bindy-api
```

| Crate | Owns |
|---|---|
| `bindy-api` | `crd`, `crd_docs`, `labels`, `constants`, `selector`, `status_reasons`; the `crdgen` and `crddoc` bins behind a `crdgen` feature. A leaf: no intra-workspace dependencies |
| `bindy-controller-sdk` | `context` (`Stores`, `MultiStore`), `namespace_scope`, the watch layer (§3), `finalizers`, `status`, `retry`, `pagination`, `resources`, `rate_limit`, `http_errors`, `metrics`, leader election, the single `ReconcileError` / `error_policy` and the requeue policy |
| `bindy-bind9` | `bind9/**`, `bind9_resources`, `bind9_acl`, `ddns`, `dns_errors`, `safe_volume`. No `kube::runtime` import |
| `bindy-controller-cluster` | `Bind9Cluster` and `ClusterBind9Provider` |
| `bindy-controller-instance` | `Bind9Instance` and `placement` |
| `bindy-controller-zone` | `DNSZone` |
| `bindy-controller-records` | the 9 record kinds: `record_operator`, `record_impls`, `record_wrappers`, `reconcilers/records/**` |
| `bindy-scout` | `scout` |
| `bindy-bootstrap` | `bootstrap` |
| `bindy` | CLI, tracing, client construction, metrics server, leader-election handoff, and running each controller crate's entry point |

Dependencies only point downward. `bindy-bind9` never depends on a controller
crate, and no controller crate depends on another controller crate: what two
controllers share moves down into the SDK. Retry/backoff helpers move into
the SDK, which removes the `bind9 → reconcilers` back-edge.

Each controller crate exposes one public entry point,
`pub async fn controller(ctx) -> anyhow::Result<()>`. Tests move with the
code they cover, keeping the `foo.rs` / `foo_tests.rs` convention.

### 3. One shared watch per kind and namespace target

The SDK owns a `WatchSet`, built once at startup, holding one reflector per
(kind, namespace target): one `store_shared()` writer, its `Store`, and its
subscriber stream. Every controller and every cross-kind watch subscribes to
it through `for_shared_stream` / `owns_shared_stream` /
`watches_shared_stream`; no controller opens its own watch. That gives one
connection and one cache per (kind, target): 14 streams in cluster-wide mode
instead of about 59.

This needs kube's `unstable-runtime-subscribe` feature, and we enable it.
Constraints:

- It is the only `unstable-runtime*` feature enabled.
- Only the SDK's watch module touches those APIs; controllers see
  `WatchSet`'s own types. If the upstream API changes or is withdrawn, the
  fix is confined to one module.
- kube stays pinned to the 4.2 line; a kube minor bump is a reviewed change
  that re-runs the watch-layer tests.

Per-namespace sharding stays: each target keeps its own `Store`, for the
reason `MultiStore` documents (merging namespace watches into one writer
corrupts it). `ClusterBind9Provider` keeps one cluster-wide watch in every
scope mode.

A shared stream that ends is restarted by the `WatchSet` with backoff and
counted in a per-kind metric, rather than ending silently as a spawned
reflector task does today.

### 4. A written self-trigger policy

Which watcher config and predicate each stream gets is decided by this
table, enforced in the SDK, not per call site:

| Stream | Config | Why |
|---|---|---|
| Primary resource, spec-driven | `Config::default()` + `predicates::generation` | Only spec changes reconcile; the controller's own status writes do not retrigger it |
| Owned children (`.owns`) | `Config::default()` | Any change to a child is drift |
| Cross-kind (`.watches`) | `Config::default()` + an explicit predicate | The mapper names the field it keys on |
| Status-driven (zone and instance) | `any_semantic()` | Deliberate, and the only place it is allowed |

With `predicates::generation` on the `DNSZone` primary stream, the 2-second
guard in `reconcile_dnszone_wrapper` (defect 2) protects nothing and is
deleted, after the predicate has run on a kind cluster for a full requeue
period without a reconcile storm.

### 5. Controllers own their work and drain on shutdown

- **Watch mappers are pure.** A mapper returns `ObjectRef`s and does no I/O.
  The `DNSZone` mapper's spawned task (defect 1) becomes a reconcile of the
  instance, so the work gets the controller's retries, backoff, concurrency
  limit and metrics.
- **Shutdown drains.** Each controller gets `graceful_shutdown_on()` with one
  shared trigger, fired by SIGTERM or by loss of the leader lease, and the
  binary joins all controllers instead of racing them (defect 3).
- **No separate startup drift pass.** `perform_startup_drift_detection`
  (defect 4) is deleted, but only once an integration test shows every
  pre-existing `Bind9Instance` reconciled within a bounded time of
  controller start from the watcher's own `InitApply` events.
- **One error policy.** A single `ReconcileError` and `error_policy` in the
  SDK, with capped exponential backoff instead of the flat
  `ERROR_REQUEUE_DURATION_SECS`.

### 6. What does not change

- **CRD schemas.** `regen-crds` must produce byte-identical
  `deploy/operator/crds/*.crd.yaml` throughout. A phase that seems to need a
  schema change stops and gets its own ADR.
- **RBAC.** The operator watches and writes the same resources with the same
  verbs. The `bootstrap.rs` / `deploy/scout/*.yaml` / `deploy/scout.yaml`
  sync contract stays, and gains a test that fails on drift.
- **ADR-0005.** One shared rate-limited client, paginated LISTs, retry with
  backoff.
- **Behaviour,** beyond the four defects above. Anything else found
  mid-refactor is logged and fixed separately.

## Consequences

**Good**

- Crate boundaries enforce the layering; a back-edge becomes a compile error.
- An edit to `bindy-api` no longer rebuilds Scout and bootstrap through the
  controllers, and `crdgen` / `crddoc` build without the controllers.
- About 4× fewer watch connections against the API server in cluster-wide
  mode, and one cache per kind, so controllers cannot act on two views of
  the same object.
- Reconcile work that runs outside the controller (the spawned mapper task)
  and the timestamp rate limiter disappear; SIGTERM and lease loss drain
  in-flight reconciles.
- Adding a record kind or a controller is a change in one crate plus one
  line in the binary.

**Bad**

- We depend on an `unstable-` kube feature. Mitigated by confining it to one
  module and pinning kube; recorded as an accepted risk in the threat model.
- A shared stream is a shared point of failure: if one dies, every
  subscriber's view of that kind goes stale. Mitigated by the restart with
  backoff and a per-kind metric; today a dead reflector already goes stale
  silently.
- Clean builds get somewhat slower (more crates, more linking); incremental
  builds get faster.
- Ten crates mean more `Cargo.toml` files. `[workspace.dependencies]` keeps
  versions in one place.
- Makefile targets, CI workflows, Dockerfiles, `cargo-deny` / `cargo-machete`
  configuration and the `regen-crds` / `regen-api-docs` / `cargo-quality`
  skills all reference the single-crate layout and must follow it.

**Rules out**

- Splitting `bindy-api` per API group now. It is a leaf and changes least;
  revisit after the split lands.
- A second binary. Scout, bootstrap and the operator stay subcommands of one
  `bindy` binary.

**Follow-ups**

- Implementation is phased in roadmap 01, one crate per PR, each leaving the
  binary working.
- CALM: the operator's runtime topology is unchanged (one Deployment, one
  binary), but `bindy-operator`'s description and the `operator-watches-api`
  relationship change to describe one shared watch per kind and namespace
  target.
- Threat model: the shared-stream accepted risk, the unstable feature
  dependency, and the drain-on-lease-loss change to leader election.
- `docs/src/architecture/` reconciliation-flow pages describe the
  single-crate layout and are updated as each phase lands.
