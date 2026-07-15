# CALM — Architecture as Code

This directory holds the [FINOS **CALM** (Common Architecture Language Model)](https://calm.finos.org/)
description of Bindy's architecture. CALM documents are machine-readable JSON
(schema **1.2**) and are the **source of truth** for the architecture diagrams
published in the docs — the Mermaid pages under `docs/src/architecture/` are
generated from these files, never hand-drawn.

## Models

| File | What it describes |
|------|-------------------|
| `bindy-control-plane.architecture.json` | The operator, its reconcilers, the CRD ownership hierarchy, the BIND9 operand pod (`named` + `bindcar`), the Kubernetes API and the ValidatingAdmissionPolicies. |
| `bindy-multi-cluster.architecture.json` | The k0rdent multi-cluster topology: the Queen Bee (management/DNS) cluster and the child/workload clusters whose Scouts fan `ARecord`s in. |

Each `*.architecture.json` has:

- **`nodes`** — components/services/data-assets (each with a stable `unique-id`).
- **`relationships`** — `connects` / `composed-of` edges between nodes, optionally
  carrying a `protocol` (e.g. `HTTPS`, `TCP`).

## Working with these files

Everything is driven from the repository `Makefile` (Node.js ≥ 20 required; the
[`@finos/calm-cli`](https://www.npmjs.com/package/@finos/calm-cli) is fetched
on demand via `npx`, pinned by `CALM_CLI_VERSION`):

```bash
make calm-validate     # schema-validate every calm/*.architecture.json (CI gate)
make calm-docs         # regenerate the Mermaid pages in docs/src/architecture/
make calm-docs-check   # fail if the committed Mermaid pages are stale
```

### Editing workflow

1. Edit or add a `*.architecture.json` model here.
2. Run `make calm-validate` — it must pass (the Build workflow enforces this on PRs).
3. Run `make calm-docs` to regenerate the diagram pages, and commit both the
   model **and** the regenerated `docs/src/architecture/calm-*.md`.

> The generated pages carry a `DO NOT EDIT` banner. Change the model, not the page.
> The Build workflow runs `make calm-docs-check` on PRs and fails if they drift.

## Why CALM

CALM keeps the architecture description versioned, reviewable and diffable
alongside the code it documents, and lets the diagrams be regenerated
deterministically rather than maintained by hand — matching this project's
compliance requirement that architecture be auditable and traceable.
