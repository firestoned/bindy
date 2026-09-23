# Roadmaps

High-level index of bindy's roadmap documents. Full detail for each item
lives in [`.github/community/`](.github/community/) — this file tracks
what each one is and its current completion status; the detailed task
lists and design rationale live in the linked doc itself.

Architecturally significant work in any roadmap below still goes
**ADR → TDD → implement → docs**, in that order (see
[`.claude/rules/testing.md`](.claude/rules/testing.md) and
[`.claude/rules/documentation.md`](.claude/rules/documentation.md)) — a
roadmap entry describes *what* and *why*, it does not skip the ADR for
*how*.

## Status legend

| Symbol | Meaning |
|---|---|
| ✅ | Done — implemented, tested, in the codebase today |
| 🔶 | In progress — some of it exists, not complete |
| ⛔ | Not started |
| 📄 | Reference doc — not a phase with a completion state |

## Index

Statuses were verified against `fix-idempotency` @ `648ff7a` on 2026-09-10.

### Reference and analysis

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [00](.github/community/00-future-refactoring-opportunities.md) | Future refactoring opportunities | 📄 | Mostly superseded — its top recommendation landed as 02, the rest is subsumed by 01 |

### Architecture and refactoring

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [01](.github/community/01-controller-crate-split.md) | Controller crate split & watch-layer simplification | ⛔ | One 40.6k-line crate; `main.rs` is 2,086 lines of hand-rolled framework, 22 watch call sites, ~36 API-server watch connections where ~15 would do. Split into a workspace and rebuild watches on kube-runtime's shared-stream APIs |
| [02](.github/community/02-records-reconciler-refactoring.md) | Records reconciler refactoring | ✅ | Generic `reconcile_record<T>()` at `src/reconcilers/records/mod.rs:1170`; the 9 per-type fns are thin wrappers. 01 turns those into trait impls |
| [03](.github/community/03-early-return-refactoring.md) | Early-return / guard-clause refactor | 🔶 | Rule codified in `.claude/rules/rust-style.md`, but all 9 named target functions still exist. Doc's line numbers are from 2026-01 and have drifted |
| [04](.github/community/04-remove-clusterref-use-ownerreference.md) | Remove `clusterRef`, use `ownerReference` | ⛔ | `pub cluster_ref` still in `src/crd.rs` at `:994`, `:3679`, `:3839`. Breaking CRD change — needs an ADR first |
| [05](.github/community/05-kubernetes-api-rate-limiting.md) | Kubernetes API rate limiting | 🔶 | `reconcilers/pagination.rs` and `retry.rs` landed; no explicit client-side rate limiter. Overlaps 01, which halves watch connections |
| [06](.github/community/06-kube-condition-derive-macro.md) | `kube-condition` derive macro | ⛔ | Not a dependency; conditions are hand-built in `src/reconcilers/status.rs`. See 08 for what shipped instead |

### Features

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [07](.github/community/07-dnssec-zone-signing.md) | DNSSEC zone signing | 🔶 | Phases 1–4 done — `dnssecPolicy` at `src/crd.rs:1228`, signing in `bind9/zone_ops.rs` and `bind9_resources.rs`. Phase 5 open |
| [08](.github/community/08-status-conditions.md) | Status conditions | ✅ | Phases 1–5 complete (`reconcilers/status.rs`, `status_reasons.rs`); phases 6–7 are explicitly future work inside the doc |
| [09](.github/community/09-external-bind9-gateway.md) | External BIND9 gateway | ⛔ | Still a draft; no external-endpoint or gateway fields in `src/crd.rs` |
| [10](.github/community/10-rndc-secret-hot-reload.md) | RNDC secret hot reload | ⛔ | Designed in [ADR-0001](docs/adr/0001-rndc-secret-reload.md); no reload path in `src/` |
| [11](.github/community/11-compliance-gamification.md) | Compliance & security gamification | ⛔ | No policy or report CRDs exist. Largest unstarted item here — needs an ADR before any of it is built |

