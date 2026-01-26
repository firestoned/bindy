#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

# Fix MkDocs broken links - comprehensive link fixing script

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Fixing MkDocs broken links...${NC}"
echo ""

cd docs/src

# Counter for fixes
FIXES=0

# 1. Fix security file references (UPPERCASE_UNDERSCORES -> lowercase-with-hyphens)
echo -e "${YELLOW}1. Fixing security file references...${NC}"

# THREAT_MODEL.md -> threat-model.md
find . -name "*.md" -exec sed -i '' 's|THREAT_MODEL\.md|threat-model.md|g' {} \;
FIXES=$((FIXES + 1))

# ARCHITECTURE.md -> architecture.md
find . -name "*.md" -exec sed -i '' 's|/ARCHITECTURE\.md|/architecture.md|g' {} \;
FIXES=$((FIXES + 1))

# INCIDENT_RESPONSE.md -> incident-response.md
find . -name "*.md" -exec sed -i '' 's|INCIDENT_RESPONSE\.md|incident-response.md|g' {} \;
FIXES=$((FIXES + 1))

# SECRET_ACCESS_AUDIT.md -> secret-access-audit.md
find . -name "*.md" -exec sed -i '' 's|SECRET_ACCESS_AUDIT\.md|secret-access-audit.md|g' {} \;
FIXES=$((FIXES + 1))

# VULNERABILITY_MANAGEMENT.md -> vulnerability-management.md
find . -name "*.md" -exec sed -i '' 's|VULNERABILITY_MANAGEMENT\.md|vulnerability-management.md|g' {} \;
FIXES=$((FIXES + 1))

# BUILD_REPRODUCIBILITY.md -> build-reproducibility.md
find . -name "*.md" -exec sed -i '' 's|BUILD_REPRODUCIBILITY\.md|build-reproducibility.md|g' {} \;
FIXES=$((FIXES + 1))

# AUDIT_LOG_RETENTION.md -> audit-log-retention.md
find . -name "*.md" -exec sed -i '' 's|AUDIT_LOG_RETENTION\.md|audit-log-retention.md|g' {} \;
FIXES=$((FIXES + 1))

# 2. Fix relative paths that go outside docs/src/
echo -e "${YELLOW}2. Fixing relative paths outside docs/src/...${NC}"

# ../../../SECURITY.md -> https://github.com/firestoned/bindy/blob/main/SECURITY.md
find . -name "*.md" -exec sed -i '' 's|../../../SECURITY\.md|https://github.com/firestoned/bindy/blob/main/SECURITY.md|g' {} \;
FIXES=$((FIXES + 1))

# ../../../CHANGELOG.md -> https://github.com/firestoned/bindy/blob/main/CHANGELOG.md
find . -name "*.md" -exec sed -i '' 's|../../../CHANGELOG\.md|https://github.com/firestoned/bindy/blob/main/CHANGELOG.md|g' {} \;
FIXES=$((FIXES + 1))

# ../../../CONTRIBUTING.md -> https://github.com/firestoned/bindy/blob/main/CONTRIBUTING.md
find . -name "*.md" -exec sed -i '' 's|../../../CONTRIBUTING\.md|https://github.com/firestoned/bindy/blob/main/CONTRIBUTING.md|g' {} \;
FIXES=$((FIXES + 1))

# ../../CONTRIBUTING.md -> https://github.com/firestoned/bindy/blob/main/CONTRIBUTING.md
find . -name "*.md" -exec sed -i '' 's|../../CONTRIBUTING\.md|https://github.com/firestoned/bindy/blob/main/CONTRIBUTING.md|g' {} \;
FIXES=$((FIXES + 1))

# ../CONTRIBUTING.md -> https://github.com/firestoned/bindy/blob/main/CONTRIBUTING.md
find . -name "*.md" -exec sed -i '' 's|../CONTRIBUTING\.md|https://github.com/firestoned/bindy/blob/main/CONTRIBUTING.md|g' {} \;
FIXES=$((FIXES + 1))

