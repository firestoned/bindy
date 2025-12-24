# Bind9Cluster

The `Bind9Cluster` resource represents a logical DNS cluster - a collection of related BIND9 instances with shared configuration.

## Overview

A Bind9Cluster defines cluster-level configuration that can be inherited by multiple Bind9Instance resources:
- Shared BIND9 version and container image
- Common configuration (recursion, ACLs, etc.)
- Custom ConfigMap references for BIND9 configuration files
- TSIG keys for authenticated zone transfers
- Access Control Lists (ACLs)

## Example

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  version: "9.18"
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"
  rndcSecretRefs:
    - name: transfer-key
      algorithm: hmac-sha256
      secret: "base64-encoded-secret"
  acls:
    internal:
      - "10.0.0.0/8"
      - "172.16.0.0/12"
    external:
      - "0.0.0.0/0"
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ClusterConfigured
      message: "Cluster configured successfully"
  instanceCount: 4
  readyInstances: 4
```

## Specification

### Optional Fields

- `spec.version` - BIND9 version for all instances in the cluster
- `spec.image` - Container image configuration for all instances
  - `image` - Full container image reference (registry/repo:tag)
  - `imagePullPolicy` - Image pull policy (Always, IfNotPresent, Never)
  - `imagePullSecrets` - List of secret names for private registries
- `spec.configMapRefs` - Custom ConfigMap references for BIND9 configuration
  - `namedConf` - Name of ConfigMap containing named.conf
  - `namedConfOptions` - Name of ConfigMap containing named.conf.options
- `spec.global` - Shared BIND9 configuration
  - `recursion` - Enable/disable recursion globally
  - `allowQuery` - List of CIDR ranges allowed to query
  - `allowTransfer` - List of CIDR ranges allowed zone transfers
  - `dnssec` - DNSSEC configuration
  - `forwarders` - DNS forwarders
  - `listenOn` - IPv4 addresses to listen on
  - `listenOnV6` - IPv6 addresses to listen on
- `spec.primary` - Primary instance configuration
  - `replicas` - Number of primary instances to create (managed instances)
- `spec.secondary` - Secondary instance configuration
  - `replicas` - Number of secondary instances to create (managed instances)
- `spec.tsigKeys` - TSIG keys for authenticated zone transfers
  - `name` - Key name
  - `algorithm` - HMAC algorithm (hmac-sha256, hmac-sha512, etc.)
  - `secret` - Base64-encoded shared secret
- `spec.acls` - Named ACL definitions that instances can reference

## Cluster vs Instance

The relationship between Bind9Cluster and Bind9Instance:

```yaml
# Cluster defines shared configuration
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: prod-cluster
spec:
  version: "9.18"
  global:
    recursion: false
  acls:
    internal:
      - "10.0.0.0/8"

---
# Instance references the cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: primary-dns
  labels:
    cluster: prod-cluster
    dns-role: primary
spec:
  clusterRef: prod-cluster
  role: primary
  replicas: 2
  # Instance-specific config can override cluster defaults
  config:
    allowQuery:
      - acl:internal  # Reference the cluster's ACL
```

## TSIG Keys

TSIG (Transaction SIGnature) keys provide authenticated zone transfers:

```yaml
spec:
  rndcSecretRefs:
    - name: primary-secondary-key
      algorithm: hmac-sha256
      secret: "K8x...base64...=="
    - name: backup-key
      algorithm: hmac-sha512
      secret: "L9y...base64...=="
```

These keys are used by:
- Primary instances for authenticated zone transfers to secondaries
- Secondary instances to authenticate when requesting zone transfers
- Dynamic DNS updates (if enabled)

## Access Control Lists (ACLs)

ACLs define reusable network access policies:

```yaml
spec:
  acls:
    # Internal networks
    internal:
      - "10.0.0.0/8"
      - "172.16.0.0/12"
      - "192.168.0.0/16"

    # External clients
    external:
      - "0.0.0.0/0"

    # Secondary DNS servers
    secondaries:
      - "10.0.1.10"
      - "10.0.2.10"
      - "10.0.3.10"
```

Instances can then reference these ACLs:

```yaml
# In Bind9Instance spec
config:
  allowQuery:
    - acl:external
  allowTransfer:
    - acl:secondaries
```

## Status

The controller updates status to reflect cluster state:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ClusterConfigured
      message: "Cluster configured with 4 instances"
  instanceCount: 4      # Total instances in cluster
  readyInstances: 4     # Instances reporting ready
  observedGeneration: 1
```

## Managed Instances

Bind9Cluster can automatically create and manage Bind9Instance resources based on the `spec.primary.replicas` and `spec.secondary.replicas` fields.

