# 01: Controller crate split & watch-layer simplification

> **Goal.** Break the single `bindy` crate into a workspace of focused
> crates (one per controller, plus a shared controller SDK) and replace
> the hand-written watch/reflector wiring in `src/main.rs` with a small
> declarative layer built on the kube-runtime APIs that already exist for
> it.
>
> **Stop condition.** `cargo build --workspace` produces the same
> `bindy` binary with the same CLI surface and the same runtime
> behaviour; `src/main.rs` is under ~300 lines and contains no
> `Controller::new` calls, no `tokio::spawn` reflector tasks, and no
> per-kind watch closures; the number of open watch connections against
> the API server drops from about 59 to 14 in cluster-wide mode (one per
> kind).

**Status:** ✅ Done 2026-10-05. Phase A 2026-10-03; Phase B (B1 to B4) 2026-10-04/05; Phases C to G in one PR on 2026-10-05: eleven crates, `main.rs` 257 lines, 19 operator watches instead of 58, controllers drain on shutdown, no startup drift pass. [ADR-0009](../../docs/adr/0009-workspace-crate-split-and-shared-watch-layer.md) Accepted, CALM updated
**Owner:** Erick Bourgeois
**Analysed against:** `main` @ `d422055` (re-measured 2026-10-03; first analysis was `fix-idempotency` @ `648ff7a`), kube / kube-runtime **4.2.0**

> **Decision record.** The crate boundaries, the shared watch layer, the
> self-trigger policy and the use of kube's `unstable-runtime-stream-control`
> feature (amended 2026-10-04 from `unstable-runtime-subscribe`) are decided
> in ADR-0009. This document is the phase plan; where
> the two disagree, the ADR wins.
>
> **Corrections from the 2026-10-03 re-measure:**
> - The shared-stream APIs (`store_shared`, `for_shared_stream`,
>   `owns_shared_stream`, `watches_shared_stream`) are compiled only with
>   kube's `unstable-runtime-subscribe` feature. §5's "no new dependencies"
>   holds for crates, not features; ADR-0009 §3 accepts the feature.
>   *Superseded 2026-10-04 (B2): those shared stores never deliver Delete
>   events, so the `WatchSet` does its own fan-out and controllers consume it
>   through `for_stream` / `watches_stream` / `owns_stream`
>   (`unstable-runtime-stream-control`). See ADR-0009 §3 as amended.*
> - Namespace scoping (`NamespaceScope`, `MultiStore`) landed after the
>   first analysis. Every namespaced reflector and controller stream is
>   now repeated per namespace target, so the `WatchSet` keys on
>   (kind, target), not kind alone.
> - The watch-connection count was undercounted. Today: 14 reflectors +
>   13 controller primaries (4 hand-written + 9 generic record controllers)
>   + 23 `.watches()`/`.owns()` in `main.rs` + 9 `DNSZone` watches in the
>   record controllers ≈ **59** in cluster-wide mode, about 59 × N with N
>   namespace targets. Target: 14.
> - `perform_startup_drift_detection` lists with `Api::all` even in
>   namespace-restricted mode. Another reason for Phase F to delete it.
> - Line references below (`main.rs:NNN`) date from the first analysis and
>   have drifted; the function names are still current.

---

## 1. Why

### 1.1 One crate, everything in it

| Metric | Today |
|---|---|
| Non-test Rust source | **44,362 lines** (was 40,643) |
| Test source (`*_tests.rs`) | 30,065 lines (was 26,220) |
| Crates | **1** (`bindy`), plus two extra `[[bin]]` targets |
| Public modules in `lib.rs` | 24, all `pub` (was 21) |
| CRD types in one file | 13 kinds in `src/crd.rs` (**4,332 lines**, was 4,203) |
| `src/main.rs` / `src/scout.rs` / `src/bootstrap.rs` | 2,182 / 4,876 / 2,149 lines |

Everything is one compilation unit, so:

- **Any** edit to `src/crd.rs` recompiles `scout.rs` (3,926 lines),
  `bootstrap.rs` (2,136 lines), every reconciler and every test module,
  even though Scout only touches `ARecord`/`DNSZone` and bootstrap only
  emits YAML.
- There is no enforced boundary. Nothing in the type system stops a
  reconciler from reaching into `scout`, or `bind9` from reaching back
  into `reconcilers`, and one such back-edge already exists
  (`src/bind9/…` → `crate::reconcilers::retry`).
