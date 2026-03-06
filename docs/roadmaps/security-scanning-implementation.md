# Security Scanning Implementation Roadmap

**Date:** 2026-03-06
**Status:** Proposed
**Author:** Erick Bourgeois
**Impact:** High - Implements comprehensive security scanning for regulated banking environment

---

## Executive Summary

Implement a multi-layered security scanning strategy to meet compliance requirements for a regulated banking environment. This roadmap covers container scanning, dependency analysis, secret detection, SAST, Kubernetes security validation, and license compliance.

**Timeline:** 5-6 weeks
**Effort:** ~20-30 hours total
**Risk:** Low - All tools are additive and non-breaking

---

## Motivation

### Current State
- ✅ `cargo audit` for Rust dependency vulnerabilities
- ✅ `cargo clippy` for code quality
- ✅ Manual security reviews
- ❌ No container image scanning
- ❌ No secret detection automation
- ❌ No Kubernetes manifest security validation
- ❌ No license compliance tracking
- ❌ No SAST for security vulnerabilities

### Target State
- ✅ Comprehensive multi-layer security scanning
- ✅ Automated security checks in CI/CD
- ✅ Secret detection in pre-commit hooks
- ✅ Container image vulnerability scanning
- ✅ Kubernetes manifest security validation
- ✅ License compliance enforcement
- ✅ SAST for security bug detection
- ✅ Supply chain security (SBOM generation)

### Business Drivers
1. **Regulatory Compliance**: Banking regulations require auditable security practices
2. **Risk Mitigation**: Detect vulnerabilities before production deployment
3. **Supply Chain Security**: Track and verify all dependencies
4. **Audit Trail**: Automated security reports for compliance audits
5. **Zero Trust**: Align with zero-trust security principles

---

## Security Scanning Stack

### Tier 1: Must Have (Weeks 1-3)
| Tool | Purpose | Priority | Complexity |
|------|---------|----------|------------|
| cargo-deny | Comprehensive dependency security + license checking | P0 | Low |
| Gitleaks | Secret scanning (pre-commit + CI) | P0 | Low |
| Dependabot | Automated dependency updates | P0 | Low (GitHub native) |
| CodeQL | SAST for Rust security bugs | P0 | Medium |
| Trivy | Container + IaC + filesystem scanning | P0 | Medium |

### Tier 2: Highly Recommended (Weeks 4-5)
| Tool | Purpose | Priority | Complexity |
|------|---------|----------|------------|
| Semgrep | Custom security rules + community rulesets | P1 | Medium |
| Kubesec | Kubernetes manifest security validation | P1 | Low |
| cargo-license | License compliance tracking | P1 | Low |

### Tier 3: Nice to Have (Week 6+)
| Tool | Purpose | Priority | Complexity |
|------|---------|----------|------------|
| Syft | SBOM generation | P2 | Medium |
| Cosign | Container image signing | P2 | High |
| Polaris | K8s best practices validation | P2 | Low |

---

## Implementation Phases

### Phase 1: Foundation (Week 1)
**Goal:** Replace cargo-audit with cargo-deny and add secret scanning

#### Tasks
1. **cargo-deny Setup**
   - [ ] Install cargo-deny: `cargo install cargo-deny`
   - [ ] Create `.cargo-deny.toml` configuration
   - [ ] Add Makefile target: `make cargo-deny`
   - [ ] Update CI workflow to use cargo-deny instead of cargo-audit
   - [ ] Test with current dependencies
   - [ ] Document in README.md

2. **Gitleaks Setup**
   - [ ] Install Gitleaks: `brew install gitleaks`
   - [ ] Create `.gitleaks.toml` configuration (if needed)
   - [ ] Add Makefile target: `make gitleaks`
   - [ ] Add pre-commit hook: `.git/hooks/pre-commit`
   - [ ] Add CI workflow job for Gitleaks
   - [ ] Scan existing codebase and remediate any findings
   - [ ] Document in README.md

