# Claude Code Skills

Step-by-step procedures for common tasks. Referenced from `CLAUDE.md` as `@.claude/SKILL.md`.

---

## Skill: `cargo-quality`

Run after **every** Rust code change. Task is NOT complete until all three pass.

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

- Fix ALL clippy warnings — do not suppress them
- Fix ALL test failures — do not skip tests
- If `cargo fmt` changes files, those changes must be committed

---

## Skill: `tdd-workflow`

**RED → GREEN → REFACTOR** — always in this order.

### Step 1: RED — Write a failing test

1. Identify the function/behavior to implement
2. Create or open `src/<module>_tests.rs`
3. Write a test that defines the expected behavior:

```rust
// src/reconcilers/dnszone_tests.rs
#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn test_<descriptive_name>() {
        // Arrange
        let input = ...;

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected);
    }
}
```

4. In the source file, declare the test module at the bottom:

```rust
// src/reconcilers/dnszone.rs
#[cfg(test)]
mod dnszone_tests;
```

5. Run the test — it MUST fail: `cargo test test_<name>`

### Step 2: GREEN — Write minimal implementation

Write only enough code to make the test pass. No extras.

Run: `cargo test test_<name>` — it MUST pass.

### Step 3: REFACTOR

Clean up the implementation without changing behavior. Tests must still pass after refactoring.

Run: `cargo test` — ALL tests must pass.

**Then run the `cargo-quality` skill.**

---

## Skill: `verify-crd-sync`

Run before investigating ANY Kubernetes behavior issue.

```bash
# 1. Check the Rust struct definition (source of truth)
rg -A 20 "pub struct Bind9InstanceSpec" src/crd.rs

# 2. Check the deployed CRD schema in the cluster
kubectl get crd bind9instances.bindy.firestoned.io -o yaml | grep -A 20 "spec:"

# 3. Check the generated YAML in deploy/crds/
grep -A 10 "<field-name>" deploy/crds/*.crd.yaml
```

If there is a mismatch, run the `regen-crds` skill, then `kubectl replace --force`.

**REMEMBER:** `kubectl apply` will fail for large CRDs (>256KB annotation limit). Always use `kubectl replace --force`.

---

## Skill: `regen-crds`

Regenerate CRD YAML files after any change to `src/crd.rs`.

```bash
cargo run --bin crdgen
```

Verify generated files in `deploy/crds/`. Then apply to the cluster:

```bash
kubectl replace --force -f deploy/crds/
```

**Do NOT run `regen-api-docs` yet** — wait until all CRD changes, example updates, and validations are complete.

---

## Skill: `regen-api-docs`

Run **LAST**, after all CRD changes and example validations are complete.

```bash
cargo run --bin crddoc > docs/src/reference/api.md
```

Then run the `build-docs` skill to verify the documentation builds.

---

## Skill: `validate-examples`

Validate all YAML examples against the cluster (dry-run).

```bash
kubectl apply --dry-run=client -f examples/
```

Fix any validation errors before proceeding. If CRDs changed, verify each example field name against `src/crd.rs` or `deploy/crds/*.crd.yaml` first.

---

## Skill: `build-docs`

Build the documentation. Always use the Makefile target — never `mdbook build` directly.

```bash
make docs
```

Fix any errors (broken links, invalid includes, missing files) before considering the task complete.

---

## Skill: `get-multiarch-digest`

Get the multi-arch manifest list digest for a Docker base image. Use this whenever updating a `FROM` line in a Dockerfile.

```bash
# ✅ CORRECT — multi-arch manifest list digest
docker buildx imagetools inspect <image>:<tag> --raw | sha256sum

# Example:
docker buildx imagetools inspect debian:12-slim --raw | sha256sum
# → outputs: abc123...  -
# Use as: FROM debian:12-slim@sha256:abc123...
```

**Never use `docker inspect` or `docker pull` to get the digest** — those return the platform-specific (AMD64-only) digest, which breaks ARM64 builds with QEMU emulation errors.

Update ALL Dockerfiles that use the same base image:
- `docker/Dockerfile`
- `docker/Dockerfile.chainguard`
- `docker/Dockerfile.chef`
- `docker/Dockerfile.fast`
- `docker/Dockerfile.local` (usually no digest needed here)

---

## Skill: `add-new-crd`

Steps to add a new Custom Resource Definition.

### Step 1: Define the Rust types in `src/crd.rs`

```rust
/// Spec for MyNewResource
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    kind = "MyNewResource",
    group = "bindy.firestoned.io",
    version = "v1beta1",
    namespaced,
    status = "MyNewResourceStatus",
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct MyNewResourceSpec {
    // fields...
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MyNewResourceStatus {
    pub conditions: Option<Vec<Condition>>,
    pub observed_generation: Option<i64>,
}
```

### Step 2: Write tests first (TDD)

Follow the `tdd-workflow` skill. Create `src/crd_tests.rs` tests for the new types.

### Step 3: Generate the CRD YAML

Run the `regen-crds` skill.

### Step 4: Create an example manifest

Create `examples/my-new-resource.yaml` verified against the generated CRD schema.

### Step 5: Validate

Run the `validate-examples` skill.

### Step 6: Create the reconciler

Create `src/reconcilers/mynewresource.rs` and `src/reconcilers/mynewresource_tests.rs`.

### Step 7: Regenerate API docs (LAST)

Run the `regen-api-docs` skill.

### Step 8: Run quality checks

Run the `cargo-quality` skill.

---

## Skill: `pre-commit-checklist`

Run this checklist before every commit. A task is **NOT complete** until all items pass.

### Gate 1: Rust Quality

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

All three must pass with zero warnings and zero failures.

### Gate 2: CRD Sync (if `src/crd.rs` was changed)

```bash
cargo run --bin crdgen
git diff --stat deploy/crds/
```

If CRDs changed, run `regen-api-docs` last.

### Gate 3: Examples Valid (if CRDs or examples changed)

```bash
kubectl apply --dry-run=client -f examples/
```

### Gate 4: Documentation

- [ ] `.claude/CHANGELOG.md` updated with **Author**, Changed, Why, Impact
- [ ] `docs/src/` pages updated for any user-visible changes
- [ ] `docs/roadmaps/` updated if a phase/milestone was completed
- [ ] Documentation builds: `make docs`

### Gate 5: Security (if `Cargo.toml` or `Cargo.lock` changed)

```bash
make cargo-deny
make license-check
```

### Gate 6: No Forbidden Patterns

- [ ] No `unwrap()` in production code (`src/` excluding `_tests.rs`)
- [ ] No hardcoded namespaces
- [ ] No magic numbers (literals other than `0` or `1`) in `src/`
- [ ] No secrets, tokens, or internal hostnames
- [ ] No `docker build`, `docker push`, or `kubectl rollout restart`

**If any gate fails, fix it before committing.**
