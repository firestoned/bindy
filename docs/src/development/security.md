# Security Requirements for Contributors

This page documents the mandatory security requirements for contributing to Bindy.

## Overview

Bindy operates in a **regulated banking environment** and must comply with:
- **SOX 404** - IT General Controls (Change Management)
- **PCI-DSS v4.0** - Payment Card Industry Data Security Standard
- **SLSA Level 2+** - Supply Chain Levels for Software Artifacts

All contributors must follow these security requirements.

## Cryptographically Signed Commits (MANDATORY)

**All commits MUST be signed with GPG or SSH keys.**

This is not optional. Unsigned commits will be **automatically rejected** by CI/CD.

### Why Commit Signing?

Commit signing provides:

✅ **Non-repudiation**: Cryptographic proof that a specific person authorized the code change
✅ **Authenticity**: Verification that commits haven't been tampered with
✅ **Compliance**: Required for SOX 404 change control audits
✅ **Supply Chain Security**: SLSA Level 2+ requirement for build provenance

### What Happens Without Signing?

- ❌ Your pull request will **fail CI/CD checks**
- ❌ Your commits will be **rejected** by branch protection
- ❌ Your changes **cannot be merged** to main
- ❌ The project **fails compliance audits**

## Setting Up Commit Signing

You have two options: **GPG** (recommended) or **SSH**.

### Option 1: GPG Signing (Recommended)

#### 1. Generate a GPG Key

```bash
gpg --full-generate-key
```

When prompted:
- **Key type**: (1) RSA and RSA (default)
- **Key size**: 4096 bits
- **Expiration**: 2 years (recommended)
- **Real name**: Your full legal name
- **Email**: Your GitHub email address

#### 2. Get Your GPG Key ID

```bash
gpg --list-secret-keys --keyid-format=long
```

Output will look like:
```
sec   rsa4096/ABCD1234ABCD1234 2024-01-01 [SC]
uid   [ultimate] Your Name <your.email@example.com>
```

Your key ID is: `ABCD1234ABCD1234`

#### 3. Configure Git

```bash
# Set your signing key
git config --global user.signingkey ABCD1234ABCD1234

# Enable automatic signing
git config --global commit.gpgsign true
```

#### 4. Add GPG Key to GitHub

1. Export your public key:
   ```bash
   gpg --armor --export ABCD1234ABCD1234
   ```

2. Copy the entire output (including `-----BEGIN PGP PUBLIC KEY BLOCK-----` and `-----END PGP PUBLIC KEY BLOCK-----`)

3. Go to https://github.com/settings/keys

4. Click **"New GPG key"**

5. Paste your public key and save

### Option 2: SSH Signing (Alternative)

#### 1. Generate SSH Key

```bash
ssh-keygen -t ed25519 -C "your.email@example.com"
```

#### 2. Configure Git

```bash
# Tell Git to use SSH for signing
git config --global gpg.format ssh

# Set your signing key
git config --global user.signingkey ~/.ssh/id_ed25519.pub

# Enable automatic signing
git config --global commit.gpgsign true
```

#### 3. Add SSH Key to GitHub

1. Copy your public SSH key:
   ```bash
   cat ~/.ssh/id_ed25519.pub
   ```

2. Go to https://github.com/settings/keys

3. Click **"New SSH key"**

4. Select **"Signing Key"** as the key type

5. Paste your public key and save

## Verifying Your Setup

After configuration, test that signing works:

```bash
# Make a test commit
echo "test" >> README.md
git commit -m "Test signed commit"

# Verify it's signed
git log --show-signature -1
```

You should see:
```
gpg: Good signature from "Your Name <your.email@example.com>"
```

Or for SSH:
```
Good "git" signature for your.email@example.com
```

On GitHub, your commits will show a green **"Verified"** badge.

## Troubleshooting

### "gpg failed to sign the data"

Add to your shell profile (`~/.bashrc`, `~/.zshrc`):
```bash
export GPG_TTY=$(tty)
```

Then reload your shell:
```bash
source ~/.bashrc  # or source ~/.zshrc
```

### "Unverified" Badge on GitHub

