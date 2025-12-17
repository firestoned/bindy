# FIPS 140-2/140-3 Compliance Guide

**Document Version:** 1.0
**Last Updated:** 2025-12-17
**Scope:** Bindy DNS Operator for Kubernetes
**Standard:** FIPS 140-2 and FIPS 140-3 (Federal Information Processing Standards)

---

## Executive Summary

This document describes how to deploy Bindy in FIPS 140-2/140-3 compliant mode for use in U.S. federal government environments and regulated industries that require validated cryptography.

**Important:** FIPS compliance requires both:
1. **FIPS-validated cryptographic modules** in the runtime environment
2. **Proper configuration** to ensure only FIPS-approved algorithms are used

**Current Status:**
- ⚠️ Bindy **CAN** be deployed in FIPS mode with appropriate runtime configuration
- ⚠️ Bindy does **NOT** include FIPS-validated crypto modules by default
- ✅ Bindy uses standard TLS libraries that support FIPS mode when properly configured

---

## FIPS 140-2 vs FIPS 140-3

### FIPS 140-2
- **Status:** Current standard, widely adopted
- **Validation:** NIST Cryptographic Module Validation Program (CMVP)
- **Security Levels:** 1 (basic) through 4 (highest)
- **Expiration:** Being phased out in favor of FIPS 140-3

### FIPS 140-3
- **Status:** New standard (effective September 2019)
- **Changes:** Aligned with ISO/IEC 19790:2012
- **Backward Compatibility:** FIPS 140-2 modules accepted during transition
- **Deadline:** September 2026 for 140-2 sunset

**Recommendation:** Deploy with FIPS 140-3 validated modules for new systems.

---

## Cryptographic Requirements

### FIPS-Approved Algorithms

**Allowed:**
- **Encryption:** AES-128, AES-192, AES-256
- **Hashing:** SHA-256, SHA-384, SHA-512
- **Digital Signatures:** RSA (2048-bit+), ECDSA (P-256, P-384, P-521)
- **Key Exchange:** ECDH, RSA key transport
- **Message Authentication:** HMAC-SHA256, HMAC-SHA384, HMAC-SHA512

**Prohibited:**
- MD5 (except for non-security purposes)
- SHA-1 (except for legacy HMAC and digital signature verification)
- DES, 3DES (deprecated)
- RC4
- RSA keys < 2048 bits

---

## FIPS Compliance in Bindy

### Cryptographic Operations in Bindy

1. **Kubernetes API Communication**
   - **Operation:** mTLS connections to Kubernetes API server
   - **Library:** System TLS (OpenSSL or BoringSSL via `rustls` or `native-tls`)
   - **FIPS Mode:** Requires FIPS-enabled TLS library

2. **DNS TSIG (Transaction Signatures)**
   - **Operation:** HMAC authentication for zone transfers
   - **Algorithm:** HMAC-SHA256 (FIPS-approved)
   - **Library:** BIND9 (supports FIPS mode when compiled with FIPS-enabled OpenSSL)

3. **Container Image Verification (Optional)**
   - **Operation:** Signature verification for container images
   - **Library:** Sigstore/Cosign
   - **FIPS Mode:** Requires FIPS-enabled crypto library

---

## FIPS Deployment Options

### Option 1: FIPS-Enabled Kubernetes Cluster (Recommended)

**Best for:** Government and regulated environments

**Requirements:**
- Kubernetes cluster built with FIPS-enabled container runtime
- FIPS-enabled Linux distribution (RHEL 8+, Ubuntu 20.04+ FIPS)
- FIPS-validated OpenSSL or BoringSSL

**Validated Platforms:**
- **Red Hat OpenShift:** FIPS mode supported (OpenSSL FIPS module)
- **Rancher Government Solutions (RGS):** FIPS 140-2 validated
- **VMware Tanzu:** FIPS mode available
- **Amazon EKS (FIPS endpoints):** FIPS-enabled API endpoints