3. **Dependabot Setup**
   - [ ] Create `.github/dependabot.yml` configuration
   - [ ] Enable Dependabot alerts in GitHub repo settings
   - [ ] Configure auto-merge for minor/patch updates (optional)
   - [ ] Document dependency update process

**Deliverables:**
- `.cargo-deny.toml` configuration file
- Makefile targets: `cargo-deny`, `gitleaks`
- `.github/dependabot.yml` configuration
- Pre-commit hook for Gitleaks
- Updated CI workflow
- Updated `.claude/CHANGELOG.md`

**Success Criteria:**
- ✅ cargo-deny runs in CI and fails on HIGH/CRITICAL vulnerabilities
- ✅ Gitleaks blocks commits with secrets locally
- ✅ Gitleaks runs in CI and fails on detected secrets
- ✅ Dependabot creates weekly PRs for dependency updates

---

### Phase 2: SAST Integration (Week 2)
**Goal:** Add static application security testing with CodeQL

#### Tasks
1. **CodeQL Setup**
   - [ ] Create `.github/workflows/codeql.yml` workflow
   - [ ] Configure Rust language analysis
   - [ ] Enable scheduled scans (weekly)
   - [ ] Enable on push to main and PRs
   - [ ] Test on current codebase
   - [ ] Review and triage any findings
   - [ ] Enable GitHub Security tab integration
   - [ ] Document in README.md

2. **CodeQL Custom Queries** (Optional)
   - [ ] Create `.github/codeql/` directory
   - [ ] Add custom queries for Kubernetes operator patterns
   - [ ] Add queries for kube-rs specific security issues

**Deliverables:**
- `.github/workflows/codeql.yml` workflow
- CodeQL configuration
- Security findings triage report
- Updated `.claude/CHANGELOG.md`

**Success Criteria:**
- ✅ CodeQL runs weekly on schedule
- ✅ CodeQL runs on all PRs
- ✅ Results visible in GitHub Security tab
- ✅ Zero HIGH/CRITICAL findings (or documented exceptions)

---

### Phase 3: Container & IaC Scanning (Week 3)
**Goal:** Add Trivy for comprehensive container and infrastructure scanning

#### Tasks
1. **Trivy Setup**
   - [ ] Install Trivy: `brew install aquasecurity/trivy/trivy`
   - [ ] Create Makefile targets:
     - `make trivy-install`
     - `make trivy-image`
     - `make trivy-fs`
     - `make trivy-k8s`
     - `make trivy-all`
   - [ ] Create `.github/workflows/security-scan.yml` reusable workflow
   - [ ] Integrate into CI workflow
   - [ ] Scan current container images
   - [ ] Scan Kubernetes manifests in `deploy/` and `examples/`
   - [ ] Review and remediate findings

2. **Trivy GitHub Integration**
   - [ ] Configure SARIF output
   - [ ] Upload results to GitHub Security tab
   - [ ] Set up scheduled scans (daily/weekly)

3. **Trivy Database Management**
   - [ ] Document database update process
   - [ ] Add database caching in CI (optional)

**Deliverables:**
- Makefile targets for Trivy scanning
- `.github/workflows/security-scan.yml` reusable workflow
- Trivy scan results and remediation report
- Updated CI workflow to include Trivy
- Updated `.claude/CHANGELOG.md`

**Success Criteria:**
- ✅ Trivy scans container images in CI
- ✅ Trivy scans Kubernetes manifests in CI
- ✅ Trivy scans filesystem for vulnerabilities
- ✅ Results uploaded to GitHub Security tab
- ✅ Zero HIGH/CRITICAL vulnerabilities (or documented exceptions)

---

### Phase 4: Advanced SAST & K8s Security (Week 4)
**Goal:** Add Semgrep for custom security rules and Kubesec for K8s validation