Check:
1. ✅ Email in GPG/SSH key matches your GitHub email
2. ✅ Key is added to your GitHub account
3. ✅ Key hasn't expired (for GPG keys)

### Need to Sign Existing Commits?

If you have unsigned commits in your branch:

```bash
# Rebase and sign all commits
git rebase --exec 'git commit --amend --no-edit -S' -i origin/main
```

⚠️ **WARNING**: This rewrites Git history. Only do this on branches you haven't shared with others.

## CI/CD Verification

All workflows automatically verify commit signatures:

```yaml
verify-commits:
  name: Verify Signed Commits
  runs-on: ubuntu-latest
  steps:
    - name: Checkout code
      uses: actions/checkout@v4
      with:
        fetch-depth: 0

    - name: Verify all commits are signed
      run: |
        # Checks every commit in your PR
        # Fails if any commit is unsigned
```

If verification fails, you'll see:
```
ERROR: Found unsigned commits:
abc1234 Your Name <your.email@example.com> Fix bug in reconciler

All commits must be signed with GPG or SSH keys.
See CONTRIBUTING.md for setup instructions.
```

## Branch Protection

The `main` branch has the following protection rules:

- ✅ Require signed commits (enforced by GitHub)
- ✅ Require linear history (no merge commits)
- ✅ Require pull request reviews (2 approvers)
- ✅ Require status checks to pass (CI/CD)
- ❌ No direct pushes to main (even for admins)

## Compliance Evidence

For audit purposes, commit signing provides:

### SOX 404 - Change Control
- **Control**: Cryptographic proof of code authorship
- **Evidence**: Git commit signatures in repository history
- **Audit**: `git log --show-signature` shows all signed commits

### PCI-DSS 6.4.6 - Code Review
- **Control**: Non-repudiable approval of code changes
- **Evidence**: GitHub PR reviews + signed commits
- **Audit**: GitHub audit logs + signature verification logs

### SLSA Level 2 - Build Provenance
- **Control**: Source integrity verification
- **Evidence**: All commits cryptographically signed
- **Audit**: CI/CD workflow logs verify signatures on every build

## Key Rotation

GPG keys should be rotated every **2 years** for security:

1. Generate a new GPG key
2. Add new key to GitHub
3. Update Git config to use new key
4. Revoke old key after transition period
5. Publish revocation certificate

## Security Best Practices

1. ✅ **Protect your private key** - Never share or commit it
2. ✅ **Use a strong passphrase** - Required for GPG keys
3. ✅ **Enable 2FA on GitHub** - Additional security layer
4. ✅ **Keep keys backed up** - Store securely (password manager, hardware key)
5. ✅ **Rotate keys regularly** - Every 2 years for GPG

## Hardware Security Keys

For enhanced security, consider using hardware security keys:

- **YubiKey** - Supports GPG signing with hardware-backed keys
- **Solo Key** - Open-source FIDO2 security key
- **Nitrokey** - Privacy-focused hardware security token

Hardware keys prevent private key theft even if your computer is compromised.

## Additional Security Requirements

### Never Commit

- ❌ Secrets, API keys, tokens, or credentials
- ❌ Private keys or certificates
- ❌ Internal hostnames or IP addresses
- ❌ Customer or transaction data
- ❌ Encryption keys or passwords

### Secret Detection

Pre-commit hooks and CI/CD will scan for secrets:
- GitHub Advanced Security secret scanning
- Custom regex patterns for common secrets
- Build fails if secrets detected

### Dependency Security

All dependencies are scanned with `cargo audit`:
```bash
cargo audit
```

CI/CD fails if vulnerabilities are found. Update dependencies promptly.

## Getting Help

- **Setup Issues**: See [CONTRIBUTING.md](../../../CONTRIBUTING.md)
- **Security Incidents**: Email security@firestoned.io
- **Compliance Questions**: Email compliance@firestoned.io

## References

- [CONTRIBUTING.md](../../../CONTRIBUTING.md) - Full contributing guide
- [SECURITY.md](../../../SECURITY.md) - Security policy
- [GitHub Commit Signing Docs](https://docs.github.com/en/authentication/managing-commit-signature-verification)
- [SLSA Framework](https://slsa.dev/)
