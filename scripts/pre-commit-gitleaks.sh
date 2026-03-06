#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# Gitleaks pre-commit hook
# This hook prevents commits containing secrets from being committed.
#
# To install: ln -sf ../../scripts/pre-commit-gitleaks.sh .git/hooks/pre-commit
# To bypass: git commit --no-verify (use with caution!)

set -e

# Check if gitleaks is installed
if ! command -v gitleaks >/dev/null 2>&1; then
    echo "Error: gitleaks is not installed"
    echo "Install with: brew install gitleaks (macOS)"
    echo "Or see: https://github.com/gitleaks/gitleaks#installing"
    exit 1
fi

# Run gitleaks on staged files
echo "🔍 Scanning staged files for secrets..."
if gitleaks protect --staged --verbose --redact; then
    echo "✓ No secrets detected"
    exit 0
else
    echo ""
    echo "❌ Secret detected in staged files!"
    echo ""
    echo "To fix:"
    echo "  1. Remove the secret from your code"
    echo "  2. Use environment variables or Kubernetes secrets instead"
    echo "  3. Add false positives to .gitleaks.toml allowlist"
    echo ""
    echo "To bypass this check (NOT RECOMMENDED):"
    echo "  git commit --no-verify"
    echo ""
    exit 1
fi
