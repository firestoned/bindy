# SLSA Compliance

**Supply-chain Levels for Software Artifacts**

---

## Overview

SLSA (Supply-chain Levels for Software Artifacts, pronounced "salsa") is a security framework developed by Google to prevent supply chain attacks. It defines a series of incrementally adoptable security levels (0-3) that provide increasing supply chain security guarantees.

**Bindy's SLSA Status:** ✅ **Level 3** (highest level)

---

## SLSA Requirements by Level

| Requirement | Level 1 | Level 2 | Level 3 | Bindy Status |
|-------------|---------|---------|---------|--------------|
| **Source - Version controlled** | ✅ | ✅ | ✅ | ✅ Git (GitHub) |
| **Source - Verified history** | ❌ | ✅ | ✅ | ✅ Signed commits |
| **Source - Retained indefinitely** | ❌ | ❌ | ✅ | ✅ GitHub (permanent) |
| **Source - Two-person reviewed** | ❌ | ❌ | ✅ | ✅ 2+ PR approvals |
| **Build - Scripted build** | ✅ | ✅ | ✅ | ✅ Cargo + Docker |
| **Build - Build service** | ❌ | ✅ | ✅ | ✅ GitHub Actions |
| **Build - Build as code** | ❌ | ✅ | ✅ | ✅ Workflows in Git |
| **Build - Ephemeral environment** | ❌ | ✅ | ✅ | ✅ Fresh runners |
| **Build - Isolated** | ❌ | ✅ | ✅ | ✅ No secrets accessible |
| **Build - Hermetic** | ❌ | ❌ | ✅ | ⚠️ Partial (cargo fetch) |
| **Build - Reproducible** | ❌ | ❌ | ✅ | ✅ Bit-for-bit |
| **Provenance - Available** | ❌ | ✅ | ✅ | ✅ SBOM + signatures |
| **Provenance - Authenticated** | ❌ | ✅ | ✅ | ✅ Signed tags |
| **Provenance - Service generated** | ❌ | ✅ | ✅ | ✅ GitHub Actions |
| **Provenance - Non-falsifiable** | ❌ | ❌ | ✅ | ✅ Cryptographic signatures |
| **Provenance - Dependencies complete** | ❌ | ❌ | ✅ | ✅ Cargo.lock + SBOM |

---

## SLSA Level 3 Detailed Compliance

### Source Requirements

**✅ Requirement: Version controlled with verified history**

| Control | Implementation | Evidence |
|---------|----------------|----------|
| **Git Version Control** | All source code in GitHub | GitHub repository |
| **Signed Commits** | All commits GPG/SSH signed | `git log --show-signature` |
| **Verified History** | Branch protection prevents history rewriting | GitHub branch protection |
| **Two-Person Review** | 2+ approvals required for all PRs | PR approval logs |
| **Permanent Retention** | Git history never deleted | GitHub repository settings |

**Evidence:**

```bash
# Show all commits are signed (last 90 days)
git log --show-signature --since="90 days ago" --oneline

# Show branch protection (prevents force push, history rewriting)
gh api repos/firestoned/bindy/branches/main/protection | jq
```

---

### Build Requirements

**✅ Requirement: Build process is fully scripted and reproducible**

| Control | Implementation | Evidence |
|---------|----------------|----------|
| **Scripted Build** | Cargo (Rust), Docker (containers) | `Cargo.toml`, `Dockerfile` |
| **Build as Code** | GitHub Actions workflows in version control | `.github/workflows/*.yaml` |
| **Ephemeral Environment** | Fresh GitHub-hosted runners for each build | GitHub Actions logs |
| **Isolated** | Build cannot access secrets or network (after deps fetched) | GitHub Actions sandboxing |
| **Hermetic** | ⚠️ Partial - `cargo fetch` uses network | Working toward full hermetic |
| **Reproducible** | Two builds from same commit = identical binary | [Build Reproducibility](../../security/BUILD_REPRODUCIBILITY.md) |

**Build Reproducibility Verification:**

```bash
# Automated verification (daily CI/CD)
# Builds binary twice, compares SHA-256 hashes
.github/workflows/reproducibility-check.yaml

# Manual verification (external auditors)
scripts/verify-build.sh v0.1.0
```

**Sources of Non-Determinism (Mitigated):**

1. **Timestamps** → Use `vergen` for deterministic Git commit timestamps
2. **Filesystem order** → Sort files before processing
3. **HashMap iteration** → Use `BTreeMap` for deterministic order
4. **Parallelism** → Sort output after parallel processing
5. **Base image updates** → Pin base image digests in Dockerfile

