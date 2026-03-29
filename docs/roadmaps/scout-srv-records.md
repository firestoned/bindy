# Scout: SRV Record Support for Services and Ingresses

**Status:** Proposed
**Date:** 2026-03-27
**Author:** Erick Bourgeois

---

## Overview

Scout currently creates `ARecord` CRs from annotated `LoadBalancer` Services and `Ingress` resources. This roadmap extends Scout to also create `SRVRecord` CRs, using the port and protocol information already available on both resource types.

SRV records allow service discovery clients (gRPC, Linkerd, custom resolvers) to locate services by service name and protocol without knowing ports in advance — a natural fit for a platform engineering toolchain.

---

## Goals

- Auto-create `SRVRecord` CRs from `Service.spec.ports` for `LoadBalancer` services
- Auto-create `SRVRecord` CRs from `Ingress` resources (HTTP port 80, HTTPS port 443 when TLS is present)
- Reuse the existing `SRVRecord` CRD and reconciler — no new DNS plumbing required
- Maintain parity with existing ARecord behavior: finalizer lifecycle, label-based cleanup, annotation opt-in

## Non-Goals

- `NodePort` or `ClusterIP` services — ARecord support is already excluded for these; SRV follows the same restriction
- `ExternalName` services
- Custom priority/weight per port (annotation overrides are a stretch goal, not in scope here)
- SRV records from `EndpointSlice` targets (pod-level SRV — a separate, more complex feature)

---

## Background

### Existing infrastructure

The `SRVRecord` CRD, BIND9 integration, and reconciler already exist and are production-ready:

| Component | Location |
|-----------|----------|
| `SRVRecordSpec` CRD type | `src/crd.rs:1576` |
| SRV reconciler | `src/reconcilers/records/mod.rs:1684` |
| BIND9 SRV writer | `src/bind9/records/srv.rs` |

Scout only needs to create `SRVRecord` CRs — the downstream pipeline handles the rest.

### SRV record format (RFC 2782)

```
_service._proto.name TTL IN SRV priority weight port target.
```

Example:
```
_grpc._tcp.my-api.example.com. 300 IN SRV 0 100 9090 my-api.example.com.
```

Fields:
- `_service` — service/port name (e.g. `_http`, `_grpc`, `_postgres`)
- `_proto` — `_tcp`, `_udp`, or `_sctp` (from `Service.spec.ports[].protocol`)
- `priority` — lower wins; default `0`
- `weight` — load distribution; default `100`
- `port` — service port from `Service.spec.ports[].port`
- `target` — FQDN of the host (the ARecord already created by Scout)

---

## Design

### SRV name derivation

The `_service` component in the SRV name is derived from the **port name** when available, falling back to the service/ingress name:

| Source | Port name present | `_service` component |
|--------|-------------------|----------------------|
| Service | yes | `_<port-name>` (e.g. `_grpc`, `_http`) |
| Service | no (unnamed port) | `_<service-name>` |
| Ingress | HTTP rule present | `_http` |
| Ingress | TLS spec present | `_https` |

The `_proto` component comes from `Service.spec.ports[].protocol`, lowercased (`TCP` → `_tcp`).

**Full SRV record name format:**
```
_<port-name>._<protocol>.<zone>
```

For a Service named `my-api` in zone `example.com`:
- Port `{name: "grpc", protocol: "TCP", port: 9090}` → `_grpc._tcp.example.com`
- Port `{name: "http", protocol: "TCP", port: 80}` → `_http._tcp.example.com`
- Port `{protocol: "UDP", port: 5353}` (unnamed) → `_my-api._udp.example.com`

### Target FQDN

The SRV `target` field must point to a hostname that resolves to an IP (i.e. the ARecord Scout already creates). The target is the fully-qualified record name:

```
<service-name>.<zone>.    (note trailing dot — required by RFC 2782)
```

Example: service `my-api` in zone `example.com` → target `my-api.example.com.`

This means **ARecord creation is a prerequisite** for SRV. When Scout processes a Service it should create the ARecord first, then the SRV records.

### Multiple ports

A Service with N ports produces N SRV records. Each SRV record is managed independently with its own finalizer-tracked CR name.

**SRV CR naming convention:**
```
scout-{cluster}-{namespace}-{service-name}-{port-name}
```

Examples:
- `scout-prod-default-my-api-grpc`
- `scout-prod-default-my-api-http`
- `scout-prod-default-my-api-unnamed-9090` (fallback for unnamed ports)

### Ingress → SRV

Ingress resources don't have explicit port lists on the object, but the ports are deterministic:

| Condition | Port | `_service._proto` |
|-----------|------|-------------------|
| Any Ingress rule with HTTP | 80 | `_http._tcp` |
| TLS stanza present | 443 | `_https._tcp` |

**Target**: the ingress hostname (first rule host), or the Scout-created ARecord name if no host is set.

Ingresses with multiple rules (multiple hostnames) produce one SRV record per unique host-port combination.

### Annotation controls

