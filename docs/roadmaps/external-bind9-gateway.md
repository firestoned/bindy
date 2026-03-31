# External Bind9 Gateway — Roadmap

**Status:** Draft
**Created:** 2026-03-30
**Author:** Erick Bourgeois

---

## Problem Statement

bindy currently assumes bind9 and bindcar run as pods inside the Kubernetes cluster. With bindcar now capable of running outside the cluster, we need a path to support bind9 instances on bare-metal or VMs — without requiring those machines to be Kubernetes nodes or have direct cluster network access.

The core challenge: bindy needs to reach the bindcar HTTP API (port 8080) and rndc (port 953) on a machine that may be:
- Behind NAT
- On an isolated network segment
- A legacy server that cannot be moved into Kubernetes

Security requirements remain unchanged: all communication must be encrypted (TLS 1.3 strictly), mutually authenticated, and auditable.

---

## Goals

- Allow bind9 + bindcar to run on **bare-metal or VMs** and be managed by bindy exactly like in-cluster instances
- bind9 **must only listen on 127.0.0.1** — no external exposure of port 53 or 953 on the remote host
- All traffic between cluster and remote host **must use TLS 1.3** with mutual authentication (mTLS or Noise_KK)
- The remote host **initiates the outbound connection** (reverse tunnel) so no inbound firewall rules are required on the remote side
- bindy's reconcilers see an **external instance identically to an in-cluster one** — no special-casing in reconciler logic
- Zero new proprietary dependencies

---

## Proposed Architecture