### Automatic Scaling

The operator automatically scales instances up and down based on the replica counts in the cluster spec:

**Scale-Up**: When you increase replica counts, the operator creates missing instances
**Scale-Down**: When you decrease replica counts, the operator deletes excess instances (highest-indexed first)

When you specify replica counts in the cluster spec, the operator automatically creates the corresponding instances:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  version: "9.18"
  primary:
    replicas: 2  # Creates 2 primary instances
  secondary:
    replicas: 3  # Creates 3 secondary instances
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

This cluster definition will automatically create 5 Bind9Instance resources:
- `production-dns-primary-0`
- `production-dns-primary-1`
- `production-dns-secondary-0`
- `production-dns-secondary-1`
- `production-dns-secondary-2`

### Management Labels

All managed instances are labeled with:
- `bindy.firestoned.io/managed-by: "Bind9Cluster"` - Identifies cluster-managed instances
- `bindy.firestoned.io/cluster: "<cluster-name>"` - Links instance to parent cluster
- `bindy.firestoned.io/role: "primary"|"secondary"` - Indicates instance role

And annotated with:
- `bindy.firestoned.io/instance-index: "<index>"` - Sequential index for the instance

Example of a managed instance:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: production-dns-primary-0
  namespace: dns-system
  labels:
    bindy.firestoned.io/managed-by: "Bind9Cluster"
    bindy.firestoned.io/cluster: "production-dns"
    bindy.firestoned.io/role: "primary"
  annotations:
    bindy.firestoned.io/instance-index: "0"
spec:
  clusterRef: production-dns
  role: Primary
  replicas: 1
  version: "9.18"
  # Configuration inherited from cluster's spec.global
```

### Configuration Inheritance

Managed instances automatically inherit configuration from the cluster:
- BIND9 version (`spec.version`)
- Container image (`spec.image`)
- ConfigMap references (`spec.configMapRefs`)
- Volumes and volume mounts
- Global configuration (`spec.global`)

### Self-Healing

The Bind9Cluster controller provides comprehensive self-healing for managed instances:

**Instance-Level Self-Healing:**
- If a managed instance (Bind9Instance CRD) is deleted (manually or accidentally), the controller automatically recreates it during the next reconciliation cycle

**Resource-Level Self-Healing:**
- If any child resource is deleted, the controller automatically triggers recreation:
  - **ConfigMap** - BIND9 configuration files
  - **Secret** - RNDC key for remote control
  - **Service** - DNS traffic routing (TCP/UDP port 53)
  - **Deployment** - BIND9 pods

This ensures complete desired state is maintained even if individual Kubernetes resources are manually deleted or corrupted.

**Example self-healing scenario:**
```bash
# Manually delete a ConfigMap
kubectl delete configmap production-dns-primary-0-config -n dns-system

# During next reconciliation (~10 seconds), the controller:
# 1. Detects missing ConfigMap
# 2. Triggers Bind9Instance reconciliation
# 3. Recreates ConfigMap with correct configuration
# 4. BIND9 pod automatically remounts updated ConfigMap
```

**Example scaling scenario:**
```bash
# Initial cluster with 2 primary instances
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  primary:
    replicas: 2
EOF

# Controller creates: production-dns-primary-0, production-dns-primary-1

# Scale up to 4 primaries
kubectl patch bind9cluster production-dns -n dns-system --type=merge -p '{"spec":{"primary":{"replicas":4}}}'

# Controller creates: production-dns-primary-2, production-dns-primary-3

# Scale down to 3 primaries
kubectl patch bind9cluster production-dns -n dns-system --type=merge -p '{"spec":{"primary":{"replicas":3}}}'

# Controller deletes: production-dns-primary-3 (highest index first)
```

### Manual vs Managed Instances

You can mix managed and manual instances:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: mixed-cluster
spec:
  version: "9.18"
  primary:
    replicas: 2  # Managed instances
  # No secondary replicas - create manually
---
# Manual instance with custom configuration
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: custom-secondary
spec:
  clusterRef: mixed-cluster
  role: Secondary
  replicas: 1
  # Custom configuration overrides
  config:
    allowQuery:
      - "192.168.1.0/24"
```

## Lifecycle Management

### Cascade Deletion

When a Bind9Cluster is deleted, the operator automatically deletes all instances that reference it via `spec.clusterRef`. This ensures clean removal of all cluster resources.

**Finalizer:** `bindy.firestoned.io/bind9cluster-finalizer`

The cluster resource uses a finalizer to ensure proper cleanup before deletion:

