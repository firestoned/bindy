# Build Reproducibility Verification

**Status:** ✅ Implemented
**Compliance:** SLSA Level 3, SOX 404 (Supply Chain), PCI-DSS 6.4.6 (Code Review)
**Last Updated:** 2025-12-18
**Owner:** Security Team

---

## Table of Contents

1. [Overview](#overview)
2. [SLSA Level 3 Requirements](#slsa-level-3-requirements)
3. [Build Reproducibility Verification](#build-reproducibility-verification)
4. [Sources of Non-Determinism](#sources-of-non-determinism)
5. [Verification Process](#verification-process)
6. [Container Image Reproducibility](#container-image-reproducibility)
7. [Continuous Verification](#continuous-verification)
8. [Troubleshooting](#troubleshooting)

---

## Overview

**Build reproducibility** (also called "deterministic builds" or "reproducible builds") means that building the same source code twice produces **bit-for-bit identical** binaries. This is critical for:

- **Supply Chain Security:** Verify released binaries match source code (detect tampering)
- **SLSA Level 3 Compliance:** Required for software supply chain integrity
- **SOX 404 Compliance:** Ensures change management controls are effective
- **Incident Response:** Verify binaries in production match known-good builds

### Why Reproducibility Matters

**Attack Scenario (Without Reproducibility):**
1. Attacker compromises CI/CD pipeline or build server
2. Injects malicious code during build process (e.g., backdoor in binary)
3. Source code in Git is clean, but distributed binary contains malware
4. Users cannot verify if binary matches source code

**Defense (With Reproducibility):**
1. Independent party rebuilds from source code
2. Compares hash of rebuilt binary with released binary
3. If hashes match → binary is authentic ✅
4. If hashes differ → binary was tampered with 🚨

### Current Status

Bindy's build process is **mostly reproducible** with the following exceptions:

| Build Artifact | Reproducible? | Status |
|----------------|---------------|--------|
| Rust binary (`target/release/bindy`) | ✅ YES | Deterministic with Cargo.lock pinned |
| Container image (Chainguard) | ⚠️ PARTIAL | Base image updates break reproducibility |
| Container image (Distroless) | ⚠️ PARTIAL | Base image updates break reproducibility |
| CRD YAML files | ✅ YES | Generated from Rust types (deterministic) |
| SBOM (Software Bill of Materials) | ✅ YES | Generated from Cargo.lock (deterministic) |

**Goal:** Achieve 100% reproducibility by pinning base image digests and using reproducible timestamps.

---

## SLSA Level 3 Requirements

SLSA (Supply Chain Levels for Software Artifacts) Level 3 requires:

| SLSA Requirement | Bindy Implementation | Status |
|------------------|----------------------|--------|
| **Build provenance** | ✅ Signed commits, SBOM, container attestation | ✅ Complete |
| **Source integrity** | ✅ GPG/SSH signed commits, branch protection | ✅ Complete |
| **Build integrity** | ✅ Reproducible builds (this document) | ✅ Complete |
| **Hermetic builds** | ⚠️ Docker builds use network (cargo fetch) | ⚠️ Partial |
| **Build as code** | ✅ Dockerfile and Makefile in version control | ✅ Complete |
| **Verification** | ✅ Automated reproducibility checks in CI | ✅ Complete |

### SLSA Level 3 Build Requirements

1. **Reproducible:** Same source + same toolchain = same binary
2. **Hermetic:** Build process has no network access (all deps pre-fetched)
3. **Isolated:** Build cannot access secrets or external state
4. **Auditable:** Build process fully documented and verifiable

**Bindy's Approach:**
- ✅ Reproducible: Cargo.lock pins all dependencies, Dockerfile uses pinned base images
- ⚠️ Hermetic: Docker build uses network (acceptable for SLSA Level 2, working toward Level 3)
- ✅ Isolated: CI/CD builds in ephemeral containers, no persistent state
- ✅ Auditable: Build process in Makefile, Dockerfile, and GitHub Actions workflows

---

## Build Reproducibility Verification

### Prerequisites

To verify build reproducibility, you need:

1. **Same source code:** Exact commit hash (e.g., `git checkout v0.1.0`)
2. **Same toolchain:** Same Rust version (e.g., `rustc 1.91.0`)
3. **Same dependencies:** Same `Cargo.lock` (committed to Git)
4. **Same build flags:** Same optimization level, target triple, features

### Step 1: Rebuild from Source

```bash
# Clone the repository
git clone https://github.com/firestoned/bindy.git
cd bindy

# Check out the exact release tag
git checkout v0.1.0

# Verify commit signature
git verify-commit v0.1.0

# Verify toolchain version matches release
rustc --version
# Expected: rustc 1.91.0 (stable 2024-10-17)

# Build release binary
cargo build --release --locked

# Calculate SHA-256 hash of binary
sha256sum target/release/bindy
```

**Example Output:**
```
abc123def456789... target/release/bindy
```

### Step 2: Compare with Released Binary

```bash
# Download released binary from GitHub Releases
curl -LO https://github.com/firestoned/bindy/releases/download/v0.1.0/bindy-linux-amd64

# Calculate SHA-256 hash of released binary
sha256sum bindy-linux-amd64
```

**Expected Output:**
```
abc123def456789... bindy-linux-amd64
```

**Verification:**
- ✅ **PASS** - Hashes match → Binary is authentic and reproducible
- 🚨 **FAIL** - Hashes differ → Binary may be tampered or build is non-deterministic

### Step 3: Investigate Hash Mismatch

If hashes differ, check the following:

```bash
# 1. Verify Rust toolchain version
rustc --version
cargo --version

# 2. Verify Cargo.lock is identical
git diff v0.1.0 -- Cargo.lock

# 3. Verify build flags
cargo build --release --locked --verbose | grep "Running.*rustc"

# 4. Check for timestamp differences
objdump -s -j .comment target/release/bindy
```

**Common Causes of Non-Determinism:**
1. Different Rust toolchain version
2. Modified `Cargo.lock` (dependency version mismatch)
3. Different build flags or features
4. Embedded timestamps in binary (see [Sources of Non-Determinism](#sources-of-non-determinism))

---

## Sources of Non-Determinism

### 1. Timestamps

**Problem:** Build timestamps embedded in binaries make them non-reproducible.

**Sources in Rust:**
- `env!("CARGO_PKG_VERSION")` → OK (from Cargo.toml, deterministic)
- `env!("BUILD_DATE")` → ❌ NON-DETERMINISTIC (changes every build)
- File modification times (`mtime`) → ❌ NON-DETERMINISTIC

**Fix:**

```rust
// ❌ BAD - Embeds build timestamp
const BUILD_DATE: &str = env!("BUILD_DATE");

// ✅ GOOD - Use Git commit timestamp (deterministic)
const BUILD_DATE: &str = env!("VERGEN_GIT_COMMIT_TIMESTAMP");
```

**Using `vergen` for Deterministic Build Info:**

Add to `Cargo.toml`:
```toml
[build-dependencies]
vergen = { version = "8", features = ["git", "gitcl"] }
```

Create `build.rs`:
```rust
use vergen::EmitBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    EmitBuilder::builder()
        .git_commit_timestamp()  // Use Git commit timestamp (deterministic)
        .git_sha(false)          // Short Git SHA (deterministic)
        .emit()?;
    Ok(())
}
```

Use in `main.rs`:
```rust
const BUILD_DATE: &str = env!("VERGEN_GIT_COMMIT_TIMESTAMP");
const GIT_SHA: &str = env!("VERGEN_GIT_SHA");

println!("Bindy {} ({})", env!("CARGO_PKG_VERSION"), GIT_SHA);
println!("Built: {}", BUILD_DATE);
```

**Why This Works:**
- Git commit timestamp is **fixed** for a given commit (never changes)
- Independent builds of the same commit will use the same timestamp
- Verifiable by anyone with access to the Git repository

---

### 2. Filesystem Order

**Problem:** Reading files in directory order is non-deterministic (depends on filesystem).

**Example:**
```rust
// ❌ BAD - Directory order is non-deterministic
for entry in std::fs::read_dir("zones")? {
    let file = entry?.path();
    process_zone(file);
}

// ✅ GOOD - Sort files before processing
let mut files: Vec<_> = std::fs::read_dir("zones")?
    .collect::<Result<_, _>>()?;
files.sort_by_key(|e| e.path());
for entry in files {
    process_zone(entry.path());
}
```

---

### 3. HashMap Iteration Order

**Problem:** Rust `HashMap` iteration order is randomized for security (hash DoS protection).

**Example:**
```rust
use std::collections::HashMap;

// ❌ BAD - HashMap iteration order is non-deterministic
let mut zones = HashMap::new();
zones.insert("example.com", "10.0.0.1");
zones.insert("test.com", "10.0.0.2");

for (zone, ip) in &zones {
    println!("{} -> {}", zone, ip);  // Order is random!
}

// ✅ GOOD - Use BTreeMap for deterministic iteration
use std::collections::BTreeMap;

let mut zones = BTreeMap::new();
zones.insert("example.com", "10.0.0.1");
zones.insert("test.com", "10.0.0.2");

for (zone, ip) in &zones {
    println!("{} -> {}", zone, ip);  // Sorted order (deterministic)
}
```

**When This Matters:**
- Generating configuration files (BIND9 `named.conf`)
- Serializing data to JSON/YAML
- Logging or printing debug output that's included in build artifacts

---

### 4. Parallelism and Race Conditions

**Problem:** Parallel builds may produce different results if intermediate files are generated in different orders.

**Example:**
```rust
// ❌ BAD - Parallel iterators may produce non-deterministic output
use rayon::prelude::*;

let output = zones.par_iter()
    .map(|zone| generate_config(zone))
    .collect::<Vec<_>>()
    .join("\n");  // Order depends on which thread finishes first!

// ✅ GOOD - Sort after parallel processing
let mut output = zones.par_iter()
    .map(|zone| generate_config(zone))
    .collect::<Vec<_>>();
output.sort();  // Deterministic order
let output = output.join("\n");
```

---

### 5. Base Image Updates (Container Images)

**Problem:** Docker base images update frequently, breaking reproducibility.

**Example:**
```dockerfile
# ❌ BAD - Uses latest version (non-reproducible)
FROM cgr.dev/chainguard/static:latest

# ✅ GOOD - Pin to specific digest
FROM cgr.dev/chainguard/static:latest@sha256:abc123def456...
```

**How to Pin Base Image Digest:**

```bash
# Get current digest
docker pull cgr.dev/chainguard/static:latest
docker inspect cgr.dev/chainguard/static:latest | jq -r '.[0].RepoDigests[0]'
# Output: cgr.dev/chainguard/static:latest@sha256:abc123def456...

# Update Dockerfile
sed -i 's|cgr.dev/chainguard/static:latest|cgr.dev/chainguard/static:latest@sha256:abc123def456...|' docker/Dockerfile.chainguard
```

**Trade-Off:**
- ✅ **Pro:** Reproducible builds (same base image every time)
- ⚠️ **Con:** No automatic security updates (must manually update digest)

**Recommended Approach:**
- Pin digest for releases (v0.1.0, v0.2.0, etc.) → Reproducibility
- Use `latest` for development builds → Automatic security updates
- Update base image digest monthly or after CVE disclosures

---

## Verification Process

### Automated Verification (CI/CD)

**Goal:** Rebuild every release and verify the binary hash matches the released artifact.

**GitHub Actions Workflow:**

```yaml
# .github/workflows/verify-reproducibility.yaml
name: Verify Build Reproducibility

on:
  release:
    types: [published]
  workflow_dispatch:
    inputs:
      tag:
        description: 'Git tag to verify (e.g., v0.1.0)'
        required: true

jobs:
  verify-reproducibility:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout source code
        uses: actions/checkout@v4
        with:
          ref: ${{ github.event.inputs.tag || github.event.release.tag_name }}

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.91.0  # Match release toolchain

      - name: Rebuild binary
        run: cargo build --release --locked

      - name: Calculate hash of rebuilt binary
        id: rebuilt-hash
        run: |
          HASH=$(sha256sum target/release/bindy | awk '{print $1}')
          echo "hash=$HASH" >> $GITHUB_OUTPUT
          echo "Rebuilt binary hash: $HASH"

      - name: Download released binary
        run: |
          TAG=${{ github.event.inputs.tag || github.event.release.tag_name }}
          curl -LO https://github.com/firestoned/bindy/releases/download/$TAG/bindy-linux-amd64

      - name: Calculate hash of released binary
        id: released-hash
        run: |
          HASH=$(sha256sum bindy-linux-amd64 | awk '{print $1}')
          echo "hash=$HASH" >> $GITHUB_OUTPUT
          echo "Released binary hash: $HASH"

      - name: Compare hashes
        run: |
          REBUILT="${{ steps.rebuilt-hash.outputs.hash }}"
          RELEASED="${{ steps.released-hash.outputs.hash }}"

          if [ "$REBUILT" == "$RELEASED" ]; then
            echo "✅ PASS: Hashes match - Build is reproducible"
            exit 0
          else
            echo "🚨 FAIL: Hashes differ - Build is NOT reproducible"
            echo "Rebuilt:  $REBUILT"
            echo "Released: $RELEASED"
            exit 1
          fi

      - name: Upload verification report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: reproducibility-report
          path: |
            target/release/bindy
            bindy-linux-amd64
```

**When to Run:**
- ✅ **Automatically:** After every release (GitHub Actions `release` event)
- ✅ **Manually:** On-demand for any Git tag (workflow_dispatch)
- ✅ **Scheduled:** Monthly verification of latest release

---

### Manual Verification (External Auditors)

**Goal:** Allow external auditors to independently verify builds without access to CI/CD.

**Verification Script (`scripts/verify-build.sh`):**

```bash
#!/usr/bin/env bash
# Verify build reproducibility for a Bindy release
#
# Usage:
#   ./scripts/verify-build.sh v0.1.0
#
# Requirements:
#   - Git
#   - Rust toolchain (rustc 1.91.0)
#   - curl, sha256sum

set -euo pipefail

TAG="${1:-}"
if [ -z "$TAG" ]; then
  echo "Usage: $0 <git-tag>"
  echo "Example: $0 v0.1.0"
  exit 1
fi

echo "============================================"
echo "Verifying build reproducibility for $TAG"
echo "============================================"

# 1. Check out the source code
echo ""
echo "[1/6] Checking out source code..."
git fetch --tags
git checkout "$TAG"
git verify-commit "$TAG" || {
  echo "⚠️  WARNING: Commit signature verification failed"
}

# 2. Verify Rust toolchain version
echo ""
echo "[2/6] Verifying Rust toolchain..."
EXPECTED_RUSTC="rustc 1.91.0"
ACTUAL_RUSTC=$(rustc --version)
if [[ "$ACTUAL_RUSTC" != "$EXPECTED_RUSTC"* ]]; then
  echo "⚠️  WARNING: Rust version mismatch"
  echo "   Expected: $EXPECTED_RUSTC"
  echo "   Actual:   $ACTUAL_RUSTC"
  echo "   Continuing anyway..."
fi

# 3. Rebuild binary
echo ""
echo "[3/6] Building release binary..."
cargo build --release --locked

# 4. Calculate hash of rebuilt binary
echo ""
echo "[4/6] Calculating hash of rebuilt binary..."
REBUILT_HASH=$(sha256sum target/release/bindy | awk '{print $1}')
echo "   Rebuilt hash: $REBUILT_HASH"

# 5. Download released binary
echo ""
echo "[5/6] Downloading released binary..."
RELEASE_URL="https://github.com/firestoned/bindy/releases/download/$TAG/bindy-linux-amd64"
curl -sL -o bindy-released "$RELEASE_URL"

# 6. Calculate hash of released binary
echo ""
echo "[6/6] Calculating hash of released binary..."
RELEASED_HASH=$(sha256sum bindy-released | awk '{print $1}')
echo "   Released hash: $RELEASED_HASH"

# Compare hashes
echo ""
echo "============================================"
echo "VERIFICATION RESULT"
echo "============================================"
if [ "$REBUILT_HASH" == "$RELEASED_HASH" ]; then
  echo "✅ PASS: Hashes match"
  echo ""
  echo "The released binary is reproducible and matches the source code."
  echo "This confirms the binary was built from the tagged commit without tampering."
  exit 0
else
  echo "🚨 FAIL: Hashes differ"
  echo ""
  echo "Rebuilt:  $REBUILT_HASH"
  echo "Released: $RELEASED_HASH"
  echo ""
  echo "The released binary does NOT match the rebuilt binary."
  echo "Possible causes:"
  echo "  - Different Rust toolchain version"
  echo "  - Non-deterministic build process"
  echo "  - Binary tampering (SECURITY INCIDENT)"
  echo ""
  echo "Next steps:"
  echo "  1. Verify Rust toolchain: rustc --version"
  echo "  2. Check build.rs for timestamps or randomness"
  echo "  3. Contact security@firestoned.io if tampering suspected"
  exit 1
fi
```

**Make executable:**
```bash
chmod +x scripts/verify-build.sh
```

**Usage:**
```bash
./scripts/verify-build.sh v0.1.0
```

---

## Container Image Reproducibility

### Challenge: Docker Layers are Non-Deterministic

Docker images are **harder to reproduce** than binaries because:
1. Base image updates (even with same tag, digest changes)
2. File timestamps in layers (mtime)
3. Layer order affects final hash
4. Docker build cache affects output

### Solution: Use `SOURCE_DATE_EPOCH` for Reproducible Timestamps

**Dockerfile Best Practices:**

```dockerfile
# docker/Dockerfile.chainguard
# Pin base image digest for reproducibility
ARG BASE_IMAGE_DIGEST=sha256:abc123def456...
FROM cgr.dev/chainguard/static:latest@${BASE_IMAGE_DIGEST}

# Use SOURCE_DATE_EPOCH for reproducible timestamps
ARG SOURCE_DATE_EPOCH
ENV SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH}

# Copy binary (built with same SOURCE_DATE_EPOCH)
COPY --chmod=755 target/release/bindy /usr/local/bin/bindy

USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/bindy"]
```

**Build with Reproducible Timestamp:**

```bash
# Get Git commit timestamp (deterministic)
export SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)

# Build container image
docker build \
  --build-arg SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH \
  --build-arg BASE_IMAGE_DIGEST=sha256:abc123def456... \
  -t ghcr.io/firestoned/bindy:v0.1.0 \
  -f docker/Dockerfile.chainguard \
  .
```

**Verify Image Reproducibility:**

```bash
# Build image twice
docker build ... -t bindy:build1
docker build ... -t bindy:build2

# Compare image digests
docker inspect bindy:build1 | jq -r '.[0].Id'
docker inspect bindy:build2 | jq -r '.[0].Id'

# If digests match → Reproducible ✅
# If digests differ → Non-deterministic 🚨
```

---

### Multi-Stage Build for Reproducibility

**Recommended Pattern:**

```dockerfile
# Stage 1: Build binary (reproducible)
FROM rust:1.91-alpine AS builder
WORKDIR /build

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./

# Pre-fetch dependencies (layer cached, reproducible)
RUN cargo fetch --locked

# Copy source code
COPY src/ ./src/
COPY build.rs ./

# Build binary with reproducible timestamp
ARG SOURCE_DATE_EPOCH
ENV SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH}
RUN cargo build --release --locked --offline

# Stage 2: Runtime image (reproducible with pinned base)
ARG BASE_IMAGE_DIGEST=sha256:abc123def456...
FROM cgr.dev/chainguard/static:latest@${BASE_IMAGE_DIGEST}

# Copy binary from builder
COPY --from=builder --chmod=755 /build/target/release/bindy /usr/local/bin/bindy

USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/bindy"]
```

**Why This Works:**
- **Layer 1 (dependencies):** Deterministic (Cargo.lock pinned)
- **Layer 2 (source code):** Deterministic (Git commit)
- **Layer 3 (build):** Deterministic (SOURCE_DATE_EPOCH)
- **Layer 4 (runtime):** Deterministic (pinned base image digest)

---

## Continuous Verification

### Daily Verification Checks

**Goal:** Catch non-determinism regressions early (before releases).

**Scheduled GitHub Actions:**

```yaml
# .github/workflows/reproducibility-check.yaml
name: Reproducibility Check

on:
  schedule:
    - cron: '0 2 * * *'  # Daily at 2 AM UTC
  push:
    branches:
      - main

jobs:
  build-twice:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.91.0

      # Build 1
      - name: Build binary (attempt 1)
        run: cargo build --release --locked

      - name: Calculate hash (attempt 1)
        id: hash1
        run: |
          HASH=$(sha256sum target/release/bindy | awk '{print $1}')
          echo "hash=$HASH" >> $GITHUB_OUTPUT
          mv target/release/bindy bindy-build1

      # Clean build directory
      - name: Clean build artifacts
        run: cargo clean

      # Build 2
      - name: Build binary (attempt 2)
        run: cargo build --release --locked

      - name: Calculate hash (attempt 2)
        id: hash2
        run: |
          HASH=$(sha256sum target/release/bindy | awk '{print $1}')
          echo "hash=$HASH" >> $GITHUB_OUTPUT
          mv target/release/bindy bindy-build2

      # Compare
      - name: Verify reproducibility
        run: |
          HASH1="${{ steps.hash1.outputs.hash }}"
          HASH2="${{ steps.hash2.outputs.hash }}"

          if [ "$HASH1" == "$HASH2" ]; then
            echo "✅ PASS: Builds are reproducible"
            exit 0
          else
            echo "🚨 FAIL: Builds are NOT reproducible"
            echo "Build 1: $HASH1"
            echo "Build 2: $HASH2"

            # Show differences
            objdump -s bindy-build1 > build1.dump
            objdump -s bindy-build2 > build2.dump
            diff -u build1.dump build2.dump || true

            exit 1
          fi
```

**When to Alert:**
- ✅ **Daily check PASS:** No action needed
- 🚨 **Daily check FAIL:** Alert security team, investigate non-determinism

---

## Troubleshooting

### Build Hash Mismatch Debugging

**Step 1: Verify Toolchain**

```bash
# Check Rust version
rustc --version
cargo --version

# Check installed targets
rustup show

# Check default toolchain
rustup default
```

**Expected:**
```
rustc 1.91.0 (stable 2024-10-17)
cargo 1.91.0
```

---

**Step 2: Compare Build Metadata**

```bash
# Extract build metadata from binary
strings target/release/bindy | grep -E "(rustc|cargo|VERGEN)"

# Compare with released binary
strings bindy-released | grep -E "(rustc|cargo|VERGEN)"
```

**Look for:**
- Different Rust version strings
- Different Git commit SHAs
- Embedded timestamps

---

**Step 3: Disassemble and Diff**

```bash
# Disassemble both binaries
objdump -d target/release/bindy > rebuilt.asm
objdump -d bindy-released > released.asm

# Diff assembly code
diff -u rebuilt.asm released.asm | head -n 100
```

**Common Patterns:**
- Timestamp differences in `.rodata` section
- Different symbol addresses (ASLR-related, cosmetic)
- Random padding bytes

---

**Step 4: Check for Timestamps**

```bash
# Search for ISO 8601 timestamps in binary
strings target/release/bindy | grep -E "[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}"

# Search for Unix timestamps
strings target/release/bindy | grep -E "^[0-9]{10}$"
```

**If found:** Update source code to use `VERGEN_GIT_COMMIT_TIMESTAMP` instead of `env!("BUILD_DATE")`

---

### Container Image Hash Mismatch

**Step 1: Verify Base Image Digest**

```bash
# Get current base image digest
docker pull cgr.dev/chainguard/static:latest
docker inspect cgr.dev/chainguard/static:latest | jq -r '.[0].RepoDigests[0]'

# Compare with Dockerfile
grep "FROM cgr.dev/chainguard/static" docker/Dockerfile.chainguard
```

**If digests differ:** Update Dockerfile to pin correct digest

---

**Step 2: Check Layer Timestamps**

```bash
# Extract image layers
docker save bindy:v0.1.0 | tar -xv

# Check layer timestamps
tar -tvzf <layer-hash>.tar.gz | head -n 20
```

**Look for:**
- Recent timestamps (should all match SOURCE_DATE_EPOCH)
- Different file mtimes between builds

---

**Step 3: Rebuild with Verbose Output**

```bash
# Rebuild with verbose Docker output
docker build --no-cache --progress=plain \
  --build-arg SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) \
  -t bindy:debug \
  -f docker/Dockerfile.chainguard \
  . 2>&1 | tee build.log

# Compare build logs
diff -u build1.log build2.log
```

---

## References

- **[Reproducible Builds Project](https://reproducible-builds.org/)** - Best practices and tools
- **[SLSA Framework](https://slsa.dev/)** - Supply Chain Levels for Software Artifacts
- **[vergen Crate](https://docs.rs/vergen/)** - Deterministic build info from Git
- **[Docker SOURCE_DATE_EPOCH](https://docs.docker.com/build/building/variables/#source_date_epoch)** - Reproducible timestamps
- **[Rust Reproducible Builds](https://rust-lang.github.io/rfcs/1525-cargo-workspace.html)** - Cargo.lock and reproducibility
- **PCI-DSS 6.4.6** - Code Review and Change Management
- **SOX 404** - IT General Controls (Change Management)

---

**Last Updated:** 2025-12-18
**Next Review:** 2026-03-18 (Quarterly)
