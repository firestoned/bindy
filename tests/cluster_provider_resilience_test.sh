#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

set -euo pipefail

# ClusterBind9Provider Resilience Integration Test
#
# This test verifies that the Bindy operator properly handles:
# 1. ClusterBind9Provider creation from examples/cluster-bind9-provider.yaml
# 2. DNSZone creation with matching labels
# 3. Creation of Bind9Cluster and Bind9Instances (3 total)
# 4. DNS zone propagation to all instances
# 5. Pod deletion and automatic recreation
# 6. DNS zone recreation after pod restart
# 7. DNSZone deletion and zone removal from all instances
#
# Usage: ./tests/cluster_provider_resilience_test.sh [--image IMAGE_REF] [--skip-deploy] [--zone ZONE_NAME]
#
# Options:
#   --image IMAGE_REF    Use pre-built image from registry (e.g., ghcr.io/firestoned/bindy:main)
#                        If not specified, builds image locally from source
#   --skip-deploy        Skip cluster and operator deployment (assumes already set up)
#   --zone ZONE_NAME     DNS zone name to use for testing (default: test-example.com)

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

NAMESPACE="test-dns-system"
CLUSTER_NAME="${CLUSTER_NAME:=bindy-test}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SKIP_DEPLOY=false
IMAGE_REF="${IMAGE_REF:-}"
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"
CLUSTER_PROVIDER_NAME="test-production-dns"
ZONE_NAME="${ZONE_NAME:-test-example.com}"

# Track if we've started creating resources
RESOURCES_CREATED=false

# Cleanup function to be called on exit or interrupt
cleanup() {
    local exit_code=$?

    if [ "$RESOURCES_CREATED" = true ]; then
        echo ""
        echo -e "${YELLOW}🧹 Caught interrupt/exit - cleaning up test resources...${NC}"

        # Delete ClusterBind9Provider (cascades to instances and cluster)
        echo -e "${YELLOW}Deleting ClusterBind9Provider '${CLUSTER_PROVIDER_NAME}'...${NC}"
        ${KUBECTL} delete clusterbind9provider "${CLUSTER_PROVIDER_NAME}" --ignore-not-found=true --timeout=60s 2>/dev/null || true

        # Give Kubernetes time to finalize deletions
        sleep 5

        # Delete test namespace (removes any remaining resources)
        echo -e "${YELLOW}Deleting namespace '${NAMESPACE}'...${NC}"
        ${KUBECTL} delete namespace "${NAMESPACE}" --ignore-not-found=true --timeout=60s 2>/dev/null || true

        echo -e "${GREEN}✓ Cleanup complete${NC}"
    fi

    # If interrupted (non-zero exit from interrupt), exit with 130 (128 + SIGINT)
    if [ $exit_code -ne 0 ]; then
        exit 130
    fi
}

# Set up trap for cleanup on script exit or interrupt (Ctrl-C)
#trap cleanup EXIT INT TERM

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-deploy)
            SKIP_DEPLOY=true
            shift
            ;;
        --image)
            IMAGE_REF="$2"
            shift 2
            ;;
        --zone)
            ZONE_NAME="$2"
            shift 2
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --image IMAGE_REF    Use pre-built image from registry (e.g., ghcr.io/firestoned/bindy:main)"
            echo "                       If not specified, builds image locally from source"
            echo "  --skip-deploy        Skip cluster and operator deployment (assumes already set up)"
            echo "  --zone ZONE_NAME     DNS zone name to use for testing (default: test-example.com)"
            echo "  --help, -h           Show this help message"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo "Usage: $0 [--image IMAGE_REF] [--skip-deploy] [--zone ZONE_NAME]"
            echo "Use --help for more information"
            exit 1
            ;;
    esac
done

echo -e "${BLUE}🧪 ClusterBind9Provider Resilience Integration Test${NC}"
echo ""

