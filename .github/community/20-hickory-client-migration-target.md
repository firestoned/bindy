# Hickory client: revisit migration target in Q3 2026

> **Status:** ✅ Revisit done 2026-10-08: **stay on `hickory-net`**. Already on the newest release (0.26.3, which carries every 0.26.2 security fix); requirement floor raised to `0.26.3`. Next revisit when hickory 0.27.0 ships. See "Outcome (2026-10-08)" below.
>
> *Migrated 2026-09-10 from the external roadmap set. Status verified against `fix-idempotency` @ `648ff7a`.*

---

## Context

bindy migrated from `hickory-client 0.24.4` to `hickory-net 0.26.1` on 2026-05-02
to remediate **RUSTSEC-2026-0119** (CPU exhaustion via O(n²) name compression in
`BinEncoder`, fixed in `hickory-proto 0.26.1`).

The migration was a same-vendor swap inside the `hickory-dns` project: the
`hickory-client` crate was deleted from the workspace between v0.25 and v0.26;
its functionality moved into `hickory-net::client` (and the published crate
`hickory-net` on crates.io). The 0.26.0-alpha.1 publication of `hickory-client`
(June 2025) is stale and incompatible with the patched `hickory-proto 0.26.1`.

## Why this is a roadmap item, not a "done" item

- The `hickory-net` API surface is functionally complete for our needs (RFC 2136
  UPDATE, TSIG, async, all required record types) but the project is still
  pre-1.0 and explicitly marked "running in production is not recommended" by
  the maintainers.
- We are pinning a dependency that is the most actively-developed Rust DNS
  library (ISRG/Prossimo-funded; targeted as Let's Encrypt's recursive
  resolver), but the 0.x version label still requires periodic re-evaluation.

## What to revisit (target: Q3 2026)

1. **Has `hickory-net` reached 1.0?** If yes, bump pinning, drop the
   "pre-1.0 risk" caveat, and verify the API stability guarantee.
2. **Is there a published, stable replacement for `hickory-client`?** Some
   downstreams still expect a high-level client crate. If hickory ships one
   (e.g. a `hickory-client` v1.x rebuilt on `hickory-net`), evaluate moving to
   it for clearer API stability promises.
3. **`domain` crate (NLnetLabs)** — has its `net::client` graduated from the
   `unstable-client-transport` feature gate? Has it gained a high-level RFC 2136
   UPDATE builder? If both yes, re-run the comparison; the compliance optics of
   a NLnetLabs-maintained DNS stack (NSD/Unbound authors) are attractive.
4. **Open RUSTSEC advisories** against `hickory-net`, `hickory-proto`, or
   `domain` — any unresolved? If yes, reassess.
5. **Maintenance cadence** — is `hickory-net` still receiving regular releases?
   Check commit frequency, release notes, GitHub issue triage.

## Decision criteria

Migrate away from `hickory-net` only if:

- A stable (≥1.0) alternative exists with **first-class** RFC 2136 UPDATE +
  TSIG support (no hand-rolled framing).
- AND the alternative has equal or better maintenance health.
- AND the migration cost is justified by a concrete compliance/operational
  benefit (e.g. `domain` going stable while `hickory-net` enters maintenance
  mode).

Stay on `hickory-net` and bump the pin if:

- It reaches 1.0 with no breaking changes that affect us.
- OR continues active development and no superior alternative emerges.

## Migration surface (for reference)

If we end up re-migrating, the bindy code that touches the DNS client lives in:

- `src/bind9/records/mod.rs` — central `build_authenticated_client()` /
  `build_query_client()` / `build_record_fqdn()` helpers
- `src/bind9/records/{a,caa,cname,mx,ns,srv,txt}.rs` — per-record-type
  `add_*_record()` functions, all going through the helpers
- `src/bind9/zone_ops.rs::verify_zone_signed` — ad-hoc unauthenticated client
- `src/bind9/rndc.rs` — TSIG signer construction

Encapsulating the client construction inside `records/mod.rs` was a deliberate
choice during the 2026-05-02 migration so that any future swap touches a small
surface (3 helper functions + one DNSSEC verification call site).

## Outcome (2026-10-08)

Answers to the five questions, checked against crates.io, the RustSec
advisory database, GitHub security advisories and the upstream repositories:

1. **1.0?** No. `hickory-net` / `hickory-proto` are at **0.26.3
   (2026-09-10)**; no 0.27 or 1.0 is published (upstream `main` reads
   `0.27.0-alpha.1`). The README no longer carries a "not recommended for
   production" caveat.
2. **High-level client crate?** No separate one is coming: the v0.26.0 release
   notes state `hickory-client` is subsumed into `hickory-net::client`, which
   is what bindy already uses.
3. **`domain` (NLnetLabs)?** 0.12.3 (2026-09-25), still pre-1.0. `net::client`
   and its TSIG transport remain behind `unstable-client-transport`, and there
   is no RFC 2136 UPDATE builder (messages would be hand-built). It also just
   shipped a batch of panic/DoS fixes (RUSTSEC-2026-0310).
4. **Open advisories?** None affecting the locked 0.26.3. RustSec lists nothing
   for hickory after RUSTSEC-2026-0119/0120. Upstream published about 40 GHSAs
   with 0.26.2 (2026-09-03), all patched there; the client-relevant ones (TSIG
   RDATA decode panic, spoofed-response UDP DoS, RR-count memory amplification,
   QDCOUNT=0 question-match bypass) are fixed in what bindy runs. bindy
   computes key tags with `calculate_key_tag()`, not the `DS::from_key` path
   whose wrong key tag 0.26.2 fixed.
5. **Maintenance?** Healthy: five stable releases from 2026-04-16 to
   2026-09-10, steady commits (228 in September 2026), active maintainers,
   ISRG/Prossimo funding, Let's Encrypt resolver as the target user.

**Decision:** stay. The only alternative fails every migration criterion (not
1.0, client transport unstable, no first-class UPDATE builder, no concrete
benefit). The workspace requirement is raised from `"0.26"` to `"0.26.3"` for
both crates so a lockfile regeneration can never fall below the patched floor.

**Next revisit:** when hickory 0.27.0 is published. Upstream `main` shows churn
in record decoding (`RecordDataDecodable::read_data()`), the TSIG RDATA parser
and DNSSEC internals, so re-check `TSigner`, `Client`/`ClientHandle` and the
`DNSKEY` key-tag/digest helpers against its release notes before bumping.

## Tracking

- Owner: TBD (security/platform)
- Re-evaluation date: 2026-09-01 (Q3 2026); done 2026-10-08. Next: on hickory 0.27.0
- Linked CHANGELOG entry: `2026-05-02 — Migrate hickory-client → hickory-net 0.26.1`
- Linked advisory: RUSTSEC-2026-0119
