# Threat Model - Bindy DNS Controller

**Version:** 1.0
**Last Updated:** 2025-12-17
**Owner:** Security Team
**Compliance:** SOX 404, PCI-DSS 6.4.1, Basel III Cyber Risk

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

This document provides a comprehensive threat model for the Bindy DNS Controller, a Kubernetes operator that manages BIND9 DNS servers. The threat model uses the STRIDE methodology (Spoofing, Tampering, Repudiation, Information Disclosure, Denial of Service, Elevation of Privilege) to identify and analyze security threats.

### Objectives

1. **Identify threats** to the DNS infrastructure managed by Bindy
2. **Assess risk** for each identified threat
3. **Document mitigations** (existing and required)
4. **Provide security guidance** for deployers and operators
5. **Support compliance** with SOX 404, PCI-DSS 6.4.1, Basel III

### Scope

**In Scope:**
- Bindy controller container and runtime
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
│  │              dns-system Namespace                   │    │
│  │                                                      │    │
│  │  ┌──────────────────────────────────────────────┐  │    │
│  │  │        Bindy Controller (Deployment)         │  │    │
│  │  │  ┌────────────────────────────────────────┐  │  │    │
│  │  │  │  Controller Pod (Non-Root, ReadOnly)   │  │    │    │
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

1. **Bindy Controller**
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
   - Services: Expose DNS (port 53) and RNDC (port 9530)
   - ServiceAccounts: RBAC for controller access

---

## Assets

### High-Value Assets

| Asset | Description | Confidentiality | Integrity | Availability | Owner |
|-------|-------------|-----------------|-----------|--------------|-------|
| **DNS Zone Data** | Authoritative DNS records for all managed domains | Medium | **Critical** | **Critical** | Teams/Platform |
| **RNDC Keys** | Symmetric HMAC keys for BIND9 control | **Critical** | **Critical** | High | Security Team |
| **Controller Binary** | Signed container image with controller logic | Medium | **Critical** | High | Development Team |
| **BIND9 Configuration** | named.conf, zone configs | Low | **Critical** | High | Platform Team |
| **Kubernetes API Access** | ServiceAccount token for controller | **Critical** | **Critical** | **Critical** | Platform Team |
| **CRD Schemas** | Define API contract for DNS management | Low | **Critical** | Medium | Development Team |
| **Audit Logs** | Record of all DNS changes and access | High | **Critical** | High | Security Team |
| **SBOM** | Software Bill of Materials for compliance | Low | **Critical** | Medium | Compliance Team |

### Asset Protection Goals

- **DNS Zone Data**: Prevent unauthorized modification (tampering), ensure availability
- **RNDC Keys**: Prevent disclosure (compromise allows full BIND9 control)
- **Controller Binary**: Prevent supply chain attacks, ensure code integrity
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
- Controller can be manipulated or replaced

---

### Boundary 2: dns-system Namespace

**Trust Level:** High
**Description:** Namespace containing Bindy controller and BIND9 pods

**Assumptions:**
- RBAC limits access to authorized ServiceAccounts only
- Secrets are encrypted at rest in etcd
- Pod Security Standards enforced (Restricted)

**Threats if Compromised:**
- Attacker can read RNDC keys
- Attacker can modify DNS zones
- Attacker can disrupt DNS service

---

### Boundary 3: Controller Container

**Trust Level:** Medium-High
**Description:** Bindy controller runtime environment

**Assumptions:**
- Container runs as non-root user
- Filesystem is read-only except /tmp
- No privileged capabilities
- Resource limits enforced

**Threats if Compromised:**
- Attacker can abuse Kubernetes API access
- Attacker can read secrets controller has access to
- Attacker can disrupt reconciliation loops

---

### Boundary 4: BIND9 Container

**Trust Level:** Medium
**Description:** BIND9 DNS server runtime

**Assumptions:**
- Container runs as non-root
- Exposed to internet (port 53)
- Configuration is managed by controller (read-only)

**Threats if Compromised:**
- Attacker can serve malicious DNS responses
- Attacker can exfiltrate zone data
- Attacker can pivot to other cluster resources (if network policies weak)

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

## STRIDE Threat Analysis

### S - Spoofing (Identity)

#### S1: Spoofed Kubernetes API Requests

**Threat:** Attacker impersonates the Bindy controller ServiceAccount to make unauthorized API calls.

**Impact:** HIGH
**Likelihood:** LOW (requires compromised cluster or stolen token)

**Attack Scenario:**
1. Attacker compromises a pod in the cluster
2. Steals ServiceAccount token from `/var/run/secrets/kubernetes.io/serviceaccount/token`
3. Uses token to impersonate controller and modify DNS zones

