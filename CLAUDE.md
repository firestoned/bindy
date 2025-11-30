# Project Instructions for Claude Code

> Platform Engineering - Kubernetes Operators & Infrastructure
> Environment: k0rdent / Capital Markets / Multi-cluster

---

## 🚨 Critical TODOs

### High Priority: CRD Code Generation
**Status:** ✅ Implemented
**Impact:** Automated - CRD YAMLs are generated from Rust types

The Rust types in `src/crd.rs` are the **source of truth**. CRD YAML files in `/deploy/crds/` are **auto-generated** from these types.

**Workflow:**
1. Edit Rust types in `src/crd.rs`
2. Run `cargo run --bin crdgen` to regenerate YAML files
3. **CRITICAL**: Update examples in `/examples/` to match new schema
4. **CRITICAL**: Update documentation in `/docs/src/` that references the CRDs
5. Validate all examples: `kubectl apply --dry-run=client -f examples/`
6. Run `cargo fmt`, `cargo clippy`, and `cargo test` to ensure code quality
7. **CRITICAL**: Run `cargo run --bin crddoc > docs/src/reference/api.md` to regenerate API docs **AFTER** all changes are complete and validated
8. Deploy with `kubectl apply -f deploy/crds/`

⚠️ **IMPORTANT**: Always run `crddoc` **LAST** after all CRD changes, example updates, and validations are complete. This ensures the API documentation reflects the final, validated state of the CRDs.

See **CRD Development - Rust as Source of Truth** section below for details.

⚠️ **IMPORTANT**: Examples and documentation MUST stay in sync with CRD schemas. After ANY CRD change, you MUST update:
- `/examples/*.yaml` - Ensure all examples can be applied successfully
- `/docs/src/` - Update any documentation that references the CRD fields
- Quickstart guide - Verify all YAML snippets are valid

---

## 🔒 Compliance & Security Context

This codebase operates in a **regulated banking environment**. All changes must be:
- Auditable with clear documentation
- Traceable to a business or technical requirement
- Compliant with zero-trust security principles

**Never commit**:
- Secrets, tokens, or credentials (even examples)
- Internal hostnames or IP addresses
- Customer or transaction data in any form

---

## 📝 Documentation Requirements

### Mandatory: Update Changelog on Every Code Change

After **ANY** code modification, update `CHANGELOG.md` with the following format:

