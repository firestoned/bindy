---
name: "[CRITICAL] Enforce Signed Commits"
about: Supply chain security - enforce cryptographic proof of code authorship
title: '[Compliance C-1] Enforce Signed Commits for Supply Chain Integrity'
labels: compliance, critical, security, supply-chain
assignees: ''
---

## Severity: CRITICAL

**Compliance Frameworks:** SOX 404, PCI-DSS 6.4.6, 12.10.6, SLSA Level 2+

## Summary

GitHub Actions workflows do not enforce signed commits or verify commit signatures. This creates a supply chain integrity risk and violates SOX change control requirements.

## Problem

**Location:** `.github/workflows/*.yaml` (all workflows)

**Risk:**
- No cryptographic proof of code authorship (SLSA Level 2+ requirement)
- Malicious actors could commit code without attribution
- Insider threats undetectable
- Fails SOX 404 change control and authorization requirements
- Violates PCI-DSS access control and change management requirements

**Impact:**
- ❌ **SOX 404:** Change control and authorization requirements not met
- ❌ **PCI-DSS 6.4.6:** Code review and approval process lacks verification
- ❌ **SLSA Level 2:** No build provenance for commits
- ❌ **Audit Trail:** Cannot prove who authorized code changes

## Solution

### Phase 1: Enable Branch Protection (Week 1)

1. **Configure GitHub Branch Protection:**
   - Navigate to: Settings → Branches → Branch protection rules
   - Add rule for `main` branch:
     - ☑️ Require signed commits
     - ☑️ Require linear history
     - ☑️ Require pull request reviews before merging (2 approvers)
     - ☑️ Dismiss stale pull request approvals when new commits are pushed

### Phase 2: Add CI/CD Verification (Week 1-2)

2. **Add commit signature verification to all workflows:**

```yaml
# Add to .github/workflows/main.yaml, pr.yaml, release.yaml
jobs:
  verify-commits:
    name: Verify Signed Commits
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4
        with:
          fetch-depth: 0  # Fetch all history for verification

      - name: Verify all commits are signed
        run: |
          # Get list of commits in this push/PR
          COMMITS=$(git log --pretty=format:"%H" origin/main..HEAD)

          UNSIGNED_COMMITS=()
          for COMMIT in $COMMITS; do
            # Check if commit is signed
            if ! git verify-commit "$COMMIT" 2>/dev/null; then
              UNSIGNED_COMMITS+=("$COMMIT")
            fi
          done

          if [ ${#UNSIGNED_COMMITS[@]} -gt 0 ]; then
            echo "ERROR: Found unsigned commits:"
            for COMMIT in "${UNSIGNED_COMMITS[@]}"; do
              git show --no-patch --format="%H %an %ae %s" "$COMMIT"
            done
            echo ""
            echo "All commits must be signed with GPG or SSH keys."
            echo "See: https://docs.github.com/en/authentication/managing-commit-signature-verification"
            exit 1
          fi

          echo "✓ All commits are properly signed"
```

### Phase 3: Developer Documentation (Week 2)

3. **Create `CONTRIBUTING.md` with signing requirements:**

```markdown
## Commit Signing Requirements

All commits to this repository MUST be signed with GPG or SSH keys.

### Setup GPG Signing

1. Generate GPG key:
   ```bash
   gpg --full-generate-key
   ```

2. Configure Git:
   ```bash
   git config --global user.signingkey <KEY_ID>
   git config --global commit.gpgsign true
   ```

3. Add GPG key to GitHub:
   - Copy public key: `gpg --armor --export <KEY_ID>`
   - Add at: https://github.com/settings/keys

### Setup SSH Signing (Alternative)

1. Generate SSH key:
   ```bash
   ssh-keygen -t ed25519 -C "your_email@example.com"
   ```

2. Configure Git:
   ```bash
   git config --global gpg.format ssh
   git config --global user.signingkey ~/.ssh/id_ed25519.pub
   git config --global commit.gpgsign true
   ```

3. Add SSH key to GitHub as signing key

### Verify Your Commits

```bash
git log --show-signature -1
```

Look for "Good signature" or "gpg: Good signature" in output.
```

### Phase 4: Update CI/CD to Require Verification (Week 2)

4. **Make verification a required check:**
   - Add `verify-commits` job as dependency to all workflows
   - Prevent merge if verification fails

```yaml
# Example: Update release workflow
jobs:
  verify-commits:
    # ... verification job from above ...

  build:
    name: Build
    needs: verify-commits  # Fail fast if commits unsigned
    runs-on: ubuntu-latest
    # ... rest of build job ...
```

## Testing Plan

### Manual Testing

