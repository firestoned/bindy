# Project Instructions for Claude Code

> Platform Engineering - Kubernetes Operators & Infrastructure
> Environment: k0rdent / Capital Markets / Multi-cluster
>
> **Service Mesh Standard**: Always use Linkerd as the example service mesh in documentation, examples, and code comments. Do not use generic "service mesh" references or other mesh implementations (Istio, Consul Connect, etc.) unless specifically required.

---

## 🚨 Critical TODOs

### Code Quality: Use Global Constants for Repeated Strings
**Status:** 🔄 Ongoing
**Impact:** Code maintainability and consistency

When a string literal appears in multiple places across the codebase, it MUST be defined as a global constant and referenced consistently.

**Why:**
- **Single Source of Truth**: Changes only need to be made in one place
- **Consistency**: Prevents typos and inconsistencies across the codebase
- **Maintainability**: Easier to refactor and update values
- **Type Safety**: Compiler catches usage errors

**When to Create a Global Constant:**
- String appears 2+ times in the same file
- String appears in multiple files
- String represents a configuration value (paths, filenames, keys, etc.)
- String is part of an API contract or protocol

**Examples:**
```rust
// ✅ GOOD - Use constants
const BIND_NAMED_CONF_PATH: &str = "/etc/bind/named.conf";
const NAMED_CONF_FILENAME: &str = "named.conf";

fn build_configmap() {
    data.insert(NAMED_CONF_FILENAME.into(), named_conf);
}

// ❌ BAD - Hardcoded strings
fn build_configmap() {
    data.insert("named.conf".into(), named_conf);
}
```

**Where to Define Constants:**
- Module-level constants: At the top of the file for file-specific use
- Crate-level constants: In a dedicated module (e.g., `src/constants.rs`) for cross-module use
- Group related constants together with documentation

**Verification:**
Before committing, search for repeated string literals:
```bash
# Find potential duplicate strings in Rust files
grep -rn '"[^"]\{5,\}"' src/ | sort | uniq -d
```

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

### Mandatory: Documentation Updates for Code Changes

**CRITICAL: After ANY code change in the `src/` directory, you MUST update all relevant documentation.**

This is a **mandatory step** that must be completed before considering any task complete. Documentation must always reflect the current state of the code.

#### Documentation Update Workflow

When adding, removing, or changing any feature in the Rust source code:

1. **Analyze the Change**:
   - What functionality was added/removed/changed?
   - What are the user-facing impacts?
   - What are the architectural implications?
   - Are there new APIs, configuration options, or behaviors?

2. **Update Documentation** (in this order):
   - **`CHANGELOG.md`** - Document the change (see format below)
   - **`docs/src/`** - Update all affected documentation pages:
     - User guides that reference the changed functionality
     - Quickstart guides with examples of the changed code
     - Configuration references for new/changed options
     - Troubleshooting guides if behavior changed
   - **`examples/`** - Update YAML examples to reflect changes
   - **Architecture diagrams** - Update if structure/flow changed
   - **API documentation** - Regenerate if CRDs changed (`cargo run --bin crddoc`)
   - **README.md** - Update if getting started steps or features changed

3. **Verify Documentation Accuracy**:
   - Read through updated docs as if you're a new user
   - Ensure all code examples compile and run
   - Verify all YAML examples validate: `kubectl apply --dry-run=client -f examples/`
   - Check that diagrams match current architecture
   - Confirm API docs reflect current CRD schemas

4. **Add Missing Documentation**:
   - If architecture changed, add/update architecture diagrams
   - If new public APIs were added, document them
   - If new configuration options exist, document them with examples
   - If new error conditions exist, document troubleshooting steps
   - If new dependencies were added, document version requirements

#### What Documentation to Update

**For Controller/Reconciler Changes** (`src/reconcilers/`):
- Update reconciliation flow diagrams
- Document new behaviors in user guides
- Update troubleshooting guides for new error conditions
- Add examples showing the new functionality

**For CRD Changes** (`src/crd.rs`):
- Run `cargo run --bin crdgen` to regenerate CRD YAMLs
- Run `cargo run --bin crddoc > docs/src/reference/api.md` to regenerate API docs
- Update ALL examples in `/examples/` that use the changed CRD
- Update quickstart guides with new field examples
- Update configuration reference documentation

