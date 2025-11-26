#!/bin/bash

# Bind9 Kubernetes Cluster - Deployment Script
# Usage: ./deploy.sh [dev|prod]

set -e

ENVIRONMENT="${1:-dev}"
NAMESPACE="bind9"

echo "🚀 Deploying Bind9 to $ENVIRONMENT environment..."

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}[✓]${NC} $1"
}

print_error() {
    echo -e "${RED}[✗]${NC} $1"
}

print_info() {
    echo -e "${YELLOW}[ℹ]${NC} $1"
}

# Check prerequisites
check_prerequisites() {
    print_info "Checking prerequisites..."
    
    if ! command -v kubectl &> /dev/null; then
        print_error "kubectl not found. Please install kubectl."
        exit 1
    fi
    
    if ! kubectl cluster-info &> /dev/null; then
        print_error "Cannot connect to Kubernetes cluster."
        exit 1
    fi
    
    print_status "kubectl is available"
}

# Create namespace
create_namespace() {
    print_info "Creating namespace..."
    kubectl apply -f namespace.yaml
    print_status "Namespace created"
}

# Create RBAC
create_rbac() {
    print_info "Creating RBAC resources..."
    kubectl apply -f rbac.yaml
    print_status "RBAC configured"
}

# Create secrets
create_secrets() {
    print_info "Creating secrets..."
    kubectl apply -f secret-rndc.yaml
    print_status "Secrets created"
}

# Create config
create_configmap() {
    print_info "Creating ConfigMap..."
    kubectl apply -f configmap.yaml
    print_status "ConfigMap created"
}

# Create PVCs
create_pvcs() {
    print_info "Creating PersistentVolumeClaims..."
    kubectl apply -f pvc.yaml
    
    print_info "Waiting for PVCs to be bound..."
    kubectl wait --for=condition=Bound pvc/bind9-cache -n $NAMESPACE --timeout=60s || true
    kubectl wait --for=condition=Bound pvc/bind9-zones -n $NAMESPACE --timeout=60s || true
    print_status "PVCs created"
}

# Deploy Bind9
deploy_bind9() {
    print_info "Deploying Bind9..."
    kubectl apply -f deployment.yaml
    
    print_info "Waiting for deployment to be ready..."
    kubectl rollout status deployment/bind9 -n $NAMESPACE --timeout=5m
    print_status "Bind9 deployed successfully"
}

# Create services
create_services() {
    print_info "Creating services..."
    kubectl apply -f service-dns.yaml
    kubectl apply -f service-rndc.yaml
    
    if [ "$ENVIRONMENT" == "prod" ]; then
        print_info "Production mode: Creating LoadBalancer service..."
        kubectl apply -f service-loadbalancer.yaml
    fi
    
    print_status "Services created"
}

# Apply network policies
apply_network_policies() {
    if [ "$ENVIRONMENT" == "prod" ]; then
        print_info "Applying NetworkPolicies..."
        kubectl apply -f networkpolicy.yaml
        print_status "NetworkPolicies applied"
    else
        print_info "Skipping NetworkPolicies in dev environment"
    fi
}

# Print summary
print_summary() {
    echo ""
    echo "=========================================="
    echo "✨ Bind9 Deployment Complete!"
    echo "=========================================="
    echo ""
    print_status "Namespace: $NAMESPACE"
    echo ""
    
    print_info "Deployed Resources:"
    kubectl get all -n $NAMESPACE
    echo ""
    
    print_info "Services:"
    kubectl get svc -n $NAMESPACE
    echo ""
    
    print_info "Storage:"
    kubectl get pvc -n $NAMESPACE
    echo ""
    
    print_info "Next Steps:"
    echo "1. Verify pods are running: kubectl get pods -n $NAMESPACE"
    echo "2. Test DNS: kubectl exec -n $NAMESPACE <pod-name> -- dig @127.0.0.1 localhost"
    echo "3. View logs: kubectl logs -n $NAMESPACE -l app=bind9 -f"
    echo "4. Port-forward for testing: kubectl port-forward -n $NAMESPACE svc/bind9-dns 5353:53"
    echo ""
    
    if [ "$ENVIRONMENT" == "prod" ]; then
        echo "📌 F5 LoadBalancer Integration:"
        echo "   Create Virtual Server pointing to cluster nodes on port 30053"
        echo ""
    fi
    
    echo "See README.md for detailed documentation"
    echo ""
}

# Main execution
main() {
    check_prerequisites
    create_namespace
    create_rbac
    create_secrets
    create_configmap
    create_pvcs
    deploy_bind9
    create_services
    apply_network_policies
    print_summary
}

# Run main function
main
