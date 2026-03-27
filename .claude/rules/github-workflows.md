# GitHub Workflows & CI/CD Standards

## CRITICAL: Never Replace `firestoned/github-actions` With Direct Action Calls

ALL GitHub Actions workflows MUST use composite actions from the `firestoned/github-actions` library. NEVER replace them with direct action calls, even if the underlying action version is outdated.

**Why:** `firestoned/github-actions` is owned by the user (Erick Bourgeois). When an underlying action needs a version bump, fix it in the `firestoned/github-actions` repo — NOT by inlining here.

**Fix process:**
1. Update action version in the `firestoned/github-actions` repository
2. Tag a new release (e.g., v1.3.7)
3. Update the version reference in this repo's workflows

```yaml
# ✅ CORRECT
- name: Cache cargo dependencies
  uses: firestoned/github-actions/rust/cache-cargo@v1.3.6

# ❌ WRONG
- name: Cache cargo dependencies
  uses: actions/cache@v5
```

**Action families:**
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

---

## CRITICAL: All Workflows Must Be Makefile-Driven

Workflows MUST only: install tools, set env vars, and call Makefile targets. All business logic lives in the Makefile.

```yaml
# ✅ GOOD
- name: Run integration tests
  env:
    IMAGE_TAG: ${{ steps.tag.outputs.tag }}
  run: make kind-integration-test-ci

# ❌ BAD
- name: Create cluster
  run: |
    kind create cluster --config deploy/kind-config.yaml
    kubectl create namespace bindy-system
    # ... 50+ lines of bash ...
```

**Rules:**
- No multi-line bash scripts (except simple tool setup)
- All `run:` commands MUST call Makefile targets (e.g., `make cargo-deny` not `cargo deny check`)
- Makefile targets MUST work identically locally and in CI
- Document targets with `## comments` for `make help`

**Integration test targets:**
- `make kind-integration-test` — Run full integration tests with local build
- `make kind-integration-test-ci` — Run integration tests in CI (requires `IMAGE_TAG` env var)

---

## CRITICAL: Workflows Must Be Reusable and Composable

New workflows MUST support both `workflow_call` (called by other workflows) and standalone triggers.

**Reusable workflow pattern:**
```yaml
on:
  workflow_call:
    inputs:
      image_tag:
        required: true
        type: string
  workflow_dispatch:
    inputs:
      image_tag:
        required: true
        type: string

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: make kind-integration-test-ci
        env:
          IMAGE_TAG: ${{ inputs.image_tag }}
```

**Calling reusable workflows:**
```yaml
jobs:
  build:
    uses: ./.github/workflows/build.yml
  integration-test:
    needs: build
    uses: ./.github/workflows/integration-test.yml
    with:
      image_tag: ${{ needs.build.outputs.image_tag }}
```

**Checklist before adding a new workflow:**
- [ ] Can this be a job in an existing workflow?
- [ ] Is it reusable via `workflow_call`?
- [ ] Does it duplicate existing logic?
- [ ] Can it be a composite action?
