#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

set -euo pipefail

# Usage: integration_test.sh [--image IMAGE_TAG] [--skip-deploy]
# Examples:
#   integration_test.sh                                    # Use local deployment
#   integration_test.sh --image main-2025.01.01-123       # Use specific image from registry
#   integration_test.sh --skip-deploy                      # Skip cluster/controller setup

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

NAMESPACE="dns-system"
CLUSTER_NAME="bindy-test"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
IMAGE_TAG=""
SKIP_DEPLOY=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --image)
            IMAGE_TAG="$2"
            shift 2
            ;;
        --skip-deploy)
            SKIP_DEPLOY=true
            shift
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo "Usage: $0 [--image IMAGE_TAG] [--skip-deploy]"
            exit 1
            ;;
    esac
done

echo -e "${BLUE}🧪 Running Bindy Integration Tests${NC}"
echo ""

if [ "$SKIP_DEPLOY" = false ]; then
    # Check if cluster exists
    if ! kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
        echo -e "${YELLOW}⚠️  Cluster '${CLUSTER_NAME}' not found${NC}"
        echo -e "${YELLOW}📦 Creating Kind cluster...${NC}"

        # Create cluster without deploying controller if IMAGE_TAG is specified
        kind create cluster --name "${CLUSTER_NAME}" --config "${PROJECT_ROOT}/deploy/kind-config.yaml" || {
            echo -e "${RED}❌ Failed to create cluster${NC}"
            exit 1
        }

        kubectl config use-context "kind-${CLUSTER_NAME}"

        # Install CRDs and RBAC
        echo -e "${GREEN}📋 Installing CRDs...${NC}"
        kubectl create namespace "${NAMESPACE}" --dry-run=client -o yaml | kubectl apply -f -
        kubectl apply -f "${PROJECT_ROOT}/deploy/crds/"

        echo -e "${GREEN}🔐 Creating RBAC...${NC}"
        kubectl apply -f "${PROJECT_ROOT}/deploy/rbac/"

        if [ -z "$IMAGE_TAG" ]; then
            # No image tag specified, build and deploy locally
            echo -e "${GREEN}🏗️  Building Docker image...${NC}"
            docker build -t bindy:latest "${PROJECT_ROOT}"

            echo -e "${GREEN}📤 Loading image into Kind...${NC}"
            kind load docker-image bindy:latest --name "${CLUSTER_NAME}"

            echo -e "${GREEN}🚀 Deploying controller...${NC}"
            kubectl apply -f "${PROJECT_ROOT}/deploy/operator/deployment.yaml"
        else
            # Image tag specified, pull from registry
            echo -e "${YELLOW}📦 Deploying controller with image: ${IMAGE_TAG}${NC}"
            sed "s|ghcr.io/firestoned/bindy:latest|ghcr.io/${GITHUB_REPOSITORY:-firestoned/bindy}:${IMAGE_TAG}|g" \
                "${PROJECT_ROOT}/deploy/operator/deployment.yaml" | kubectl apply -f -
        fi

        echo -e "${GREEN}⏳ Waiting for controller to be ready...${NC}"
        kubectl wait --for=condition=available --timeout=300s deployment/bindy -n "${NAMESPACE}" || {
            echo -e "${RED}❌ Controller failed to start. Checking logs:${NC}"
            kubectl logs -n "${NAMESPACE}" -l app=bindy --tail=50
            exit 1
        }
    else
        echo -e "${GREEN}✅ Using existing cluster '${CLUSTER_NAME}'${NC}"
        kubectl config use-context "kind-${CLUSTER_NAME}" > /dev/null

        # If IMAGE_TAG is specified, update the controller deployment
        if [ -n "$IMAGE_TAG" ]; then
            echo -e "${YELLOW}📦 Updating controller with image: ${IMAGE_TAG}${NC}"

            # Update deployment with specific image
            kubectl set image deployment/bindy \
                controller="ghcr.io/${GITHUB_REPOSITORY:-firestoned/bindy}:${IMAGE_TAG}" \
                -n "${NAMESPACE}"

            # Wait for rollout
            kubectl rollout status deployment/bindy -n "${NAMESPACE}" --timeout=300s || {
                echo -e "${RED}❌ Controller rollout failed${NC}"
                kubectl logs -n "${NAMESPACE}" -l app=bindy --tail=50
                exit 1
            }
        fi
    fi
