# Docker Image Variants

Bindy provides two production-ready container image variants, both optimized for security and compliance in regulated environments.

## Image Variants

### 1. Chainguard (Default - Recommended) ⭐

**Image Tag:** `ghcr.io/firestoned/bindy:latest`

**Base Image:** Chainguard glibc-dynamic
- **Size:** ~15MB
- **CVEs:** **Zero** (rebuilt daily with security patches)
- **FIPS:** FIPS-ready (use `:latest-fips` tag when available)
- **SBOM:** Included by default
- **Compliance:** Designed for SOX, FedRAMP, PCI-DSS
- **Support:** Free for public images, commercial support available

**Use When:**
- ✅ **Running in regulated environments** (banking, healthcare, government)
- ✅ **Zero-CVE requirement** for security audits
- ✅ **FIPS 140-2/140-3 compliance** needed
- ✅ **Fastest security patching** required (daily rebuilds)
- ✅ **Supply chain security** is critical (built-in SBOMs)

**Example:**
```bash
# Pull the default Chainguard-based image
docker pull ghcr.io/firestoned/bindy:latest

# Run with FIPS mode (when FIPS tag available)
docker pull ghcr.io/firestoned/bindy:latest-fips
```

---

### 2. Distroless (Alternative)

**Image Tag:** `ghcr.io/firestoned/bindy-distroless:latest`