**Mitigations:**
- ✅ RBAC least privilege (controller cannot delete resources)
- ✅ Pod Security Standards (non-root, read-only filesystem)
- ✅ Short-lived ServiceAccount tokens (TokenRequest API)
- ❌ **MISSING**: Network policies to restrict egress from controller pod
- ❌ **MISSING**: Audit logging for all ServiceAccount API calls

**Residual Risk:** MEDIUM (need network policies and audit logs)

---

#### S2: Spoofed RNDC Commands

**Threat:** Attacker gains access to RNDC key and sends malicious commands to BIND9.

**Impact:** CRITICAL
**Likelihood:** LOW (RNDC keys stored in Kubernetes Secrets with RBAC)

**Attack Scenario:**
1. Attacker compromises controller pod or namespace
2. Reads RNDC key from Kubernetes Secret
3. Connects to BIND9 RNDC port (9530) and issues commands (e.g., `reload`, `freeze`, `thaw`)

**Mitigations:**
- ✅ Secrets encrypted at rest (Kubernetes)
- ✅ RBAC limits secret read access to controller only
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
3. Controller pulls compromised image on next rollout

**Mitigations:**
- ✅ All images signed with provenance attestation (SLSA Level 2)
- ✅ SBOM generated for all releases
- ✅ GitHub Actions signed commits verification
- ✅ Multi-stage builds minimize attack surface
- ❌ **MISSING**: Image digests pinned (not tags) - see M-1
- ❌ **MISSING**: Admission controller to verify image signatures (e.g., Sigstore Cosign)

**Residual Risk:** LOW (strong supply chain controls, but pinning digests would further reduce risk)

---

#### T3: Tampering with ConfigMaps/Secrets

**Threat:** Attacker modifies BIND9 configuration or RNDC keys via Kubernetes API.

**Impact:** HIGH
**Likelihood:** LOW (RBAC protects ConfigMaps/Secrets)

**Attack Scenario:**
1. Attacker gains elevated privileges in `dns-system` namespace
2. Modifies BIND9 ConfigMap to disable security features or add backdoor zones
3. BIND9 pod restarts with malicious configuration

**Mitigations:**
- ✅ Controller has NO delete permissions on Secrets/ConfigMaps (C-2)
- ✅ RBAC limits write access to controller only
- ✅ Immutable ConfigMaps (once created, cannot be modified - requires recreation)
- ❌ **MISSING**: ConfigMap/Secret integrity checks (hash validation)
- ❌ **MISSING**: Automated drift detection (compare running config vs desired state)

**Residual Risk:** MEDIUM (need integrity checks)

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
- ✅ BIND9 configuration managed by controller (prevents manual misconfig)
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

#### D2: Controller Resource Exhaustion

**Threat:** Attacker creates thousands of DNSZone CRs, overwhelming controller.

**Impact:** HIGH (controller fails, DNS updates stop)
**Likelihood:** LOW (requires cluster access)

**Attack Scenario:**
1. Attacker gains write access to Kubernetes API
2. Creates 10,000+ DNSZone CRs
3. Controller reconciliation queue overwhelms CPU/memory
4. Controller crashes or becomes unresponsive

**Mitigations:**
- ✅ Resource limits on controller pod
- ✅ Exponential backoff for failed reconciliations
- ❌ **MISSING**: Rate limiting on reconciliation loops (M-3)
- ❌ **MISSING**: Admission webhook to limit number of CRs per namespace
- ❌ **MISSING**: Horizontal scaling of controller (leader election)

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
- ✅ No privileged capabilities
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
- ✅ RBAC least privilege (controller has NO delete permissions) - C-2
- ✅ Automated RBAC verification script (`deploy/rbac/verify-rbac.sh`)
- ✅ No wildcard permissions in controller RBAC
- ✅ Regular RBAC audits (quarterly)
- ❌ **MISSING**: RBAC policy-as-code validation (OPA/Gatekeeper)

**Residual Risk:** VERY LOW (strong RBAC controls)

---

#### E3: Exploiting Vulnerable Dependencies

**Threat:** Attacker exploits vulnerability in Rust dependency to gain code execution.

**Impact:** HIGH
**Likelihood:** LOW (automated vulnerability scanning, rapid patching)

**Attack Scenario:**
1. CVE disclosed in dependency (e.g., `tokio`, `hyper`, `kube`)
2. Attacker crafts malicious Kubernetes API response to trigger vulnerability
3. Controller crashes or attacker gains RCE in controller pod

