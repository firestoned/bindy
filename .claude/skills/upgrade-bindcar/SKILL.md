---
name: upgrade-bindcar
description: Upgrade the bindcar sidecar dependency to a new version — Cargo bump plus every image-tag reference across src/, examples/, CRD YAMLs, tests, and docs, then API-compat check against the bindcar sources. Use when asked to upgrade bindcar (e.g. "upgrade to bindcar v0.8.0").
---

# upgrade-bindcar

Given `NEW_VERSION` (e.g. `0.8.0`) and `NEW_TAG` (e.g. `v0.8.0`):

## 1. Cargo bump

```bash
# In Cargo.toml: bindcar = "<NEW_VERSION>"
sed -i '' 's/^bindcar = ".*"/bindcar = "<NEW_VERSION>"/' Cargo.toml
cargo update bindcar
```

## 2. Update ALL image-tag references

| File | What to change |
|------|----------------|
| `Cargo.toml` | `bindcar = "<NEW_VERSION>"` |
| `crates/bindy-api/src/constants.rs` | `DEFAULT_BINDCAR_IMAGE` → `ghcr.io/firestoned/bindcar:<NEW_TAG>` |
| `crates/bindy-api/src/crd.rs` | rustdoc example `/// Example: "ghcr.io/firestoned/bindcar:<NEW_TAG>"` |
| `crates/bindy-bootstrap/src/bootstrap.rs` | Any hardcoded image references (check with rg) |
| `examples/*.yaml` | All `image: "ghcr.io/firestoned/bindcar:*"` lines |
| `deploy/operator/crds/*.crd.yaml` | Regenerate via `regen-crds` (rustdoc example flows through) |
| `tests/integration_test.sh` | All `image: "ghcr.io/firestoned/bindcar:*"` lines |
| `docs/src/**/*.md` | Any `ghcr.io/firestoned/bindcar:v*` references (skip placeholder examples using other registries) |

```bash
# Verify no old version strings remain
rg 'firestoned/bindcar:v' . --glob '!target/' --glob '!.claude/CHANGELOG.md'
```

## 3. Check for API breaking changes

- Read `/Users/erick/dev/bindcar/src/lib.rs` and compare exported types
  against what bindy imports.
- If types/fields were removed or renamed, update all usages in `src/`.
- Track migration in the corresponding `.github/community/` bindcar-migration
  roadmap doc if one exists for this version.

## 4. Quality + audit trail

- Run the `cargo-quality` skill (compile + clippy + tests must all pass).
- Run the `update-changelog` skill.

## Verification

`rg 'firestoned/bindcar:v' . --glob '!target/' --glob '!.claude/CHANGELOG.md'`
shows only the new tag. `cargo test` passes.
