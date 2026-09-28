# bindcar `v0.8.0` → `v0.8.2` — bindy Integration & Upgrade Guide

> **Status:** ✅ Applied (2026-09-28) — bindy is on the 0.8.1+ types
> (crates.io; 0.8.2 is version-bump-only over 0.8.1, `git diff
> v0.8.1..v0.8.2` touches exactly `Cargo.toml`), the default operand image is
> `v0.8.2`, and `DNSZone.status.dnssec.nextKeyRollover` is wired
> (ADR-0006 as amended). Open follow-ups: bump the Cargo.lock pin to 0.8.2
> once its crates.io publish lands, and the two ADR-gated items in §2/§3.
> Supersedes [25](25-bindcar-migration-v0-8-0.md) as the live guide in the
> series. Built from `git diff v0.8.0..v0.8.1` in `firestoned/bindcar`
> (35 files, +3,613/−134) and verified against that tree on 2026-09-28.
>
> The payoff for bindy: v0.8.2 is the release we asked for — it ships the
> DNSSEC key-state surface that
> [ADR-0006](../../docs/adr/0006-dnssec-ds-record-status-reporting.md)
> deferred, so `DNSZone.status.dnssec.nextKeyRollover` can finally be
> populated.

---

## 0. Version facts (verified 2026-09-28)

| | v0.8.0 | v0.8.1 | v0.8.2 |
|---|---|---|---|
| Tag commit | `v0.8.0` | `2d4322f` | `88aed32` (= `main`) |
| `Cargo.toml` `version` | `0.8.0` ✅ | **`0.8.0`** ⚠️ divergence | **`0.8.2`** ✅ |
| Live-zone DNSSEC lifecycle | ✗ | **✓** (bindcar roadmap 07, bindcar ADR-0001) | ✓ (identical) |
| DS retrieval endpoint | ✗ | **✓** `GET /api/v1/zones/{zone}/ds` | ✓ |
| Zone-status DNSSEC block | ✗ | **✓** (`ZoneStatusResponse.dnssec`) | ✓ |
| Key timing (`rndc dnssec -status`) | ✗ | **✓** (`DnssecKeyStatus.next_rollover` + per-key states) | ✓ |

## 1. ✅ RESOLVED: the tag/version divergence (v0.8.1 → v0.8.2)

The original `v0.8.1` tag pointed at a tree whose `Cargo.toml` still said
`0.8.0` — the exact regression [24 §19](24-bindcar-migration-v0-7-4.md)
flagged for v0.7.4. **Resolution (2026-09-28):** the version was bumped in
two commits (`19183c6` → 0.8.1, published to crates.io; `88aed32` → 0.8.2)
and **`v0.8.2` is the shipping release**. Consequences that remain true:

- crates.io has `0.8.1` (API-identical to 0.8.2); bindy's `Cargo.toml` floor
  is `0.8.1` and the lock should be bumped to `0.8.2` when its publish lands.
- **Skip `v0.8.1` images** — they self-report `0.8.0` on `/api/v1/health`,
  OpenAPI `info.version`, the startup log and `bindcar_app_info{version}`.
  Deploy `v0.8.2`.

## 2. 🟢 The DNSSEC key-state surface bindy filed for

New unconditional module `bindcar::dnssec` (types-first; `ToSchema` derives
and route handlers are gated behind the `server` feature, so bindy's
`default-features = false` import keeps working):

- `DnssecStatus` — zone-level signing state, carried on the new
  `ZoneStatusResponse.dnssec` field.
- `DnssecKeyStatus` — per key: role, goal state, `dnskey_state`, `ds_state`,
  `zone_rrsig_state`, `key_rrsig_state`, and **`next_rollover`**
  (parsed from `rndc dnssec -status`, converted to ISO 8601,
  server-local clock).
- `DsRecord` / `DnskeyRecord`, plus `DsRecordView` / `DsSetResponse` for the
  new `GET /api/v1/zones/{zone}/ds` endpoint (reads `dsset-*` files).
- `CheckdsRequest` + `POST /api/v1/zones/{zone}/checkds` — tell named the DS
  was published at (or withdrawn from) the parent, driving KSK rollover
  forward.

**Action (bindy) — completes ADR-0006's deferred fields:**

- [x] Extend the zone-status polling path to read `ZoneStatusResponse.dnssec`
      and populate `DNSZone.status.dnssec.nextKeyRollover` from the KSK's
      `next_rollover` — done 2026-09-28: `zone_ops::{parse_zone_status_dnssec,
      next_ksk_rollover}` + `fetch_next_ksk_rollover` in the DNSZone
      reconciler (best-effort, keeps the previous value on transient
      failure). ADR-0006 amended.
- [x] `lastKeyRollover` **still has no source** — v0.8.1 exposes the next
      event and current states, not history. Leave null; note in the API
      docs. (Deriving it from state transitions is possible but stateful —
      out of scope.)
- [ ] Optional hardening from ADR-0006's future list: cross-check the
      DNS-derived DS records against `GET /zones/{zone}/ds` and surface a
      mismatch as a Degraded condition.
- [ ] Decide whether the operator should call `checkds` on behalf of users
      (needs a signal that the DS is live at the parent — likely a DNSZone
      annotation or spec field; **ADR required** before building).

## 3. 🟢 Live-zone DNSSEC enable/disable

`ModifyZoneRequest` gains `dnssec_policy` / `inline_signing`, and the
rndc-side `ZoneConfig` now carries `dnssec_policy` as a **typed** field —
bindcar's ADR-0001 notes the showzone → modzone round-trip must never drop
it: verified on BIND 9.18.50, re-issuing a zone config without the directive
**abruptly unsigns the zone** at the next reconfig.

**Action (bindy):**

- [ ] bindy currently only sets `dnssec_policy` at zone **creation**
      (`add_primary_zone`). With v0.8.1 the DNSZone reconciler can honor a
      `spec.dnssecPolicy` **change** on a live zone via modify instead of
      requiring delete-and-recreate. Architecturally significant (contract +
      unsigning hazard above) — **ADR required**.
- [ ] Until that ADR: verify bindy's existing modify paths cannot
      accidentally hit the unsigning hazard (bindy does not use
      showzone→modzone round-trips today — confirm and pin with a test).

## 4. 🟠 Mechanical upgrade tasks (after the release fix)

- [x] `Cargo.toml`: `bindcar = { version = "0.8.1", default-features = false }`;
      lock resolves 0.8.1 (bump to 0.8.2 when its crates.io publish lands).
- [x] `src/constants.rs`: `DEFAULT_BINDCAR_IMAGE` → `…:v0.8.2`; `src/crd.rs`
      doc example, `regen-crds` + `regen-api-docs`, examples, tests fixtures,
      README, docs — 13 references bumped.
- [x] `rg 'firestoned/bindcar:v' . --glob '!target/'` — no stale tags.
- [ ] §23/§24/§25 of [25](25-bindcar-migration-v0-8-0.md) need no re-work:
      the diff adds no new env vars, no rate-limit changes, and no new
      default features.

## 5. Not bindy-relevant

The remainder of the diff is bindcar-internal: e2e workflow expansion
(+132 lines), Makefile targets, docs, and its own roadmap/changelog updates.

## Commit range covered

`v0.8.0` → `v0.8.1 (2d4322f)`: PR #132 (live-zone DNSSEC lifecycle,
bindcar roadmap 07 / bindcar ADR-0001) plus CI dependency bumps (#130).
