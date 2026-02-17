# Dependency License Policy

**Status:** ✅ Enforced (M-2)
**Compliance:** Basel III Legal Risk, SOX 404 IT Controls
**Last Updated:** 2025-12-18
**Owner:** Legal + Engineering

---

## Overview

This document defines Bindy's policy for acceptable software licenses in dependencies. All Rust dependencies (`Cargo.toml`) must use approved open-source licenses to avoid legal conflicts with proprietary code and ensure compliance with banking regulations.

**Policy Enforcement:**
- ✅ Automated license scanning (GitHub Actions) on every PR
- ✅ Builds fail on unapproved licenses (GPL, LGPL, AGPL, etc.)
- ✅ Quarterly license review (Q1, Q2, Q3, Q4)

---

## Approved Licenses

The following licenses are **pre-approved** for use in Bindy dependencies:

| License | Type | Commercial Use | Distribution | Rationale |
|---------|------|----------------|--------------|-----------|
| **MIT** | Permissive | ✅ Yes | ✅ Yes | Industry standard, no restrictions |
| **Apache-2.0** | Permissive | ✅ Yes | ✅ Yes | Patent grant, Rust ecosystem standard |
| **BSD-3-Clause** | Permissive | ✅ Yes | ✅ Yes | Permissive, widely used |
| **BSD-2-Clause** | Permissive | ✅ Yes | ✅ Yes | Even more permissive than BSD-3 |
| **ISC** | Permissive | ✅ Yes | ✅ Yes | Similar to MIT, OpenBSD preferred |
| **0BSD** | Permissive | ✅ Yes | ✅ Yes | Public domain-equivalent |
| **Unlicense** | Public Domain | ✅ Yes | ✅ Yes | Public domain dedication |
| **CC0-1.0** | Public Domain | ✅ Yes | ✅ Yes | Creative Commons public domain |
| **Zlib** | Permissive | ✅ Yes | ✅ Yes | Short and simple, game dev standard |

**Dual-Licensed Dependencies:**
- ✅ **Allowed** if at least one license is approved (e.g., `MIT OR Apache-2.0`)
- ❌ **Not allowed** if all licenses are unapproved (e.g., `GPL-3.0 OR AGPL-3.0`)

---

## Unapproved Licenses (Copyleft)

The following licenses are **NOT APPROVED** due to copyleft restrictions:

| License | Type | Why Unapproved |
|---------|------|----------------|
| **GPL-2.0** | Strong Copyleft | Requires Bindy source code to be GPL (conflicts with proprietary bank code) |
| **GPL-3.0** | Strong Copyleft | Same as GPL-2.0 + anti-tivoization clauses |
| **LGPL-2.1** | Weak Copyleft | Allows linking but requires source disclosure on modification |
| **LGPL-3.0** | Weak Copyleft | Same as LGPL-2.1 with additional restrictions |
| **AGPL-3.0** | Network Copyleft | Requires source disclosure for network services (DNS is a network service!) |
| **SSPL** | Commercial License | MongoDB Server Side Public License (not OSI-approved) |
| **BUSL-1.1** | Commercial License | Business Source License (converts to open source after time limit) |
| **CC-BY-SA** | Share-Alike | Creative Commons with copyleft (for documentation/assets only, not code) |

**Why Copyleft is Problematic:**
- **GPL/AGPL**: Forces all code that links with GPL code to also be GPL (viral licensing)
- **Banking Conflict**: Banks have proprietary code that cannot be open-sourced
- **Legal Risk**: Violating GPL could result in copyright infringement lawsuits
- **Audit Complexity**: Requires legal review for every GPL dependency

---

## License Scanning Process

### Automated Scanning (GitHub Actions)

Every pull request and commit to `main` triggers automated license scanning:

```bash
# Install cargo-license
cargo install cargo-license --locked

# Generate license report
cargo license --json > licenses.json

# Check for unapproved licenses
cat licenses.json | jq -r '.[] | select(.license | contains("GPL")) | .name'
```

**Workflow:** `.github/workflows/license-scan.yaml`

**Enforcement:**
- ❌ **PR FAILS** if unapproved licenses detected (GPL, LGPL, AGPL, etc.)
- ⚠️ **PR WARNS** if unknown licenses detected (missing license metadata)
- ✅ **PR PASSES** if all licenses are approved

---

### Quarterly License Review

**Schedule:** First day of Q1 (Jan 1), Q2 (Apr 1), Q3 (Jul 1), Q4 (Oct 1)

