# Claude Skills Reference

Reusable procedural skills extracted from CLAUDE.md. Each skill has a canonical name (kebab-case), trigger conditions, ordered steps, and a verification check. Invoke a skill by name: *"run the cargo-quality skill"* or *"do a verify-crd-sync"*.

---

## `verify-crd-sync`

**When to use:**
- Before investigating reconciliation loops or infinite loops
- Before debugging "field not appearing in kubectl output" issues
- After ANY modification to structs in `src/crd.rs`
- When status patches succeed but data doesn't persist
- When user reports unexpected controller behavior

**Steps:**
```bash
# 1. Check deployed CRD schema in cluster
kubectl get crd <crd-name>.bindy.firestoned.io -o yaml | grep -A 20 "<field-name>:"

# 2. Check Rust struct definition
rg -A 10 "pub struct <StructName>" src/crd.rs

# 3. If mismatch detected, regenerate CRDs
cargo run --bin crdgen

# 4. Apply updated CRDs (use replace --force to avoid annotation size limits)
kubectl replace --force -f deploy/operator/crds/<crd-name>.crd.yaml
```

**Verification:** Field appears in `kubectl get` output after patch; no infinite reconciliation loop.

---

## `regen-crds`

**When to use:**
- After ANY edit to Rust types in `src/crd.rs`
- Before deploying CRD changes to a cluster

**Steps:**
```bash
# 1. Regenerate all CRD YAML files from Rust types
cargo run --bin crdgen

# 2. Verify generated YAMLs
for file in deploy/operator/crds/*.crd.yaml; do
  echo "Checking $file"
  kubectl apply --dry-run=client -f "$file"
done

# 3. Update examples to match new schema (see validate-examples skill)

# 4. Deploy
kubectl replace --force -f deploy/operator/crds/
# Or for first install:
kubectl create -f deploy/operator/crds/
```

**Verification:** `kubectl apply --dry-run=client -f deploy/operator/crds/` succeeds for all files.

---

## `regen-api-docs`

**When to use:**
- After all CRD changes, example updates, and validations are complete (run this LAST)
- Before any documentation release

**Steps:**
```bash
# Regenerate API reference from CRD types
cargo run --bin crddoc > docs/src/reference/api.md
```

**Verification:** `docs/src/reference/api.md` reflects the current CRD schema. Run `make docs` to confirm the full docs build succeeds.

---

## `cargo-quality`

**When to use:**
- After adding or modifying ANY `.rs` file
- Before committing any Rust code changes
- At the end of EVERY task involving Rust code (NON-NEGOTIABLE)

**Steps:**
```bash
# 0. Ensure cargo is in PATH
source ~/.zshrc

# 1. Format
cargo fmt

# 2. Lint with strict warnings (fix ALL warnings)
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -A clippy::module_name_repetitions

# 3. Test (ALL tests must pass)
cargo test

# 4. Security audit (optional, if installed)
cargo audit 2>/dev/null || true
```

**Verification:** All three commands exit with code 0. No warnings, no test failures.

---

## `tdd-workflow`

**When to use:**
- Adding any new feature or function
- Fixing a bug
- Refactoring existing code

**Steps:**

**RED — Write failing tests first (before any implementation):**
```bash
# Edit src/<module>_tests.rs — add test(s) that define expected behavior
cargo test <test_name>   # Must FAIL at this point
```

**GREEN — Implement minimum code to pass tests:**
```bash
# Edit src/<module>.rs — write simplest code that makes tests pass
cargo test <test_name>   # Must PASS now
```

**REFACTOR — Improve while keeping tests green:**
```bash
# Extract constants, add docs, improve error handling
cargo test               # Must still PASS
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -A clippy::module_name_repetitions
```

**Test file pattern:**
- Source: `src/foo.rs` → declare `#[cfg(test)] mod foo_tests;` at the bottom
- Tests: `src/foo_tests.rs` → wrap in `#[cfg(test)] mod tests { use super::super::*; ... }`

**Verification:** All tests pass, clippy is clean, test covers success path + error paths + edge cases.

---

## `update-changelog`

**When to use:**
- After ANY code modification (mandatory for auditing in a regulated environment)

**Steps:**

Open `.claude/CHANGELOG.md` and prepend an entry in this exact format:

```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

**Author:** <Name of requester or approver>

### Changed
- `path/to/file.rs`: Description of the change

### Why
Brief explanation of the business or technical reason.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only
```

**Verification:** Entry has `**Author:**` line (MANDATORY — no exceptions), timestamp, and at least one `### Changed` item.