#### Tasks
1. **Semgrep Setup**
   - [ ] Install Semgrep: `pip3 install semgrep` or use GitHub Action
   - [ ] Create Makefile target: `make semgrep`
   - [ ] Configure rulesets:
     - `p/rust` - Rust security rules
     - `p/kubernetes` - Kubernetes security rules
     - `p/docker` - Docker security rules
   - [ ] Add GitHub Action: `semgrep/semgrep-action@v1`
   - [ ] Test on current codebase
   - [ ] Review and triage findings

2. **Custom Semgrep Rules**
   - [ ] Create `.semgrep/` directory
   - [ ] Write custom rules for:
     - Kubernetes operator patterns (e.g., missing finalizers)
     - DNS security (e.g., zone file generation)
     - Secret handling (e.g., no hardcoded credentials)

3. **Kubesec Setup**
   - [ ] Install Kubesec: `brew install kubesec`
   - [ ] Create Makefile target: `make kubesec`
   - [ ] Scan all manifests in `deploy/` and `examples/`
   - [ ] Review and remediate findings
   - [ ] Add to CI workflow

**Deliverables:**
- Makefile targets: `semgrep`, `kubesec`
- `.semgrep/` directory with custom rules
- Semgrep GitHub Action workflow
- Kubesec scan results and remediation report
- Updated `.claude/CHANGELOG.md`

**Success Criteria:**
- ✅ Semgrep runs on all PRs
- ✅ Custom Semgrep rules detect operator-specific security issues
- ✅ Kubesec validates all Kubernetes manifests
- ✅ All HIGH/CRITICAL findings resolved or documented

---

### Phase 5: License Compliance (Week 5)
**Goal:** Implement license compliance tracking and enforcement

#### Tasks
1. **cargo-license Setup**
   - [ ] Install: `cargo install cargo-license`
   - [ ] Create Makefile target: `make license-check`
   - [ ] Generate license report: `licenses.json`
   - [ ] Document allowed licenses in `.cargo-deny.toml`
   - [ ] Add license check to CI workflow

2. **License Policy Definition**
   - [ ] Define allowed licenses (MIT, Apache-2.0, BSD-3-Clause)
   - [ ] Define denied licenses (GPL, AGPL, SSPL)
   - [ ] Document license policy in `docs/`

3. **License Compliance Workflow**
   - [ ] Create GitHub Action for license checking
   - [ ] Fail CI on GPL/AGPL/SSPL licenses
   - [ ] Generate license report on releases

**Deliverables:**
- Makefile target: `license-check`
- License policy documentation
- Updated `.cargo-deny.toml` with license rules
- CI workflow integration
- Updated `.claude/CHANGELOG.md`

**Success Criteria:**
- ✅ License compliance enforced in CI
- ✅ CI fails on GPL/AGPL/SSPL licenses
- ✅ License report generated on every release
- ✅ All dependencies have approved licenses

---

### Phase 6: Supply Chain Security (Week 6+)
**Goal:** Implement SBOM generation and image signing (Optional)

#### Tasks
1. **Syft SBOM Generation**
   - [ ] Install Syft: `brew install syft`
   - [ ] Create Makefile target: `make syft-sbom`
   - [ ] Generate SBOM in SPDX and CycloneDX formats
   - [ ] Add SBOM generation to release workflow
   - [ ] Store SBOMs as release artifacts

2. **Grype Scanning** (Alternative to Trivy)
   - [ ] Install Grype: `brew install grype`
   - [ ] Create Makefile target: `make grype-scan`
   - [ ] Scan generated SBOMs
   - [ ] Compare Grype vs Trivy results

3. **Cosign Image Signing**
   - [ ] Install Cosign: `brew install cosign`
   - [ ] Generate signing key pair
   - [ ] Create Makefile targets: `make cosign-sign`, `make cosign-verify`
   - [ ] Add image signing to release workflow
   - [ ] Document signature verification process
   - [ ] Add signature verification to deployment process

