# bindcar `v0.6.0` → `main` (0.7.0) — bindy Integration & Upgrade Guide

> **Status:** 📄 Reference — **superseded** by 56. Read
> [`56-bindcar-migration-v0-7-4.md`](56-bindcar-migration-v0-7-4.md) instead; this is kept
> for the v0.6.0→v0.7.0 detail it carries and for the audit trail.
>
> *Migrated 2026-09-12 from the external roadmap set into `.github/community/`.*

> Purpose: everything the **bindy** operator must change/run/verify to consume bindcar `0.7.0`.
> Built from the actual `git diff v0.6.0..main`. There is no `v0.7.0` tag yet — this reflects current `main` / `Cargo.toml version = 0.7.0`.

---

## 0. Which sections apply to you? (read this first)

The **published release binary / default container image is built WITHOUT `--features k8s-token-review`**
(`default = []` in `Cargo.toml`; the `build-binary` CI step passes no features). So:

| Your deployment | Applies |
|---|---|
| **Mode A** — default image, basic / shared-secret auth (most bindy deployments) | Sections 1, 3–10 |
| **Mode B** — you build bindcar with `--features k8s-token-review` (in-cluster TokenReview) | All sections, **including 2** |

Severity legend: 🔴 **hard break** (bindcar won't start / request fails) · 🟠 deploy change · 🟢 informational.

---

## 1. 🔴 bindcar now refuses to start without real auth (new startup guard, "B-4")

**This is the #1 thing that will break your rollout.** At v0.6.0 bindcar booted with presence-only auth.
Now, on a non-loopback bind (your pod's `0.0.0.0:8080`), it **refuses to start** unless one of:

| Option | What to do |
|---|---|
| **Shared-secret auth (recommended for Mode A)** | Set env `BIND_API_TOKEN=<random-secret>` on the bindcar container **and** have bindy send `Authorization: Bearer <same-secret>` on every request. |
| TokenReview build | Use Mode B (Section 2). |
| Escape hatch | Pass CLI flag `--i-know-this-is-insecure` (or bind to loopback). **Not for production.** |

**Action (Mode A) — bindcar sidecar env in the bindy pod template:**
```yaml
env:
  - name: BIND_API_TOKEN
    valueFrom:
      secretKeyRef:
        name: bindcar-api-token
        key: token
```

**Action — bindy HTTP client, add to every bindcar request:**
```go
req.Header.Set("Authorization", "Bearer "+apiToken)
```

Notes:
- The comparison is constant-time over a SHA-256 digest, so token length does not leak.
- Empty / missing `Authorization` header → `401`. Malformed (`Bearer` prefix missing) → `401`.

---

## 2. 🔴 (Mode B only) TokenReview: audience enforced + allowlists fail-closed

Only if you run bindcar built with `--features k8s-token-review`.

### 2a. Audience is now enforced (was advisory)
bindcar verifies the token's `status.audiences` from the TokenReview response. A **default ServiceAccount
token (apiserver audience) is now rejected.** Mint tokens for the `bindcar` audience:

```yaml
# bindy pod: projected token volume for the SA that calls bindcar
volumes:
  - name: bindcar-token
    projected:
      sources:
        - serviceAccountToken:
            audience: bindcar          # must match BIND_TOKEN_AUDIENCES (default "bindcar")
            expirationSeconds: 3600
            path: token
```
…or set `BIND_TOKEN_AUDIENCES` to the audience your tokens actually carry.

### 2b. Allowlists are fail-closed (was allow-all)
With **empty** `BIND_ALLOWED_NAMESPACES` **and** `BIND_ALLOWED_SERVICE_ACCOUNTS`, bindcar now **refuses to start**.
Configure at least one:

```yaml
env:
  - name: BIND_ALLOWED_SERVICE_ACCOUNTS
    value: "system:serviceaccount:bindy-system:bindy-controller"   # your operator SA
  # and/or:
  - name: BIND_ALLOWED_NAMESPACES
    value: "bindy-system"
  # explicit opt-out (NOT recommended): BIND_ALLOW_ANY_SERVICEACCOUNT=true
```

---

## 3. 🔴 Stricter API request validation — payloads that passed at v0.6.0 now return HTTP 400

At v0.6.0 **none** of the zone validators existed and record validation was minimal. bindy request bodies
must now satisfy these or receive `400 Bad Request`:

| Field (endpoint) | New rule |
|---|---|
| `zoneName` / `{name}` path param (all zone endpoints) | `[A-Za-z0-9._-]` only, must start alphanumeric, no `..`, ≤ 253 chars |
| record `name` (create-zone records + `/records`) | `[A-Za-z0-9._-@*]` only |
| record `value` | control chars (`\n \r \0`) rejected for **all** types; `A` → valid IPv4, `AAAA` → valid IPv6, `CNAME`/`NS`/`PTR`/`MX` → must end with `.` |
| `primaries`, `alsoNotify`, `allowTransfer`, `allowUpdate` | every entry must parse as a valid IP address |
| `updateKeyName`, `dnssecPolicy` | `[A-Za-z0-9._-]`, ≤ 253 chars |
| SOA `primaryNs` / `adminEmail`, `nameServers[]`, `nameServerIps` keys | `[A-Za-z0-9._-]` (FQDN with trailing dot OK); `nameServerIps` values must be valid IPs |

**Most common bites for bindy:**
- Sending a `CNAME`/`MX`/`NS`/`PTR` `value` **without** the trailing `.`.
- Putting a hostname (non-IP) into `primaries` / `allowTransfer` / `alsoNotify`.
- Any accidental whitespace/control char in a name or value.

**Action:** audit how bindy constructs these fields before serializing the request.

---

## 4. 🔴 RNDC / nsupdate key must be SHA-2 + config hygiene

- **`hmac-md5` and `hmac-sha1` are rejected** — bindcar will not create the RNDC executor. The BIND9 key
  (`rndc.key` / `RNDC_ALGORITHM` / `NSUPDATE_ALGORITHM`) must be `hmac-sha256` (or `sha224`/`sha384`/`sha512`).
- **Multi-key `rndc.conf` now requires an explicit `default-key`** (no more arbitrary selection) — errors otherwise.
- **Empty RNDC secret is now rejected** (fail-fast instead of silent empty-secret client).

**Action:** ensure the BIND9 key bindy provisions uses `algorithm hmac-sha256;`.

---

## 5. 🟠 Apply the new RBAC (Mode B)

`deploy/rbac.yaml` no longer binds the built-in `system:auth-delegator`. It now ships a purpose-built
ClusterRole `bindcar-tokenreview` granting only `create tokenreviews` (drops the unused
`subjectaccessreviews` recon primitive).

**Action:** re-apply `deploy/rbac.yaml`; remove any old binding to `system:auth-delegator` you templated.
Mode A needs no cluster RBAC — keep `automountServiceAccountToken: false` on the SA.

```yaml
# deploy/rbac.yaml (excerpt)
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindcar-tokenreview
rules:
  - apiGroups: ["authentication.k8s.io"]
    resources: ["tokenreviews"]
    verbs: ["create"]
```

---

## 6. 🟠 Pod must satisfy Pod Security Admission `restricted`

`deploy/pod-hardening.yaml` now ships an **enforced** namespace label
`pod-security.kubernetes.io/enforce: restricted`. Your bindy-managed pod template must comply or admission
**rejects the pod**:

- `runAsNonRoot: true`, `runAsUser: 65532`
- `allowPrivilegeEscalation: false`
- `capabilities.drop: ["ALL"]`
- `seccompProfile.type: RuntimeDefault`
- `readOnlyRootFilesystem: true` → **mount a writable `emptyDir` at `/tmp` and set `TMPDIR`** (bindcar writes
  a 0600 TSIG key file there for `nsupdate -k`).
- The **`named` container** may add back only `NET_BIND_SERVICE` (allowed under `restricted`) for port 53.

```yaml
# bindcar sidecar container securityContext
securityContext:
  allowPrivilegeEscalation: false
  readOnlyRootFilesystem: true
  runAsNonRoot: true
  runAsUser: 65532
  capabilities: { drop: ["ALL"] }
  seccompProfile: { type: RuntimeDefault }
volumeMounts:
  - { name: tmp, mountPath: /tmp }
volumes:
  - { name: tmp, emptyDir: { medium: Memory } }
# env: TMPDIR=/tmp
```

> If the bindy operator owns the `bindy-system` namespace, apply the PSA labels to its manifest rather than
> shipping a competing Namespace object (or `kubectl label ns bindy-system pod-security.kubernetes.io/enforce=restricted --overwrite`).

---

## 7. 🟠 NetworkPolicy egress needs a real API-server CIDR

`deploy/pod-hardening.yaml` egress is now scoped (no more `0.0.0.0/0`). It ships a **fail-closed placeholder**
`10.0.0.1/32` for the Kubernetes API server.

**Action:** replace `10.0.0.1/32` with your kube-apiserver endpoint/CIDR — otherwise TokenReview egress
(Mode B) is denied. DNS egress is scoped to `kube-system`; add your secondary/forwarder CIDRs if `named`
peers externally.

---

## 8. 🟠 Swagger / OpenAPI are OFF by default

`/api/v1/docs` (Swagger UI) and `/api/v1/openapi.json` are no longer served unless `BIND_ENABLE_DOCS=true`.

**Action:** if bindy or your CI pulls the OpenAPI spec from a running bindcar, set `BIND_ENABLE_DOCS=true`
(dev only) or vendor the spec at build time. `/api/v1/health`, `/api/v1/ready`, `/metrics` are unchanged.

---

## 9. 🟢 Rust crate consumers (if bindy imports the `bindcar` crate for shared types)

- **Version bump:** `bindcar = "0.7"` (was `0.6`). **No public type or route signatures changed** —
  `CreateZoneRequest`, `ZoneConfig`, `SoaRecord`, `DnsRecord`, the record request/response types, and every
  endpoint have identical shapes. Your serialization code is source-compatible.
- **Dependency alignment (only if you enable `k8s-token-review`):** bindcar moved `kube` 3.0 → **4.0** and
  `k8s-openapi` 0.27 → **0.28**. If bindy shares these, bump together. kube 4.0's only breaking change that
  touched bindcar is the flattened `other` field on kubeconfig `Named*` wrappers — initialize with
  `..Default::default()`. New transitive dep: `sha2 = "0.10"`.
- `RndcConfig` / `KeyBlock` now have a **redacting `Debug`** (`secret` prints `[REDACTED]`). They still
  implement `Debug`, so no compile break — just don't expect the secret in debug output.

---

## 10. 🟢 Container image

`docker/Dockerfile.chainguard` is re-pinned to the multi-arch manifest-list digest
`@sha256:ea9eab0adc5716fb9937ab60155a31bce9cbc8b56e6f2e21fb9af9218be195b7` (OCI image index, amd64 + arm64).
No action unless you pin bindcar's base image yourself downstream.

---

## Environment variable reference (v0.6.0 → 0.7.0)

| Variable | Status | Notes |
|---|---|---|
| `BIND_API_TOKEN` | **NEW** | Shared-secret auth. Set it (Mode A) to satisfy the startup guard. |
| `BIND_ALLOW_ANY_SERVICEACCOUNT` | **NEW** | Mode B opt-out of fail-closed allowlists. Avoid in prod. |
| `BIND_ENABLE_DOCS` | **NEW** | `true` to serve Swagger/OpenAPI (off by default). |
| `BIND_TOKEN_AUDIENCES` | changed behavior | Default `bindcar`; now **enforced** against `status.audiences` (Mode B). |
| `BIND_ALLOWED_NAMESPACES` | changed behavior | Empty is now **fail-closed** at startup (Mode B). |
| `BIND_ALLOWED_SERVICE_ACCOUNTS` | changed behavior | Empty is now **fail-closed** at startup (Mode B). |
| `RNDC_ALGORITHM` / `NSUPDATE_ALGORITHM` | changed behavior | `hmac-md5` / `hmac-sha1` now **rejected**; SHA-2 only. |
| `DISABLE_AUTH` | unchanged | Still gated by the startup guard on non-loopback binds. |
| `BIND_ZONE_DIR`, `RNDC_*`, `NSUPDATE_*`, `KUBE_*` | unchanged | — |

---

## Pre-upgrade checklist for bindy

- [ ] **(Mode A)** Set `BIND_API_TOKEN` on bindcar **and** send it as `Bearer` from bindy — else bindcar won't start.
- [ ] **(Mode B)** Mint `audience: bindcar` tokens + set `BIND_ALLOWED_SERVICE_ACCOUNTS`.
- [ ] Verify every zone name, record name/value, and IP-list field bindy sends passes the Section 3 rules.
- [ ] Ensure the BIND9 RNDC key uses `hmac-sha256`.
- [ ] **(Mode B)** Re-apply `deploy/rbac.yaml`.
- [ ] Make the pod template PSA-`restricted` compliant + writable `/tmp` (`TMPDIR`).
- [ ] Set the real API-server CIDR in the egress NetworkPolicy.
- [ ] Set `BIND_ENABLE_DOCS=true` only if you consume the live OpenAPI spec.
- [ ] Bump the `bindcar` crate dep to `0.7` (and `kube`/`k8s-openapi` if you enable TokenReview).

---

## Commit range covered

`v0.6.0 (7080edc)` → current `main` (0.7.0), including PRs #46, #47, #64, #66, #67, #68 plus the
`security-sweep` remediation (findings A1–A20). The rollout-blocking change is the **auth startup guard
(Section 1)**.
