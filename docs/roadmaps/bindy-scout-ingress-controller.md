# Bindy Scout — Ingress-to-ARecord Controller

**Status:** Planning
**Created:** 2026-03-18
**Author:** Erick Bourgeois

---

## Overview

`bindy scout` is a new sub-command of the `bindy` binary that watches Kubernetes Ingresses across
namespaces and automatically creates `ARecord` CRs on the bindy cluster when an Ingress is annotated
to opt in. The name comes from bee biology: scout bees range out from the hive, discover new resources,
and report back — exactly what this controller does with Ingress hostnames.

This work also restructures the `bindy` binary to use **clap sub-commands**, replacing the current
single-entrypoint design:

```
bindy run   # existing operator behaviour (unchanged)
bindy scout      # new ingress watcher
```

---

## Architecture

### Topology

```
┌──────────────────────────────────────────────────┐
│  Workload Cluster (k0rdent child / app cluster)  │
│                                                  │
│   bindy-scout                                    │
│   ├── LOCAL client (in-cluster ServiceAccount)  │
│   │   ├── Watches Ingresses (all ns except own) │
│   │   ├── Manages finalizers on Ingresses        │
│   │   └── Reads remote kubeconfig Secret (P1)   │
│   │                                              │
│   └── REMOTE client                             │
│       ├── Phase 1: kubeconfig from Secret        │
│       ├── Phase 2: Linkerd mTLS service mirror   │
│       ├── Reads DNSZones (validation cache)      │
│       └── Creates / deletes ARecords            │
└──────────────────────────────────────────────────┘
                        │
          Phase 1: kubeconfig Secret
          Phase 2: Linkerd mTLS
                        ▼
┌──────────────────────────────────────────────────┐
│  Bindy Cluster (dedicated DNS management cluster)│
│                                                  │
│   bindy run                                 │
│   ├── ServiceAccount per child cluster           │
│   │   (scoped to ARecords + DNSZones read-only)  │
│   └── Reconciles ARecords → BIND9 zone files     │
└──────────────────────────────────────────────────┘
```

### Two-client model

Scout requires two independent Kubernetes clients:

| | Local client | Remote client |
|---|---|---|
| **Auth** | In-cluster ServiceAccount | Kubeconfig Secret (Phase 1) / Linkerd (Phase 2) |
| **Watches** | Ingresses | DNSZones (reflector cache for validation) |
| **Writes** | Finalizers on Ingresses | ARecords |

---

## Ingress Selection

### Opt-in annotation (required)

```yaml
bindy.firestoned.io/recordKind: "ARecord"
```

### Zone label (required)

Identifies which bindy-managed DNS zone owns this hostname. Scout does not auto-detect zones from
hostname suffixes; the zone must be stated explicitly to avoid ambiguity.

```yaml
bindy.firestoned.io/zone: "example.com"
```

### Optional annotation overrides

```yaml
bindy.firestoned.io/ip: "1.2.3.4"      # override LB IP (static/non-LB ingresses)
bindy.firestoned.io/ttl: "300"         # TTL override for created ARecord
```

### Ignored namespaces

Scout's own namespace is always excluded. Additional exclusions are configurable via
`BINDY_SCOUT_EXCLUDE_NAMESPACES` (comma-separated). Default: scout's own namespace only.

---

## Record Creation

### IP resolution (in order)

1. `bindy.firestoned.io/ip` annotation — explicit override
2. `status.loadBalancer.ingress[].ip` — first non-empty IP from the LB status
3. `status.loadBalancer.ingress[].hostname` — not used for A records; emits a warning event and skips

If no IP can be resolved, scout emits a Kubernetes warning event on the Ingress and skips the record.

### ARecord name derivation

Given `Ingress.spec.rules[].host = "app.example.com"` and `bindy.firestoned.io/zone: "example.com"`:

- ARecord `spec.name` = `app` (strip zone suffix from host)
- ARecord `spec.ipv4_addresses` = resolved IP(s)
- ARecord CR `metadata.name` = `scout-{namespace}-{ingress-name}-{host-index}` (sanitized to DNS label rules)
- ARecord CR `metadata.namespace` = configured remote namespace (see env vars)

Multiple `spec.rules` entries on one Ingress each produce a separate ARecord CR.

### Labels on created ARecords

```yaml
labels:
  bindy.firestoned.io/managed-by: scout
  bindy.firestoned.io/source-cluster: "<BINDY_SCOUT_CLUSTER_NAME>"
  bindy.firestoned.io/source-namespace: "<ingress-namespace>"
  bindy.firestoned.io/source-ingress: "<ingress-name>"
  # zone label so bindy's DNSZone selector can pick it up:
  bindy.firestoned.io/zone: "example.com"
```

