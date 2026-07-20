# Threat Model - Bindy DNS Operator

**Version:** 1.1
**Last Updated:** 2026-07-19
**Owner:** Security Team
**Compliance:** SOX 404, PCI-DSS 6.4.1, Basel III Cyber Risk

> **Revision note (v1.1):** The v1.0 model predates the **Scout** controller
> (added 2026-03-20) and did not cover it. This revision adds Scout as a
> first-class component, trust boundary, and set of STRIDE threats, and
> updates the model to reflect several mitigations merged since v1.0: the B-5
> Secret-RBAC split, the opt-in namespace-scoped operator mode, the BIND9
> operand's move to an unprivileged DNS port (no `NET_BIND_SERVICE`), the
> expansion of `ValidatingAdmissionPolicy` coverage from 0 to 16 policies, Scout
> namespace whitelisting (`--namespace-selector`, M-30), and automated
> Dependabot auto-merge.
>
> **This draft identified a CRITICAL finding (I4/E4/Scenario 6): Scout's
> `ClusterRole` carried an unscoped `secrets: get` grant across every namespace
> in the cluster.** Per this project's disclosure practice — fix before
> publishing exploit-level detail — that finding was remediated (M-25) the same
> day it was drafted, before this revision was published. The document below
> retains the full historical write-up of the finding (marked ✅ FIXED) for
> audit-trail completeness; there is no live unpatched CRITICAL item in this
> revision as published.

---

## Table of Contents

