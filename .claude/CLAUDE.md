@.claude/SKILL.md

# Project Instructions for Claude Code

> Platform Engineering - Kubernetes Operators & Infrastructure
> Environment: k0rdent / Capital Markets / Multi-cluster
>
> **Service Mesh Standard**: Always use Linkerd as the example service mesh in documentation, examples, and code comments. Do not use generic "service mesh" references or other mesh implementations (Istio, Consul Connect, etc.) unless specifically required.
>
> **CRITICAL Coding Patterns**:
> - **Test-Driven Development (TDD)**: ALWAYS write tests FIRST before implementing functionality. Write failing tests that define the expected behavior, then implement code to make tests pass. This ensures all code is testable and has comprehensive test coverage from the start.
> - **Event-Driven Programming**: In Kubernetes controller development, ALWAYS use or recommend event-driven programming (e.g., "watch" on kube API) as opposed to polling. Controllers must react to cluster state changes efficiently.
> - **Early Returns**: Use as few `else` statements as possible. Return from functions as soon as you can to minimize nesting and improve code clarity (see Early Return / Guard Clause Pattern section).
> - **ALWAYS Run cargo fmt**: At the end of EVERY task or phase involving Rust code, you MUST run `cargo fmt` (and `cargo clippy`, `cargo test`). This is NON-NEGOTIABLE and MANDATORY. See "CRITICAL: Always Run cargo fmt and clippy After Code Changes" section for full requirements.

---

## ⚙️ Claude Code Configuration

### 🚨 CRITICAL: Keep Bootstrap RBAC in Sync With Static YAML Files and Docs

**MANDATORY REQUIREMENT:** Any time you modify ClusterRole, Role, ClusterRoleBinding, or RoleBinding definitions in `src/bootstrap.rs`, you MUST update ALL of the following in the same change:

| File | What to update |
|------|---------------|
| `deploy/scout/clusterrole.yaml` | Mirror the exact rules from `build_scout_cluster_role` |
| `deploy/scout/clusterrolebinding.yaml` | Mirror any binding changes |
| `deploy/scout/role.yaml` | Mirror rules from `build_scout_role` (if exists) |
| `deploy/scout/rolebinding.yaml` | Mirror any role binding changes |
| `deploy/scout.yaml` | The single-file install — update the same RBAC sections inline |
| `docs/src/guide/scout.md` | Update any ClusterRole/Role YAML examples shown to users |

**Why This Matters:**
- `src/bootstrap.rs` generates RBAC at runtime via `bindy bootstrap scout`
- The static YAML files in `deploy/scout/` are used for manual installs (`kubectl apply -f`)
- `deploy/scout.yaml` is the single-file "quick install" published in releases
- If they diverge, manually-installed scouts will have different permissions than bootstrapped ones
- Permission gaps cause 403 errors that are hard to diagnose (as seen with the `ingresses patch` bug)

**Verification Checklist:**
- [ ] `deploy/scout/clusterrole.yaml` matches `build_scout_cluster_role` in `bootstrap.rs`
- [ ] `deploy/scout.yaml` ClusterRole section matches the above
- [ ] `docs/src/guide/scout.md` ClusterRole YAML example matches the above
- [ ] Tests in `bootstrap_tests.rs` cover the updated rules

**REMEMBER:** `bootstrap.rs` and the static YAML files are THREE separate representations of the same RBAC policy. They must always be kept in sync.

---

### 🚨 CRITICAL: Always Verify CRD Schema Sync

**MANDATORY REQUIREMENT:** Before investigating any Kubernetes-related issue, ALWAYS verify that deployed CRDs match the Rust code definitions.

**Why This Matters:**
- CRD YAML files in `deploy/crds/` are **AUTO-GENERATED** from Rust types in `src/crd.rs`
- If CRDs are not regenerated after code changes, schema mismatches cause silent failures
- Kubernetes API server may accept patches (HTTP 200) but ignore fields not in the CRD schema
- This leads to confusing bugs where code works but data doesn't persist

> **How:** Run the `verify-crd-sync` skill.

**When to Check:**
- ✅ Before investigating reconciliation loops or infinite loops
- ✅ Before debugging "field not appearing in kubectl output" issues
- ✅ After ANY modification to structs in `src/crd.rs`
- ✅ When status patches succeed but data doesn't persist
- ✅ When user reports unexpected controller behavior

**Example Failure Mode:**
```rust
// Rust code has new field
pub struct Bind9InstanceStatus {
    pub zones: Vec<ZoneReference>,  // Added in recent commit
    // ...
}

// But deployed CRD is missing the field
// Result: Patches succeed (200 OK) but zones never appear in kubectl output
// Controller sees empty zones every reconciliation → infinite loop
```

**REMEMBER:** Always verify CRD schema sync BEFORE making assumptions about code logic issues.

---

### 🚨 CRITICAL: Always Review Official Documentation When Unsure

**MANDATORY REQUIREMENT:** When you are unsure of a decision, DO NOT take the easiest or fastest path. ALWAYS review the official documentation for the product, tool, or framework you are working with.

**Why This Matters:**
- Taking shortcuts leads to incorrect implementations that must be fixed later
- Quick assumptions often miss critical configuration options or best practices
- Official documentation is the authoritative source of truth
- Proper research prevents technical debt and rework