---

## Zone Validation

Scout maintains a **remote reflector** (DNSZone store via the remote client). Before creating an
ARecord, it checks that the zone named in `bindy.firestoned.io/zone` exists in the store.

If the zone is not found:
- Scout emits a `Warning` event on the Ingress: `Zone "example.com" not found on bindy cluster`
- Scout adds a status annotation to the Ingress: `bindy.firestoned.io/arecord-status: zone-not-found`
- Scout skips ARecord creation until the zone appears (re-queues via watch)

---

## Cleanup / Finalizer

Cross-namespace owner references do not work across clusters. Scout uses a finalizer instead:

```
bindy.firestoned.io/arecord-finalizer
```

**Flow:**
1. Scout adds the finalizer to the Ingress on first reconcile.
2. When `metadata.deletionTimestamp` is set, scout deletes all ARecords it created on the remote cluster.
3. Once all remote ARecords are deleted, scout removes the finalizer.
4. If the remote client is unreachable, the finalizer blocks deletion (safe default) and emits a warning
   event on a configurable interval.

---

## Error Handling

| Condition | Behaviour |
|---|---|
| No IP on LB status | Warning event on Ingress, skip, requeue |
| Zone not found on bindy cluster | Warning event + status annotation on Ingress, skip, requeue |
| Remote client unreachable | Warning event, exponential backoff retry |
| Duplicate host across clusters | Remote ARecord updated in place (last-write-wins); future: conflict detection |
| Ingress host != zone suffix | Warning event, skip this host rule |

---

## Future: Gateway API

The same controller pattern will be extended to watch `HTTPRoute` and `Gateway` resources from the
Kubernetes Gateway API (`gateway.networking.k8s.io`). The annotation and label scheme is identical.
This is deliberately left out of Phase 1 to keep scope manageable.

---

## Phase 0 — Binary Restructure (clap subcommands)

**Goal:** Add `clap` and restructure `main.rs` so that `bindy run` runs the existing logic and
`bindy scout` is a new entry point. No functional change to the operator.

**Status:** ✅ Complete (2026-03-18)

### Tasks

- [x] Add `clap` dependency to `Cargo.toml` (with `derive` feature)
- [x] Introduce top-level `Cli` struct and `Commands` enum in `main.rs`
- [x] Rename `async_main()` to `run_command()` in `main.rs`
- [x] Wire `bindy run` subcommand to the operator logic
- [x] Add stub `bindy scout` subcommand
- [x] Update `cargo fmt`, `cargo clippy`, `cargo test` — all pass
- [x] Update Docker `CMD` in all Dockerfiles to `["bindy", "run"]`
- [x] Update deploy manifests to pass `run` subcommand (`args: ["run"]`)
- [x] Update `.claude/CHANGELOG.md`

### New `BINDY_` env vars

None for Phase 0. All existing env vars remain unchanged.

---

## Phase 1 — Scout: Same-cluster mode + shell completion

**Goal:** `bindy scout` watches Ingresses on the **same** cluster and creates ARecords in a
configurable namespace on that same cluster. Remote connectivity is not yet required. This validates
the core reconciliation loop before adding network complexity.

Also adds `bindy completion <shell>` for shell completion (bash, zsh, fish, powershell).

**Status:** ✅ Complete (2026-03-18)

### Tasks

- [x] Define `ScoutContext` struct (local client, stores)
- [x] Implement reconciler:
  - [x] Filter by annotation `bindy.firestoned.io/recordKind` with value `"ARecord"`
  - [x] Read zone annotation `bindy.firestoned.io/zone`
  - [x] Resolve IP from LB status or annotation override
  - [x] Validate zone exists (local DNSZone reflector for same-cluster mode)
  - [x] Derive ARecord name and spec from Ingress host rules
  - [x] Create / update ARecord CR (server-side apply)
  - [ ] Add finalizer to Ingress (Phase 1.5 — deferred)
  - [ ] Handle deletion (finalizer cleanup, delete ARecords) (Phase 1.5 — deferred)
  - [x] Log warnings for all error conditions
- [x] Write tests (`src/scout_tests.rs`) — 16 unit tests, TDD-first
- [x] Add `clap_complete = "4"` and `bindy completion` subcommand
- [ ] RBAC manifest for scout ServiceAccount (same cluster)
- [x] Update `.claude/CHANGELOG.md`

### New `BINDY_SCOUT_` env vars