# Check for required tools
check_required_tools() {
    local missing_tools=()

    if ! command -v kind &>/dev/null; then
        missing_tools+=("kind")
    fi

    if ! command -v kubectl &>/dev/null; then
        missing_tools+=("kubectl")
    fi

    if ! command -v docker &>/dev/null; then
        missing_tools+=("docker")
    fi

    if [ ${#missing_tools[@]} -gt 0 ]; then
        echo -e "${RED}❌ Missing required tools: ${missing_tools[*]}${NC}"
        echo ""
        echo "Please install the missing tools:"
        for tool in "${missing_tools[@]}"; do
            case $tool in
                kind)
                    echo "  - kind: https://kind.sigs.k8s.io/docs/user/quick-start/#installation"
                    ;;
                kubectl)
                    echo "  - kubectl: https://kubernetes.io/docs/tasks/tools/"
                    ;;
                docker)
                    echo "  - docker: https://docs.docker.com/get-docker/"
                    ;;
            esac
        done
        exit 1
    fi
}

# Function to wait for resource to be ready
wait_for_resource_ready() {
    local resource_type=$1
    local resource_name=$2
    local namespace=$3
    local timeout=${4:-180}  # Default 3 minutes
    local elapsed=0

    echo -e "${YELLOW}⏳ Waiting for ${resource_type} '${resource_name}' to be ready (timeout: ${timeout}s)...${NC}"

    while [ $elapsed -lt $timeout ]; do
        if ${KUBECTL} get "${resource_type}" "${resource_name}" -n "${namespace}" &>/dev/null; then
            # Check if resource has a Ready condition
            local ready_status=$(${KUBECTL} get "${resource_type}" "${resource_name}" -n "${namespace}" -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}' 2>/dev/null || echo "")

            if [ "$ready_status" = "True" ]; then
                echo -e "${GREEN}✓ ${resource_type} '${resource_name}' is ready${NC}"
                return 0
            fi

            echo "  Status: ${ready_status:-Unknown} (${elapsed}s elapsed)"
        else
            echo "  Waiting for ${resource_type} '${resource_name}' to be created (${elapsed}s elapsed)"
        fi

        sleep 5
        elapsed=$((elapsed + 5))
    done

    echo -e "${RED}✗ Timeout waiting for ${resource_type} '${resource_name}' to be ready${NC}"
    return 1
}

# Function to wait for pods to be ready
wait_for_pods_ready() {
    local label_selector=$1
    local namespace=$2
    local expected_count=${3:-1}
    local timeout=${4:-180}
    local elapsed=0

    echo -e "${YELLOW}⏳ Waiting for ${expected_count} pod(s) with selector '${label_selector}' to be ready...${NC}"

    while [ $elapsed -lt $timeout ]; do
        # Use JSONPath to get pods where Ready condition status is True, then count them
        local ready_pods=$(${KUBECTL} get pods -n "${namespace}" -l "${label_selector}" \
            -o jsonpath='{range .items[*]}{.metadata.name}{" "}{range .status.conditions[?(@.type=="Ready")]}{.status}{end}{"\n"}{end}' 2>/dev/null)

        # Count lines that have "True" in them (indicating Ready=True)
        local ready_count=$(echo "$ready_pods" | grep -c "True" 2>/dev/null || echo "0")

        if [ "${ready_count}" -ge "${expected_count}" ]; then
            echo -e "${GREEN}✓ ${ready_count}/${expected_count} pod(s) ready${NC}"
            return 0
        fi

        echo "  Pods ready: ${ready_count}/${expected_count} (${elapsed}s elapsed)"
        sleep 5
        elapsed=$((elapsed + 5))
    done

    echo -e "${RED}✗ Timeout waiting for pods to be ready${NC}"
    return 1
}

# Check for required tools
check_required_tools

