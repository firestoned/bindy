---
name: verify-crd-sync
description: Verify that the generated CRD YAMLs in deploy/operator/crds/ match the Rust source of truth in src/crd.rs. Use BEFORE investigating any reconciliation loop, infinite requeue, "field not appearing in kubectl output", or status patch that returns HTTP 200 but doesn't persist; and AFTER any edit to structs in src/crd.rs. Catches schema drift that causes silent field pruning.
---

# verify-crd-sync

CRD YAMLs in `deploy/operator/crds/` are **auto-generated** from `src/crd.rs` by
the `crdgen` binary. When they drift, the API server prunes struct fields that
exist in Rust but not in the deployed schema: patches succeed (HTTP 200) but the
data never persists, causing silent reconciliation loops and missing `kubectl`
output.

`src/crd.rs` is the single source of truth. Never hand-edit the YAMLs.

## When to use

- **After** any change to a struct in `src/crd.rs` (new field, rename, serde attr).
- **Before** investigating: reconciliation/requeue loops, a field not appearing in
  `kubectl get -o yaml`, a status patch that returns 200 but doesn't stick, or any
  "the controller ignores my change" report.
- As part of a regression run after merging branches that touched `src/crd.rs`
  (large CRDs like `bind9instances.crd.yaml` exceed patch/merge tooling limits and
  are the ones most likely to be left stale).

## Steps

### 1. Detect drift offline (no cluster required — preferred)

Regenerate from Rust and diff against what's committed on disk:

```bash
cargo run --bin crdgen
git diff --stat -- deploy/operator/crds/
```

- **No files listed** → CRDs are in sync. Done.
- **Files listed** → those YAMLs were stale; `crdgen` has just corrected them.
  Inspect the change to confirm it's the expected field, then keep it:

```bash
git diff -- deploy/operator/crds/<name>.crd.yaml
```

The regenerated files ARE the fix — leave them in the working tree so they get
committed alongside the `src/crd.rs` change.

### 2. Confirm a specific field round-trips (optional, cluster required)

If a field still misbehaves after step 1, verify the deployed cluster schema:

```bash
# Deployed schema for the field
kubectl get crd <plural>.bindy.firestoned.io -o yaml | grep -A 20 "<fieldName>:"

# Rust definition it should match
rg -A 10 "pub struct <StructName>" src/crd.rs
```

### 3. Apply the corrected CRDs to the cluster (only when asked)

`Bind9Instance` CRD exceeds the 256 KB annotation limit, so `kubectl apply`
fails — use `replace --force`:

```bash
kubectl replace --force -f deploy/operator/crds/<name>.crd.yaml
```

Image build/push and cluster mutations are the user's to run — surface the
command, don't execute it unprompted.

## Verification

- `git diff --stat -- deploy/operator/crds/` is empty immediately after
  `cargo run --bin crdgen` (regenerating twice is idempotent).
- The previously-missing field appears in `kubectl get -o yaml` after the patch.
- The reconciliation loop stops (observedGeneration catches up to
  metadata.generation).

## Related

- `regen-crds` — the generation half of this workflow (edit `src/crd.rs` →
  `crdgen` → update `examples/` → `regen-api-docs` LAST).
- CRD API reference docs are regenerated separately:
  `cargo run --bin crddoc > docs/src/reference/api.md`.
