# bindcar `v0.7.2` → `v0.7.4` — bindy Integration & Upgrade Guide

> **Status:** 📄 Reference — **superseded** by
> [`57-bindcar-migration-v0-8-0.md`](57-bindcar-migration-v0-8-0.md). Its §14
> (`bindcarConfig.envVars` override) and §15 (the `bindcar_zones_managed_total`
> rename) are still open bindy-side and are carried forward there; §19's
> tag/version divergence is fixed as of bindcar v0.8.0.
>
> Originally: the live guide in the series, superseding
> [`55-bindcar-migration-v0-7-2.md`](55-bindcar-migration-v0-7-2.md). Built from
> `git diff v0.7.2..v0.7.4` in `firestoned/bindcar` and verified against that
> tree on 2026-09-12.
>
> *Written 2026-09-12. Sections 1–14 are historical; the actionable content is
> §15–§19.*

---

## 0. Version facts (verified 2026-09-12)

| | v0.7.2 | v0.7.3 | v0.7.4 |
|---|---|---|---|
| Tag commit | `b4bdc01` | `0a8cf51` | `bc8fbe2` |
| Tag date | 2026-07-05 | 2026-07-13 | 2026-09-07 |
| `Cargo.toml` `version` | **`0.7.0`** ⚠️ | `0.7.3` | **`0.7.3`** ⚠️ |
| `sha2` | 0.10 | 0.11 | 0.11 |
| TokenReview in release artifacts | ✅ (since v0.7.2) | ✅ | ✅ |

Two of the three tags disagree with the crate version they contain — see §19.
There is no `v0.7.4` crate release; v0.7.4 is a **tag over the same 0.7.3 crate
version** plus dependency and CI changes.

## 1–14. Everything through v0.7.2 — DONE in bindy

Sections 1–10 (v0.6.0 → v0.7.0), 11–12 (→ v0.7.1) and 13–14 (→ v0.7.2) are
unchanged and were absorbed in bindy between 2026-07-01 and 2026-07-06. See
[`53`](53-bindcar-migration-v0-7-0.md), [`54`](54-bindcar-migration-v0-7-1.md)
and [`55`](55-bindcar-migration-v0-7-2.md).

One carry-over worth re-checking: **§14's `bindcarConfig.envVars` override hole**
(a tenant-supplied `BIND_API_TOKEN` or `KUBE_API_SERVER` silently overrides the
operator-managed value, because the kubelet takes the last duplicate). Nothing
in v0.7.3/v0.7.4 changes that; the fix is entirely bindy-side (reserved-env
guard in `build_api_sidecar_container`, plus VAP 15/16). Confirm it landed.

---

## 15. 🔴 Prometheus metric renamed — `bindcar_zones_managed_total` is gone

`src/metrics.rs`. The gauge was renamed to drop the `_total` suffix, which
Prometheus convention reserves for counters:

| v0.7.2 | v0.7.3+ |
|---|---|
| `bindcar_zones_managed_total` | `bindcar_zones_managed` |

The Rust symbol changed with it (`ZONES_MANAGED_TOTAL` → `ZONES_MANAGED`), and
`bindcar::metrics` is a **public** module (`pub mod metrics` in `src/lib.rs`),
so a crate-level reference breaks at compile time. The scrape-name change does
**not** break at compile time — it fails silently:

- A dashboard panel or recording rule on `bindcar_zones_managed_total` returns
  no data.
- An alert on it stops firing, including alerts meant to catch zone loss.

**Action (bindy):**

- [ ] `rg -n "bindcar_zones_managed_total"` across the repo — dashboards,
      `PrometheusRule` manifests, docs, e2e assertions.
- [ ] Rename every hit to `bindcar_zones_managed`.
- [ ] If both bindcar versions run during a rollout, use
      `bindcar_zones_managed or bindcar_zones_managed_total` in queries until
      every operand is on v0.7.3+, then drop the fallback.

## 16. 🟠 `primaries` / `also-notify` accept `ip:port` — lets the operand drop `NET_BIND_SERVICE`

`src/zones.rs`, PR #82. `POST /api/v1/zones` now accepts a port on each transfer
endpoint. This was added **for bindy**: cross-pod AXFR/NOTIFY previously forced
transfers onto port 53, which forced the operand pod to hold the
`NET_BIND_SERVICE` capability.

| Field | v0.7.2 | v0.7.3+ |
|---|---|---|
| `primaries` | bare IP only | bare IP **or** `ip:port` |
| `alsoNotify` | bare IP only | bare IP **or** `ip:port` |
| `allowTransfer` | bare IP only | **bare IP only** (unchanged — it is an ACL and takes no port) |

Accepted forms: `192.0.2.1`, `2001:db8::1`, `192.0.2.2:5353`,
`[2001:db8::2]:53`. Backward compatible — bare IPs still work unchanged.

> ⚠️ **The wire format is `ip:port`, not `ip port N`.** bindcar's own
> `CHANGELOG.md` entry for this change describes it as `"<ip> port <n>"`; that
> is wrong. `src/zones_test.rs:1293` explicitly asserts
> `"192.0.2.1 port 5353"` is **rejected** with HTTP 400. bindcar parses the
> compact `ip:port` form and renders BIND's `ip port N;` syntax internally.
> Send the compact form.

