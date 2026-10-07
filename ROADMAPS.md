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
| [01](.github/community/01-controller-crate-split.md) | Controller crate split & watch-layer simplification | ✅ | Done 2026-10-05 ([ADR-0009](docs/adr/0009-workspace-crate-split-and-shared-watch-layer.md), amended; threat model v1.11). Eleven crates: `bindy-api`, `bindy-controller-sdk`, `bindy-bind9`, one per controller (cluster, instance, zone, records), `bindy-scout`, `bindy-bootstrap`, and the `bindy` binary (`main.rs` 257 lines, was 2,182). One shared watch per kind: 19 operator watches on kind instead of 58 (v0.7.1). Controllers drain on SIGTERM and lease loss; the zone controller ignores its own status writes (2-second limiter gone); the instance mapper no longer spawns work; the startup drift pass is gone, gated by the restart e2e. Scout RBAC drift is a failing test. The other primaries need no status filter (measured on kind: their status writes are guarded; record status carries the zone's `zoneRef`); the instance controller filters zone events to selection changes so record timestamps cannot fan out. Logged, not fixed: `status.zones` omits cross-namespace zones |
| [02](.github/community/02-records-reconciler-refactoring.md) | Records reconciler refactoring | ✅ | Generic `reconcile_record<T>()` at `src/reconcilers/records/mod.rs:1170`; the 9 per-type fns are thin wrappers. 01 turns those into trait impls |
| [03](.github/community/03-early-return-refactoring.md) | Early-return / guard-clause refactor | ✅ | Completed 2026-09-27 — all 9 target functions refactored or already compliant; behavior-preserving, with pinning tests for the global-fallback and role-precedence paths. Both deferred quirks fixed 2026-09-27 via ADR-0007 (explicit dnssec-validation honored; cluster-level transfer deny-by-default) |
| [04](.github/community/04-remove-clusterref-use-ownerreference.md) | Remove `clusterRef`, use `ownerReference` | ⛔ | `pub cluster_ref` still in `src/crd.rs` at `:994`, `:3679`, `:3839`. Breaking CRD change — needs an ADR first |
| [05](.github/community/05-kubernetes-api-rate-limiting.md) | Kubernetes API rate limiting | ✅ | Complete 2026-09-27 (ADR-0005): tower `RateLimitLayer` client (20 QPS/30 burst, env-tunable), pagination + retry applied to all call sites, `kube_api_*` Prometheus metrics. Scale validation → 18 |
| [06](.github/community/06-kube-condition-derive-macro.md) | `kube-condition` derive macro | ⛔ | Not a dependency; conditions are hand-built in `src/reconcilers/status.rs`. See 08 for what shipped instead |

### Features

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [07](.github/community/07-dnssec-zone-signing.md) | DNSSEC zone signing | ✅ | Complete 2026-09-27 (ADR-0006): DS records auto-extracted from DNSKEYs and published in `DNSZone.status.dnssec` + `DNSSEC` print column. e2e suite → 19's harness. 2026-10-04 (ADR-0012): `keysFrom.secretRef` fixed, keys shared by every primary |
| [08](.github/community/08-status-conditions.md) | Status conditions | ✅ | Phases 1–5 complete (`reconcilers/status.rs`, `status_reasons.rs`); phases 6–7 are explicitly future work inside the doc |
| [09](.github/community/09-external-bind9-gateway.md) | External BIND9 gateway | ⛔ | Still a draft; no external-endpoint or gateway fields in `src/crd.rs` |
| [10](.github/community/10-rndc-secret-hot-reload.md) | RNDC secret hot reload | ⛔ | Designed in [ADR-0001](docs/adr/0001-rndc-secret-reload.md); no reload path in `src/` |
| [11](.github/community/11-compliance-gamification.md) | Compliance & security gamification | ⛔ | No policy or report CRDs exist. Largest unstarted item here — needs an ADR before any of it is built |

### Scout

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [12](.github/community/12-scout-ingress-controller.md) | Scout — Ingress → ARecord controller | ✅ | Complete 2026-09-28: Phase 3 closed via ADR-0008 (endpoint + token-file remote mode, fail-closed vs. kubeconfig Secret; original Linkerd wording superseded — API servers aren't meshed). Live Linkerd verification → staging. Leftover Qs → 27 |
| [27](.github/community/27-scout-followups.md) | Scout follow-ups | ⛔ | Survivors of 12's closure: Scout Prometheus metrics, cross-cluster conflict detection, AAAA support, bootstrap parity for the endpoint mode, live Linkerd verification |
| [13](.github/community/13-scout-namespace-selectors.md) | Scout — namespace label selectors | 🔶 | `namespace_selector` landed (#437), evaluated per event by `source_namespace_eligible()`. The `Namespace` **watch** — the part that actually cuts event volume — is not implemented |
| [14](.github/community/14-scout-srv-records.md) | Scout — SRV record support | ⛔ | No `SRVRecord` reference anywhere in `src/scout.rs` |

### Security and compliance

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [15](.github/community/15-security-scanning.md) | Security scanning | 🔶 | Through phase 5 (license compliance), plus phase 6 SBOM/signing/provenance (2026-10-03, ADR-0010): NTIA-gated CycloneDX SBOM per binary and image, attested to the artifact digest; SLSA Build L3 for tarballs, manifests, SBOMs and images. Open: VEX generation, Polaris, required PR reviews (threat model M-36) |
| [16](.github/community/16-audit-logging-secret-operations.md) | Audit logging for Secret operations | ⛔ | Approved 2026-03-09, never implemented — no audit-log emission in `src/`. Compliance-relevant; worth re-triaging rather than leaving to drift |
| [17](.github/community/17-vex-documents.md) | VEX documents | ⛔ | No VEX generation step in `.github/workflows/`. Builds on the SBOM pipeline from 15 |
| [28](.github/community/28-pqc-readiness.md) | Post-quantum cryptography (PQC) readiness | 🔶 | Phase 0 complete 2026-10-04 (ADR-0011): curated CycloneDX 1.6 CBOM per release, lockfile-stamped and PR-gated (`make cbom-stage`, `cbom` job), docs page + threat model v1.9 (M-39/M-40, accepted risk 8). Next: Phase 1 (TSIG HMAC-SHA1/224 deprecation); Phase 2 (hybrid `X25519MLKEM768`) needs a crypto-provider ADR on both bindy and bindcar ends; DNSSEC + sigstore phases are upstream watch items |
| [29](.github/community/29-hornet-config-rendering.md) | Validate and render BIND9 configuration with hornet | ✅ | [ADR-0013](docs/adr/0013-validate-and-render-bind9-config-with-hornet.md). Done 2026-10-05: every rendered config is parsed in CI, the operator refuses to publish one that does not parse (`Ready=False`, `ConfigurationInvalid`), and `named.conf` / `named.conf.options` are written by hornet 0.3.0's writer (templates retired; checked with `named-checkconf` 9.18 and 9.20). `validation: true` now renders `dnssec-validation auto` |

### Testing, operations and dependencies

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [18](.github/community/18-load-testing-framework.md) | Load testing framework | ⛔ | `crates/` exists since 01, `crates/loadtest/` does not. Manual rc.2/rc.3 load tests drove ADR-0014, ADR-0015 and ADR-0016 (no periodic resync, 2026-10-06); the burst milestone should assert their measurements, and a rollout scenario the ADR-0017 zones-loaded gate (no `REFUSED` while every primary rolls), its termination handover and ADR-0018 staggered rollouts (no interval in which every nameserver of a zone times out; rc.5 showed 9 to 12 s before them). Audited 2026-10-07 |
| [19](.github/community/19-integration-testing.md) | Integration testing | ✅ | Superseded by the shipped harness: `tests/integration_test.sh`, `tests/multi_tenancy_integration.rs`, `make kind-integration-test` |
| [20](.github/community/20-hickory-client-migration-target.md) | Hickory client migration target | ⛔ | A scheduled revisit, not a build. `Cargo.toml` pins hickory 0.26; re-evaluate in Q3 2026 |
| [21](.github/community/21-bindcar-migration-v0-7-0.md) | bindcar upgrade — v0.7.0 | 📄 | Superseded by 24. Absorbed 2026-07-01/02 (Mode B / TokenReview) |
| [22](.github/community/22-bindcar-migration-v0-7-1.md) | bindcar upgrade — v0.7.1 | 📄 | Superseded by 24. Absorbed 2026-07-05 |
| [23](.github/community/23-bindcar-migration-v0-7-2.md) | bindcar upgrade — v0.7.2 | 📄 | Superseded by 24. §14's `bindcarConfig.envVars` override hole is bindy-side and may still be open |
| [24](.github/community/24-bindcar-migration-v0-7-4.md) | bindcar upgrade — v0.7.4 | 📄 | Superseded by 25. Its §14 (envVars override) and §15 (metric rename) remain open bindy-side |
| [25](.github/community/25-bindcar-migration-v0-8-0.md) | bindcar upgrade — v0.8.0 | 🔶 | Re-audited 2026-09-28: code complete — TLS CRD surface + client, reserved-env guard AND admission policy (shipped as VAP 19/20, not the planned "15/16") all in the tree. Only live-cluster verification remains (`make tls-transport-test` / `e2e-tls` + `regression-test`); until then P2-4 is unverified. New work → 26 |
| [26](.github/community/26-bindcar-migration-v0-8-2.md) | bindcar upgrade — v0.8.2 | ✅ | Applied 2026-09-28: crate floor 0.8.1 (API-identical; lock→0.8.2 when published), image v0.8.2, `status.dnssec.nextKeyRollover` wired (ADR-0006 amended). Deferred behind ADRs: live-zone DNSSEC enable, checkds automation. Skip v0.8.1 images (self-report 0.8.0) |

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
