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
| [02](.github/community/02-FUTURE-REFACTORING-OPPORTUNITIES.md) | Future refactoring opportunities | 📄 | Mostly superseded — its top recommendation landed as 12, the rest is subsumed by 10 |

### Architecture and refactoring

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [10](.github/community/10-CONTROLLER-CRATE-SPLIT.md) | Controller crate split & watch-layer simplification | ⛔ | One 40.6k-line crate; `main.rs` is 2,086 lines of hand-rolled framework, 22 watch call sites, ~36 API-server watch connections where ~15 would do. Split into a workspace and rebuild watches on kube-runtime's shared-stream APIs |
| [12](.github/community/12-RECORDS-RECONCILER-REFACTORING.md) | Records reconciler refactoring | ✅ | Generic `reconcile_record<T>()` at `src/reconcilers/records/mod.rs:1170`; the 9 per-type fns are thin wrappers. 10 turns those into trait impls |
| [13](.github/community/13-EARLY-RETURN-REFACTORING.md) | Early-return / guard-clause refactor | 🔶 | Rule codified in `.claude/rules/rust-style.md`, but all 9 named target functions still exist. Doc's line numbers are from 2026-01 and have drifted |
| [14](.github/community/14-REMOVE-CLUSTERREF-USE-OWNERREFERENCE.md) | Remove `clusterRef`, use `ownerReference` | ⛔ | `pub cluster_ref` still in `src/crd.rs` at `:994`, `:3679`, `:3839`. Breaking CRD change — needs an ADR first |
| [15](.github/community/15-KUBERNETES-API-RATE-LIMITING.md) | Kubernetes API rate limiting | 🔶 | `reconcilers/pagination.rs` and `retry.rs` landed; no explicit client-side rate limiter. Overlaps 10, which halves watch connections |
| [16](.github/community/16-KUBE-CONDITION-DERIVE-MACRO.md) | `kube-condition` derive macro | ⛔ | Not a dependency; conditions are hand-built in `src/reconcilers/status.rs`. See 21 for what shipped instead |

### Features

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [20](.github/community/20-DNSSEC-ZONE-SIGNING.md) | DNSSEC zone signing | 🔶 | Phases 1–4 done — `dnssecPolicy` at `src/crd.rs:1228`, signing in `bind9/zone_ops.rs` and `bind9_resources.rs`. Phase 5 open |
| [21](.github/community/21-STATUS-CONDITIONS.md) | Status conditions | ✅ | Phases 1–5 complete (`reconcilers/status.rs`, `status_reasons.rs`); phases 6–7 are explicitly future work inside the doc |
| [22](.github/community/22-EXTERNAL-BIND9-GATEWAY.md) | External BIND9 gateway | ⛔ | Still a draft; no external-endpoint or gateway fields in `src/crd.rs` |
| [23](.github/community/23-RNDC-SECRET-HOT-RELOAD.md) | RNDC secret hot reload | ⛔ | Designed in [ADR-0001](docs/adr/0001-rndc-secret-reload.md); no reload path in `src/` |
| [24](.github/community/24-COMPLIANCE-GAMIFICATION.md) | Compliance & security gamification | ⛔ | No policy or report CRDs exist. Largest unstarted item here — needs an ADR before any of it is built |

### Scout

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [30](.github/community/30-SCOUT-INGRESS-CONTROLLER.md) | Scout — Ingress → ARecord controller | 🔶 | Phases 1/1.5 shipped, phase 2 (`BINDY_SCOUT_REMOTE_SECRET`) implemented. Scout has outgrown the doc: 5 controllers today (Ingress, Service, HTTPRoute, TLSRoute, TCPRoute) |
| [31](.github/community/31-SCOUT-NAMESPACE-SELECTORS.md) | Scout — namespace label selectors | 🔶 | `namespace_selector` landed (#437), evaluated per event by `source_namespace_eligible()`. The `Namespace` **watch** — the part that actually cuts event volume — is not implemented |
| [32](.github/community/32-SCOUT-SRV-RECORDS.md) | Scout — SRV record support | ⛔ | No `SRVRecord` reference anywhere in `src/scout.rs` |

### Security and compliance

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [41](.github/community/41-SECURITY-SCANNING.md) | Security scanning | 🔶 | Through phase 5 (license compliance). `security-scan.yaml`, `sbom.yml`, `license-scan.yaml`, `codeql.yml`, `scorecard.yml` all in place |
| [42](.github/community/42-AUDIT-LOGGING-SECRET-OPERATIONS.md) | Audit logging for Secret operations | ⛔ | Approved 2026-03-09, never implemented — no audit-log emission in `src/`. Compliance-relevant; worth re-triaging rather than leaving to drift |
| [43](.github/community/43-VEX-DOCUMENTS.md) | VEX documents | ⛔ | No VEX generation step in `.github/workflows/`. Builds on the SBOM pipeline from 41 |

### Testing, operations and dependencies

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [50](.github/community/50-LOAD-TESTING-FRAMEWORK.md) | Load testing framework | ⛔ | Target `crates/loadtest/` does not exist — the repo has no `crates/` directory, so this lands after the workspace conversion in 10. Merged from two external docs on migration |
| [51](.github/community/51-INTEGRATION-TESTING.md) | Integration testing | ✅ | Superseded by the shipped harness: `tests/integration_test.sh`, `tests/multi_tenancy_integration.rs`, `make kind-integration-test` |
| [52](.github/community/52-HICKORY-CLIENT-MIGRATION-TARGET.md) | Hickory client migration target | ⛔ | A scheduled revisit, not a build. `Cargo.toml` pins hickory 0.26; re-evaluate in Q3 2026 |

## Tracked privately

Roadmap numbers **01**, **11** and **40** are assigned but intentionally not
published here: they cover in-flight security hardening work, and are tracked
privately until that work lands. The numbers are reserved — do not reuse them.

## Keeping this current

When a roadmap item's status changes (something lands, something new
starts), update its row here in the same PR/commit that makes the change
— this file is a status board, not documentation of intent. Detailed
task-level tracking stays inside each roadmap doc; this file only tracks
the item-level state.
