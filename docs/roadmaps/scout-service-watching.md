# Scout: Service Watching Support

**Status:** Complete
**Created:** 2026-03-25
**Completed:** 2026-03-27
**Author:** Erick Bourgeois

---

## Overview

Extend Scout to watch `kind: Service` (in addition to `kind: Ingress`) and automatically create ARecords from `LoadBalancer` services. The service name becomes the hostname prefix, the DNS zone is resolved via the same annotation as Ingress, and the IP is taken from `status.loadBalancer.ingress[].ip`. If no external IP is assigned yet, Scout warns and re-queues — no ARecord is created until the IP is available.

---

## Motivation

Ingress is the primary entry point for HTTP workloads, but many services (gRPC, TCP, custom protocols) are exposed directly via `LoadBalancer` Services without an Ingress. Today, operators must manually create ARecords for these services. Service watching extends Scout's zero-touch DNS automation to cover this common pattern.

---

## Behavioural Specification

### Opt-in

Same annotation as Ingress — Service must explicitly opt in:

```yaml
metadata:
  annotations:
    bindy.firestoned.io/scout-enabled: "true"
    bindy.firestoned.io/zone: "example.com"          # required if no --default-zone
    # bindy.firestoned.io/ip: "1.2.3.4"              # optional override
    # bindy.firestoned.io/ttl: "300"                  # optional override
```

### ARecord Name Derivation

The service **name** is used as the hostname prefix. The same `derive_record_name` logic applies:

| Service Name | Zone | ARecord `.spec.name` |
|---|---|---|
| `my-api` | `example.com` | `my-api` |
| `example-com` | `example.com` | *(invalid — no suffix match, error)* |

The full DNS name becomes `{service-name}.{zone}`, e.g. `my-api.example.com`.

> Note: Unlike Ingress (which has multiple `rules[].host` entries), a Service has exactly **one** record: `{service.metadata.name}.{zone}`. No index suffix in the CR name.

### ARecord CR Name

```
scout-{cluster}-{namespace}-{service-name}
```

Follows the same sanitisation and 253-char truncation as Ingress ARecord names, using the new `LABEL_SOURCE_SERVICE` label instead of `LABEL_SOURCE_INGRESS`.

### IP Resolution (Priority Order)

Identical priority chain to Ingress:

1. `bindy.firestoned.io/ip` annotation (explicit override)
2. `--default-ips` / `BINDY_SCOUT_DEFAULT_IPS` (shared VIP topology)
3. `service.status.loadBalancer.ingress[].ip` (first non-empty entry)
4. **None → `warn!` + requeue** — no ARecord created or updated until IP is available

```
WARN scout: service demo-apps/my-api has no external IP yet; requeueing in 30s
```

> Services of type `ClusterIP` or `NodePort` have no `loadBalancer` status. Scout **skips** them without warning (non-LB services are not expected to have external IPs).

### Finalizer and Cleanup

Scout adds `bindy.firestoned.io/arecord-finalizer` to opted-in Services (same finalizer name as Ingress). On deletion, Scout deletes the ARecord matching the label selector:

```
bindy.firestoned.io/managed-by=scout,
bindy.firestoned.io/source-cluster={cluster},
bindy.firestoned.io/source-namespace={namespace},
bindy.firestoned.io/source-service={service-name}
```

### Labels on Created ARecords

```yaml
labels:
  bindy.firestoned.io/managed-by: "scout"
  bindy.firestoned.io/source-cluster: "${cluster}"
  bindy.firestoned.io/source-namespace: "${namespace}"
  bindy.firestoned.io/source-service: "${service-name}"   # NEW label (vs source-ingress)
  bindy.firestoned.io/zone: "example.com"
```

---

## Implementation Plan

### Phase 1 — Core Service Reconciler (TDD)

#### Step 1.1 — New Constant

Add to `src/scout.rs` constants block:

```rust
pub const LABEL_SOURCE_SERVICE: &str = "bindy.firestoned.io/source-service";
```

#### Step 1.2 — Write Tests First (RED)

Add to `src/scout_tests.rs`:

```
test_derive_service_record_name_simple           → "my-api" + "example.com" → "my-api"
test_derive_service_record_name_apex             → "example-com" + "example.com" → error
test_service_arecord_cr_name_format              → correct prefix + sanitisation
test_resolve_ip_no_lb_status_requeues            → Service with no LB IP → None
test_resolve_ip_from_service_lb_status           → Service with LB IP → Some("1.2.3.4")
test_resolve_ip_service_cluster_ip_skipped       → ClusterIP service → None (no warn)
test_build_service_arecord_sets_source_service_label
test_build_service_arecord_uses_service_name_as_record_name
test_delete_arecords_for_service_label_selector  → correct label selector string
```

#### Step 1.3 — New Helper Functions (GREEN)

In `src/scout.rs`:

```rust
/// Returns true if the Service is a LoadBalancer type.
pub fn is_loadbalancer_service(svc: &Service) -> bool

/// Extracts the first non-empty IP from Service LoadBalancer status.
pub fn resolve_ip_from_service_lb_status(svc: &Service) -> Option<String>

/// Derives the ARecord CR name for a Service.
/// Format: scout-{cluster}-{namespace}-{service-name}
pub fn service_arecord_cr_name(cluster: &str, namespace: &str, service_name: &str) -> String

/// Builds the ARecord spec for a Service.
pub fn build_service_arecord(
    cluster: &str,
    namespace: &str,
    service_name: &str,
    zone: &str,
    ips: Vec<String>,
    ttl: Option<i32>,
) -> Result<ARecord, ScoutError>
```