```
┌─────────────────────────────────────────────────────┐
│  Kubernetes Cluster                                  │
│                                                      │
│  ┌──────────────────────────────────────────────┐   │
│  │  Bind9Gateway Pod (new "meta pod")            │   │
│  │                                               │   │
│  │  ┌─────────────────┐  ┌────────────────────┐ │   │
│  │  │  rathole-server  │  │  Service endpoint  │ │   │
│  │  │  :2333 (TLS 1.3) │  │  :8080 → bindcar  │ │   │
│  │  │  :8080 exposed   │  │  :953  → rndc      │ │   │
│  │  └────────┬─────────┘  └────────────────────┘ │   │
│  └───────────┼────────────────────────────────────┘   │
│              │  bindy talks to this Service            │
└──────────────┼──────────────────────────────────────┘
               │
               │  TLS 1.3 / Noise_KK reverse tunnel
               │  (remote initiates outbound)
               │
┌──────────────┼──────────────────────────────────────┐
│  Bare-Metal / VM                                     │
│              │                                       │
│  ┌───────────┴──────────────────────────────────┐   │
│  │  rathole-client (new "bind9gateway-agent")    │   │
│  │  Exposes: 127.0.0.1:8080 (bindcar)           │   │
│  │           127.0.0.1:953  (rndc)               │   │
│  └───────────────────────────────────────────────┘   │
│                                                      │
│  ┌──────────────┐   ┌────────────────────────────┐  │
│  │  bind9        │   │  bindcar                   │  │
│  │  127.0.0.1:53 │   │  127.0.0.1:8080            │  │
│  │  127.0.0.1:953│   │  (HTTP API → rndc → bind9) │  │
│  └──────────────┘   └────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### Data flow (bindy → remote bind9)

1. bindy reconciler issues HTTP request to bindcar API (e.g., `POST /zones`)
2. Request hits the `Bind9Gateway` Service in the cluster
3. Service routes to the rathole-server container in the gateway pod
4. rathole-server forwards over the established reverse tunnel (TLS 1.3)
5. rathole-client on the remote host receives the request and forwards to `127.0.0.1:8080` (bindcar)
6. bindcar executes rndc commands against `127.0.0.1:953` (bind9)
7. Response travels back through the same tunnel

---

## Tunnel Solution: rathole

After evaluating rathole, frp, inlets-pro, chisel, bore, stunnel, and socat:

**Selected: [rathole](https://github.com/rapiz1/rathole)** (Apache 2.0, Rust)

| Criterion | rathole | Notes |
|-----------|---------|-------|
| TLS 1.3 | ✅ via `rustls` feature flag | Strict, no downgrade |
| Mutual auth | ✅ `Noise_KK_25519_ChaChaPoly_BLAKE2s` | Or mTLS with X.509 certs |
| Language | Rust | Aligns with this project |
| License | Apache 2.0 | No copyleft concerns |
| Reverse tunnel | ✅ | Remote initiates; no inbound firewall rules needed |
| Binary size | ~2MB | Suitable as sidecar |
| Hot-reload | ✅ | Config changes without restart |
| Kubernetes-native | ❌ | We provide this layer via the CRD |

Rejected alternatives:
- **frp**: Go, cannot enforce TLS 1.3-only (Go's `crypto/tls` does not allow exclusive TLS 1.3)
- **inlets-pro**: Proprietary license, expensive VM-provisioning model
- **bore**: No TLS — not acceptable
- **stunnel / socat**: GPL v2 copyleft, not reverse-tunnel designs
- **chisel**: Go, TLS 1.3 enforcement gap (same as frp), no Kubernetes integration

### Noise Protocol vs TLS

rathole supports both. For this use case:

- **Noise_KK** (recommended): both sides have static keys known in advance, mutual authentication, no PKI/CA required, forward secrecy. Keys are provisioned as Kubernetes Secrets.
- **TLS 1.3 + mTLS** (alternative): use if integration with an existing PKI (e.g., cert-manager) is required.

The CRD will support both via a `transport` field.

---

## New CRD: `Bind9Gateway`

A `Bind9Gateway` represents the cluster-side half of the tunnel. It is namespace-scoped and creates:
- A `Deployment` running the rathole-server container
- A headless `Service` that bindy reconcilers can address
- A `Secret` reference for the Noise or TLS keys

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Gateway
metadata:
  name: dc1-primary-gateway
  namespace: bindy-system
spec:
  # Tunnel listen configuration
  tunnel:
    listenPort: 2333
    transport: noise-kk          # or: tls
    secretRef:
      name: dc1-gateway-keys     # Kubernetes Secret with noise keys or TLS certs

  # Services exposed through the tunnel (one per remote bind9 instance)
  services:
    - name: bindcar-api
      remoteToken: "dc1-primary" # matches rathole-client token
      exposedPort: 8080           # bindy uses this port to reach bindcar

  # Resource limits for the gateway pod
  resources:
    limits:
      cpu: "100m"
      memory: "64Mi"

status:
  conditions:
    - type: TunnelConnected
      status: "True"
  connectedClients: 1
  lastConnectedAt: "2026-03-30T12:00:00Z"
```

### Bind9Instance extension

`Bind9Instance` gains an optional `gatewayRef` field. When set, the instance is treated as external — no pod is created; bindy routes bindcar calls through the gateway instead.

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dc1-primary
  namespace: bindy-system
spec:
  role: primary
  gatewayRef:
    name: dc1-primary-gateway
    service: bindcar-api
  # bindcarConfig.image is still used — it's what runs on the remote host
  bindcarConfig:
    image: "ghcr.io/firestoned/bindcar:v0.6.0"
```

When `gatewayRef` is present:
- The instance reconciler skips pod/StatefulSet creation
- bindcar API calls are routed to `dc1-primary-gateway-svc:8080` instead of a pod IP
- rndc calls are routed via bindcar (as today — no change to zone_ops.rs)

---

## Remote Host: bind9gateway-agent

A small, standalone binary (or Docker image) the operator deploys on the remote host. It wraps rathole-client with opinionated defaults for the bindy use case.

```
ghcr.io/firestoned/bind9gateway-agent:v0.1.0
```

Configuration (TOML, rathole-compatible):

```toml
[client]
remote_addr = "k8s-cluster-ingress:2333"

[client.transport]
type = "noise"

[client.transport.noise]
pattern = "Noise_KK_25519_ChaChaPoly_BLAKE2s"
local_private_key = "/etc/bind9gateway/client.key"
remote_public_key = "/etc/bind9gateway/server.pub"

