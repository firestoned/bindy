---
name: regen-crds
description: Regenerate the CRD YAMLs in deploy/operator/crds/ from the Rust source of truth (crates/bindy-api/src/crd.rs) via the crdgen binary. Use after ANY edit to types in crates/bindy-api/src/crd.rs and before deploying CRD changes. Never hand-edit the generated YAMLs.
---

# regen-crds

CRD YAMLs in `deploy/operator/crds/` are auto-generated from `crates/bindy-api/src/crd.rs` by
the `crdgen` binary. `crates/bindy-api/src/crd.rs` is the single source of truth; never edit
the YAMLs directly.

## Steps

```bash
# 1. Regenerate all CRD YAML files from Rust types
cargo run -p bindy-api --features crdgen --bin crdgen

# 2. Verify generated YAMLs
for file in deploy/operator/crds/*.crd.yaml; do
  echo "Checking $file"
  kubectl apply --dry-run=client -f "$file"
done
```

3. Update `examples/` to match the new schema, then run the
   `validate-examples` skill.
4. Deploying is the user's call — surface the command, don't run it:

```bash
# Bind9Instance CRD exceeds the 256KB annotation limit — replace, not apply
kubectl replace --force -f deploy/operator/crds/
# Or for first install:
kubectl create -f deploy/operator/crds/
```

## Follow-ups (in order)

1. `validate-examples` skill
2. Update `docs/src/` for user-facing schema changes
3. `regen-api-docs` skill — always LAST

## Verification

- `kubectl apply --dry-run=client -f deploy/operator/crds/` succeeds for all files.
- Re-running `cargo run -p bindy-api --features crdgen --bin crdgen` produces no further diff (idempotent).

## Related

- `verify-crd-sync` — the drift-detection half of this workflow.
- `add-new-crd` — full procedure for introducing a new CRD.
