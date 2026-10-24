# Roadmaps

High-level index of bindy's roadmap documents. Full detail for each item
lives in [`.github/community/`](.github/community/); this file tracks
what each one is and its current completion status; the detailed task
lists and design rationale live in the linked doc itself.

Architecturally significant work in any roadmap below still goes
**ADR → TDD → implement → docs**, in that order (see
[`.claude/rules/testing.md`](.claude/rules/testing.md) and
[`.claude/rules/documentation.md`](.claude/rules/documentation.md)): a
roadmap entry describes *what* and *why*, it does not skip the ADR for
*how*.

## Status legend

| Symbol | Meaning |
|---|---|
| ✅ | Done: implemented, tested, in the codebase today |
| 🔶 | In progress: some of it exists, not complete |
| ⛔ | Not started |
| 📄 | Reference doc: not a phase with a completion state |

## Index

Statuses were verified against `fix-idempotency` @ `648ff7a` on 2026-09-10.

### Reference and analysis

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [00](.github/community/00-future-refactoring-opportunities.md) | Future refactoring opportunities | 📄 | Mostly superseded: its top recommendation landed as 02, the rest is subsumed by 01 |

### Architecture and refactoring

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [01](.github/community/01-controller-crate-split.md) | Controller crate split & watch-layer simplification | ✅ | Done 2026-10-05 ([ADR-0009](docs/adr/0009-workspace-crate-split-and-shared-watch-layer.md), amended; threat model v1.11). Eleven crates: `bindy-api`, `bindy-controller-sdk`, `bindy-bind9`, one per controller (cluster, instance, zone, records), `bindy-scout`, `bindy-bootstrap`, and the `bindy` binary (`main.rs` 257 lines, was 2,182). One shared watch per kind: 19 operator watches on kind instead of 58 (v0.7.1). Controllers drain on SIGTERM and lease loss; the zone controller ignores its own status writes (2-second limiter gone); the instance mapper no longer spawns work; the startup drift pass is gone, gated by the restart e2e. Scout RBAC drift is a failing test. The other primaries need no status filter (measured on kind: their status writes are guarded; record status carries the zone's `zoneRef`); the instance controller filters zone events to selection changes so record timestamps cannot fan out. Logged, not fixed: `status.zones` omits cross-namespace zones |
| [02](.github/community/02-records-reconciler-refactoring.md) | Records reconciler refactoring | ✅ | Generic `reconcile_record<T>()` at `src/reconcilers/records/mod.rs:1170`; the 9 per-type fns are thin wrappers. 01 turns those into trait impls |
| [03](.github/community/03-early-return-refactoring.md) | Early-return / guard-clause refactor | ✅ | Completed 2026-09-27: all 9 target functions refactored or already compliant; behavior-preserving, with pinning tests for the global-fallback and role-precedence paths. Both deferred quirks fixed 2026-09-27 via ADR-0007 (explicit dnssec-validation honored; cluster-level transfer deny-by-default) |
| [04](.github/community/04-remove-clusterref-use-ownerreference.md) | Remove `clusterRef`, use `ownerReference` | ⛔ | `pub cluster_ref` still in `src/crd.rs` at `:994`, `:3679`, `:3839`. Breaking CRD change; needs an ADR first |
| [05](.github/community/05-kubernetes-api-rate-limiting.md) | Kubernetes API rate limiting | ✅ | Complete 2026-09-27 (ADR-0005): tower `RateLimitLayer` client (20 QPS/30 burst, env-tunable), pagination + retry applied to all call sites, `kube_api_*` Prometheus metrics. Scale validation → 18 |
| [06](.github/community/06-kube-condition-derive-macro.md) | `kube-condition` derive macro | ⛔ | Not a dependency; conditions are hand-built in `src/reconcilers/status.rs`. See 08 for what shipped instead |
| [30](.github/community/30-code-simplification.md) | Code simplification and fail-fast review | ✅ | Done 2026-10-14, PRs 1 to 7 merged (#544, #545, #546, #548, #549, #550, #551): 16 fail-fast fixes, dead code removed, shared SDK helpers, zone/instance/cluster/bootstrap restructured, record kinds declared once (ADR-0020), atomic RRset replacement (ADR-0021). Scout sources share one pipeline (ADR-0022, closes accepted risk 20). PR 7: RNDC `secretRef` keys mirrored (ADR-0023), one-source volume allow-list, one ACL grammar, typed bindcar retries, one shared HTTP client. Non-test Rust 50,846 to 46,281. Four follow-ups recorded in the detail doc (retry-loop merge, TSIG BADKEY log noise, a stale `rndcSecretRef` rustdoc, field-manager ownership) |

### Features

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [07](.github/community/07-dnssec-zone-signing.md) | DNSSEC zone signing | ✅ | Complete 2026-09-27 (ADR-0006): DS records auto-extracted from DNSKEYs and published in `DNSZone.status.dnssec` + `DNSSEC` print column. e2e suite → 19's harness. 2026-10-04 (ADR-0012): `keysFrom.secretRef` fixed, keys shared by every primary |
| [08](.github/community/08-status-conditions.md) | Status conditions | ✅ | Phases 1–5 complete (`reconcilers/status.rs`, `status_reasons.rs`); phases 6–7 are explicitly future work inside the doc |
| [09](.github/community/09-external-bind9-gateway.md) | External BIND9 gateway | ⛔ | Still a draft; no external-endpoint or gateway fields in `src/crd.rs` |
| [10](.github/community/10-rndc-secret-hot-reload.md) | RNDC secret hot reload | ⛔ | Designed in [ADR-0001](docs/adr/0001-rndc-secret-reload.md); no reload path in `src/` |
| [11](.github/community/11-compliance-gamification.md) | Compliance & security gamification | ⛔ | No policy or report CRDs exist. Largest unstarted item here; needs an ADR before any of it is built |

### Scout

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [12](.github/community/12-scout-ingress-controller.md) | Scout: Ingress → ARecord controller | ✅ | Complete 2026-09-28: Phase 3 closed via ADR-0008 (endpoint + token-file remote mode, fail-closed vs. kubeconfig Secret; original Linkerd wording superseded; API servers aren't meshed). Live Linkerd verification → staging. Leftover Qs → 27 |
| [27](.github/community/27-scout-followups.md) | Scout follow-ups | 🔶 | Done 2026-10-23: cross-cluster conflict detection ([ADR-0027](docs/adr/0027-scout-name-conflicts-detected-not-overwritten.md): first publisher keeps a name, the other is held back `RRsetConflict`). Open: Scout Prometheus metrics, AAAA support, bootstrap parity for the endpoint mode, live Linkerd verification |
| [13](.github/community/13-scout-namespace-selectors.md) | Scout: namespace label selectors | 🔶 | `namespace_selector` landed (#437), evaluated per event by `source_namespace_eligible()`. The `Namespace` **watch**, the part that actually cuts event volume, is not implemented |
| [14](.github/community/14-scout-srv-records.md) | Scout: SRV record support | ⛔ | No `SRVRecord` reference anywhere in `src/scout.rs` |
| [31](.github/community/31-scout-source-guardrails.md) | Scout source guardrails: records bound to their cluster and zones | ✅ | Done 2026-10-20 ([ADR-0025](docs/adr/0025-scout-records-bound-to-cluster-and-zones.md)): Scout `--allowed-zones` / `--allowed-ip-ranges`, the `bindy-scout-sources` registry, admission policy `bindy-scout-source-binding`, zone-label rule and refused-delete handling; verified live with two edge clusters against one bindy cluster (`make e2e-scout-guardrails`). Open in 27 item 2: one zone registered for two clusters stays last-writer-wins |
| [32](.github/community/32-scout-one-record-per-dns-name.md) | Scout publishes one record per DNS name (issue #556) | ✅ | Phase 8 (ADR-0028, issue #569) done 2026-10-24: gateway selectors publish every matching VIP scoped per Gateway, routes re-resolve from watch caches, threat model v1.33, proven on kind (`make e2e-scout-gateway`). Phases 1 to 7 done 2026-10-20 ([ADR-0026](docs/adr/0026-scout-one-record-per-dns-name.md)): every load balancer IPv4 address published; one record per DNS name with a contributor map, conditional writes and handover from per-source records; docs, migration guide, threat model v1.29 (T6, M-73); proven on two kind clusters on slate (`make e2e-scout-multi`) |

### Security and compliance

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [15](.github/community/15-security-scanning.md) | Security scanning | 🔶 | Through phase 5 (license compliance), plus phase 6 SBOM/signing/provenance (2026-10-03, ADR-0010): NTIA-gated CycloneDX SBOM per binary and image, attested to the artifact digest; SLSA Build L3 for tarballs, manifests, SBOMs and images. Open: VEX generation, Polaris, required PR reviews (threat model M-36) |
| [16](.github/community/16-audit-logging-secret-operations.md) | Audit logging for Secret operations | 🔶 | In review 2026-10-14 ([ADR-0024](docs/adr/0024-secret-operations-audit-log.md)): one `bindy::audit` event per Secret operation through one audited `SecretApi` (a test forbids any other), metadata-only checks and a metadata Secret watch, audit target always on unless explicitly off, `secret_operations_total` counter. Was: approved 2026-03-09, never implemented |
| [17](.github/community/17-vex-documents.md) | VEX documents | ⛔ | No VEX generation step in `.github/workflows/`. Builds on the SBOM pipeline from 15 |
| [28](.github/community/28-pqc-readiness.md) | Post-quantum cryptography (PQC) readiness | 🔶 | Phase 0 complete 2026-10-04 (ADR-0011): curated CycloneDX 1.6 CBOM per release, lockfile-stamped and PR-gated (`make cbom-stage`, `cbom` job), docs page + threat model v1.9 (M-39/M-40, accepted risk 8). Next: Phase 1 (TSIG HMAC-SHA1/224 deprecation); Phase 2 (hybrid `X25519MLKEM768`) needs a crypto-provider ADR on both bindy and bindcar ends; DNSSEC + sigstore phases are upstream watch items |
| [29](.github/community/29-hornet-config-rendering.md) | Validate and render BIND9 configuration with hornet | ✅ | [ADR-0013](docs/adr/0013-validate-and-render-bind9-config-with-hornet.md). Done 2026-10-05: every rendered config is parsed in CI, the operator refuses to publish one that does not parse (`Ready=False`, `ConfigurationInvalid`), and `named.conf` / `named.conf.options` are written by hornet 0.3.0's writer (templates retired; checked with `named-checkconf` 9.18 and 9.20). `validation: true` now renders `dnssec-validation auto` |

### Testing, operations and dependencies

| # | Roadmap | Status | Notes |
|---|---|---|---|
| [18](.github/community/18-load-testing-framework.md) | Load testing framework | 🔶 | Chaos scenario done 2026-10-07 as a bash e2e (`make e2e-chaos`, CI job): 15 failure steps on a 3-node kind cluster, fixed and seeded random order, invariants on served records, serials, transfer peers, truthful status, a quiet operator and per-Service DNS gaps. It found the rc.7 stale-transfer-peer bug (ADR-0019) and four more (leader lease, env removal, cluster wake, two-minute dead-endpoint retry). `crates/loadtest/` (burst, sustained, the rollout scenario at scale) still not started; the burst milestone should assert the ADR-0014/15/16 measurements, the rollout scenario ADR-0017/18 (no zone without a nameserver, no no-op Deployment PATCH, no stale `RolloutQueued`). Audited 2026-10-07 |
| [19](.github/community/19-integration-testing.md) | Integration testing | ✅ | Superseded by the shipped harness: `tests/integration_test.sh`, `tests/multi_tenancy_integration.rs`, `make kind-integration-test` |
| [20](.github/community/20-hickory-client-migration-target.md) | Hickory client migration target | ✅ | Revisit done 2026-10-08: stay on `hickory-net` (already on newest 0.26.3, all 0.26.2 security fixes; `domain` 0.12.3 still has an unstable client transport and no UPDATE builder). Requirement floor raised to `0.26.3`. Next revisit when hickory 0.27.0 ships |
| [21](.github/community/21-bindcar-migration-v0-7-0.md) | bindcar upgrade — v0.7.0 | 📄 | Superseded by 24. Absorbed 2026-07-01/02 (Mode B / TokenReview) |
| [22](.github/community/22-bindcar-migration-v0-7-1.md) | bindcar upgrade — v0.7.1 | 📄 | Superseded by 24. Absorbed 2026-07-05 |
| [23](.github/community/23-bindcar-migration-v0-7-2.md) | bindcar upgrade — v0.7.2 | 📄 | Superseded by 24. §14's `bindcarConfig.envVars` override hole is bindy-side and may still be open |
| [24](.github/community/24-bindcar-migration-v0-7-4.md) | bindcar upgrade — v0.7.4 | 📄 | Superseded by 25. Its §14 (envVars override) and §15 (metric rename) remain open bindy-side |
| [25](.github/community/25-bindcar-migration-v0-8-0.md) | bindcar upgrade, v0.8.0 | ✅ | Complete 2026-10-08: live verification on kind against main (`sha-8ccf613`). `make tls-transport-test` verified P2-4 end to end (cert-manager cert, TLS-only sidecar, CA-verified operator calls, no-CA instance refused); `make regression-test` 55/55. New work → 26 |
| [26](.github/community/26-bindcar-migration-v0-8-2.md) | bindcar upgrade — v0.8.2 | ✅ | Applied 2026-09-28: crate floor 0.8.1 (API-identical; lock→0.8.2 when published), image v0.8.2, `status.dnssec.nextKeyRollover` wired (ADR-0006 amended). Deferred behind ADRs: live-zone DNSSEC enable, checkds automation. Skip v0.8.1 images (self-report 0.8.0) |

## Tracked privately

Some in-flight security hardening work is tracked privately until it lands and
so has no row above. Those documents carry **no number** while they are outside
this repo; numbers here are contiguous and hold no gaps, so a private document
is numbered only when it is moved in, taking the next free number at that point.

## Numbering

Numbers are a zero-padded two-digit prefix, contiguous from `00` with no gaps
and no thematic banding. They are an ordering, not an identity: inserting or
retiring a roadmap renumbers the run, and every reference to the moved numbers
is fixed in the same commit. Reference a roadmap by its padded number in prose
("roadmap 07") so the number greps against the filename.

## Keeping this current

When a roadmap item's status changes (something lands, something new
starts), update its row here in the same PR/commit that makes the change:
this file is a status board, not documentation of intent. Detailed
task-level tracking stays inside each roadmap doc; this file only tracks
the item-level state.