```bash
# Delete the cluster
kubectl delete bind9cluster production-dns

# The operator will:
# 1. Detect deletion timestamp
# 2. Find all instances with clusterRef: production-dns
# 3. Delete each instance
# 4. Remove finalizer
# 5. Allow cluster deletion to complete
```

**Example deletion logs:**

```
INFO Deleting Bind9Cluster production-dns
INFO Found 5 instances to delete
INFO Deleted instance production-dns-primary-0
INFO Deleted instance production-dns-primary-1
INFO Deleted instance production-dns-secondary-0
INFO Deleted instance production-dns-secondary-1
INFO Deleted instance production-dns-secondary-2
INFO Removed finalizer from cluster
INFO Cluster deletion complete
```

### Important Warnings

⚠️ **Deleting a Bind9Cluster will delete ALL instances that reference it**, including:
- Managed instances (created by `spec.primary.replicas` and `spec.secondary.replicas`)
- Manual instances (created separately but referencing the cluster via `spec.clusterRef`)

To preserve instances during cluster deletion, remove the `spec.clusterRef` field from instances first:

```bash
# Remove clusterRef from an instance to preserve it
kubectl patch bind9instance my-instance --type=json -p='[{"op": "remove", "path": "/spec/clusterRef"}]'

# Now safe to delete the cluster without affecting this instance
kubectl delete bind9cluster production-dns
```

### Troubleshooting Stuck Deletions

If a cluster is stuck in `Terminating` state:

```bash
# Check for finalizers
kubectl get bind9cluster production-dns -o jsonpath='{.metadata.finalizers}'

# Check operator logs
kubectl logs -n dns-system deployment/bindy -f

# If operator is not running, manually remove finalizer (last resort)
kubectl patch bind9cluster production-dns -p '{"metadata":{"finalizers":null}}' --type=merge
```

## Use Cases

### Multi-Region DNS Cluster

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: global-dns
spec:
  version: "9.18"
  global:
    recursion: false
    dnssec:
      enabled: true
      validation: true
  rndcSecretRefs:
    - name: region-sync-key
      algorithm: hmac-sha256
      secret: "..."
  acls:
    us-east:
      - "10.1.0.0/16"
    us-west:
      - "10.2.0.0/16"
    eu-west:
      - "10.3.0.0/16"
```

### Development Cluster

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: dev-dns
  namespace: dns-system
spec:
  version: "9.18"
  global:
    recursion: true  # Allow recursion for dev
    allowQuery:
      - "0.0.0.0/0"
    forwarders:
      - "8.8.8.8"
      - "8.8.4.4"
  acls:
    dev-team:
      - "192.168.1.0/24"
```

### Custom Image Cluster

Use a custom container image across all instances:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: custom-image-cluster
  namespace: dns-system
spec:
  version: "9.18"
  # Custom image with organization-specific patches
  image:
    image: "my-registry.example.com/bind9:9.18-custom"
    imagePullPolicy: "IfNotPresent"
    imagePullSecrets:
      - docker-registry-secret
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

All Bind9Instances referencing this cluster will inherit the custom image configuration unless they override it.

### Custom ConfigMap Cluster

Share custom BIND9 configuration files across all instances:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: shared-bind9-options
  namespace: dns-system
data:
  named.conf.options: |
    options {
      directory "/var/cache/bind";
      recursion no;
      allow-query { any; };
      allow-transfer { 10.0.2.0/24; };
      dnssec-validation auto;

      # Custom logging
      querylog yes;

      # Rate limiting
      rate-limit {
        responses-per-second 10;
        window 5;
      };
    };
---
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: custom-config-cluster
  namespace: dns-system
spec:
  version: "9.18"
  configMapRefs:
    namedConfOptions: "shared-bind9-options"
```

All instances in this cluster will use the custom configuration, while named.conf is auto-generated.

## Best Practices

1. **One cluster per environment** - Separate clusters for production, staging, development
2. **Consistent TSIG keys** - Use the same keys across all instances in a cluster
3. **Version pinning** - Specify exact BIND9 versions to avoid unexpected updates
4. **ACL organization** - Define ACLs at cluster level for consistency
5. **DNSSEC** - Enable DNSSEC at the cluster level for all zones
6. **Image management** - Define container images at cluster level for consistency; override at instance level only for canary testing
7. **ConfigMap strategy** - Use cluster-level ConfigMaps for shared configuration; use instance-level ConfigMaps for instance-specific customizations
8. **Image pull secrets** - Configure imagePullSecrets at cluster level to avoid duplicating secrets across instances

## Next Steps

- [Bind9Instance](./bind9instance.md) - Learn about DNS instances
- [DNSZone](./dnszone.md) - Learn about DNS zones
- [Multi-Region Setup](../guide/multi-region.md) - Deploy across multiple regions