**For Core Logic Changes** (`src/bind9.rs`, `src/bind9_resources.rs`, etc.):
- Update architecture documentation explaining the change
- Update API documentation if public interfaces changed
- Add code examples for new public functions
- Update troubleshooting guides for new behaviors

**For New Features**:
- Add feature documentation to `/docs/src/features/`
- Update feature list in README.md
- Add usage examples
- Create architecture diagrams showing how the feature works
- Document configuration options
- Add troubleshooting section

**For Bug Fixes**:
- Update troubleshooting guides with the fix
- Document workarounds (if applicable) in known issues
- Update behavior documentation if expectations changed

#### Documentation Quality Standards

- **Completeness**: All user-visible changes must be documented
- **Accuracy**: Documentation must match the actual code behavior
- **Examples**: Include working examples for all features
- **Clarity**: Write for users who haven't seen the code
- **Diagrams**: Use Mermaid diagrams for complex flows
- **Versioning**: Date all changes in CHANGELOG.md

#### Validation Checklist

Before considering a task complete, verify:
- [ ] CHANGELOG.md updated with change details
- [ ] All affected documentation pages updated
- [ ] All YAML examples validate successfully
- [ ] API documentation regenerated (if CRDs changed)
- [ ] Architecture diagrams updated (if structure changed)
- [ ] Code examples compile and run
- [ ] README.md updated (if getting started or features changed)
- [ ] No broken links in documentation
- [ ] Documentation reviewed as if reading for the first time

### Mandatory: Update Changelog on Every Code Change

After **ANY** code modification, update `CHANGELOG.md` with the following format:

```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

**Author:** [Author Name]

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

**CRITICAL REQUIREMENT**:
- The `**Author:**` line is **MANDATORY** for ALL changelog entries
- This is required for auditing and accountability in a regulated environment
- The author field should contain the name of the person who requested or approved the change
- **NO exceptions** - every changelog entry must have an author attribution
- If the author is unknown, use "Unknown" but investigate to identify the proper author

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

6. **Test File Organization:**
   - **CRITICAL**: ALWAYS place tests in separate `_tests.rs` files (see Testing Requirements section below)
   - NEVER embed large test modules directly in source files
   - Follow the pattern: `foo.rs` → `foo_tests.rs`

**VERIFICATION:**
- After ANY Rust code change, run `cargo test` in the modified file's crate
- ALL tests MUST pass before the task is considered complete
- If you add code but cannot write a test, document WHY in the code comments

**Example:**
If you modify `src/reconcilers/records.rs`:
1. Update/add tests in `src/reconcilers/records_tests.rs` (separate file)
2. Ensure `src/reconcilers/records.rs` has: `#[cfg(test)] mod records_tests;`
3. Run `cargo test --lib reconcilers::records` to verify
4. Ensure ALL tests pass before moving on

### Rust Style Guidelines

- Use `thiserror` for error types, not string errors
- Prefer `anyhow::Result` in binaries, typed errors in libraries
- Use `tracing` for logging, not `println!` or `log`
- Async functions should use `tokio`
- All k8s API calls must have timeout and retry logic
- **No magic numbers**: Any numeric literal other than `0` or `1` MUST be declared as a named constant

#### Magic Numbers Rule

**CRITICAL: Eliminate all magic numbers from the codebase.**

A "magic number" is any numeric literal (other than `0` or `1`) that appears directly in code without explanation.

**Why:**
- **Readability**: Named constants make code self-documenting
- **Maintainability**: Change the value in one place, not scattered throughout
- **Semantic Meaning**: The constant name explains *why* the value matters
- **Type Safety**: Constants prevent accidental typos in numeric values

**Rules:**
- **`0` and `1` are allowed** - These are ubiquitous and self-explanatory (empty, none, first item, etc.)
- **All other numbers MUST be named constants** - No exceptions
- Use descriptive names that explain the *purpose*, not just the value

**Examples:**

