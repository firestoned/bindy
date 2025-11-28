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
- **Storage**:
  - Minimal for controller (configuration only)
  - **StorageClass**: Required for persistent zone data (optional but recommended)

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

## Storage Provisioner

For persistent zone data storage across pod restarts, you need a StorageClass configured in your cluster.

### Production Environments

Use your cloud provider's StorageClass:

- **AWS**: EBS (`gp3` or `gp2`)
- **GCP**: Persistent Disk (`pd-standard` or `pd-ssd`)
- **Azure**: Azure Disk (`managed-premium` or `managed`)
- **On-Premises**: NFS, Ceph, or other storage solutions

Verify a default StorageClass exists:

```bash
kubectl get storageclass
```

### Development/Testing (Kind, k3s, local clusters)

For local development, install the local-path provisioner:

```bash
# Install local-path provisioner
kubectl apply -f https://raw.githubusercontent.com/rancher/local-path-provisioner/v0.0.28/deploy/local-path-storage.yaml

# Wait for provisioner to be ready
kubectl wait --for=condition=available --timeout=60s \
  deployment/local-path-provisioner -n local-path-storage

# Check if local-path StorageClass was created
if kubectl get storageclass local-path &>/dev/null; then
  # Set local-path as default if no default exists
  kubectl patch storageclass local-path -p '{"metadata": {"annotations":{"storageclass.kubernetes.io/is-default-class":"true"}}}'
else
  # Create a default StorageClass using local-path provisioner
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

# Verify installation
kubectl get storageclass
```

Expected output (either `local-path` or `default` will be marked as default):
```
NAME                   PROVISIONER             RECLAIMPOLICY   VOLUMEBINDINGMODE      ALLOWVOLUMEEXPANSION   AGE
local-path (default)   rancher.io/local-path   Delete          WaitForFirstConsumer   false                  1m
```

Or:
```
NAME                PROVISIONER             RECLAIMPOLICY   VOLUMEBINDINGMODE      ALLOWVOLUMEEXPANSION   AGE
default (default)   rancher.io/local-path   Delete          WaitForFirstConsumer   false                  1m
```

> **Note**: The local-path provisioner stores data on the node's local disk. It's **not suitable for production** but works well for development and testing.

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