Following the existing pattern:

| Annotation | Effect |
|------------|--------|
| `bindy.firestoned.io/scout-srv: "false"` | Opt-out of SRV creation for this resource (still creates ARecord) |
| `bindy.firestoned.io/scout-srv-priority: "10"` | Override SRV priority (default `0`) |
| `bindy.firestoned.io/scout-srv-weight: "50"` | Override SRV weight (default `100`) |

SRV creation follows the same opt-in gate as ARecord (`bindy.firestoned.io/scout-enabled: "true"` is still required).

### Defaults

| Field | Default | Notes |
|-------|---------|-------|
| `priority` | `0` | Highest priority; reasonable for single-cluster |
| `weight` | `100` | Standard default |
| `ttl` | Inherited from ARecord TTL annotation or zone default | |

---

## Implementation Plan

### Phase 1 — Service → SRV

1. **Add helpers** in `src/scout.rs`:
   - `srv_record_name_for_service_port(cluster, namespace, service_name, port_name_or_fallback) -> String`
   - `srv_label_selector_for_service(cluster, namespace, service_name) -> BTreeMap`
   - `derive_srv_service_component(port: &ServicePort, service_name: &str) -> String`
   - `derive_proto_component(protocol: &str) -> String`
   - `build_service_srv_record(params: ServiceSRVRecordParams) -> SRVRecord`

2. **Extend `reconcile_service`** in `src/scout.rs`:
   - After successful ARecord apply, iterate `service.spec.ports`
   - Skip ports with `protocol == "SCTP"` for now (uncommon, no SRV standard)
   - Build and apply one `SRVRecord` CR per port
   - Add label `bindy.firestoned.io/source-port-name` for targeted cleanup

3. **Extend `delete_arecords_for_service`** (or add `delete_srv_records_for_service`):
   - Delete SRV CRs via label selector on finalizer cleanup

4. **Write tests** in `src/scout_tests.rs` (TDD — write first):
   - `test_derive_srv_service_component_uses_port_name_when_present`
   - `test_derive_srv_service_component_falls_back_to_service_name`
   - `test_derive_proto_component_lowercases_protocol`
   - `test_build_service_srv_record_sets_correct_target_fqdn`
   - `test_reconcile_service_creates_srv_per_port`
   - `test_reconcile_service_skips_srv_when_annotation_false`
   - `test_srv_cr_name_is_stable_and_unique`

### Phase 2 — Ingress → SRV

1. **Add helpers**:
   - `srv_records_for_ingress(ingress, zone, cluster, defaults) -> Vec<SRVRecord>`
   - Detect HTTP rules: if `ingress.spec.rules` non-empty → port 80, `_http._tcp`
   - Detect HTTPS: if `ingress.spec.tls` non-empty → port 443, `_https._tcp`

2. **Extend `reconcile_ingress`** in `src/scout.rs`:
   - After ARecord apply, build and apply SRV CRs
   - One SRV per (host × port) combination

3. **Extend ingress finalizer cleanup** to also delete SRV CRs.

4. **Write tests** (TDD — write first):
   - `test_ingress_with_no_tls_creates_only_http_srv`
   - `test_ingress_with_tls_creates_both_http_and_https_srv`
   - `test_ingress_srv_target_is_ingress_arecord_fqdn`

### Phase 3 — RBAC and deploy sync

Per the RBAC sync rule in CLAUDE.md, update all of:
- `src/bootstrap.rs` — verify `SRVRecord` create/patch/delete is in Scout ClusterRole (likely already present — verify)
- `deploy/scout/clusterrole.yaml` — mirror
- `deploy/scout.yaml` — mirror
- `docs/src/guide/scout.md` — document new behavior and annotations

---

## Open Questions

1. **Unnamed ports**: Should Scout silently skip unnamed ports (safest), warn and skip, or use the service name as the fallback `_service` component? Recommendation: use service name as fallback so no ports are silently ignored.

2. **Port collisions**: Two services in the same zone with the same port name (e.g. both expose `_http._tcp.example.com`) will produce conflicting SRV records. Should Scout detect and warn? Recommend: last-writer-wins (SSA semantics handle this), log a warning.

3. **SCTP**: RFC 2782 technically supports `_sctp` but it's vanishingly rare on k8s. Skip for now; add later if requested.

4. **Priority/weight per-port**: The annotation approach gives per-resource overrides but not per-port. If needed in future, a JSON annotation (`bindy.firestoned.io/scout-srv-ports: '[{"name":"grpc","priority":5}]'`) could extend this.

---

## Testing Plan

- Unit tests (TDD — written before implementation): covered in Phase 1 and 2 above
- Integration test: deploy a multi-port LoadBalancer Service, verify SRV CRs created with correct name/port/target; delete Service, verify SRV CRs cleaned up
- Integration test: deploy Ingress with TLS, verify two SRV CRs (`_http._tcp`, `_https._tcp`); update to remove TLS, verify `_https._tcp` SRV deleted
