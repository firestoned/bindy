# Cerebrum

> OpenWolf's learning memory. Updated automatically as the AI learns from interactions.
> Do not edit manually unless correcting an error.
> Last updated: 2026-04-28

## User Preferences

<!-- How the user likes things done. Code style, tools, patterns, communication. -->

## Key Learnings

- **Project:** bindy
- **Description:** [![License](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
- **DNS client crate (2026-05-02):** bindy uses `hickory-net 0.26` (not `hickory-client`). In hickory v0.26 the `client` crate was removed from the workspace and its functionality merged into `hickory-net::client`. The new pattern is `UdpClientStream::builder(addr, TokioRuntimeProvider::default()).with_signer(Some(tsigner)).build()` → `Client::<TokioRuntimeProvider>::from_sender(stream)` → `tokio::spawn(bg)`. All RFC 2136 calls (`.append`, `.delete_rrset`, `.query`, `.create`) are async. Helpers `build_authenticated_client()` / `build_query_client()` / `build_record_fqdn()` live in `src/bind9/records/mod.rs` — use them from new record modules.
- **TSIG path (2026-05-02):** `TSigner` is at `hickory_proto::rr::TSigner` (re-exported from `hickory_proto::rr::tsig`). `TsigAlgorithm` is at `hickory_proto::rr::rdata::tsig::TsigAlgorithm`. There is no longer a `hickory_proto::rr::dnssec::tsig::*` path.

## Do-Not-Repeat

<!-- Mistakes made and corrected. Each entry prevents the same mistake recurring. -->
<!-- Format: [YYYY-MM-DD] Description of what went wrong and what to do instead. -->

- **[2026-05-02] hickory `Record` field-vs-method collision.** In hickory 0.26, `Record` exposes `name`, `dns_class`, `ttl`, `data` as **public fields** AND keeps same-name accessor methods (`name()`, `data()`, etc.). Rust's parser resolves `record.data()` as `(record.data)()` — calling the field as a function — which fails because `RData` isn't callable. **Always use field access** (`record.data`, `&record.data` for matching, `record.dns_class = DNSClass::IN` for assignment). Same applies to `rdata::CAA` (`issuer_critical`, `tag`, `value`), `rdata::MX` (`preference`, `exchange`), `rdata::SRV` (`priority`, `weight`, `port`, `target`), `rdata::TXT` (`txt_data`) — all switched from method accessors to public fields.
- **[2026-05-02] `hickory-client` is dead — don't attempt to upgrade it.** The crate was deleted from the hickory-dns workspace between 0.25 and 0.26. `hickory-client 0.26.0-alpha.1` was published on crates.io in June 2025 but is incompatible with the patched `hickory-proto 0.26.1` (it requires the `text-parsing` feature that was removed in proto 0.26.0-beta.4). Migrate to `hickory-net` instead.

## Decision Log

<!-- Significant technical decisions with rationale. Why X was chosen over Y. -->

- **[2026-05-02] Migrated from `hickory-client 0.24` to `hickory-net 0.26.1` (not to `domain` crate).** RUSTSEC-2026-0119 forced the upgrade. `domain` (NLnetLabs) was the only credible third-party alternative, but its `net::client` is gated behind `unstable-client-transport` and lacks a high-level RFC 2136 UPDATE message builder — we'd hand-roll the encoder. In a banking-compliance codebase, "wrote our own DNS UPDATE encoder" is worse audit optics than "pinned a same-vendor pre-1.0 crate". `hickory-net` is the same project, same maintainers (ISRG/Prossimo), backing Let's Encrypt's recursive resolver in production. Trade-off accepted: pre-1.0 status. Re-evaluate Q3 2026 (see `docs/roadmaps/hickory-client-stable-upgrade.md`).
