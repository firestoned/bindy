#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

# Verification script for RBAC least privilege implementation
# Tests that the bindy controller ServiceAccount has minimal required permissions
# and NO delete permissions on any resources (PCI-DSS 7.1.2 compliance)

set -e

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

NAMESPACE="dns-system"
SERVICEACCOUNT="system:serviceaccount:${NAMESPACE}:bindy"

PASSED=0
FAILED=0

echo "========================================"
echo "RBAC Least Privilege Verification"
echo "========================================"
echo ""
echo "Testing ServiceAccount: ${SERVICEACCOUNT}"
echo "Namespace: ${NAMESPACE}"
echo ""

# Function to test permission (should be allowed)
test_allowed() {
    local verb=$1
    local resource=$2
    local namespace_flag=$3

    echo -n "Testing: ${verb} ${resource}... "

    if kubectl auth can-i "${verb}" "${resource}" --as="${SERVICEACCOUNT}" ${namespace_flag} &>/dev/null; then
        echo -e "${GREEN}✓ PASS${NC} (allowed)"
        ((PASSED++))
        return 0
    else
        echo -e "${RED}✗ FAIL${NC} (should be allowed)"
        ((FAILED++))
        return 1
    fi
}

# Function to test permission (should be denied)
test_denied() {
    local verb=$1
    local resource=$2
    local namespace_flag=$3

    echo -n "Testing: ${verb} ${resource}... "

    if kubectl auth can-i "${verb}" "${resource}" --as="${SERVICEACCOUNT}" ${namespace_flag} &>/dev/null; then
        echo -e "${RED}✗ FAIL${NC} (should be denied)"
        ((FAILED++))
        return 1
    else
        echo -e "${GREEN}✓ PASS${NC} (denied as expected)"
        ((PASSED++))
        return 0
    fi
}

echo "========================================"
echo "1. Testing Bind9Instance CRD Permissions"
echo "========================================"
echo ""

# Controller SHOULD be able to read/write Bind9Instance
test_allowed "get" "bind9instances.bindy.firestoned.io"
test_allowed "list" "bind9instances.bindy.firestoned.io"
test_allowed "watch" "bind9instances.bindy.firestoned.io"
test_allowed "create" "bind9instances.bindy.firestoned.io"
test_allowed "update" "bind9instances.bindy.firestoned.io"
test_allowed "patch" "bind9instances.bindy.firestoned.io"

# Controller MUST NOT be able to delete Bind9Instance (CRITICAL)
test_denied "delete" "bind9instances.bindy.firestoned.io"
test_denied "deletecollection" "bind9instances.bindy.firestoned.io"

echo ""
echo "========================================"
echo "2. Testing DNSZone CRD Permissions"
echo "========================================"
echo ""

# Controller SHOULD be able to read/write DNSZone
test_allowed "get" "dnszones.bindy.firestoned.io"
test_allowed "list" "dnszones.bindy.firestoned.io"
test_allowed "watch" "dnszones.bindy.firestoned.io"
test_allowed "create" "dnszones.bindy.firestoned.io"
test_allowed "update" "dnszones.bindy.firestoned.io"
test_allowed "patch" "dnszones.bindy.firestoned.io"

# Controller MUST NOT be able to delete DNSZone (CRITICAL)
test_denied "delete" "dnszones.bindy.firestoned.io"
test_denied "deletecollection" "dnszones.bindy.firestoned.io"

echo ""
echo "========================================"
echo "3. Testing DNS Record CRD Permissions"
echo "========================================"
echo ""

# Test all record types
for record_type in "arecords" "aaaarecords" "cnamerecords" "mxrecords" "txtrecords" "srvrecords" "nsrecords" "ptrrecords" "soarecords"; do
    test_allowed "get" "${record_type}.bindy.firestoned.io"
    test_allowed "update" "${record_type}.bindy.firestoned.io"
    test_denied "delete" "${record_type}.bindy.firestoned.io"
done

echo ""
echo "========================================"
echo "4. Testing Secrets Permissions (CRITICAL)"
echo "========================================"
echo ""

# Controller SHOULD be able to READ secrets (for RNDC keys)
test_allowed "get" "secrets" "--namespace=${NAMESPACE}"
test_allowed "list" "secrets" "--namespace=${NAMESPACE}"
test_allowed "watch" "secrets" "--namespace=${NAMESPACE}"

# Controller MUST NOT be able to modify/delete secrets (PCI-DSS 7.1.2)
test_denied "create" "secrets" "--namespace=${NAMESPACE}"
test_denied "update" "secrets" "--namespace=${NAMESPACE}"
test_denied "patch" "secrets" "--namespace=${NAMESPACE}"
test_denied "delete" "secrets" "--namespace=${NAMESPACE}"

echo ""
echo "========================================"
echo "5. Testing ConfigMap Permissions"
echo "========================================"
echo ""

