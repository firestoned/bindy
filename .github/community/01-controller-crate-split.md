# 01: Controller crate split & watch-layer simplification

> **Goal.** Break the single `bindy` crate into a workspace of focused
> crates — one per controller, plus a shared controller SDK — and replace
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

**Status:** 🔶 Phase A done (2026-10-03); Phase B in progress: B1 (crate + framework modules), B2 (shared `WatchSet`) and B3 (`RecordKind`, `context` in the SDK) done 2026-10-04; B4 (leader election) next. [ADR-0009](../../docs/adr/0009-workspace-crate-split-and-shared-watch-layer.md) Accepted, CALM updated
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
  `bootstrap.rs` (2,136 lines), every reconciler and every test module —
  even though Scout only touches `ARecord`/`DNSZone` and bootstrap only
  emits YAML.
- There is no enforced boundary. Nothing in the type system stops a
  reconciler from reaching into `scout`, or `bind9` from reaching back
  into `reconcilers` — and one such back-edge already exists
  (`src/bind9/…` → `crate::reconcilers::retry`).
- `src/main.rs` (**2,086 lines**) is simultaneously the CLI, the
  controller framework, the reflector supervisor, the leader-election
  driver, the metrics server and the drift-detection pass.

The good news, which is why this split is cheap: **the dependency graph
is already almost a DAG.** `src/crd.rs` has *zero* intra-crate `use`
statements — it is a pure leaf. `src/reconcilers/**` imports
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
is a 14-field struct that has to be extended by hand — and in three
places (`initialize_shared_context`, the struct, `records_matching_selector`)
— every time a record kind is added. `src/scout.rs` then builds **5 more**
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
   for `InitApply` — ~45 lines where a single predicate over
   `event.into_iter()` would be three.

5. **No stated policy for `semantic_watcher_config()` vs
   `default_watcher_config()`.** Both exist (`main.rs:801`/`819`); which
   one a given `.watches()` gets is decided per call site with no rule
   written down, and it is exactly this choice that drives problem 3.

6. **No shared shutdown.** `run_all_operators`'s `tokio::select!` means
   the first controller to return **drops the other twelve mid-reconcile**
   — futures, not tasks, so there is no drain. kube-runtime has
   `Controller::graceful_shutdown_on()` / `.shutdown_on_signal()`;
   neither is used. SIGTERM handling lives one level up in
   `run_operators_with_leader_election` and likewise cancels rather than
   drains.

7. **Startup drift detection duplicates the controller's own startup.**
   `perform_startup_drift_detection` serially lists and reconciles every
   `ClusterBind9Provider`, `Bind9Cluster` and `Bind9Instance` **before**
   any controller starts. But a kube-runtime watcher emits
   `Init`/`InitApply` for every existing object on start, and the
   Controller enqueues each one — so the same full pass happens anyway,
   moments later, with backoff and concurrency control. The manual pass
   only delays becoming ready after acquiring the lease.

Plus two duplicates worth deleting on the way past: `ReconcileError` and
`error_policy` are defined *identically* in both `src/main.rs:47/2071`
and `src/record_operator.rs:31/38`.

---

## 2. Target shape

