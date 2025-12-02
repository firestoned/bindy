# Labels and Annotations Reference

This page documents all labels and annotations used by Bindy for resource management and tracking.

## Overview

Bindy uses labels and annotations to:
- Track ownership and relationships between resources
- Identify managed vs. manual resources
- Enable declarative instance management
- Provide metadata for automation and monitoring

## Namespace

All Bindy labels and annotations use the prefix:
```
bindy.firestoned.io/
```

## Labels

### Management Labels

Labels used to identify cluster-managed instances:

#### `bindy.firestoned.io/managed-by`

**Applied to:** Bind9Instance

**Purpose:** Identifies instances that are managed by a Bind9Cluster

**Values:**
- `Bind9Cluster` - Instance is managed by a cluster's `spec.primary.replicas` or `spec.secondary.replicas`

**Example:**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: production-dns-primary-0
  labels:
    bindy.firestoned.io/managed-by: "Bind9Cluster"
```

**Usage:**
- Set automatically by Bind9Cluster controller when creating managed instances
- Used to identify instances for self-healing (automatic recreation)
- Used to filter managed vs. manual instances

**Filter managed instances:**
```bash
kubectl get bind9instances -A \
  -l bindy.firestoned.io/managed-by=Bind9Cluster
```

---

#### `bindy.firestoned.io/cluster`

**Applied to:** Bind9Instance

**Purpose:** Links instance to its parent Bind9Cluster

**Values:** Name of the parent Bind9Cluster resource

**Example:**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: production-dns-primary-0
  labels:
    bindy.firestoned.io/cluster: "production-dns"
```

**Usage:**
- Set automatically by Bind9Cluster controller
- Used for cascade deletion (find all instances belonging to a cluster)
- Used for grouping and filtering instances by cluster

**List all instances in a cluster:**
```bash
kubectl get bind9instances -A \
  -l bindy.firestoned.io/cluster=production-dns
```

---

#### `bindy.firestoned.io/role`

**Applied to:** Bind9Instance

**Purpose:** Indicates the DNS role of the instance

**Values:**
- `primary` - Instance is a primary DNS server
- `secondary` - Instance is a secondary DNS server

**Example:**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: production-dns-primary-0
  labels:
    bindy.firestoned.io/role: "primary"
```

**Usage:**
- Set automatically by Bind9Cluster controller
- Used to differentiate between primary and secondary instances
- Used for role-based filtering and monitoring

**List all primary instances:**
```bash
kubectl get bind9instances -A \
  -l bindy.firestoned.io/role=primary
```

**List all secondary instances:**
```bash
kubectl get bind9instances -A \
  -l bindy.firestoned.io/role=secondary
```

---

### User-Defined Labels

Users can add custom labels to any Bindy resource for organization and filtering:

**Example:**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: production-dns
  labels:
    environment: production
    team: platform
    cost-center: "12345"
```

**Common patterns:**
- `environment: production|staging|dev`
- `team: <team-name>`
- `region: us-east-1|us-west-2|eu-west-1`
- `purpose: internal|external|testing`

## Annotations

### Management Annotations

Annotations used to store instance metadata:

#### `bindy.firestoned.io/instance-index`

**Applied to:** Bind9Instance

**Purpose:** Stores the sequential index of a managed instance

**Values:** String representation of integer (e.g., `"0"`, `"1"`, `"2"`)

**Example:**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: production-dns-primary-0
  annotations:
    bindy.firestoned.io/instance-index: "0"
```

**Usage:**
- Set automatically by Bind9Cluster controller
- Used to determine instance naming: `<cluster-name>-<role>-<index>`
- Helps track instance order for primary/secondary relationships
- Used for recreation to maintain consistent naming

**View instance index:**
```bash
kubectl get bind9instance production-dns-primary-0 \
  -o jsonpath='{.metadata.annotations.bindy\.firestoned\.io/instance-index}'
```

---

### User-Defined Annotations

Users can add custom annotations for metadata and automation:

**Example:**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  annotations:
    contact: "dns-team@example.com"
    description: "Production zone for example.com"
    docs: "https://wiki.example.com/dns/example-com"
    last-audit: "2025-11-30"
```

**Common patterns:**
- `contact: <email>` - Team or person responsible
- `description: <text>` - Human-readable description
- `docs: <url>` - Link to documentation
- `ticket: <id>` - Reference to change request
- `last-audit: <date>` - Compliance tracking

## Combined Usage Examples

### Managed Instance with All Labels

A complete managed instance created by Bind9Cluster:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: production-dns-primary-0
  namespace: dns-system
  labels:
    # Bindy management labels
    bindy.firestoned.io/managed-by: "Bind9Cluster"
    bindy.firestoned.io/cluster: "production-dns"
    bindy.firestoned.io/role: "primary"
    # User-defined labels
    environment: production
    region: us-east-1
  annotations:
    # Bindy management annotation
    bindy.firestoned.io/instance-index: "0"
    # User-defined annotations
    contact: "platform-team@example.com"
spec:
  clusterRef: production-dns
  role: Primary
  replicas: 1
```

### Finding Resources

#### All managed instances in a cluster:
```bash
kubectl get bind9instances -A \
  -l bindy.firestoned.io/managed-by=Bind9Cluster,bindy.firestoned.io/cluster=production-dns
```

#### All primary instances across all clusters:
```bash
kubectl get bind9instances -A \
  -l bindy.firestoned.io/role=primary
```

#### All managed primary instances:
```bash
kubectl get bind9instances -A \
  -l bindy.firestoned.io/managed-by=Bind9Cluster,bindy.firestoned.io/role=primary