# ../SECURITY.md -> https://github.com/firestoned/bindy/blob/main/SECURITY.md
find . -name "*.md" -exec sed -i '' 's|../SECURITY\.md|https://github.com/firestoned/bindy/blob/main/SECURITY.md|g' {} \;
FIXES=$((FIXES + 1))

# 3. Fix source code links (../../src -> GitHub source links)
echo -e "${YELLOW}3. Fixing source code links...${NC}"

# ../../src/reconcilers/*.rs -> GitHub links
find . -name "*.md" -exec sed -i '' 's|../../src/reconcilers/|https://github.com/firestoned/bindy/blob/main/src/reconcilers/|g' {} \;
FIXES=$((FIXES + 1))

# ../../src/bind9_resources.rs -> GitHub link
find . -name "*.md" -exec sed -i '' 's|../../src/bind9_resources\.rs|https://github.com/firestoned/bindy/blob/main/src/bind9_resources.rs|g' {} \;
FIXES=$((FIXES + 1))

# ../../../src/reconcilers -> GitHub links
find . -name "*.md" -exec sed -i '' 's|../../../src/reconcilers/|https://github.com/firestoned/bindy/blob/main/src/reconcilers/|g' {} \;
FIXES=$((FIXES + 1))

# 4. Fix .github references
echo -e "${YELLOW}4. Fixing .github references...${NC}"

# ../../../.github/COMPLIANCE_ROADMAP.md -> GitHub link
find . -name "*.md" -exec sed -i '' 's|../../../\.github/COMPLIANCE_ROADMAP\.md|https://github.com/firestoned/bindy/blob/main/.github/COMPLIANCE_ROADMAP.md|g' {} \;
FIXES=$((FIXES + 1))

# 5. Fix deploy/ references
echo -e "${YELLOW}5. Fixing deploy/ references...${NC}"

# ../../../deploy/rbac/ -> GitHub links
find . -name "*.md" -exec sed -i '' 's|../../../deploy/rbac/|https://github.com/firestoned/bindy/blob/main/deploy/rbac/|g' {} \;
FIXES=$((FIXES + 1))

# 6. Fix roadmaps references (../../roadmaps -> ../roadmaps)
echo -e "${YELLOW}6. Fixing roadmaps references...${NC}"

# ../../roadmaps/ -> GitHub links (roadmaps are in docs/roadmaps, not docs/src/roadmaps)
find . -name "*.md" -exec sed -i '' 's|../../roadmaps/|https://github.com/firestoned/bindy/blob/main/docs/roadmaps/|g' {} \;
FIXES=$((FIXES + 1))

# 7. Fix Cargo.toml reference
echo -e "${YELLOW}7. Fixing Cargo.toml reference...${NC}"

# ../../Cargo.toml -> GitHub link
find . -name "*.md" -exec sed -i '' 's|../../Cargo\.toml|https://github.com/firestoned/bindy/blob/main/Cargo.toml|g' {} \;
FIXES=$((FIXES + 1))

# 8. Fix examples/ references
echo -e "${YELLOW}8. Fixing examples/ references...${NC}"

# ../../examples/ -> GitHub links
find . -name "*.md" -exec sed -i '' 's|../../examples/|https://github.com/firestoned/bindy/blob/main/examples/|g' {} \;
FIXES=$((FIXES + 1))

echo ""
echo -e "${GREEN}✓ Applied $FIXES link fix patterns${NC}"
echo ""
echo -e "${YELLOW}Verifying build...${NC}"

cd ..
if poetry run mkdocs build --quiet 2>&1 | grep -q "ERROR"; then
    echo -e "${RED}✗ Build has errors${NC}"
    exit 1
fi

WARNING_COUNT=$(poetry run mkdocs build 2>&1 | grep -c "WARNING" || true)
echo -e "${GREEN}✓ Build succeeded${NC}"
echo -e "${YELLOW}Remaining warnings: $WARNING_COUNT${NC}"

if [ $WARNING_COUNT -lt 50 ]; then
    echo -e "${GREEN}✓ Warning count significantly reduced!${NC}"
else
    echo -e "${YELLOW}⚠ Still many warnings - may need manual review${NC}"
fi

echo ""
echo -e "${GREEN}Link fixing complete!${NC}"
