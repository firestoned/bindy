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
3. Deploy with `kubectl apply -k deploy/crds/`

See **CRD Development - Rust as Source of Truth** section below for details.

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

After **ANY** code modification, update `/docs/CHANGELOG.md` with the following format:

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
4. Document why the dependency was added in CHANGELOG.md

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
5. **Update CHANGELOG.md** documenting the CRD change
6. **Deploy updated CRDs**:
   ```bash
   kubectl apply -k deploy/crds/
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
3. Document rollback procedure in CHANGELOG.md

---

## 🧪 Testing Requirements

### Unit Tests

Every public function should have tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reconcile_creates_zone() {
        // Arrange
        let (client, _mock) = mock_client().await;
        
        // Act
        let result = reconcile(zone, ctx).await;
        
        // Assert
        assert!(result.is_ok());
    }
}
```

### Integration Tests

Place in `/tests/` directory:
- Use `k8s-openapi` test fixtures
- Mock external services (BIND API, etc.)
- Test failure scenarios, not just happy path

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
  - [ ] `cargo fmt` passes (REQUIRED)
  - [ ] `cargo clippy -- -D warnings` passes (REQUIRED - fix ALL warnings)
  - [ ] `cargo test` passes (REQUIRED)
- [ ] **If `src/crd.rs` was modified**:
  - [ ] Run `cargo run --bin crdgen` to regenerate CRD YAMLs
  - [ ] Run `cargo run --bin crddoc > docs/src/reference/api.md` to regenerate API docs
- [ ] CRD YAML files validate: `kubectl apply --dry-run=client -f deploy/crds/`
- [ ] CHANGELOG.md updated
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