- [Overview](#overview)
- [System Description](#system-description)
- [Assets](#assets)
- [Trust Boundaries](#trust-boundaries)
- [STRIDE Threat Analysis](#stride-threat-analysis)
- [Attack Surface](#attack-surface)
- [Threat Scenarios](#threat-scenarios)
- [Mitigations](#mitigations)
- [Residual Risks](#residual-risks)
- [Security Architecture](#security-architecture)

---

## Overview

This document provides a comprehensive threat model for the Bindy DNS Operator, a Kubernetes operator that manages BIND9 DNS servers. The threat model uses the STRIDE methodology (Spoofing, Tampering, Repudiation, Information Disclosure, Denial of Service, Elevation of Privilege) to identify and analyze security threats.

### Objectives

1. **Identify threats** to the DNS infrastructure managed by Bindy
2. **Assess risk** for each identified threat
3. **Document mitigations** (existing and required)
4. **Provide security guidance** for deployers and operators
5. **Support compliance** with SOX 404, PCI-DSS 6.4.1, Basel III

### Scope

**In Scope:**
- Bindy operator container and runtime
- Custom Resource Definitions (CRDs) and Kubernetes API interactions
- BIND9 pods managed by Bindy
- DNS zone data and configuration
- RNDC (Remote Name Daemon Control) communication
- Container images and supply chain
- CI/CD pipeline security

**Out of Scope:**
- Kubernetes cluster security (managed by platform team)
- Network infrastructure security (managed by network team)
- Physical security of data centers
- DNS client security (recursive resolvers outside our control)

---

## System Description

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     Kubernetes Cluster                       │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │              bindy-system Namespace                   │    │
│  │                                                      │    │
│  │  ┌──────────────────────────────────────────────┐  │    │
│  │  │        Bindy Operator (Deployment)         │  │    │
│  │  │  ┌────────────────────────────────────────┐  │  │    │
│  │  │  │  Operator Pod (Non-Root, ReadOnly)   │  │    │    │
│  │  │  │  - Watches CRDs                        │  │  │    │
│  │  │  │  - Reconciles DNS zones                │  │  │    │
│  │  │  │  - Manages BIND9 pods                  │  │  │    │
│  │  │  │  - Uses RNDC for zone updates         │  │  │    │
│  │  │  └────────────────────────────────────────┘  │  │    │
│  │  └──────────────────────────────────────────────┘  │    │
│  │                                                      │    │
│  │  ┌──────────────────────────────────────────────┐  │    │
│  │  │       BIND9 Primary (StatefulSet)           │  │    │
│  │  │  ┌────────────────────────────────────────┐  │  │    │
│  │  │  │  BIND Pod (Non-Root, ReadOnly)         │  │  │    │
│  │  │  │  - Authoritative DNS (Port 53)         │  │  │    │
│  │  │  │  - RNDC Control (Port 9530)             │  │  │    │
│  │  │  │  - Zone files (ConfigMaps)             │  │  │    │
│  │  │  │  - RNDC key (Secret, read-only)        │  │  │    │
│  │  │  └────────────────────────────────────────┘  │  │    │
│  │  └──────────────────────────────────────────────┘  │    │
│  │                                                      │    │
│  │  ┌──────────────────────────────────────────────┐  │    │
│  │  │      BIND9 Secondaries (StatefulSet)        │  │    │
│  │  │  - Receive zone transfers from primary       │  │    │
│  │  │  - Provide redundancy                        │  │    │
│  │  │  - Geographic distribution                   │  │    │
│  │  └──────────────────────────────────────────────┘  │    │
│  │                                                      │    │
│  └────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │         Other Namespaces (Multi-Tenancy)           │    │
│  │  - team-web (DNSZone CRs)                          │    │
│  │  - team-api (DNSZone CRs)                          │    │
│  │  - platform-dns (Bind9Cluster CRs)                 │    │
│  └────────────────────────────────────────────────────┘    │
│                                                              │
└─────────────────────────────────────────────────────────────┘
          │                           ▲
          │ DNS Queries (UDP/TCP 53)  │
          ▼                           │
    ┌─────────────────────────────────────┐
    │       External DNS Clients          │
    │  - Recursive resolvers              │
    │  - Corporate clients                │
    │  - Internet users                   │
    └─────────────────────────────────────┘
```

### Components

1. **Bindy Operator**
   - Kubernetes operator written in Rust
   - Watches custom resources (Bind9Cluster, Bind9Instance, DNSZone, DNS records)
   - Reconciles desired state with actual state
   - Manages BIND9 deployments, ConfigMaps, Secrets, Services
   - Uses RNDC to update zones on running BIND9 instances

2. **BIND9 Pods**
   - Authoritative DNS servers running BIND9
   - Primary server handles zone updates
   - Secondary servers replicate zones via AXFR/IXFR
   - Exposed via LoadBalancer or NodePort services

3. **Custom Resources (CRDs)**
   - `Bind9Cluster`: Cluster-scoped, defines BIND9 cluster topology
   - `Bind9Instance`: Namespaced, defines individual BIND9 server
   - `DNSZone`: Namespaced, defines DNS zone (e.g., example.com)
   - DNS Records: `ARecord`, `CNAMERecord`, `MXRecord`, etc.

4. **Supporting Resources**
   - ConfigMaps: Store BIND9 configuration and zone files
   - Secrets: Store RNDC keys (symmetric HMAC keys)
   - Services: Expose DNS (port 53, forwarding to the operand's unprivileged
     container port 5353) and RNDC (port 9530)
   - ServiceAccounts: RBAC for operator access

5. **Bindy Scout** *(added 2026-03-20; see [Trust Boundary 6](#boundary-6-scout-controller))*
   - Separate binary/`ServiceAccount` from the main operator, its own `ClusterRole` (`bindy-scout`)
   - Watches `Ingress`, `Service` (LoadBalancer), and Gateway API `HTTPRoute`/`TLSRoute`/`TCPRoute`
     resources **cluster-wide, across all namespaces**, for an opt-in annotation
   - On opt-in, auto-creates/deletes `ARecord` CRs and adds/removes its own finalizer —
     which requires cluster-wide `patch`/`update` on the watched resource types, not
     just `get`/`list`/`watch`
   - **Phase 2 (multi-cluster) mode**: reads a kubeconfig from a Kubernetes `Secret` to
     target a *different* cluster's Bindy install; the RBAC for this is
     `resources: ["secrets"], verbs: ["get"]` with **no namespace or resourceName
     scoping** — i.e. cluster-wide read of every Secret in the cluster where Scout
     runs. The code comment authoring this rule flags it explicitly: *"Scope this to
     a Role in the specific Secret's namespace for production deployments."* This has
     not yet been done — see [Residual Risks](#residual-risks).

---

## Assets

### High-Value Assets

| Asset | Description | Confidentiality | Integrity | Availability | Owner |
|-------|-------------|-----------------|-----------|--------------|-------|
| **DNS Zone Data** | Authoritative DNS records for all managed domains | Medium | **Critical** | **Critical** | Teams/Platform |
| **RNDC Keys** | Symmetric HMAC keys for BIND9 control | **Critical** | **Critical** | High | Security Team |
| **Operator Binary** | Signed container image with operator logic | Medium | **Critical** | High | Development Team |
| **BIND9 Configuration** | named.conf, zone configs | Low | **Critical** | High | Platform Team |
| **Kubernetes API Access** | ServiceAccount token for operator | **Critical** | **Critical** | **Critical** | Platform Team |
| **Scout ServiceAccount Token** | Grants cluster-wide `secrets:get` + cluster-wide `patch`/`update` on Ingress/Service/Gateway-API routes | **Critical** | **Critical** | High | Platform Team |
| **All Cluster Secrets (via Scout)** | Any Secret in any namespace, readable by the Scout ServiceAccount today | **Critical** | N/A | N/A | Security Team |
| **CRD Schemas** | Define API contract for DNS management | Low | **Critical** | Medium | Development Team |
| **Audit Logs** | Record of all DNS changes and access | High | **Critical** | High | Security Team |
| **SBOM** | Software Bill of Materials for compliance | Low | **Critical** | Medium | Compliance Team |

### Asset Protection Goals

- **DNS Zone Data**: Prevent unauthorized modification (tampering), ensure availability
- **RNDC Keys**: Prevent disclosure (compromise allows full BIND9 control)
- **Operator Binary**: Prevent supply chain attacks, ensure code integrity
- **Kubernetes API Access**: Prevent privilege escalation, enforce least privilege
- **Audit Logs**: Ensure non-repudiation, prevent tampering, retain for compliance

---

## Trust Boundaries

### Boundary 1: Kubernetes Cluster Perimeter

**Trust Level:** High
**Description:** Kubernetes API server, etcd, and cluster networking

**Assumptions:**
- Kubernetes RBAC is properly configured
- etcd is encrypted at rest
- Network policies are enforced
- Node security is managed by platform team

**Threats if Compromised:**
- Attacker gains full control of all resources in cluster
- DNS data can be exfiltrated or modified
- Operator can be manipulated or replaced

---

### Boundary 2: bindy-system Namespace

**Trust Level:** High
**Description:** Namespace containing Bindy operator and BIND9 pods

**Assumptions:**
- RBAC limits access to authorized ServiceAccounts only
- Secrets are encrypted at rest in etcd
- Pod Security Standards enforced (Restricted)

**Threats if Compromised:**
- Attacker can read RNDC keys
- Attacker can modify DNS zones
- Attacker can disrupt DNS service

---

### Boundary 3: Operator Container

**Trust Level:** Medium-High
**Description:** Bindy operator runtime environment

**Assumptions:**
- Container runs as non-root user
- Filesystem is read-only except /tmp
- No privileged capabilities
- Resource limits enforced

**Threats if Compromised:**
- Attacker can abuse Kubernetes API access
- Attacker can read secrets operator has access to
- Attacker can disrupt reconciliation loops

---

### Boundary 4: BIND9 Container

**Trust Level:** Medium
**Description:** BIND9 DNS server runtime

**Assumptions:**
- Container runs as non-root, and — as of the operand's move to the
  **unprivileged container port 5353** — with **no added Linux capabilities**
  (`NET_BIND_SERVICE` has been dropped; the container `securityContext`
  `capabilities.add` is empty). The `named` process cannot bind any port `<
  1024` even if further compromised. The client-facing `Service` still exposes
  the standard DNS port 53 and forwards to the container's 5353.
- Exposed to internet (Service port 53 → container port 5353)
- Configuration is managed by operator (read-only)

**Threats if Compromised:**
- Attacker can serve malicious DNS responses
- Attacker can exfiltrate zone data
- Attacker can pivot to other cluster resources (if network policies weak) —
  a reference `NetworkPolicy` now exists (`deploy/pod-hardening.yaml`,
  ingress/egress scoped to 5353 for peer transfers and 53 for CoreDNS) but is
  **not applied by any install target** — it is opt-in and must be applied
  manually. See M-17 in [Mitigations](#mitigations).

---

### Boundary 5: External Network (Internet)

**Trust Level:** Untrusted
**Description:** Public internet where DNS clients reside

**Assumptions:**
- All traffic is potentially hostile
- DDoS attacks are likely
- DNS protocol vulnerabilities will be exploited

**Threats:**
- DNS amplification attacks (abuse open resolvers)
- Cache poisoning attempts
- Zone enumeration (AXFR abuse)
- DoS via query floods

---

### Boundary 6: Scout Controller

**Trust Level:** Medium (cluster-wide blast radius, narrower purpose than the main operator)
**Description:** Separate binary/`ServiceAccount`/`ClusterRole` (`bindy-scout`) that
watches source objects (`Ingress`, `Service`, `HTTPRoute`, `TLSRoute`, `TCPRoute`)
**across every namespace** and creates `ARecord` CRs in response to an opt-in
annotation. In Phase 2 (multi-cluster) mode it also reads a kubeconfig `Secret` to
act against a remote cluster.

**Assumptions:**
- Runs as its own Deployment/ServiceAccount, distinct from the main operator
- Pod-hardening posture (non-root, read-only rootfs, seccomp) matches the main operator
- The opt-in annotation (`bindy.firestoned.io/scout-enabled: "true"`) is the only gate
  before Scout mutates a resource — any tenant who can set that annotation on their
  own `Ingress`/`Service`/route object can cause Scout to write `ARecord`s
- **(Fixed 2026-07-19, M-25)** Scout's Secret access is namespaced and
  `resourceNames`-restricted to the single Phase 2 kubeconfig Secret — no longer
  cluster-wide. Same-cluster-only deployments (the default) get no Secret access at all.

**Threats if Compromised:**
- **Cross-tenant object tampering.** Scout's cluster-wide `patch`/`update` on
  `Ingress`/`Service`/route types (required for its own finalizer bookkeeping) means
  a compromised Scout could, in principle, modify any tenant's Ingress/Service/route
  object in any namespace — not only add/remove its own finalizer. This remains the
  primary residual risk for this component — see
  [T4](#t4-cross-tenant-tampering-via-scouts-cluster-wide-write-rbac).
- ~~Cluster-wide Secret exfiltration~~ — **fixed 2026-07-19 (M-25)**. Scout's
  `ClusterRole` no longer grants any Secret access; a namespaced,
  `resourceNames`-restricted Role scoped to the single Phase 2 kubeconfig Secret is
  used instead (`deploy/scout/secrets-reader-rbac.yaml`, applied only when Phase 2
  mode is configured). See [I4](#i4-scout-cluster-wide-secret-read) and
  [Scenario 6](#scenario-6-compromised-scout-pod) for the historical analysis and
  current (resolved) status.
- A compromised Scout is **not** a path to DNS zone data or RNDC keys directly (it
  only creates `ARecord`s, gated by the same zone-authorization check as the main
  operator) — its distinctive risk is the unscoped Secret read and cross-tenant
  write surface above.

---

## STRIDE Threat Analysis

### S - Spoofing (Identity)

#### S1: Spoofed Kubernetes API Requests

**Threat:** Attacker impersonates the Bindy operator ServiceAccount to make unauthorized API calls.

**Impact:** HIGH
**Likelihood:** LOW (requires compromised cluster or stolen token)

**Attack Scenario:**
1. Attacker compromises a pod in the cluster
2. Steals ServiceAccount token from `/var/run/secrets/kubernetes.io/serviceaccount/token`
3. Uses token to impersonate operator and modify DNS zones

**Mitigations:**
- ✅ RBAC least privilege (operator cannot delete resources)
- ✅ Pod Security Standards (non-root, read-only filesystem)
- ✅ Short-lived ServiceAccount tokens (TokenRequest API)
- ❌ **MISSING**: Network policies to restrict egress from operator pod
- ❌ **MISSING**: Audit logging for all ServiceAccount API calls

**Residual Risk:** MEDIUM (need network policies and audit logs)

---

#### S2: Spoofed RNDC Commands

**Threat:** Attacker gains access to RNDC key and sends malicious commands to BIND9.

**Impact:** CRITICAL
**Likelihood:** LOW (RNDC keys stored in Kubernetes Secrets with RBAC)

**Attack Scenario:**
1. Attacker compromises operator pod or namespace
2. Reads RNDC key from Kubernetes Secret
3. Connects to BIND9 RNDC port (9530) and issues commands (e.g., `reload`, `freeze`, `thaw`)

**Mitigations:**
- ✅ Secrets encrypted at rest (Kubernetes)
- ✅ RBAC limits secret read access to operator only
- ✅ RNDC port (9530) not exposed externally
- ❌ **MISSING**: Secret access audit trail (H-3)
- ❌ **MISSING**: RNDC key rotation policy

**Residual Risk:** MEDIUM (need secret audit trail)

---

#### S3: Spoofed Git Commits (Supply Chain)

**Threat:** Attacker forges commits without proper signature, injecting malicious code.

**Impact:** CRITICAL
**Likelihood:** VERY LOW (branch protection enforces signed commits)

**Attack Scenario:**
1. Attacker compromises GitHub account or uses stolen SSH key
2. Pushes unsigned commit to feature branch
3. Attempts to merge to main without proper review

**Mitigations:**
- ✅ All commits MUST be signed (GPG/SSH)
- ✅ GitHub branch protection requires signed commits
- ✅ CI/CD verifies commit signatures
- ✅ 2+ reviewers required for all PRs
- ✅ Linear history (no merge commits)

**Residual Risk:** VERY LOW (strong controls in place)

---

### T - Tampering (Data Integrity)

#### T1: Tampering with DNS Zone Data

**Threat:** Attacker modifies DNS records to redirect traffic or cause outages.

**Impact:** CRITICAL
**Likelihood:** LOW (requires Kubernetes API access)

**Attack Scenario:**
1. Attacker gains write access to DNSZone CRs (via compromised RBAC or stolen credentials)
2. Modifies A/CNAME records to point to attacker-controlled servers
3. Traffic is redirected, enabling phishing, data theft, or service disruption

**Mitigations:**
- ✅ RBAC enforces least privilege (users can only modify zones in their namespace)
- ✅ GitOps workflow (changes via pull requests, not direct kubectl)
- ✅ Audit logging in Kubernetes (all CR modifications logged)
- ❌ **MISSING**: Webhook validation for DNS records (prevent obviously malicious changes)
- ❌ **MISSING**: DNSSEC signing (prevents tampering of DNS responses in transit)

**Residual Risk:** MEDIUM (need validation webhooks and DNSSEC)

---

#### T2: Tampering with Container Images

**Threat:** Attacker replaces legitimate Bindy/BIND9 container image with malicious version.

**Impact:** CRITICAL
**Likelihood:** VERY LOW (signed images, supply chain controls)

**Attack Scenario:**
1. Attacker compromises CI/CD pipeline or registry credentials
2. Pushes malicious image with same tag (e.g., `:latest`)
3. Operator pulls compromised image on next rollout

**Mitigations:**
- ✅ All images signed with provenance attestation (SLSA Level 2)
- ✅ SBOM generated for all releases
- ✅ GitHub Actions signed commits verification
- ✅ Multi-stage builds minimize attack surface
- ❌ **MISSING**: Image digests pinned (not tags) - see M-1
- ❌ **MISSING**: Admission operator to verify image signatures (e.g., Sigstore Cosign)

**Residual Risk:** LOW (strong supply chain controls, but pinning digests would further reduce risk)

---

#### T3: Tampering with ConfigMaps/Secrets

**Threat:** Attacker modifies BIND9 configuration or RNDC keys via Kubernetes API.

**Impact:** HIGH
**Likelihood:** LOW (RBAC protects ConfigMaps/Secrets)

**Attack Scenario:**
1. Attacker gains elevated privileges in `bindy-system` namespace
2. Modifies BIND9 ConfigMap to disable security features or add backdoor zones
3. BIND9 pod restarts with malicious configuration

**Mitigations:**
- ✅ Operator has NO delete permissions on Secrets/ConfigMaps (C-2)
- ✅ RBAC limits write access to operator only
- ✅ **B-5 hardening (2026-06-30):** the operator's cluster-wide `ClusterRole` grants
  only `get`/`list`/`watch` on Secrets; the mutating verbs
  (`create`/`update`/`patch`/`delete`) were moved to a namespaced Role
  (`bindy-secrets-writer`) bound **only in the operator's own namespace**. A
  compromised operator can no longer create/modify/delete Secrets in other
  namespaces such as `kube-system`.
- ✅ Immutable ConfigMaps (once created, cannot be modified - requires recreation)
- ❌ **MISSING**: ConfigMap/Secret integrity checks (hash validation)
- ❌ **MISSING**: Automated drift detection (compare running config vs desired state)

**Residual Risk:** MEDIUM (need integrity checks; note the B-5 split reduces but does
not eliminate risk — the operator can still write Secrets within its own namespace.
Scout's Secret access, formerly a separate larger gap, was fixed 2026-07-19 — see I4.)

---

#### T4: Cross-Tenant Tampering via Scout's Cluster-Wide Write RBAC

**Threat:** A compromised Scout pod/token modifies an `Ingress`, `Service`, or
Gateway API route object belonging to a different team/namespace.

**Impact:** HIGH
**Likelihood:** LOW (requires compromising the Scout pod or its ServiceAccount token)

**Attack Scenario:**
1. Attacker exploits a vulnerability in Scout (memory corruption, dependency CVE) or
   steals its ServiceAccount token from a compromised node
2. Scout's `ClusterRole` grants `patch`/`update` on `ingresses`, `services`,
   `httproutes`, `tlsroutes`, `tcproutes` **cluster-wide** (required so
   `kube-rs`'s `finalizer::finalizer()` helper can add/remove Scout's finalizer on
   the *main resource*, not just a subresource — see the comments in
   `deploy/scout/clusterrole.yaml`)
3. Attacker uses this to modify a tenant's Ingress/Service/route object in a
   namespace Scout has no legitimate business reason to touch that day

**Mitigations:**
- ✅ Scope is limited to `patch`/`update` — no `create`/`delete` on these types
- ✅ Same pod-hardening posture as the main operator (non-root, read-only rootfs)
- ✅ **Namespace whitelisting (`--namespace-selector` / `BINDY_SCOUT_NAMESPACE_SELECTOR`,
  M-30):** when configured, a namespace must match the selector *and* the individual
  object must carry its own opt-in annotation before Scout acts. This reduces the
  set of namespaces Scout's *application logic* will touch during normal operation —
  ⚠️ but does **not** shrink the underlying `ClusterRole` grant. A directly compromised
  ServiceAccount token still technically holds cluster-wide `patch`/`update` on these
  types regardless of the selector (the selector is enforced by Scout's own
  reconciler code, not by RBAC). **Opt-in and unset by default** — see M-30.
- ❌ **MISSING**: No admission policy constrains *what* Scout can patch on
  these types (e.g. restrict to only the finalizer/annotation fields) — M-28

**Residual Risk:** MEDIUM (bounded by patch/update-only scope; namespace whitelisting
reduces likelihood of an *opt-in-triggered* incident when configured, but does not
change what a *token-holding* attacker could reach — no field-level admission control
exists to constrain that further)

---

### R - Repudiation (Non-Repudiation)

#### R1: Unauthorized DNS Changes Without Attribution

**Threat:** Attacker modifies DNS zones and there's no audit trail proving who made the change.

**Impact:** HIGH (compliance violation, incident response hindered)
**Likelihood:** LOW (Kubernetes audit logs capture API calls)

**Attack Scenario:**
1. Attacker gains access to cluster with weak RBAC
2. Modifies DNSZone CRs
3. No log exists linking the change to a specific user or ServiceAccount

**Mitigations:**
- ✅ Kubernetes audit logs enabled (captures all API requests)
- ✅ All commits signed (non-repudiation for code changes)
- ✅ GitOps workflow (changes traceable to Git commits and PR reviews)
- ❌ **MISSING**: Centralized log aggregation with tamper-proof storage (H-2)
- ❌ **MISSING**: Log retention policy (90 days active, 1 year archive per PCI-DSS)
- ❌ **MISSING**: Audit trail queries documented for compliance reviews

**Residual Risk:** MEDIUM (need H-2 - Audit Log Retention Policy)

---

#### R2: Secret Access Without Audit Trail

**Threat:** Attacker reads RNDC keys from Secrets, no record of who accessed them.

**Impact:** HIGH
**Likelihood:** LOW (secret access is logged by Kubernetes, but not prominently tracked)

**Attack Scenario:**
1. Attacker compromises ServiceAccount with secret read access
2. Reads RNDC key from Kubernetes Secret
3. Uses key to control BIND9, but no clear audit trail of secret access

**Mitigations:**
- ✅ Kubernetes audit logs capture Secret read operations
- ❌ **MISSING**: Dedicated audit trail for secret access (H-3)
- ❌ **MISSING**: Alerts on unexpected secret reads
- ❌ **MISSING**: Secret access dashboard for compliance reviews

**Residual Risk:** MEDIUM (need H-3 - Secret Access Audit Trail)

---

### I - Information Disclosure

#### I1: Exposure of RNDC Keys

**Threat:** RNDC keys leaked via logs, environment variables, or insecure storage.

**Impact:** CRITICAL
**Likelihood:** VERY LOW (secrets stored in Kubernetes Secrets, not in code)

**Attack Scenario:**
1. Developer hardcodes RNDC key in code or logs it for debugging
2. Key is committed to Git or appears in log aggregation system
3. Attacker finds key and uses it to control BIND9

**Mitigations:**
- ✅ Secrets stored in Kubernetes Secrets (encrypted at rest)
- ✅ Pre-commit hooks to detect secrets in code
- ✅ GitHub secret scanning enabled
- ✅ CI/CD fails if secrets detected
- ❌ **MISSING**: Log sanitization (ensure secrets never appear in logs)
- ❌ **MISSING**: Secret rotation policy (rotate RNDC keys periodically)

**Residual Risk:** LOW (good controls, but rotation would improve)

---

#### I2: Zone Data Enumeration

**Threat:** Attacker uses AXFR (zone transfer) to download entire zone contents.

**Impact:** MEDIUM (zone data is semi-public, but bulk enumeration aids reconnaissance)
**Likelihood:** MEDIUM (AXFR often left open by mistake)

**Attack Scenario:**
1. Attacker sends AXFR request to BIND9 server
2. If AXFR is not restricted, server returns all records in zone
3. Attacker uses zone data for targeted attacks (subdomain enumeration, email harvesting)

**Mitigations:**
- ✅ AXFR restricted to secondary servers only (BIND9 `allow-transfer` directive)
- ✅ BIND9 configuration managed by operator (prevents manual misconfig)
- ❌ **MISSING**: TSIG authentication for zone transfers (H-4)
- ❌ **MISSING**: Rate limiting on AXFR requests

**Residual Risk:** MEDIUM (need TSIG for AXFR)

---

#### I3: Container Image Vulnerability Disclosure

**Threat:** Container images contain vulnerabilities that could be exploited if disclosed.

**Impact:** MEDIUM
**Likelihood:** MEDIUM (vulnerabilities exist in all software)

**Attack Scenario:**
1. Vulnerability is disclosed in a dependency (e.g., CVE in glibc)
2. Attacker scans for services using vulnerable version
3. Exploits vulnerability to gain RCE or escalate privileges

**Mitigations:**
- ✅ Automated vulnerability scanning (cargo-audit + Trivy) - C-3
- ✅ CI blocks on CRITICAL/HIGH vulnerabilities
- ✅ Daily scheduled scans detect new CVEs
- ✅ Remediation SLAs defined (CRITICAL: 24h, HIGH: 7d)
- ✅ Chainguard zero-CVE base images used

**Residual Risk:** LOW (strong vulnerability management)

---

#### I4: Scout Cluster-Wide Secret Read

**Status: ✅ FIXED 2026-07-19 (M-25).** Kept in full below as the historical record of
the finding and its fix — see the "Fix" subsection for current state.

**Threat (historical):** A compromised Scout pod, or anyone able to exec into it or
steal its ServiceAccount token, could read **every Secret in the cluster** — not
just the one kubeconfig Secret it legitimately needs for multi-cluster mode.

**Impact (historical):** CRITICAL
**Likelihood:** LOW (requires compromising the Scout pod/token specifically), but
**this had the largest blast radius of any threat in this document** — worse than
compromising the main operator, whose Secret access is namespace-scoped for
mutation and (with `BINDY_WATCH_NAMESPACES` set) can be namespace-scoped for read too.

**Original finding:** Scout's `ClusterRole` (`deploy/scout/clusterrole.yaml`) granted
`apiGroups: [""], resources: ["secrets"], verbs: ["get"]` with **no `resourceNames`
and no `Role`/namespace scoping** — a `ClusterRole` applies cluster-wide by
construction, so this let a compromised Scout token read RNDC keys, other teams'
database credentials, TLS private keys, CI/CD tokens — any Secret in any namespace
in the cluster. The code's own comment had documented the intended fix ("Scope this
to a Role in the specific Secret's namespace for production deployments") since
before this was reported, but it had not been implemented.

**Fix (2026-07-19):**
- ✅ The cluster-wide `secrets` `PolicyRule` was **removed entirely** from the
  `bindy-scout` `ClusterRole` (`build_scout_cluster_role` / `clusterrole.yaml`).
- ✅ Replaced with a **namespaced**, **`resourceNames`-restricted** Role
  (`bindy-scout-secrets-reader`) + RoleBinding, granting `get` on exactly the one
  configured Secret — not every Secret in the namespace, and not any Secret in any
  other namespace.
- ✅ This Role/RoleBinding is applied **only when Phase 2 (multi-cluster) mode is
  configured** (`--remote-secret` at bootstrap time, or manually via the new opt-in
  `deploy/scout/secrets-reader-rbac.yaml`, mirroring the existing
  `remote-cluster-rbac.yaml` pattern). Same-cluster-only deployments (the default)
  now get **zero** Secret access.
- ✅ Tests assert the ClusterRole has no `secrets` rule at all, and that the new
  Role's rule is `get`-only and `resourceNames`-restricted.
- ❌ **Still missing**: audit alerting on Scout's Secret reads (a defense-in-depth
  addition, not required now that the grant itself is minimal).

**Residual Risk:** **LOW** (down from HIGH). The blast radius of a Scout
compromise for Secret confidentiality is now bounded to the single Phase 2
kubeconfig Secret, in deployments that use Phase 2 mode at all.

---

### D - Denial of Service

#### D1: DNS Query Flood (DDoS)

**Threat:** Attacker floods BIND9 servers with DNS queries, exhausting resources.

**Impact:** CRITICAL (DNS unavailability impacts all services)
**Likelihood:** HIGH (DNS is a common DDoS target)

**Attack Scenario:**
1. Attacker uses botnet to send millions of DNS queries to BIND9 servers
2. BIND9 CPU/memory exhausted, becomes unresponsive
3. Legitimate DNS queries fail, causing outages

**Mitigations:**
- ✅ Rate limiting in BIND9 (`rate-limit` directive)
- ✅ Resource limits on BIND9 pods (CPU/memory requests/limits)
- ✅ Horizontal scaling (multiple BIND9 secondaries)
- ❌ **MISSING**: DDoS protection at network edge (e.g., CloudFlare, AWS Shield)
- ❌ **MISSING**: Query pattern analysis and anomaly detection
- ❌ **MISSING**: Automated pod scaling based on query load (HPA)

**Residual Risk:** MEDIUM (need edge DDoS protection)

---

#### D2: Operator Resource Exhaustion

**Threat:** Attacker creates thousands of DNSZone CRs, overwhelming operator.

**Impact:** HIGH (operator fails, DNS updates stop)
**Likelihood:** LOW (requires cluster access)

**Attack Scenario:**
1. Attacker gains write access to Kubernetes API
2. Creates 10,000+ DNSZone CRs
3. Operator reconciliation queue overwhelms CPU/memory
4. Operator crashes or becomes unresponsive

**Mitigations:**
- ✅ Resource limits on operator pod
- ✅ Exponential backoff for failed reconciliations
- ❌ **MISSING**: Rate limiting on reconciliation loops (M-3)
- ❌ **MISSING**: Admission webhook to limit number of CRs per namespace
- ❌ **MISSING**: Horizontal scaling of operator (leader election)

**Residual Risk:** MEDIUM (need M-3 - Rate Limiting)

---

#### D3: AXFR Amplification Attack

**Threat:** Attacker abuses AXFR to amplify traffic in DDoS attack.

**Impact:** MEDIUM
**Likelihood:** LOW (AXFR restricted to secondaries)

**Attack Scenario:**
1. Attacker spoofs source IP of DDoS target
2. Sends AXFR request to BIND9
3. BIND9 sends large zone file to spoofed IP (amplification)

**Mitigations:**
- ✅ AXFR restricted to known secondary IPs (`allow-transfer`)
- ✅ BIND9 does not respond to spoofed source IPs (anti-spoofing)
- ❌ **MISSING**: Response rate limiting (RRL) for AXFR

**Residual Risk:** LOW (AXFR restrictions effective)

---

### E - Elevation of Privilege

#### E1: Container Escape to Node

**Threat:** Attacker escapes from Bindy or BIND9 container to underlying Kubernetes node.

**Impact:** CRITICAL (full node compromise, lateral movement)
**Likelihood:** VERY LOW (Pod Security Standards enforced)

**Attack Scenario:**
1. Attacker exploits container runtime vulnerability (e.g., runc CVE)
2. Escapes container to host filesystem
3. Gains root access on node, compromises kubelet and other pods

**Mitigations:**
- ✅ Non-root containers (uid 1000+)
- ✅ Read-only root filesystem
- ✅ No privileged capabilities — the BIND9 operand previously required
  `NET_BIND_SERVICE` to bind privileged port 53; it now binds the **unprivileged
  container port 5353** (Service still exposes 53 to clients) and adds **zero**
  Linux capabilities back after `drop: ["ALL"]`. This closes the one capability
  the operand used to carry.
- ✅ Pod Security Standards (Restricted)
- ✅ seccomp profile (restrict syscalls)
- ✅ AppArmor/SELinux profiles
- ❌ **MISSING**: Regular node patching (managed by platform team)

**Residual Risk:** VERY LOW (defense in depth)

---

#### E2: RBAC Privilege Escalation

**Threat:** Attacker escalates from limited RBAC role to cluster-admin.

**Impact:** CRITICAL
**Likelihood:** VERY LOW (RBAC reviewed, least privilege enforced)

**Attack Scenario:**
1. Attacker compromises ServiceAccount with limited permissions
2. Exploits RBAC misconfiguration (e.g., wildcard permissions)
3. Gains cluster-admin and full control of cluster

**Mitigations:**
- ✅ RBAC least privilege (operator has NO delete permissions) - C-2
- ✅ **B-5 hardening:** operator's cluster-wide Secret access reduced to
  read-only; mutating verbs confined to a namespaced Role in the operator's own
  namespace (see T3)
- ✅ **Namespace-scoped operator mode (opt-in):** `BINDY_WATCH_NAMESPACES`
  restricts the set of namespaces the operator watches; when set, the operator
  uses `Api::namespaced` and needs only per-namespace RoleBindings instead of a
  cluster-wide ClusterRoleBinding — eliminating cluster-wide Secret/workload
  access entirely for deployments that opt in. **Default remains cluster-wide**
  (unset = watch everything), so this mitigation is not yet load-bearing unless
  a deployer explicitly configures it.
- ✅ Automated RBAC verification script (`deploy/rbac/verify-rbac.sh`)
- ✅ No wildcard permissions in operator RBAC
- ✅ Regular RBAC audits (quarterly)
- ⚠️ **Scout is a separate, less-reviewed RBAC surface** — see T4/I4/E4 and
  [Trust Boundary 6](#boundary-6-scout-controller). The verification script and
  quarterly audits should be confirmed to cover `deploy/scout/clusterrole.yaml`,
  not just the main operator's RBAC.
- ❌ **MISSING**: RBAC policy-as-code validation (OPA/Gatekeeper)

**Residual Risk:** VERY LOW for the main operator (strong RBAC controls); see E4
for Scout, which is **not** covered by this residual-risk rating.

---

#### E3: Exploiting Vulnerable Dependencies

**Threat:** Attacker exploits vulnerability in Rust dependency to gain code execution.

**Impact:** HIGH
**Likelihood:** LOW (automated vulnerability scanning, rapid patching)

**Attack Scenario:**
1. CVE disclosed in dependency (e.g., `tokio`, `hyper`, `kube`)
2. Attacker crafts malicious Kubernetes API response to trigger vulnerability
3. Operator crashes or attacker gains RCE in operator pod

**Mitigations:**
- ✅ Automated vulnerability scanning (cargo-audit) - C-3
- ✅ CI blocks on CRITICAL/HIGH vulnerabilities
- ✅ Remediation SLAs enforced (CRITICAL: 24h)
- ✅ Daily scheduled scans
- ✅ Dependency updates via Dependabot
- ⚠️ **Dependabot auto-merge is now automated** (`dependabot-auto-merge.yaml`):
  patch/minor updates are merged automatically once the full e2e gate (integration
  + regression suites) and all required status checks pass, with no human review
  step. Major version bumps are still held open for manual review. This trades a
  manual-review control for a broader-but-automated test gate — the risk this
  accepts is that the e2e suite may not exercise every code path a malicious or
  broken dependency update could affect. Partially offset by: signed-commit
  verification remains a required branch-protection check (not skipped), and the
  gate that verifies the *PR opener* is genuinely `dependabot[bot]`
  (`pull_request.user.login`) rather than the more easily-spoofed `github.actor`.

**Residual Risk:** LOW-MEDIUM (excellent vulnerability *scanning*, but the new
auto-merge automation removes a human checkpoint from the merge path for
patch/minor dependency updates — worth an explicit accept/revisit decision by
Security Team, not just an implicit one)

---

#### E4: Scout ServiceAccount Compromise → Cluster-Wide Pivot

**Status: ✅ FIXED 2026-07-19 (M-25).** This threat's root cause was the same
unscoped Secret read documented under I4; kept in full below as the historical
record.

**Threat (historical):** An attacker who compromises the Scout pod or steals its
ServiceAccount token uses the cluster-wide Secret read (I4) to pivot into other
workloads' trust domains — e.g., reading another team's database credentials or a
CI/CD token stored as a Secret, then using *those* credentials to escalate further.

**Impact (historical):** CRITICAL
**Likelihood:** LOW (requires an initial compromise of Scout specifically)

**Attack Scenario (historical):**
1. Attacker achieves code execution in the Scout pod (dependency CVE, container
   escape, or a stolen ServiceAccount token from a compromised node)
2. Uses Scout's cluster-wide `secrets:get` (I4) to enumerate Secrets across
   namespaces the attacker has no other access to
3. Finds and uses a higher-privilege credential (e.g., another operator's
   ServiceAccount token stored as a Secret, a cloud-provider credential, a CI/CD
   deploy key) to escalate beyond what Scout's own RBAC would allow

**Mitigations:**
- ✅ Scout has no `create`/`delete` on Secrets — read-only
- ✅ Pod Security Standards applied to the Scout Deployment (same as main operator)
- ✅ **I4 fixed (M-25)**: Scout's Secret RBAC is now namespaced and
  `resourceNames`-restricted to the single Phase 2 kubeconfig Secret. Step 2 above
  ("enumerate Secrets across namespaces") is no longer possible — the RBAC to do so
  doesn't exist.
- ❌ **MISSING**: Network policy restricting Scout's egress (it does not need to
  reach most in-cluster services directly — only the Kubernetes API and, in Phase
  2 mode, a remote cluster's API) — M-27

**Residual Risk:** **LOW** (down from HIGH). The root-cause Secret read is closed;
egress restriction (M-27) remains a defense-in-depth item, not a live path to this
scenario.

---

## Attack Surface

### 1. Kubernetes API

**Exposure:** Internal (within cluster)
**Authentication:** ServiceAccount token (JWT)
**Authorization:** RBAC (least privilege)

**Attack Vectors:**
- Token theft from compromised pod
- RBAC misconfiguration allowing excessive permissions
- API server vulnerability (CVE in Kubernetes)

**Mitigations:**
- Short-lived tokens (TokenRequest API)
- RBAC verification script
- Regular Kubernetes upgrades

**Risk:** MEDIUM

---

### 2. DNS Port 53 (UDP/TCP)

**Exposure:** External (internet-facing) — the Kubernetes `Service` exposes the
standard port 53 and forwards to the BIND9 container's **unprivileged port 5353**
(`named` no longer binds a privileged port and carries no `NET_BIND_SERVICE`
capability). This is an internal implementation detail; the client-facing exposure
and risk profile below are unchanged.
**Authentication:** None (public DNS)
**Authorization:** None

**Attack Vectors:**
- DNS amplification attacks
- Query floods (DDoS)
- Cache poisoning attempts (if recursion enabled)
- NXDOMAIN attacks

**Mitigations:**
- Rate limiting (BIND9 `rate-limit`)
- Recursion disabled (authoritative-only)
- DNSSEC (planned)
- DDoS protection at edge
- Operand runs with zero added Linux capabilities (see E1)

**Risk:** HIGH (public-facing, no authentication)

---

### 3. RNDC Port 9530

**Exposure:** Internal (within cluster, not exposed externally)
**Authentication:** HMAC key (symmetric)
**Authorization:** Key-based (all-or-nothing)

**Attack Vectors:**
- RNDC key theft from Kubernetes Secret
- Brute-force HMAC key (unlikely with strong key)
- MITM attack (if network not encrypted)

**Mitigations:**
- Secrets encrypted at rest
- RBAC limits secret read access
- RNDC port not exposed externally
- NetworkPolicy (planned - L-1)

**Risk:** MEDIUM

---

### 4. Container Images (Supply Chain)

**Exposure:** Public (GitHub Container Registry)
**Authentication:** Pull is unauthenticated (public repo)
**Authorization:** Push requires GitHub token with packages:write

**Attack Vectors:**
- Compromised CI/CD pipeline pushing malicious image
- Dependency confusion (malicious crate with same name)
- Compromised base image (upstream supply chain attack)

**Mitigations:**
- Signed commits (all code changes)
- Signed container images (provenance)
- SBOM generation
- Vulnerability scanning (Trivy)
- Chainguard zero-CVE base images
- Dependabot for dependency updates

**Risk:** LOW (strong supply chain security)

---

### 5. Custom Resource Definitions (CRDs)

**Exposure:** Internal (Kubernetes API)
**Authentication:** Kubernetes user/ServiceAccount
**Authorization:** RBAC (namespace-scoped for DNSZone)

**Attack Vectors:**
- Malicious CRs with crafted input (e.g., XXL zone names)
- Schema validation bypass
- CR injection via compromised user

**Mitigations:**
- Schema validation in CRD (OpenAPI v3)
- Input sanitization in operator
- Namespace isolation (RBAC)
- Admission webhooks (planned)

**Risk:** MEDIUM

---

### 6. Git Repository (Code)

**Exposure:** Public (GitHub)
**Authentication:** Push requires GitHub 2FA + signed commits
**Authorization:** Branch protection on `main`

**Attack Vectors:**
- Compromised GitHub account
- Unsigned commit merged to main
- Malicious PR approved by reviewers

**Mitigations:**
- All commits signed (GPG/SSH) - C-1
- Branch protection (2+ reviewers required)
- CI/CD verifies signatures
- Linear history (no merge commits)

**Risk:** VERY LOW (strong controls)

---

### 7. Scout Controller (Cluster-Wide RBAC)

**Exposure:** Internal (Kubernetes API), but with **cluster-wide** scope — every
namespace, not just `bindy-system`
**Authentication:** ServiceAccount token (JWT), same mechanism as the main operator
**Authorization:** `ClusterRole` `bindy-scout` — `get`/`list`/`watch`/`patch`/`update`
on `Ingress`/`Service`/`HTTPRoute`/`TLSRoute`/`TCPRoute` cluster-wide. **No Secret
access at all** on the ClusterRole — see below.

**Attack Vectors:**
- Token theft from a compromised Scout pod (as with the main operator's Attack
  Surface #1, but the resulting read/write scope is cluster-wide by design here)
- Any tenant setting the `bindy.firestoned.io/scout-enabled` annotation on their own
  resource can cause Scout to write an `ARecord` — this is expected/intended
  behavior, not a vulnerability, but means Scout's write path is reachable by any
  namespace user, not just admins
- ~~The unscoped `secrets: get`~~ — **fixed 2026-07-19 (M-25)**. Secret access is now
  a namespaced, `resourceNames`-restricted Role
  (`deploy/scout/secrets-reader-rbac.yaml`), applied only in deployments using Phase
  2 (multi-cluster) mode. See [I4](#i4-scout-cluster-wide-secret-read).

**Mitigations:**
- Same pod-hardening posture as the main operator (non-root, read-only rootfs, seccomp)
- Zone-authorization check gates `ARecord` creation (same-namespace or explicit
  allow-list — prevents cross-tenant DNS hijack via Scout)
- ✅ Secret access scoped to a namespaced, `resourceNames`-restricted Role (M-25, fixed)
- ✅ Namespace whitelisting available (`--namespace-selector`, M-30, opt-in) to bound
  which namespaces Scout's patch/update reach extends to
- ❌ **MISSING**: Field-level admission constraining *what* Scout can patch on
  Ingress/Service/route objects (M-28)

**Risk:** MEDIUM (down from HIGH). The cluster-wide `patch`/`update` on
Ingress/Service/route types (T4) is the remaining open item for this component —
see T4, [Trust Boundary 6](#boundary-6-scout-controller))

---

## Threat Scenarios

### Scenario 1: Compromised Operator Pod

**Severity:** HIGH

**Attack Path:**
1. Attacker exploits vulnerability in operator code (e.g., memory corruption, logic bug)
2. Gains code execution in operator pod
3. Reads ServiceAccount token from `/var/run/secrets/`
4. Uses token to modify DNSZone CRs or read RNDC keys from Secrets

**Impact:**
- Attacker can modify DNS records (redirect traffic)
- Attacker can disrupt DNS service (delete zones, BIND9 pods)
- Attacker can pivot to other namespaces (if RBAC is weak)

**Mitigations:**
- Operator runs as non-root, read-only filesystem
- RBAC least privilege (no delete permissions)
- Resource limits prevent resource exhaustion
- Vulnerability scanning (cargo-audit, Trivy)
- Network policies (planned - L-1)

**Residual Risk:** MEDIUM (need network policies)

---

### Scenario 2: DNS Cache Poisoning

**Severity:** MEDIUM

**Attack Path:**
1. Attacker sends forged DNS responses to recursive resolver
2. Resolver caches malicious record (e.g., A record for bank.com pointing to attacker IP)
3. Clients query resolver, receive poisoned response
4. Traffic redirected to attacker (phishing, MITM)

**Impact:**
- Users redirected to malicious sites
- Credentials stolen
- Man-in-the-middle attacks

**Mitigations:**
- DNSSEC (planned) - cryptographically signs DNS responses
- BIND9 is authoritative-only (not vulnerable to cache poisoning)
- Recursive resolvers outside our control (client responsibility)

**Residual Risk:** MEDIUM (DNSSEC would eliminate this risk)

---

### Scenario 3: Supply Chain Attack via Malicious Dependency

**Severity:** CRITICAL

**Attack Path:**
1. Attacker compromises popular Rust crate (e.g., via compromised maintainer account)
2. Malicious code injected into crate update
3. Bindy operator depends on compromised crate
4. Malicious code runs in operator, exfiltrates secrets or modifies DNS zones

**Impact:**
- Complete compromise of DNS infrastructure
- Data exfiltration (secrets, zone data)
- Backdoor access to cluster

**Mitigations:**
- Dependency scanning (cargo-audit) - C-3
- SBOM generation (track all dependencies)
- Signed commits (code changes traceable)
- Dependency version pinning in `Cargo.lock`
- Manual review for major dependency updates

**Residual Risk:** LOW (strong supply chain controls)

---

### Scenario 4: Insider Threat (Malicious Admin)

**Severity:** HIGH

**Attack Path:**
1. Malicious cluster admin with `cluster-admin` RBAC role
2. Directly modifies DNSZone CRs to redirect traffic
3. Deletes audit logs to cover tracks
4. Exfiltrates RNDC keys from Secrets

**Impact:**
- DNS records modified without attribution
- Service disruption
- Data theft

**Mitigations:**
- GitOps workflow (changes via PRs, not direct kubectl)
- All changes require 2+ reviewers
- Immutable audit logs (planned - H-2)
- Secret access audit trail (planned - H-3)
- Separation of duties (no single admin has all access)

**Residual Risk:** MEDIUM (need H-2 and H-3)

---

### Scenario 5: DDoS Attack on DNS Infrastructure

**Severity:** CRITICAL

**Attack Path:**
1. Attacker launches volumetric DDoS attack (millions of queries/sec)
2. BIND9 pods overwhelmed, become unresponsive
3. DNS queries fail, causing outages for all dependent services

**Impact:**
- Complete DNS outage
- All services depending on DNS become unavailable
- Revenue loss, SLA violations

**Mitigations:**
- Rate limiting in BIND9
- Horizontal scaling (multiple secondaries)
- Resource limits (prevent total resource exhaustion)
- DDoS protection at edge (planned - CloudFlare, AWS Shield)
- Autoscaling (planned - HPA based on query load)

**Residual Risk:** MEDIUM (need edge DDoS protection)

---

### Scenario 6: Compromised Scout Pod

**Severity:** CRITICAL (historical) → **MEDIUM (current, post-M-25)**

**Status: steps 2–3 below (the Secret-exfiltration path) were closed 2026-07-19
(M-25).** Step 4 (cross-tenant patch/update tampering) remains a live, open,
MEDIUM-severity path — kept as the current scope of this scenario.

**Attack Path (historical, steps 2–3 no longer possible):**
1. Attacker exploits a vulnerability in Scout (dependency CVE, container escape) or
   steals its ServiceAccount token from a compromised node
2. ~~Scout's `ClusterRole` grants unscoped `secrets: get` across every namespace in
   the cluster~~ — **this RBAC rule no longer exists.** Scout's Secret access is now
   a namespaced, `resourceNames`-restricted Role scoped to the single Phase 2
   kubeconfig Secret (or no Secret access at all, in same-cluster-only deployments).
3. ~~Attacker uses a harvested credential to pivot~~ — no longer reachable; there is
   nothing to harvest beyond the one Secret Scout legitimately needs, if even that.
4. **(Still live)** Scout's cluster-wide `patch`/`update` on `Ingress`/`Service`/route
   objects could be used to tamper with another tenant's networking configuration —
   see [T4](#t4-cross-tenant-tampering-via-scouts-cluster-wide-write-rbac).

**Impact (current scope, step 4 only):**
- Cross-tenant tampering with Ingress/Service/route objects in namespaces Scout has
  no legitimate reason to touch that day
- No confidentiality impact — Scout can no longer read Secrets beyond its own narrow need

**Mitigations:**
- Same pod-hardening posture as the main operator (non-root, read-only rootfs, seccomp)
- Zone-authorization check limits what DNS records Scout can actually create
- ✅ **Secret RBAC scoped (M-25, fixed 2026-07-19):** closes the confidentiality half
  of this scenario entirely (formerly steps 2–3 above).
- ✅ **Namespace whitelisting (M-30, opt-in):** when `--namespace-selector` is
  configured, Scout's own reconcile logic only acts on objects in labeled
  namespaces — bounds step 4 during *normal operation*. Enforced by Scout's
  reconciler code, not RBAC, so a directly-held stolen token is unaffected by this
  control alone.
- ❌ **MISSING**: Field-level admission constraining what Scout may patch (M-28)
- ❌ **MISSING**: Egress NetworkPolicy limiting Scout to the Kubernetes API only (M-27)

**Residual Risk:** **MEDIUM** (down from CRITICAL). The confidentiality path is
closed; the remaining risk is bounded to cross-tenant `Ingress`/`Service`/route
tampering (T4), not cluster-wide Secret exposure.

---

## Mitigations

### Existing Mitigations (Implemented)

| ID | Mitigation | Threats Mitigated | Compliance |
|----|------------|-------------------|------------|
| M-01 | Signed commits required | S3 (spoofed commits) | ✅ C-1 |
| M-02 | RBAC least privilege | E2 (privilege escalation) | ✅ C-2 |
| M-03 | Vulnerability scanning | I3 (CVE disclosure), E3 (dependency exploit) | ✅ C-3 |
| M-04 | Non-root containers | E1 (container escape) | ✅ Pod Security |
| M-05 | Read-only filesystem | T2 (tampering), E1 (escape) | ✅ Pod Security |
| M-06 | Secrets encrypted at rest | I1 (RNDC key disclosure) | ✅ Kubernetes |
| M-07 | AXFR restricted to secondaries | I2 (zone enumeration) | ✅ BIND9 config |
| M-08 | Rate limiting (BIND9) | D1 (DNS query flood) | ✅ BIND9 config |
| M-09 | SBOM generation | T2 (supply chain) | ✅ SLSA Level 2 |
| M-10 | Chainguard zero-CVE images | I3 (CVE disclosure) | ✅ Container security |
| M-21 | **B-5 Secret RBAC split** (2026-06-30): operator's cluster-wide `ClusterRole` is read-only on Secrets; mutating verbs moved to a namespaced Role bound only in the operator's own namespace | T3 (Secret tampering), E2 (privilege escalation) | ✅ RBAC |
| M-22 | **Namespace-scoped operator mode** (opt-in via `BINDY_WATCH_NAMESPACES`): when set, operator uses per-namespace RoleBindings instead of a cluster-wide ClusterRoleBinding, eliminating cluster-wide Secret/workload access | E2, R2, I1 | ⚠️ Opt-in — default is still cluster-wide |
| M-23 | **Unprivileged DNS port + capability drop**: BIND9 operand binds container port 5353 (Service still exposes 53) and adds zero Linux capabilities (`NET_BIND_SERVICE` removed) | E1 (container escape) | ✅ Pod Security |
| M-24 | **ValidatingAdmissionPolicy suite** (16 policies as of 2026-07-01): ACL syntax, zone-name validation, RNDC strictness, operand pod shape, DNSSEC policy, operator-workload ServiceAccount identity, DNS record value validation, image provenance, and `volumeMount.mountPath` allow-listing (`safe_volume.rs`, closes audit finding F-001) | T1 (DNS tampering), T3 (ConfigMap/Secret tampering), E1 (container escape via malicious volume mounts), T2 (image provenance) | ✅ Kubernetes VAP — supersedes M-13 below |
| M-30 | **Scout namespace whitelisting** (`--namespace-selector` / `BINDY_SCOUT_NAMESPACE_SELECTOR`, new in v1.1): a source object's namespace must match a configured Kubernetes label selector *in addition to* the object's own opt-in annotation before Scout acts on it. Label-selector matching delegates to the API server (no client-side selector parser). Reduces Scout's day-to-day operating footprint — bounds T4 (cross-tenant patch/update) during normal operation. **Does not reduce the `ClusterRole`'s RBAC ceiling** for Ingress/Service/route types — a directly compromised token is unaffected for those. **Opt-in — unset by default**, matching pre-v1.1 behavior for backward compatibility; Scout logs a startup warning when unset. See the Scout guide's "Namespace Whitelisting" section for the rollout/migration note. | T4 (partial) | ⚠️ Opt-in, recommended for all production deployments |
| M-25 | **Scout Secret RBAC scoped** (fixed 2026-07-19, same day as this finding's discovery): removed the cluster-wide `secrets: get` `PolicyRule` from the `bindy-scout` `ClusterRole` entirely. Replaced with a namespaced, `resourceNames`-restricted Role (`bindy-scout-secrets-reader`) scoped to exactly the one Phase 2 kubeconfig Secret, applied only when `--remote-secret` is configured. Same-cluster-only deployments (the default) now get zero Secret access. See I4/E4/Scenario 6 for the full before/after. | I4, E4, T4 (Secret-read component), Scenario 6 | ✅ RBAC — **was the highest-priority open item in v1.1; closed same-day** |

---

### Planned Mitigations (Roadmap)

| ID | Mitigation | Threats Mitigated | Priority | Roadmap Item |
|----|------------|-------------------|----------|--------------|
| M-11 | Audit log retention policy | R1 (non-repudiation) | HIGH | H-2 |
| M-12 | Secret access audit trail | R2 (secret access), I1 (disclosure) | HIGH | H-3 |
| ~~M-13~~ | ~~Admission webhooks~~ **DONE — see M-24** | T1 (DNS tampering) | — | Completed |
| M-14 | DNSSEC signing | T1 (tampering), Scenario 2 (cache poisoning) | MEDIUM | Future |
| M-15 | Image digest pinning | T2 (image tampering) | MEDIUM | M-1 |
| M-16 | Rate limiting (operator) | D2 (operator exhaustion) | MEDIUM | M-3 |
| M-17 | Network policies — a reference manifest now exists (`deploy/pod-hardening.yaml`, ingress/egress scoped to container port 5353) but is **not applied by any install target**; remains opt-in/manual | S1 (API spoofing), E1 (lateral movement), T4/E4 (Scout egress) | LOW | L-1 |
| M-18 | DDoS edge protection | D1 (DNS query flood) | HIGH | External |
| M-19 | RNDC key rotation | I1 (key disclosure) | MEDIUM | Future |
| M-20 | TSIG for AXFR | I2 (zone enumeration) | MEDIUM | Future |
| ~~M-25~~ | ~~Scope Scout's `secrets: get` to a namespaced Role~~ **DONE — see Existing Mitigations table above.** Fixed same-day as discovery (2026-07-19), before this revision was published. | I4, E4, T4, Scenario 6 | — | **Completed (v1.1)** |
| ~~M-26~~ | ~~Namespace-scoping option for Scout~~ **DONE — see M-30** (`--namespace-selector`) | T4 | — | Completed (v1.1) |
| M-27 | Egress NetworkPolicy for Scout (API server + remote-cluster API only) | E4 | MEDIUM | New (v1.1) |
| M-28 | Field-level admission policy constraining what Scout may `patch` on Ingress/Service/route objects (e.g. only finalizer/annotation fields) | T4 | MEDIUM | New (v1.1) |
| M-29 | Revisit Dependabot auto-merge: consider requiring a human approval step for patch/minor merges, or expand e2e coverage to compensate | E3 (dependency exploit via unreviewed auto-merge) | MEDIUM | New (v1.1) |

---

## Residual Risks

### Critical Residual Risks

None identified. **Scout's cluster-wide Secret read (I4/E4/Scenario 6)** — which
had CRITICAL impact and essentially no compensating control — was identified and
fixed the same day (2026-07-19), before this revision was published. See the
Existing Mitigations table (M-25) and I4/E4/Scenario 6 for the full record. No
other CRITICAL-impact threat in this document currently lacks a strong mitigation.

---

### High Residual Risks

1. **DDoS Attacks (D1)** - Risk reduced by rate limiting and horizontal scaling, but edge DDoS protection is needed for volumetric attacks (100+ Gbps).

2. **Insider Threats (Scenario 4)** - Risk reduced by GitOps and RBAC, but immutable audit logs (H-2) and secret access audit trail (H-3) are needed for full non-repudiation.

---

### Medium Residual Risks

1. **DNS Tampering (T1)** - Substantially reduced by RBAC and, as of 2026-07-01, a 16-policy `ValidatingAdmissionPolicy` suite (M-24) covering ACLs, zone names, RNDC strictness, pod shape, and record values. DNSSEC signing remains the main outstanding defense-in-depth gap for in-transit tampering (Scenario 2).

2. **Operator Resource Exhaustion (D2)** - Risk reduced by resource limits, but rate limiting (M-3) and admission webhooks are needed.

3. **Zone Enumeration (I2)** - Risk reduced by AXFR restrictions, but TSIG authentication would eliminate AXFR abuse.

4. **Compromised Operator Pod (Scenario 1)** - Risk reduced by Pod Security Standards, but network policies (L-1) would prevent lateral movement. A reference NetworkPolicy manifest now exists (`deploy/pod-hardening.yaml`) but is not applied by any install target.

5. **Cross-Tenant Tampering via Scout (T4)** - Bounded by patch/update-only RBAC scope and, when configured, namespace whitelisting (M-30, opt-in — not on by default). No field-level admission control (M-28) yet constrains what Scout can patch. (Scout's Secret-read risk, formerly part of this component's overall exposure, was resolved separately — see M-25.)

---

## Security Architecture

### Defense in Depth Layers

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 7: Monitoring & Response                             │
│  - Audit logs (Kubernetes API)                              │
│  - Vulnerability scanning (daily)                           │
│  - Incident response playbooks                              │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 6: Application Security                              │
│  - Input validation (CRD schemas)                           │
│  - Least privilege RBAC                                     │
│  - Signed commits (non-repudiation)                         │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 5: Container Security                                │
│  - Non-root user (uid 1000+)                                │
│  - Read-only filesystem                                     │
│  - No privileged capabilities                               │
│  - Vulnerability scanning (Trivy)                           │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 4: Pod Security                                      │
│  - Pod Security Standards (Restricted)                      │
│  - seccomp profile (restrict syscalls)                      │
│  - AppArmor/SELinux profiles                                │
│  - Resource limits (CPU/memory)                             │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 3: Namespace Isolation                               │
│  - RBAC (namespace-scoped roles)                            │
│  - Network policies (planned)                               │
│  - Resource quotas                                          │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 2: Cluster Security                                  │
│  - etcd encryption at rest                                  │
│  - API server authentication/authorization                  │
│  - Secrets management                                       │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 1: Infrastructure Security                           │
│  - Node OS hardening (managed by platform team)             │
│  - Network segmentation                                     │
│  - Physical security                                        │
└─────────────────────────────────────────────────────────────┘
```

---

## Security Controls Summary

| Control Category | Implemented | Planned | Residual Risk |
|------------------|-------------|---------|---------------|
| **Access Control** | RBAC least privilege (main operator), signed commits, B-5 Secret RBAC split, namespace-scoped operator mode (opt-in), 16 `ValidatingAdmissionPolicy` policies, Scout namespace whitelisting (opt-in, M-30), **Scout Secret RBAC scoped (M-25, fixed 2026-07-19)** | Field-level admission for Scout patches (M-28), Scout egress NetworkPolicy (M-27) | MEDIUM — driven by Scout's remaining cluster-wide `patch`/`update` on Ingress/Service/route (T4); the formerly-HIGH Secret-read risk (I4/E4) is resolved |
| **Data Protection** | Secrets encrypted, AXFR restricted | DNSSEC, TSIG | MEDIUM |
| **Supply Chain** | Signed commits/images, SBOM, vuln scanning | Image digest pinning; revisit Dependabot auto-merge human-review gap (M-29) | LOW-MEDIUM (automated auto-merge removed a manual checkpoint — see E3) |
| **Monitoring** | Kubernetes audit logs, vuln scanning | Audit retention policy, secret access trail | MEDIUM |
| **Resilience** | Rate limiting, resource limits | Edge DDoS protection, HPA | MEDIUM |
| **Container Security** | Non-root, read-only FS, Pod Security Standards, unprivileged DNS port + zero added capabilities (M-23) | Network policies (reference manifest exists, not auto-applied — M-17) | LOW |

---

## References

- [OWASP Threat Modeling](https://owasp.org/www-community/Threat_Modeling)
- [Microsoft STRIDE Methodology](https://learn.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- [Kubernetes Threat Model](https://github.com/kubernetes/community/blob/master/sig-security/security-audit-2019/findings/Kubernetes%20Threat%20Model.pdf)
- [NIST SP 800-154 - Guide to Data-Centric System Threat Modeling](https://csrc.nist.gov/publications/detail/sp/800-154/draft)

---

**Last Updated:** 2026-07-19
**Next Review:** 2026-10-19 (Quarterly)
**Approved By:** Security Team *(pending re-approval for v1.1 — this revision has not yet been formally reviewed/signed off; see the revision note at the top of this document)*