```rust
// ✅ GOOD - Named constants
const DEFAULT_ZONE_TTL: u32 = 3600;
const MAX_RETRY_ATTEMPTS: u8 = 3;
const RECONCILE_INTERVAL_SECS: u64 = 300;
const DNS_PORT: u16 = 53;

fn build_zone(ttl: Option<u32>) -> Zone {
    Zone {
        ttl: ttl.unwrap_or(DEFAULT_ZONE_TTL),
        ..
    }
}

fn reconcile() -> Action {
    Action::requeue(Duration::from_secs(RECONCILE_INTERVAL_SECS))
}

// ❌ BAD - Magic numbers
fn build_zone(ttl: Option<u32>) -> Zone {
    Zone {
        ttl: ttl.unwrap_or(3600),  // What does 3600 mean? Why this value?
        ..
    }
}

fn reconcile() -> Action {
    Action::requeue(Duration::from_secs(300))  // Why 300?
}
```

**Special Cases:**

- **Unit conversions**: Still need constants
  ```rust
  // ✅ GOOD
  const MILLISECONDS_PER_SECOND: u64 = 1000;
  const SECONDS_PER_HOUR: u64 = 3600;

  // ❌ BAD
  Duration::from_millis(timeout_secs * 1000)
  ```

- **Array sizes/indexing**: Use constants if size is meaningful
  ```rust
  // ✅ GOOD
  const MAX_DNS_LABELS: usize = 127;
  let labels = vec![String::new(); MAX_DNS_LABELS];

  // ✅ ACCEPTABLE - indexing with 0 or 1
  let first = items[0];
  let second = items[1];

  // ❌ BAD - other index values
  let third = items[2];  // Should be named if it has semantic meaning
  ```

- **Buffer sizes**: Always use named constants
  ```rust
  // ✅ GOOD
  const READ_BUFFER_SIZE: usize = 8192;
  let mut buf = vec![0u8; READ_BUFFER_SIZE];

  // ❌ BAD
  let mut buf = vec![0u8; 8192];
  ```

**Where to Define Constants:**
- Module-level: For constants used only within one file
- Crate-level (`src/constants.rs`): For constants used across modules
- Group related constants together with documentation

**Test Files Exception:**
Test files (`*_tests.rs`) may use literal values for test data when it improves readability and the values are only used once. However, if the same test value appears multiple times or represents a meaningful configuration value, it should still use the global constants.

**Verification:**
Before committing, manually scan code for numeric literals:
```bash
# Find numeric literals other than 0 and 1 in Rust files (excludes test files)
grep -Ern '\b[2-9][0-9]*\b' src/ --include="*.rs" --exclude="*_tests.rs" | grep -v '^[^:]*:[^:]*://.*$'
```

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

#### Test File Organization

**CRITICAL: ALWAYS place unit tests in separate `_tests.rs` files, NOT embedded in the source file.**

This is the **required pattern** for this codebase. Do NOT embed tests directly in source files.

**Correct Pattern:**

For a source file `src/foo.rs`:
1. Create a separate test file `src/foo_tests.rs`
2. In `src/foo.rs`, add at the bottom:
   ```rust
   #[cfg(test)]
   mod foo_tests;
   ```
3. In `src/foo_tests.rs`, write all tests:
   ```rust
   // Copyright (c) 2025 Erick Bourgeois, firestoned
   // SPDX-License-Identifier: MIT

   //! Unit tests for `foo.rs`

   #[cfg(test)]
   mod tests {
       use super::super::*;  // Import from parent module

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

**Why Separate Test Files?**
1. **Faster Compilation**: Tests only compile when running `cargo test`
2. **Better Organization**: Clear separation between production and test code
3. **Easier Maintenance**: All tests for a module in one dedicated file
4. **Cleaner Code**: Main source files remain focused on production logic

**Examples in This Codebase:**
- `src/main.rs` → `src/main_tests.rs`
- `src/bind9.rs` → `src/bind9_tests.rs`
- `src/crd.rs` → `src/crd_tests.rs`
- `src/bind9_resources.rs` → `src/bind9_resources_tests.rs`
- `src/reconcilers/bind9cluster.rs` → `src/reconcilers/bind9cluster_tests.rs`

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
├── main.rs                  # Entry point, CLI setup
├── main_tests.rs            # Tests for main.rs
├── lib.rs                   # Library exports
├── bind9.rs                 # BIND9 zone file generation
├── bind9_tests.rs           # Tests for bind9.rs
├── bind9_resources.rs       # BIND9 Kubernetes resource builders
├── bind9_resources_tests.rs # Tests for bind9_resources.rs
├── crd.rs                   # Custom Resource Definitions
├── crd_tests.rs             # Tests for crd.rs
├── crd_docs.rs              # CRD documentation helpers
├── crd_docs_tests.rs        # Tests for crd_docs.rs
├── labels.rs                # Standard Kubernetes labels
├── reconcilers/             # Reconciliation logic
│   ├── mod.rs               # Module exports
│   ├── bind9cluster.rs      # Bind9Cluster reconciler
│   ├── bind9cluster_tests.rs # Tests for bind9cluster.rs
│   ├── bind9instance.rs     # Bind9Instance reconciler
│   ├── bind9instance_tests.rs # Tests for bind9instance.rs
│   ├── dnszone.rs           # DNSZone reconciler
│   ├── dnszone_tests.rs     # Tests for dnszone.rs
│   ├── records.rs           # DNS record reconcilers
│   └── records_tests.rs     # Tests for records.rs
└── bin/
    ├── crdgen.rs            # CRD YAML generator
    └── crddoc.rs            # CRD documentation generator
```