- `src/main.rs` (**2,086 lines**) is simultaneously the CLI, the
  controller framework, the reflector supervisor, the leader-election
  driver, the metrics server and the drift-detection pass.

The good news, which is why this split is cheap: **the dependency graph
is already almost a DAG.** `src/crd.rs` has *zero* intra-crate `use`
statements; it is a pure leaf. `src/reconcilers/**` imports
`crate::crd` 66 times, `crate::constants` 10, `crate::labels` 7,
`crate::bind9*` 9, and essentially nothing else outside itself. The
layering exists; it is just not expressed as crates.

### 1.2 `main.rs` is a hand-rolled controller framework

| Thing in `main.rs` | Lines | Shape |
|---|---|---|
| `initialize_shared_context` | 447–695 (**248**) | 14 hand-written `tokio::spawn` reflector tasks, 13 of them identical modulo the type |
| `run_dnszone_operator` | 1627–1908 (**281**) | 11 `.watches()`, **9 of them byte-identical** apart from the store field |
| `run_bind9instance_operator` | 1451–1583 (**132**) | 5 `.owns()` + 3 `.watches()`, one of which performs writes from inside the mapper |
| `run_all_operators` | 1184–1258 (**74**) | a 13-arm `tokio::select!` with a copy-pasted `error!`/`bail!` body per arm |
| `perform_startup_drift_detection` | 893–998 (**105**) | manual `list()` + reconcile over 3 kinds |
| `reconcile_dnszone_wrapper` | 1908–2071 (**163**) | finalizer + a hand-rolled 2-second rate limiter |

Across `main.rs` there are **22** `.watches()` / `.owns()` call sites and
5 hand-written `run_*_operator` functions. `Stores` in `src/context.rs`
is a 14-field struct that has to be extended by hand, in three
places (`initialize_shared_context`, the struct, `records_matching_selector`),
every time a record kind is added. `src/scout.rs` then builds **5 more**
controllers with its own copy of the same pattern.

### 1.3 The watch layer specifically

Seven concrete problems, in rough order of severity:

1. **Every CRD is watched twice.** `initialize_shared_context` spawns a
   `reflector` per kind for the shared `Stores`; then each `Controller`
   opens *its own* independent watch for the same kinds via
   `Controller::new` / `.watches()` / `.owns()`. That is 14 reflector
   streams + about 45 controller streams ≈ **59 watch connections** in
   cluster-wide mode (re-counted 2026-10-03; first counted as 36) where 14
   would do, with two independent caches of the same objects that can
   disagree mid-flight. kube-runtime 4.2 has the building blocks behind its
   unstable features. *B2 used `unstable-runtime-stream-control`
   (`Controller::for_stream()`, `.owns_stream()`, `.watches_stream()`) over
   the SDK's own fan-out, because the `unstable-runtime-subscribe` shared
   stores (`store_shared()`, `for_shared_stream()`) drop Delete events.*

2. **A watch mapper performs writes.** In `run_bind9instance_operator`,
   the `DNSZone` mapper (`main.rs:1490`) `tokio::spawn`s a task that
   `get()`s each instance and calls `reconcile_instance_zones()`, then
   returns `vec![]` so the controller does nothing. The comment says
   this is deliberate ("avoid triggering full reconciliation"), but the
   consequence is that this work runs **outside** the controller: no
   backoff, no retry on failure, no rate limiting, no concurrency limit,
   no dedup against a concurrent reconcile of the same instance, not
   counted in `metrics::record_reconciliation_*`, and errors are `warn!`
   and dropped. It fires on *every* `DNSZone` event, including the ones
   this very operator's own status writes produce. This is the single
   most fragile piece of the watch layer.

3. **A rate limiter bolted on top of the scheduler.**
   `reconcile_dnszone_wrapper` (`main.rs:1926`) skips reconciliation when
   `status.bind9Instances[0].lastReconciledAt`, parsed from an RFC3339
   *string*, is less than 2 seconds old. This is a symptom, not a fix:
   the zone controller uses `any_semantic()` watcher config, so its own
   status writes retrigger it. kube-runtime's scheduler already
   deduplicates and debounces; the correct fix is to stop the
   self-trigger (`predicates::generation`, or a `managedFields`
   field-manager filter) rather than to gate on a parsed timestamp from
   an arbitrary array element.