### Scout

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [12](.github/community/12-scout-ingress-controller.md) | Scout — Ingress → ARecord controller | 🔶 | Phases 1/1.5 shipped, phase 2 (`BINDY_SCOUT_REMOTE_SECRET`) implemented. Scout has outgrown the doc: 5 controllers today (Ingress, Service, HTTPRoute, TLSRoute, TCPRoute) |
| [13](.github/community/13-scout-namespace-selectors.md) | Scout — namespace label selectors | 🔶 | `namespace_selector` landed (#437), evaluated per event by `source_namespace_eligible()`. The `Namespace` **watch** — the part that actually cuts event volume — is not implemented |
| [14](.github/community/14-scout-srv-records.md) | Scout — SRV record support | ⛔ | No `SRVRecord` reference anywhere in `src/scout.rs` |

### Security and compliance

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [15](.github/community/15-security-scanning.md) | Security scanning | 🔶 | Through phase 5 (license compliance). `security-scan.yaml`, `sbom.yml`, `license-scan.yaml`, `codeql.yml`, `scorecard.yml` all in place |
| [16](.github/community/16-audit-logging-secret-operations.md) | Audit logging for Secret operations | ⛔ | Approved 2026-03-09, never implemented — no audit-log emission in `src/`. Compliance-relevant; worth re-triaging rather than leaving to drift |
| [17](.github/community/17-vex-documents.md) | VEX documents | ⛔ | No VEX generation step in `.github/workflows/`. Builds on the SBOM pipeline from 15 |

### Testing, operations and dependencies

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [18](.github/community/18-load-testing-framework.md) | Load testing framework | ⛔ | Target `crates/loadtest/` does not exist — the repo has no `crates/` directory, so this lands after the workspace conversion in 01. Merged from two external docs on migration |
| [19](.github/community/19-integration-testing.md) | Integration testing | ✅ | Superseded by the shipped harness: `tests/integration_test.sh`, `tests/multi_tenancy_integration.rs`, `make kind-integration-test` |
| [20](.github/community/20-hickory-client-migration-target.md) | Hickory client migration target | ⛔ | A scheduled revisit, not a build. `Cargo.toml` pins hickory 0.26; re-evaluate in Q3 2026 |
| [21](.github/community/21-bindcar-migration-v0-7-0.md) | bindcar upgrade — v0.7.0 | 📄 | Superseded by 24. Absorbed 2026-07-01/02 (Mode B / TokenReview) |
| [22](.github/community/22-bindcar-migration-v0-7-1.md) | bindcar upgrade — v0.7.1 | 📄 | Superseded by 24. Absorbed 2026-07-05 |
| [23](.github/community/23-bindcar-migration-v0-7-2.md) | bindcar upgrade — v0.7.2 | 📄 | Superseded by 24. §14's `bindcarConfig.envVars` override hole is bindy-side and may still be open |
| [24](.github/community/24-bindcar-migration-v0-7-4.md) | bindcar upgrade — v0.7.4 | 📄 | Superseded by 25. Its §14 (envVars override) and §15 (metric rename) remain open bindy-side |
| [25](.github/community/25-bindcar-migration-v0-8-0.md) | bindcar upgrade — v0.8.0 | 🔶 | **Current and actionable.** 🟢 TLS/mTLS available (remediates audit P2-4, needs a bindy CRD surface for scheme + CA); 🟢 cert hot-reload; 🟠 `default-features = false` sheds 80 of 178 crates; 🔴 rate-limit defaults changed (100→600 req, burst 10→50); 🔴 the envVars override hole now reaches `BIND_TLS_*` |

## Tracked privately

Some in-flight security hardening work is tracked privately until it lands and
so has no row above. Those documents carry **no number** while they are outside
this repo — numbers here are contiguous and hold no gaps, so a private document
is numbered only when it is moved in, taking the next free number at that point.

## Numbering

Numbers are a zero-padded two-digit prefix, contiguous from `00` with no gaps
and no thematic banding. They are an ordering, not an identity: inserting or
retiring a roadmap renumbers the run, and every reference to the moved numbers
is fixed in the same commit. Reference a roadmap by its padded number in prose
("roadmap 07") so the number greps against the filename.

## Keeping this current

When a roadmap item's status changes (something lands, something new
starts), update its row here in the same PR/commit that makes the change
— this file is a status board, not documentation of intent. Detailed
task-level tracking stays inside each roadmap doc; this file only tracks
the item-level state.
