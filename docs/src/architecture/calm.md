# Architecture as Code (CALM)

Bindy's architecture is described as code using the
[FINOS **CALM** (Common Architecture Language Model)](https://calm.finos.org/)
specification. The machine-readable models live in the
[`calm/`](https://github.com/firestoned/bindy/tree/main/calm) directory of the
repository and are the **single source of truth** for the diagrams below — the
Mermaid on these pages is regenerated from the models with `make calm-docs`,
never drawn by hand.

This gives us architecture that is:

- **Versioned & diffable** — changes to the system show up in pull requests like
  any other code change.
- **Validated** — every model is schema-checked (CALM 1.2) in CI, and the
  generated diagrams are drift-checked against the models.
- **Auditable** — a traceable, reviewable description of the system, as required
  in a regulated environment.

## Diagrams

- **[Control Plane](calm-control-plane.md)** — the operator, its reconcilers, the
  CRD ownership hierarchy (ClusterBind9Provider → Bind9Cluster → Bind9Instance →
  operand pod), the `named` + `bindcar` operand pod, the Kubernetes API, and the
  ValidatingAdmissionPolicies.
- **[Multi-Cluster](calm-multi-cluster.md)** — the k0rdent topology: the Queen Bee
  (management/DNS) cluster and the child/workload clusters whose Scouts fan
  `ARecord`s into the Queen Bee `bindy-system` namespace.

## Regenerating

```bash
make calm-validate   # schema-validate the models
make calm-docs       # regenerate the Mermaid pages
make calm-docs-check # verify the committed pages are up to date
```

See [`calm/README.md`](https://github.com/firestoned/bindy/blob/main/calm/README.md)
for the full editing workflow.
