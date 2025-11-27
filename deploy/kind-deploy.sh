#!/usr/bin/env bash
set -euo pipefail

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Configuration
CLUSTER_NAME="bindy-test"
NAMESPACE="dns-system"

echo -e "${GREEN}🚀 Deploying Bindy Controller to Kind cluster${NC}"

# Check if kind is installed
if ! command -v kind &> /dev/null; then
    echo -e "${RED}❌ kind is not installed. Please install it first:${NC}"
    echo "   brew install kind  # macOS"
    echo "   or visit https://kind.sigs.k8s.io/docs/user/quick-start/#installation"
    exit 1
fi

# Check if kubectl is installed
if ! command -v kubectl &> /dev/null; then
    echo -e "${RED}❌ kubectl is not installed. Please install it first.${NC}"
    exit 1
fi

# Check if cluster exists
if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    echo -e "${YELLOW}⚠️  Cluster '${CLUSTER_NAME}' already exists${NC}"
    read -p "Do you want to delete and recreate it? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo -e "${YELLOW}🗑️  Deleting existing cluster...${NC}"
        kind delete cluster --name "${CLUSTER_NAME}"
    else
        echo -e "${GREEN}✅ Using existing cluster${NC}"
    fi
fi

# Create cluster if it doesn't exist
if ! kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    echo -e "${GREEN}📦 Creating Kind cluster...${NC}"
    kind create cluster --config deploy/kind-config.yaml
fi

# Set kubectl context
kubectl config use-context "kind-${CLUSTER_NAME}"

echo -e "${GREEN}📋 Installing CRDs via kubectl apply...${NC}"
# Use kubectl kubectl apply path to apply split CRD manifests
kubectl apply -f deploy/crds

echo -e "${GREEN}🔐 Creating namespace and RBAC...${NC}"
kubectl create namespace "${NAMESPACE}" --dry-run=client -o yaml | kubectl apply -f -
kubectl apply -f deploy/rbac/

echo -e "${GREEN}🏗️  Building Docker image...${NC}"
docker build -t bindy:latest .

echo -e "${GREEN}📤 Loading image into Kind...${NC}"
kind load docker-image bindy:latest --name "${CLUSTER_NAME}"

echo -e "${GREEN}🚀 Deploying controller...${NC}"
kubectl apply -f deploy/controller/deployment.yaml

echo -e "${GREEN}⏳ Waiting for controller to be ready...${NC}"
kubectl wait --for=condition=available --timeout=120s deployment/bindy -n "${NAMESPACE}" || {
    echo -e "${RED}❌ Controller failed to start. Checking logs:${NC}"
    kubectl logs -n "${NAMESPACE}" -l app=bindy --tail=50
    exit 1
}

echo -e "${GREEN}✅ Bindy controller deployed successfully!${NC}"
echo ""
echo -e "${YELLOW}📊 Cluster Status:${NC}"
kubectl get pods -n "${NAMESPACE}"
echo ""
echo -e "${YELLOW}📝 Next Steps:${NC}"
echo "1. Deploy a Bind9Instance:"
echo "   kubectl apply -f examples/bind9-instance.yaml"
echo ""
echo "2. Create a DNS zone:"
echo "   kubectl apply -f examples/dns-zone.yaml"
echo ""
echo "3. Add DNS records:"
echo "   kubectl apply -f examples/dns-records.yaml"
echo ""
echo "4. Watch controller logs:"
echo "   kubectl logs -n ${NAMESPACE} -l app=bindy -f"
echo ""
echo "5. Test DNS resolution:"
echo "   kubectl run -it --rm debug --image=nicolaka/netshoot --restart=Never -- dig @<pod-ip> example.com"
echo ""
echo -e "${GREEN}🎉 Happy testing!${NC}"
