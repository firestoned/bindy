# Compliance Documentation

This directory contains comprehensive compliance and security documentation for the Bindy DNS Operator, designed for use in regulated banking and financial services environments.

## Overview

Bindy implements controls and maintains documentation for the following regulatory frameworks:

- **SOX (Sarbanes-Oxley Act)** - IT General Controls (ITGC) for financial systems
- **NIST 800-53 Rev 5** - Federal security and privacy controls
- **CIS Kubernetes Benchmark** - Industry-standard hardening guidelines
- **FIPS 140-2/140-3** - Cryptographic module validation standards

## Documentation Index

### 1. [SOX Controls](sox-controls.md)
**Sarbanes-Oxley IT General Controls (ITGC)**

Comprehensive mapping of Bindy's implementation to SOX 404 requirements for IT systems supporting financial reporting.

**Coverage:**
- Change Management (CM) - 5 controls ✅
- Access Controls (AC) - 4 controls ✅
- Data Integrity (DI) - 4 controls ✅
- Computer Operations (CO) - 4 controls ✅
- Segregation of Duties (SD) - 2 controls ✅

**Key Features:**
- Evidence collection procedures
- Audit artifacts and trails
- Control testing procedures
- Remediation workflows

**For:** Compliance officers, external auditors, SOX 404 compliance

---

### 2. [NIST 800-53](nist-800-53.md)
**NIST SP 800-53 Rev 5 - Security and Privacy Controls**

Detailed implementation of NIST security controls required for federal information systems and FedRAMP certification.

**Implementation Rate:** 94% (33/35 controls)

**Control Families Covered:**
- AC - Access Control (4/4) ✅
- AU - Audit and Accountability (4/4) ✅
- CM - Configuration Management (4/4) ✅
- IA - Identification and Authentication (3/3) ✅
- IR - Incident Response (2/2) ✅
- RA - Risk Assessment (2/2) ✅
- SA - System Acquisition (4/4) ✅
- SC - System Protection (5/7) ⚠️
- SI - System Integrity (5/5) ✅

**For:** Federal contractors, FedRAMP applicants, government agencies

---

### 3. [CIS Kubernetes Benchmark](cis-kubernetes.md)
**CIS Kubernetes Benchmark v1.8.0 Compliance**

Workload-level security controls following the Center for Internet Security (CIS) Kubernetes hardening guidelines.

**Compliance Levels:**
- **Level 1 (Essential):** 84% (16/19 controls) ✅
- **Level 2 (Defense-in-Depth):** 50% (4/8 controls) ⚠️

**Key Controls:**
- RBAC and service accounts ✅
- Pod security standards ✅
- Network policies ⚠️ (deployment-specific)
- Secret management ✅
- Security contexts ✅

**Includes:**
- Verification commands
- Automated scanning tools
- Hardening recommendations
- NetworkPolicy examples

**For:** Platform engineers, security teams, Kubernetes administrators

---

### 4. [FIPS 140-2/140-3](fips.md)
**Federal Information Processing Standards - Cryptographic Module Validation**

Deployment guide for running Bindy in FIPS mode using validated cryptographic modules.

**Contents:**
- FIPS-enabled deployment options (cluster-level, container-level, AWS-LC)
- Algorithm configuration (TLS cipher suites, TSIG keys)
- Validation and testing procedures
- Troubleshooting common FIPS issues
- Compliance statement templates