else
    echo -e "${YELLOW}⏭️  Skipping cluster and controller deployment${NC}"
    kubectl config use-context "kind-${CLUSTER_NAME}" > /dev/null || {
        echo -e "${RED}❌ Cluster '${CLUSTER_NAME}' not found${NC}"
        exit 1
    }
fi

echo ""
echo -e "${GREEN}1️⃣  Running Rust integration tests...${NC}"
cd "${PROJECT_ROOT}"

# Run integration tests with kind cluster
export KUBECONFIG="${HOME}/.kube/config"
cargo test --test simple_integration -- --ignored --test-threads=1 --nocapture

TEST_EXIT=$?

if [ $TEST_EXIT -eq 0 ]; then
    echo -e "${GREEN}✅ Integration tests passed!${NC}"
else
    echo -e "${RED}❌ Integration tests failed with exit code ${TEST_EXIT}${NC}"
    echo -e "${YELLOW}Checking controller logs:${NC}"
    kubectl logs -n "${NAMESPACE}" -l app=bindy --tail=50 || true
fi

echo ""
echo -e "${GREEN}2️⃣  Running functional tests with kubectl...${NC}"

# Test Bind9Instance creation
echo -e "${YELLOW}Testing Bind9Instance creation...${NC}"
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: integration-test-primary
  namespace: ${NAMESPACE}
  labels:
    test: integration
    role: primary
spec:
  replicas: 1
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
EOF

sleep 2

# Test DNSZone creation
echo -e "${YELLOW}Testing DNSZone creation...${NC}"
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: integration-test-zone
  namespace: ${NAMESPACE}
spec:
  zoneName: integration.test
  type: primary
  instanceSelector:
    matchLabels:
      role: primary
  soaRecord:
    primaryNS: ns1.integration.test.
    adminEmail: admin@integration.test
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
EOF

sleep 3

# Test all record types
echo -e "${YELLOW}Testing all DNS record types...${NC}"

# A Record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: integration-a
  namespace: ${NAMESPACE}
spec:
  zone: integration.test
  name: www
  ipv4Address: "192.0.2.10"
  ttl: 300
EOF

# AAAA Record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: AAAARecord
metadata:
  name: integration-aaaa
  namespace: ${NAMESPACE}
spec:
  zone: integration.test
  name: www
  ipv6Address: "2001:db8::1"
  ttl: 300
EOF

# CNAME Record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: CNAMERecord
metadata:
  name: integration-cname
  namespace: ${NAMESPACE}
spec:
  zone: integration.test
  name: blog
  target: www.integration.test.
  ttl: 300
EOF

# MX Record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: MXRecord
metadata:
  name: integration-mx
  namespace: ${NAMESPACE}
spec:
  zone: integration.test
  name: "@"
  priority: 10
  mailServer: mail.integration.test.
  ttl: 3600
EOF

# TXT Record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: integration-txt
  namespace: ${NAMESPACE}
spec:
  zone: integration.test
  name: "@"
  text:
    - "v=spf1 mx ~all"
  ttl: 3600
EOF

# NS Record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: NSRecord
metadata:
  name: integration-ns
  namespace: ${NAMESPACE}
spec:
  zone: integration.test
  name: "@"
  nameserver: ns2.integration.test.
  ttl: 3600
EOF

# SRV Record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: integration-srv
  namespace: ${NAMESPACE}
spec:
  zone: integration.test
  name: _sip._tcp
  priority: 10
  weight: 60
  port: 5060
  target: sip.integration.test.
  ttl: 3600
EOF

# CAA Record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: CAARecord
metadata:
  name: integration-caa
  namespace: ${NAMESPACE}
spec:
  zone: integration.test
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org
  ttl: 3600
EOF