**Configuration:**
1. Enable FIPS mode on cluster nodes:
   ```bash
   # RHEL 8+
   sudo fips-mode-setup --enable
   sudo reboot

   # Ubuntu 20.04+ (requires ubuntu-advantage-tools)
   sudo ua enable fips
   sudo reboot
   ```

2. Verify FIPS mode:
   ```bash
   cat /proc/sys/crypto/fips_enabled
   # Should output: 1
   ```

3. Deploy Bindy normally - inherits FIPS mode from host

---

### Option 2: FIPS-Enabled Container Image

**Best for:** Mixed environments (FIPS and non-FIPS workloads)

**Requirements:**
- Build Bindy with FIPS-enabled Rust toolchain
- Use FIPS-validated base image
- Link against FIPS-enabled OpenSSL

**Base Images (FIPS-enabled):**
- Red Hat UBI 8/9 Minimal (FIPS mode available)
- Iron Bank (DoD approved, FIPS-validated)
- Google Distroless FIPS variant (if available)

**Build Steps:**

1. **Dockerfile with FIPS OpenSSL:**
   ```dockerfile
   # Use FIPS-enabled base image
   FROM registry.access.redhat.com/ubi9/ubi-minimal:latest AS builder

   # Install FIPS-enabled OpenSSL
   RUN microdnf install -y openssl openssl-devel

   # Enable FIPS mode in container
   RUN fips-mode-setup --enable

   # Build Rust binary with FIPS-enabled OpenSSL
   ENV OPENSSL_FORCE_FIPS_MODE=1
   COPY . /src
   WORKDIR /src
   RUN cargo build --release

   # Runtime image
   FROM registry.access.redhat.com/ubi9/ubi-minimal:latest
   RUN fips-mode-setup --enable
   COPY --from=builder /src/target/release/bindy /usr/local/bin/bindy
   ENV OPENSSL_FORCE_FIPS_MODE=1
   USER 1000
   ENTRYPOINT ["/usr/local/bin/bindy"]
   ```

2. **Build and verify:**
   ```bash
   docker build -t bindy:fips .
   docker run --rm bindy:fips openssl version
   # Should show: OpenSSL <version> FIPS
   ```

---

### Option 3: AWS-LC (BoringSSL) for FIPS

**Best for:** Cloud-native environments

**AWS Libcrypto (AWS-LC)** is a FIPS 140-3 validated cryptographic library based on BoringSSL.

**Rust Integration:**
```toml
# Cargo.toml
[dependencies]
# Use AWS-LC instead of ring or rustls-native-certs
aws-lc-rs = { version = "1.0", features = ["fips"] }
rustls = { version = "0.21", default-features = false, features = ["aws-lc-rs"] }
```