4. **Triplicated stream filters.** The `Deployment` reflector filter
   (`main.rs:510`) repeats the same `owner_references.kind ==
   "Bind9Instance"` check once for `Apply`, once for `Delete` and once
   for `InitApply`: ~45 lines where a single predicate over
   `event.into_iter()` would be three.

5. **No stated policy for `semantic_watcher_config()` vs
   `default_watcher_config()`.** Both exist (`main.rs:801`/`819`); which
   one a given `.watches()` gets is decided per call site with no rule
   written down, and it is exactly this choice that drives problem 3.

6. **No shared shutdown.** `run_all_operators`'s `tokio::select!` means
   the first controller to return **drops the other twelve mid-reconcile**
   (futures, not tasks), so there is no drain. kube-runtime has
   `Controller::graceful_shutdown_on()` / `.shutdown_on_signal()`;
   neither is used. SIGTERM handling lives one level up in
   `run_operators_with_leader_election` and likewise cancels rather than
   drains.

7. **Startup drift detection duplicates the controller's own startup.**
   `perform_startup_drift_detection` serially lists and reconciles every
   `ClusterBind9Provider`, `Bind9Cluster` and `Bind9Instance` **before**
   any controller starts. But a kube-runtime watcher emits
   `Init`/`InitApply` for every existing object on start, and the
   Controller enqueues each one, so the same full pass happens anyway,
   moments later, with backoff and concurrency control. The manual pass
   only delays becoming ready after acquiring the lease.

Plus two duplicates worth deleting on the way past: `ReconcileError` and
`error_policy` are defined *identically* in both `src/main.rs:47/2071`
and `src/record_operator.rs:31/38`.

---

## 2. Target shape

```
crates/
├── bindy-api/              # leaf, no intra-workspace deps
│   └── crd, labels, constants, selector, status_reasons
├── bindy-controller-sdk/   # the framework main.rs currently hand-rolls
│   └── context/stores, watch, finalizers, status, retry,
│       pagination, error_policy, requeue policy, metrics, leader
├── bindy-bind9/            # BIND9 domain logic, no controllers
│   └── bind9/**, bind9_resources, bind9_acl, ddns, dns_errors,
│       rndc, safe_volume
├── bindy-controller-cluster/    # Bind9Cluster + ClusterBind9Provider
├── bindy-controller-instance/   # Bind9Instance + placement
├── bindy-controller-zone/       # DNSZone
├── bindy-controller-records/    # all 9 record kinds, generic
├── bindy-scout/                 # Ingress/Service/*Route → ARecord
├── bindy-bootstrap/             # `bindy bootstrap …`
└── bindy/                       # thin binary: CLI, wiring, main
```

Dependency direction is strictly downward:

```
bindy (bin)
  ├─→ bindy-bootstrap ──→ bindy-api
  ├─→ bindy-scout ──────→ bindy-controller-sdk ──→ bindy-api
  └─→ bindy-controller-{cluster,instance,zone,records}
              ├─→ bindy-controller-sdk ──→ bindy-api
              └─→ bindy-bind9 ──────────→ bindy-api
```

`crdgen` and `crddoc` move to `bindy-api` as feature-gated bins, so
regenerating CRDs stops rebuilding the controllers.

### Each controller crate has the same shape

```
src/
├── lib.rs          // pub fn controller(ctx) -> impl Future<Output = Result<()>>
├── reconcile.rs    // the reconcile fn, pure domain logic
├── watch.rs        // this controller's watch wiring, declaratively
└── status.rs       // this kind's condition helpers
```

`main.rs` then becomes: parse CLI → build client → build the shared
store set → `try_join_all` over `controller()` from each crate → done.

---

## 3. Watch-layer redesign

The SDK owns one `WatchSet` built once at startup, and every controller
subscribes to it instead of opening its own streams.

```rust
// bindy-controller-sdk/src/watch.rs

/// One shared reflector per kind: one connection, one cache, N subscribers.
pub struct WatchSet { /* Store<K> + shared stream per kind */ }

impl WatchSet {
    pub fn store<K: Watched>(&self) -> Store<K>;
    pub fn stream<K: Watched>(&self) -> impl Stream<Item = Arc<K>>;
}

/// Replaces the 5 hand-written `run_*_operator` fns.
pub fn controller_for<K: Watched>(ws: &WatchSet) -> Controller<K> {
    Controller::for_shared_stream(ws.stream::<K>(), ws.store::<K>())
}
```

and the 9 identical record mappers in `run_dnszone_operator` collapse to
one generic helper plus nine one-line call sites:

```rust
// today: 9 × ~14 lines of identical closure
.watches(arecord_api,    default_watcher_config(), move |r| { /* 14 lines */ })
.watches(aaaarecord_api, default_watcher_config(), move |r| { /* same 14 lines */ })
// … ×7 more

// after:
.watches_records::<ARecord>(&ws)
.watches_records::<AAAARecord>(&ws)
// … ×7 more, one line each
```

Same treatment for `Stores`: replace the 14 hand-listed fields and the
`collect_matching!` macro with a `RecordKind` trait implemented once per
record type, so adding a 10th record kind is one `impl`, not edits in
five files.

### Self-trigger policy (fixes problem 3)

Write it down once, in the SDK, and apply it uniformly:

| Stream | Config | Rationale |
|---|---|---|
| Primary CR (spec-driven) | `Config::default()` + `predicates::generation` | Only spec changes reconcile; our own status writes do not |
| Owned children | `Config::default()` | Any change to a child is real drift |
| Cross-kind (`.watches`) | `Config::default()` + explicit predicate | Mapper decides; document what field it keys on |
| Status-driven (zone ⇄ instance) | `any_semantic()` | Deliberate, and the *only* place it is allowed |

With `predicates::generation` on the `DNSZone` primary stream, the
2-second rate limiter in `reconcile_dnszone_wrapper` has nothing left to
protect against and is deleted outright.

---

## 4. Phases

Each phase is independently mergeable and leaves the binary working.
Per `.claude/rules/testing.md`, every phase is TDD: tests move with the
code they cover, and `cargo-quality` gates each one.

### Phase A: Workspace scaffold, no logic moves

- [x] Convert the root `Cargo.toml` to a `[workspace]` with
      `[workspace.package]` and `[workspace.dependencies]`, hoisting every
      shared dependency (`kube`, `k8s-openapi`, `serde`, `tokio`, …) so
      versions are pinned once; keep `bindy` as `crates/bindy`.
      *Landed 2026-10-03: virtual workspace, profiles and
      `[workspace.lints]` (`unsafe_code = "forbid"`) at the root; the Rust
      integration tests moved to `crates/bindy/tests/`, the shell suites
      stay in `tests/`. The release workflow's version rewrite still hits
      the single `version = ` line in the root `Cargo.toml`.*
- [x] Extract `crates/bindy-api`: `crd.rs`, `crd_docs.rs`, `labels.rs`,
      `constants.rs`, `selector.rs`, `status_reasons.rs` + their
      `_tests.rs` siblings. Zero behaviour change; `pub use` from
      `bindy` so nothing else has to move yet.
      *Landed 2026-10-03. `bindy/src/lib.rs` re-exports all six modules.
      Doctests now say `bindy_api::`. `bindy` dropped its direct
      `schemars` dependency (only the CRD types used it).*
- [x] Move `crdgen` / `crddoc` bins into `bindy-api` behind a `crdgen`
      feature. Update `make` targets, `regen-crds` and `regen-api-docs`
      skills, and `.github/workflows/*` accordingly.
      *Landed 2026-10-03: `cargo run -p bindy-api --features crdgen --bin
      crdgen` (and `crddoc`) in the Makefile and every skill; workflow
      path filters moved from `src/**` to `crates/**`; `.cargo/deny.toml`
      sets `allow-wildcard-paths` for the path-only workspace deps.*
- [x] **DoD:** `cargo build --workspace` green; `regen-crds` produces a
      byte-identical `deploy/operator/crds/*.crd.yaml`; `verify-crd-sync`
      passes.
      *Met 2026-10-03: CRD YAMLs byte-identical; 1597 passed / 95 ignored
      tests before and after; clippy `-D warnings`, rustdoc,
      `cargo-machete` and `cargo-deny` clean. `crddoc` regenerated
      `docs/src/reference/api.md` with one change, which was already stale
      on `main` (the DNSSEC fix's `dnssecPolicy` description had not been
      regenerated into it).*

### Phase B: `bindy-controller-sdk`

Landed in steps, each its own PR. **B1** (2026-10-04) is the mechanical part:
the crate exists, the framework modules live in it, behaviour is unchanged.

- [x] Create the crate with `status`, `retry`, `pagination`, `resources`,
      `metrics` (plus `rate_limit`, `namespace_scope`, `http_errors`, per
      ADR-0009 §2). *B1. `bindy` re-exports each under its old path
      (`crate::metrics`, `crate::reconcilers::retry`, ...), so no call site
      changed.*