**What to Review:**
- **Configuration Documentation**: For tools like MkDocs, mdbook, GitHub Actions, etc.
- **CRD API Reference**: When working with Kubernetes resources
- **Framework Documentation**: When using libraries or frameworks
- **Best Practices Guides**: Official recommendations for tool usage
- **Release Notes**: Changes in behavior between versions

**Examples:**
- ✅ Unsure about MkDocs configuration → Read the [MkDocs Configuration Reference](https://www.mkdocs.org/user-guide/configuration/)
- ✅ Unsure about Kubernetes CRD fields → Read `/deploy/crds/*.crd.yaml` or `src/crd.rs`
- ✅ Unsure about GitHub Actions syntax → Read the [GitHub Actions Documentation](https://docs.github.com/en/actions)
- ✅ Unsure about Rust kube-rs API → Read the [kube-rs API documentation](https://docs.rs/kube/)

**When to Stop and Research:**
- You're about to make an assumption about how something works
- Multiple approaches seem viable but you're not sure which is correct
- You're implementing a feature you haven't used before
- Documentation or configuration syntax is unfamiliar
- You're debugging an issue and the root cause is unclear

**REMEMBER:** Spending 5 minutes reading documentation prevents hours of debugging incorrect implementations. When in doubt, research first, implement second.

---

### 🚨 CRITICAL: Docker Base Image Digests Must Be Multi-Arch

**MANDATORY REQUIREMENT:** When updating Docker base images in Dockerfiles, ALWAYS use multi-arch manifest list digests, NOT platform-specific digests.

**Why This Matters:**
- Platform-specific digests (e.g., AMD64-only) force Docker BuildKit to use QEMU emulation for other architectures
- QEMU emulation often fails with cryptic errors like: `.buildkit_qemu_emulator: Invalid ELF image for this architecture`
- Multi-arch manifest list digests allow Docker BuildKit to select the correct platform-native image automatically
- This ensures builds work correctly for both `linux/amd64` and `linux/arm64` without emulation

> **How:** Run the `get-multiarch-digest` skill.

**Dockerfile Pattern:**
```dockerfile
# ✅ CORRECT - Multi-arch manifest list digest
# NOTE: This digest points to the multi-arch manifest list (supports both AMD64 and ARM64)
FROM debian:12-slim@sha256:74d56e3931e0d5a1dd51f8c8a2466d21de84a271cd3b5a733b803aa91abf4421 AS builder

# ❌ WRONG - Platform-specific digest (AMD64 only)
FROM debian:12-slim@sha256:0836c58489cd1baee1f617ab06a4fb1b908604d4416022173e4da43ff12399de AS builder
```

**Required Dockerfiles:**
When updating base images, you MUST update ALL Dockerfiles that use the same base image:
- ✅ `docker/Dockerfile` (Distroless variant)
- ✅ `docker/Dockerfile.chainguard` (Chainguard variant)
- ✅ `docker/Dockerfile.chef` (cargo-chef variant)
- ✅ `docker/Dockerfile.fast` (fast build variant)
- ✅ `docker/Dockerfile.local` (local development - usually no digest)

**Common Mistake to Avoid:**
```bash
# ❌ WRONG - This gives you the platform-specific digest for your current architecture
docker pull debian:12-slim
docker inspect debian:12-slim | grep -A 5 RepoDigests

# ✅ CORRECT - This gives you the multi-arch manifest list digest
docker buildx imagetools inspect debian:12-slim --raw | sha256sum
```

**REMEMBER:** Multi-arch manifest list digests prevent ARM64 build failures and QEMU emulation errors while maintaining supply chain security through digest pinning.

---

### 🔍 MANDATORY SEARCH TOOL: ripgrep (rg)
**OBLIGATORY RULE:** ALWAYS use `ripgrep` (command: `rg`) as your PRIMARY and FIRST tool for ANY code search, pattern matching, or grepping task. This is NON-NEGOTIABLE.

### 🛠️ Tool Configuration & Usage:
*   **Default Command:** `rg`
*   **Rust-Specific Flags:** For Rust projects, use `rg -trs <PATTERN>` to search only Rust files (`-trs`) recursively.
*   **Exclusions:** Use `-g '!target/'` to ignore the build directory.
*   **Example Search:** `rg -trs "my_function_name" . -g '!target/'`
*   **No grep/lsof/find:** If Claude tries to use `grep`, `lsof`, or other tools, remind it to use `rg` instead.

### 💡 Workflow for Rust Code Tasks:
1.  **Analyze:** Understand the request and identify relevant files/functions.
2.  **Search (rg):** Use `rg -trs "pattern" .` to find code snippets, definitions, or usages.
3.  **Context:** Use search results to build understanding of the codebase structure.
4.  **Generate/Modify:** Provide Rust code based on analysis, ensuring it adheres to best practices.

## 🚫 CRITICAL: Docker and Kubernetes Operations Restrictions

**NEVER build or push Docker images yourself. The user handles all Docker image operations.**

### Allowed kubectl Operations (Read-Only + Annotations):
- ✅ `kubectl get` - Read resources
- ✅ `kubectl describe` - View resource details
- ✅ `kubectl logs` - Read pod logs
- ✅ `kubectl annotate` - Add/modify annotations
- ✅ Any other read-only operations

### FORBIDDEN Operations:
- ❌ `docker build` - NEVER build Docker images
- ❌ `docker push` - NEVER push images to registries
- ❌ `docker tag` - NEVER tag images
- ❌ `kind load` - NEVER load images into kind
- ❌ `kubectl rollout restart` - NEVER restart deployments/pods
- ❌ `kubectl delete pods` - NEVER delete pods to trigger restarts
- ❌ `kubectl apply` - NEVER apply manifests (unless explicitly requested)
- ❌ `kubectl patch` - NEVER patch resources (unless explicitly requested)
- ❌ Any Docker or deployment operations

**Why:**
- The user manages the deployment pipeline
- Building/pushing images can interfere with the user's workflow
- The user knows when and how to deploy changes
- Claude should focus on code changes, not deployment

**What to do instead:**
1. Make code changes and run `cargo fmt`, `cargo clippy`, `cargo test`
2. Inform the user that changes are ready
3. Let the user handle building, pushing, and deploying

**Example:**
```
❌ WRONG:
"Let me build the Docker image and restart the controller..."
docker build -t ...
kubectl rollout restart ...

✅ CORRECT:
"I've fixed the issue in src/reconcilers/dnszone.rs and updated .claude/CHANGELOG.md.
All tests pass. The changes are ready for you to build and deploy."
```

---

## 🚨 Critical TODOs

### CRITICAL: Plans and Roadmaps Location
**Status:** ✅ MANDATORY REQUIREMENT
**Impact:** Documentation organization and discoverability

**ALWAYS add plans or roadmaps to `docs/roadmaps/`, NO WHERE ELSE.**

**Why:**
- **Centralized Location**: All planning documents in one discoverable location
- **Documentation Structure**: Maintains consistent docs/ directory organization
- **Version Control**: Plans tracked alongside code changes
- **Easy Reference**: Developers know exactly where to find planning documents

**Naming Convention:**
- **ALWAYS** use **lowercase** filenames (MANDATORY)
- **ALWAYS** use **hyphens** (`-`) to separate words, NEVER underscores (`_`) (MANDATORY)
- Use descriptive names indicating the purpose
- NO uppercase letters anywhere in the filename
- NO underscores anywhere in the filename

**Examples:**
```
✅ CORRECT:
docs/roadmaps/integration-test-plan.md
docs/roadmaps/phase4-implementation.md
docs/roadmaps/feature-roadmap-2025.md
docs/roadmaps/zones-from-label-selector-support.md
docs/roadmaps/refactoring-complete.md
docs/roadmaps/phase-1-2-implementation-plan.md

❌ WRONG:
INTEGRATION_TEST_PLAN.md (root directory, uppercase, underscores)
ROADMAP.md (root directory, uppercase)
planning/test-plan.md (wrong directory)
docs/roadmaps/ZONES_FROM_LABEL_SELECTOR.md (uppercase, underscores)
docs/roadmaps/Phase_3_Analysis.md (uppercase and underscores)
docs/roadmaps/Refactoring_Complete.md (uppercase and underscores)
docs/roadmaps/REFACTORING-COMPLETE.md (uppercase)
```

**CRITICAL RULE**: Before creating ANY file in `docs/roadmaps/`, verify the filename is:
1. All lowercase letters
2. Words separated by hyphens (`-`)
3. No underscores (`_`)
4. No uppercase letters

**What Goes in docs/roadmaps/:**
- Integration test plans
- Feature implementation roadmaps
- Phase/milestone planning documents
- Release planning documents
- Architecture decision records (ADRs) planning
- Migration plans

**REMEMBER:** Before creating any planning document, ALWAYS put it in `docs/roadmaps/`. Never in the root directory or elsewhere.

---

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

### CRITICAL: Always Run cargo fmt and clippy After Code Changes
**Status:** ✅ Required Standard
**Impact:** Code quality, consistency, and CI/CD pipeline success

**MANDATORY REQUIREMENT:** Whenever you add or modify code in tests or source files, run the `cargo-quality` skill before considering the task complete.

> **How:** Run the `cargo-quality` skill.

**Why This Matters:**
- **CI/CD Will Fail**: GitHub Actions will reject commits with formatting violations or clippy warnings
- **Catches Bugs Early**: Clippy identifies common mistakes and non-idiomatic code before they reach production
- **Maintains Consistency**: Ensures uniform code style across the entire project
- **Saves Time**: Prevents wasted CI minutes and failed builds due to formatting issues

**When to Run:**
- ✅ After adding new functions or types
- ✅ After modifying existing code
- ✅ After writing or updating tests
- ✅ Before committing any `.rs` file changes
- ✅ At the end of EVERY task involving Rust code

**What to Do If They Fail:**
- **cargo fmt failures**: Run `cargo fmt` and commit the formatting changes
- **cargo clippy warnings**: Fix ALL warnings - do not ignore them
- **cargo test failures**: Fix the failing tests - the task is NOT complete until all tests pass

**REMEMBER:** A task involving Rust code is **NOT complete** until cargo fmt, cargo clippy, and cargo test all pass successfully.

### High Priority: CRD Code Generation
**Status:** ✅ Implemented
**Impact:** Automated - CRD YAMLs are generated from Rust types

The Rust types in `src/crd.rs` are the **source of truth**. CRD YAML files in `/deploy/crds/` are **auto-generated** from these types.

> **How:** Run the `regen-crds` skill, then the `regen-api-docs` skill (LAST). Use `kubectl replace --force` not `kubectl apply` — the `Bind9Instance` CRD is ~393KB and exceeds the 256KB annotation size limit.

⚠️ **IMPORTANT**: Always run `crddoc` **LAST** after all CRD changes, example updates, and validations are complete. This ensures the API documentation reflects the final, validated state of the CRDs.

See **CRD Development - Rust as Source of Truth** section below for details.

⚠️ **IMPORTANT**: Examples and documentation MUST stay in sync with CRD schemas. After ANY CRD change, you MUST update:
- `/examples/*.yaml` - Ensure all examples can be applied successfully
- `/docs/src/` - Update any documentation that references the CRD fields
- Quickstart guide - Verify all YAML snippets are valid

### CRITICAL: Documentation Examples Must Reference CRDs

**Status:** ✅ Required Standard
**Impact:** Documentation accuracy and user trust

**CRITICAL REQUIREMENTS:**

1. **Before Creating Examples in Documentation:**
   - **ALWAYS** read the relevant CRD YAML files in `/deploy/crds/` OR the Rust types in `src/crd.rs` first
   - **VERIFY** the exact field names and structure from the source of truth
   - **NEVER** guess or assume field names based on convention
   - **NEVER** copy examples from memory or other documentation without verification

2. **When CRDs Change:**
   - **SEARCH** all documentation in `/docs/src/` for examples using the changed CRD
   - **UPDATE** every example to match the new schema
   - **VALIDATE** that all examples can be applied: `kubectl apply --dry-run=client -f examples/`
   - **BUILD** documentation to ensure no broken references: `make docs`

**Why This Matters:**
- Incorrect examples break user trust and cause support burden
- Documentation is often the first touchpoint for users
- Outdated examples lead to failed deployments and frustration
- Schema mismatches are hard to debug for users

**Example of Critical Failure:**
```yaml
# ❌ WRONG - Using spec.config instead of spec.global
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9GlobalCluster
spec:
  config:  # This field doesn't exist!
    recursion: true

# ✅ CORRECT - Verified against CRD
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9GlobalCluster
spec:
  global:  # Correct field name from CRD schema
    recursion: true
```

**Workflow for Documentation Examples:**
1. Identify which CRD the example uses
2. Read `/deploy/crds/<crd-name>.crd.yaml` or `src/crd.rs`
3. Verify the exact field names and types
4. Write the example using the verified schema
5. Validate the example: `kubectl apply --dry-run=client`
6. If CRD changes, search docs for ALL examples: `grep -r "kind: <CRDName>" docs/src/`

**Verification Checklist:**
- [ ] Read CRD schema before writing examples
- [ ] All field names match CRD exactly (case-sensitive)
- [ ] All required fields are present
- [ ] No deprecated or removed fields used
- [ ] Examples validate successfully with kubectl

**REMEMBER:** The CRD schema is the contract. Documentation must honor that contract exactly.

---

## 🔧 GitHub Workflows & CI/CD

### 🚨 CRITICAL: Never Replace `firestoned/github-actions` With Direct Action Calls

**Status:** ✅ Required Standard
**Impact:** Consistency, maintainability, and vendor control

**MANDATORY REQUIREMENT:** ALL GitHub Actions workflows in this repository MUST use composite actions from the `firestoned/github-actions` library. NEVER replace them with direct action calls, even if the underlying action version is outdated.

**Why This Matters:**
- `firestoned/github-actions` is owned and maintained by the user (Erick Bourgeois)
- It provides centralized, opinionated wrappers around third-party actions (e.g., `actions/cache`, `actions/upload-artifact`)
- Inlining direct action calls bypasses organizational control and creates inconsistency
- When an underlying action needs a version bump (e.g., `actions/cache@v4` → `actions/cache@v5`), the fix belongs in the `firestoned/github-actions` repo — NOT inlined here

**Repository:** `firestoned/github-actions` (user owns this repo)

**Correct Fix When an Underlying Action Needs Updating:**
1. Update the action version inside the `firestoned/github-actions` repository
2. Tag a new release of `firestoned/github-actions` (e.g., v1.3.7)
3. Update the version reference in this repo's workflows (`@v1.3.6` → `@v1.3.7`)

**Examples:**
```yaml
# ✅ CORRECT - Use firestoned composite action
- name: Cache cargo dependencies
  uses: firestoned/github-actions/rust/cache-cargo@v1.3.6

# ❌ WRONG - Never inline the direct action call
- name: Cache cargo dependencies
  uses: actions/cache@v5
  with:
    path: ~/.cargo/registry
    key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}
```

**Pattern of Action Families:**
- `firestoned/github-actions/rust/cache-cargo` — Cargo dependency caching
- `firestoned/github-actions/rust/setup-rust-build` — Linux cross-compilation setup
- `firestoned/github-actions/rust/build-binary` — Binary compilation
- `firestoned/github-actions/rust/generate-sbom` — SBOM generation
- `firestoned/github-actions/rust/security-scan` — Cargo audit
- `firestoned/github-actions/docker/setup-docker` — Docker login + buildx
- `firestoned/github-actions/security/license-check` — SPDX header verification
- `firestoned/github-actions/security/verify-signed-commits` — Commit signature verification
- `firestoned/github-actions/security/trivy-scan` — Container vulnerability scan
- `firestoned/github-actions/versioning/extract-version` — Image tag generation

**REMEMBER:** When you see a deprecation warning about an underlying action (e.g., Node.js version), recommend updating `firestoned/github-actions`. Never bypass it.

---

### CRITICAL: All Workflows Must Be Makefile-Driven

**Status:** ✅ Required Standard
**Impact:** Consistency, maintainability, and local reproducibility

All GitHub Actions workflows MUST delegate complex logic to Makefile targets. Workflows should only:
1. Install required tools (Kind, kubectl, Rust, etc.)
2. Set up environment variables
3. Call Makefile targets

**Why:**
- **Local Reproducibility**: Developers can run the exact same commands locally
- **Consistency**: Same logic runs in CI and locally
- **Maintainability**: Business logic lives in one place (Makefile), not scattered across workflows
- **Testability**: Makefile targets can be tested independently
- **Simplicity**: Workflows become declarative configuration, not complex scripts

**Pattern:**

```yaml
# ✅ GOOD - Workflow delegates to Makefile
jobs:
  integration-test:
    steps:
      - name: Install Kind
        uses: helm/kind-action@v1

      - name: Install kubectl
        uses: azure/setup-kubectl@v4

      - name: Run integration tests
        env:
          IMAGE_TAG: ${{ steps.tag.outputs.tag }}
          REGISTRY: ghcr.io
        run: make kind-integration-test-ci

# ❌ BAD - Complex logic in workflow
jobs:
  integration-test:
    steps:
      - name: Create cluster
        run: |
          kind create cluster --config deploy/kind-config.yaml
          kubectl create namespace bindy-system
          kubectl apply -f deploy/crds/
          # ... 50+ lines of bash ...
```

**Requirements:**
- Workflows MUST NOT contain multi-line bash scripts (except simple tool setup)
- **All workflow run commands MUST call Makefile targets** - Never call tools directly (e.g., use `make cargo-deny` not `cargo deny check`)
- All test orchestration MUST be in Makefile targets
- All deployment logic MUST be in Makefile targets
- Makefile targets MUST work identically locally and in CI
- Document Makefile targets with `## comments` for `make help`

**Examples:**
```yaml
# ✅ CORRECT - Use Makefile targets
- name: Run security scans
  run: make cargo-deny

- name: Scan for secrets
  run: make gitleaks

# ❌ WRONG - Direct tool invocation
- name: Run security scans
  run: cargo deny check

- name: Scan for secrets
  run: gitleaks detect --source .
```

**Available Integration Test Targets:**
- `make kind-integration-test` - Run full integration tests with local build
- `make kind-integration-test-ci` - Run integration tests in CI mode (requires IMAGE_TAG env var)

### CRITICAL: Workflows Must Be Reusable and Composable

**Status:** ✅ Required Standard
**Impact:** Maintainability, DRY principles, and workflow consistency

When adding new GitHub Actions workflows, they MUST be designed for reusability and integration with existing workflows.

**Why:**
- **DRY Principle**: Avoid duplicating workflow logic across multiple files
- **Consistency**: Same steps produce same results across different contexts
- **Maintainability**: Update shared logic once, not in every workflow
- **Composability**: Workflows can call other workflows or be called by others
- **Flexibility**: Standalone execution and integration into larger workflows

**Requirements:**

1. **Use Reusable Workflows** (`.github/workflows/*.yml` with `workflow_call`):
   - Define workflows that can be called by other workflows
   - Accept inputs for configuration
   - Define outputs for downstream steps
   - Make them standalone executable (support both `workflow_call` and manual triggers)

2. **Use Composite Actions** (`.github/actions/*/action.yml`):
   - For complex multi-step operations that are used across multiple workflows
   - For shared setup/teardown logic
   - For operations that need to be consistent across workflows

3. **Integration Strategy**:
   - New workflows MUST be callable from existing workflows
   - Existing workflows SHOULD be able to include new workflow steps
   - Avoid creating isolated workflows that duplicate existing logic

**Pattern for Reusable Workflows:**

```yaml
# .github/workflows/integration-test.yml - Reusable workflow
name: Integration Tests

on:
  workflow_call:
    inputs:
      image_tag:
        required: true
        type: string
      registry:
        required: false
        type: string
        default: ghcr.io
    outputs:
      test_result:
        description: "Test execution result"
        value: ${{ jobs.test.outputs.result }}
  workflow_dispatch:
    inputs:
      image_tag:
        required: true
        type: string

jobs:
  test:
    runs-on: ubuntu-latest
    outputs:
      result: ${{ steps.test.outputs.result }}
    steps:
      - uses: actions/checkout@v4
      - name: Run integration tests
        id: test
        env:
          IMAGE_TAG: ${{ inputs.image_tag }}
          REGISTRY: ${{ inputs.registry }}
        run: make kind-integration-test-ci
```

**Pattern for Composite Actions:**

```yaml
# .github/actions/setup-rust/action.yml - Composite action
name: Setup Rust Environment
description: Install Rust toolchain and cache dependencies

inputs:
  toolchain:
    description: 'Rust toolchain version'
    required: false
    default: 'stable'

runs:
  using: composite
  steps:
    - name: Install Rust
      uses: actions-rs/toolchain@v1
      with:
        toolchain: ${{ inputs.toolchain }}

    - name: Cache cargo registry
      uses: actions/cache@v3
      with:
        path: ~/.cargo/registry
        key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}

    - name: Cache cargo build
      uses: actions/cache@v3
      with:
        path: target
        key: ${{ runner.os }}-cargo-build-${{ hashFiles('**/Cargo.lock') }}
```

**Pattern for Calling Reusable Workflows:**

```yaml
# .github/workflows/ci.yml - Main CI workflow that composes others
name: CI

on: [push, pull_request]

jobs:
  build:
    uses: ./.github/workflows/build.yml
    with:
      rust_version: stable

  integration-test:
    needs: build
    uses: ./.github/workflows/integration-test.yml
    with:
      image_tag: ${{ needs.build.outputs.image_tag }}
      registry: ghcr.io

  security-scan:
    needs: build
    uses: ./.github/workflows/security.yml
    with:
      image_tag: ${{ needs.build.outputs.image_tag }}
```

**Checklist for New Workflows:**

Before adding a new workflow, ask:
- [ ] Can this be added as a job to an existing workflow?
- [ ] Can this be made into a reusable workflow that others can call?
- [ ] Does this duplicate logic from an existing workflow?
- [ ] Can this be extracted into a composite action for reuse?
- [ ] Will existing workflows benefit from calling this workflow?
- [ ] Can this workflow be triggered both standalone and as a called workflow?

**Examples of Good Reusability:**

- ✅ Build workflow that can be called by CI, release, and PR workflows
- ✅ Integration test workflow that works locally and in CI
- ✅ Security scan that runs standalone, in CI, and on schedule
- ✅ Composite action for Kind cluster setup used across multiple workflows

**Examples of Poor Design:**

- ❌ Duplicating the same build steps in CI and release workflows
- ❌ Creating a workflow that can only run in one specific context
- ❌ Hardcoding values that should be inputs
- ❌ Creating standalone workflows that can't be composed

**REMEMBER:** Every new workflow should enhance the ecosystem, not create a silo. Design for reuse from day one.

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

### 🚨 CRITICAL: Always Verify if Documentation Needs Updates

**MANDATORY: Before considering ANY task complete, ALWAYS ask yourself: "Does documentation need to be updated?"**

This verification applies to:
- ✅ Code changes (new features, bug fixes, refactoring)
- ✅ CRD modifications (fields added/removed/changed)
- ✅ API changes (function signatures, behavior)
- ✅ Configuration changes (new env vars, settings)
- ✅ Architecture changes (new components, flows)

**If YES to any above:** Documentation MUST be updated according to the checklist below.

---

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
   - **`.claude/CHANGELOG.md`** - Document the change (see format below)
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
- **Versioning**: Date all changes in .claude/CHANGELOG.md

#### Building Documentation

**CRITICAL: Always use the Makefile target — never run `mdbook build` directly.**

> **How:** Run the `build-docs` skill (`make docs`).

#### Validation Checklist

Before considering a task complete, verify:
- [ ] .claude/CHANGELOG.md updated with change details
- [ ] All affected documentation pages updated
- [ ] All YAML examples validate successfully
- [ ] API documentation regenerated (if CRDs changed)
- [ ] Architecture diagrams updated (if structure changed)
- [ ] Code examples compile and run
- [ ] README.md updated (if getting started or features changed)
- [ ] No broken links in documentation
- [ ] Documentation reviewed as if reading for the first time
- [ ] **Documentation builds successfully: `make docs`**

### Mandatory: Update Changelog on Every Code Change

After **ANY** code modification, update `.claude/CHANGELOG.md` with the following format:

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

### CRITICAL: Test-Driven Development (TDD) Workflow

**MANDATORY: ALWAYS write tests FIRST before implementing functionality.**

This project follows strict Test-Driven Development practices. You MUST follow the Red-Green-Refactor cycle for ALL code changes.

> **How:** Follow the `tdd-workflow` skill (RED → GREEN → REFACTOR).

#### TDD Benefits:

- **Design First**: Forces you to think about API and behavior before implementation
- **Complete Coverage**: All code has tests because tests come first
- **Prevents Over-Engineering**: Only write code needed to pass tests
- **Regression Safety**: Refactoring is safe because tests verify behavior
- **Living Documentation**: Tests document expected behavior

#### When to Write Tests First:

- ✅ **New Features**: Write tests defining the feature behavior, then implement
- ✅ **Bug Fixes**: Write a failing test that reproduces the bug, then fix it
- ✅ **Refactoring**: Ensure existing tests pass, add new tests for edge cases
- ✅ **Performance Optimizations**: Write performance tests, then optimize

#### Exceptions to TDD:

TDD is MANDATORY except for:
- Exploratory/prototype code (must be marked as such and removed before merging)
- Simple refactoring that doesn't change behavior (existing tests verify correctness)

**REMEMBER**: If you're writing implementation code before tests, STOP and write tests first!

### After Modifying Any `.rs` File

**CRITICAL: At the end of EVERY task that modifies Rust files, run the `cargo-quality` skill.**

> **How:** Run the `cargo-quality` skill. Fix ALL clippy warnings. Task is NOT complete until all three commands pass.

**CRITICAL: After ANY Rust code modification, you MUST also verify:**

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
   - Ensure `.claude/CHANGELOG.md` reflects the changes
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
- **Use early returns/guard clauses**: Minimize nesting by handling edge cases early and returning

#### Early Return / Guard Clause Pattern

**CRITICAL: Prefer early returns over nested if-else statements.**

The "early return" or "guard clause" coding style emphasizes minimizing nested if-else statements and promoting clearer, more linear code flow. This is achieved by handling error conditions or special cases at the beginning of a function and exiting early if those conditions are met. The remaining code then focuses on the "happy path" or main logic.

**Key Principles:**

1. **Handle preconditions first**: Validate input parameters and other preconditions at the start of a function. If a condition is not met, return immediately (e.g., `return Err(...)`, `return None`, or `return Ok(())`). This prevents the main logic from executing with invalid data.

   ```rust
   // ✅ GOOD - Early return for validation
   pub async fn reconcile_dnszone(
       client: Client,
       dnszone: DNSZone,
       zone_manager: &Bind9Manager,
   ) -> Result<()> {
       // Guard clause: Check if work is needed
       if !needs_reconciliation {
           debug!("Spec unchanged, skipping reconciliation");
           return Ok(());  // Early return - no work needed
       }

       // Main logic continues here (happy path)
       let secondary_ips = find_all_secondary_pod_ips(&client, &namespace, &cluster_ref).await?;
       add_dnszone(client, dnszone, zone_manager).await?;
       Ok(())
   }

   // ❌ BAD - Nested if-else
   pub async fn reconcile_dnszone(
       client: Client,
       dnszone: DNSZone,
       zone_manager: &Bind9Manager,
   ) -> Result<()> {
       if needs_reconciliation {
           let secondary_ips = find_all_secondary_pod_ips(&client, &namespace, &cluster_ref).await?;
           add_dnszone(client, dnszone, zone_manager).await?;
           Ok(())
       } else {
           debug!("Spec unchanged, skipping reconciliation");
           Ok(())
       }
   }
   ```

2. **Minimize else statements**: Instead of using if-else for mutually exclusive conditions, use early returns within if blocks. If a condition is met and an action is performed, return the result. The code after the if block then implicitly handles the "else" case.

   ```rust
   // ✅ GOOD - No else needed
   fn calculate_discount(price: f64, is_premium_member: bool) -> f64 {
       if is_premium_member {
           return price * 0.90;  // Apply 10% discount and return
       }
       // No 'else' needed; non-premium members are handled here
       price * 0.95  // Apply 5% discount
   }

   // ❌ BAD - Unnecessary else
   fn calculate_discount(price: f64, is_premium_member: bool) -> f64 {
       if is_premium_member {
           price * 0.90
       } else {
           price * 0.95
       }
   }
   ```

3. **Prioritize readability and clarity**: The goal is to make the code easier to understand by reducing indentation levels and keeping related logic together. When a reader encounters an early return, they know that specific branch of execution has concluded.

4. **Use `?` for error propagation**: Rust's `?` operator is a form of early return for errors. Use it liberally to keep the happy path unindented.

   ```rust
   // ✅ GOOD - Early error returns with ?
   pub async fn add_dnszone(client: Client, dnszone: DNSZone) -> Result<()> {
       let namespace = dnszone.namespace().ok_or_else(|| anyhow!("No namespace"))?;
       let instances = find_instances(&client, &namespace).await?;

       if instances.is_empty() {
           return Err(anyhow!("No instances found"));
       }

       for instance in instances {
           add_zone_to_instance(&instance).await?;
       }

       Ok(())
   }

   // ❌ BAD - Nested match/if
   pub async fn add_dnszone(client: Client, dnszone: DNSZone) -> Result<()> {
       match dnszone.namespace() {
           Some(namespace) => {
               match find_instances(&client, &namespace).await {
                   Ok(instances) => {
                       if !instances.is_empty() {
                           for instance in instances {
                               add_zone_to_instance(&instance).await?;
                           }
                           Ok(())
                       } else {
                           Err(anyhow!("No instances found"))
                       }
                   }
                   Err(e) => Err(e),
               }
           }
           None => Err(anyhow!("No namespace")),
       }
   }
   ```

**Benefits:**
- **Reduced nesting**: Improves readability and reduces cognitive load
- **Clearer code flow**: The main logic is less cluttered by error handling
- **Easier to test**: Each condition can be tested in isolation
- **Fail-fast approach**: Catches invalid states or inputs early in the execution
- **More maintainable**: Changes to edge cases don't affect the main logic

**When to Use:**
- Input validation at function start
- Checking preconditions before expensive operations
- Handling special cases before the general case
- Error handling in async functions
- State validation in reconciliation loops

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
4. Document why the dependency was added in `.claude/CHANGELOG.md`

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

> **How:** Run the `regen-crds` skill, then `regen-api-docs` skill (LAST).

#### Generated YAML Format:

All generated YAML files include:
- Copyright header: `# Copyright (c) 2025 Erick Bourgeois, firestoned`
- SPDX license identifier: `# SPDX-License-Identifier: MIT`
- Auto-generated warning: `# DO NOT EDIT MANUALLY - Run 'cargo run --bin crdgen' to regenerate`

**Never edit the YAML files directly** - your changes will be overwritten on next generation.

#### Adding New CRDs:

> **How:** Follow the `add-new-crd` skill.

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
    version = "v1beta1",
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

#### Event-Driven Programming (Watch, Not Poll)

**CRITICAL: Kubernetes controllers MUST use event-driven programming, NOT polling.**

Controllers should react to cluster state changes via the Kubernetes watch API, not poll resources on a timer.

**Why Event-Driven:**
- **Efficiency**: Only react when changes occur, not wastefully checking repeatedly
- **Scalability**: Watch API scales to thousands of resources without overwhelming the API server
- **Responsiveness**: Immediate reaction to changes instead of waiting for next poll interval
- **Best Practice**: Aligns with Kubernetes controller design patterns

**✅ CORRECT - Event-Driven with Watch:**
```rust
use kube::runtime::Controller;

// Controller watches resources and reacts to events
Controller::new(api, Config::default())
    .run(reconcile, error_policy, context)
    .for_each(|_| futures::future::ready(()))
    .await;
```

**❌ WRONG - Polling Pattern:**
```rust
// Don't do this - wasteful polling
loop {
    let resources = api.list(&ListParams::default()).await?;
    for resource in resources {
        reconcile(resource).await?;
    }
    tokio::time::sleep(Duration::from_secs(30)).await; // Polling!
}
```

**When Polling is Acceptable:**
- Reconciling with external systems (non-Kubernetes APIs) that don't support webhooks
- Periodic cleanup or maintenance tasks (use `requeue_after` in reconcile result)
- Time-based operations (certificate renewal, lease expiration)

**Event-Driven Best Practices:**
- Use `Controller::new()` from kube-runtime for standard reconciliation loops
- Use `.watches()` to observe related resources (e.g., watch Pods when reconciling Deployments)
- Use `.run()` to start the event-driven reconciliation loop
- Return `Action::requeue(duration)` for periodic checks, not manual polling
- Use informers/reflectors for local caching of cluster state

**General Controller Best Practices:**
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
3. Document rollback procedure in `.claude/CHANGELOG.md`

---

## 🧪 Testing Requirements

### Unit Tests

**MANDATORY: Every public function MUST have corresponding unit tests.**

#### Test File Organization

**CRITICAL: ALWAYS place unit tests in separate `_tests.rs` files, NOT embedded in the source file.**

This is the **required pattern** for this codebase. Do NOT embed tests directly in source files.

**Correct Pattern:** `src/foo.rs` → declare `#[cfg(test)] mod foo_tests;` at the bottom; `src/foo_tests.rs` → `#[cfg(test)] mod tests { use super::super::*; ... }`.

> **See:** `tdd-workflow` skill for the full file pattern and Arrange-Act-Assert examples.

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

> **How:** Run the `cargo-quality` skill. For a specific module: `cargo test --lib <module_path>`. For verbose output: `cargo test -- --nocapture`.

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

docs/
├── roadmaps/                # CRITICAL: All roadmaps and implementation planning docs MUST go here
│   └── *.md                 # Future feature plans, optimization strategies, design proposals
├── adr/                     # Architecture Decision Records
├── src/                     # mdBook documentation source
└── ...
```

**Test File Pattern:**
- Every `foo.rs` has a corresponding `foo_tests.rs`
- Test files are in the same directory as the source file
- Source file declares: `#[cfg(test)] mod foo_tests;`
- Test file contains: `#[cfg(test)] mod tests { ... }`

**Documentation File Pattern:**
- **CRITICAL**: ALL roadmaps and implementation planning documents MUST be stored in `docs/roadmaps/`
- Use descriptive, uppercase filenames with underscores (e.g., `CLUSTER_PROVIDER_RECONCILIATION_OPTIMIZATION.md`)
- Include date, status, and impact in document header
- This is MANDATORY - never store roadmaps or planning docs in the root or other directories

---

## 🚫 Things to Avoid

- **Never** use `unwrap()` in production code - use `?` or explicit error handling
- **Never** hardcode namespaces - make them configurable
- **Never** use `sleep()` for synchronization - use proper k8s watch/informers
- **Never** ignore errors in finalizers - this blocks resource deletion
- **Never** store state outside of Kubernetes - operators must be stateless

---

## 💡 Helpful Commands

See `.claude/SKILL.md` for full step-by-step procedures. Quick one-liners:

```bash
# Run operator locally against current kubeconfig
RUST_LOG=debug cargo run

# Validate all manifests
kubectl apply --dry-run=server -f deploy/
```

Skills for common operations: `regen-crds`, `regen-api-docs`, `validate-examples`, `cargo-quality`, `build-docs`, `get-multiarch-digest`.

---

## 📋 PR/Commit Checklist

**MANDATORY: Run this checklist at the end of EVERY task before considering it complete.**

> **How:** Follow the `pre-commit-checklist` skill in `.claude/SKILL.md` for the full gated checklist.

**A task is NOT complete until the pre-commit-checklist passes.**

**Documentation is NOT optional** - it is a critical requirement equal in importance to the code itself.

---

## 🔗 Project References

- [kube-rs documentation](https://kube.rs/)
- [Kubernetes API conventions](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md)
- [Operator pattern](https://kubernetes.io/docs/concepts/extend-kubernetes/operator/)
- Internal: k0rdent platform docs (check Confluence)