| Variable | Default | Description |
|---|---|---|
| `BINDY_SCOUT_NAMESPACE` | `bindy-system` | Namespace on target cluster where ARecords are created |
| `BINDY_SCOUT_EXCLUDE_NAMESPACES` | scout's own namespace | Comma-separated list of namespaces to skip |
| `BINDY_SCOUT_CLUSTER_NAME` | `""` (required) | Logical name of this cluster, used in ARecord CR labels |

---

## Phase 2 — Scout: Remote cluster mode

**Goal:** `bindy scout` creates ARecords on a **remote** bindy cluster using a kubeconfig stored in
a Kubernetes Secret on the local cluster.

### Tasks

- [ ] Implement `RemoteClientBuilder`:
  - [ ] Read Secret named by `BINDY_SCOUT_REMOTE_SECRET` from local cluster
  - [ ] Parse kubeconfig from `Secret.data["kubeconfig"]`
  - [ ] Build `kube::Client` from parsed config
  - [ ] Implement health check / reconnect on startup
- [ ] Extend `ScoutContext` with `remote_client` and `remote_zone_store`
- [ ] Switch ARecord and DNSZone API calls to use `remote_client`
- [ ] Keep local client for Ingress watch and finalizer management
- [ ] RBAC manifests:
  - [ ] Local cluster: add `secrets: get` for the remote kubeconfig Secret
  - [ ] Remote (bindy) cluster: ServiceAccount scoped to `arecords` (crud) + `dnszones` (read)
- [ ] Example Secret manifest (redacted kubeconfig) in `deploy/scout/`
- [ ] Update `.claude/CHANGELOG.md`

### New `BINDY_SCOUT_` env vars

| Variable | Default | Description |
|---|---|---|
| `BINDY_SCOUT_REMOTE_SECRET` | `""` (if unset: same-cluster mode) | Name of Secret containing remote kubeconfig |
| `BINDY_SCOUT_REMOTE_SECRET_NAMESPACE` | scout's own namespace | Namespace of the remote kubeconfig Secret |

---

## Phase 3 — Scout: Linkerd mTLS

**Goal:** Replace the kubeconfig Secret with a Linkerd multicluster service mirror connection. The
bindy cluster's Kubernetes API is exposed as a mirrored service in the workload cluster. Scout
connects to this endpoint using its local ServiceAccount token, authenticated via Linkerd mTLS.

### Tasks

- [ ] Research Linkerd multicluster service mirror API endpoint configuration
- [ ] Update `RemoteClientBuilder` to support a `BINDY_SCOUT_REMOTE_ENDPOINT` override (skip
  kubeconfig Secret; use endpoint + in-cluster token)
- [ ] Add Linkerd mesh annotations to scout Deployment manifest
- [ ] Verify mTLS policy enforces server identity (bindy cluster API server certificate)
- [ ] Document Linkerd setup in `docs/src/scout/linkerd.md`
- [ ] Update `.claude/CHANGELOG.md`

### New `BINDY_SCOUT_` env vars

| Variable | Default | Description |
|---|---|---|
| `BINDY_SCOUT_REMOTE_ENDPOINT` | `""` | Override API server URL (Linkerd mirrored service). When set, `BINDY_SCOUT_REMOTE_SECRET` is not required |

---

## RBAC Summary

### Local cluster (workload)

```yaml
rules:
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses/finalizers"]
    verbs: ["update"]
  - apiGroups: [""]
    resources: ["secrets"]
    verbs: ["get"]                    # Phase 2: read remote kubeconfig Secret
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["create", "patch"]
```

### Remote cluster (bindy)

```yaml
rules:
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["arecords"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["dnszones"]
    verbs: ["get", "list", "watch"]   # read-only, for zone validation
```

---

## Open Questions

1. **Conflict detection across clusters:** If two workload clusters both create an ARecord for
   `app.example.com`, last-write-wins today. Should bindy detect this and set a conflict status?
   Deferred to post-Phase 2.

2. **AAAARecord support:** Scout could also watch for `bindy.firestoned.io/aaaarecord: "true"` and
   create `AAAARecord` CRs using the same pattern. Deferred to post-Phase 1.

3. **Gateway API:** `HTTPRoute` and `Gateway` resources use the same annotation/label scheme.
   Deferred to post-Phase 2. Requires the `gateway.networking.k8s.io` API group in RBAC.

4. **Metrics:** Scout should expose its own Prometheus metrics (ingresses_watched, arecords_created,
   errors_total). Shared metrics infrastructure with the operator. Design in Phase 1.
