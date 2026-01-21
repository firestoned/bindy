# Installation

This section guides you through installing Bindy in your Kubernetes cluster.

## Overview

Installing Bindy involves these steps:

1. **Prerequisites** - Ensure your environment meets the requirements
2. **Install CRDs** - Deploy Custom Resource Definitions
3. **Create RBAC** - Set up service accounts and permissions
4. **Deploy Operator** - Install the Bindy operator
5. **Create BIND9 Instances** - Deploy your DNS servers

## Installation Methods

### Standard Installation

The standard installation uses kubectl to apply YAML manifests:

```bash
# Create namespace
kubectl create namespace dns-system

# Install CRDs (use kubectl create to avoid annotation size limits)
kubectl create -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/crds/

# Install RBAC
kubectl apply -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/rbac/

# Deploy operator
kubectl apply -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/operator/deployment.yaml
```

### Development Installation

For development or testing, you can build and deploy from source:

```bash
# Clone the repository
git clone https://github.com/firestoned/bindy.git
cd bindy

# Build the operator
cargo build --release

# Build Docker image
docker build -t bindy:dev .

# Deploy with your custom image
kubectl apply -f deploy/
```

## Verification

After installation, verify that all components are running:

```bash
# Check CRDs are installed
kubectl get crd | grep bindy.firestoned.io

# Check operator is running
kubectl get pods -n dns-system

# Check operator logs
kubectl logs -n dns-system -l app=bind9-operator
```

You should see output similar to:

```
NAME                                READY   STATUS    RESTARTS   AGE
bind9-operator-7d4b8c4f9b-x7k2m   1/1     Running   0          1m
```

## Next Steps

- [Quick Start](./quickstart.md) - Deploy your first DNS zone
- [Prerequisites](./prerequisites.md) - Detailed system requirements
- [Installing CRDs](./crds.md) - Understanding the Custom Resources
- [Deploying the Operator](./operator.md) - Operator configuration options
