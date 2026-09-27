# Claude Skills Reference

All procedural skills are **registered, invocable skills** — one directory per
skill at `.claude/skills/<name>/SKILL.md` (YAML frontmatter `name` +
`description`, then the steps). Invoke by name via the Skill tool. This file
is only the index; the skill files are canonical.

| Skill | Use it… |
|---|---|
| `cargo-quality` | after ANY `.rs` change: fmt + clippy `-D warnings` + test (NON-NEGOTIABLE) |
| `tdd-workflow` | before writing code: RED → GREEN → REFACTOR, tests in `_tests.rs` files |
| `verify-crd-sync` | before debugging reconcile loops / non-persisting fields; after `src/crd.rs` edits |
| `regen-crds` | after editing `src/crd.rs` — regenerate `deploy/operator/crds/` |
| `validate-examples` | after schema changes / before committing `examples/` |
| `regen-api-docs` | LAST step of any CRD change — regenerate `docs/src/reference/api.md` |
| `add-new-crd` | adding a new CRD (full ~20-file checklist) |
| `build-docs` | build/verify docs via `make docs` (never `mkdocs build` directly) |
| `update-docs` | the procedure to update CHANGELOG → docs → examples → diagrams |
| `docs-sync-check` | the drift gate: prove nothing user-facing shipped undocumented |
| `update-changelog` | after ANY change — `.claude/CHANGELOG.md` entry with `**Author:**` |
| `get-multiarch-digest` | pinning Docker base images (manifest-list digest, never platform-specific) |
| `upgrade-bindcar` | bumping the bindcar sidecar version everywhere |
| `pre-commit-checklist` | mandatory gate before EVERY commit |

New procedures go in a new `.claude/skills/<name>/SKILL.md`, plus a row here.
