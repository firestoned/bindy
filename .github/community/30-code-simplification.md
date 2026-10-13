# 30: Code simplification and fail-fast review

> **Goal.** Every crate, read from entry point to execution, has no
> duplicated logic, no dead code, and no function that logs an error and
> carries on as if it had succeeded. Each fix lands in its own PR, gated by
> the workspace tests and the chaos e2e suite on Kubernetes 1.31 and 1.37.
>
> **Stop condition.** The six PRs below are merged; the remaining review
> findings are either done or recorded as follow-ups with a reason.

**Status:** 🔶 In progress (PRs 1 to 4 merged; PR 5 in review; PR 6 next).
**Owner:** Erick Bourgeois

## Why

A full review of the ten crates (about 51,000 non-test lines at `7a188c3`)
found the same logic hand-written up to five times, about 3,000 lines with
no production caller, and paths that turned API failures into success
(finalizers dropped, primaries skipped, partial lists used as complete). The
copies had already drifted apart, and several of the drifts were bugs.

## PRs

| PR | Scope | Status |
|---|---|---|
| 1 (#544) | Fail-fast fixes B1 to B16: errors stop the operation instead of proceeding on partial data; threat model v1.22 (M-56 to M-60) | ✅ Merged |
| 2 (#545) | Dead code removed across every crate; `serde_yaml` replaced by `serde-saphyr` (RUSTSEC-2024-0449); `bootstrap mc` writes its Secret to a `0600` file (M-61) | ✅ Merged |
| 3 (#546) | Shared SDK helpers (errors, resources, finalizers, conditions, fan-out, instrumentation) and every controller moved onto them; scout's finalizer race fixed; Kubernetes 1.33+ Deployment status noise filtered; `NotifyTargetPending` | ✅ Merged |
| 4 (#548) | Zone instances partitioned once per reconcile; role accessors; `bind9_resources.rs` split into modules; generic bootstrap `apply` | ✅ Merged |
| 5 | [ADR-0020](../../docs/adr/0020-record-kinds-declared-once.md) record kinds declared once; [ADR-0021](../../docs/adr/0021-rrset-replaced-in-one-update.md) an RRset replaced in one DNS UPDATE | 🔶 In review |
| 6 | Generic Scout source reconciler (one pipeline for Ingress, Service, HTTPRoute, TLSRoute, TCPRoute); source-kind label to close threat-model accepted risk 20 | ⛔ Needs an ADR |

## Lines (Rust under `crates/`)

| After | Non-test | Tests |
|---|---|---|
| Start (`7a188c3`) | 50,846 | 42,721 |
| PR 1 | 51,932 | 44,790 |
| PR 2 | 48,777 | 41,090 |
| PR 3 | 48,860 | 42,655 |
| PR 4 | 48,285 | 43,890 |
| PR 5 | 47,114 | 44,882 |

## Follow-ups recorded during the work

- [ ] A `secretRef` RNDC Secret with custom `keyNameKey` / `secretKey` is
      not read with those keys (`parse_rndc_secret_data` reads the standard
      keys only).
- [ ] The scout's cross-kind ARecord name collision (threat model accepted
      risk 20): addressed by PR 6.
- [ ] `create_or_force_apply` gives the provider's constant labels to the
      `bindy-controller` field manager (values unchanged; noted in PR 3).