---

## `update-docs`

**When to use:**
- After any code change in `src/`
- After CRD changes, API changes, configuration changes, or new features

**Steps:**
1. Identify what changed (feature, CRD field, behavior, error condition).
2. Update `.claude/CHANGELOG.md` (see `update-changelog` skill).
3. Update affected pages in `docs/src/`:
   - User guides, quickstart guides, configuration references, troubleshooting guides
4. Update `examples/*.yaml` to reflect schema or behavior changes.
5. Update architecture diagrams if structure changed (Mermaid in `docs/src/architecture/`).
6. If CRDs changed: run `regen-api-docs` skill (LAST step).
7. If README getting-started or features changed: update `README.md`.
8. Run `build-docs` skill to confirm no broken references.

**Verification checklist:**
- [ ] `.claude/CHANGELOG.md` updated with author
- [ ] All affected `docs/src/` pages updated
- [ ] All YAML examples validate: `kubectl apply --dry-run=client -f examples/`
- [ ] API docs regenerated if CRDs changed
- [ ] Architecture diagrams match current implementation
- [ ] `make docs` succeeds

---

## `build-docs`

**When to use:**
- After any documentation change
- Before any documentation release
- To verify docs are not broken

**Steps:**
```bash
# Build all documentation components in the correct order
make docs
```

What `make docs` does:
1. Installs docs dependencies via Poetry: `cd docs && poetry install`
2. Generates CRD API reference: `cargo run --bin crddoc > docs/src/reference/api.md`
3. Builds rustdoc: `cargo doc --no-deps --all-features`
4. Builds the MkDocs site: `cd docs && poetry run mkdocs build`
5. Copies rustdoc into `docs/site/rustdoc/` and creates an index redirect

**Verification:** `make docs` exits 0 with no errors. Output site is viewable at `docs/site/index.html`.

---

## `docs-sync-check`

Verify that documentation is in sync with the code changes on the current branch,
with special attention to **new features** and **breaking changes**. Where
`update-docs` is the *procedure to update* docs, this skill is the *gate that
detects drift* — run it to prove nothing user-facing shipped undocumented.

**When to use:**
- After implementing a new feature, or any change to CRDs, config/env vars,
  RBAC, admission policies, defaults, or validation
- Whenever the latest change alters user-facing behavior (a new flag, a new
  default-deny, a renamed field, a stricter validation)
- As a final gate before `pre-commit-checklist` / before telling the user a
  task is complete

**Steps:**

1. **Scope the change.** Enumerate exactly what changed on this branch:
   ```bash
   BASE=$(git merge-base HEAD main)
   git diff --stat "$BASE"..HEAD                 # committed changes
   git status --porcelain && git diff --stat     # uncommitted working tree
   ```

2. **Classify each changed path → the docs it obligates.** Any row that matches
   MUST have a corresponding doc update in the same change:

   | Changed path / signal | Required documentation |
   |---|---|
   | `src/crd.rs` (fields/structs) | `regen-api-docs` (`docs/src/reference/api.md`), `examples/*.yaml`, config reference in `docs/src/` |
   | New `BINDY_*` / other env var | `docs/src/` config/deployment reference + `deploy/operator/deployment.yaml` if applicable |
   | `src/reconcilers/` behavior | `docs/src/architecture/` flow diagrams, user guide, troubleshooting |
   | New CRD | `add-new-crd` skill (guide + api docs + examples) |
   | `deploy/admission-policies/NN-*.yaml` (new) | `deploy/admission-policies/README.md` table + regenerate combined file: `make admission-policies-yaml` |
   | `deploy/**/rbac/`, `src/bootstrap.rs` | `deploy/operator/rbac/README.md`, RBAC guide, keep bootstrap↔YAML in sync (see `.claude/CLAUDE.md`) |
   | `src/scout.rs` behavior | `docs/src/guide/scout.md`, `docs/src/installation/scout.md` |
   | New public module / feature | `docs/src/features/` or relevant guide + `README.md` features section |

