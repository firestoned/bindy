# Security Policy

## Supported Versions

We release patches for security vulnerabilities for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| main    | :white_check_mark: |
| < 1.0   | :x:                |

## Reporting a Vulnerability

**DO NOT** open a public GitHub issue for security vulnerabilities.

### How to Report

Please report security vulnerabilities by emailing:

**security@firestoned.io**

Include in your report:
- Description of the vulnerability
- Steps to reproduce the issue
- Potential impact
- Suggested fix (if any)

### What to Expect

- **Acknowledgment**: Within 24 hours of report
- **Initial Assessment**: Within 72 hours
- **Status Updates**: Weekly until resolution
- **Fix Timeline**: Critical issues within 7 days, high priority within 30 days

### Disclosure Policy

- We practice **coordinated disclosure**
- Security advisories will be published after a fix is available
- We will credit reporters unless they prefer to remain anonymous

## Security Documentation

This section provides an overview of Bindy's security posture. For detailed information, refer to the following documents:

- **[Threat Model](docs/security/THREAT_MODEL.md)**: STRIDE threat analysis, attack surface, threat scenarios
- **[Security Architecture](docs/security/ARCHITECTURE.md)**: Trust boundaries, authentication, secrets management, network security
- **[Incident Response Playbooks](docs/security/INCIDENT_RESPONSE.md)**: Step-by-step response procedures for security incidents
- **[Audit Log Retention Policy](docs/security/AUDIT_LOG_RETENTION.md)**: 7-year retention, WORM storage, log integrity verification
- **[Secret Access Audit Trail](docs/security/SECRET_ACCESS_AUDIT.md)**: Kubernetes audit logs, pre-built compliance queries, alerting rules
- **[Build Reproducibility Verification](docs/security/BUILD_REPRODUCIBILITY.md)**: How to rebuild a release and compare digests (a manual procedure; not yet automated in CI)
- **[Vulnerability Management Policy](docs/security/VULNERABILITY_MANAGEMENT.md)**: Remediation SLAs, scanning process, exception handling
- **[RBAC Verification](deploy/rbac/verify-rbac.sh)**: Automated script to verify least privilege RBAC implementation

---

## Security Measures

### Commit Signing (CRITICAL)

**All commits MUST be cryptographically signed with GPG or SSH keys.**

This requirement ensures:
- ✅ Cryptographic proof of code authorship
- ✅ Prevention of commit forgery
- ✅ Audit trail for compliance (SOX 404, PCI-DSS 6.4.6)
- ✅ Source integrity, one of the supply-chain controls listed in [SLSA compliance](docs/src/compliance/slsa.md)

**Enforcement:**
- Branch protection requires signed commits on `main`
- CI/CD verifies all commits are signed
- Unsigned commits will fail PR checks

