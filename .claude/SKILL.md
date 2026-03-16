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
kubectl replace --force -f deploy/crds/<crd-name>.crd.yaml
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
for file in deploy/crds/*.crd.yaml; do
  echo "Checking $file"
  kubectl apply --dry-run=client -f "$file"
done

# 3. Update examples to match new schema (see validate-examples skill)

# 4. Deploy
kubectl replace --force -f deploy/crds/
# Or for first install:
kubectl create -f deploy/crds/
```

**Verification:** `kubectl apply --dry-run=client -f deploy/crds/` succeeds for all files.

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
1. Generates CRD API reference: `cargo run --bin crddoc > docs/src/reference/api.md`
2. Builds rustdoc: `cargo doc --no-deps --all-features`
3. Installs mermaid assets and builds mdBook: `cd docs && mdbook-mermaid install && mdbook build`
4. Copies rustdoc into output and creates index redirects

**Verification:** `make docs` exits 0 with no errors. Output site is viewable at `docs/book/`.

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
docker buildx imagetools inspect debian:12-slim --raw | sha256sum | awk '{print "sha256:"$1}'
docker buildx imagetools inspect rust:1.94.0 --raw | sha256sum | awk '{print "sha256:"$1}'
docker buildx imagetools inspect gcr.io/distroless/cc-debian12:nonroot --raw | sha256sum | awk '{print "sha256:"$1}'
```

Use the digest in Dockerfiles as:
```dockerfile
# NOTE: This digest points to the multi-arch manifest list (supports both AMD64 and ARM64)
FROM debian:12-slim@sha256:<digest> AS builder
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

**Verification:** `kubectl apply --dry-run=client -f deploy/crds/mynewresources.crd.yaml` succeeds; API docs include the new resource.

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
- [ ] `kubectl apply --dry-run=client -f deploy/crds/` succeeds
- [ ] No secrets, tokens, credentials, internal hostnames, or IP addresses committed
- [ ] No `.unwrap()` in production code

**Verification:** Every checked box above passes. A task is NOT complete until the full checklist is green.