1. **Test unsigned commit rejection:**
   ```bash
   # Create test branch with unsigned commit
   git checkout -b test-unsigned
   git config commit.gpgsign false
   echo "test" >> README.md
   git commit -m "test unsigned commit"
   git push origin test-unsigned

   # Create PR - should fail CI
   ```

2. **Test signed commit acceptance:**
   ```bash
   # Create test branch with signed commit
   git checkout -b test-signed
   git config commit.gpgsign true
   echo "test" >> README.md
   git commit -S -m "test signed commit"
   git push origin test-signed

   # Create PR - should pass CI
   ```

### Automated Testing

- [ ] CI/CD workflow fails on unsigned commits
- [ ] CI/CD workflow passes on signed commits
- [ ] Branch protection prevents direct push to main
- [ ] GitHub UI shows "Verified" badge on signed commits

## Documentation Updates

Required documentation:
1. **CONTRIBUTING.md** - Commit signing setup and requirements
2. **docs/development/pr-process.md** - Add signing requirement to PR checklist
3. **README.md** - Add "Commits are signed" badge
4. **docs/advanced/security.md** - Document commit verification process
5. **SECURITY.md** - Add commit signing to security policy

## Success Criteria

- [ ] Branch protection enabled requiring signed commits on `main`
- [ ] CI/CD verification job added to all workflows
- [ ] Verification job fails on unsigned commits
- [ ] Verification job passes on signed commits
- [ ] Documentation complete (CONTRIBUTING.md, SECURITY.md)
- [ ] All active contributors have configured commit signing
- [ ] At least 7 days of signed commits before enforcing
- [ ] No unsigned commits merged to `main` after enforcement date

## Rollout Plan

**Week 1:**
- Monday: Enable CI verification (non-blocking warnings)
- Tuesday-Friday: Notify all contributors, share setup guides
- Weekend: Contributors configure signing

**Week 2:**
- Monday: Make verification blocking in CI
- Wednesday: Enable branch protection (enforcement begins)
- Friday: Audit all commits merged since Monday

**Week 3+:**
- Monitor compliance
- Reject any PRs with unsigned commits
- Quarterly audit of commit signatures

## Migration for Existing Commits

⚠️ **Note:** Existing commits before enforcement date will remain unsigned. This is acceptable as:
- Enforcement is forward-looking only
- Audit trail begins at enforcement date
- Historical commits are part of immutable Git history

**Document in CHANGELOG.md:**
```markdown
## [YYYY-MM-DD] - Commit Signing Enforcement

**Security Enhancement:**

Starting YYYY-MM-DD, all commits must be cryptographically signed with GPG or SSH keys.

- Enforced via branch protection on `main` branch
- CI/CD verification added to all workflows
- Required for SOX 404 and PCI-DSS 6.4.6 compliance
- See CONTRIBUTING.md for setup instructions

**Impact:** Contributors must configure GPG or SSH signing before merging to `main`.
```

## Security Considerations

**Benefits:**
- ✅ Cryptographic proof of commit authorship
- ✅ Prevents commit forgery (cannot impersonate other developers)
- ✅ Audit trail for compliance (SOX, PCI-DSS)
- ✅ Supply chain integrity (SLSA Level 2+)

**Limitations:**
- ⚠️ Signing proves commit was made by key owner, not that code is safe
- ⚠️ Key compromise could allow impersonation
- ⚠️ Additional developer setup required

**Mitigations:**
- Regular key rotation policy (every 2 years)
- Hardware security keys (YubiKey) for high-risk contributors
- Two-person review requirement (prevents single compromised key)

## Compliance Attestation

Once complete, update compliance documentation:

**File:** `docs/compliance/CONTROLS.md`
```markdown
### SOX 404 - Change Control

**Control:** All code changes require cryptographic signature verification

**Implementation:**
- Branch protection requires signed commits (enabled: YYYY-MM-DD)
- CI/CD verifies commit signatures on every build
- Two-person approval required for merge

**Evidence:**
- GitHub branch protection settings
- CI/CD workflow logs showing verification
- Audit log of all commits with signatures

**Status:** ✅ Implemented
```

## Related Issues

- Depends on: None (standalone requirement)
- Blocks: SLSA Level 3 certification
- Related: #TBD (Two-person review enforcement)
- Related: #TBD (Vulnerability scanning in CI/CD)

## References

- GitHub Docs: [Managing commit signature verification](https://docs.github.com/en/authentication/managing-commit-signature-verification)
- SLSA Framework: [Build L2 - Signed provenance](https://slsa.dev/spec/v1.0/levels#build-l2)
- SOX 404: IT General Controls (ITGC) - Change Management
- PCI-DSS v4.0: Requirement 6.4.6 - Review of custom code