**Setup Instructions**: See [CONTRIBUTING.md](CONTRIBUTING.md#commit-signing-requirements)

### Code Review Requirements

**All code changes require:**
- 2+ approving reviews from maintainers
- Cryptographically signed commits from all authors
- Passing CI/CD security checks:
  - Dependency vulnerability scanning (`cargo audit`)
  - Clippy linting with strict warnings
  - Automated tests (unit + integration)
  - SBOM generation for releases

**No exceptions** - even for hotfixes or urgent changes.

### Dependency Management & Vulnerability Scanning

**Security requirements for dependencies:**
- All dependencies scanned with `cargo audit`
- Only use dependencies from crates.io or verified sources
- Dependencies MUST be actively maintained (commits in last 6 months)
- Security vulnerabilities remediated within defined SLAs (see below)

**Automated Vulnerability Scanning:**
- **Rust Dependencies**: `cargo audit` scans all dependencies in every PR, push to main, and release
- **Container Images**: Trivy scans all container images for OS and library vulnerabilities
- **Scheduled Scans**: Daily security scans run automatically at 00:00 UTC
- **SARIF Upload**: Results uploaded to GitHub Security tab for tracking
- **Issue Creation**: GitHub issues automatically created for new vulnerabilities

**CI/CD checks:**
- `cargo audit --deny warnings` runs on every PR and push to main
- **CI FAILS on CRITICAL/HIGH vulnerabilities** - must be remediated before merge
- Trivy scans containers with severity threshold: CRITICAL, HIGH
- Automated security reports uploaded as workflow artifacts
- Manual review required for major version updates

**Remediation SLAs:**
- 🔴 **CRITICAL** (CVSS 9.0-10.0): 24 hours
- 🟠 **HIGH** (CVSS 7.0-8.9): 7 days
- 🟡 **MEDIUM** (CVSS 4.0-6.9): 30 days
- 🔵 **LOW** (CVSS 0.1-3.9): 90 days

**Policy**: See [docs/security/VULNERABILITY_MANAGEMENT.md](docs/security/VULNERABILITY_MANAGEMENT.md) for complete vulnerability management policy

### Access Control

**Repository access:**
- Read access: Public
- Write access: Requires GPG/SSH signed commits + 2FA
- Admin access: Limited to project maintainers
- Release access: Requires signed tags + manual approval

**Branch protection on `main`:**
- ✅ Require signed commits
- ✅ Require linear history (no merge commits)
- ✅ Require pull request reviews (2 approvers)
- ✅ Dismiss stale pull request approvals when new commits pushed
- ✅ Require status checks to pass (CI/CD verification)
- ✅ Require branches to be up to date before merging
- ❌ No direct pushes to `main` (even for admins)

### Supply Chain Security

**Build provenance (SLSA v1.0 Build L3, [ADR-0010](docs/adr/0010-release-sbom-and-slsa-build-l3.md)):**
- Release tarballs, install manifests, SBOMs and images carry SLSA Build L3
  provenance from the isolated `slsa-github-generator` workflows
- Every release binary and image has a CycloneDX SBOM that passes an NTIA
  minimum-elements gate and is a signed attestation bound to the artifact's digest
- Release builds use the committed `Cargo.lock` (`--locked`); base images are
  pinned by digest
- Verify with `make verify-provenance`, `make verify-image-provenance` and
  `make verify-sbom-attestation` (see [Signed Releases](docs/src/security/signed-releases.md))
- Builds are not hermetic, and reproducibility is not yet checked in CI

**Container security:**
- Multi-stage builds minimize attack surface
- Non-root user for runtime
- No secrets in container images
- Regular base image updates for security patches

**Release process:**
- Signed Git tags for all releases
- Automated SBOM generation (CycloneDX format)
- Checksum verification (SHA256)
- Signed container images with provenance attestation

### Secrets Management

**NEVER commit:**
- ❌ API keys, tokens, or credentials
- ❌ Private keys or certificates
- ❌ Internal hostnames or IP addresses
- ❌ Customer or transaction data
- ❌ Encryption keys or secrets

**Approved methods:**
- GitHub Secrets for CI/CD
- Kubernetes Secrets for runtime
- Environment variables (never hardcoded)
- External secret managers (Vault, AWS Secrets Manager)

**Detection:**
- Pre-commit hooks to detect secrets
- GitHub Advanced Security secret scanning
- CI/CD fails if secrets detected

## Compliance

This project operates in a **regulated banking environment** and adheres to:

### SOX 404 - IT General Controls (ITGC)

**Change Management:**
- All code changes require cryptographic signature verification
- Two-person approval required for all merges
- Audit trail maintained via Git history and signed commits
- Changes traceable to business/technical requirements

**Access Controls:**
- Least privilege access (write requires 2FA + signing)
- Regular access reviews (quarterly)
- Separation of duties (2+ reviewers required)

**Evidence Collection:**
- GitHub audit logs
- CI/CD workflow logs
- Commit signature verification logs

### PCI-DSS v4.0

**Requirement 6.4.6 - Code Review:**
- Two-person review required (enforced via GitHub)
- Signed commits provide non-repudiation
- Automated security scanning (cargo audit)
- Change documentation (CHANGELOG.md)

**Requirement 12.10.6 - Change Management:**
- Documented approval process (PR + 2 reviews)
- Testing required before deployment (CI/CD)
- Rollback procedures documented
- Change tracking via Git history

### SLSA (Supply Chain Levels for Software Artifacts)

**SLSA v1.0 Build L3** for release artifacts; see
[SLSA compliance](docs/src/compliance/slsa.md) for the requirement mapping,
what is not claimed, and verification.

**Evidence:**
- `<version>.intoto.jsonl` provenance on every release; image provenance in GHCR
- `*.cdx.json` SBOMs on every release, plus their signed attestations
- Cosign signatures (`*.tar.gz.bundle`, image signatures in GHCR)
- Signed commits in Git history; CI/CD workflow logs

## Security Best Practices

### For Contributors

1. **Sign all commits** - See [CONTRIBUTING.md](CONTRIBUTING.md#commit-signing-requirements)
2. **Keep dependencies updated** - Run `cargo update` regularly
3. **Run security audit** - Run `cargo audit` before committing
4. **Follow least privilege** - Request only necessary permissions
5. **Use 2FA** - Enable two-factor authentication on GitHub

### For Maintainers

1. **Review all code changes** - Even for trusted contributors
2. **Verify commit signatures** - Check "Verified" badge on GitHub
3. **Test security controls** - Regularly verify branch protection works
4. **Rotate keys regularly** - GPG keys expire, plan rotation
5. **Monitor dependencies** - Review Dependabot PRs promptly
6. **Audit access** - Quarterly review of repository access

### For Deployers

1. **Verify signatures** - Check Git tags and container signatures
2. **Validate checksums** - Verify SHA256 checksums for binaries
3. **Scan containers** - Run container security scanning before deployment
4. **Use SBOM** - Review SBOM for known vulnerabilities
5. **Monitor runtime** - Deploy with security monitoring enabled

## Security Incident Response

For detailed incident response procedures, see **[Incident Response Playbooks](docs/security/INCIDENT_RESPONSE.md)**.

### Incident Classification

| Severity | Response Time | Examples |
|----------|---------------|----------|
| 🔴 **CRITICAL** | Immediate (< 15 min) | RCE vulnerability, data breach, supply chain compromise, DNS outage |
| 🟠 **HIGH** | < 1 hour | Authentication bypass, RNDC key compromise, unauthorized DNS changes |
| 🟡 **MEDIUM** | < 4 hours | DoS vulnerability, suspicious activity, vulnerability without exploit |
| 🔵 **LOW** | < 24 hours | Minor improvements, documentation, non-exploitable issues |

### Available Playbooks

- **[P1: Critical Vulnerability Detected](docs/security/INCIDENT_RESPONSE.md#p1-critical-vulnerability-detected)** - CVSS 9.0-10.0 vulnerability, 24-hour remediation SLA
- **[P2: Compromised Operator Pod](docs/security/INCIDENT_RESPONSE.md#p2-compromised-operator-pod)** - Unauthorized access, anomalous behavior
- **[P3: DNS Service Outage](docs/security/INCIDENT_RESPONSE.md#p3-dns-service-outage)** - All BIND9 pods down, queries failing
- **[P4: RNDC Key Compromise](docs/security/INCIDENT_RESPONSE.md#p4-rndc-key-compromise)** - Key leaked, unauthorized RNDC access
- **[P5: Unauthorized DNS Changes](docs/security/INCIDENT_RESPONSE.md#p5-unauthorized-dns-changes)** - Unexpected zone modifications
- **[P6: DDoS Attack](docs/security/INCIDENT_RESPONSE.md#p6-ddos-attack)** - Query flood, resource exhaustion
- **[P7: Supply Chain Compromise](docs/security/INCIDENT_RESPONSE.md#p7-supply-chain-compromise)** - Malicious commit, compromised dependency

### Response Process

All incidents follow the NIST Incident Response Lifecycle:

1. **Preparation**: Playbooks, tools, team readiness
2. **Detection & Analysis**: Confirm incident, assess impact
3. **Containment**: Isolate affected systems, prevent escalation
4. **Eradication**: Remove threat, patch vulnerability
5. **Recovery**: Restore service, verify integrity
6. **Post-Incident Activity**: Document lessons learned, improve defenses

### Communication

- **Internal**: Slack war room (`#incident-[date]-[number]`), status updates every 30 minutes
- **Public**: Coordinated disclosure after fix available
- **Customers**: Email notification for critical/high incidents
- **Regulators**: PCI-DSS (24h), SOX (quarterly), Basel III (risk committee)

## Security Contacts

- **Security Team**: security@firestoned.io
- **Project Maintainers**: See [CODEOWNERS](.github/CODEOWNERS)
- **Compliance Officer**: compliance@firestoned.io (for SOX/PCI-DSS issues)

## Acknowledgments

We thank the security researchers and contributors who help keep Bindy secure:

<!-- Security researchers will be listed here after coordinated disclosure -->

## References

- [GitHub Security Best Practices](https://docs.github.com/en/code-security)
- [SLSA Framework](https://slsa.dev/)
- [OWASP Kubernetes Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Kubernetes_Security_Cheat_Sheet.html)
- [SOC 2 Type II Controls](https://www.aicpa.org/interestareas/frc/assuranceadvisoryservices/aicpasoc2report.html)

---

**Last Updated**: 2025-12-18
**Next Review**: 2026-03-18 (Quarterly)
