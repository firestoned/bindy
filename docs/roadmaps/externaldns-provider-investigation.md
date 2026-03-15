# ExternalDNS Provider / Plugin for Bindy — Investigation Roadmap

**Date:** 2026-03-10
**Status:** Investigation
**Author:** Erick Bourgeois
**Impact:** New integration capability — no breaking changes to existing CRDs

---

## Overview

This roadmap investigates adding an **ExternalDNS provider** for Bindy, enabling ExternalDNS to automatically manage DNS records and sub-zones inside BIND9 clusters controlled by Bindy.

### Primary Use Case

In a **k0rdent multi-cluster environment**:

1. k0rdent provisions **child clusters**
2. **k0smotron** creates `LoadBalancer` service endpoints for each child cluster's control plane and workload services
3. The operator wants DNS sub-zones auto-created per child cluster (e.g., `prod-cluster1.example.com`) with NS delegation from the parent zone
4. ExternalDNS watches the k0smotron `LoadBalancer` services and automatically creates `A`/`AAAA` records inside those sub-zones

### Problems to Solve

There are **two distinct but related problems**:

| Problem | Description | Mechanism |
|---------|-------------|-----------|
| **Record management** | ExternalDNS creates/deletes `A`, `AAAA`, `CNAME`, `TXT` records in Bindy zones as services appear/disappear | ExternalDNS Webhook Provider API |
| **Zone provisioning** | When a k0rdent child cluster is created, automatically create a `DNSZone` sub-zone and add `NS` delegation in the parent zone | New Bindy reconciler watching k0rdent CRDs |

---

## Background

### ExternalDNS Webhook Provider (v0.14+)

ExternalDNS v0.14 introduced an **external webhook provider** mechanism:

- ExternalDNS runs a sidecar container alongside the webhook provider binary
- ExternalDNS communicates with the provider over `localhost` HTTP (no external exposure needed)
- The provider implements a small REST API that ExternalDNS calls to list, apply, and adjust records
- This eliminates the need to fork ExternalDNS or submit upstream provider code

Reference implementations exist for:
- Cloudflare (built-in)
- Azure Private DNS
- Unbound DNS (community)