if [ "$SKIP_DEPLOY" = false ]; then
    # Check if cluster exists
    if ! kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
        echo -e "${YELLOW}📦 Cluster '${CLUSTER_NAME}' not found. Creating new cluster...${NC}"

        # Create kind cluster
        if [ -f "${PROJECT_ROOT}/deploy/kind-config.yaml" ]; then
            kind create cluster --name "${CLUSTER_NAME}" --config "${PROJECT_ROOT}/deploy/kind-config.yaml" || {
                echo -e "${RED}❌ Failed to create kind cluster${NC}"
                exit 1
            }
        else
            kind create cluster --name "${CLUSTER_NAME}" || {
                echo -e "${RED}❌ Failed to create kind cluster${NC}"
                exit 1
            }
        fi

        echo -e "${GREEN}✓ Created kind cluster '${CLUSTER_NAME}'${NC}"
    else
        echo -e "${GREEN}✅ Using existing cluster '${CLUSTER_NAME}'${NC}"
    fi

    kubectl config use-context "kind-${CLUSTER_NAME}" > /dev/null

    # Check if Bindy operator is running
    echo ""
    echo -e "${YELLOW}🔍 Checking if Bindy operator is deployed...${NC}"

    if ! ${KUBECTL} get namespace dns-system &>/dev/null; then
        echo -e "${YELLOW}📦 Bindy not deployed. Deploying Bindy operator...${NC}"

        # Create dns-system namespace
        ${KUBECTL} create namespace dns-system || true

        # Deploy CRDs
        echo -e "${GREEN}📋 Installing CRDs...${NC}"
        ${KUBECTL} replace --force -f "${PROJECT_ROOT}/deploy/crds/" 2>/dev/null || ${KUBECTL} create -f "${PROJECT_ROOT}/deploy/crds/" || {
            echo -e "${RED}❌ Failed to install CRDs${NC}"
            exit 1
        }

        # Deploy RBAC
        echo -e "${GREEN}🔐 Installing RBAC...${NC}"
        ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/rbac/" || {
            echo -e "${RED}❌ Failed to install RBAC${NC}"
            exit 1
        }

        # Handle Docker image - either use provided image or build locally
        if [ -n "$IMAGE_REF" ]; then
            echo -e "${GREEN}📦 Using pre-built image: ${IMAGE_REF}${NC}"

            # Deploy operator with custom image
            echo -e "${GREEN}🚀 Deploying operator...${NC}"
            sed "s|ghcr.io/firestoned/bindy:latest|${IMAGE_REF}|g" "${PROJECT_ROOT}/deploy/operator/deployment.yaml" | ${KUBECTL} apply -f - || {
                echo -e "${RED}❌ Failed to deploy operator${NC}"
                exit 1
            }
        else
            # Build and load Docker image locally
            echo -e "${GREEN}🏗️  Building Docker image from source...${NC}"
            docker build -t bindy:latest "${PROJECT_ROOT}" || {
                echo -e "${RED}❌ Failed to build Docker image${NC}"
                exit 1
            }

            echo -e "${GREEN}📤 Loading image into kind...${NC}"
            kind load docker-image bindy:latest --name "${CLUSTER_NAME}" || {
                echo -e "${RED}❌ Failed to load image into kind${NC}"
                exit 1
            }

            # Deploy operator with local image
            echo -e "${GREEN}🚀 Deploying operator...${NC}"
            ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/operator/deployment.yaml" || {
                echo -e "${RED}❌ Failed to deploy operator${NC}"
                exit 1
            }
        fi

        # Wait for operator to be ready
        echo -e "${GREEN}⏳ Waiting for operator to be ready...${NC}"
        ${KUBECTL} wait --for=condition=available --timeout=300s deployment/bindy -n dns-system || {
            echo -e "${RED}❌ Operator failed to start. Checking logs:${NC}"
            ${KUBECTL} logs -n dns-system -l app=bindy --tail=50
            exit 1
        }

        echo -e "${GREEN}✓ Bindy operator deployed successfully${NC}"
    elif ! ${KUBECTL} get deployment bindy -n dns-system &>/dev/null; then
        echo -e "${YELLOW}📦 Bindy namespace exists but operator not deployed. Deploying...${NC}"

        # Deploy CRDs (may already exist)
        echo -e "${GREEN}📋 Installing/Updating CRDs...${NC}"
        ${KUBECTL} replace --force -f "${PROJECT_ROOT}/deploy/crds/" 2>/dev/null || ${KUBECTL} create -f "${PROJECT_ROOT}/deploy/crds/" || true

        # Deploy RBAC
        echo -e "${GREEN}🔐 Installing RBAC...${NC}"
        ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/rbac/" || true

        # Handle Docker image - either use provided image or build locally
        if [ -n "$IMAGE_REF" ]; then
            echo -e "${GREEN}📦 Using pre-built image: ${IMAGE_REF}${NC}"

            # Deploy operator with custom image
            echo -e "${GREEN}🚀 Deploying operator...${NC}"
            sed "s|ghcr.io/firestoned/bindy:latest|${IMAGE_REF}|g" "${PROJECT_ROOT}/deploy/operator/deployment.yaml" | ${KUBECTL} apply -f - || {
                echo -e "${RED}❌ Failed to deploy operator${NC}"
                exit 1
            }
        else
            # Build and load Docker image locally
            echo -e "${GREEN}🏗️  Building Docker image from source...${NC}"
            docker build -t bindy:latest "${PROJECT_ROOT}" || {
                echo -e "${RED}❌ Failed to build Docker image${NC}"
                exit 1
            }

            echo -e "${GREEN}📤 Loading image into kind...${NC}"
            kind load docker-image bindy:latest --name "${CLUSTER_NAME}" || {
                echo -e "${RED}❌ Failed to load image into kind${NC}"
                exit 1
            }

            # Deploy operator with local image
            echo -e "${GREEN}🚀 Deploying operator...${NC}"
            ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/operator/deployment.yaml" || {
                echo -e "${RED}❌ Failed to deploy operator${NC}"
                exit 1
            }
        fi

        # Wait for operator to be ready
        echo -e "${GREEN}⏳ Waiting for operator to be ready...${NC}"
        ${KUBECTL} wait --for=condition=available --timeout=300s deployment/bindy -n dns-system || {
            echo -e "${RED}❌ Operator failed to start${NC}"
            exit 1
        }

        echo -e "${GREEN}✓ Bindy operator deployed successfully${NC}"
    else
        # Operator exists, check if it's running
        if ${KUBECTL} get deployment bindy -n dns-system -o jsonpath='{.status.conditions[?(@.type=="Available")].status}' 2>/dev/null | grep -q "True"; then
            echo -e "${GREEN}✓ Bindy operator is running${NC}"
        else
            echo -e "${YELLOW}⚠️  Bindy operator exists but not ready. Waiting...${NC}"
            ${KUBECTL} wait --for=condition=available --timeout=60s deployment/bindy -n dns-system || {
                echo -e "${RED}❌ Operator not ready${NC}"
                exit 1
            }
        fi
    fi