The narrow grammar (IP literal, `:`, `u16`) is what preserves the C-1
`rndc addzone` injection guard, so do not expect anything looser to pass.

**Action (bindy):**

- [ ] Decide whether the operand runs `named` on an unprivileged transfer port.
      If yes: emit `primaries` / `alsoNotify` as `ip:port` and drop
      `NET_BIND_SERVICE` from the operand `securityContext`.
- [ ] Gate that on the bindcar version — a v0.7.2 sidecar returns **HTTP 400**
      for a ported entry. Do not emit `ip:port` until the operand image is
      v0.7.3+.
- [ ] Leave `allowTransfer` as bare IPs; a ported entry there is rejected.

## 17. 🟠 Crate consumers: `sha2` 0.10 → 0.11

`Cargo.toml`. A semver-major bump of a transitive dependency. `sha2` is not in
bindcar's public API (it backs the constant-time shared-secret comparison in
`src/auth.rs`), so **no bindy source change is required** — but if bindy pins
`sha2 0.10` directly, `cargo tree -d` will now show a duplicate. The
feature-gate roadmap
([`02` in bindcar](https://github.com/firestoned/bindcar/blob/main/.github/community/02-feature-gate-http-server.md))
counts that duplicate as one of the costs of importing the full bindcar graph.

`serial_test 3.4 → 4.0` also moved, but it is a dev-dependency and does not
reach consumers.

**Action (bindy):**

- [ ] `cargo update -p bindcar` and check `cargo tree -d | rg sha2`.
- [ ] If a duplicate appears, bump bindy's own `sha2` to 0.11.

## 18. 🟢 Version reporting is finally truthful

`src/main.rs` / `src/metrics.rs`, PR #83. Before v0.7.3 the crate version in
`Cargo.toml` was stale at `0.7.0`, and the OpenAPI `info.version` was a
hardcoded `"0.1.0"`. Now a single `VERSION` constant
(`env!("CARGO_PKG_VERSION")`) feeds all four call sites:

| Surface | v0.7.1 / v0.7.2 reported | v0.7.3+ reports |
|---|---|---|
| `GET /api/v1/health` `.version` | `0.7.0` | `0.7.3` |
| OpenAPI `info.version` | `0.1.0` | `0.7.3` |
| Startup log line | `0.7.0` | `0.7.3` |
| `bindcar_app_info{version=…}` | `0.7.0` | `0.7.3` |

**Action (bindy):** if anything asserts on these strings — e2e checks, the
`0.1.0` OpenAPI value in particular — update the expected values. Otherwise
informational.

## 19. ⚠️ Do not use the tag as the version — v0.7.4 reports `0.7.3`

The `v0.7.4` tag was cut without bumping `Cargo.toml`, so a v0.7.4 image
reports `0.7.3` on every surface in §18. Combined with the pre-v0.7.3 staleness,
the tag and the self-reported version agree on **v0.7.3 only**.

**Action (bindy):**

- [ ] Pin `DEFAULT_BINDCAR_IMAGE` by tag, but do **not** derive a version
      comparison from `/api/v1/health` or `bindcar_app_info` — they cannot
      distinguish v0.7.3 from v0.7.4.
- [ ] If a version gate is needed for the §16 `ip:port` feature, gate on the
      configured image tag, not on the reported version.
- [ ] Worth raising upstream: bindcar should bump `Cargo.toml` in the release
      commit so tag and crate version cannot diverge.

## 20. 🟢 CI only — no bindy action

- **New `e2e.yaml`** (PR #88) — reusable self-contained e2e (drone integration
  + kind), and a Dependabot auto-merge workflow gated on it.
- **Auto-merge policy** refined for major vs minor/patch.
- Base-image, GitHub Actions and cargo dependency bumps (the bulk of the 34
  commits in this range).
- `rust-toolchain.toml` moved with the `rust` base image.

None of this changes the bindcar artifact's behavior.

---

## Pre-upgrade checklist for bindy (v0.7.2 → v0.7.4)

- [ ] `src/constants.rs`: `DEFAULT_BINDCAR_IMAGE` → `…:v0.7.4`; refresh the
      `src/crd.rs` doc example; `make crds`; update bindy's migration guide.
- [ ] `rg -n "bindcar_zones_managed_total"` → rename to `bindcar_zones_managed`
      in dashboards, `PrometheusRule`s, docs and e2e assertions (§15).
- [ ] `cargo update -p bindcar`; `cargo tree -d | rg sha2` (§17).
- [ ] Decide on the unprivileged transfer port (§16). If adopting: emit
      `ip:port`, drop `NET_BIND_SERVICE`, and gate on the image tag.
- [ ] Update any assertion on `/api/v1/health` `.version` or the OpenAPI
      `info.version` (§18).
- [ ] Confirm the §14 reserved-env guard and VAP 15/16 actually landed — they
      are still outstanding as far as this series records.
- [ ] `cargo-quality` + `make regression-test`.

## Commit range covered

`v0.7.2 (b4bdc01)` → `v0.7.4 (bc8fbe2)`: 34 commits, PRs #72–#115. The only
`src/` changes are PR #82 (per-endpoint ports, §16) and PR #83 (version
reporting, §18, which carried the metric rename in §15). Everything else is
dependency, CI and documentation work.