3. **Flag BREAKING changes explicitly.** Treat as breaking (each needs the full
   breaking-change treatment below): a removed/renamed CRD field; a new required
   field; a changed default; validation that now rejects previously-valid input
   (e.g. a new name-prefix requirement); a new default-deny gate; a removed/renamed
   env var or flag; an RBAC scope reduction; a changed API/route/annotation contract.
   Quick signals:
   ```bash
   # Deletions/renames in CRD types and public signatures
   git diff "$BASE"..HEAD -- src/crd.rs | rg '^-\s*pub '
   # New rejections / default-deny / required prefixes introduced
   git diff "$BASE"..HEAD -- src/ | rg -i '^\+.*(reject|deny|must (start|be)|required|Err\()'
   ```
   For **every** breaking change require ALL of:
   - `.claude/CHANGELOG.md`: `- [x] Breaking change` checked (see `update-changelog`).
   - A **migration note** for users in `docs/src/operations/migration-guide.md`
     (what breaks, how to detect, the exact remediation — e.g. annotate the
     resource, rename the secret).
   - Updated `examples/*.yaml` so shipped examples still validate under the new rule.

4. **Confirm NEW FEATURES are documented.** For each new feature/flag/policy:
   a user-facing doc page or guide section, a `README.md` mention if it changes
   getting-started/features, at least one `example`, and a CHANGELOG entry.

5. **Greppable-sync checks** (each new symbol must appear in docs):
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

6. **Build.** Run the `build-docs` skill (`make docs`). It MUST exit 0 and MUST
   NOT introduce any *new* `WARNING - ... not found` broken-link lines versus the
   pre-change baseline.

7. **Emit a drift report.** One line per changed path: `✅ doc updated`,
   `⚠️ breaking — migration note added`, or `❌ MISSING <what>`. Do not report the
   task complete while any `❌` remains; if a change genuinely needs no docs, say
   so explicitly with a one-line justification.

**Verification:**
- [ ] Every changed path maps to an updated doc, or a stated "no doc needed" reason.
- [ ] Every breaking change has: CHANGELOG breaking flag + a `migration-guide.md`
      entry + validated examples.
- [ ] Every new `BINDY_*` env var, admission policy, and CRD field is greppable in `docs/`.
- [ ] `regen-api-docs` was run if `src/crd.rs` changed.
- [ ] `make docs` exits 0 with no new broken-link warnings.

---

## `get-multiarch-digest`

**When to use:**
- Before pinning a Docker base image digest in any Dockerfile
- When updating base image versions

**Steps:**
```bash
# Get the multi-arch manifest list digest (NOT platform-specific)
docker buildx imagetools inspect <image>:<tag> --raw | sha256sum | awk '{print "sha256:"$1}'

# Examples:
docker buildx imagetools inspect debian:13-slim --raw | sha256sum | awk '{print "sha256:"$1}'
docker buildx imagetools inspect rust:1.94.0 --raw | sha256sum | awk '{print "sha256:"$1}'
docker buildx imagetools inspect gcr.io/distroless/cc-debian13:nonroot --raw | sha256sum | awk '{print "sha256:"$1}'
```

Use the digest in Dockerfiles as:
```dockerfile
# NOTE: This digest points to the multi-arch manifest list (supports both AMD64 and ARM64)
FROM debian:13-slim@sha256:<digest> AS builder
```

Update ALL Dockerfiles that use the same base image:
- `docker/Dockerfile`
- `docker/Dockerfile.chainguard`
- `docker/Dockerfile.chef`
- `docker/Dockerfile.fast`
- `docker/Dockerfile.local` (usually no digest)

**Verification:**
```bash
docker buildx imagetools inspect <image>@<digest>
# Output must show BOTH: Platform: linux/amd64 AND Platform: linux/arm64
```

---

## `validate-examples`

**When to use:**
- After any CRD schema change
- Before committing changes to `examples/`
- As part of the `pre-commit-checklist`

**Steps:**
```bash
# Validate all example YAML files
kubectl apply --dry-run=client -f examples/

# Or validate individually
for file in examples/*.yaml; do
  echo "Validating $file"
  kubectl apply --dry-run=client -f "$file"
done
```

**Verification:** All files pass dry-run with no errors. No `unknown field` or `required field missing` errors.

---

## `add-new-crd`

**When to use:**
- When adding a new Custom Resource Definition to the operator

**Steps:**
1. Add the new `CustomResource` struct to `src/crd.rs`:
   ```rust
   #[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
   #[kube(
       group = "bindy.firestoned.io",
       version = "v1beta1",
       kind = "MyNewResource",
       namespaced
   )]
   #[serde(rename_all = "camelCase")]
   pub struct MyNewResourceSpec {
       pub field_name: String,
   }
   ```
2. Register it in `src/bin/crdgen.rs`:
   ```rust
   generate_crd::<MyNewResource>("mynewresources.crd.yaml", output_dir)?;
   ```