4. **Polaris Best Practices**
   - [ ] Install Polaris: `brew install fairwindsops/tap/polaris`
   - [ ] Create Makefile target: `make polaris`
   - [ ] Scan Kubernetes manifests
   - [ ] Review and remediate findings

**Deliverables:**
- Makefile targets: `syft-sbom`, `grype-scan`, `cosign-sign`, `cosign-verify`, `polaris`
- SBOM generation in release workflow
- Image signing in release workflow
- Polaris scan results
- Updated `.claude/CHANGELOG.md`

**Success Criteria:**
- ✅ SBOMs generated for all releases
- ✅ Container images signed with Cosign
- ✅ Signature verification documented
- ✅ Polaris validates all Kubernetes manifests

---

## Makefile Targets Summary

```makefile
# Security scanning targets
.PHONY: security-scan-full
security-scan-full: cargo-deny gitleaks trivy-all semgrep kubesec license-check ## Run all security scans

.PHONY: security-scan-quick
security-scan-quick: cargo-deny gitleaks trivy-fs ## Run quick security scans (for CI)

.PHONY: security-scan-local
security-scan-local: cargo-deny gitleaks ## Run local security scans (pre-commit)

# Individual tool targets
.PHONY: cargo-deny
cargo-deny: ## Check dependencies for security, licenses, and supply chain issues

.PHONY: gitleaks
gitleaks: ## Scan for hardcoded secrets

.PHONY: trivy-install
trivy-install: ## Install Trivy scanner

.PHONY: trivy-image
trivy-image: ## Scan Docker image for vulnerabilities

.PHONY: trivy-fs
trivy-fs: ## Scan filesystem for vulnerabilities and secrets

.PHONY: trivy-k8s
trivy-k8s: ## Scan Kubernetes manifests for misconfigurations

.PHONY: trivy-all
trivy-all: trivy-fs trivy-k8s ## Run all Trivy security scans (except image)

.PHONY: semgrep
semgrep: ## Run Semgrep security analysis

.PHONY: kubesec
kubesec: ## Scan Kubernetes manifests with Kubesec

.PHONY: license-check
license-check: ## Check dependency licenses

.PHONY: syft-sbom
syft-sbom: ## Generate SBOM for container image

.PHONY: grype-scan
grype-scan: ## Scan SBOM with Grype

.PHONY: cosign-sign
cosign-sign: ## Sign container image with Cosign

.PHONY: cosign-verify
cosign-verify: ## Verify container image signature

.PHONY: polaris
polaris: ## Check Kubernetes manifests with Polaris
```

---

## GitHub Workflows

### New Workflows to Create

1. **`.github/workflows/security-scan.yml`** (Reusable)
   - Trivy filesystem scan
   - Trivy Kubernetes manifest scan
   - Trivy container image scan (optional)
   - SARIF upload to GitHub Security tab

2. **`.github/workflows/codeql.yml`**
   - CodeQL analysis for Rust
   - Scheduled weekly scans
   - Run on push to main and PRs

3. **`.github/workflows/security-full.yml`**
   - Comprehensive weekly security scan
   - All tools: cargo-deny, gitleaks, trivy, semgrep, kubesec, license-check

### Workflows to Update

1. **`.github/workflows/ci.yml`**
   - Add cargo-deny (replace cargo-audit)
   - Add Gitleaks scan
   - Call security-scan.yml workflow

2. **`.github/workflows/release.yml`**
   - Add Trivy image scan
   - Add SBOM generation (Syft)
   - Add image signing (Cosign)

---

## Configuration Files

### New Files to Create

1. **`.cargo-deny.toml`**
   - Dependency security checks
   - License compliance rules
   - Supply chain security rules

2. **`.gitleaks.toml`** (Optional - uses defaults)
   - Custom secret detection rules
   - Allowlist for false positives

3. **`.github/dependabot.yml`**
   - Cargo dependency updates
   - GitHub Actions updates
   - Docker base image updates