**Validated Modules:**
- OpenSSL FIPS Module (Cert #4282, #4816)
- AWS Libcrypto (Cert #4816)
- BoringCrypto (Cert #3678)

**For:** Federal deployments, FIPS-required environments, government contractors

---

### 5. [Cryptographic Audit](crypto-audit.md)
**Cryptographic Implementation Security Assessment**

Complete inventory and security audit of all cryptographic operations in Bindy.

**Cryptographic Operations Covered:**
1. TLS/mTLS (Kubernetes API communication)
2. HMAC-SHA256 (DNS TSIG keys)
3. JWT (ServiceAccount tokens)
4. Container image signatures (planned)
5. Hashing (non-cryptographic)

**Audit Findings:**
- ✅ All crypto uses industry-standard libraries
- ✅ No custom cryptographic implementations
- ✅ FIPS-compatible algorithms by default
- ✅ Proper secret management
- ⚠️ Manual TSIG key rotation (enhancement planned)

**For:** Security auditors, cryptographic compliance, NIST SC-13 validation

---

## Compliance Badges

The following badges are displayed in the [README](../../README.md):

[![SOX Controls](https://img.shields.io/badge/SOX-Controls%20Documented-purple)](sox-controls.md)
[![NIST 800-53](https://img.shields.io/badge/NIST%20800--53-94%25%20Compliant-blue)](nist-800-53.md)
[![CIS Kubernetes](https://img.shields.io/badge/CIS%20Kubernetes-Level%201%20(84%25)-green)](cis-kubernetes.md)
[![FIPS 140-2](https://img.shields.io/badge/FIPS%20140--2-Compatible-blue)](fips.md)
[![Crypto Audit](https://img.shields.io/badge/crypto-audited-brightgreen)](crypto-audit.md)

---

## Using This Documentation

### For Auditors

**SOX Audit:**
1. Review [sox-controls.md](sox-controls.md) for control implementation
2. Verify evidence in [CHANGELOG.md](../../CHANGELOG.md) and Git history
3. Check automated controls in [.github/workflows/](../../.github/workflows/)
4. Validate RBAC definitions in [deploy/rbac/](../../deploy/rbac/)

**NIST 800-53 Assessment:**
1. Review [nist-800-53.md](nist-800-53.md) control-by-control
2. Verify technical controls in source code (links provided)
3. Check security configuration in [deploy/](../../deploy/)
4. Validate incident response process in [SECURITY.md](../../SECURITY.md)

**FIPS Validation:**
1. Review [fips.md](fips.md) deployment guide
2. Verify algorithm usage in [crypto-audit.md](crypto-audit.md)
3. Check CMVP certificate numbers and security policies
4. Validate runtime configuration and testing procedures

### For Developers

**Before Committing:**
- Ensure changes don't break compliance controls
- Update relevant compliance documentation if security-related
- Run `cargo audit` to check for CVEs
- Verify RBAC changes don't violate least privilege

**After Modifying CRDs:**
- Update [API documentation](../src/reference/api.md)
- Check if NIST SI-10 (input validation) controls affected
- Verify CIS pod security controls still apply

### For Operators

**Deployment Checklist:**
- [ ] Review RBAC permissions in [deploy/rbac/](../../deploy/rbac/)
- [ ] Apply namespace isolation (CIS 5.7.1)
- [ ] Enable etcd encryption (NIST SC-28)
- [ ] Configure NetworkPolicies (CIS 5.3.2)
- [ ] Enable FIPS mode if required ([fips.md](fips.md))
- [ ] Set up log retention (SOX CO-3, NIST AU-9)
- [ ] Configure Kubernetes audit logging (SOX AC-4, NIST AU-12)

---

## Continuous Compliance

### Automated Controls

**Daily:**
- Dependency vulnerability scans (`cargo audit`)
- SBOM generation and CVE checks
- Security advisory monitoring

**Weekly:**
- OpenSSF Scorecard assessment
- Dependency update reviews (Dependabot)

**Per Commit:**
- Code quality checks (clippy, rustfmt)
- Unit and integration tests
- RBAC permission validation

### Manual Reviews

**Quarterly:**
- Access control reviews (NIST AC-2)
- Secret rotation status (NIST SC-12)
- Compliance documentation updates

**Annually:**
- Full control testing (SOX, NIST)
- Cryptographic audit refresh
- Third-party security assessment

---

## Compliance Gaps and Roadmap

### Current Gaps

1. **Seccomp Profile** (CIS 5.7.2)
   - Status: Not implemented
   - Priority: Medium
   - Plan: Add `RuntimeDefault` seccomp profile to SecurityContext

2. **TSIG Key Rotation** (NIST SC-12)
   - Status: Manual process
   - Priority: Medium
   - Plan: Automate periodic key rotation

3. **Container Image Signing** (NIST SI-7)
   - Status: In progress (workflow created)
   - Priority: High
   - Plan: Complete Sigstore/Cosign integration

4. **FIPS Container Images** (FIPS 140-2)
   - Status: Deployment guide only
   - Priority: Low
   - Plan: Provide pre-built FIPS-enabled images

### Enhancement Roadmap

**Q1 2026:**
- [ ] Automated TSIG key rotation
- [ ] Container image signing in CI/CD
- [ ] Seccomp profile implementation
- [ ] Read-only root filesystem enforcement

**Q2 2026:**
- [ ] HSM integration for key storage
- [ ] External secret management (Vault/ESO)
- [ ] FIPS-enabled container images
- [ ] Service mesh mTLS documentation

**Q3 2026:**
- [ ] OPA/Kyverno policy examples
- [ ] Runtime security monitoring (Falco)
- [ ] SLSA Level 4 (reproducible builds)

---

## Compliance Statement

**For inclusion in System Security Plans (SSP):**

> The Bindy DNS Operator is designed and operated in accordance with industry-standard security frameworks including SOX IT General Controls, NIST 800-53 Rev 5, CIS Kubernetes Benchmark, and FIPS 140-2/140-3 cryptographic standards.
>
> All compliance documentation is version-controlled, auditor-ready, and includes evidence locations, verification procedures, and remediation workflows.
>
> **Compliance Rates:**
> - SOX ITGC: 100% (19/19 controls)
> - NIST 800-53: 94% (33/35 controls)
> - CIS Kubernetes Level 1: 84% (16/19 controls)
> - FIPS 140-2: Compatible (deployment guide provided)
>
> **Audit Trail:**
> All changes are tracked in Git with signed commits, automated testing, and changelog documentation. Security controls are continuously monitored via OpenSSF Scorecard and vulnerability scanning.

---

## References

### Regulatory Frameworks
- [SOX Section 404](https://www.sec.gov/rules/final/33-8238.htm)
- [NIST SP 800-53 Rev 5](https://csrc.nist.gov/publications/detail/sp/800-53/rev-5/final)
- [CIS Kubernetes Benchmark v1.8.0](https://www.cisecurity.org/benchmark/kubernetes)
- [FIPS 140-2 Standard](https://csrc.nist.gov/publications/detail/fips/140/2/final)
- [FIPS 140-3 Standard](https://csrc.nist.gov/publications/detail/fips/140/3/final)

### Security Resources
- [OpenSSF Best Practices](https://bestpractices.coreinfrastructure.org/)
- [SLSA Framework](https://slsa.dev/)
- [NIST Cybersecurity Framework](https://www.nist.gov/cyberframework)
- [Kubernetes Security](https://kubernetes.io/docs/concepts/security/)

### Project Resources
- [Bindy GitHub Repository](https://github.com/firestoned/bindy)
- [Security Policy](../../SECURITY.md)
- [Changelog](../../CHANGELOG.md)
- [Contributing Guide](../../CONTRIBUTING.md)

---

## Document Control

| Version | Date       | Author          | Changes                          |
|---------|------------|-----------------|----------------------------------|
| 1.0     | 2025-12-17 | Erick Bourgeois | Initial compliance documentation |

**Next Review Date:** 2026-12-17 (Annual)

---

## Contact

For compliance questions or audit requests:
- **GitHub Issues:** https://github.com/firestoned/bindy/issues
- **Security:** See [SECURITY.md](../../SECURITY.md) for security-related inquiries
- **Email:** See SECURITY.md for responsible disclosure contact
