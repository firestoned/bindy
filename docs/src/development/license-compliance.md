# License Compliance

Bindy enforces strict license compliance for all dependencies to meet regulatory requirements
in banking and financial services environments.

## Policy

### Allowed Licenses

The following licenses are approved for use in Bindy dependencies:

| License | Category |
|---------|----------|
| MIT | Permissive |
| Apache-2.0 | Permissive |
| BSD-2-Clause | Permissive |
| BSD-3-Clause | Permissive |
| ISC | Permissive |
| 0BSD | Permissive |
| Zlib | Permissive |
| Unicode-3.0 | Permissive |
| OpenSSL | Permissive |
| MPL-2.0 | Weak Copyleft (file-level) |
| CDLA-Permissive-2.0 | Permissive (data) |

### Prohibited Licenses

The following licenses are **not permitted** and will cause CI to fail:

| License | Reason |
|---------|--------|
| GPL-2.0, GPL-3.0 | Strong copyleft — would infect all linked code |
| AGPL-3.0 | Network-copyleft — incompatible with SaaS deployment |
| LGPL-2.0, LGPL-2.1, LGPL-3.0 | Weak copyleft — requires dynamic linking or source release |
| SSPL-1.0 | Service copyleft — incompatible with commercial use |
| EUPL-1.1, EUPL-1.2 | Strong copyleft — EU public license |
| CDDL-1.0 | Weak copyleft — incompatible with Apache-2.0 |

## Enforcement

License compliance is enforced at two levels:

### 1. cargo-deny (Policy Enforcement)

`cargo-deny` blocks builds if any dependency has a prohibited license. This is the primary
enforcement gate and runs on every PR and push to main.

Configuration: [`.cargo/deny.toml`](../../../.cargo/deny.toml)

```bash
make cargo-deny
```

### 2. cargo-license (Reporting)

`cargo-license` generates human-readable reports of all dependency licenses. Used for
compliance audits and release artifacts.

```bash
# Check for violations (exits non-zero if prohibited licenses found)
make license-check

# Generate full JSON report (outputs licenses.json)
make license-report
```

## CI Integration

License compliance is checked on every pull request in the `security` job of
[`.github/workflows/pr.yaml`](../../../.github/workflows/pr.yaml):

1. **`make cargo-deny`** — Blocks the build on prohibited licenses (strict enforcement)
2. **`make license-check`** — Reports any violations not caught by cargo-deny

License reports are generated as release artifacts in the release workflow.

## Adding a New Dependency

Before adding a new crate:

1. Check the license on [crates.io](https://crates.io) or the crate's repository
2. Verify the license is in the **Allowed** list above
3. If the license is not listed, raise it for review before adding the dependency
4. After adding, run `make license-check` locally to confirm compliance

```bash
# Check licenses after adding a dependency
make license-check

# View full license breakdown
make license-report
```

## Handling Exceptions

If a required dependency has a non-approved license, escalate to the security team.
Exceptions require:

1. Written justification for why the dependency is necessary
2. Legal review and approval
3. Documentation in `.cargo/deny.toml` under the `[licenses.exceptions]` section
4. Entry in `.claude/CHANGELOG.md` with the approver's name

## Compliance References

- **PCI-DSS 6.3**: Protect web-facing applications and identify security vulnerabilities
- **SOX IT Controls**: Third-party software risk management
- **NIST SP 800-53 SA-4**: Acquisition process — evaluate license terms for security risk
