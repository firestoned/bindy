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
| 02 | [`02-FUTURE-REFACTORING-OPPORTUNITIES.md`](02-FUTURE-REFACTORING-OPPORTUNITIES.md) | Post-DNSZone-refactor survey; mostly superseded by 10 and 12 |

### Architecture and refactoring

| # | File | What |
|---|---|---|
| 10 | [`10-CONTROLLER-CRATE-SPLIT.md`](10-CONTROLLER-CRATE-SPLIT.md) | Split the single crate into a workspace and simplify the watch layer |
| 12 | [`12-RECORDS-RECONCILER-REFACTORING.md`](12-RECORDS-RECONCILER-REFACTORING.md) | One generic record reconciler in place of 9 near-identical ones |
| 13 | [`13-EARLY-RETURN-REFACTORING.md`](13-EARLY-RETURN-REFACTORING.md) | Guard-clause refactor of 9 deeply nested functions |
| 14 | [`14-REMOVE-CLUSTERREF-USE-OWNERREFERENCE.md`](14-REMOVE-CLUSTERREF-USE-OWNERREFERENCE.md) | Replace `Bind9Instance.spec.clusterRef` with `ownerReference` |
| 15 | [`15-KUBERNETES-API-RATE-LIMITING.md`](15-KUBERNETES-API-RATE-LIMITING.md) | Pagination, retry and explicit client-side rate limiting |
| 16 | [`16-KUBE-CONDITION-DERIVE-MACRO.md`](16-KUBE-CONDITION-DERIVE-MACRO.md) | Adopt a `kube-condition` derive macro for status conditions |

### Features

| # | File | What |
|---|---|---|
| 20 | [`20-DNSSEC-ZONE-SIGNING.md`](20-DNSSEC-ZONE-SIGNING.md) | DNSSEC zone signing, key management and rotation |
| 21 | [`21-STATUS-CONDITIONS.md`](21-STATUS-CONDITIONS.md) | Standard status conditions across every CRD |
| 22 | [`22-EXTERNAL-BIND9-GATEWAY.md`](22-EXTERNAL-BIND9-GATEWAY.md) | Manage BIND9 on bare metal / VMs outside the cluster |
| 23 | [`23-RNDC-SECRET-HOT-RELOAD.md`](23-RNDC-SECRET-HOT-RELOAD.md) | Rotate RNDC keys without restarting pods (ADR-0001) |
| 24 | [`24-COMPLIANCE-GAMIFICATION.md`](24-COMPLIANCE-GAMIFICATION.md) | Cluster-scoped security/compliance policies with scored reports |

### Scout

| # | File | What |
|---|---|---|
| 30 | [`30-SCOUT-INGRESS-CONTROLLER.md`](30-SCOUT-INGRESS-CONTROLLER.md) | Ingress → ARecord controller, same-cluster and remote modes |
| 31 | [`31-SCOUT-NAMESPACE-SELECTORS.md`](31-SCOUT-NAMESPACE-SELECTORS.md) | Label-selector namespace inclusion/exclusion via a `Namespace` watch |
| 32 | [`32-SCOUT-SRV-RECORDS.md`](32-SCOUT-SRV-RECORDS.md) | Create `SRVRecord` CRs from Services and Ingresses |

### Security and compliance

| # | File | What |
|---|---|---|
| 41 | [`41-SECURITY-SCANNING.md`](41-SECURITY-SCANNING.md) | Container, dependency, secret, SAST and license scanning |
| 42 | [`42-AUDIT-LOGGING-SECRET-OPERATIONS.md`](42-AUDIT-LOGGING-SECRET-OPERATIONS.md) | Structured audit trail for every Secret operation |
| 43 | [`43-VEX-DOCUMENTS.md`](43-VEX-DOCUMENTS.md) | VEX documents in the release pipeline |

### Testing, operations and dependencies

| # | File | What |
|---|---|---|
| 50 | [`50-LOAD-TESTING-FRAMEWORK.md`](50-LOAD-TESTING-FRAMEWORK.md) | `crates/loadtest` — performance and failure-mode validation |
| 51 | [`51-INTEGRATION-TESTING.md`](51-INTEGRATION-TESTING.md) | Live-cluster integration testing for the DNSZone consolidation |
| 52 | [`52-HICKORY-CLIENT-MIGRATION-TARGET.md`](52-HICKORY-CLIENT-MIGRATION-TARGET.md) | Scheduled Q3 2026 revisit of the hickory migration target |

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
roadmap set, and each carries a `> **Status:**` block under its title
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