fi

echo ""
echo -e "${GREEN}📋 Step 1: Creating test namespace '${NAMESPACE}'...${NC}"

${KUBECTL} create namespace "${NAMESPACE}" --dry-run=client -o yaml | ${KUBECTL} apply -f - || {
    echo -e "${RED}❌ Failed to create namespace${NC}"
    exit 1
}

echo ""
echo -e "${GREEN}📦 Step 2: Applying ClusterBind9Provider from examples/cluster-bind9-provider.yaml...${NC}"

# Modify the example to use test namespace and rename to avoid conflicts
sed -e "s/namespace: dns-system/namespace: ${NAMESPACE}/g" \
    -e "s/name: production-dns$/name: ${CLUSTER_PROVIDER_NAME}/g" \
    "${PROJECT_ROOT}/examples/cluster-bind9-provider.yaml" | ${KUBECTL} apply -f - || {
    echo -e "${RED}❌ Failed to apply ClusterBind9Provider${NC}"
    exit 1
}

# Mark that we've created resources - cleanup trap will now run
RESOURCES_CREATED=true

echo ""
echo -e "${GREEN}📋 Step 2.5: Creating DNSZone '${ZONE_NAME}' for testing...${NC}"

# Create a test DNSZone that matches the ClusterBind9Provider's zonesFrom selector
# Convert zone name to a valid Kubernetes resource name (replace dots with hyphens)
ZONE_RESOURCE_NAME=$(echo "${ZONE_NAME}" | sed 's/\./-/g')

${KUBECTL} apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: ${ZONE_RESOURCE_NAME}
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/environment: production
    bindy.firestoned.io/team: platform
spec:
  bind9InstancesFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/cluster: test-production-dns
          bindy.firestoned.io/managed-by: Bind9Cluster
        matchExpressions:
          - key: bindy.firestoned.io/role
            operator: In
            values:
              - primary
              - secondary
  zoneName: ${ZONE_NAME}
  nameServerIps:
    ns1.${ZONE_NAME}.: 192.168.0.60
  soaRecord:
    primaryNs: ns1.${ZONE_NAME}.
    adminEmail: admin.${ZONE_NAME}.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
  recordsFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/zone: ${ZONE_NAME}
EOF

if [ $? -ne 0 ]; then
    echo -e "${RED}❌ Failed to create DNSZone${NC}"
    exit 1
fi

echo -e "${GREEN}✓ DNSZone '${ZONE_RESOURCE_NAME}' created${NC}"