```

#### Production instances only:
```bash
kubectl get bind9instances -A \
  -l environment=production
```

### Counting Resources

Count managed instances per cluster:

```bash
# Total managed instances
kubectl get bind9instances -A \
  -l bindy.firestoned.io/managed-by=Bind9Cluster \
  --no-headers | wc -l

# Managed instances for specific cluster
kubectl get bind9instances -A \
  -l bindy.firestoned.io/cluster=production-dns \
  --no-headers | wc -l

# Primary vs secondary breakdown
echo "Primary: $(kubectl get bind9instances -A -l bindy.firestoned.io/role=primary --no-headers | wc -l)"
echo "Secondary: $(kubectl get bind9instances -A -l bindy.firestoned.io/role=secondary --no-headers | wc -l)"
```

### Monitoring and Alerting

Use labels in Prometheus queries:

```promql
# Count instances by role
count by (role) (
  kube_pod_labels{
    label_bindy_firestoned_io_role=~"primary|secondary"
  }
)

# Alert on missing managed instances
(
  kube_deployment_spec_replicas{deployment="bindy"}
  -
  count(kube_pod_labels{label_bindy_firestoned_io_managed_by="Bind9Cluster"})
) > 0
```

## Label and Annotation Lifecycle

### Creation

**Managed instances:**
- Labels and annotations are set automatically by Bind9Cluster controller
- Users should NOT manually set Bindy management labels/annotations
- User-defined labels/annotations can be added

**Manual instances:**
- No Bindy management labels/annotations are set
- Users can add any labels/annotations they want

### Updates

**Managed instances:**
- Bindy management labels/annotations are immutable (managed by controller)
- User-defined labels/annotations can be modified
- Changes to management labels will be reverted by controller

**Manual instances:**
- All labels/annotations can be freely modified

### Deletion

When a Bind9Cluster is deleted:
1. Controller finds all instances with matching `bindy.firestoned.io/cluster` label
2. Deletes each instance (cascade deletion)
3. Labels and annotations are removed with the instance

## Best Practices

### 1. Don't Modify Management Labels

**❌ Bad:**
```bash
# Don't manually modify management labels
kubectl label bind9instance production-dns-primary-0 \
  bindy.firestoned.io/role=secondary
```

**✅ Good:**
```bash
# Let the controller manage these labels
# They are set based on cluster spec
```

### 2. Use Consistent Labeling Scheme

**❌ Bad:**
```yaml
# Inconsistent labels across resources
metadata:
  labels:
    env: prod      # Some use "env"
    environment: production  # Others use "environment"
```

**✅ Good:**
```yaml
# Consistent labels
metadata:
  labels:
    environment: production
    team: platform
    region: us-east-1
```

### 3. Document Custom Labels

Create a labeling policy document for your organization:

```yaml
# labels-policy.yaml
# Standard labels for all Bindy resources
environment:
  values: [production, staging, development]
  required: true
team:
  values: [platform, infrastructure, security]
  required: true
region:
  values: [us-east-1, us-west-2, eu-west-1]
  required: false
```

### 4. Use Labels for Cost Allocation

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: production-dns
  labels:
    environment: production
    team: platform
    cost-center: "12345"
    business-unit: "infrastructure"
```

Then track costs:
```bash
# Cost by team
kubectl get bind9clusters -A -L team

# Cost by environment
kubectl get bind9clusters -A -L environment
```

### 5. Avoid High-Cardinality Labels

**❌ Bad:**
```yaml
metadata:
  labels:
    timestamp: "2025-11-30T12:34:56Z"  # Changes constantly
    request-id: "abc123"               # Unique per request
```

**✅ Good:**
```yaml
metadata:
  annotations:
    created-at: "2025-11-30T12:34:56Z"
    created-by: "user@example.com"
```

## Querying Tips

### Label Selectors

```bash
# Equality-based
kubectl get bind9instances -l bindy.firestoned.io/role=primary

# Set-based
kubectl get bind9instances -l 'bindy.firestoned.io/role in (primary,secondary)'

# Multiple labels (AND)
kubectl get bind9instances -l environment=production,team=platform

# Negation
kubectl get bind9instances -l 'bindy.firestoned.io/managed-by!=Bind9Cluster'

# Label exists
kubectl get bind9instances -l bindy.firestoned.io/managed-by

# Label does not exist
kubectl get bind9instances -l '!bindy.firestoned.io/managed-by'
```

### JSON Path Queries

```bash
# Get instance index
kubectl get bind9instance my-instance \
  -o jsonpath='{.metadata.annotations.bindy\.firestoned\.io/instance-index}'

# Get cluster name
kubectl get bind9instance my-instance \
  -o jsonpath='{.metadata.labels.bindy\.firestoned\.io/cluster}'

# List all managed instances with their roles
kubectl get bind9instances -A \
  -o jsonpath='{range .items[?(@.metadata.labels.bindy\.firestoned\.io/managed-by=="Bind9Cluster")]}{.metadata.name}{"\t"}{.metadata.labels.bindy\.firestoned\.io/role}{"\n"}{end}'
```

## See Also

- [Bind9Cluster Spec](./bind9cluster-spec.md) - Cluster resource specification
- [Bind9Instance Spec](./bind9instance-spec.md) - Instance resource specification
- [Kubernetes Labels and Selectors](https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/)
- [Kubernetes Annotations](https://kubernetes.io/docs/concepts/overview/working-with-objects/annotations/)