echo -e "${GREEN}⏳ Waiting for reconciliation (10 seconds)...${NC}"
sleep 10

echo ""
echo -e "${GREEN}3️⃣  Verifying resources...${NC}"

# Check if resources were created
ERRORS=0

if kubectl get bind9instance integration-test-primary -n "${NAMESPACE}" &>/dev/null; then
    echo -e "  ${GREEN}✓${NC} Bind9Instance created"
else
    echo -e "  ${RED}✗${NC} Bind9Instance not found"
    ERRORS=$((ERRORS + 1))
fi

if kubectl get dnszone integration-test-zone -n "${NAMESPACE}" &>/dev/null; then
    echo -e "  ${GREEN}✓${NC} DNSZone created"
else
    echo -e "  ${RED}✗${NC} DNSZone not found"
    ERRORS=$((ERRORS + 1))
fi

# Check all record types
RECORD_TYPES=("arecord:integration-a" "aaaarecord:integration-aaaa" "cnamerecord:integration-cname" "mxrecord:integration-mx" "txtrecord:integration-txt" "nsrecord:integration-ns" "srvrecord:integration-srv" "caarecord:integration-caa")

for record in "${RECORD_TYPES[@]}"; do
    IFS=':' read -r type name <<< "$record"
    if kubectl get "${type}" "${name}" -n "${NAMESPACE}" &>/dev/null; then
        echo -e "  ${GREEN}✓${NC} ${type} created"
    else
        echo -e "  ${RED}✗${NC} ${type} not found"
        ERRORS=$((ERRORS + 1))
    fi
done

echo ""
echo -e "${GREEN}4️⃣  Resource Status:${NC}"
echo -e "${BLUE}Bind9Instances:${NC}"
kubectl get bind9instances -n "${NAMESPACE}" -l test=integration 2>/dev/null || echo "  No Bind9Instances found"

echo ""
echo -e "${BLUE}DNSZones:${NC}"
kubectl get dnszones -n "${NAMESPACE}" 2>/dev/null || echo "  No DNSZones found"

echo ""
echo -e "${BLUE}DNS Records:${NC}"
kubectl get arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords -n "${NAMESPACE}" -l test=integration 2>/dev/null || \
kubectl get arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords -n "${NAMESPACE}" 2>/dev/null || \
echo "  No DNS records found"

echo ""
echo -e "${GREEN}5️⃣  Cleanup test resources...${NC}"
kubectl delete bind9instance integration-test-primary -n "${NAMESPACE}" --ignore-not-found=true
kubectl delete dnszone integration-test-zone -n "${NAMESPACE}" --ignore-not-found=true
kubectl delete arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords -l test=integration -n "${NAMESPACE}" --ignore-not-found=true 2>/dev/null || true
kubectl delete arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords integration-a,integration-aaaa,integration-cname,integration-mx,integration-txt,integration-ns,integration-srv,integration-caa -n "${NAMESPACE}" --ignore-not-found=true 2>/dev/null || true

echo ""
if [ $ERRORS -eq 0 ] && [ $TEST_EXIT -eq 0 ]; then
    echo -e "${GREEN}✅ All integration tests passed!${NC}"
    echo ""
    echo -e "${YELLOW}📋 Summary:${NC}"
    echo "  - Rust integration tests: PASSED"
    echo "  - Functional tests: PASSED"
    echo "  - All 8 DNS record types tested: PASSED"
    exit 0
else
    echo -e "${RED}❌ Some tests failed${NC}"
    echo ""
    echo -e "${YELLOW}📋 Summary:${NC}"
    echo "  - Rust integration tests: $([ $TEST_EXIT -eq 0 ] && echo 'PASSED' || echo 'FAILED')"
    echo "  - Functional tests: $([ $ERRORS -eq 0 ] && echo 'PASSED' || echo "FAILED ($ERRORS errors)")"
    echo ""
    echo -e "${YELLOW}Controller logs (last 30 lines):${NC}"
    kubectl logs -n "${NAMESPACE}" -l app=bindy --tail=30 || true
    exit 1
fi