echo ""
echo -e "${GREEN}⏳ Step 3: Waiting for resources to be created (timeout: 3 minutes)...${NC}"

# Wait for ClusterBind9Provider to be ready
if ! wait_for_resource_ready "clusterbind9provider" "${CLUSTER_PROVIDER_NAME}" "default" 180; then
    echo -e "${YELLOW}⚠️  ClusterBind9Provider not ready, continuing anyway...${NC}"
fi

# Wait for Bind9Instances to be created and ready
echo ""
echo -e "${YELLOW}Waiting for Bind9Instances to be created...${NC}"

EXPECTED_INSTANCES=(
    "test-production-dns-primary-0"
    "test-production-dns-primary-1"
    "test-production-dns-secondary-0"
)

READY_INSTANCES=0
for instance in "${EXPECTED_INSTANCES[@]}"; do
    if wait_for_resource_ready "bind9instance" "${instance}" "${NAMESPACE}" 180; then
        READY_INSTANCES=$((READY_INSTANCES + 1))
    else
        echo -e "${YELLOW}⚠️  Instance '${instance}' not ready${NC}"
    fi
done

echo ""
echo -e "${BLUE}📊 Instance Status: ${READY_INSTANCES}/${#EXPECTED_INSTANCES[@]} instances ready${NC}"

# Show all instances
echo ""
echo -e "${BLUE}Current Bind9Instances:${NC}"
${KUBECTL} get bind9instances -n "${NAMESPACE}" -o wide || echo "  No instances found"

# Verify all expected instances exist (even if not ready)
echo ""
echo -e "${GREEN}✓ Step 3 Complete: Verifying all resources exist...${NC}"

MISSING_RESOURCES=0
for instance in "${EXPECTED_INSTANCES[@]}"; do
    if ${KUBECTL} get bind9instance "${instance}" -n "${NAMESPACE}" &>/dev/null; then
        echo -e "  ${GREEN}✓${NC} Bind9Instance '${instance}' exists"
    else
        echo -e "  ${RED}✗${NC} Bind9Instance '${instance}' missing"
        MISSING_RESOURCES=$((MISSING_RESOURCES + 1))
    fi
done

if [ $MISSING_RESOURCES -gt 0 ]; then
    echo -e "${RED}❌ Missing ${MISSING_RESOURCES} expected instance(s)${NC}"
    echo -e "${YELLOW}Cleaning up...${NC}"
    ${KUBECTL} delete clusterbind9provider "${CLUSTER_PROVIDER_NAME}" --ignore-not-found=true
    ${KUBECTL} delete namespace "${NAMESPACE}" --ignore-not-found=true
    exit 1
fi

echo ""
echo -e "${GREEN}🔍 Step 4: Validating DNS zone '${ZONE_NAME}' exists on all instances (before deletion)...${NC}"

# Wait for pods to be fully ready
sleep 5

# Helper function to get the latest Running and Ready pod for an instance
get_latest_running_pod() {
    local instance=$1
    local instance_label="app.kubernetes.io/instance=${instance}"

    # Get the most recently created Running pod that is Ready
    # Sort by creation timestamp (newest first) and pick the first one
    local pod_name=$(${KUBECTL} get pods -n "${NAMESPACE}" -l "${instance_label}" \
        --field-selector=status.phase=Running \
        --sort-by='{.metadata.creationTimestamp}' \
        -o jsonpath='{range .items[*]}{.metadata.name}{" "}{range .status.conditions[?(@.type=="Ready")]}{.status}{end}{"\n"}{end}' 2>/dev/null \
        | grep "True$" | tail -1 | awk '{print $1}')

    echo "$pod_name"
}