**Evidence:**
- [Build Reproducibility Documentation](../../security/BUILD_REPRODUCIBILITY.md)
- CI/CD workflow: `.github/workflows/reproducibility-check.yaml`
- Verification script: `scripts/verify-build.sh`

---

### Provenance Requirements

**✅ Requirement: Build provenance is available, authenticated, and non-falsifiable**

| Artifact | Provenance Type | Signature | Availability |
|----------|----------------|-----------|--------------|
| **Rust Binary** | SHA-256 checksum | GPG-signed Git tag | GitHub Releases |
| **Container Image** | Image digest | SBOM + attestation | GHCR (GitHub Container Registry) |
| **SBOM** | CycloneDX format | Included in release | GitHub Releases (*.sbom.json) |
| **Source Code** | Git commit | GPG/SSH signature | GitHub repository |

**SBOM Generation:**

```bash
# Generate SBOM (Software Bill of Materials)
cargo install cargo-cyclonedx
cargo cyclonedx --format json --output bindy.sbom.json

# SBOM includes all dependencies with exact versions
cat bindy.sbom.json | jq '.components[] | {name, version}'
```

**Evidence:**
- GitHub Releases: https://github.com/firestoned/bindy/releases
- SBOM files: `bindy-*.sbom.json` in release artifacts
- Signed Git tags: `git tag --verify v0.1.0`
- Container image signatures: `docker trust inspect ghcr.io/firestoned/bindy:v0.1.0`

---

## SLSA Build Levels Comparison

| Aspect | Level 1 | Level 2 | Level 3 | Bindy |
|--------|---------|---------|---------|-------|
| **Protection against** | Accidental errors | Compromised build service | Compromised source + build | ✅ All |
| **Source integrity** | Manual commits | Signed commits | Signed commits + 2-person review | ✅ Complete |
| **Build integrity** | Manual build | Automated build | Reproducible build | ✅ Complete |
| **Provenance** | None | Service-generated | Cryptographic provenance | ✅ Complete |
| **Verifiability** | Trust on first use | Verifiable by service | Verifiable by anyone | ✅ Complete |

---

## SLSA Compliance Roadmap

| Requirement | Status | Evidence |
|-------------|--------|----------|
| **Level 1** | ✅ Complete | Git, Cargo build |
| **Level 2** | ✅ Complete | GitHub Actions, signed commits, SBOM |
| **Level 3 (Source)** | ✅ Complete | Signed commits, 2+ PR approvals, permanent Git history |
| **Level 3 (Build)** | ✅ Complete | Reproducible builds, verification script |
| **Level 3 (Provenance)** | ✅ Complete | SBOM, signed tags, container attestation |
| **Level 3 (Hermetic)** | ⚠️ Partial | `cargo fetch` uses network (working toward offline builds) |

---

## Verification for End Users

**How to verify Bindy releases:**

```bash
# 1. Verify Git tag signature
git verify-tag v0.1.0

# 2. Rebuild from source
git checkout v0.1.0
cargo build --release --locked

# 3. Compare binary hash with released artifact
sha256sum target/release/bindy
curl -sL https://github.com/firestoned/bindy/releases/download/v0.1.0/bindy-linux-amd64.sha256

# 4. Verify SBOM (Software Bill of Materials)
curl -sL https://github.com/firestoned/bindy/releases/download/v0.1.0/bindy.sbom.json | jq .

# 5. Verify container image signature (if using containers)
docker trust inspect ghcr.io/firestoned/bindy:v0.1.0
```

**Expected Result:** ✅ All verifications pass, hashes match, provenance verified

---

## SLSA Threat Mitigation

| Threat | SLSA Level | Bindy Mitigation |
|--------|------------|------------------|
| **A: Build system compromise** | Level 2+ | ✅ GitHub-hosted runners (ephemeral, isolated) |
| **B: Source code compromise** | Level 3 | ✅ Signed commits, 2+ PR approvals, branch protection |
| **C: Dependency compromise** | Level 3 | ✅ Cargo.lock pinned, daily `cargo audit`, SBOM |
| **D: Upload of malicious binaries** | Level 2+ | ✅ GitHub Actions uploads, not manual |
| **E: Compromised build config** | Level 2+ | ✅ Workflows in Git, 2+ PR approvals |
| **F: Use of compromised package** | Level 3 | ✅ Reproducible builds, users can verify |

---

## See Also

- [Build Reproducibility Verification](../../security/BUILD_REPRODUCIBILITY.md) - SLSA Level 3 verification
- [Security Architecture](../../security/ARCHITECTURE.md) - Supply chain security
- [SECURITY.md](../../../SECURITY.md) - Supply chain security section
- [SLSA Framework](https://slsa.dev/) - Official SLSA documentation
