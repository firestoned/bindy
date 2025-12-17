# Contributing to Bindy

Thank you for your interest in contributing to Bindy! This document provides guidelines and requirements for contributing to this project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Commit Signing Requirements](#commit-signing-requirements)
- [Development Workflow](#development-workflow)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)

## Code of Conduct

This project operates in a professional banking/finance environment. All contributors are expected to:

- Maintain professional communication
- Respect security and compliance requirements
- Follow established coding standards
- Document all changes thoroughly

## Getting Started

### Prerequisites

- Rust 1.70 or later
- Docker (for integration tests)
- kubectl (for Kubernetes integration)
- GPG or SSH keys configured for commit signing (see below)

### Local Development Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/firestoned/bindy.git
   cd bindy
   ```

2. Install development dependencies:
   ```bash
   cargo install cargo-audit
   cargo install mdbook mdbook-mermaid
   ```

3. Set up commit signing (REQUIRED - see next section)

4. Build the project:
   ```bash
   cargo build
   ```

5. Run tests:
   ```bash
   cargo test
   ```

## Commit Signing Requirements

**CRITICAL: All commits to this repository MUST be cryptographically signed with GPG or SSH keys.**

This is a mandatory security requirement for:
- **SOX 404** - Change control and authorization compliance
- **PCI-DSS 6.4.6** - Code review and approval process verification
- **SLSA Level 2+** - Supply chain security and build provenance

### Why Commit Signing?

Commit signing provides:
- ✅ Cryptographic proof of commit authorship
- ✅ Prevention of commit forgery (cannot impersonate other developers)
- ✅ Audit trail for regulatory compliance (SOX, PCI-DSS)
- ✅ Supply chain integrity (SLSA Level 2+)

### Option 1: GPG Signing (Recommended)

#### 1. Generate a GPG Key

```bash
# Generate a new GPG key
gpg --full-generate-key

# When prompted:
# - Key type: (1) RSA and RSA (default)
# - Key size: 4096 bits
# - Expiration: 2 years (recommended)
# - Real name: Your full name
# - Email: Your GitHub email address
```

#### 2. Get Your GPG Key ID

```bash
# List your GPG keys
gpg --list-secret-keys --keyid-format=long

# Output will look like:
# sec   rsa4096/ABCD1234ABCD1234 2024-01-01 [SC]
#       1234567890ABCDEF1234567890ABCDEF12345678
# uid                 [ultimate] Your Name <your.email@example.com>

# Your key ID is: ABCD1234ABCD1234
```

#### 3. Configure Git to Sign Commits

```bash
# Set your GPG signing key
git config --global user.signingkey ABCD1234ABCD1234

# Enable commit signing by default
git config --global commit.gpgsign true

# (Optional) Enable tag signing by default
git config --global tag.gpgsign true
```

#### 4. Export Your Public GPG Key

```bash
# Export your public key
gpg --armor --export ABCD1234ABCD1234
```

#### 5. Add GPG Key to GitHub

1. Copy the output from the export command (including `-----BEGIN PGP PUBLIC KEY BLOCK-----` and `-----END PGP PUBLIC KEY BLOCK-----`)
2. Go to https://github.com/settings/keys
3. Click "New GPG key"
4. Paste your public key
5. Click "Add GPG key"

### Option 2: SSH Signing (Alternative)

#### 1. Generate an SSH Key

```bash
# Generate a new SSH key (if you don't have one)
ssh-keygen -t ed25519 -C "your.email@example.com"

# When prompted:
# - File location: Press Enter for default (~/.ssh/id_ed25519)
# - Passphrase: Enter a strong passphrase
```

#### 2. Configure Git to Use SSH Signing

```bash
# Tell Git to use SSH for signing
git config --global gpg.format ssh

# Set your SSH signing key
git config --global user.signingkey ~/.ssh/id_ed25519.pub

# Enable commit signing by default
git config --global commit.gpgsign true
```

#### 3. Add SSH Key to GitHub as Signing Key

1. Copy your public SSH key:
   ```bash
   cat ~/.ssh/id_ed25519.pub
   ```

2. Go to https://github.com/settings/keys
3. Click "New SSH key"
4. Select "Signing Key" as the key type
5. Paste your public key
6. Click "Add SSH key"

### Verify Your Commits Are Signed

After configuring commit signing, verify it's working:

```bash
# Make a test commit
echo "test" >> README.md
git commit -m "Test signed commit"

# Verify the commit is signed
git log --show-signature -1
```

You should see output like:
```
gpg: Good signature from "Your Name <your.email@example.com>"
```

Or for SSH:
```
Good "git" signature for your.email@example.com with ED25519 key SHA256:...
```

### Troubleshooting

#### GPG "failed to sign the data" Error

If you see this error:
```bash
error: gpg failed to sign the data
fatal: failed to write commit object
```

Try:
```bash
# Set GPG_TTY environment variable
export GPG_TTY=$(tty)

# Add to your shell profile (~/.bashrc, ~/.zshrc, etc.)
echo 'export GPG_TTY=$(tty)' >> ~/.bashrc
```

#### GitHub Shows "Unverified" Badge

1. Ensure the email in your GPG/SSH key matches your GitHub email
2. Verify the key is added to your GitHub account
3. Check that the key hasn't expired

#### Signing Existing Commits

If you have unsigned commits in a branch:
```bash
# Rebase and sign all commits
git rebase --exec 'git commit --amend --no-edit -S' -i origin/main
```

**WARNING**: This rewrites history. Only do this on branches that haven't been pushed or shared.

## Development Workflow

### Before Making Changes

1. Create a new branch from `main`:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Ensure you're up to date:
   ```bash
   git pull origin main
   ```

### Making Changes

1. Make your changes following the [Coding Standards](#coding-standards)

2. Ensure all tests pass:
   ```bash
   cargo test
   cargo clippy -- -D warnings
   cargo fmt
   ```

3. Update documentation as needed (see `CLAUDE.md` for documentation requirements)

4. Add your changes and commit (will be automatically signed):
   ```bash
   git add .
   git commit -m "Brief description of changes"
   ```

5. Push your branch:
   ```bash
   git push origin feature/your-feature-name
   ```

### CI/CD Verification

All pull requests will automatically verify:
- ✅ All commits are cryptographically signed
- ✅ Code formatting (cargo fmt)
- ✅ Linting (cargo clippy)
- ✅ All tests pass
- ✅ Documentation builds successfully
- ✅ Security audit (cargo audit)

**If commit verification fails, your PR will be blocked.** Ensure all commits are signed before pushing.

## Pull Request Process

### Before Submitting

1. Ensure all commits in your branch are signed
2. Verify all CI checks pass locally
3. Update `CHANGELOG.md` with your changes
4. Update relevant documentation

### Submitting a Pull Request

1. Go to https://github.com/firestoned/bindy/pulls
2. Click "New pull request"
3. Select your branch
4. Fill out the PR template completely
5. Request review from at least 2 maintainers

### PR Requirements

- [ ] All commits are cryptographically signed (verified by CI)
- [ ] All CI checks pass
- [ ] Code follows Rust style guidelines
- [ ] Tests added/updated for changes
- [ ] Documentation updated
- [ ] `CHANGELOG.md` updated with author attribution
- [ ] At least 2 approving reviews

### Merge Requirements

- ✅ All commits signed and verified
- ✅ 2+ approving reviews from maintainers
- ✅ All CI/CD checks passing
- ✅ No merge conflicts with `main`
- ✅ Branch protection rules satisfied

## Coding Standards

### Rust Code Guidelines

See `CLAUDE.md` for comprehensive coding standards. Key requirements:

- Use `cargo fmt` for formatting
- Pass `cargo clippy -- -D warnings` with no warnings
- Write rustdoc comments for all public items
- Include unit tests for all new code
- Follow early return/guard clause pattern
- No magic numbers (define constants)
- No `.unwrap()` in production code

### Documentation Requirements

- Update rustdoc for all code changes
- Update user-facing documentation in `/docs/src/`
- Include examples for new features
- Update `CHANGELOG.md` with author attribution

### Testing Requirements

- Unit tests in separate `*_tests.rs` files
- Integration tests in `/tests/` directory
- Test both success and failure paths
- Mock external dependencies

### Enhancement Requirements

**CRITICAL: All new enhancements and features MUST meet 100% test coverage and documentation standards.**

When submitting a pull request for a new enhancement or feature, you MUST provide:

1. **100% Unit Test Coverage**
   - Every new function MUST have corresponding unit tests
   - All code paths (success and failure) MUST be tested
   - Edge cases and boundary conditions MUST be covered
   - Tests MUST be in separate `*_tests.rs` files (see `CLAUDE.md`)
   - Run `cargo tarpaulin` to verify coverage (if available)

2. **100% Integration Test Coverage**
   - End-to-end workflows MUST be tested in `/tests/` directory
   - Multi-resource interactions MUST be tested
   - Kubernetes API interactions MUST be tested with mocks
   - Cleanup and finalizer logic MUST be tested
   - State transitions MUST be verified

3. **Complete Documentation**
   - **Rustdoc**: All public functions, types, and modules MUST have comprehensive rustdoc comments
   - **User Documentation**: Feature documentation MUST be added to `/docs/src/features/`
   - **API Documentation**: CRD changes MUST regenerate API docs (`cargo run --bin crddoc`)
   - **Examples**: Working YAML examples MUST be added to `/examples/`
   - **Architecture Diagrams**: Complex features MUST include Mermaid diagrams showing flow
   - **Changelog**: Entry MUST be added to `CHANGELOG.md` with author attribution
   - **README**: Feature list MUST be updated if it's a user-facing feature
   - **Troubleshooting**: Common issues and solutions MUST be documented

**Verification Checklist for Enhancements:**

Before submitting an enhancement PR, verify:

- [ ] **Unit Tests**: All new code has unit tests with 100% coverage
- [ ] **Integration Tests**: End-to-end workflows are tested
- [ ] **Rustdoc**: All public items have comprehensive documentation
- [ ] **User Docs**: Feature guide added to `/docs/src/features/`
- [ ] **Examples**: Working YAML examples added and validated
- [ ] **Architecture**: Diagrams added for complex logic
- [ ] **Changelog**: Entry added with author name
- [ ] **README**: Feature list updated if user-facing
- [ ] **Tests Pass**: `cargo test` succeeds with no failures
- [ ] **Code Quality**: `cargo clippy -- -D warnings` passes with no warnings
- [ ] **Formatting**: `cargo fmt` applied
- [ ] **Documentation Builds**: `make docs` completes successfully

**PRs for enhancements that do not meet these requirements will be rejected.**

This is not negotiable - comprehensive testing and documentation are critical requirements in our regulated banking environment for auditability, maintainability, and compliance.

## Security and Compliance

### Never Commit

- ❌ Secrets, tokens, or credentials (even examples)
- ❌ Internal hostnames or IP addresses
- ❌ Customer or transaction data in any form
- ❌ Private keys or certificates

### Compliance Requirements

This codebase operates in a regulated banking environment. All changes must be:
- Auditable with clear documentation
- Traceable to a business or technical requirement
- Compliant with zero-trust security principles

## Questions or Issues?

- For questions about contributing: Open a GitHub issue
- For security vulnerabilities: See `SECURITY.md`
- For feature requests: Open a GitHub issue with the "enhancement" label

## License

By contributing to this project, you agree that your contributions will be licensed under the MIT License.
