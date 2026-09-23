# bindy Roadmap Index

This directory holds bindy's roadmap documents. Each one describes a body
of work — *what* and *why*, with a task list and a definition of done.
[`../../ROADMAPS.md`](../../ROADMAPS.md) is the status board that indexes
them and carries the current completion state.

## Numbering

Numbers are a zero-padded two-digit prefix, **contiguous from `00` with no
gaps** and no thematic banding. They are an ordering, not an identity —
inserting or retiring a roadmap renumbers the run, and every reference to the
moved numbers is fixed in the same commit. The section headings below carry the
theme; the numbers only carry the order.

Roadmaps are ordered reference and analysis → architecture and refactoring →
features → Scout → security and compliance → testing, operations and
dependencies. A new roadmap takes the number at the end of its section and
everything after it shifts up.

## Index

### Reference and analysis

| # | File | What |
|---|---|---|
| 00 | [`00-future-refactoring-opportunities.md`](00-future-refactoring-opportunities.md) | Post-DNSZone-refactor survey; mostly superseded by 01 and 02 |

### Architecture and refactoring

| # | File | What |
|---|---|---|
| 01 | [`01-controller-crate-split.md`](01-controller-crate-split.md) | Split the single crate into a workspace and simplify the watch layer |
| 02 | [`02-records-reconciler-refactoring.md`](02-records-reconciler-refactoring.md) | One generic record reconciler in place of 9 near-identical ones |
| 03 | [`03-early-return-refactoring.md`](03-early-return-refactoring.md) | Guard-clause refactor of 9 deeply nested functions |
| 04 | [`04-remove-clusterref-use-ownerreference.md`](04-remove-clusterref-use-ownerreference.md) | Replace `Bind9Instance.spec.clusterRef` with `ownerReference` |
| 05 | [`05-kubernetes-api-rate-limiting.md`](05-kubernetes-api-rate-limiting.md) | Pagination, retry and explicit client-side rate limiting |
| 06 | [`06-kube-condition-derive-macro.md`](06-kube-condition-derive-macro.md) | Adopt a `kube-condition` derive macro for status conditions |

### Features

| # | File | What |
|---|---|---|
| 07 | [`07-dnssec-zone-signing.md`](07-dnssec-zone-signing.md) | DNSSEC zone signing, key management and rotation |
| 08 | [`08-status-conditions.md`](08-status-conditions.md) | Standard status conditions across every CRD |
| 09 | [`09-external-bind9-gateway.md`](09-external-bind9-gateway.md) | Manage BIND9 on bare metal / VMs outside the cluster |
| 10 | [`10-rndc-secret-hot-reload.md`](10-rndc-secret-hot-reload.md) | Rotate RNDC keys without restarting pods (ADR-0001) |
| 11 | [`11-compliance-gamification.md`](11-compliance-gamification.md) | Cluster-scoped security/compliance policies with scored reports |

### Scout

| # | File | What |
|---|---|---|
| 12 | [`12-scout-ingress-controller.md`](12-scout-ingress-controller.md) | Ingress → ARecord controller, same-cluster and remote modes |
| 13 | [`13-scout-namespace-selectors.md`](13-scout-namespace-selectors.md) | Label-selector namespace inclusion/exclusion via a `Namespace` watch |
| 14 | [`14-scout-srv-records.md`](14-scout-srv-records.md) | Create `SRVRecord` CRs from Services and Ingresses |

### Security and compliance

| # | File | What |
|---|---|---|
| 15 | [`15-security-scanning.md`](15-security-scanning.md) | Container, dependency, secret, SAST and license scanning |
| 16 | [`16-audit-logging-secret-operations.md`](16-audit-logging-secret-operations.md) | Structured audit trail for every Secret operation |
| 17 | [`17-vex-documents.md`](17-vex-documents.md) | VEX documents in the release pipeline |

### Testing, operations and dependencies

| # | File | What |
|---|---|---|
| 18 | [`18-load-testing-framework.md`](18-load-testing-framework.md) | `crates/loadtest` — performance and failure-mode validation |
| 19 | [`19-integration-testing.md`](19-integration-testing.md) | Live-cluster integration testing for the DNSZone consolidation |
| 20 | [`20-hickory-client-migration-target.md`](20-hickory-client-migration-target.md) | Scheduled Q3 2026 revisit of the hickory migration target |
| 21 | [`21-bindcar-migration-v0-7-0.md`](21-bindcar-migration-v0-7-0.md) | bindcar v0.6.0 → v0.7.0 upgrade guide — superseded by 24 |
| 22 | [`22-bindcar-migration-v0-7-1.md`](22-bindcar-migration-v0-7-1.md) | bindcar v0.6.0 → v0.7.1 upgrade guide — superseded by 24 |
| 23 | [`23-bindcar-migration-v0-7-2.md`](23-bindcar-migration-v0-7-2.md) | bindcar v0.6.0 → v0.7.2 upgrade guide — superseded by 24 |
| 24 | [`24-bindcar-migration-v0-7-4.md`](24-bindcar-migration-v0-7-4.md) | bindcar v0.7.2 → v0.7.4 upgrade guide — superseded by 25 |
| 25 | [`25-bindcar-migration-v0-8-0.md`](25-bindcar-migration-v0-8-0.md) | bindcar v0.7.4 → v0.8.0 upgrade guide — **current**: TLS, mTLS, cert reload, feature gating |

## Privately tracked roadmaps

Some in-flight security hardening work is tracked privately until it lands, so
it has no file here. Those documents carry **no number** while they are outside
the repo — numbering here is contiguous and holds no gaps, so such a document is
numbered only when it is moved in, taking the next free number in its section at
that point.

## How these relate to the rest of the repo

- **Roadmaps say what and why.** An architecturally significant *how*
  still goes through an ADR in [`docs/adr/`](../../docs/adr/) first — a
  roadmap entry does not substitute for one.
- **Task lists are the source of truth.** Check items off in the file as
  they land, in the same PR that lands them.
- **Status changes go in `ROADMAPS.md`** in that same PR. That file is a
  board, not documentation of intent.
- Code style, testing and documentation rules live in
  [`.claude/rules/`](../../.claude/rules/), not here.

## Reading a migrated doc

Everything here was migrated from an external roadmap set on 2026-09-10 (the
bindcar upgrade guides followed on 2026-09-12, from the bindcar repo), and each
carries a `> **Status:**` block under its title recording what was verified
against the tree at that point. **The body
below that block is the document as originally written** — file paths and
line numbers in older docs have drifted (several modules have since been
split into directories). Trust the status block; re-verify the body.

## Adding a roadmap

1. Take the number at the end of the section it belongs to, and renumber
   every roadmap after it — files, index rows and every reference in the
   repo — in the same commit.
2. Filename: `NN-lowercase-hyphenated-title.md`. Lowercase only, hyphens as
   the only separator; `README.md` is the one exception.
3. Open with a `> **Goal.**` / `> **Stop condition.**` block so a reader
   knows what "done" means before reading the analysis.
4. Add a row to the table above **and** to `ROADMAPS.md`.