- [x] Move `context` (`Stores`, `MultiStore`, `Context`). *B3 (2026-10-04):
      `Context`, `Stores`, `RecordRef` and `Metrics` live in
      `bindy_controller_sdk::context` (`MultiStore` moved with the
      `WatchSet` in B2). The BIND9 half (`resolve_bindcar_tls`,
      `create_bind9_manager_for_instance*`) stayed in `bindy` as the
      `StoresBind9Ext` trait, which moves to `bindy-bind9` in Phase C.*
- [x] Move `finalizers`. *Done with Phase D (2026-10-05): the
      `FinalizerCleanup` trait is gone; `handle_deletion` /
      `handle_cluster_deletion` take the cleanup as an async closure, so no SDK
      trait is implemented for a `bindy-api` type and the orphan rule no longer
      applies. The four placeholder tests that needed a cluster became six real
      ones against a mock API server.*
- [x] Move `crate::reconcilers::retry` here and cut the
      `bind9 → reconcilers` back-edge. *B1: `bind9/zone_ops.rs` imports
      `bindy_controller_sdk::retry`; `rg crate::reconcilers
      crates/bindy/src/bind9` is empty.*
- [x] **Single** `ReconcileError` + **single** `error_policy`: delete
      the copies in `main.rs` and `record_operator.rs`. Give
      `error_policy` exponential backoff (capped) instead of the current
      flat `ERROR_REQUEUE_DURATION_SECS`. *B1: `sdk::error`, with tests.
      The capped backoff (`retry::reconcile_error_backoff`, 2s doubling to
      60s, reset after 5 min quiet) had already landed in both copies; B1
      only deduplicated them.*
- [x] Move the requeue policy (`REQUEUE_WHEN_READY_SECS` /
      `REQUEUE_WHEN_NOT_READY_SECS`) out of `record_wrappers` into one
      documented `sdk::requeue` module. *B1; `record_wrappers` re-exports
      them.*
- [x] Implement `WatchSet` on `reflector::store_shared()` +
      `Controller::for_shared_stream()`. *B2 (2026-10-04), on a different
      mechanism than planned: kube's `store_shared()` subscribers never
      receive Delete events, which `.owns()` (a deleted child) and the zone
      controller (a deleted record) rely on. `sdk::watch::WatchSet` runs one
      watcher per (kind, namespace target), applies events to the store, and
      broadcasts `InitApply` / `Apply` / `Delete` over `async-broadcast`
      (backpressure, no loss); late subscribers get the store replayed first.
      Controllers consume it with `for_stream` / `watches_stream` /
      `owns_stream` (`unstable-runtime-stream-control`, ADR-0009 §3
      amended). 15 kinds registered (incl. `Endpoints` of bindy's Services,
      label-selected server-side); owned Secret/ConfigMap/ServiceAccount/
      Service keep ordinary watches (single watcher, never cached).
      Cluster-wide: 19 watch connections, down from about 57. The zone
      controller subscribes to `Bind9Instance` and `Endpoints` across all
      namespaces (`subscribe_all`) for cross-namespace targeting.
      Every cached kind now uses `Config::default()`; the `any_semantic()`
      primaries changed only initial-LIST freshness. Restarts with backoff
      and `watch_{events,errors,restarts}_total` /
      `watch_last_event_timestamp_seconds` metrics. Still to verify on a
      kind cluster (see Phase G).*
- [x] Implement the `RecordKind` trait and rebuild `Stores` on top of
      it; delete the `collect_matching!` macro. *B3: `RecordKind` (kind name,
      `RecordRef` variant) is implemented once per record kind; one
      `RECORD_KINDS` list drives WatchSet registration, the typed
      `RecordStores` map and `records_matching_selector` (same kind order as
      before). `Stores`' nine record fields are gone. `DnsRecordType` (the
      record controllers' trait) takes `RecordKind` as its supertrait, so
      each kind's name is declared once. A new record kind is one
      `record_kind!` line, one `RECORD_KINDS` entry and its `RecordRef`
      variant.*
