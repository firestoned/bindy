#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

# Multi-Tenancy Integration Tests Runner
# Run these tests against a local Kubernetes cluster (kind, k3d, minikube, etc.)

set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo "================================================"
echo "  Bindy Multi-Tenancy Integration Tests"
echo "================================================"
echo ""

# Check if kubectl is available
if ! command -v kubectl &> /dev/null; then
    echo -e "${RED}✗ kubectl not found. Please install kubectl.${NC}"
    exit 1
fi

# Check if connected to a cluster
if ! kubectl cluster-info &> /dev/null; then
    echo -e "${RED}✗ Not connected to a Kubernetes cluster.${NC}"
    echo ""
    echo "Please ensure you have a Kubernetes cluster running."
    echo "Examples:"
    echo "  - kind create cluster"
    echo "  - k3d cluster create"
    echo "  - minikube start"
    exit 1
fi

echo -e "${GREEN}✓ Connected to Kubernetes cluster${NC}"
kubectl cluster-info | head -n 1
echo ""

# Check if CRDs are installed
echo "Checking for Bindy CRDs..."
CRD_COUNT=$(kubectl get crd -o name | grep -c "bindy.firestoned.io" || true)

if [ "$CRD_COUNT" -eq 0 ]; then
    echo -e "${YELLOW}⚠ Bindy CRDs not found. Installing...${NC}"

    # Check if deploy/crds directory exists
    if [ ! -d "deploy/crds" ]; then
        echo -e "${RED}✗ deploy/crds directory not found.${NC}"
        echo "Please run this script from the project root directory."
        exit 1
    fi

    # Install CRDs
    echo "Installing CRDs..."
    kubectl create -f deploy/crds/ 2>/dev/null || kubectl replace --force -f deploy/crds/

    echo -e "${GREEN}✓ CRDs installed${NC}"
else
    echo -e "${GREEN}✓ Found $CRD_COUNT Bindy CRDs${NC}"
fi

echo ""
echo "CRDs installed:"
kubectl get crd -o name | grep "bindy.firestoned.io" | sed 's/customresourcedefinition.apiextensions.k8s.io\//  - /'
echo ""

# Run the tests
echo "================================================"
echo "  Running Integration Tests"
echo "================================================"
echo ""

# Test selection
TEST_FILTER="${1:-}"

if [ -z "$TEST_FILTER" ]; then
    echo "Running ALL multi-tenancy integration tests..."
    echo ""
    cargo test --test multi_tenancy_integration -- --ignored --nocapture --test-threads=1
else
    echo "Running tests matching: $TEST_FILTER"
    echo ""
    cargo test --test multi_tenancy_integration "$TEST_FILTER" -- --ignored --nocapture --test-threads=1
fi

EXIT_CODE=$?

echo ""
echo "================================================"

if [ $EXIT_CODE -eq 0 ]; then
    echo -e "${GREEN}✓ All tests passed!${NC}"
else
    echo -e "${RED}✗ Some tests failed.${NC}"
fi

echo "================================================"

# Cleanup test resources
echo ""
echo "Cleaning up test resources..."

# Get all test namespaces
TEST_NAMESPACES=$(kubectl get namespace -l managed-by=bindy-integration-test -o jsonpath='{.items[*].metadata.name}' 2>/dev/null || echo "")

if [ -n "$TEST_NAMESPACES" ]; then
    for NS in $TEST_NAMESPACES; do
        echo "Cleaning up namespace: $NS"

        # Delete resources in reverse dependency order to prevent stuck Terminating state
        # 1. DNSZones (depend on clusters)
        echo -e "${YELLOW}⚠ Deleting dnszones in ${NS}"
        kubectl delete dnszones -n "$NS" --all --ignore-not-found=true --timeout=2s &> /dev/null || true

        # 2. Bind9Instances (depend on clusters)
        echo -e "${YELLOW}⚠ Deleting bind9instances in ${NS}"
        kubectl delete bind9instances -n "$NS" --all --ignore-not-found=true --timeout=2s &> /dev/null || true

        # 3. Bind9Clusters
        echo -e "${YELLOW}⚠ Deleting bind9clusters in ${NS}"
        kubectl delete bind9clusters -n "$NS" --all --ignore-not-found=true --timeout=2s &> /dev/null || true

        # 4. Delete the namespace
        echo -e "${YELLOW}⚠ Deleting namespace ${NS}"
        kubectl delete namespace "$NS" --ignore-not-found=true --timeout=2s &> /dev/null || true
    done

    # 5. Force-delete any stuck namespaces using the force-delete-ns.sh script
    STUCK_NAMESPACES=()
    for NS in $TEST_NAMESPACES; do
        if kubectl get namespace "$NS" &> /dev/null; then
            NS_STATUS=$(kubectl get namespace "$NS" -o jsonpath='{.status.phase}' 2>/dev/null || echo "")
            if [ "$NS_STATUS" = "Terminating" ] || [ -n "$NS_STATUS" ]; then
                STUCK_NAMESPACES+=("$NS")
            fi
        fi
    done

    if [ ${#STUCK_NAMESPACES[@]} -gt 0 ]; then
        echo -e "${YELLOW}⚠ Namespaces stuck in Terminating state, force-deleting...${NC}"
        # Use force-delete-ns.sh script in non-interactive mode
        SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
        yes y 2>/dev/null | "$SCRIPT_DIR/force-delete-ns.sh" "${STUCK_NAMESPACES[@]}" || true
    fi
fi

# Also cleanup any global clusters created by tests
kubectl delete bind9globalclusters -l test=multi-tenancy --ignore-not-found=true --timeout=5s &> /dev/null || true

echo -e "${GREEN}✓ Cleanup complete${NC}"

exit $EXIT_CODE