#### Step 1.4 — Finalizer Cleanup

```rust
/// Deletes all ARecords created by Scout for a given Service.
async fn delete_arecords_for_service(
    remote_client: &Client,
    target_namespace: &str,
    cluster: &str,
    svc_namespace: &str,
    svc_name: &str,
) -> Result<(), ScoutError>
```

#### Step 1.5 — Service Reconcile Function

Mirror the Ingress reconcile function structure:

```rust
async fn reconcile_service(
    svc: Arc<Service>,
    ctx: Arc<ScoutContext>,
) -> Result<Action, ScoutError>
```

Key logic:

```
1. Skip if namespace is excluded
2. On deletion → delete_arecords_for_service + remove finalizer
3. Skip if not scout-enabled annotation
4. Skip if not LoadBalancer type (no warn)
5. Add finalizer
6. Resolve zone (annotation → default_zone → error)
7. Validate zone exists in zone_store
8. Resolve IPs (annotation → default_ips → LB status)
   └── None → warn!(...) + requeue(SCOUT_ERROR_REQUEUE_SECS)
9. build_service_arecord(...)
10. Server-side apply to target_namespace
```

#### Step 1.6 — Error Policy

Reuse the existing `error_policy` function — no changes needed.

### Phase 2 — Wire the Service Controller

In `run_scout` (`src/scout.rs`):

**Add Service watcher alongside the Ingress controller:**

```rust
// Existing Ingress controller
let ingress_controller = Controller::new(ingress_api, Config::default())
    .run(reconcile_ingress, error_policy, ctx.clone())
    ...;

// NEW: Service controller
let svc_api: Api<Service> = Api::all(ctx.client.clone());
let service_controller = Controller::new(svc_api, Config::default())
    .run(reconcile_service, error_policy, ctx.clone())
    ...;

// Drive both concurrently
futures::future::join(
    ingress_controller.for_each(|_| futures::future::ready(())),
    service_controller.for_each(|_| futures::future::ready(())),
).await;
```

### Phase 3 — RBAC Updates (MUST SYNC ALL FILES)

> ⚠️ Per CLAUDE.md: Any RBAC change in `bootstrap.rs` MUST be mirrored in `deploy/scout/clusterrole.yaml`, `deploy/scout.yaml`, and `docs/src/guide/scout.md`.

Add to the Scout ClusterRole:

```yaml
# Watch Services for external IP → ARecord automation
- apiGroups: [""]
  resources: ["services"]
  verbs: ["get", "list", "watch", "patch", "update"]
# Finalizers subresource for forward-compatibility
- apiGroups: [""]
  resources: ["services/finalizers"]
  verbs: ["update"]
```

**Files to update in lockstep:**

| File | Change |
|---|---|
| `src/bootstrap.rs` → `build_scout_cluster_role()` | Add `services` and `services/finalizers` rules |
| `deploy/scout/clusterrole.yaml` | Mirror the above |
| `deploy/scout.yaml` | Mirror the above (inline ClusterRole section) |
| `docs/src/guide/scout.md` | Update ClusterRole YAML example |

### Phase 4 — Documentation Updates

- `docs/src/guide/scout.md`: Add "Service watching" section explaining:
  - How to annotate a Service
  - Difference from Ingress (single record, service name as hostname)
  - The "no external IP → requeue" behaviour
  - Example annotated Service YAML
- `docs/src/installation/scout.md`: Mention Service support in feature list
- `README.md`: Update feature list if referenced there

### Phase 5 — Bootstrap Deployment (No Change Needed)

No changes to CLI args or env vars. Service watching is automatic once the RBAC is updated — Scout will start watching Services when the controller starts.

---

## File Change Summary

| File | Change Type |
|---|---|
| `src/scout.rs` | New constant, 4 new helper functions, new reconcile function, wire controller |
| `src/scout_tests.rs` | 9+ new unit tests (TDD first) |
| `src/bootstrap.rs` | Update `build_scout_cluster_role` — add services rules |
| `deploy/scout/clusterrole.yaml` | Add services + services/finalizers rules |
| `deploy/scout.yaml` | Add services + services/finalizers rules (inline) |
| `docs/src/guide/scout.md` | Service watching section + updated ClusterRole example |

---

## Key Design Decisions

### Why service name (not `spec.loadBalancerIP`) as hostname?

`spec.loadBalancerIP` is deprecated in Kubernetes 1.24+ and unsupported on most cloud providers. The service name is the stable, human-readable identifier that maps naturally to a DNS hostname.

### Why skip non-LoadBalancer services silently?

`ClusterIP` and `NodePort` services are intra-cluster constructs with no routable external IP. Warning on every such service would flood logs. Silence is correct here — only opted-in `LoadBalancer` services are relevant.

### Why requeue on missing IP instead of creating an ARecord with no addresses?

An ARecord with `ipv4_addresses: []` is invalid and would likely cause BIND9 to reject it. Re-queuing is the correct Kubernetes pattern — eventually-consistent reconciliation handles the race between Service creation and IP assignment by the cloud provider.

### Why the same finalizer name as Ingress?

The finalizer is scoped to the resource it's placed on (`Service` or `Ingress`), so sharing the name is safe and keeps the implementation consistent. Scout identifies ARecords to clean up via label selectors, not the finalizer name.

---

## Out of Scope (Future Work)

- `ExternalName` service type → CNAME records (requires CnameRecord CRD)
- `NodePort` services with explicit `bindy.firestoned.io/ip` annotation (rare use case)
- Multiple IPs per service (round-robin) — would require iterating `status.loadBalancer.ingress[]` fully (currently uses first non-empty; consistent with Ingress behaviour)