[client.services.bindcar-api]
token = "dc1-primary"
local_addr = "127.0.0.1:8080"

[client.services.rndc]
token = "dc1-primary-rndc"
local_addr = "127.0.0.1:953"
```

bind9 named.conf change required on remote host:

```
// Before (unsafe — listens on all interfaces)
listen-on { any; };

// After (loopback only — all external access goes through bindcar/tunnel)
listen-on { 127.0.0.1; };
controls { inet 127.0.0.1 port 953 allow { 127.0.0.1; }; };
```

---

## Security Model

| Threat | Mitigation |
|--------|-----------|
| MITM on tunnel | Noise_KK / TLS 1.3 mTLS — both sides authenticated before any data flows |
| Key exfiltration | Keys stored in Kubernetes Secrets (RBAC-gated); remote side uses file permissions 0600 |
| bind9 port exposure | bind9 listens only on 127.0.0.1 — unreachable without the tunnel agent running |
| Token replay | Noise_KK provides forward secrecy — past sessions cannot be decrypted |
| Unauthorized tunnel client | Server-side token per service; unknown tokens rejected |
| Tunnel disruption (DoS) | rathole reconnects with exponential backoff; Bind9Gateway status reflects disconnect; bindy pauses reconciliation for disconnected gateways |

### Key provisioning

A `bind9gateway keygen` subcommand generates the Noise_KK keypair:

```bash
bind9gateway-agent keygen \
  --out-secret k8s-secret-dc1-gateway-keys.yaml \
  --out-client-config /etc/bind9gateway/agent.toml
