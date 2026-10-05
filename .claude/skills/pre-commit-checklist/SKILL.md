---
name: pre-commit-checklist
description: The mandatory gate before EVERY commit. Walks the Rust, CRD, reconciler, and always-on checklists (tests, clippy, docs, changelog, examples, no secrets). A task is NOT complete until every applicable box is green.
---

# pre-commit-checklist

Run before committing any change. Every applicable box must pass.

## If ANY `.rs` file was modified

- [ ] Tests updated/added/deleted to match changes (TDD — see `tdd-workflow`)
- [ ] All new public functions have tests
- [ ] All deleted functions have tests removed
- [ ] `cargo fmt` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes (fix ALL warnings)
- [ ] `cargo test` passes (ALL tests green)
- [ ] Rustdoc comments on all public items, accurate to actual behavior
- [ ] `docs/src/` updated for user-facing changes

## If `crates/bindy-api/src/crd.rs` was modified

- [ ] `cargo run -p bindy-api --features crdgen --bin crdgen` run (`regen-crds` skill)
- [ ] `examples/*.yaml` updated to match new schema
- [ ] `docs/src/` documentation updated
- [ ] `kubectl apply --dry-run=client -f examples/` passes (`validate-examples`)
- [ ] `cargo run -p bindy-api --features crdgen --bin crddoc > docs/src/reference/api.md` run LAST (`regen-api-docs`)

## If a controller crate (`crates/bindy-controller-*/src/`) was modified

- [ ] Reconciliation flow diagrams updated in `docs/src/architecture/`
- [ ] New behaviors documented in user guides
- [ ] Troubleshooting guides updated for new error conditions

## If the change was architecturally significant (ADD)

- [ ] ADR in `docs/adr/` (metadata bullets format)
- [ ] CALM models updated; `make calm-validate` + `make calm-docs` + `make calm-docs-check` clean
- [ ] Roadmap detail doc AND `ROADMAPS.md` updated for anything completed
- [ ] Threat-model pass done, header stamp bumped

## Always

- [ ] `.claude/CHANGELOG.md` updated with **Author:** line (MANDATORY — `update-changelog`)
- [ ] `make docs` succeeds (`build-docs` skill)
- [ ] All YAML examples validate: `kubectl apply --dry-run=client -f examples/`
- [ ] `kubectl apply --dry-run=client -f deploy/operator/crds/` succeeds
- [ ] No secrets, tokens, credentials, internal hostnames, or IP addresses committed
- [ ] No `.unwrap()` in production code
- [ ] `docs-sync-check` skill run — no `❌ MISSING` rows

## Commit shape

Commits only when Erick asks, always `git commit -s -S -m "<message>"` —
authored as Erick, never Claude, no co-author trailers or generated-by footers.

## Verification

Every checked box above passes. A task is NOT complete until the full
checklist is green.
