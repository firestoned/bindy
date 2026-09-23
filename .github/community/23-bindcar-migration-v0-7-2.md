# bindcar `v0.6.0` → `v0.7.2` — bindy Integration & Upgrade Guide

> **Status:** 📄 Reference — superseded by
> [`24-bindcar-migration-v0-7-4.md`](24-bindcar-migration-v0-7-4.md), which carries
> the v0.7.2 → v0.7.4 delta this guide stops short of.
>
> *Migrated 2026-09-12 from the external roadmap set into `.github/community/`.*

> Supersedes the v0.7.0 and v0.7.1 guides. Built from `git diff v0.6.0..v0.7.2`
> and verified against published artifacts (ghcr + crates.io + CI runs) on
> 2026-07-06.
>
> **bindy status:** v0.6.0→v0.7.1 fully absorbed (Mode B / TokenReview; see
> bindy `.claude/CHANGELOG.md` 2026-07-01…07-05). This guide's actionable
> content is the **v0.7.1→v0.7.2 delta** (§13–§14).

---

## 0. Version facts (verified 2026-07-06)

| | v0.7.0 | v0.7.1 | v0.7.2 |
|---|---|---|---|
| Tag | `14505e5` (07-01) | `29c71ef` (07-05) | `b4bdc01` (07-06) |
| `src/` delta from previous | (see §1–10) | **none** | `auth.rs`, `main.rs`, `auth_test.rs` |
| Release binary has TokenReview | ❌ | ❌ | ✅ **`--features k8s-token-review` shipped** |
| `nsupdate` in image | ❌ | ✅ | ✅ |
| ghcr images | ✅ | ✅ | ✅ `v0.7.2`, `v0.7.2-distroless` (CI green 07-06) |
| crates.io | 0.7.0 | 0.7.1 | 0.7.2 |

---

## 1–10. v0.6.0 → v0.7.0 — DONE in bindy

Auth startup guard, TokenReview audience + fail-closed allowlists, strict
request validation (plain-IP `primaries` → DNS moved to :53 + `NET_BIND_SERVICE`),
SHA-2-only RNDC, tokenreview RBAC, PSA `restricted` + `/tmp`/`TMPDIR`,
NetworkPolicy, docs-off-by-default, crate `0.7`. See the superseded v0.7.1
guide for the per-section table; regression-pinned by bindy's
`make regression-test`.

## 11–12. v0.7.0 → v0.7.1 — DONE in bindy

Packaging-only (`src/` byte-identical): images gained the `nsupdate` binary;
`DISABLE_AUTH` no longer baked as image ENV; CI consolidated. bindy bumped the
default image to `v0.7.1`. The Mode B blocker (feature not compiled) persisted
at v0.7.1 — fixed in v0.7.2 below.

---

## 13. 🟢 v0.7.2: release artifacts ship TokenReview (Mode B unblocked)

PR #71. `build.yaml`'s Build binary step now passes
`extra-args: "--features k8s-token-review"`, and the feature pins the
k8s-openapi API version in `Cargo.toml`
(`k8s-token-review = ["kube", "k8s-openapi", "k8s-openapi/v1_32"]`), removing
the `K8S_OPENAPI_ENABLED_VERSION` env from CI.

**Consequence for bindy:** `ghcr.io/firestoned/bindcar:v0.7.2` is the **first
published image deployable under bindy's Mode B configuration**. No more
custom builds; bindy's regression Phase B/C can run against the stock image.

**Action (bindy):** bump `DEFAULT_BINDCAR_IMAGE` → `…:v0.7.2` (+ the
`crd.rs` doc example, migration guide, CRD regen).

## 14. 🟠 v0.7.2: auth-mode selection changed — `BIND_API_TOKEN` now *disables* TokenReview

`src/auth.rs` / `src/main.rs`: shared-secret and TokenReview are now **mutually
exclusive, shared-secret wins**. In a feature-enabled binary:

| `BIND_API_TOKEN` on the sidecar | Behavior (v0.7.1, feature build) | Behavior (v0.7.2) |
|---|---|---|
| unset | TokenReview enforced; A2 fail-closed posture at startup | same ✅ |
| set (non-empty) | TokenReview **also** enforced on every request | **TokenReview skipped entirely; A2 posture check skipped; the shared secret is the only credential** |

bindy's own sidecar env does not set `BIND_API_TOKEN`, so the operator's
TokenReview configuration is untouched ✅.

**But this creates a new bypass via `bindcarConfig.envVars`** (user-writable on
`Bind9Instance`/`Bind9Cluster`/`ClusterBind9Provider`): user env vars are
appended *after* the operator-managed ones, and for duplicate names the kubelet
uses the **last** occurrence — so a tenant-supplied entry silently overrides
the operator's. As of v0.7.2:

- `BIND_API_TOKEN=<attacker-value>` → TokenReview off; attacker knows the
  secret; the operator's SA-token requests start failing 401 (DoS on zone ops)
  while the attacker has full API access.
- `DISABLE_AUTH=true`, `BIND_ALLOW_ANY_SERVICEACCOUNT=true` → auth weakened.
- `BIND_ALLOWED_SERVICE_ACCOUNTS` / `BIND_ALLOWED_NAMESPACES` /
  `BIND_TOKEN_AUDIENCES` → allowlist/audience tampering.
- `KUBE_API_SERVER` / `KUBE_*` → point TokenReview at a rogue API server that
  approves anything (full auth bypass).
- `RNDC_SECRET` / `RNDC_ALGORITHM` / `BIND_ZONE_DIR` → break or hijack the
  operator-managed RNDC/zone plumbing.

**Action (bindy) — close the override hole at both layers:**

1. **Reconciler**: `build_api_sidecar_container` must drop (and log) any
   user-supplied env var whose name is operator-reserved (list above, plus the
   `KUBE_` prefix) instead of appending it.
2. **Admission (VAP 15/16)**: reject `bindcarConfig.envVars` entries with
   reserved names on `Bind9Instance`/`Bind9Cluster`/`ClusterBind9Provider` at
   the API server, with test fixtures — same defense-in-depth pattern as
   VAP 01–14.

(Before v0.7.2 this was a lower-severity concern — an injected
`BIND_API_TOKEN` couldn't turn TokenReview off. The mode-selection change is
what promotes it to a real bypass.)

---

## Pre-upgrade checklist for bindy (v0.7.1 → v0.7.2)

- [ ] `src/constants.rs`: `DEFAULT_BINDCAR_IMAGE` → `…:v0.7.2`; refresh
      `src/crd.rs` example; `make crds`; update migration guide.
- [ ] Crate: `cargo update -p bindcar` (API of the types bindy imports is
      unchanged; `auth.rs`/`main.rs` are binary-side).
- [ ] Reserved-env guard in `build_api_sidecar_container` (+ unit tests, TDD).
- [ ] New VAP `15-bindy-bindcar-env-policy.yaml` + `16` binding + fixtures;
      README + `admission-policies-install` target.
- [ ] `cargo-quality` + `make regression-test` (Phase B/C now viable against
      the stock `v0.7.2` image — no custom build needed).

## Commit range covered

`v0.6.0 (7080edc)` → `v0.7.2 (b4bdc01)`: PRs #46–#71 incl. the security sweep
(A1–A20). v0.7.2 = PR #71 only.