- [x] Move leader election (`LeaseManagerBuilder` wiring,
      `load_leader_election_config`, `monitor_leadership`) into
      `sdk::leader`. *B4: `LeaderElectionConfig::from_env` (built on a pure
      `from_lookup`, so the env parsing is tested without touching process
      env), `acquire_leadership` (builds the lease, waits until this replica
      leads, returns a `Leadership`) and `leadership_lost`. Env vars, defaults
      and behaviour unchanged. The signal handling duplicated across both run
      functions stays in `main.rs` for Phase F's graceful shutdown.
      `kube-lease-manager` is now an SDK dependency only.*
- [x] **DoD:** `cargo test -p bindy-controller-sdk` green; unit tests
      cover `WatchSet` subscriber fan-out and each predicate. *B1: 174 SDK
      tests green (1609 workspace-wide, up from 1605 by the 4 new
      `error` tests). B2: 11 `watch` tests (store follows init/apply/delete,
      every subscriber gets deletes, late-subscriber replay, predicate,
      errors, restart, routing); 1620 workspace-wide. "Each predicate" waits
      for the self-trigger policy step. B3: 17 `context` tests. B4: 7 `leader`
      tests; 197 SDK tests, 1651 workspace-wide. The predicate tests land with
      the generation predicate in Phase D.*
- **Fixed in B2:** the zone controller watched `Endpoints` with `Api::all`
  in every mode. In namespace-restricted mode that needs cluster-wide
  `endpoints` access the namespaced RBAC does not grant, so the watch was
  refused and a replaced BIND9 pod waited for the zone's requeue. Now a
  per-namespace, label-selected `WatchSet` kind; cross-namespace zones are
  covered by `subscribe_all`. No RBAC change needed.
- **Observed in B2, not fixed (logged per §5):** in namespace-restricted
  mode a zone in namespace A that selects an instance in namespace B does not
  appear in B's `status.zones`. B2 put this down to the instance controller's
  same-namespace `DNSZone` subscription; Phase D found the cause is
  `reconcile_instance_zones` itself, which counts only zones in the
  instance's own namespace. Subscribing across namespaces would only add
  reconciles that change nothing, so the subscription stays per namespace.
  Fixing it changes what `status.zones` reports, which is a separate change.

### Phase C: `bindy-bind9`

*Landed 2026-10-05, with D to G, in one PR.*

- [x] Move `bind9/**`, `bind9_resources.rs`, `bind9_acl.rs`, `ddns.rs`,
      `dns_errors.rs`, `safe_volume.rs`. *Also `placement` (it and
      `bind9_resources` call each other), `StoresBind9Ext` (as
      `bindy_bind9::context`), and, so that no controller crate depends on
      another (ADR-0009 §2, amended 2026-10-05), two pieces both the zone and
      the record controllers use: `instances` (which instances a zone targets
      and how to reach them, from the zone controller's `helpers`/`validation`;
      `primary.rs` whole) and `record_push` (the BIND9 record write, delete and
      replay path, from the record controllers). Tests moved with each.*
- [x] Assert the boundary: this crate depends on `bindy-api` and
      `bindy-controller-sdk` only, never on a controller crate. *Its
      `Cargo.toml` lists no controller crate; a back-edge is a compile error.*
- [x] **DoD:** `cargo test -p bindy-bind9` green; no `kube::runtime`
      import anywhere in the crate. *Green. No production import; the 25 uses
      left are `reflector::store()` fixtures in `instances_tests.rs` that build
      the `MultiStore`s the tests pass in.*

### Phase D: Split the controllers

*Landed 2026-10-05 in the same PR as C, E, F and G (not one PR per crate, by
request).*

- [x] `bindy-controller-cluster`: `bind9cluster/**` +
      `clusterbind9provider.rs`. *`controller(ctx)` runs both kinds.*
- [x] `bindy-controller-instance`: `bind9instance/**` (`placement` went to
      `bindy-bind9`, see C). **Deleted the `tokio::spawn` in the `DNSZone`
      mapper** (problem 2): `instances_selected_by_zone` returns the instances
      the zone selected, and their reconcile already refreshes `status.zones`.
      *Measured on kind, a pure mapper alone was worse than the task: every
      record reconcile stamps `lastReconciledAt` into its zone's status, and
      each of those writes enqueued a full reconcile of every instance the
      zone selected (30 instance reconciles in 330 s with 10 records and 3
      instances; records × instances at scale). The `DNSZone` stream is now
      filtered by `sdk::watch::changed_only` on what `status.zones` is built
      from (`zone_selection_key`: the selected instances, `spec.zoneName`,
      deletion), so timestamp writes are dropped and deletes always pass.*
