#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
# Validate all example YAML files against CRD schemas
# This script ensures examples stay in sync with CRD definitions

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
EXAMPLES_DIR="$PROJECT_ROOT/examples"

echo "🔍 Validating examples against CRD schemas..."
echo

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Track validation results
TOTAL=0
PASSED=0
FAILED=0

# Validate each YAML file
for file in "$EXAMPLES_DIR"/*.yaml; do
    if [ -f "$file" ]; then
        TOTAL=$((TOTAL + 1))
        filename=$(basename "$file")

        echo -n "Validating $filename... "

        if kubectl apply --dry-run=client -f "$file" > /dev/null 2>&1; then
            echo -e "${GREEN}✓ PASS${NC}"
            PASSED=$((PASSED + 1))
        else
            echo -e "${RED}✗ FAIL${NC}"
            FAILED=$((FAILED + 1))
            echo -e "${YELLOW}Error details:${NC}"
            kubectl apply --dry-run=client -f "$file" 2>&1 | sed 's/^/  /'
            echo
        fi
    fi
done

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Results: $PASSED/$TOTAL passed"

if [ $FAILED -gt 0 ]; then
    echo -e "${RED}❌ $FAILED example(s) failed validation${NC}"
    echo
    echo "To fix:"
    echo "1. Check the error messages above"
    echo "2. Update examples to match CRD schemas in deploy/crds/"
    echo "3. Run: cargo run --bin crdgen (if CRDs changed)"
    exit 1
else
    echo -e "${GREEN}✅ All examples are valid!${NC}"
    exit 0
fi