**Mitigations:**
- ✅ Automated vulnerability scanning (cargo-audit) - C-3
- ✅ CI blocks on CRITICAL/HIGH vulnerabilities
- ✅ Remediation SLAs enforced (CRITICAL: 24h)
- ✅ Daily scheduled scans
- ✅ Dependency updates via Dependabot

**Residual Risk:** LOW (excellent vulnerability management)

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

**Exposure:** External (internet-facing)
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
- Input sanitization in controller
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

## Threat Scenarios

### Scenario 1: Compromised Controller Pod

**Severity:** HIGH

**Attack Path:**
1. Attacker exploits vulnerability in controller code (e.g., memory corruption, logic bug)
2. Gains code execution in controller pod
3. Reads ServiceAccount token from `/var/run/secrets/`
4. Uses token to modify DNSZone CRs or read RNDC keys from Secrets

**Impact:**
- Attacker can modify DNS records (redirect traffic)
- Attacker can disrupt DNS service (delete zones, BIND9 pods)
- Attacker can pivot to other namespaces (if RBAC is weak)

**Mitigations:**
- Controller runs as non-root, read-only filesystem
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
3. Bindy controller depends on compromised crate
4. Malicious code runs in controller, exfiltrates secrets or modifies DNS zones

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

---

### Planned Mitigations (Roadmap)

| ID | Mitigation | Threats Mitigated | Priority | Roadmap Item |
|----|------------|-------------------|----------|--------------|
| M-11 | Audit log retention policy | R1 (non-repudiation) | HIGH | H-2 |
| M-12 | Secret access audit trail | R2 (secret access), I1 (disclosure) | HIGH | H-3 |
| M-13 | Admission webhooks | T1 (DNS tampering) | MEDIUM | Future |
| M-14 | DNSSEC signing | T1 (tampering), Scenario 2 (cache poisoning) | MEDIUM | Future |
| M-15 | Image digest pinning | T2 (image tampering) | MEDIUM | M-1 |
| M-16 | Rate limiting (controller) | D2 (controller exhaustion) | MEDIUM | M-3 |
| M-17 | Network policies | S1 (API spoofing), E1 (lateral movement) | LOW | L-1 |
| M-18 | DDoS edge protection | D1 (DNS query flood) | HIGH | External |
| M-19 | RNDC key rotation | I1 (key disclosure) | MEDIUM | Future |
| M-20 | TSIG for AXFR | I2 (zone enumeration) | MEDIUM | Future |

---

## Residual Risks

### Critical Residual Risks

None identified (all critical threats have strong mitigations).

---

### High Residual Risks

1. **DDoS Attacks (D1)** - Risk reduced by rate limiting and horizontal scaling, but edge DDoS protection is needed for volumetric attacks (100+ Gbps).

2. **Insider Threats (Scenario 4)** - Risk reduced by GitOps and RBAC, but immutable audit logs (H-2) and secret access audit trail (H-3) are needed for full non-repudiation.

---

### Medium Residual Risks

1. **DNS Tampering (T1)** - Risk reduced by RBAC, but admission webhooks and DNSSEC would provide defense-in-depth.

2. **Controller Resource Exhaustion (D2)** - Risk reduced by resource limits, but rate limiting (M-3) and admission webhooks are needed.

3. **Zone Enumeration (I2)** - Risk reduced by AXFR restrictions, but TSIG authentication would eliminate AXFR abuse.

4. **Compromised Controller Pod (Scenario 1)** - Risk reduced by Pod Security Standards, but network policies (L-1) would prevent lateral movement.

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
| **Access Control** | RBAC least privilege, signed commits | Admission webhooks | LOW |
| **Data Protection** | Secrets encrypted, AXFR restricted | DNSSEC, TSIG | MEDIUM |
| **Supply Chain** | Signed commits/images, SBOM, vuln scanning | Image digest pinning | LOW |
| **Monitoring** | Kubernetes audit logs, vuln scanning | Audit retention policy, secret access trail | MEDIUM |
| **Resilience** | Rate limiting, resource limits | Edge DDoS protection, HPA | MEDIUM |
| **Container Security** | Non-root, read-only FS, Pod Security Standards | Network policies | LOW |

---

## References

- [OWASP Threat Modeling](https://owasp.org/www-community/Threat_Modeling)
- [Microsoft STRIDE Methodology](https://learn.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- [Kubernetes Threat Model](https://github.com/kubernetes/community/blob/master/sig-security/security-audit-2019/findings/Kubernetes%20Threat%20Model.pdf)
- [NIST SP 800-154 - Guide to Data-Centric System Threat Modeling](https://csrc.nist.gov/publications/detail/sp/800-154/draft)

---

**Last Updated:** 2025-12-17
**Next Review:** 2025-03-17 (Quarterly)
**Approved By:** Security Team