- [x] `bindy-controller-zone`: `dnszone/**`. **Deleted the 2-second rate
      limiter** (problem 3); the 9 record mappers are nine
      `watch_records::<T>()` lines. *The primary predicate is
      `generation + finalizers + labels + annotations`, not `generation`
      alone: kube's `finalizer()` helper waits for its own patch event
      (ADR-0009 §4, amended).*
- [x] `bindy-controller-records`: `records/**` + `record_operator.rs`
      + `record_impls.rs` + `record_wrappers.rs`. *The 9 `reconcile_*_record`
      wrappers are gone: `DnsRecordType::reconcile_record` is a provided
      method over the generic path; the unused `generate_record_wrapper!`
      macro is deleted.*
- [x] **DoD per crate:** its tests move with it and pass; the crate
      exposes exactly one public entry point,
      `pub async fn controller(ctx) -> Result<()>`. *Every other module is
      private. That exposed dead code the `pub` modules had hidden, deleted
      with its tests: `create_managed_instance`, `delete_bind9cluster`,
      `delete_clusterbind9provider`, two `delete_bind9instance`s,
      `is_resource_ready`, `find_zones_selecting_record`,
      `detect_spec_changes`, `detect_instance_changes`, `refetch_zone`,
      `handle_duplicate_zone`, `find_all_secondary_pods`,
      `for_each_secondary_endpoint`, the zone `constants` module and
      `ConflictingZone::instance_names`. The shared wrapper bookkeeping
      moved to `sdk::reconcile` (`instrumented`, `finalizer_error`).*

### Phase E: `bindy-scout` + `bindy-bootstrap`

- [x] Move `scout.rs` into `bindy-scout` and rebuild its 5 controllers on
      the SDK's `WatchSet`. *Two `WatchSet`s: one over the local client
      (Ingress, Service and each served route kind) and one over the remote
      client for the target namespace's `DNSZone`s, replacing the hand-rolled
      reflector and its sleep-on-error loop. Its watch-error diagnosis moved
      into the SDK, so every watch logs it. `scout_integration.rs` moved with
      the crate.*
- [x] Move `bootstrap.rs` into `bindy-bootstrap`. Keep the RBAC sync
      contract from `CLAUDE.md` intact. *`rbac_drift` compares the Scout
      ClusterRole, writer and secrets-reader Roles and their bindings with
      `deploy/scout/*.yaml` and the `docs/src/guide/scout.md` examples
      (excerpts may show only rules the role has). Verified by mutating a
      verb. `deploy/scout.yaml` never existed; `CLAUDE.md` is corrected.*
- [x] **DoD:** `bindy bootstrap …` and `bindy scout …` behave
      identically; `bootstrap_tests.rs` moves with the crate and passes.
      *Same clap definitions and dispatch, moved to `crates/bindy/src/cli.rs`.*

### Phase F: Thin the binary

- [x] `main.rs` keeps only: CLI (`clap`), tracing init, client
      construction, metrics server, leader election handoff, and
      `try_join_all` over each crate's `controller()`. *The clap types and the
      bootstrap/scout dispatch are in `cli.rs`; `Context::new` (the
      `WatchSet` registration) moved to the SDK. The `bindy` library target
      is gone; its integration tests use `bindy_api`.*
- [x] Replace the 13-arm `tokio::select!` with
      `graceful_shutdown_on(shutdown.clone())` per controller +
      `futures::future::try_join_all` (problem 6), so SIGTERM and
      leadership loss **drain** instead of cancelling. *`sdk::shutdown`: a
      trigger and a cloneable signal (held in `Context`), and `supervise`,
      which turns a controller that stops before shutdown into an error.
      SIGTERM drains and exits 0; lease loss drains and exits non-zero. A
      SIGTERM while waiting for the lease exits cleanly.*
- [x] Delete `perform_startup_drift_detection` (problem 7), gated on an
      integration test. *The restart e2e suite now scales the operator to
      zero, deletes every instance's Service, scales back up and requires
      each Service recreated within 120 s of start (ADR-0009 §5, amended).
      Results in Phase G.*
- [x] **DoD:** `src/main.rs` < 300 lines; `rg 'Controller::new' crates/bindy/src`
      returns nothing. *257 lines; no `Controller::new` anywhere in the
      workspace.*

### Phase G: Verify

*Run 2026-10-05 on slate (kind, podman) with an image built from the branch.*