```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

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

### Code Comments

All public functions and types **must** have rustdoc comments:

```rust
/// Reconciles the BindZone custom resource.
///
/// # Arguments
/// * `zone` - The BindZone CR to reconcile
/// * `ctx` - Controller context with client and state
///
/// # Errors
/// Returns `ReconcileError` if DNS zone update fails or API is unreachable.
pub async fn reconcile(zone: Arc<BindZone>, ctx: Arc<Context>) -> Result<Action, ReconcileError> {
```

### Architecture Decision Records (ADRs)

For significant design decisions, create `/docs/adr/NNNN-title.md`:

```markdown
# ADR-NNNN: Title

## Status
Proposed | Accepted | Deprecated | Superseded by ADR-XXXX

## Context
What is the issue we're facing?

## Decision
What have we decided to do?

## Consequences
What are the trade-offs?
```

---

## 🦀 Rust Workflow

### After Modifying Any `.rs` File

**CRITICAL: At the end of EVERY task that modifies Rust files, ALWAYS run these commands in order:**

```bash
# 1. Format code
cargo fmt

# 2. Run clippy with strict warnings
cargo clippy -- -D warnings -W clippy::pedantic -A clippy::module_name_repetitions

# 3. Run tests
cargo test

# 4. Check for security vulnerabilities (if cargo-audit installed)
cargo audit 2>/dev/null || true
```

**IMPORTANT:**
- This is MANDATORY at the end of every task involving Rust code changes
- Fix ALL clippy warnings before considering the task complete
- Do NOT skip these steps - they catch bugs and ensure code quality
- If clippy or tests fail, the task is NOT complete

**CRITICAL: After ANY Rust code modification, you MUST verify:**

1. **Function documentation is accurate**:
   - Check rustdoc comments match what the function actually does
   - Verify all `# Arguments` match the actual parameters
   - Verify `# Returns` matches the actual return type
   - Verify `# Errors` describes all error cases
   - Update examples in doc comments if behavior changed

2. **Unit tests are accurate and passing**:
   - Check test assertions match the new behavior
   - Update test expectations if behavior changed
   - Ensure all tests compile and run successfully
   - Add new tests for new behavior/edge cases

3. **End-user documentation is updated**:
   - Update relevant files in `docs/` directory
   - Update examples in `examples/` directory
   - Ensure `CHANGELOG.md` reflects the changes
   - Verify example YAML files validate successfully

### Unit Testing Requirements

**CRITICAL: When modifying ANY Rust code, you MUST update, add, or delete unit tests accordingly:**

1. **Adding New Functions/Methods:**
   - MUST add unit tests for ALL new public functions
   - Test both success and failure scenarios
   - Include edge cases and boundary conditions

2. **Modifying Existing Functions:**
   - MUST update existing tests to reflect changes
   - Add new tests if new behavior or code paths are introduced
   - Ensure ALL existing tests still pass

3. **Deleting Functions:**
   - MUST delete corresponding unit tests
   - Remove or update integration tests that depended on deleted code

4. **Refactoring Code:**
   - Update test names and assertions to match refactored code
   - Verify test coverage remains the same or improves
   - If refactoring changes function signatures, update ALL tests

5. **Test Quality Standards:**
   - Use descriptive test names (e.g., `test_reconcile_creates_zone_when_missing`)
   - Follow the Arrange-Act-Assert pattern
   - Mock external dependencies (k8s API, external services)
   - Test error conditions, not just happy paths
   - Ensure tests are deterministic (no flaky tests)

**VERIFICATION:**
- After ANY Rust code change, run `cargo test` in the modified file's crate
- ALL tests MUST pass before the task is considered complete
- If you add code but cannot write a test, document WHY in the code comments

**Example:**
If you modify `src/reconcilers/records.rs`:
1. Update/add tests in the `#[cfg(test)]` module at the bottom of the same file
2. Run `cargo test --lib reconcilers::records` to verify
3. Ensure ALL tests pass before moving on

### Rust Style Guidelines

- Use `thiserror` for error types, not string errors
- Prefer `anyhow::Result` in binaries, typed errors in libraries
- Use `tracing` for logging, not `println!` or `log`
- Async functions should use `tokio`
- All k8s API calls must have timeout and retry logic

### Dependency Management

Before adding a new dependency:
1. Check if existing deps solve the problem
2. Verify the crate is actively maintained (commits in last 6 months)
3. Prefer crates from well-known authors or the Rust ecosystem
4. Document why the dependency was added in `CHANGELOG.md`

---

## ☸️ Kubernetes Operator Patterns

### CRD Development - Rust as Source of Truth

**CRITICAL: Rust types in `src/crd.rs` are the source of truth.**

CRD YAML files in `/deploy/crds/` are **AUTO-GENERATED** from the Rust types. This ensures:
- Type safety enforced at compile time
- CRDs deployed to clusters match what the operator expects
- Schema validation in Kubernetes matches Rust types
- No drift between deployed CRDs and operator code

#### Workflow for CRD Changes:

1. **Edit the Rust types** in `src/crd.rs`
2. **Regenerate CRD YAML files**:
   ```bash
   cargo run --bin crdgen
   ```
3. **Regenerate API documentation**:
   ```bash
   cargo run --bin crddoc > docs/src/reference/api.md
   ```
4. **Verify generated YAMLs** look correct
5. **Update `CHANGELOG.md`** documenting the CRD change
6. **Deploy updated CRDs**:
   ```bash
   kubectl apply -f deploy/crds/
   ```

#### Generated YAML Format:

All generated YAML files include:
- Copyright header: `# Copyright (c) 2025 Erick Bourgeois, firestoned`
- SPDX license identifier: `# SPDX-License-Identifier: MIT`
- Auto-generated warning: `# DO NOT EDIT MANUALLY - Run 'cargo run --bin crdgen' to regenerate`

**Never edit the YAML files directly** - your changes will be overwritten on next generation.

#### Adding New CRDs:

1. Add the new CustomResource to `src/crd.rs`:
   ```rust
   #[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
   #[kube(
       group = "dns.firestoned.io",
       version = "v1alpha1",
       kind = "MyNewResource",
       namespaced
   )]
   #[serde(rename_all = "camelCase")]
   pub struct MyNewResourceSpec {
       pub field_name: String,
   }
   ```

2. Add it to the crdgen binary in `src/bin/crdgen.rs`:
   ```rust
   generate_crd::<MyNewResource>("mynewresources.crd.yaml", output_dir)?;
   ```

3. Regenerate YAMLs:
   ```bash
   cargo run --bin crdgen
   ```

#### CI/CD Integration:

Add this to your CI pipeline to ensure CRDs and documentation stay in sync:

```bash
# Generate CRDs
cargo run --bin crdgen

# Generate API documentation
cargo run --bin crddoc > docs/src/reference/api.md

# Check if any files changed
if ! git diff --quiet deploy/crds/ docs/src/reference/api.md; then
  echo "ERROR: CRD YAML files or API documentation are out of sync with src/crd.rs"
  echo "Run: cargo run --bin crdgen"
  echo "Run: cargo run --bin crddoc > docs/src/reference/api.md"
  exit 1
fi
```

#### Example CRD Structure:

```rust
/// Spec for BindZone resource - MUST match dnszones.crd.yaml
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    kind = "DNSZone",
    group = "dns.firestoned.io",
    version = "v1alpha1",
    namespaced,
    status = "DNSZoneStatus",
    printcolumn = r#"{"name":"Zone","type":"string","jsonPath":".spec.zoneName"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct DNSZoneSpec {
    /// The DNS zone name (e.g., "example.com")
    pub zone_name: String,
    // ... other fields - verify against YAML!
}
```

### Controller Best Practices

- Always set `ownerReferences` for child resources
- Use finalizers for cleanup logic
- Implement exponential backoff for retries
- Set appropriate `requeue_after` durations
- Log reconciliation start/end with resource name and namespace

### Status Conditions

Always update status conditions following Kubernetes conventions:

```rust
Condition {
    type_: "Ready".to_string(),
    status: "True".to_string(),
    reason: "ReconcileSucceeded".to_string(),
    message: "Zone synchronized successfully".to_string(),
    last_transition_time: Some(Time(Utc::now())),
    observed_generation: Some(zone.metadata.generation.unwrap_or(0)),
}
```

---

## 🔄 FluxCD / GitOps Integration

### Kustomization Structure

```
clusters/
├── base/
│   ├── kustomization.yaml
│   └── resources/
└── overlays/
    ├── dev/
    ├── staging/
    └── prod/
```

### HelmRelease Changes

When modifying HelmRelease manifests:
1. Bump the chart version or values checksum
2. Add suspend annotation for breaking changes
3. Document rollback procedure in `CHANGELOG.md`

---

## 🧪 Testing Requirements

### Unit Tests

**MANDATORY: Every public function MUST have corresponding unit tests.**

Place unit tests in a `#[cfg(test)]` module at the bottom of the same file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reconcile_creates_zone() {
        // Arrange
        let (client, _mock) = mock_client().await;
        let zone = create_test_zone("example.com");
        let ctx = create_test_context(client);

        // Act
        let result = reconcile(zone, ctx).await;

        // Assert
        assert!(result.is_ok());
        assert_eq!(result.unwrap().requeue_after, Some(Duration::from_secs(300)));
    }

    #[tokio::test]
    async fn test_reconcile_handles_api_error() {
        // Arrange
        let (client, mock) = mock_client_with_error().await;
        let zone = create_test_zone("example.com");
        let ctx = create_test_context(client);

        // Act
        let result = reconcile(zone, ctx).await;

        // Assert
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ReconcileError::ApiError(_)));
    }
}
```

**Test Coverage Requirements:**
- **Success path:** Test the primary expected behavior
- **Failure paths:** Test error handling for each possible error type
- **Edge cases:** Empty strings, null values, boundary conditions
- **State changes:** Verify correct state transitions
- **Async operations:** Test timeouts, retries, and cancellation

**When to Update Tests:**
- **ALWAYS** when adding new functions → Add new tests
- **ALWAYS** when modifying functions → Update existing tests
- **ALWAYS** when deleting functions → Delete corresponding tests
- **ALWAYS** when refactoring → Verify tests still cover the same behavior

### Integration Tests

Place in `/tests/` directory:
- Use `k8s-openapi` test fixtures
- Mock external services (BIND API, etc.)
- Test failure scenarios, not just happy path
- Test end-to-end workflows (create → update → delete)
- Verify finalizers and cleanup logic

### Test Execution

**Before committing ANY Rust changes:**
```bash
# Run all tests
cargo test

