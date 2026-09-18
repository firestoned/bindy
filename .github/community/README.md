# bindy Roadmap Index

This directory holds bindy's roadmap documents. Each one describes a body
of work — *what* and *why*, with a task list and a definition of done.
[`../../ROADMAPS.md`](../../ROADMAPS.md) is the status board that indexes
them and carries the current completion state.

## Numbering

Numbers are stable once assigned and grouped by band:

| Band | Theme |
|---|---|
| `00`–`09` | Reference and analysis — no completion state |
| `10`–`19` | Architecture and refactoring |
| `20`–`29` | Features |
| `30`–`39` | Scout |
| `40`–`49` | Security and compliance |
| `50`–`59` | Testing, operations and dependencies |

## Index

### Reference and analysis

| # | File | What |
|---|---|---|
| 02 | [`02-future-refactoring-opportunities.md`](02-future-refactoring-opportunities.md) | Post-DNSZone-refactor survey; mostly superseded by 10 and 12 |

### Architecture and refactoring

| # | File | What |
|---|---|---|
| 10 | [`10-controller-crate-split.md`](10-controller-crate-split.md) | Split the single crate into a workspace and simplify the watch layer |
| 12 | [`12-records-reconciler-refactoring.md`](12-records-reconciler-refactoring.md) | One generic record reconciler in place of 9 near-identical ones |
| 13 | [`13-early-return-refactoring.md`](13-early-return-refactoring.md) | Guard-clause refactor of 9 deeply nested functions |
| 14 | [`14-remove-clusterref-use-ownerreference.md`](14-remove-clusterref-use-ownerreference.md) | Replace `Bind9Instance.spec.clusterRef` with `ownerReference` |
| 15 | [`15-kubernetes-api-rate-limiting.md`](15-kubernetes-api-rate-limiting.md) | Pagination, retry and explicit client-side rate limiting |
| 16 | [`16-kube-condition-derive-macro.md`](16-kube-condition-derive-macro.md) | Adopt a `kube-condition` derive macro for status conditions |

### Features

| # | File | What |
|---|---|---|
| 20 | [`20-dnssec-zone-signing.md`](20-dnssec-zone-signing.md) | DNSSEC zone signing, key management and rotation |
| 21 | [`21-status-conditions.md`](21-status-conditions.md) | Standard status conditions across every CRD |
| 22 | [`22-external-bind9-gateway.md`](22-external-bind9-gateway.md) | Manage BIND9 on bare metal / VMs outside the cluster |
| 23 | [`23-rndc-secret-hot-reload.md`](23-rndc-secret-hot-reload.md) | Rotate RNDC keys without restarting pods (ADR-0001) |
| 24 | [`24-compliance-gamification.md`](24-compliance-gamification.md) | Cluster-scoped security/compliance policies with scored reports |

### Scout

| # | File | What |
|---|---|---|
| 30 | [`30-scout-ingress-controller.md`](30-scout-ingress-controller.md) | Ingress → ARecord controller, same-cluster and remote modes |
| 31 | [`31-scout-namespace-selectors.md`](31-scout-namespace-selectors.md) | Label-selector namespace inclusion/exclusion via a `Namespace` watch |
| 32 | [`32-scout-srv-records.md`](32-scout-srv-records.md) | Create `SRVRecord` CRs from Services and Ingresses |

### Security and compliance

| # | File | What |
|---|---|---|
| 41 | [`41-security-scanning.md`](41-security-scanning.md) | Container, dependency, secret, SAST and license scanning |
| 42 | [`42-audit-logging-secret-operations.md`](42-audit-logging-secret-operations.md) | Structured audit trail for every Secret operation |
| 43 | [`43-vex-documents.md`](43-vex-documents.md) | VEX documents in the release pipeline |

### Testing, operations and dependencies

| # | File | What |
|---|---|---|
| 50 | [`50-load-testing-framework.md`](50-load-testing-framework.md) | `crates/loadtest` — performance and failure-mode validation |
| 51 | [`51-integration-testing.md`](51-integration-testing.md) | Live-cluster integration testing for the DNSZone consolidation |
| 52 | [`52-hickory-client-migration-target.md`](52-hickory-client-migration-target.md) | Scheduled Q3 2026 revisit of the hickory migration target |
| 53 | [`53-bindcar-migration-v0-7-0.md`](53-bindcar-migration-v0-7-0.md) | bindcar v0.6.0 → v0.7.0 upgrade guide — superseded by 56 |
| 54 | [`54-bindcar-migration-v0-7-1.md`](54-bindcar-migration-v0-7-1.md) | bindcar v0.6.0 → v0.7.1 upgrade guide — superseded by 56 |
| 55 | [`55-bindcar-migration-v0-7-2.md`](55-bindcar-migration-v0-7-2.md) | bindcar v0.6.0 → v0.7.2 upgrade guide — superseded by 56 |
| 56 | [`56-bindcar-migration-v0-7-4.md`](56-bindcar-migration-v0-7-4.md) | bindcar v0.7.2 → v0.7.4 upgrade guide — **current**, actionable |

## Reserved numbers

`01`, `11` and `40` are **assigned but deliberately absent** — they cover
in-flight security hardening work and are tracked privately until it lands. Do
not reuse the numbers; they are reserved for those documents.

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

Everything numbered `01`–`52` was migrated on 2026-09-10 from an external
roadmap set (`53`–`55` followed on 2026-09-12, from the bindcar repo; `56` was
written there on the same date), and each carries a `> **Status:**` block under its title
recording what was verified against the tree at that point. **The body
below that block is the document as originally written** — file paths and
line numbers in older docs have drifted (several modules have since been
split into directories). Trust the status block; re-verify the body.

## Adding a roadmap

1. Pick the next free number in the right band.
2. Filename: `NN-SCREAMING-KEBAB-TITLE.md`.
3. Open with a `> **Goal.**` / `> **Stop condition.**` block so a reader
   knows what "done" means before reading the analysis.
4. Add a row to the table above **and** to `ROADMAPS.md`.
