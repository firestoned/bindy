---
name: update-docs
description: The documentation update procedure for any code, CRD, API, configuration, or feature change. Walks CHANGELOG → docs/src/ → examples/ → diagrams → API docs → README in the required order. Use before marking any task complete.
---

# update-docs

Documentation is a critical requirement equal in importance to the code
(see `rules/documentation.md`). Where `docs-sync-check` *detects* drift, this
skill is the *procedure to update*.

## Steps (in order)

1. Identify what changed (feature, CRD field, behavior, error condition).
2. Update `.claude/CHANGELOG.md` — `update-changelog` skill.
3. Update affected pages in `docs/src/`: user guides, quickstart,
   configuration references, troubleshooting.
4. Update `examples/*.yaml` to reflect schema or behavior changes — verify
   field names against `src/crd.rs` or `deploy/operator/crds/*.crd.yaml`,
   never guess.
5. Update architecture diagrams if structure changed (Mermaid in
   `docs/src/architecture/`; CALM-generated pages via `make calm-docs`).
6. If CRDs changed: run the `regen-api-docs` skill (LAST).
7. If getting-started or features changed: update `README.md`.
8. Run the `build-docs` skill to confirm no broken references.

## By change type

- **Reconcilers** (`src/reconcilers/`): flow diagrams, user guides,
  troubleshooting.
- **CRDs** (`src/crd.rs`): `regen-crds` → examples → `regen-api-docs` (LAST).
- **New features**: `docs/src/features/`, `README.md`, examples,
  troubleshooting.
- **Bug fixes**: troubleshooting guides.

## Verification checklist

- [ ] `.claude/CHANGELOG.md` updated with author
- [ ] All affected `docs/src/` pages updated
- [ ] All YAML examples validate: `kubectl apply --dry-run=client -f examples/`
- [ ] API docs regenerated if CRDs changed
- [ ] Architecture diagrams match current implementation
- [ ] `make docs` succeeds