# Helper function to validate zone on an instance
validate_zone_on_instance() {
    local instance=$1

    # Get the latest Running and Ready pod
    POD_NAME=$(get_latest_running_pod "${instance}")

    if [ -z "$POD_NAME" ]; then
        echo -e "${RED}  ✗ No running pod found for instance '${instance}'${NC}"
        echo -e "${YELLOW}  Manual verification command:${NC}"
        echo "    kubectl exec -it \$(kubectl get po -n ${NAMESPACE} -l app.kubernetes.io/instance=${instance} --field-selector=status.phase=Running -o jsonpath='{.items[0].metadata.name}') -n ${NAMESPACE} -- dig @127.0.0.1 -p 5353 SOA ${ZONE_NAME}"
        return 1
    fi

    echo -e "${YELLOW}  Pod: ${POD_NAME}${NC}"

    # Execute dig command to check if zone exists (non-empty output expected)
    SOA_RECORD=$(${KUBECTL} exec -n "${NAMESPACE}" "${POD_NAME}" -- dig @127.0.0.1 -p 5353 SOA ${ZONE_NAME} +short 2>/dev/null || echo "")

    if [ -n "$SOA_RECORD" ]; then
        # Zone exists (non-empty output) - this is expected
        echo -e "${GREEN}  ✓ DNS query successful - zone '${ZONE_NAME}' exists${NC}"
        echo -e "${BLUE}  SOA Record: ${SOA_RECORD}${NC}"
        return 0
    else
        # Zone does not exist (empty output) - this is a failure
        echo -e "${RED}  ✗ DNS query failed - zone may not be loaded${NC}"
        echo -e "${YELLOW}  Manual verification command:${NC}"
        echo "    kubectl exec -it ${POD_NAME} -n ${NAMESPACE} -- dig @127.0.0.1 -p 5353 SOA ${ZONE_NAME}"
        return 1
    fi
}

# Validate zone exists on all instances before starting deletion tests
ZONE_VALIDATION_FAILURES=0
for instance in "${EXPECTED_INSTANCES[@]}"; do
    echo ""
    echo -e "${YELLOW}Checking instance: ${instance}${NC}"

    if ! validate_zone_on_instance "${instance}"; then
        ZONE_VALIDATION_FAILURES=$((ZONE_VALIDATION_FAILURES + 1))
    fi
done

if [ $ZONE_VALIDATION_FAILURES -gt 0 ]; then
    echo ""
    echo -e "${RED}✗ Zone validation failed on ${ZONE_VALIDATION_FAILURES}/${#EXPECTED_INSTANCES[@]} instances${NC}"
    echo -e "${RED}✗ Test FAILED - DNS zone not present before deletion test${NC}"
    exit 1
fi

echo ""
echo -e "${GREEN}✓ Zone '${ZONE_NAME}' validated on all ${#EXPECTED_INSTANCES[@]} instances${NC}"

# Step 5: Delete each instance's deployment and verify recreation with zone
echo ""
echo -e "${GREEN}🗑️  Step 5: Testing resilience - delete each deployment and verify zone recreation...${NC}"