# Controller SHOULD be able to read/write ConfigMaps (for zone configs)
test_allowed "get" "configmaps" "--namespace=${NAMESPACE}"
test_allowed "list" "configmaps" "--namespace=${NAMESPACE}"
test_allowed "watch" "configmaps" "--namespace=${NAMESPACE}"
test_allowed "create" "configmaps" "--namespace=${NAMESPACE}"
test_allowed "update" "configmaps" "--namespace=${NAMESPACE}"
test_allowed "patch" "configmaps" "--namespace=${NAMESPACE}"

# Controller MUST NOT be able to delete ConfigMaps (least privilege)
test_denied "delete" "configmaps" "--namespace=${NAMESPACE}"

echo ""
echo "========================================"
echo "6. Testing Deployment Permissions"
echo "========================================"
echo ""

# Controller SHOULD be able to manage Deployments (for BIND pods)
test_allowed "get" "deployments" "--namespace=${NAMESPACE}"
test_allowed "list" "deployments" "--namespace=${NAMESPACE}"
test_allowed "watch" "deployments" "--namespace=${NAMESPACE}"
test_allowed "create" "deployments" "--namespace=${NAMESPACE}"
test_allowed "update" "deployments" "--namespace=${NAMESPACE}"
test_allowed "patch" "deployments" "--namespace=${NAMESPACE}"

# Controller MUST NOT be able to delete Deployments (least privilege)
test_denied "delete" "deployments" "--namespace=${NAMESPACE}"

echo ""
echo "========================================"
echo "7. Testing Service Permissions"
echo "========================================"
echo ""

# Controller SHOULD be able to manage Services
test_allowed "get" "services" "--namespace=${NAMESPACE}"
test_allowed "list" "services" "--namespace=${NAMESPACE}"
test_allowed "watch" "services" "--namespace=${NAMESPACE}"
test_allowed "create" "services" "--namespace=${NAMESPACE}"
test_allowed "update" "services" "--namespace=${NAMESPACE}"
test_allowed "patch" "services" "--namespace=${NAMESPACE}"

# Controller MUST NOT be able to delete Services (least privilege)
test_denied "delete" "services" "--namespace=${NAMESPACE}"

echo ""
echo "========================================"
echo "8. Testing ServiceAccount Permissions"
echo "========================================"
echo ""

# Controller SHOULD be able to manage ServiceAccounts
test_allowed "get" "serviceaccounts" "--namespace=${NAMESPACE}"
test_allowed "list" "serviceaccounts" "--namespace=${NAMESPACE}"
test_allowed "watch" "serviceaccounts" "--namespace=${NAMESPACE}"
test_allowed "create" "serviceaccounts" "--namespace=${NAMESPACE}"
test_allowed "update" "serviceaccounts" "--namespace=${NAMESPACE}"
test_allowed "patch" "serviceaccounts" "--namespace=${NAMESPACE}"

# Controller MUST NOT be able to delete ServiceAccounts (least privilege)
test_denied "delete" "serviceaccounts" "--namespace=${NAMESPACE}"

echo ""
echo "========================================"
echo "9. Testing Pod Permissions"
echo "========================================"
echo ""

# Controller SHOULD be able to read Pods (for status)
test_allowed "get" "pods" "--namespace=${NAMESPACE}"
test_allowed "list" "pods" "--namespace=${NAMESPACE}"
test_allowed "watch" "pods" "--namespace=${NAMESPACE}"

# Controller MUST NOT be able to delete Pods (managed by Deployments)
test_denied "delete" "pods" "--namespace=${NAMESPACE}"

echo ""
echo "========================================"
echo "RESULTS"
echo "========================================"
echo ""
echo -e "Total tests: $((PASSED + FAILED))"
echo -e "${GREEN}Passed: ${PASSED}${NC}"
echo -e "${RED}Failed: ${FAILED}${NC}"
echo ""

if [ ${FAILED} -eq 0 ]; then
    echo -e "${GREEN}✓ ALL TESTS PASSED${NC}"
    echo ""
    echo "RBAC configuration follows least privilege principle:"
    echo "  - Controller has NO delete permissions on any resources"
    echo "  - Secrets are READ-ONLY (PCI-DSS 7.1.2 compliant)"
    echo "  - Destructive operations require bindy-admin-role"
    echo ""
    exit 0
else
    echo -e "${RED}✗ SOME TESTS FAILED${NC}"
    echo ""
    echo "RBAC configuration does NOT meet least privilege requirements."
    echo "Review deploy/rbac/role.yaml and ensure:"
    echo "  - NO 'delete' verbs on any resources"
    echo "  - Secrets have ONLY: get, list, watch"
    echo "  - Admin operations use bindy-admin-role (NOT controller role)"
    echo ""
    exit 1
fi