4. **`.semgrep/`** (Optional)
   - Custom security rules
   - Kubernetes operator-specific rules

5. **`.trivyignore`** (Optional)
   - Ignore specific vulnerabilities (with justification)

### Files to Update

1. **`Makefile`**
   - Add all security scanning targets
   - Update `help` target documentation

2. **`README.md`**
   - Document security scanning workflow
   - Add badges for security scans
   - Link to security policy

3. **`.claude/CHANGELOG.md`**
   - Document each phase implementation

4. **`docs/src/development/security.md`** (New)
   - Comprehensive security scanning documentation
   - Tool descriptions and usage
   - Troubleshooting guide

---

## Documentation Requirements

### New Documentation to Create

1. **`docs/src/development/security.md`**
   - Overview of security scanning strategy
   - Tool descriptions and purposes
   - Local development workflow
   - CI/CD integration
   - Troubleshooting common issues
   - Remediation guidelines

2. **`docs/src/development/license-compliance.md`**
   - License policy
   - Allowed and denied licenses
   - License checking workflow
   - How to handle license violations

3. **`SECURITY.md`** (Root directory)
   - Security policy
   - Vulnerability reporting process
   - Supported versions
   - Security contact information

### Documentation to Update

1. **`README.md`**
   - Add "Security" section
   - Add security scanning badges
   - Link to SECURITY.md

2. **`docs/src/development/contributing.md`**
   - Add security scanning to development workflow
   - Document pre-commit hooks
   - Add security checklist to PR template

3. **`.claude/CLAUDE.md`**
   - Add security scanning to PR/Commit Checklist
   - Update "After Modifying Any `.rs` File" section
   - Add security scanning to CI/CD requirements

---

## Success Metrics

### Key Performance Indicators (KPIs)

1. **Vulnerability Detection**
   - Zero HIGH/CRITICAL vulnerabilities in production
   - 100% of vulnerabilities remediated within SLA (7 days for CRITICAL, 30 days for HIGH)
   - Mean time to remediation (MTTR) < 7 days for CRITICAL

2. **Secret Detection**
   - Zero secrets committed to repository
   - 100% of commits scanned by Gitleaks
   - Zero false positives in secret detection (after tuning)

3. **License Compliance**
   - 100% of dependencies have approved licenses
   - Zero GPL/AGPL/SSPL licenses in production
   - License report generated for every release

4. **CI/CD Integration**
   - 100% of PRs pass security scans before merge
   - Security scans complete within 5 minutes (quick scan)
   - Security scans complete within 15 minutes (full scan)

5. **Coverage**
   - 100% of container images scanned
   - 100% of Kubernetes manifests validated
   - 100% of Rust code analyzed by SAST

### Acceptance Criteria

Phase complete when:
- ✅ All tasks completed
- ✅ All tests passing
- ✅ Documentation updated
- ✅ CI/CD workflows passing
- ✅ Security findings triaged and remediated
- ✅ `.claude/CHANGELOG.md` updated

---

## Risk Assessment

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Too many false positives | High | Medium | Tune configurations, add allowlists, document exceptions |
| CI/CD pipeline too slow | Medium | Low | Use quick scans in PRs, full scans weekly/nightly |
| Tool installation failures | Low | Low | Use GitHub Actions with pre-installed tools |
| Security findings block development | High | Medium | Define SLAs, allow warnings for LOW severity |
| License compliance blocks updates | Medium | Low | Define clear license policy, review exceptions |
| Tool maintenance burden | Medium | Medium | Use Dependabot for tool updates, prefer GitHub native tools |

---

## Dependencies

### External Dependencies
- GitHub repository (public or private with Advanced Security)
- GitHub Actions runners
- Rust toolchain (already installed)
- Docker (already installed)
- kubectl (already installed)

