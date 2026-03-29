# Scout: Namespace Inclusion/Exclusion via Label Selectors

**Status:** Proposed
**Date:** 2026-03-28
**Author:** Erick Bourgeois

---

## Overview

Scout currently watches all Ingress and Service resources cluster-wide and filters by namespace at reconcile time using a static exclusion list (`BINDY_SCOUT_EXCLUDE_NAMESPACES`). Every event — from every namespace — reaches the controller, even for namespaces Scout will never act on.

This roadmap introduces **label-selector-based namespace inclusion and exclusion**, replacing static filtering with a dynamic `Namespace` watch. The effective watched set is the **intersection** of the inclusion set and the complement of the exclusion set. Scout reacts to namespace label changes in real time, significantly reducing event volume on large clusters.

---

## Goals

- Add `BINDY_SCOUT_INCLUDE_NAMESPACE_SELECTOR` — a label selector namespaces must match to be watched
- Add `BINDY_SCOUT_EXCLUDE_NAMESPACE_SELECTOR` — a label selector namespaces matching it are always skipped
- Define the effective set as `include ∩ (all − exclude − static_excludes)`
- Watch `Namespace` resources reactively — when a namespace gains or loses qualifying labels, Scout updates the active set and re-reconciles resources accordingly
- Remain fully backward compatible — no inclusion selector set means all namespaces are included (current behaviour preserved)

## Non-Goals

- Per-namespace dynamic `Controller` instances (complex lifecycle management; deferred)
- Integration with Kubernetes `NetworkPolicy` or `ResourceQuota` namespace scoping
- Automatic cleanup of ARecords when a namespace is de-listed (initial pass: log a warning; full cleanup is a follow-up)

---

## Background

### Current state

```rust
// scout.rs — ScoutContext
pub excluded_namespaces: Vec<String>,   // static, computed once at startup

// reconcile() and reconcile_service() — check at the TOP of every reconcile call
if ctx.excluded_namespaces.contains(&namespace) {
    return Ok(Action::await_change());
}
```

Both the Ingress and Service controllers use `Api::all(local_client)` with `WatcherConfig::default()` — no field or label selectors. All Ingress and Service events from the entire cluster are delivered to the reconcilers, even for excluded namespaces.

### Performance problem

On a cluster with 500 namespaces where Scout should only act on 20, 96% of all watch events are discarded at reconcile time. The Kubernetes API server streams all events, the kube-rs controller runtime queues them, and the reconcilers drop them immediately. This wastes CPU, memory, and API server watch connections at scale.

### Why namespace label selectors help

Kubernetes supports label selectors on `Namespace` list/watch operations natively. By watching only `Namespace` resources with a given label, Scout gets a compact, live-updated view of the active namespace set. When namespace membership changes (label added/removed), Scout can react: re-trigger reconciliation for newly-included namespaces or suppress future events from newly-excluded ones.

---

## Design

### Effective namespace set

```
effective = inclusion_matches ∩ (all_namespaces − exclusion_matches − static_excludes)
```

| Scenario | Effective set |
|----------|--------------|
| No selectors configured | All namespaces minus `static_excludes` (current behaviour) |
| Inclusion selector only | Namespaces with matching labels |
| Exclusion selector only | All namespaces minus matching labels minus `static_excludes` |
| Both selectors | Inclusion matches minus exclusion matches minus `static_excludes` |

`static_excludes` are: `POD_NAMESPACE` + `BINDY_SCOUT_EXCLUDE_NAMESPACES` (comma-separated names). These always take precedence over any label selector.

### New configuration

| Env var | CLI flag | Default | Description |
|---------|----------|---------|-------------|
| `BINDY_SCOUT_INCLUDE_NAMESPACE_SELECTOR` | `--include-namespace-selector` | *(unset — all)* | Label selector that namespaces must match to be watched (e.g. `scout.bindy.io/enabled=true`). When unset, all namespaces are included. |
| `BINDY_SCOUT_EXCLUDE_NAMESPACE_SELECTOR` | `--exclude-namespace-selector` | *(unset — none)* | Label selector for namespaces to always skip (e.g. `scout.bindy.io/exclude=true`). |

Both accept standard Kubernetes label selector syntax: `key=value`, `key in (v1,v2)`, `!key`, etc.

### Dynamic namespace set — `Arc<RwLock<HashSet<String>>>`

The active namespace set is stored in a shared, reader-writer locked set on `ScoutContext`:

```rust
pub struct ScoutContext {
    // existing fields ...

    /// Dynamically maintained set of namespaces Scout is active in.
    /// Updated by the Namespace watcher when labels change.
    /// Checked at reconcile time instead of (or in addition to) excluded_namespaces.
    pub active_namespaces: Option<Arc<RwLock<HashSet<String>>>>,
    // None = no inclusion selector configured = all namespaces allowed (current behaviour)
}
```

When `active_namespaces` is `Some`, reconcilers check:

```rust
if let Some(ref active) = ctx.active_namespaces {
    if !active.read().unwrap().contains(&namespace) {
        return Ok(Action::await_change());
    }
}
```

When `None`, the existing `excluded_namespaces` check applies as today.

### Namespace watcher

A new dedicated async task watches `Namespace` resources using both selectors:

```rust
// Pseudo-code — exact implementation determined during development
let ns_api: Api<Namespace> = Api::all(local_client.clone());

// Use the inclusion selector on the watcher to reduce events from the API server
let watcher_config = match &config.include_namespace_selector {
    Some(sel) => WatcherConfig::default().labels(sel),
    None => WatcherConfig::default(),
};

let ns_stream = watcher(ns_api, watcher_config);
```

The watcher task drives the `active_namespaces` set:

1. **Namespace ADDED or MODIFIED**: re-evaluate against both selectors and `static_excludes`:
   - If it should now be active → insert into set; trigger re-reconcile of existing Ingresses/Services in that namespace
   - If it should no longer be active → remove from set; log a warning (ARecord cleanup is follow-up work)

2. **Namespace DELETED**: remove from active set; log a warning.

3. **Restart/re-list**: rebuild the full active set from the current cluster state (handled naturally by the kube-rs watcher restart loop).

### Re-triggering reconciliation for newly-included namespaces

When a namespace enters the active set (e.g. platform team adds `scout.bindy.io/enabled: "true"` to it), Scout should process the existing Ingresses and Services in that namespace rather than waiting for them to change naturally.

Implementation: after inserting the namespace into the active set, list all Ingresses and Services in that namespace via the local client and enqueue them for reconciliation using the existing reconciler functions directly (or by publishing a synthetic reconcile request if kube-rs exposes that API).

### Backward compatibility

| Existing configuration | Behaviour after this change |
|------------------------|----------------------------|
| Neither selector set | Identical — `active_namespaces = None`, falls through to `excluded_namespaces` check |
| Only `BINDY_SCOUT_EXCLUDE_NAMESPACES` set | Identical — static excludes still applied |
| New selectors set alongside old excludes | Static excludes still take precedence |

No existing Scout deployments need to change their configuration.

---

## Implementation Plan

### Phase 1 — Namespace reflector and active set

1. **Add configuration fields** to `ScoutConfig` / CLI args:
   - `include_namespace_selector: Option<String>`
   - `exclude_namespace_selector: Option<String>`

2. **Add `active_namespaces: Option<Arc<RwLock<HashSet<String>>>>` to `ScoutContext`**.

3. **Build and populate the initial active set** at startup by listing all Namespaces and applying both selectors plus `static_excludes`.

4. **Write tests first (TDD)** in `src/scout_tests.rs`:
   - `test_active_namespaces_no_selector_is_none`
   - `test_active_namespaces_include_selector_filters_correctly`
   - `test_active_namespaces_exclude_selector_removes_matches`
   - `test_active_namespaces_intersection_of_include_and_exclude`
   - `test_static_excludes_always_remove_from_active_set`
   - `test_own_namespace_always_excluded_regardless_of_selectors`

### Phase 2 — Namespace watcher task

1. **Add a namespace watcher async task** using kube-rs `watcher()` with the inclusion label selector (reduces events at the Kubernetes API level).

2. **Drive the `active_namespaces` set** reactively as namespaces are added, modified (re-label), or deleted.

3. **On namespace entering active set**: list and re-reconcile existing Ingresses and Services in that namespace.

4. **Write tests** (async, using mock k8s objects):
   - `test_namespace_entering_active_set_triggers_reconcile`
   - `test_namespace_leaving_active_set_suppresses_future_events`
   - `test_namespace_watcher_rebuilds_set_on_restart`

### Phase 3 — Reconciler integration

1. **Update `reconcile` (Ingress)** and **`reconcile_service`** to check `active_namespaces` when set, before the existing `excluded_namespaces` check.

2. **Update `ScoutContext` construction** to pass the new fields.

3. **Run `cargo-quality` skill** after all changes.

### Phase 4 — Documentation and configuration

1. **Update `docs/src/guide/scout.md`**:
   - New "Namespace Scoping" section explaining inclusion/exclusion selectors
   - Example: opt-in model (`scout.bindy.io/enabled=true` on namespace)
   - Example: opt-out model (`scout.bindy.io/skip=true` on namespace)
   - Update annotations/configuration reference tables

2. **Update `docs/src/installation/scout.md`**: add new env vars to config reference table.

3. **Follow RBAC sync rule**: if Namespace `get/list/watch` is not already in the Scout ClusterRole, add it to `bootstrap.rs`, `deploy/scout/clusterrole.yaml`, and `deploy/scout.yaml`.

---

## Open Questions

1. **ARecord cleanup on namespace exclusion**: when a namespace is removed from the active set (label removed), should Scout immediately delete all ARecords it created from resources in that namespace? Recommendation: yes for a clean model, but deferred — initial implementation logs a warning and leaves records in place.

2. **Inclusion selector semantics when unset**: "all namespaces" is the safe default and preserves backward compat. Should we require an explicit `"*"` or leave unset to mean "all"? Recommendation: unset = all (less config for existing deployments).

3. **Re-reconcile on namespace entry**: listing and re-triggering all Ingresses/Services in a newly-active namespace is correct but could be expensive for large namespaces. Should there be a rate limit or batch size? Recommendation: no limit initially; add if it proves problematic.

4. **Interaction with `--default-zone`**: if a namespace enters the active set but the Ingresses in it have no `zone` annotation and no `--default-zone` is configured, Scout will emit warnings and skip them. This is existing behaviour and unchanged by this feature.
