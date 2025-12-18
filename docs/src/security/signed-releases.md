# Signed Releases

Bindy releases are cryptographically signed using [Cosign](https://github.com/sigstore/cosign) with keyless signing (Sigstore). This ensures:

- **Authenticity**: Verify that releases come from the official Bindy GitHub repository
- **Integrity**: Detect any tampering with release artifacts
- **Non-repudiation**: Cryptographic proof that artifacts were built by official CI/CD
- **Transparency**: All signatures are recorded in the Sigstore transparency log (Rekor)

## What Is Signed

Every Bindy release includes signed artifacts:

1. **Container Images**:
   - `ghcr.io/firestoned/bindy:*` (Chainguard base)
   - `ghcr.io/firestoned/bindy-distroless:*` (Google Distroless base)

2. **Binary Tarballs**:
   - `bindy-linux-amd64.tar.gz`
   - `bindy-linux-arm64.tar.gz`

3. **Signature Artifacts** (uploaded to releases):
   - `*.tar.gz.bundle` - Cosign signature bundles for binaries
   - Container signatures are stored in the OCI registry

## Installing Cosign

To verify signatures, install [Cosign](https://docs.sigstore.dev/cosign/installation):

```bash
# macOS
brew install cosign

# Linux (download binary)
LATEST_VERSION=$(curl -s https://api.github.com/repos/sigstore/cosign/releases/latest | grep tag_name | cut -d '"' -f 4)
curl -Lo cosign https://github.com/sigstore/cosign/releases/download/${LATEST_VERSION}/cosign-linux-amd64
chmod +x cosign
sudo mv cosign /usr/local/bin/

# Verify installation
cosign version
```

## Verifying Container Images

Cosign uses **keyless signing** with Sigstore, which means:
- No private keys to manage or distribute
- Signatures are verified against the GitHub Actions OIDC identity
- All signatures are logged in the public Rekor transparency log

### Quick Verification

```bash
# Verify the latest Chainguard image
cosign verify \
  --certificate-identity-regexp='https://github.com/firestoned/bindy' \
  --certificate-oidc-issuer='https://token.actions.githubusercontent.com' \
  ghcr.io/firestoned/bindy:latest

# Verify a specific version
cosign verify \
  --certificate-identity-regexp='https://github.com/firestoned/bindy' \
  --certificate-oidc-issuer='https://token.actions.githubusercontent.com' \
  ghcr.io/firestoned/bindy:v0.1.0

# Verify the Distroless variant
cosign verify \
  --certificate-identity-regexp='https://github.com/firestoned/bindy' \
  --certificate-oidc-issuer='https://token.actions.githubusercontent.com' \
  ghcr.io/firestoned/bindy-distroless:latest
```

### Understanding the Verification Output

When verification succeeds, Cosign returns JSON output with signature details:

```json
[
  {
    "critical": {
      "identity": {
        "docker-reference": "ghcr.io/firestoned/bindy"
      },
      "image": {
        "docker-manifest-digest": "sha256:abcd1234..."
      },
      "type": "cosign container image signature"
    },
    "optional": {
      "Bundle": {
        "SignedEntryTimestamp": "...",
        "Payload": {
          "body": "...",
          "integratedTime": 1234567890,
          "logIndex": 12345678,
          "logID": "..."
        }
      },
      "Issuer": "https://token.actions.githubusercontent.com",
      "Subject": "https://github.com/firestoned/bindy/.github/workflows/release.yaml@refs/tags/v0.1.0"
    }
  }
]
```

Key fields to verify:
- **Subject**: Shows the exact GitHub workflow that created the signature
- **Issuer**: Confirms it came from GitHub Actions
- **integratedTime**: Unix timestamp when signature was created
- **logIndex**: Entry in the Rekor transparency log (publicly auditable)

### Verification Failures

If verification fails, you'll see an error like:

```
Error: no matching signatures:
```

**Do NOT use unverified images in production.** This indicates:
- The image was not signed by the official Bindy release workflow
- The image may have been tampered with
- The image may be a counterfeit

## Verifying Binary Releases

Binary tarballs are signed with Cosign blob signing. Each release includes `.bundle` files containing the signature.

### Download and Verify

```bash
# Download the binary tarball and signature bundle from GitHub Releases
VERSION="v0.1.0"
PLATFORM="linux-amd64"  # or linux-arm64

# Download tarball
curl -LO "https://github.com/firestoned/bindy/releases/download/${VERSION}/bindy-${PLATFORM}.tar.gz"

# Download signature bundle
curl -LO "https://github.com/firestoned/bindy/releases/download/${VERSION}/bindy-${PLATFORM}.tar.gz.bundle"

# Verify the signature
cosign verify-blob \
  --bundle "bindy-${PLATFORM}.tar.gz.bundle" \
  --certificate-identity-regexp='https://github.com/firestoned/bindy' \
  --certificate-oidc-issuer='https://token.actions.githubusercontent.com' \
  "bindy-${PLATFORM}.tar.gz"
```

### Verification Success

If successful, you'll see:

```
Verified OK
```

You can now safely extract and use the binary:

```bash
tar xzf bindy-${PLATFORM}.tar.gz
./bindy --version
```

### Automated Verification Script

Create a script to download and verify releases automatically:

```bash
#!/bin/bash
set -euo pipefail

VERSION="${1:-latest}"
PLATFORM="${2:-linux-amd64}"

if [ "$VERSION" = "latest" ]; then
  VERSION=$(curl -s https://api.github.com/repos/firestoned/bindy/releases/latest | grep tag_name | cut -d '"' -f 4)
fi

echo "Downloading Bindy $VERSION for $PLATFORM..."

# Download artifacts
curl -LO "https://github.com/firestoned/bindy/releases/download/${VERSION}/bindy-${PLATFORM}.tar.gz"
curl -LO "https://github.com/firestoned/bindy/releases/download/${VERSION}/bindy-${PLATFORM}.tar.gz.bundle"

# Verify signature
echo "Verifying signature..."
cosign verify-blob \
  --bundle "bindy-${PLATFORM}.tar.gz.bundle" \
  --certificate-identity-regexp='https://github.com/firestoned/bindy' \
  --certificate-oidc-issuer='https://token.actions.githubusercontent.com' \
  "bindy-${PLATFORM}.tar.gz"

# Extract
echo "Extracting..."
tar xzf "bindy-${PLATFORM}.tar.gz"

echo "✓ Bindy $VERSION successfully verified and installed"
./bindy --version
```

## Additional Security Verification

### Check SHA256 Checksums

Every release includes a `checksums.sha256` file with SHA256 hashes of all artifacts:

```bash
# Download checksums
curl -LO "https://github.com/firestoned/bindy/releases/download/${VERSION}/checksums.sha256"

# Verify the tarball checksum
sha256sum -c checksums.sha256 --ignore-missing
```

### Inspect Rekor Transparency Log

All signatures are recorded in the public [Rekor transparency log](https://search.sigstore.dev/):

```bash
# Search for Bindy signatures
rekor-cli search --email noreply@github.com --rekor_server https://rekor.sigstore.dev

# Or use the web interface:
# https://search.sigstore.dev/?email=noreply@github.com
```

### Verify SLSA Provenance

Bindy releases also include [SLSA provenance](https://slsa.dev/) attestations:

```bash
# Verify SLSA provenance for the container image
cosign verify-attestation \
  --type slsaprovenance \
  --certificate-identity-regexp='https://github.com/firestoned/bindy' \
  --certificate-oidc-issuer='https://token.actions.githubusercontent.com' \
  ghcr.io/firestoned/bindy:${VERSION}
```

## Kubernetes Deployment Verification

When deploying to Kubernetes, use [policy-controller](https://docs.sigstore.dev/policy-controller/overview) or [Kyverno](https://kyverno.io/) to enforce signature verification:

### Kyverno Policy Example

```yaml
apiVersion: kyverno.io/v1
kind: ClusterPolicy
metadata:
  name: verify-bindy-images
spec:
  validationFailureAction: enforce
  background: false
  rules:
    - name: verify-bindy-signature
      match:
        any:
          - resources:
              kinds:
                - Pod
      verifyImages:
        - imageReferences:
            - "ghcr.io/firestoned/bindy*"
          attestors:
            - entries:
                - keyless:
                    subject: "https://github.com/firestoned/bindy/.github/workflows/release.yaml@*"
                    issuer: "https://token.actions.githubusercontent.com"
                    rekor:
                      url: https://rekor.sigstore.dev
```

This policy ensures:
- Only signed Bindy images can run in the cluster
- Signatures must come from the official release workflow
- Signatures are verified against the Rekor transparency log

## Troubleshooting

### "Error: no matching signatures"

**Cause**: Image/artifact is not signed or signature doesn't match the identity.

**Solution**:
- Verify you're using an official release from `ghcr.io/firestoned/bindy*`
- Check the tag/version exists on the GitHub releases page
- Ensure you're not using a locally-built image

### "Error: unable to verify bundle"

**Cause**: Signature bundle is corrupted or doesn't match the artifact.

**Solution**:
- Re-download the artifact and bundle
- Verify the SHA256 checksum matches `checksums.sha256`
- Report the issue if checksums match but verification fails

### "Error: fetching bundle: context deadline exceeded"

**Cause**: Network issue connecting to Sigstore services.

**Solution**:
- Check your internet connection
- Verify you can reach `https://rekor.sigstore.dev` and `https://fulcio.sigstore.dev`
- Try again with increased timeout: `COSIGN_TIMEOUT=60s cosign verify ...`

## Security Contact

If you discover a security issue with signed releases:

- **DO NOT** open a public GitHub issue
- Report to: [security@firestoned.io](mailto:security@firestoned.io)
- Include: artifact name, version, verification output, and steps to reproduce

See [SECURITY.md](SECURITY.md) for our security policy and vulnerability disclosure process.

## SPDX License Headers

All Bindy source files include SPDX license identifiers for automated license compliance tracking.

### What is SPDX?

SPDX (Software Package Data Exchange) is an [ISO standard (ISO/IEC 5962:2021)](https://www.iso.org/standard/81870.html) for communicating software license information. SPDX identifiers enable:

- **Automated SBOM generation**: Tools like `cargo-cyclonedx` detect licenses automatically
- **License compliance auditing**: Verify no GPL contamination in MIT-licensed project
- **Supply chain transparency**: Clear license identification at file granularity
- **Tooling integration**: GitHub, Snyk, Trivy, and other tools recognize SPDX headers

### Required Header Format

All source files MUST include SPDX headers in the first 10 lines:

**Rust files (`.rs`):**
```rust
// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT
```

**Shell scripts (`.sh`, `.bash`):**
```bash
#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
```

**Makefiles (`Makefile`, `*.mk`):**
```makefile
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
```

**GitHub Actions workflows (`.yaml`, `.yml`):**
```yaml
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
name: My Workflow
```

### Automated Verification

Bindy enforces SPDX headers via CI/CD:

**Workflow:** `.github/workflows/license-check.yaml`

**Checks:**
- All Rust files (`.rs`)
- All Shell scripts (`.sh`, `.bash`)
- All Makefiles (`Makefile`, `*.mk`)
- All GitHub Actions workflows (`.yaml`, `.yml`)

**Enforcement:**
- Runs on every pull request
- Runs on every push to `main`
- Pull requests **fail** if any source files lack SPDX headers
- Provides clear error messages with examples for missing headers

**Output Example:**
```
✅ All 347 source files have SPDX license headers

File types checked:
  - Rust files (.rs)
  - Shell scripts (.sh, .bash)
  - Makefiles (Makefile, *.mk)
  - GitHub Actions workflows (.yaml, .yml)
```

### License: MIT

Bindy is licensed under the [MIT License](https://opensource.org/licenses/MIT), one of the most permissive open source licenses.

**Permissions:**
- ✅ Commercial use
- ✅ Modification
- ✅ Distribution
- ✅ Private use

**Conditions:**
- 📋 Include copyright notice
- 📋 Include license text

**Limitations:**
- ❌ No liability
- ❌ No warranty

Full license text: [LICENSE](../../../LICENSE)

### Compliance Evidence

**SOX 404 (Sarbanes-Oxley):**
- **Control**: License compliance and intellectual property tracking
- **Evidence**: All source files tagged with SPDX identifiers, automated verification
- **Audit Trail**: Git history shows when SPDX headers were added

**PCI-DSS 6.4.6 (Payment Card Industry):**
- **Requirement**: Code review and approval processes
- **Evidence**: SPDX verification blocks unapproved code (missing headers) from merging
- **Automation**: CI/CD enforces license compliance before code review

**SLSA Level 3 (Supply Chain Security):**
- **Requirement**: Build environment provenance and dependencies
- **Evidence**: SPDX headers enable automated SBOM generation with license info
- **Transparency**: Every dependency's license is machine-readable

---

## References

- [Sigstore Documentation](https://docs.sigstore.dev/)
- [Cosign Documentation](https://docs.sigstore.dev/cosign/overview)
- [Keyless Signing](https://docs.sigstore.dev/cosign/keyless)
- [Rekor Transparency Log](https://docs.sigstore.dev/rekor/overview)
- [SLSA Framework](https://slsa.dev/)
- [Supply Chain Security Best Practices](https://github.com/ossf/scorecard)
- [SPDX Specification](https://spdx.dev/learn/handling-license-info/)
- [ISO/IEC 5962:2021 (SPDX)](https://www.iso.org/standard/81870.html)