**Spec:** [ExternalDNS Webhook Provider Spec](https://github.com/kubernetes-sigs/external-dns/blob/master/docs/tutorials/webhook-provider.md)

### Bindy's Relevant CRDs

| CRD | Purpose |
|-----|---------|
| `DNSZone` | Authoritative zone (e.g., `cluster1.example.com`) |
| `ARecord` | IPv4 address record |
| `AAAARecord` | IPv6 address record |
| `CNAMERecord` | Alias record |
| `TXTRecord` | TXT record (ExternalDNS uses these for ownership tracking) |
| `NSRecord` | Nameserver delegation (for sub-zone NS records in parent zone) |

### k0rdent / k0smotron Resources (To Investigate)

- **k0rdent**: Manages multi-cluster lifecycle. Need to identify the CRD that represents a provisioned child cluster and its status
- **k0smotron**: Creates hosted control planes with `LoadBalancer` services. The `LoadBalancer` `.status.loadBalancer.ingress` field contains the IPs/hostnames ExternalDNS reads

---

## Architecture

### Option A: Webhook Provider Only (Record Management)

```
┌─────────────────────────────────┐
│  ExternalDNS Pod                │
│  ┌─────────────┐  HTTP localhost│
│  │ externaldns │◄───────────────┼──► Bindy Webhook Provider
│  └─────────────┘                │     (new binary/container)
└─────────────────────────────────┘
                                        │
                                        │ creates/deletes
                                        ▼
                               Bindy CRDs (ARecord,
                               TXTRecord, CNAMERecord...)
                                        │
                                        │ reconciled by
                                        ▼
                               BIND9 Server (bindcar)
```

**Scope:** ExternalDNS manages `A`, `AAAA`, `CNAME`, `TXT` records inside **existing** Bindy `DNSZone` resources.

**Limitation:** Zones must be pre-created. Does not handle sub-zone provisioning.

### Option B: Webhook Provider + Zone Provisioner (Full Solution)

Adds a **new Bindy reconciler** that watches k0rdent `ClusterDeployment` (or equivalent) resources and:

1. Creates a `DNSZone` for each child cluster (e.g., `cluster1.example.com`)
2. Creates `NSRecord` entries in the parent zone delegating to that sub-zone's nameservers
3. Tears down zone and NS records when the cluster is deleted

```
k0rdent ClusterDeployment ──watch──► Zone Provisioner Reconciler
                                              │
                                    ┌─────────┴────────┐
                                    │                  │
                               create DNSZone     create NSRecord
                               cluster1.example   in parent zone
                               .com               example.com
                                    │
                                    ▼
                         ExternalDNS Webhook Provider
                         (populates A/AAAA records for
                          k0smotron LoadBalancer IPs
                          inside cluster1.example.com)
```

**Recommended:** Option B provides the full automation story for k0rdent.

---

## Investigation Phases

### Phase 1: Research & Specification Review

**Goal:** Fully understand all external APIs before writing any code.

#### 1.1 ExternalDNS Webhook Provider Spec

- [ ] Read the [ExternalDNS webhook provider spec](https://github.com/kubernetes-sigs/external-dns/blob/master/docs/tutorials/webhook-provider.md)
- [ ] Identify all required HTTP endpoints:
  - `GET /` — provider health / domain filters
  - `GET /records` — list all DNS records
  - `POST /records` — apply record changes (upsert + delete batch)
  - `POST /adjustendpoints` — normalize endpoints before apply
- [ ] Understand the `Endpoint` struct ExternalDNS sends/receives
- [ ] Understand the `DomainFilter` mechanism (which zones does this provider manage?)
- [ ] Review ownership semantics: ExternalDNS uses `TXT` records for ownership — how does this translate to Bindy's `TXTRecord` CRD?
- [ ] Review an existing webhook provider (e.g., [external-dns-hetzner-webhook](https://github.com/mconfalonieri/external-dns-hetzner-webhook)) for implementation patterns

#### 1.2 k0rdent / k0smotron API Research

- [ ] Identify the k0rdent CRD that represents a provisioned child cluster:
  - Likely `ClusterDeployment` or similar in `k0rdent.io` API group
  - Check if it has a `status.ready` condition or similar lifecycle state
- [ ] Identify the k0smotron CRD for hosted control planes and associated `LoadBalancer` services:
  - Likely `K0smotronControlPlane` or `Machine` resources
  - Locate how to get the control plane endpoint (`LoadBalancer.status.loadBalancer.ingress`)
- [ ] Determine the naming convention for sub-zones:
  - `<cluster-name>.<parent-zone>` — e.g., `cluster1.example.com`
  - Or `<cluster-name>-<namespace>.<parent-zone>`
- [ ] Review k0rdent cluster deletion lifecycle — are there finalizers that guarantee DNS cleanup before cluster resources disappear?

#### 1.3 Bindy CRD Capability Assessment

- [ ] Confirm `NSRecord` CRD supports all fields needed for sub-zone delegation:
  - Nameserver FQDN(s) pointing to the Bindy BIND9 instance
  - Proper TTL
- [ ] Confirm `TXTRecord` CRD can hold the multi-value TXT content ExternalDNS emits for ownership tracking (e.g., `"heritage=external-dns,external-dns/owner=default"`)
- [ ] Verify record deletion flow: when ExternalDNS removes a record, does deleting the Bindy CRD correctly propagate to BIND9?
- [ ] Assess whether a new CRD (e.g., `ExternalDNSProvider`) is needed to configure the webhook provider, or if `ClusterBind9Provider` + environment variables suffice

---

### Phase 2: Architecture Design

**Goal:** Produce an architecture decision document before writing any code.

#### 2.1 Webhook Provider Design Decisions

- [ ] **Deployment model**: Run as:
  - A sidecar container in the ExternalDNS pod (standard pattern), OR
  - A separate `Deployment` in the cluster with a `Service` ExternalDNS calls
  - *Recommendation*: Sidecar on localhost — simpler, no RBAC exposure
- [ ] **Binary location**: Add as a new binary in `src/bin/externaldns-webhook.rs` inside Bindy, or as a separate repo?
  - *Recommendation*: Separate binary in Bindy — shares CRD types, no duplication
- [ ] **Authentication**: How does the webhook binary authenticate to the Kubernetes API to create Bindy CRDs?
  - Standard ServiceAccount + RBAC (same as main operator)
- [ ] **Namespace scope**: Which namespace are the generated `ARecord`/`TXTRecord` CRDs created in?
  - Same namespace as the `DNSZone`? Requires ExternalDNS awareness of namespace
  - Fixed operator namespace? Simpler but less flexible
- [ ] **Label strategy**: What labels does the webhook put on generated records so Bindy `DNSZone.spec.recordsFrom` label selectors can discover them?
  - Proposed: `externaldns.alpha.kubernetes.io/owner=<owner-id>`, `bindy.firestoned.io/zone=<zone-name>`

#### 2.2 Zone Provisioner Design Decisions

- [ ] **Watch target**: Which k0rdent CRD to watch? Confirm CRD group/version/kind
- [ ] **Parent zone reference**: How does the provisioner know which `DNSZone` is the parent?
  - Annotation on the k0rdent `ClusterDeployment`? e.g., `bindy.firestoned.io/parent-zone=example.com`
  - Or a `Bind9ZoneProvisioner` CRD that maps cluster label selectors to parent zones?
- [ ] **Nameserver IPs**: The `NSRecord` in the parent zone needs to point to the Bindy BIND9 server's IPs. How are these discovered?
  - From `Bind9Instance.status`?
  - From a `Service` associated with the `Bind9Cluster`?
- [ ] **Finalizer strategy**: The provisioner must add a finalizer to the k0rdent `ClusterDeployment` to ensure DNS cleanup before the cluster resource is deleted

#### 2.3 Write ADR

- [ ] Document architecture decisions in `docs/adr/0005-externaldns-webhook-provider.md`

---

### Phase 3: ExternalDNS Webhook Provider Implementation

**Goal:** Implement the HTTP webhook server that ExternalDNS calls.

#### 3.1 New Binary Setup

- [ ] Create `src/bin/externaldns_webhook.rs`
- [ ] Add Axum HTTP server with required routes:
  - `GET /` — return `DomainFilter` (zones this provider manages)
  - `GET /records` — list all Bindy-managed records as ExternalDNS `Endpoint` structs
  - `POST /records` — receive `Changes` struct, translate to CRD create/delete
  - `POST /adjustendpoints` — normalize `Endpoint` list (can be a passthrough initially)
- [ ] Add health/readiness endpoints for Kubernetes probes

#### 3.2 Type Definitions

- [ ] Define Rust structs matching ExternalDNS webhook JSON schema:
  - `Endpoint` — target, record type, TTL, labels, provider-specific config
  - `Changes` — `Create`, `UpdateOld`, `UpdateNew`, `Delete` lists of endpoints
  - `DomainFilter` — list of zones this provider manages

#### 3.3 Record Translation Layer

- [ ] `A` endpoint → create/delete `ARecord` CRD
- [ ] `AAAA` endpoint → create/delete `AAAARecord` CRD
- [ ] `CNAME` endpoint → create/delete `CNAMERecord` CRD
- [ ] `TXT` endpoint → create/delete `TXTRecord` CRD
- [ ] `NS` endpoint → create/delete `NSRecord` CRD (needed for sub-zone delegation)
- [ ] Handle name normalization: ExternalDNS uses FQDNs, Bindy records use relative names within a zone

#### 3.4 List Records Implementation

- [ ] List all Bindy record CRDs with `externaldns.alpha.kubernetes.io/managed=true` label
- [ ] Translate each CRD to an `Endpoint` struct
- [ ] Filter by `DomainFilter` to return only records in managed zones

#### 3.5 Apply Changes Implementation

- [ ] Process `Changes.Create` — create corresponding CRDs
- [ ] Process `Changes.Delete` — delete corresponding CRDs by name/type/content
- [ ] Process `Changes.UpdateNew` / `UpdateOld` — delete old CRD, create new CRD (or patch)
- [ ] Ensure `TXTRecord` ownership records are managed correctly

#### 3.6 TDD Test Suite

- [ ] Unit tests for all translation functions (endpoint ↔ CRD)
- [ ] Integration tests using `wiremock` to mock k8s API
- [ ] Test `POST /records` with create, delete, and update scenarios
- [ ] Test `GET /records` returns correct endpoints for existing CRDs

---

### Phase 4: k0rdent Zone Provisioner

**Goal:** Automatically create/delete sub-zones when k0rdent child clusters are created/deleted.

#### 4.1 New Reconciler

- [ ] Create `src/reconcilers/k0rdent_zone_provisioner.rs`
- [ ] Watch k0rdent `ClusterDeployment` resources (or confirmed equivalent CRD)
- [ ] On cluster **creation/ready**:
  1. Create `DNSZone` named `<cluster-name>.<parent-zone>` in the operator namespace
  2. Create `NSRecord` in parent `DNSZone` pointing to Bindy BIND9 nameserver IPs
  3. Add finalizer to `ClusterDeployment`
- [ ] On cluster **deletion** (via finalizer):
  1. Delete `NSRecord` from parent zone
  2. Delete the child `DNSZone` (and all records within it — verify cascade behavior)
  3. Remove finalizer from `ClusterDeployment`

#### 4.2 Configuration CRD (if needed)

Evaluate if a new CRD is required to configure the zone provisioner:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: K0rdentZoneProvisioner
metadata:
  name: default
spec:
  parentZone: example.com              # Parent DNSZone name
  clusterSelector:                     # Which ClusterDeployments to watch
    matchLabels:
      bindy.firestoned.io/provision-zone: "true"
  nameservers:                         # Bind9Instance providing nameservice
    bind9InstanceRef: my-bind9-instance
```

- [ ] Decide if annotation-based configuration (no new CRD) is sufficient for initial implementation
- [ ] If new CRD is needed, add to `src/crd.rs` and regenerate YAMLs

#### 4.3 TDD Test Suite

- [ ] Test sub-zone `DNSZone` is created when `ClusterDeployment` becomes ready
- [ ] Test `NSRecord` is created in parent zone
- [ ] Test finalizer is added to `ClusterDeployment`
- [ ] Test cleanup: `NSRecord` and `DNSZone` deleted when `ClusterDeployment` is deleted
- [ ] Test idempotency: re-reconcile does not create duplicate zones

---

### Phase 5: Integration Testing

**Goal:** Validate end-to-end flow in a real cluster.

- [ ] Deploy Bindy with a `Bind9Cluster` and parent `DNSZone` (e.g., `example.com`)
- [ ] Deploy ExternalDNS with the Bindy webhook provider sidecar
- [ ] Create a k0smotron `LoadBalancer` service
- [ ] Verify ExternalDNS discovers the `LoadBalancer` IP and calls the webhook provider
- [ ] Verify an `ARecord` and `TXTRecord` CRD are created in the correct `DNSZone`
- [ ] Verify BIND9 returns the correct DNS response via `dig`
- [ ] Simulate k0rdent child cluster creation via a mock `ClusterDeployment` resource
- [ ] Verify sub-zone `DNSZone` and `NSRecord` are created
- [ ] Delete the `ClusterDeployment` and verify DNS cleanup
- [ ] Test NXDOMAIN after cleanup

---

### Phase 6: Documentation & Packaging

- [ ] Write tutorial: `docs/src/integrations/externaldns.md`
  - Installation steps
  - ExternalDNS Deployment YAML with webhook sidecar
  - Required RBAC
  - Configuration reference
  - k0rdent zone provisioner setup
- [ ] Write Helm chart values for deploying ExternalDNS + Bindy webhook provider together
- [ ] Update `README.md` feature list to include ExternalDNS integration
- [ ] Add Dockerfile for webhook provider binary (separate image or same multi-binary image?)
- [ ] Add GitHub Action workflow for building and publishing the webhook provider image

---

## Key Questions to Answer During Investigation

1. **Does ExternalDNS support namespace-scoped record creation?** By default ExternalDNS is cluster-scoped. If Bindy records are namespace-scoped CRDs, the webhook provider needs a namespace target strategy.

2. **How does ExternalDNS handle zone ownership?** ExternalDNS uses `TXT` records with `heritage=external-dns` to mark records it owns. The webhook provider must implement this correctly to avoid ExternalDNS repeatedly re-creating records.

3. **What is the k0rdent CRD for a child cluster?** This is the trigger for the zone provisioner. Needs investigation against a running k0rdent environment or the k0rdent API documentation.

4. **Does k0smotron expose a stable DNS hostname or only IPs?** `CNAME` records (if hostname) vs `A` records (if IP) — affects ExternalDNS `--target-annotation` behavior.

5. **Can a single Bindy `TXTRecord` hold multiple values?** ExternalDNS emits multi-value TXT records for ownership. Verify the current `TXTRecord` CRD spec supports this.

6. **Conflict with existing Bindy record reconciliation?** If ExternalDNS creates `ARecord` CRDs and Bindy's own reconciler also manages `ARecord` CRDs in the same zone, there could be conflicts. Need to define clear ownership boundaries (e.g., via labels).

7. **Should the webhook provider be a separate binary or a separate repo?** A separate repo allows independent versioning aligned with ExternalDNS release cadence. A single repo simplifies CRD type sharing. Investigate what works best for the k0rdent ecosystem.

---

## Open Architecture Questions

- Should the webhook provider use a `ClusterBind9Provider` reference for cluster-scoped record management, or `Bind9Cluster` references for namespace-scoped management?
- Is a dedicated `ExternalDNSEndpoint` CRD better than reusing existing `ARecord`/`TXTRecord` CRDs? (Avoids conflicts with manually managed records but adds CRD complexity)
- For the zone provisioner, should it live in Bindy or in a separate k0rdent-specific operator/addon?

---

## Success Criteria

- [ ] ExternalDNS can manage `A`, `AAAA`, `CNAME`, `TXT` records in Bindy-managed BIND9 zones without manual CRD creation
- [ ] k0rdent child cluster creation automatically provisions a sub-zone with correct NS delegation
- [ ] k0rdent child cluster deletion removes all DNS entries with no manual cleanup
- [ ] k0smotron `LoadBalancer` IPs are resolvable via DNS within 60 seconds of service creation
- [ ] All operations are idempotent
- [ ] Full test coverage following Bindy TDD standards
- [ ] Documentation sufficient for an operator unfamiliar with Bindy internals

---

## References

- [ExternalDNS Webhook Provider Tutorial](https://github.com/kubernetes-sigs/external-dns/blob/master/docs/tutorials/webhook-provider.md)
- [ExternalDNS Webhook Provider Spec (API)](https://github.com/kubernetes-sigs/external-dns/blob/master/pkg/apis/externaldns/types.go)
- [Example: Hetzner Webhook Provider](https://github.com/mconfalonieri/external-dns-hetzner-webhook)
- [Example: Unbound Webhook Provider](https://github.com/MacroPower/external-dns-unbound-webhook)
- k0rdent documentation (internal)
- k0smotron documentation (internal)
- [RFC 2136 — Dynamic Updates in DNS](https://datatracker.ietf.org/doc/html/rfc2136)
- [bindcar crate](https://crates.io/crates/bindcar) — Bindy's BIND9 HTTP API client
