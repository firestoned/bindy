# bindcar `v0.7.4` → `v0.8.0` — bindy Integration & Upgrade Guide

> **Status:** 🔶 Mostly applied (2026-09-19). §21 is implemented end to end —
> CRD surface, sidecar wiring and operator client — pending live verification.
> §23 (types-only dependency),
> §24 (rate-limit defaults reviewed — no bindy change needed) and the
> carried-over metric rename are **done**: bindy is on `bindcar 0.8.0` with
> `default-features = false`, shedding 31 crates. **§21 (CRD surface for TLS)
> and §25 (reserved-env guard + VAP 15/16) remain open** — until §21 lands,
> audit finding P2-4 is not remediated end to end.
>
> Originally: actionable, the live guide in the series, superseding
> [`56-bindcar-migration-v0-7-4.md`](56-bindcar-migration-v0-7-4.md). Built from
> `git diff v0.7.4..v0.8.0` in `firestoned/bindcar` and verified against that
> tree on 2026-09-19.
>
> *Sections 1–20 are historical; the actionable content is §21–§25.*

---

## 0. Version facts (verified 2026-09-19)

| | v0.7.3 | v0.7.4 | v0.8.0 |
|---|---|---|---|
| Tag commit | `0a8cf51` | `bc8fbe2` | `v0.8.0` |
| `Cargo.toml` `version` | `0.7.3` | **`0.7.3`** ⚠️ | **`0.8.0`** ✅ |
| TLS listener | ✗ | ✗ | **✓** |
| Mutual TLS | ✗ | ✗ | **✓** |
| Cert hot-reload | ✗ | ✗ | **✓** |
| Cargo features | `k8s-token-review` | `k8s-token-review` | `server`, `tls`, `k8s-token-review` |

**The tag/version divergence flagged in [56 §19](56-bindcar-migration-v0-7-4.md)
is fixed.** `v0.8.0` reports `0.8.0` on `/api/v1/health`, in the OpenAPI
`info.version`, in the startup log and in `bindcar_app_info{version}`. From this
release onward the reported version is a reliable discriminator again.

## 1–20. Everything through v0.7.4

Absorbed previously; see guides
[53](53-bindcar-migration-v0-7-0.md)–[56](56-bindcar-migration-v0-7-4.md).

Two carry-overs still worth confirming, both bindy-side and neither touched by
v0.8.0:

- **[56 §15] The metric rename.** `bindcar_zones_managed_total` →
  `bindcar_zones_managed` at v0.7.3. Silent: a dashboard or `PrometheusRule` on
  the old name returns no data rather than erroring.
- **[56 §14] The `bindcarConfig.envVars` override hole.** A tenant-supplied
  `BIND_API_TOKEN` or `KUBE_API_SERVER` overrides the operator-managed value
  because the kubelet takes the last duplicate. The fix is bindy-side: a
  reserved-env guard in `build_api_sidecar_container` plus VAP 15/16.
  **v0.8.0 raises the stakes** — see §25.

---

## 21. 🟢 TLS is available; bindcar can now serve HTTPS

