#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

set -euo pipefail

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Configuration
CLUSTER_NAME="bindy-test"
NAMESPACE="dns-system"

echo -e "${GREEN}🚀 Deploying Bindy Operator to Kind cluster${NC}"

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

echo -e "${GREEN}💾 Installing local-path storage provisioner...${NC}"
# Install local-path provisioner for persistent storage
kubectl apply -f https://raw.githubusercontent.com/rancher/local-path-provisioner/v0.0.28/deploy/local-path-storage.yaml

# Wait for local-path provisioner to be ready
echo -e "${GREEN}⏳ Waiting for local-path provisioner to be ready...${NC}"
kubectl wait --for=condition=available --timeout=60s deployment/local-path-provisioner -n local-path-storage || {
    echo -e "${YELLOW}⚠️  local-path provisioner deployment not ready, continuing anyway...${NC}"
}

# Check if local-path StorageClass was created
if kubectl get storageclass local-path &>/dev/null; then
    # local-path exists, set it as default if no default exists
    if ! kubectl get storageclass 2>/dev/null | grep -q "(default)"; then
        echo -e "${GREEN}🔧 Setting local-path as default StorageClass...${NC}"
        kubectl patch storageclass local-path -p '{"metadata": {"annotations":{"storageclass.kubernetes.io/is-default-class":"true"}}}'
    fi
else
    # local-path doesn't exist, create a default StorageClass that uses local-path provisioner
    echo -e "${GREEN}🔧 Creating default StorageClass for local-path...${NC}"
    cat <<EOF | kubectl apply -f -
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: default
  annotations:
    storageclass.kubernetes.io/is-default-class: "true"
provisioner: rancher.io/local-path
volumeBindingMode: WaitForFirstConsumer
reclaimPolicy: Delete
EOF
fi

echo -e "${GREEN}📋 Installing CRDs...${NC}"
# Use 'kubectl replace --force' to avoid annotation size limits with large CRDs
kubectl replace --force -f deploy/crds 2>/dev/null || kubectl create -f deploy/crds

echo -e "${GREEN}🔐 Creating namespace and RBAC...${NC}"
kubectl create namespace "${NAMESPACE}" --dry-run=client -o yaml | kubectl apply -f -
kubectl apply -f deploy/rbac/

#echo -e "${GREEN}🏗️  Building Docker image...${NC}"
#docker build -t bindy:latest .

#echo -e "${GREEN}📤 Loading image into Kind...${NC}"
#kind load docker-image bindy:latest --name "${CLUSTER_NAME}"

echo -e "${GREEN}🚀 Deploying operator...${NC}"
kubectl apply -f deploy/operator/deployment.yaml

echo -e "${GREEN}⏳ Waiting for operator to be ready...${NC}"
kubectl wait --for=condition=available --timeout=120s deployment/bindy -n "${NAMESPACE}" || {
    echo -e "${RED}❌ Operator failed to start. Checking logs:${NC}"
    kubectl logs -n "${NAMESPACE}" -l app=bindy --tail=50
    exit 1
}

echo -e "${GREEN}✅ Bindy operator deployed successfully!${NC}"
echo ""
echo -e "${YELLOW}📊 Cluster Status:${NC}"
kubectl get pods -n "${NAMESPACE}"
echo ""
echo -e "${YELLOW}💾 Storage:${NC}"
kubectl get storageclass
echo ""
echo -e "${YELLOW}📝 Next Steps:${NC}"
echo "1. Deploy a Bind9Cluster with persistent storage:"
echo "   kubectl apply -f examples/bind9-cluster-persistent.yaml"
echo ""
echo "2. Deploy a Bind9Instance:"
echo "   kubectl apply -f examples/bind9-instance.yaml"
echo ""
echo "2. Create a DNS zone:"
echo "   kubectl apply -f examples/dns-zone.yaml"
echo ""
echo "3. Add DNS records:"
echo "   kubectl apply -f examples/dns-records.yaml"
echo ""
echo "4. Watch operator logs:"
echo "   kubectl logs -n ${NAMESPACE} -l app=bindy -f"
echo ""
echo "5. Test DNS resolution:"
echo "   kubectl run -it --rm debug --image=nicolaka/netshoot --restart=Never -- dig @<pod-ip> example.com"
echo ""
echo -e "${GREEN}🎉 Happy testing!${NC}"
