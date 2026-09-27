---
name: validate-examples
description: Validate all example YAML manifests in examples/ against the current CRD schemas with kubectl dry-run. Use after any CRD schema change, before committing changes to examples/, and as part of pre-commit-checklist.
---

# validate-examples

Shipped examples must always validate against the current CRD schemas. Field
names come from `src/crd.rs` / `deploy/operator/crds/*.crd.yaml` — never
guessed.

## Steps

```bash
# Validate all example YAML files
kubectl apply --dry-run=client -f examples/

# Or validate individually to isolate a failure
for file in examples/*.yaml; do
  echo "Validating $file"
  kubectl apply --dry-run=client -f "$file"
done
```

If the schema just changed, run the `regen-crds` skill first so the dry-run
validates against the regenerated CRDs (server-side dry-run additionally
requires the CRDs installed in the cluster).

## Verification

All files pass dry-run with no errors — no `unknown field` and no
`required field missing`.