**Test File Pattern:**
- Every `foo.rs` has a corresponding `foo_tests.rs`
- Test files are in the same directory as the source file
- Source file declares: `#[cfg(test)] mod foo_tests;`
- Test file contains: `#[cfg(test)] mod tests { ... }`

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
  - [ ] **Documentation updated** for code changes (REQUIRED - see Documentation Requirements section):
    - [ ] Rustdoc comments on ALL public items (functions, types, modules)
    - [ ] Function documentation matches actual behavior (parameters, returns, errors)
    - [ ] `/docs/src/` updated for user-facing changes
    - [ ] Architecture diagrams updated if structure changed
    - [ ] Examples added for new features
    - [ ] Troubleshooting docs updated for new error conditions
- [ ] **If `src/crd.rs` was modified**:
  - [ ] Run `cargo run --bin crdgen` to regenerate CRD YAMLs
  - [ ] **Update `/examples/*.yaml` to match new schema** (CRITICAL)
  - [ ] **Update `/docs/src/` documentation** that references the CRDs (CRITICAL)
  - [ ] Run `./scripts/validate-examples.sh` to verify all examples are valid (REQUIRED)
  - [ ] Run `cargo fmt`, `cargo clippy`, and `cargo test` to ensure everything passes
  - [ ] **LAST STEP**: Run `cargo run --bin crddoc > docs/src/reference/api.md` to regenerate API docs **AFTER** all validations pass
- [ ] **If `src/reconcilers/` was modified**:
  - [ ] Update reconciliation flow diagrams in `/docs/src/architecture/`
  - [ ] Document new behaviors in user guides
  - [ ] Update troubleshooting guides for new error conditions
  - [ ] Add examples showing the new functionality
  - [ ] Verify all examples still work with the changes
- [ ] **Documentation verification** (CRITICAL):
  - [ ] `CHANGELOG.md` updated with detailed change description **AND author attribution** (REQUIRED)
  - [ ] Author name included in changelog entry (e.g., `**Author:** Erick Bourgeois`)
  - [ ] All affected documentation pages reviewed and updated
  - [ ] All YAML examples validate: `kubectl apply --dry-run=client -f examples/`
  - [ ] Code examples in docs compile and run
  - [ ] Architecture diagrams match current implementation
  - [ ] API documentation reflects current CRD schemas
  - [ ] README.md updated if getting started or features changed
  - [ ] No broken links in documentation
- [ ] CRD YAML files validate: `kubectl apply --dry-run=client -f deploy/crds/`
- [ ] No secrets or sensitive data
- [ ] Error handling uses proper types (no `.unwrap()`)

**A task is NOT complete until all of the above items pass successfully.**

**Documentation is NOT optional** - it is a critical requirement equal in importance to the code itself.

---

## 🔗 Project References

- [kube-rs documentation](https://kube.rs/)
- [Kubernetes API conventions](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md)
- [Operator pattern](https://kubernetes.io/docs/concepts/extend-kubernetes/operator/)
- Internal: k0rdent platform docs (check Confluence)