**Benefits:**
- FIPS 140-3 validated (Cert #4816)
- Better performance than OpenSSL FIPS module
- Native Rust integration

**Limitations:**
- Requires Rust code changes
- Not yet as widely adopted as OpenSSL

---

## FIPS Configuration Checklist

### Runtime Environment

- [ ] **FIPS mode enabled on host OS**
  ```bash
  cat /proc/sys/crypto/fips_enabled  # Should be 1
  ```

- [ ] **FIPS-validated OpenSSL/BoringSSL installed**
  ```bash
  openssl version
  # Should include "FIPS" in output
  ```

- [ ] **FIPS module loaded in OpenSSL**
  ```bash
  openssl list -providers
  # Should show FIPS provider
  ```

### Application Configuration

- [ ] **Environment variable set (if using OpenSSL):**
  ```yaml
  env:
  - name: OPENSSL_FORCE_FIPS_MODE
    value: "1"
  ```

- [ ] **Kubernetes API FIPS endpoints (AWS EKS):**
  ```yaml
  # Use FIPS endpoints for EKS API
  cluster:
    server: https://<cluster-id>.yl4.us-gov-west-1.eks.amazonaws.com
  ```

- [ ] **Only FIPS-approved algorithms configured:**
  - TLS 1.2+ only (no TLS 1.0/1.1)
  - AES-GCM cipher suites only
  - SHA-256+ hashing only

### BIND9 FIPS Mode

- [ ] **BIND9 compiled with FIPS-enabled OpenSSL**
- [ ] **TSIG keys use HMAC-SHA256 (not HMAC-MD5)**
  ```yaml
  # In Bind9Instance CR
  spec:
    tsigAlgorithm: hmac-sha256  # FIPS-approved
  ```

---

## FIPS Algorithm Configuration

### TLS Cipher Suites (FIPS-Approved)

**Recommended for Kubernetes API:**
```
TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
```

**Prohibited (Non-FIPS):**
```
TLS_RSA_WITH_RC4_128_SHA            # RC4 not FIPS-approved
TLS_RSA_WITH_3DES_EDE_CBC_SHA       # 3DES deprecated
TLS_RSA_WITH_AES_128_CBC_SHA        # CBC mode vulnerable
```

### TSIG Key Algorithms

**FIPS-Approved:**
- `hmac-sha256` ✅
- `hmac-sha384` ✅
- `hmac-sha512` ✅

**Not FIPS-Approved:**
- `hmac-md5` ❌

**Configuration:**
```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary
spec:
  tsigKeySecret:
    name: tsig-keys
    key: transfer-key
  # Ensure TSIG algorithm is FIPS-approved (default is hmac-sha256)
```

---

## FIPS Validation and Testing

### Verify FIPS Mode is Active

**On Kubernetes Node:**
```bash
# Check kernel FIPS flag
cat /proc/sys/crypto/fips_enabled
# Expected: 1

# Check OpenSSL FIPS mode
openssl md5 /dev/null
# Expected: Error (MD5 disabled in FIPS mode)
```

**In Bindy Container:**
```bash
# Exec into operator pod
kubectl exec -it deployment/bindy-operator -- sh

# Verify FIPS mode
cat /proc/sys/crypto/fips_enabled

# Check OpenSSL provider (if OpenSSL CLI available)
openssl list -providers
```

### Test Cryptographic Operations

**Test TLS Connection:**
```bash
# Verify only FIPS cipher suites accepted
openssl s_client -connect kubernetes.default.svc:443 -cipher 'FIPS'
# Should succeed

openssl s_client -connect kubernetes.default.svc:443 -cipher 'RC4'
# Should fail (RC4 not FIPS-approved)
```

**Test TSIG Keys:**
```bash
# Verify TSIG algorithm
kubectl get secret tsig-keys -o yaml | grep algorithm
# Should show: hmac-sha256 or hmac-sha512
```

---

## FIPS Certification Evidence

### Required Documentation for Auditors

1. **CMVP Certificate Numbers:**
   - OpenSSL FIPS Module: Cert #4282 (140-2) or #4816 (140-3)
   - AWS-LC: Cert #4816 (140-3)
   - BoringCrypto: Cert #3678 (140-2)

2. **Security Policy Documents:**
   - [OpenSSL FIPS 140-2 Security Policy](https://www.openssl.org/docs/fips/SecurityPolicy-3.0.pdf)
   - [AWS-LC FIPS 140-3 Security Policy](https://csrc.nist.gov/CSRC/media/projects/cryptographic-module-validation-program/documents/security-policies/140sp4816.pdf)

3. **Configuration Evidence:**
   - Output of `cat /proc/sys/crypto/fips_enabled`
   - OpenSSL version output showing FIPS
   - Kubernetes API server audit logs (TLS cipher suites)
   - Bindy deployment YAML with FIPS environment variables

4. **Algorithm Usage Documentation:**
   - This document (fips.md)
   - TLS cipher suite configuration
   - TSIG algorithm configuration

---

## Non-FIPS Cryptographic Operations

### Operations That Do NOT Require FIPS Validation

Some operations use cryptography for non-security purposes and are exempt from FIPS requirements:

1. **Checksums for Data Integrity (Non-Authenticated):**
   - File checksums (MD5, CRC32) for corruption detection
   - Not used for security decisions

2. **Randomness for Non-Security Purposes:**
   - Jitter/backoff timers
   - Load balancing distribution

3. **Legacy Interoperability:**
   - SHA-1 for Git commit hashing (not security-critical)

**Important:** Document these exemptions for auditors.

---

## Troubleshooting FIPS Issues

### Error: "FIPS mode not enabled"

**Cause:** Host OS not in FIPS mode

**Solution:**
```bash
sudo fips-mode-setup --enable
sudo reboot
```

---

### Error: "Error setting cipher list"

**Cause:** Non-FIPS cipher suite requested

**Solution:** Ensure only FIPS-approved cipher suites configured in Kubernetes API server and applications.

---

### Error: "MD5 not available in FIPS mode"

**Cause:** Application attempting to use MD5 hashing

**Solution:**
- If for security: Replace with SHA-256
- If for checksums: Document as non-security exemption

---

### BIND9 TSIG Errors in FIPS Mode

**Cause:** HMAC-MD5 TSIG keys not FIPS-approved

**Solution:**
```yaml
# Regenerate TSIG keys with FIPS-approved algorithm
tsig-keygen -a hmac-sha256 transfer-key > /etc/bind/keys/transfer-key.conf
```

---

## Compliance Statement Template

For inclusion in System Security Plans (SSP) and FedRAMP documentation:

```
FIPS 140-2/140-3 Compliance Statement

The Bindy DNS Operator is deployed in FIPS mode using the following validated
cryptographic modules:

1. Cryptographic Module: OpenSSL FIPS Object Module
   - CMVP Certificate: #4282 (FIPS 140-2) / #4816 (FIPS 140-3)
   - Security Level: Level 1
   - Validation Date: [Date from certificate]
   - Embodiment: Software
   - Description: Cryptographic library providing TLS and hashing functions

2. FIPS Mode Verification:
   - Kernel FIPS flag enabled: /proc/sys/crypto/fips_enabled = 1
   - OpenSSL version: OpenSSL 3.0.x FIPS
   - Environment: OPENSSL_FORCE_FIPS_MODE=1

3. Approved Algorithms in Use:
   - TLS: TLS 1.2+ with AES-GCM cipher suites
   - Hashing: SHA-256, SHA-384, SHA-512
   - Message Authentication: HMAC-SHA256
   - Digital Signatures: RSA-2048, ECDSA P-256

4. Non-FIPS Operations (Documented Exemptions):
   - Git commit hashing (SHA-1, non-security purpose)
   - None other

Signed: _______________________  Date: _____________
```

---

## References

- [NIST CMVP](https://csrc.nist.gov/projects/cryptographic-module-validation-program) - Cryptographic Module Validation Program
- [FIPS 140-2 Standard](https://csrc.nist.gov/publications/detail/fips/140/2/final) - Security Requirements for Cryptographic Modules
- [FIPS 140-3 Standard](https://csrc.nist.gov/publications/detail/fips/140/3/final) - New validation standard
- [OpenSSL FIPS Module](https://www.openssl.org/docs/fips.html) - FIPS validated OpenSSL builds
- [AWS-LC FIPS](https://aws.amazon.com/blogs/security/introducing-aws-libcrypto-for-rust-an-open-source-cryptographic-library-for-rust/) - FIPS 140-3 validated BoringSSL fork
- [Red Hat FIPS Compliance](https://access.redhat.com/documentation/en-us/red_hat_enterprise_linux/8/html/security_hardening/using-the-system-wide-cryptographic-policies_security-hardening) - RHEL FIPS mode

---

## Document Control

| Version | Date       | Author          | Changes                          |
|---------|------------|-----------------|----------------------------------|
| 1.0     | 2025-12-17 | Erick Bourgeois | Initial FIPS compliance guide     |