3. Run `regen-crds` skill.
4. Add examples to `examples/`.
5. Run `validate-examples` skill.
6. Add documentation in `docs/src/`.
7. Run `regen-api-docs` skill (LAST).
8. Run `cargo-quality` skill.
9. Run `update-changelog` skill.

**Verification:** `kubectl apply --dry-run=client -f deploy/operator/crds/mynewresources.crd.yaml` succeeds; API docs include the new resource.

---

## `pre-commit-checklist`

**When to use:**
- Before committing any change (mandatory gate)

**Checklist:**

### If ANY `.rs` file was modified:
- [ ] Tests updated/added/deleted to match changes (TDD — see `tdd-workflow`)
- [ ] All new public functions have tests
- [ ] All deleted functions have tests removed
- [ ] `cargo fmt` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes (fix ALL warnings)
- [ ] `cargo test` passes (ALL tests green)
- [ ] Rustdoc comments on all public items, accurate to actual behavior
- [ ] `docs/src/` updated for user-facing changes

### If `src/crd.rs` was modified:
- [ ] `cargo run --bin crdgen` run
- [ ] `examples/*.yaml` updated to match new schema
- [ ] `docs/src/` documentation updated
- [ ] `kubectl apply --dry-run=client -f examples/` passes
- [ ] `cargo run --bin crddoc > docs/src/reference/api.md` run (LAST)

### If `src/reconcilers/` was modified:
- [ ] Reconciliation flow diagrams updated in `docs/src/architecture/`
- [ ] New behaviors documented in user guides
- [ ] Troubleshooting guides updated for new error conditions

### Always:
- [ ] `.claude/CHANGELOG.md` updated with **Author:** line (MANDATORY)
- [ ] `make docs` succeeds
- [ ] All YAML examples validate: `kubectl apply --dry-run=client -f examples/`
- [ ] `kubectl apply --dry-run=client -f deploy/operator/crds/` succeeds
- [ ] No secrets, tokens, credentials, internal hostnames, or IP addresses committed
- [ ] No `.unwrap()` in production code

**Verification:** Every checked box above passes. A task is NOT complete until the full checklist is green.

---

## `upgrade-bindcar`

**When to use:**
- When the user asks to upgrade bindcar to a new version (e.g., "upgrade to bindcar v0.7.0")

**Steps:**

Given `NEW_VERSION` (e.g., `0.7.0`) and `NEW_TAG` (e.g., `v0.7.0`):

```bash
# 1. Update Cargo dependency
# In Cargo.toml: bindcar = "<NEW_VERSION>"
sed -i '' 's/^bindcar = ".*"/bindcar = "<NEW_VERSION>"/' Cargo.toml

# 2. Resolve the new version in Cargo.lock
cargo update bindcar
```

Then update ALL of the following files (use rg to verify nothing is missed):

| File | What to change |
|------|----------------|
| `Cargo.toml` | `bindcar = "<NEW_VERSION>"` |
| `src/constants.rs` | `DEFAULT_BINDCAR_IMAGE` → `ghcr.io/firestoned/bindcar:<NEW_TAG>` |
| `src/crd.rs` | rustdoc example `/// Example: "ghcr.io/firestoned/bindcar:<NEW_TAG>"` |
| `src/bootstrap.rs` | Any hardcoded image references (check with rg) |
| `examples/*.yaml` | All `image: "ghcr.io/firestoned/bindcar:*"` lines |
| `deploy/operator/crds/*.crd.yaml` | All `Example: "ghcr.io/firestoned/bindcar:*"` lines |
| `tests/integration_test.sh` | All `image: "ghcr.io/firestoned/bindcar:*"` lines |
| `docs/src/**/*.md` | Any `ghcr.io/firestoned/bindcar:v*` references (skip placeholder examples using other registries) |

```bash
# Verify no old version strings remain (replace OLD with the prior version tag)
rg 'firestoned/bindcar:v' . --glob '!target/' --glob '!.claude/CHANGELOG.md'
```

3. Check for API breaking changes between old and new bindcar versions:
   - Read `/Users/erick/dev/bindcar/src/lib.rs` and compare exported types against what bindy imports
   - If types/fields were removed or renamed, update all usages in `src/`

4. Run `cargo-quality` skill (compile + clippy + tests must all pass).

5. Run `update-changelog` skill.

**Verification:** `rg 'firestoned/bindcar:v' . --glob '!target/' --glob '!.claude/CHANGELOG.md'` shows only the new tag. `cargo test` passes.
