# Quick Start

Get Bindy installed quickly with these concise installation commands.

For a complete end-to-end walkthrough including creating clusters, instances, zones, and records, see the [Step-by-Step Guide](./step-by-step.md).

## Overview

Installing Bindy involves these steps:

1. **Prerequisites** - Ensure your environment meets the requirements
2. **Install CRDs** - Deploy Custom Resource Definitions
3. **Create RBAC** - Set up service accounts and permissions
4. **Deploy Operator** - Install the Bindy operator
5. **Create BIND9 Instances** - Deploy your DNS servers

## Installation Methods

### Standard Installation (From Latest Release)

Install the latest stable release using kubectl:

```bash
# Create namespace
kubectl create namespace dns-system

# Install CRDs
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml

# Install RBAC
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/serviceaccount.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/role.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/rolebinding.yaml

# Deploy operator
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/operator/deployment.yaml
```

### Installation from Specific Version

Install from a specific version:

```bash
# Create namespace
kubectl create namespace dns-system

# Install CRDs from a specific version (e.g., v0.3.0)
kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/crds.yaml

# Install RBAC from a specific version
kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/rbac/serviceaccount.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/rbac/role.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/rbac/rolebinding.yaml

# Deploy operator from a specific version
kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/operator/deployment.yaml
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

- [Step-by-Step Guide](./step-by-step.md) - Complete walkthrough from installation to DNS testing
- [Prerequisites](./prerequisites.md) - Detailed system requirements
- [Installing CRDs](./crds.md) - Understanding the Custom Resources
- [Deploying the Operator](./controller.md) - Operator configuration options