for instance in "${EXPECTED_INSTANCES[@]}"; do
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${YELLOW}Testing instance: ${instance}${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

    # Get the deployment name (same as instance name)
    DEPLOYMENT_NAME="${instance}"
    INSTANCE_LABEL_SELECTOR="app.kubernetes.io/instance=${instance}"

    # Step 5a: Get current pod ID before deletion
    echo ""
    echo -e "${YELLOW}5a. Recording current pod ID before deletion...${NC}"

    OLD_POD_NAME=$(get_latest_running_pod "${instance}")

    if [ -z "$OLD_POD_NAME" ]; then
        echo -e "${RED}✗ No running pod found for instance '${instance}'${NC}"
        echo -e "${RED}✗ Test FAILED - cannot find pod to delete${NC}"
        exit 1
    fi

    echo -e "${GREEN}✓ Current pod: ${OLD_POD_NAME}${NC}"

    # Step 5b: Delete deployment
    echo ""
    echo -e "${YELLOW}5b. Deleting deployment '${DEPLOYMENT_NAME}'...${NC}"

    if ${KUBECTL} get deployment "${DEPLOYMENT_NAME}" -n "${NAMESPACE}" &>/dev/null; then
        ${KUBECTL} delete deployment "${DEPLOYMENT_NAME}" -n "${NAMESPACE}" || {
            echo -e "${RED}❌ Failed to delete deployment${NC}"
            exit 1
        }
        echo -e "${GREEN}✓ Deleted deployment: ${DEPLOYMENT_NAME}${NC}"

        # Wait for old pod to start terminating
        echo -e "${YELLOW}Waiting for old pod to start terminating...${NC}"
        sleep 5
    else
        echo -e "${RED}✗ Deployment '${DEPLOYMENT_NAME}' not found${NC}"
        echo -e "${RED}✗ Test FAILED - expected deployment missing${NC}"
        exit 1
    fi

    # Step 5c: Wait for NEW pod (different from old pod ID)
    echo ""
    echo -e "${YELLOW}5c. Waiting for NEW pod to be created and ready...${NC}"

    WAIT_START=$(date +%s)
    WAIT_TIMEOUT=180
    NEW_POD_READY=false

    while [ $(($(date +%s) - WAIT_START)) -lt $WAIT_TIMEOUT ]; do
        # Get all Running pods for this instance
        CURRENT_PODS=$(${KUBECTL} get pods -n "${NAMESPACE}" -l "${INSTANCE_LABEL_SELECTOR}" \
            --field-selector=status.phase=Running \
            -o jsonpath='{range .items[*]}{.metadata.name}{" "}{range .status.conditions[?(@.type=="Ready")]}{.status}{end}{"\n"}{end}' 2>/dev/null)

        # Look for a Ready pod that is NOT the old pod
        while IFS= read -r line; do
            POD_NAME=$(echo "$line" | awk '{print $1}')
            POD_STATUS=$(echo "$line" | awk '{print $2}')

            # Check if this is a new pod (not the old one) and it's Ready
            if [ "$POD_NAME" != "$OLD_POD_NAME" ] && [ "$POD_STATUS" = "True" ]; then
                NEW_POD_NAME="$POD_NAME"
                NEW_POD_READY=true
                break 2
            fi
        done <<< "$CURRENT_PODS"

        echo "  Waiting for new pod (old pod: ${OLD_POD_NAME})... ($(($(date +%s) - WAIT_START))s elapsed)"
        sleep 5
    done

    if [ "$NEW_POD_READY" = false ]; then
        echo -e "${RED}❌ New pod was not created within timeout${NC}"
        echo -e "${RED}✗ Test FAILED - pod resilience test failed for ${instance}${NC}"
        exit 1
    fi

    echo -e "${GREEN}✓ New pod created and ready: ${NEW_POD_NAME}${NC}"

    # Show pod details
    echo ""
    echo -e "${BLUE}Current pods for instance:${NC}"
    ${KUBECTL} get pods -n "${NAMESPACE}" -l "${INSTANCE_LABEL_SELECTOR}" -o wide

    # Step 5d: Validate zone was recreated on the NEW pod
    echo ""
    echo -e "${YELLOW}5d. Validating zone '${ZONE_NAME}' was recreated on new pod...${NC}"

    # Give DNS time to fully load zone
    sleep 5

    if ! validate_zone_on_instance "${instance}"; then
        echo -e "${RED}✗ Test FAILED - Zone not recreated on ${instance} after pod restart${NC}"
        exit 1
    fi

    echo -e "${GREEN}✓ Zone successfully recreated on ${instance} (new pod: ${NEW_POD_NAME})${NC}"
done

echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✓ All instances passed resilience test!${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

# Step 6: Delete DNSZone and verify zone removal
echo ""
echo -e "${GREEN}🗑️  Step 6: Testing DNSZone deletion - verify zone removal from all instances...${NC}"

echo ""
echo -e "${YELLOW}6a. Deleting DNSZone '${ZONE_RESOURCE_NAME}'...${NC}"

if ${KUBECTL} get dnszone ${ZONE_RESOURCE_NAME} -n "${NAMESPACE}" &>/dev/null; then
    ${KUBECTL} delete dnszone ${ZONE_RESOURCE_NAME} -n "${NAMESPACE}" || {
        echo -e "${RED}❌ Failed to delete DNSZone${NC}"
        exit 1
    }
    echo -e "${GREEN}✓ Deleted DNSZone: ${ZONE_RESOURCE_NAME}${NC}"

    # Wait for reconciliation to process the deletion
    echo -e "${YELLOW}Waiting for reconciliation to process deletion...${NC}"
    sleep 10
else
    echo -e "${RED}✗ DNSZone '${ZONE_RESOURCE_NAME}' not found${NC}"
    echo -e "${RED}✗ Test FAILED - expected DNSZone missing${NC}"
    exit 1
fi

echo ""
echo -e "${YELLOW}6b. Validating zone '${ZONE_NAME}' is removed from all instances...${NC}"

# Helper function to validate zone does NOT exist on an instance
validate_zone_removed_from_instance() {
    local instance=$1

    # Get the latest Running and Ready pod
    POD_NAME=$(get_latest_running_pod "${instance}")

    if [ -z "$POD_NAME" ]; then
        echo -e "${RED}  ✗ No running pod found for instance '${instance}'${NC}"
        return 1
    fi

    echo -e "${YELLOW}  Pod: ${POD_NAME}${NC}"

    # Execute dig command - zone should NOT exist (empty output expected)
    SOA_RECORD=$(${KUBECTL} exec -n "${NAMESPACE}" "${POD_NAME}" -- dig @127.0.0.1 -p 5353 SOA ${ZONE_NAME} +short 2>/dev/null || echo "")

    if [ -n "$SOA_RECORD" ]; then
        # Zone still exists (non-empty output) - this is a failure
        echo -e "${RED}  ✗ Zone '${ZONE_NAME}' still exists (should be deleted)${NC}"
        echo -e "${RED}  SOA Record: ${SOA_RECORD}${NC}"
        return 1
    else
        # Zone does not exist (empty output) - this is expected
        echo -e "${GREEN}  ✓ Zone '${ZONE_NAME}' successfully removed${NC}"
        return 0
    fi
}

# Validate zone removed from all instances
ZONE_REMOVAL_FAILURES=0
for instance in "${EXPECTED_INSTANCES[@]}"; do
    echo ""
    echo -e "${YELLOW}Checking instance: ${instance}${NC}"

    if ! validate_zone_removed_from_instance "${instance}"; then
        ZONE_REMOVAL_FAILURES=$((ZONE_REMOVAL_FAILURES + 1))
    fi
done

if [ $ZONE_REMOVAL_FAILURES -gt 0 ]; then
    echo ""
    echo -e "${RED}✗ Zone removal validation failed on ${ZONE_REMOVAL_FAILURES}/${#EXPECTED_INSTANCES[@]} instances${NC}"
    echo -e "${RED}✗ Test FAILED - DNS zone not removed after DNSZone deletion${NC}"
    exit 1
fi

echo ""
echo -e "${GREEN}✓ Zone '${ZONE_NAME}' successfully removed from all ${#EXPECTED_INSTANCES[@]} instances${NC}"

echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✓ All tests passed!${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

echo ""
echo -e "${GREEN}✅ Test completed successfully!${NC}"
echo ""
echo -e "${YELLOW}Note: Test resources will be cleaned up by the exit trap...${NC}"

# Mark resources as not created to skip cleanup trap (normal exit path does cleanup itself)
RESOURCES_CREATED=false

# Perform cleanup manually for successful test completion
echo -e "${GREEN}🧹 Cleanup: Removing test resources...${NC}"

# Delete ClusterBind9Provider (will cascade delete instances)
${KUBECTL} delete clusterbind9provider "${CLUSTER_PROVIDER_NAME}" --ignore-not-found=true --timeout=60s 2>/dev/null || true

# Give Kubernetes time to finalize deletions
sleep 5

# Delete namespace
${KUBECTL} delete namespace "${NAMESPACE}" --ignore-not-found=true --timeout=60s 2>/dev/null || true

echo -e "${GREEN}✓ Cleanup complete${NC}"
echo ""
echo -e "${YELLOW}📋 Summary:${NC}"
echo "  - ClusterBind9Provider created: ✓"
echo "  - DNSZone created: ✓"
echo "  - Bind9Instances created: ${READY_INSTANCES}/${#EXPECTED_INSTANCES[@]}"
echo "  - Initial zone validation: ✓ (validated on all ${#EXPECTED_INSTANCES[@]} instances)"
echo "  - Resilience testing: ✓ (all ${#EXPECTED_INSTANCES[@]} instances tested)"
echo "    - Deployment deletions: ${#EXPECTED_INSTANCES[@]}/${#EXPECTED_INSTANCES[@]}"
echo "    - Pod recreations: ${#EXPECTED_INSTANCES[@]}/${#EXPECTED_INSTANCES[@]}"
echo "    - Zone recreations: ${#EXPECTED_INSTANCES[@]}/${#EXPECTED_INSTANCES[@]}"
echo "  - DNSZone deletion testing: ✓"
echo "    - Zone removed from all instances: ${#EXPECTED_INSTANCES[@]}/${#EXPECTED_INSTANCES[@]}"

exit 0