`src/tls.rs` (new, PR #124). bindcar terminates TLS, optionally with mutual TLS.

| CLI flag | Env var | Meaning |
|---|---|---|
| `--tls-cert` | `BIND_TLS_CERT` | PEM chain, leaf first |
| `--tls-key` | `BIND_TLS_KEY` | PEM private key |
| `--tls-client-ca` | `BIND_TLS_CLIENT_CA` | PEM CA bundle; presence enables mTLS |

TLS is **opt-in**: with nothing configured bindcar serves plaintext exactly as
before, so an upgrade to v0.8.0 changes nothing until bindy configures it.

Fail-closed rules worth knowing before wiring this up:

- Setting exactly one of cert/key is a **startup error** (exit 1), never a
  silent fallback to plaintext.
- A client CA without a key pair is a **startup error**.
- Unreadable or malformed PEM is a **startup error**.

**Action (bindy):** this is the payoff for audit finding **P2-4** — the
ServiceAccount token currently crosses the pod network in the clear on every
zone operation. Two pieces of bindy work are needed:

- [ ] **CRD surface.** `Bind9Instance` / `Bind9Cluster` / `ClusterBind9Provider`
      need a way to express the sidecar's scheme and its CA bundle. `build_api_url`
      (`src/bind9/zone_ops.rs`) already passes an explicit `https://` through, so
      the *client* side needs no change — only the configuration to say so.
- [ ] **Certificate provisioning.** cert-manager issuing a `Secret` the operand
      pod mounts is the obvious route.

Prefer **mTLS** where both ends are in-cluster: a client certificate replaces the
bearer token on the wire rather than merely wrapping it.

## 22. 🟢 Certificate rotation no longer needs a restart

`TlsReloader` (PR #125). bindcar re-reads the certificate, key and client CA and
swaps them in without restarting.

| Env var | Default | Meaning |
|---|---|---|
| `BIND_TLS_RELOAD_INTERVAL` | `60` | Seconds between checks; `0` disables |

`SIGHUP` forces an immediate check.

Three properties that matter for an operator wiring this up:

- **Established connections are unaffected** — the config is read per accepted
  connection, so a rotation drops nothing.
- **A failed reload is a non-event.** A half-written renewal (new cert, stale
  key) logs `KeyMismatch` and *keeps serving the previous certificate*, retrying
  on the next interval. It will not take the sidecar down.
- **Detection is by content hash, not mtime** — which is what makes it work
  against the `..data` symlink swap Kubernetes uses for `Secret` mounts.

**Action (bindy):** none required. Worth knowing that a cert-manager renewal
needs no pod rollout, which matters because restarting the bindcar sidecar
restarts the BIND9 operand beside it.

## 23. 🟠 Crate consumers: new `server` and `tls` features

`Cargo.toml` (PRs #126, #127). The HTTP stack and the TLS stack are now optional,
both on by default.

| Feature | Default | Gates |
|---|---|---|
| `server` | on | axum, tower, tower-http, utoipa, utoipa-swagger-ui, tower_governor |
| `tls` | on | rustls, tokio-rustls, rustls-pki-types, hyper, hyper-util — implies `server` |
| `k8s-token-review` | off | kube, k8s-openapi — implies `server` |

bindy imports bindcar for its **data types**, so it can drop both:

```toml
bindcar = { version = "0.8", default-features = false }
```

**That takes bindcar's dependency graph from 178 crates to 98 — 80 shed (45%).**
The entire axum/tower/utoipa stack and the entire rustls/ring crypto stack leave
the tree.

Every public path is unchanged. `bindcar::ZoneConfig`, `bindcar::SoaRecord`,
`bindcar::AddRecordRequest` and the rest are re-exported from new
`zones_types` / `records_types` modules precisely so they keep resolving without
the server feature.

Two things that do **not** survive `default-features = false`, because they are
axum-bound:

- `bindcar::ApiError`, `bindcar::AppState`, `bindcar::ErrorResponse`
- the `tower_governor` re-exports from `bindcar::rate_limit`
  (`RateLimitConfig` itself is fine)

**Action (bindy):**

- [ ] `rg -n "bindcar::(ApiError|AppState|ErrorResponse)" src/` — if any hit,
      either keep default features or stop importing them.
- [ ] Set `default-features = false`, then `cargo tree | wc -l` before and after
      to confirm the drop.
- [ ] `cargo build && cargo test` — the API surface is unchanged, so this should
      be a no-op beyond the dependency reduction.

## 24. 🔴 Rate limit defaults changed — verify your burst

PR #123, in the v0.7.4→v0.8.0 range. Two changes:

| | before | v0.8.0 |
|---|---|---|
| `RATE_LIMIT_REQUESTS` default | `100` | **`600`** |
| `burst_size` default | `10` | **`50`** (`MIN_BURST_FOR_ZONE_REPLAY`) |

The replenish period was also being computed wrongly: governor is configured by
*period per cell*, not requests per period, and the old integer division
produced the wrong rate.

The reason this is 🔴 rather than informational is in bindcar's own comment: when
a BIND9 Pod restarts empty, bindy replays everything it should be serving —
create zone, freeze, apply each record, thaw, notify, poll status. That is
comfortably more than 10 calls and arrives as one burst per zone. A burst below
`50` **guaranteed HTTP 429 on every Pod restart**, which bindy answers with
exponential backoff, stretching a ~30s recovery into ~130s.

**Action (bindy):**

- [ ] If bindy sets `RATE_LIMIT_REQUESTS` or the burst explicitly anywhere
      (sidecar env, CRD defaults, Helm values), re-check the value against the
      new defaults — an explicit `10` still reproduces the old stall.
- [ ] If it does not set them, no action: the new defaults are strictly better.

## 25. 🔴 The envVars override hole now reaches TLS

[56 §14] established that user-supplied `bindcarConfig.envVars` override
operator-managed ones, because the kubelet takes the last duplicate. v0.8.0 adds
four more names to the list of things that can be tampered with:

- `BIND_TLS_CERT` / `BIND_TLS_KEY` — point the listener at attacker-supplied
  material, or half-configure it to force a startup failure (DoS on zone ops).
- `BIND_TLS_CLIENT_CA` — widen or replace the mTLS trust anchor.
- `BIND_TLS_RELOAD_INTERVAL` — set `0` to pin a certificate that is being
  rotated *away from*, e.g. one known to be compromised.

**Action (bindy):** the fix is unchanged from 56 §14 and still outstanding as far
as this series records — but the reserved-name list must now include the
`BIND_TLS_` prefix:

- [ ] Reserved-env guard in `build_api_sidecar_container`: drop and log any
      user-supplied var whose name is operator-reserved, now including
      `BIND_TLS_*`.
- [ ] VAP 15/16 to reject reserved names at admission, with fixtures.

---

## Pre-upgrade checklist for bindy (v0.7.4 → v0.8.0)

- [x] `src/constants.rs`: `DEFAULT_BINDCAR_IMAGE` → `…:v0.8.0`; refresh the
      `src/crd.rs` doc example; `make crds`; update bindy's migration guide.
- [x] `Cargo.toml`: `bindcar = { version = "0.8", default-features = false }`
      (§23), after checking for `ApiError`/`AppState` imports.
- [x] Re-check any explicit rate-limit configuration against the new defaults (§24).
- [x] Reserved-env guard: `is_reserved_bindcar_env` in `src/bind9_resources.rs`
      drops and logs tenant-supplied `BIND_TLS_*`, `KUBE_*`, `BIND_API_TOKEN`,
      `DISABLE_AUTH`, the allowlists and the RNDC vars.
- [ ] VAP 15/16 to reject reserved names at admission (defence in depth; the
      reconciler guard above is the load-bearing fix).
- [x] Still outstanding from 56: rename `bindcar_zones_managed_total` →
      `bindcar_zones_managed` in dashboards and alerts.
- [x] Design the CRD surface for TLS scheme + CA bundle (§21) — `bindcarConfig.tls`
      with `enabled` / `secretName` / `caBundle` / `serverName` /
      `reloadIntervalSeconds`; see [ADR-0004](../../docs/adr/0004-bindcar-tls-transport.md).
- [x] Sidecar wiring: Secret mounted read-only, `BIND_TLS_CERT`/`KEY`/
      `RELOAD_INTERVAL` set, CA-pinned verifier and client builder.
- [x] `Bind9Manager` plumbing: per-instance TLS config, lazily built and cached
      CA-pinned client, scheme qualification on every endpoint. Fails closed if
      the CA bundle cannot be read.
- [ ] End-to-end verification against a live cluster with cert-manager (not run
      here — no cluster available in this environment).
- [x] `cargo-quality` — fmt, clippy `-D warnings` and 1,461 tests all pass.
- [ ] `make regression-test` against a live cluster (not run here).

## Commit range covered

`v0.7.4 (bc8fbe2)` → `v0.8.0`: PRs #113–#127. The substantive source changes are
#123 (rate limiting, §24), #124 (TLS, §21), #125 (hot-reload, §22), #126 and
#127 (feature gating, §23). The remainder is dependency and CI work.
