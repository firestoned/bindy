# Prerequisites

Before installing Bindy, ensure your environment meets these requirements.

## Kubernetes Cluster

- **Kubernetes Version**: 1.24 or later
- **Access Level**: Cluster admin access (for CRD and RBAC installation)
- **Namespace**: Ability to create namespaces (recommended: `dns-system`)

### Supported Kubernetes Distributions

Bindy has been tested on:
- Kubernetes (vanilla)
- k0s
- MKE
- k0RDENT
- Amazon EKS
- Google GKE
- Azure AKS
- Red Hat OpenShift
- k3s
- kind (for development/testing)

## Client Tools

### Required

- **kubectl**: 1.24+ - [Install kubectl](https://kubernetes.io/docs/tasks/tools/)

### Optional (for development)

- **Rust**: 1.70+ - [Install Rust](https://rustup.rs/)
- **Cargo**: Included with Rust
- **Docker**: For building images - [Install Docker](https://docs.docker.com/get-docker/)

## Cluster Resources

### Minimum Requirements

- **CPU**: 100m per controller pod
- **Memory**: 128Mi per controller pod
- **Storage**: Minimal (for configuration only)

### Recommended for Production

- **CPU**: 500m per controller pod (2 replicas)
- **Memory**: 512Mi per controller pod
- **High Availability**: 3 controller replicas across different nodes

## BIND9 Infrastructure

Bindy manages existing BIND9 servers. You'll need:

- BIND9 version 9.16 or later (9.18+ recommended)
- Network connectivity from Bindy controller to BIND9 pods
- Shared volume for zone files (ConfigMap, PVC, or similar)

## Network Requirements

### Controller to API Server
- Outbound HTTPS (443) to Kubernetes API server
- Required for watching resources and updating status

### Controller to BIND9 Pods
- Access to BIND9 configuration volumes
- Typical setup uses Kubernetes ConfigMaps or PersistentVolumes

### BIND9 to Network
- UDP/TCP port 53 for DNS queries
- Port 953 for RNDC (if using remote name daemon control)
- Zone transfer ports (configured in BIND9)

## Permissions

### Cluster-Level Permissions Required

The person installing Bindy needs:

```yaml
# Ability to create CRDs
- apiGroups: ["apiextensions.k8s.io"]
  resources: ["customresourcedefinitions"]
  verbs: ["create", "get", "list"]

# Ability to create ClusterRoles and ClusterRoleBindings
- apiGroups: ["rbac.authorization.k8s.io"]
  resources: ["clusterroles", "clusterrolebindings"]
  verbs: ["create", "get", "list"]
```

### Namespace Permissions Required

For the DNS system namespace:

- Create ServiceAccounts
- Create Deployments
- Create ConfigMaps
- Create Services

## Optional Components

### For Production Deployments

- **Monitoring**: Prometheus for metrics collection
- **Logging**: Elasticsearch/Loki for log aggregation
- **GitOps**: ArgoCD or Flux for declarative management
- **Backup**: Velero for disaster recovery

### For Development

- **kind**: Local Kubernetes for testing
- **tilt**: For rapid development cycles
- **k9s**: Terminal UI for Kubernetes

## Verification

Check your cluster meets the requirements:

```bash
# Check Kubernetes version
kubectl version --short

# Check you have cluster-admin access
kubectl auth can-i create customresourcedefinitions

# Check available resources
kubectl top nodes

# Verify connectivity
kubectl cluster-info
```

Expected output:

```
Client Version: v1.28.0
Server Version: v1.27.3
```

```
yes
```

## Next Steps

Once your environment meets these prerequisites:

1. [Install CRDs](./crds.md)
2. [Deploy the Controller](./controller.md)
3. [Quick Start Guide](./quickstart.md)
