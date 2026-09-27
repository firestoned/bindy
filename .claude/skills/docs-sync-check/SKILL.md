---
name: docs-sync-check
description: Drift gate proving nothing user-facing shipped undocumented on this branch. Use after implementing a feature or changing CRDs, config/env vars, RBAC, admission policies, defaults, or validation — and as the final gate before pre-commit-checklist. Flags breaking changes needing migration-guide entries.
---

# docs-sync-check

Where `update-docs` is the *procedure to update* docs, this skill is the *gate
that detects drift* — run it to prove nothing user-facing shipped undocumented.

## Steps

### 1. Scope the change

```bash
BASE=$(git merge-base HEAD main)
git diff --stat "$BASE"..HEAD                 # committed changes
git status --porcelain && git diff --stat     # uncommitted working tree
```

### 2. Classify each changed path → the docs it obligates

| Changed path / signal | Required documentation |
|---|---|
| `src/crd.rs` (fields/structs) | `regen-api-docs` (`docs/src/reference/api.md`), `examples/*.yaml`, config reference in `docs/src/` |
| New `BINDY_*` / other env var | `docs/src/` config/deployment reference + `deploy/operator/deployment.yaml` if applicable |
| `src/reconcilers/` behavior | `docs/src/architecture/` flow diagrams, user guide, troubleshooting |
| New CRD | `add-new-crd` skill (guide + api docs + examples) |
| `deploy/admission-policies/NN-*.yaml` (new) | `deploy/admission-policies/README.md` table + `make admission-policies-yaml` |
| `deploy/**/rbac/`, `src/bootstrap.rs` | `deploy/operator/rbac/README.md`, RBAC guide, keep bootstrap↔YAML in sync (see `.claude/CLAUDE.md`) |
| `src/scout.rs` behavior | `docs/src/guide/scout.md`, `docs/src/installation/scout.md` |
| New public module / feature | `docs/src/features/` or relevant guide + `README.md` features section |

### 3. Flag BREAKING changes explicitly

Breaking: removed/renamed CRD field; new required field; changed default;
validation now rejecting previously-valid input; new default-deny gate;
removed/renamed env var or flag; RBAC scope reduction; changed
API/route/annotation contract.

```bash
# Deletions/renames in CRD types and public signatures
git diff "$BASE"..HEAD -- src/crd.rs | rg '^-\s*pub '
# New rejections / default-deny / required prefixes introduced
git diff "$BASE"..HEAD -- src/ | rg -i '^\+.*(reject|deny|must (start|be)|required|Err\()'
```

Every breaking change requires ALL of:

- `.claude/CHANGELOG.md`: `- [x] Breaking change` checked (`update-changelog`).
- A migration note in `docs/src/operations/migration-guide.md` (what breaks,
  how to detect, the exact remediation).
- Updated `examples/*.yaml` so shipped examples still validate under the new rule.

### 4. Confirm NEW FEATURES are documented

Each new feature/flag/policy: a user-facing doc page or guide section, a
`README.md` mention if it changes getting-started/features, at least one
example, and a CHANGELOG entry.

### 5. Greppable-sync checks

```bash
# Every new BINDY_* env var must be documented somewhere under docs/
for v in $(git diff "$BASE"..HEAD -- src/ | rg -o 'BINDY_[A-Z_]+' | sort -u); do
  rg -q "$v" docs/ .claude/CHANGELOG.md || echo "UNDOCUMENTED env var: $v"
done
# Every new admission policy must be in the README table
for p in $(git diff --name-only --diff-filter=A "$BASE"..HEAD -- 'deploy/admission-policies/[0-9][0-9]-*.yaml'); do
  name=$(rg -m1 '^\s*name:\s*(\S+)' "$p" -or '$1'); rg -q "$name" deploy/admission-policies/README.md || echo "UNDOCUMENTED policy: $p"
done
```

### 6. Build

Run the `build-docs` skill (`make docs`). Must exit 0 with no *new*
`WARNING - ... not found` broken-link lines versus the pre-change baseline.

### 7. Emit a drift report

One line per changed path: `✅ doc updated`, `⚠️ breaking — migration note
added`, or `❌ MISSING <what>`. Do not report the task complete while any `❌`
remains; a genuine "no doc needed" gets a one-line justification.

## Verification

- [ ] Every changed path maps to an updated doc, or a stated "no doc needed" reason.
- [ ] Every breaking change has: CHANGELOG breaking flag + `migration-guide.md` entry + validated examples.
- [ ] Every new `BINDY_*` env var, admission policy, and CRD field is greppable in `docs/`.
- [ ] `regen-api-docs` was run if `src/crd.rs` changed.
- [ ] `make docs` exits 0 with no new broken-link warnings.