- [x] Count watch connections before/after against a `kind` cluster
      (API-server `apiserver_longrunning_requests` or `kubectl get
      --raw /metrics`). Expect roughly **59 → 14** in cluster-wide mode,
      and per-namespace-target scaling to match. *Open WATCH requests per
      resource with the operator running, minus the same with it scaled to
      zero: **58** with bindy v0.7.1, **20** with this branch (one per
      cached kind plus the four uncached owned kinds; the code opens one
      ConfigMap watch and the measured delta shows two, so 19 is the
      operator's). Not 14: the uncached owned kinds keep ordinary watches by
      design (ADR-0009 §3). Per-namespace scaling follows from the
      `WatchSet` keying on (kind, target), covered by its unit tests and the
      multi-tenancy suite.*
- [x] `make kind-integration-test` green end-to-end. *`tests/integration_test.sh`
      passed (Rust API, lifecycle, idempotency, restart, including the
      startup-repair gate: Services back 11 to 16 s after start), and
      `tests/e2e/scout_test.sh` passed. Also passed: multi-tenancy, regression, TLS transport, and the live `scout_integration.rs`. Zone-spread passed too, once slate's `fs.inotify.max_user_instances` was raised to 512 for the four-node cluster. After the instance fan-out fix (Phase D) the integration, multi-tenancy and zone-spread suites were run again on the new image and passed.
      With the fixture's zones applied, 17 `DNSZone` reconciles in 330 s
      across 5 zones, all from the requeue and error backoff: no reconcile
      storm without the 2-second limiter.*
- [x] Compare `cargo build` wall-clock for a one-line change to
      `crd.rs`, before vs after. *Dev profile, warm, slate: 10.5 to 11.1 s
      before (`main` @ `a6c95c2`), 6.7 to 8.6 s after. A one-line change in
      the zone controller or Scout now rebuilds in about 4 s.*
- [x] Update `docs/src/architecture/` and the concept pages. *Watch wiring,
      instance mapper, drain and crate tree in `concepts/architecture.md`
      and `development/setup.md`; source paths across `docs/src/**`. The
      CALM-generated diagrams already described the target state.*
- [x] `.claude/CHANGELOG.md` entry with `**Author:**` per phase. *One
      entry for C to G (one PR).*
- [x] Threat model full pass (ADR-0009 follow-up). *v1.11: M-41 to M-43,
      residual risks 10 and 11.*

---

## 5. Explicitly out of scope

- **No CRD schema changes.** This is a code-organisation change; if a
  phase seems to need a schema change, that is a separate ADR.
- **No behaviour changes** beyond the four defects called out above
  (spawned-write mapper, rate limiter, non-draining shutdown, redundant
  drift pass). Anything else observed mid-refactor gets logged, not
  fixed in the same PR.
- **No new dependencies.** Every API this plan relies on
  (`store_shared`, `for_shared_stream`, `owns_shared_stream`,
  `watches_shared_stream`, `graceful_shutdown_on`, `shutdown_on_signal`,
  `predicates::generation`) is already present in the pinned
  kube-runtime 4.2.0. The stream APIs B2 uses need kube-runtime's
  `unstable-runtime-stream-control` feature turned on: a new feature, not a
  new crate, accepted in ADR-0009 §3 (amended). `async-broadcast`, already in
  the tree through kube-runtime, became a direct dependency of the SDK.
- **Splitting `bindy-api` per API group.** Tempting at 4,203 lines, but
  it is a leaf with no intra-crate deps and it churns least; revisit
  after Phase G.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| A shared reflector store is a single point of failure: if one stream dies, every subscriber goes stale | `WatchSet` supervises each stream with `StreamBackoff` and exposes per-kind staleness as a metric; today's per-controller streams already fail silently (`warn!("… reflector stream ended")` and the task simply ends) |
| Removing the DNSZone rate limiter re-exposes a hot loop | Land `predicates::generation` **first**, in its own PR, and watch `bindy_reconciliations_total{kind="DNSZone"}` on a `kind` cluster for a full requeue period before deleting the limiter |
| Deleting startup drift detection loses a real recovery path | Phase F gates the deletion on a passing integration test, not on reasoning |
| A 10-crate workspace slows clean builds | Clean builds get slightly slower; *incremental* builds, the ones that matter day to day, get much faster, because `crd.rs` stops invalidating scout, bootstrap and every reconciler |
| Long-lived refactor branch rots against `main` | One crate per PR, each independently mergeable and green, per the phase list above |
