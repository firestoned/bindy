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

## Security Measures

### Commit Signing (CRITICAL)

**All commits MUST be cryptographically signed with GPG or SSH keys.**

This requirement ensures:
- ✅ Cryptographic proof of code authorship
- ✅ Prevention of commit forgery
- ✅ Audit trail for compliance (SOX 404, PCI-DSS 6.4.6)
- ✅ Supply chain integrity (SLSA Level 2+)

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

### Dependency Management

**Security requirements for dependencies:**
- All dependencies scanned with `cargo audit`
- Only use dependencies from crates.io or verified sources
- Dependencies MUST be actively maintained (commits in last 6 months)
- Security vulnerabilities addressed within 30 days of disclosure

**CI/CD checks:**
- `cargo audit` runs on every PR and push to main
- Automated dependabot updates for security patches
- Manual review required for major version updates

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

**Build provenance (SLSA Level 2+):**
- All commits cryptographically signed
- SBOM (Software Bill of Materials) generated for all releases
- Docker images include SBOM and attestation
- Reproducible builds with pinned dependencies

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

**SLSA Level 2 Requirements:**
- ✅ **Build provenance**: Signed commits provide authorship proof
- ✅ **Source integrity**: GPG/SSH signatures verify source authenticity
- ✅ **Build integrity**: Reproducible builds with SBOM
- ✅ **Availability**: Public repository with immutable history

**Evidence:**
- Signed commits in Git history
- SBOM files in release artifacts
- Container image attestations
- CI/CD workflow logs

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

### Incident Classification

**Critical** (Response: Immediate):
- Remote code execution vulnerability
- Privilege escalation in production
- Data breach or unauthorized access
- Supply chain compromise

**High** (Response: 24 hours):
- Authentication bypass
- SQL injection or XSS vulnerability
- Dependency with critical CVE
- Unsigned commits merged to main

**Medium** (Response: 7 days):
- Denial of service vulnerability
- Information disclosure
- Dependency with high CVE

**Low** (Response: 30 days):
- Minor security improvements
- Documentation updates
- Non-exploitable edge cases

### Response Procedures

1. **Identify**: Security issue reported or detected
2. **Contain**: If in production, deploy hotfix or rollback
3. **Investigate**: Determine root cause and impact
4. **Remediate**: Develop and test fix
5. **Deploy**: Emergency release following standard process
6. **Document**: Update CHANGELOG.md and security advisory
7. **Review**: Post-incident review and process improvement

### Communication

- **Internal**: Notify maintainers immediately
- **Public**: Coordinated disclosure after fix available
- **Customers**: Email notification for critical issues
- **Regulators**: Follow compliance reporting requirements

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

**Last Updated**: 2025-12-16
**Next Review**: 2026-03-16 (Quarterly)