**Process:**
1. **Run license scan** (automated via GitHub Actions scheduled workflow)
2. **Generate license report** (uploaded as GitHub artifact: `license-report.md`)
3. **Legal review** (Legal team reviews report for new dependencies)
4. **Document exceptions** (If any GPL dependencies are required, document in `CHANGELOG.md`)
5. **File report** (Save report in `docs/compliance/license-reviews/YYYY-QN.md`)

**Deliverable:** Quarterly license review report signed by Legal team

---

## Handling Unapproved Licenses

If a dependency with an unapproved license is required:

### Option 1: Find Alternative Dependency (Preferred)

```bash
# Search for alternatives on crates.io
cargo search <package-name>

# Check license of alternatives
cargo license --json | jq -r '.[] | select(.name == "<alternative>") | .license'
```

**Example:**
- ❌ **GPL library** `gpl-http-client`
- ✅ **MIT alternative** `reqwest` (Apache-2.0 / MIT)

---

### Option 2: Request Legal Exception

If no alternative exists, request a legal exception:

**Legal Exception Request Template:**

```markdown
# Legal Exception Request: GPL Dependency

**Date:** 2025-12-18
**Requester:** [Engineer Name]
**Package:** `gpl-required-library`
**Version:** `1.2.3`
**License:** GPL-3.0
**Repository:** https://github.com/example/gpl-required-library

**Why Needed:**
This library provides [specific functionality] that has no MIT/Apache alternative.

**Usage:**
- Build-time dependency only (not distributed with binary)
- Used for [specific purpose]
- No GPL code included in Bindy binary

**Alternatives Considered:**
1. **Alternative A** - Rejected because [reason]
2. **Alternative B** - Rejected because [reason]
3. **Rewrite from scratch** - Estimated 4 weeks of effort

**Legal Review:**
- [ ] Reviewed by Legal team
- [ ] Approved with conditions: [conditions]
- [ ] Documented in `CHANGELOG.md` and compliance roadmap

**Approval:**
- Legal: [Signature] Date: [Date]
- Engineering Manager: [Signature] Date: [Date]
```

**Process:**
1. Submit exception request to Legal team
2. Legal reviews GPL license terms and usage
3. If approved, document exception in `CHANGELOG.md`
4. Add exception to license scan allow-list (if needed)

---

## Unknown or Missing Licenses

If a dependency has `UNKNOWN` or missing license metadata:

```bash
# Check package on crates.io
open "https://crates.io/crates/<package-name>"

# Check GitHub repository
cargo metadata --format-version 1 | \
  jq -r '.packages[] | select(.name == "<package-name>") | .repository'

# Check LICENSE file in source
git clone <repository-url>
cat LICENSE
```

**Actions:**
1. **Verify license** in package repository (GitHub, crates.io)
2. **Update metadata** (submit PR to upstream if license is missing)
3. **Document license** in `CHANGELOG.md` until upstream is fixed
4. **Consider alternative** if license cannot be determined

---

## Compliance Evidence for Auditors

### Basel III Legal Risk

**Requirement:** Banks must manage legal and compliance risks in third-party software.

**Evidence:**
- Automated license scanning (GitHub Actions workflow)
- Quarterly license review reports
- Legal exception requests (if any)
- Approved license list (this document)

### SOX 404 IT General Controls

**Requirement:** Change management must prevent introduction of unlicensed or improperly licensed code.

**Evidence:**
- License scan runs on every PR (CI/CD gate)
- PRs fail if unapproved licenses detected
- License report uploaded as artifact (90-day retention)

---

## License Report Example

**Sample Output from `cargo license`:**

```
| Package | Version | License | Repository |
|---------|---------|---------|------------|
| anyhow | 1.0.75 | MIT OR Apache-2.0 | https://github.com/dtolnay/anyhow |
| axum | 0.7.2 | MIT | https://github.com/tokio-rs/axum |
| kube | 0.87.1 | Apache-2.0 | https://github.com/kube-rs/kube |
| serde | 1.0.193 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| tokio | 1.35.0 | MIT | https://github.com/tokio-rs/tokio |
| tracing | 0.1.40 | MIT | https://github.com/tokio-rs/tracing |
```

✅ **All licenses approved** (MIT, Apache-2.0, or dual-licensed)

---

## See Also

- [License Scan Workflow](../../.github/workflows/license-scan.yaml) - Automated license scanning
- [Compliance Roadmap](../compliance/overview.md) - M-2: Dependency License Scanning
- [SECURITY.md](../../SECURITY.md) - Dependency management policy
- [Basel III Compliance](../compliance/basel-iii.md) - Legal risk management