### Tool Dependencies
- cargo-deny (Rust)
- Gitleaks (standalone)
- Trivy (standalone)
- Semgrep (Python or GitHub Action)
- Kubesec (standalone)
- Syft (standalone)
- Cosign (standalone)

---

## Timeline

```
Week 1: Foundation
├── Mon-Tue: cargo-deny setup + testing
├── Wed-Thu: Gitleaks setup + pre-commit hooks
└── Fri: Dependabot configuration + documentation

Week 2: SAST Integration
├── Mon-Wed: CodeQL setup + testing
├── Thu: Triage findings + remediation
└── Fri: Documentation + review

Week 3: Container & IaC Scanning
├── Mon-Tue: Trivy setup + Makefile targets
├── Wed: GitHub workflow integration
├── Thu: Scan existing resources + remediation
└── Fri: Documentation + review

Week 4: Advanced SAST & K8s Security
├── Mon-Tue: Semgrep setup + custom rules
├── Wed-Thu: Kubesec setup + validation
└── Fri: Documentation + review

Week 5: License Compliance
├── Mon-Tue: cargo-license setup + policy definition
├── Wed-Thu: CI integration + testing
└── Fri: Documentation + review

Week 6+: Supply Chain Security (Optional)
├── SBOM generation (Syft)
├── Image signing (Cosign)
├── Polaris best practices
└── Final documentation
```

---

## Rollout Plan

### Phase 1-2: Non-Blocking (Weeks 1-2)
- Tools run in CI but don't block merges
- Collect baseline metrics
- Tune configurations to reduce false positives
- Document exceptions and allowlists

### Phase 3: Warning Mode (Week 3)
- Tools fail on CRITICAL severity only
- HIGH severity generates warnings
- Continue tuning and remediation

### Phase 4: Enforcement (Week 4+)
- Tools fail on HIGH and CRITICAL severity
- All findings must be remediated or documented
- Full enforcement in CI/CD

---

## Maintenance Plan

### Weekly
- Review Dependabot PRs and merge updates
- Triage new security findings from scans
- Update allowlists/ignores as needed

### Monthly
- Review and update security tool versions
- Review and update custom Semgrep rules
- Audit security exceptions and remove stale entries
- Generate security report for compliance

### Quarterly
- Review and update license policy
- Audit tool effectiveness (false positive rate, MTTR)
- Evaluate new security tools
- Update security documentation

---

## Open Questions

1. **GitHub Advanced Security**: Do we have access to GitHub Advanced Security features (CodeQL, Secret Scanning)?
2. **SBOM Requirements**: Are SBOM reports required by regulators? If so, what format (SPDX, CycloneDX)?
3. **Image Signing**: Is image signing required for production deployments?
4. **Severity Thresholds**: What severity levels should block CI/CD? (Recommendation: CRITICAL + HIGH)
5. **Exception Process**: What is the approval process for security exceptions?
6. **Audit Frequency**: How often are security audit reports required for compliance?
7. **Tool Budget**: Are there budget constraints for commercial tools (if needed)?

---

## References

- [cargo-deny Documentation](https://embarkstudios.github.io/cargo-deny/)
- [Gitleaks Documentation](https://github.com/gitleaks/gitleaks)
- [Trivy Documentation](https://aquasecurity.github.io/trivy/)
- [Semgrep Documentation](https://semgrep.dev/docs/)
- [CodeQL Documentation](https://codeql.github.com/)
- [Kubesec Documentation](https://kubesec.io/)
- [Syft Documentation](https://github.com/anchore/syft)
- [Cosign Documentation](https://docs.sigstore.dev/cosign/overview/)
- [OWASP Top 10](https://owasp.org/www-project-top-ten/)

---

## Approval

**Created By:** Erick Bourgeois
**Review Required:** Yes
**Approvers:** [TBD]
**Status:** Awaiting Approval

---

## Changelog

| Date | Author | Change |
|------|--------|--------|
| 2026-03-06 | Erick Bourgeois | Initial roadmap created |