# Run tests for a specific file
cargo test --lib <module_path>

# Run tests with output
cargo test -- --nocapture

# Run tests with coverage (if tarpaulin installed)
cargo tarpaulin --out Html
```

**ALL tests MUST pass before code is considered complete.**

---

## 📁 File Organization

```
src/
├── main.rs           # Entry point, CLI setup
├── lib.rs            # Library exports
├── controller/       # Reconciliation logic
│   ├── mod.rs
│   └── bindzone.rs
├── crd/              # Custom Resource definitions
│   ├── mod.rs
│   └── types.rs
├── error.rs          # Error types
└── metrics.rs        # Prometheus metrics
```

---

## 🚫 Things to Avoid

- **Never** use `unwrap()` in production code - use `?` or explicit error handling
- **Never** hardcode namespaces - make them configurable
- **Never** use `sleep()` for synchronization - use proper k8s watch/informers
- **Never** ignore errors in finalizers - this blocks resource deletion
- **Never** store state outside of Kubernetes - operators must be stateless

---

## 💡 Helpful Commands

```bash
# Generate CRD YAML files from Rust types
cargo run --bin crdgen

# Generate API documentation from CRDs
cargo run --bin crddoc > docs/src/reference/api.md

# Validate generated CRD YAML files
for file in deploy/crds/*.crd.yaml; do
  echo "Checking $file"
  kubectl apply --dry-run=client -f "$file"
done

# Run operator locally against current kubeconfig
RUST_LOG=debug cargo run

# Build multi-arch container image
docker buildx build --platform linux/amd64,linux/arm64 -t registry/operator:tag .

# Validate all manifests
kubectl apply --dry-run=server -f deploy/
```

---

## 📋 PR/Commit Checklist

**MANDATORY: Run this checklist at the end of EVERY task before considering it complete.**

Before committing:

- [ ] **If ANY `.rs` file was modified**:
  - [ ] **Unit tests updated/added/deleted** to match code changes (REQUIRED)
  - [ ] All new public functions have corresponding tests (REQUIRED)
  - [ ] All modified functions have updated tests (REQUIRED)
  - [ ] All deleted functions have tests removed (REQUIRED)
  - [ ] `cargo fmt` passes (REQUIRED)
  - [ ] `cargo clippy -- -D warnings` passes (REQUIRED - fix ALL warnings)
  - [ ] `cargo test` passes (REQUIRED - ALL tests must pass)
- [ ] **If `src/crd.rs` was modified**:
  - [ ] Run `cargo run --bin crdgen` to regenerate CRD YAMLs
  - [ ] **Update `/examples/*.yaml` to match new schema** (CRITICAL)
  - [ ] **Update `/docs/src/` documentation** that references the CRDs (CRITICAL)
  - [ ] Run `./scripts/validate-examples.sh` to verify all examples are valid (REQUIRED)
  - [ ] Run `cargo fmt`, `cargo clippy`, and `cargo test` to ensure everything passes
  - [ ] **LAST STEP**: Run `cargo run --bin crddoc > docs/src/reference/api.md` to regenerate API docs **AFTER** all validations pass
- [ ] CRD YAML files validate: `kubectl apply --dry-run=client -f deploy/crds/`
- [ ] `CHANGELOG.md` updated
- [ ] No secrets or sensitive data
- [ ] Rustdoc comments on public items
- [ ] Error handling uses proper types (no `.unwrap()`)

**A task is NOT complete until all of the above items pass successfully.**

---

## 🔗 Project References

- [kube-rs documentation](https://kube.rs/)
- [Kubernetes API conventions](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md)
- [Operator pattern](https://kubernetes.io/docs/concepts/extend-kubernetes/operator/)
- Internal: k0rdent platform docs (check Confluence)
