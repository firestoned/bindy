---
name: regen-api-docs
description: Regenerate the CRD API reference (docs/src/reference/api.md) from Rust types via the crddoc binary. Run LAST — after all CRD changes, example updates, and validations are complete — and before any documentation release.
---

# regen-api-docs

The API reference at `docs/src/reference/api.md` is generated from the CRD
types in `crates/bindy-api/src/crd.rs` by the `crddoc` binary. Regenerate it as the LAST step
of any CRD change, after `regen-crds`, example updates, and validation.

## Steps

```bash
cargo run -p bindy-api --features crdgen --bin crddoc > docs/src/reference/api.md
```

## Verification

- `docs/src/reference/api.md` reflects the current CRD schema (spot-check the
  changed field).
- Run the `build-docs` skill (`make docs`) to confirm the full docs build
  succeeds.