**Base Image:** Google Distroless (cc-debian12:nonroot)
- **Size:** ~20MB
- **CVEs:** Minimal (Google's security patches)
- **FIPS:** Compatible (requires configuration)
- **SBOM:** Generated in CI/CD
- **Compliance:** Suitable for most regulatory requirements
- **Support:** Community-supported by Google

**Use When:**
- ✅ **Google Distroless ecosystem** preference
- ✅ **Debian-based compatibility** required
- ✅ **No Chainguard account** available
- ✅ **Community support** is sufficient

**Example:**
```bash
# Pull the Distroless variant
docker pull ghcr.io/firestoned/bindy-distroless:latest
```

---

## Image Comparison

| Feature | Chainguard (Default) | Distroless |
|---------|---------------------|------------|
| **Image Size** | ~15MB | ~20MB |
| **Known CVEs** | 0 (daily rebuilds) | Minimal (periodic patches) |
| **FIPS Mode** | Built-in FIPS images | Requires configuration |
| **SBOM Included** | ✅ Yes (automatic) | ✅ Yes (CI/CD) |
| **Rebuild Frequency** | Daily | On Google's schedule |
| **Commercial Support** | Available | Community |
| **Best For** | Banking, Gov, Healthcare | General production use |

---

## Available Tags

### Chainguard (Default) - Repository: `ghcr.io/firestoned/bindy`

| Tag | Description | Stability | Example |
|-----|-------------|-----------|---------|
| `latest` | Latest release (Chainguard) | Stable | `ghcr.io/firestoned/bindy:latest` |
| `v1.2.3` | Specific version (Chainguard) | Stable | `ghcr.io/firestoned/bindy:v0.2.0` |
| `main-YYYY.MM.DD` | Latest main branch (Chainguard) | Development | `ghcr.io/firestoned/bindy:main-2025.12.17` |
| `pr-N` | Pull request build (Chainguard) | Testing | `ghcr.io/firestoned/bindy:pr-42` |
| `sha-abc123` | Specific commit (Chainguard) | Debugging | `ghcr.io/firestoned/bindy:sha-abc123` |

### Distroless (Alternative) - Repository: `ghcr.io/firestoned/bindy-distroless`

| Tag | Description | Stability | Example |
|-----|-------------|-----------|---------|
| `latest` | Latest release (Distroless) | Stable | `ghcr.io/firestoned/bindy-distroless:latest` |
| `v1.2.3` | Specific version (Distroless) | Stable | `ghcr.io/firestoned/bindy-distroless:v0.2.0` |
| `main-YYYY.MM.DD` | Latest main branch (Distroless) | Development | `ghcr.io/firestoned/bindy-distroless:main-2025.12.17` |
| `pr-N` | Pull request build (Distroless) | Testing | `ghcr.io/firestoned/bindy-distroless:pr-42` |
| `sha-abc123` | Specific commit (Distroless) | Debugging | `ghcr.io/firestoned/bindy-distroless:sha-abc123` |

---

## Multi-Architecture Support

Both image variants support multiple architectures:
- **linux/amd64** (x86_64)
- **linux/arm64** (aarch64)

Docker automatically pulls the correct architecture for your platform.

---

## Security Scanning

All images are automatically scanned for vulnerabilities on every build:

| Scan Type | Frequency | Tool | Results |
|-----------|-----------|------|---------|
| Dependency Scan | Every commit | `cargo audit` | [GitHub Security](https://github.com/firestoned/bindy/security) |
| Container Scan | Every build | Trivy | [GitHub Security](https://github.com/firestoned/bindy/security) |
| SBOM Generation | Every build | CycloneDX | [Artifacts](https://github.com/firestoned/bindy/actions) |
| Supply Chain | Weekly | OpenSSF Scorecard | [Badge in README](../README.md) |

**Expected Results:**
- **Chainguard:** ✅ Zero CVEs (rebuilt daily)
- **Distroless:** ⚠️ Occasional low-severity CVEs (patched by Google)

---

## Deployment Examples

### Kubernetes Deployment (Default - Chainguard)

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: bindy-operator
  namespace: dns-system
spec:
  replicas: 2
  selector:
    matchLabels:
      app: bindy-operator
  template:
    metadata:
      labels:
        app: bindy-operator
    spec:
      serviceAccountName: bindy-operator
      containers:
      - name: bindy
        image: ghcr.io/firestoned/bindy:latest  # Chainguard by default
        imagePullPolicy: IfNotPresent
        securityContext:
          runAsNonRoot: true
          runAsUser: 65532
          allowPrivilegeEscalation: false
          capabilities:
            drop: ["ALL"]
        resources:
          requests:
            memory: "64Mi"
            cpu: "100m"
          limits:
            memory: "256Mi"
            cpu: "500m"
```

### Kubernetes Deployment (Distroless)

```yaml
# ... same as above but with:
        image: ghcr.io/firestoned/bindy-distroless:latest
```

### Docker Compose

```yaml
version: '3.8'

services:
  bindy:
    image: ghcr.io/firestoned/bindy:latest  # Chainguard default
    # OR
    # image: ghcr.io/firestoned/bindy-distroless:latest
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - ./config:/config:ro
```

---

## Building Custom Images

### Build Chainguard Variant

```bash
# Prerequisites: Build binaries first
make build

# Build Chainguard image
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -f docker/Dockerfile.chainguard \
  -t myregistry/bindy:chainguard \
  .
```

### Build Distroless Variant

```bash
# Build Distroless image
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -f docker/Dockerfile \
  -t myregistry/bindy:distroless \
  .
```

---

## FIPS Mode Deployment

For FIPS 140-2/140-3 compliance, see [docs/compliance/fips.md](../docs/compliance/fips.md).

**Quick Start (Chainguard FIPS):**
```yaml
spec:
  containers:
  - name: bindy
    image: ghcr.io/firestoned/bindy:latest-fips  # When available
    env:
    - name: OPENSSL_FORCE_FIPS_MODE
      value: "1"
```

**Distroless FIPS:** Requires custom build with FIPS-enabled OpenSSL (see FIPS docs).

---

## Vulnerability Disclosure

Security vulnerabilities are tracked in:
- [GitHub Security Advisories](https://github.com/firestoned/bindy/security/advisories)
- [Trivy Scan Results](https://github.com/firestoned/bindy/security/code-scanning)

**Chainguard Images:** Automatically patched within 24 hours of CVE disclosure.
**Distroless Images:** Patched on Google's schedule (usually within 1-2 weeks).

---

## Compliance Documentation

For regulatory compliance evidence:
- [SOX Controls](../docs/compliance/sox-controls.md)
- [NIST 800-53](../docs/compliance/nist-800-53.md)
- [CIS Kubernetes Benchmark](../docs/compliance/cis-kubernetes.md)
- [FIPS 140-2/140-3](../docs/compliance/fips.md)
- [Cryptographic Audit](../docs/compliance/crypto-audit.md)

---

## Frequently Asked Questions

### Why is Chainguard the default?

Chainguard provides **zero known CVEs** and is designed specifically for regulated environments. This aligns with Bindy's focus on banking and financial services where security audits require demonstrable zero-CVE compliance.

### Can I switch between variants?

Yes! Both variants are functionally identical. Simply change your image from `ghcr.io/firestoned/bindy:latest` to `ghcr.io/firestoned/bindy-distroless:latest` or vice versa.

### Which variant should I use?

- **Banking/Finance/Government:** Use Chainguard (zero CVEs required)
- **General Production:** Either variant works (Chainguard recommended)
- **Development/Testing:** Either variant works

### Are both variants supported equally?

Yes. Both variants are built, tested, and scanned in CI/CD on every commit. Bug fixes and features apply to both.

### What about Alpine-based images?

We don't provide Alpine-based images because:
1. Bindy is compiled with glibc (not musl)
2. Static musl builds are larger and slower to compile
3. Chainguard already provides minimal, zero-CVE images

---

## Support

For issues or questions about Docker images:
- [GitHub Issues](https://github.com/firestoned/bindy/issues)
- [Security Issues](https://github.com/firestoned/bindy/security)
- [GitHub Discussions](https://github.com/firestoned/bindy/discussions)