```

Outputs a ready-to-apply Kubernetes Secret and a client config for the remote host.

---

## Implementation Phases

### Phase 1 — Proof of Concept (spike)

**Goal:** Validate that rathole with Noise_KK can tunnel bindcar traffic reliably.

- [ ] Run rathole-server in a local container, rathole-client on localhost
- [ ] Point a local bindy dev instance at rathole-server
- [ ] Exercise the full zone create/update/delete flow through the tunnel
- [ ] Measure latency overhead (target: < 5ms added per call on LAN)
- [ ] Confirm TLS 1.3 / Noise_KK is enforced (wireshark or `openssl s_client`)

Deliverable: documented spike results in `docs/roadmaps/external-bind9-gateway-spike.md`

---

### Phase 2 — CRD Design

- [ ] Add `Bind9Gateway` struct to `src/crd.rs`
  - `spec.tunnel` (port, transport, secretRef)
  - `spec.services[]` (name, token, exposedPort)
  - `status.conditions` (TunnelConnected)
- [ ] Add `gatewayRef` field to `Bind9InstanceSpec`
- [ ] Run `regen-crds` skill → update examples → run `regen-api-docs` skill
- [ ] Write TDD tests for new spec validation logic

---

### Phase 3 — Bind9Gateway Reconciler

New reconciler: `src/reconcilers/bind9gateway.rs`

- [ ] On `Bind9Gateway` create/update:
  - Create rathole-server `Deployment` (sidecar pattern: rathole image + configmap)
  - Create headless `Service` per exposed port
  - Write rathole `ConfigMap` from spec
  - Set `TunnelConnected: Unknown` initially
- [ ] Watch for tunnel liveness:
  - Implement a `/health` probe or TCP check against the rathole-server listen port
  - Flip `TunnelConnected` condition based on probe result
- [ ] On `Bind9Gateway` delete: clean up owned resources (ownerReferences handle GC)

---

### Phase 4 — Bind9Instance External Mode

Changes to `src/reconcilers/bind9instance.rs` (and bind9cluster instance creation):

- [ ] If `spec.gatewayRef` is set → skip StatefulSet/Pod creation entirely (early return guard clause)
- [ ] Resolve bindcar endpoint: check if `gatewayRef` is set → use gateway Service DNS name instead of pod IP
- [ ] Gate reconciliation on `Bind9Gateway.status.conditions[TunnelConnected] == True`
- [ ] Expose gateway connectivity in `Bind9Instance` status conditions

---

### Phase 5 — bind9gateway-agent Binary

New binary (separate crate or workspace member: `crates/bind9gateway-agent/`):

- [ ] `bind9gateway-agent serve` — wraps rathole-client, reads TOML config
- [ ] `bind9gateway-agent keygen` — generates Noise_KK keypair, outputs K8s Secret YAML + client config
- [ ] `bind9gateway-agent check` — connectivity test (connects, sends a probe, exits 0 on success)
- [ ] Docker image: `ghcr.io/firestoned/bind9gateway-agent`
- [ ] Systemd unit file for bare-metal deployment
- [ ] Integration test: spin up agent in a container, validate full zone roundtrip

---

### Phase 6 — Documentation & Hardening

- [ ] User guide: `docs/src/guide/external-bind9.md`
  - Getting-started walkthrough (keygen → deploy agent → create Bind9Gateway → create Bind9Instance with gatewayRef)
  - bind9 `named.conf` hardening (127.0.0.1 only)
  - Firewall rules (only tunnel port needs to be reachable from remote to cluster ingress)
- [ ] Troubleshooting guide: tunnel not connecting, certificate issues, reconnect behavior
- [ ] Update `docs/src/architecture/` diagrams to show hybrid topology
- [ ] Security review: confirm Noise_KK key rotation procedure
- [ ] Add `upgrade-bindcar` skill step: also check `bind9gateway-agent` image version

---

## Open Questions

1. **Ingress exposure of rathole-server port**: The rathole-server listen port (default 2333) needs to be reachable from the remote host. Options: NodePort, LoadBalancer, or existing TCP ingress (e.g., nginx-ingress TCP passthrough). Which is standard for this environment?

2. **Multiple remote instances per gateway**: Should one `Bind9Gateway` pod serve multiple remote bind9 instances (one tunnel, multiple services/tokens), or one pod per instance? One-pod-per-gateway is simpler to reason about but multiplexing reduces resource cost.

3. **Noise_KK vs cert-manager TLS**: If cert-manager is already deployed in the cluster, TLS 1.3 mTLS with auto-rotating certs may be preferable over static Noise keys. The CRD should support both. Does this environment use cert-manager?

4. **rathole version pinning**: rathole is not as actively maintained as frp (last release 2023). We should evaluate whether to vendor the rathole binary, build from source in CI, or build our own thin tunnel using `tokio` + `rustls` directly (more work but zero dependency on an external project).

5. **DNS resolution for external instances**: With external bind9 instances, the `Bind9Instance` won't have a pod IP. Does Scout (the service discovery component) need changes to handle instances without pod IPs?

---

## Considered Alternatives

### Build a custom tunnel using tokio + rustls

**Pro:** No dependency on rathole; full control; can use the exact Noise_KK or TLS 1.3 config we want; stays entirely in the Rust/bindcar ecosystem.
**Con:** Significant implementation work; tunnel reliability (reconnect, flow control, multiplexing) is non-trivial to get right.
**Verdict:** Revisit if rathole maintenance becomes a concern. For phase 1, rathole lets us validate the architecture quickly.

### Use WireGuard

**Pro:** Kernel-level, extremely high performance, widely deployed, TLS-equivalent security.
**Con:** Requires kernel module on the remote host (not always available on older VMs); requires CAP_NET_ADMIN in the pod; more complex key management; creates a full network rather than a service tunnel.
**Verdict:** Better fit for full mesh networking (multi-cluster). Overkill for single-service tunneling.

### SSH reverse tunnel

**Pro:** Ubiquitous, well-understood, available everywhere.
**Con:** Not designed for production automated tunneling; key rotation is complex; no native Kubernetes integration; performance overhead of SSH protocol.
**Verdict:** Suitable for manual testing only.

### Service Mesh (Linkerd multicluster)

**Pro:** Linkerd is already the service mesh standard for this project; multicluster extension is well-supported.
**Con:** The remote host is not a Kubernetes cluster — Linkerd multicluster requires a Kubernetes control plane on both sides.
**Verdict:** Not applicable for bare-metal/VM targets.