```
crates/
├── bindy-api/              # leaf — no intra-workspace deps
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
├── reconcile.rs    // the reconcile fn — pure domain logic
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
| Status-driven (zone ⇄ instance) | `any_semantic()` | Deliberate — and the *only* place it is allowed |

With `predicates::generation` on the `DNSZone` primary stream, the
2-second rate limiter in `reconcile_dnszone_wrapper` has nothing left to
protect against and is deleted outright.

---

## 4. Phases

Each phase is independently mergeable and leaves the binary working.
Per `.claude/rules/testing.md`, every phase is TDD — tests move with the
code they cover, and `cargo-quality` gates each one.

### Phase A — Workspace scaffold, no logic moves

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
- [ ] Move `finalizers`. *Blocked by the orphan rule: with
      `FinalizerCleanup` in the SDK and the CRD types in `bindy-api`,
      `impl FinalizerCleanup for Bind9Cluster` is illegal in any controller
      crate. Needs the trait reshaped so the cleanup is a type the
      controller owns; do it with Phase D, where the impls move.*
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
- [ ] Move leader election (`LeaseManagerBuilder` wiring,
      `load_leader_election_config`, `monitor_leadership`) into
      `sdk::leader`. *B4.*
- [ ] **DoD:** `cargo test -p bindy-controller-sdk` green; unit tests
      cover `WatchSet` subscriber fan-out and each predicate. *B1: 174 SDK
      tests green (1609 workspace-wide, up from 1605 by the 4 new
      `error` tests). B2: 11 `watch` tests (store follows init/apply/delete,
      every subscriber gets deletes, late-subscriber replay, predicate,
      errors, restart, routing); 1620 workspace-wide. "Each predicate" waits
      for the self-trigger policy step. Box ticks when B3/B4 land.*
- **Fixed in B2:** the zone controller watched `Endpoints` with `Api::all`
  in every mode. In namespace-restricted mode that needs cluster-wide
  `endpoints` access the namespaced RBAC does not grant, so the watch was
  refused and a replaced BIND9 pod waited for the zone's requeue. Now a
  per-namespace, label-selected `WatchSet` kind; cross-namespace zones are
  covered by `subscribe_all`. No RBAC change needed.
- **Observed in B2, not fixed (logged per §5):** in namespace-restricted
  mode the `Bind9Instance` controller subscribes to `DNSZone` in its own
  namespace only, so a zone in namespace A selecting an instance in B does
  not refresh B's `status.zones` until B's next reconcile. `subscribe_all`
  would fix it, but that mapper does work itself (the spawned task, problem
  2), which would then run once per namespace; fix it with that mapper in
  Phase D.

### Phase C — `bindy-bind9`

- [ ] Move `bind9/**`, `bind9_resources.rs`, `bind9_acl.rs`, `ddns.rs`,
      `dns_errors.rs`, `safe_volume.rs`.
- [ ] Assert the boundary: this crate depends on `bindy-api` and
      `bindy-controller-sdk` only — never on a controller crate.
- [ ] **DoD:** `cargo test -p bindy-bind9` green; no `kube::runtime`
      import anywhere in the crate.

### Phase D — Split the controllers (one PR per crate)

- [ ] `bindy-controller-cluster` — `bind9cluster/**` +
      `clusterbind9provider.rs`.
- [ ] `bindy-controller-instance` — `bind9instance/**` + `placement.rs`.
      **Delete the `tokio::spawn` in the `DNSZone` mapper** (problem 2):
      the mapper returns real `ObjectRef`s and
      `reconcile_instance_zones()` runs inside the reconciler where it
      gets retries, backoff and metrics.
- [ ] `bindy-controller-zone` — `dnszone/**`. Apply
      `predicates::generation` to the primary stream and **delete the
      2-second rate limiter** (problem 3). Collapse the 9 record mappers
      to `watches_records::<T>()` (problem, §3).
- [ ] `bindy-controller-records` — `records/**` + `record_operator.rs`
      + `record_impls.rs` + `record_wrappers.rs`, all generic over
      `RecordKind`. The 9 thin `reconcile_*_record` wrappers
      (`records/mod.rs:1373`–`1516`) become trait impls.
- [ ] **DoD per crate:** its tests move with it and pass; the crate
      exposes exactly one public entry point,
      `pub async fn controller(ctx) -> Result<()>`.

### Phase E — `bindy-scout` + `bindy-bootstrap`

- [ ] Move `scout.rs` (3,926 lines) into `bindy-scout` and rebuild its 5
      controllers on the SDK's `WatchSet` so Scout stops carrying its own
      copy of the pattern.
- [ ] Move `bootstrap.rs` into `bindy-bootstrap`. Keep the RBAC sync
      contract from `CLAUDE.md` intact — `deploy/scout/*.yaml`,
      `deploy/scout.yaml` and `docs/src/guide/scout.md` must still
      mirror `build_scout_cluster_role` / `build_scout_role`. Add a test
      that fails when they drift, so the rule is enforced rather than
      remembered.
- [ ] **DoD:** `bindy bootstrap …` and `bindy scout …` behave
      identically; `bootstrap_tests.rs` moves with the crate and passes.

### Phase F — Thin the binary

- [ ] `main.rs` keeps only: CLI (`clap`), tracing init, client
      construction, metrics server, leader election handoff, and
      `try_join_all` over each crate's `controller()`.
- [ ] Replace the 13-arm `tokio::select!` with
      `graceful_shutdown_on(shutdown.clone())` per controller +
      `futures::future::try_join_all` (problem 6), so SIGTERM and
      leadership loss **drain** instead of cancelling.
- [ ] Delete `perform_startup_drift_detection` (problem 7) — but first
      add an integration test that asserts every pre-existing
      `Bind9Instance` is reconciled within N seconds of controller start
      from the watcher's own `Init`/`InitApply` events. Only delete once
      that test is green.
- [ ] **DoD:** `src/main.rs` < 300 lines; `rg 'Controller::new' crates/bindy/src`
      returns nothing.

### Phase G — Verify

- [ ] Count watch connections before/after against a `kind` cluster
      (API-server `apiserver_longrunning_requests` or `kubectl get
      --raw /metrics`). Expect roughly **59 → 14** in cluster-wide mode,
      and per-namespace-target scaling to match.
- [ ] `make kind-integration-test` green end-to-end.
- [ ] Compare `cargo build` wall-clock for a one-line change to
      `crd.rs`, before vs after.
- [ ] Update `docs/src/architecture/` — the reconciliation-flow diagrams
      describe the current single-crate layout and will be wrong.
- [ ] `.claude/CHANGELOG.md` entry with `**Author:**` per phase.

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
| A shared reflector store is a single point of failure — if one stream dies, every subscriber goes stale | `WatchSet` supervises each stream with `StreamBackoff` and exposes per-kind staleness as a metric; today's per-controller streams already fail silently (`warn!("… reflector stream ended")` and the task simply ends) |
| Removing the DNSZone rate limiter re-exposes a hot loop | Land `predicates::generation` **first**, in its own PR, and watch `bindy_reconciliations_total{kind="DNSZone"}` on a `kind` cluster for a full requeue period before deleting the limiter |
| Deleting startup drift detection loses a real recovery path | Phase F gates the deletion on a passing integration test, not on reasoning |
| A 10-crate workspace slows clean builds | Clean builds get slightly slower; *incremental* builds — the ones that matter day to day — get much faster, because `crd.rs` stops invalidating scout, bootstrap and every reconciler |
| Long-lived refactor branch rots against `main` | One crate per PR, each independently mergeable and green, per the phase list above |
