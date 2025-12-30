# Bindy Examples

This directory contains example YAML manifests for deploying and configuring Bindy resources.

## ⚠️ Important: Schema Validation

All examples in this directory MUST be valid and match the CRD schemas in `/deploy/crds/`.

After any CRD changes, run:

```bash
./scripts/validate-examples.sh
```

This ensures all examples can be successfully applied to a Kubernetes cluster.

## 🔗 Understanding Resource Relationships

**CRITICAL:** The `clusterRef` field must be consistent across resources:

```
Bind9Cluster
    ↓ (referenced by clusterRef)
Bind9Instance(s)
    ↓ (referenced by clusterRef via DNSZone)
DNSZone(s)
    ↓ (discovered by label selectors via recordsFrom)
DNS Records
```

**Example:**
```yaml
# Bind9Cluster name
name: production-dns

# Bind9Instance references the cluster
spec:
  clusterRef: production-dns  # ← Must match cluster name

# DNSZone references the cluster (NOT the instance!)
spec:
  clusterRef: production-dns  # ← Must match cluster name
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com  # ← Selects records with this label

# Records are discovered via labels (no explicit zoneRef)
metadata:
  labels:
    zone: example.com  # ← Matches DNSZone selector
```

## Examples Overview

### Quick Start

0. **[complete-setup.yaml](complete-setup.yaml)** - **START HERE!** Complete working example
   - Shows the full relationship between all resources
   - Includes Bind9Cluster, Bind9Instance, DNSZone, and DNS Records
   - Heavily commented to explain how clusterRef works
   - Best example for understanding the architecture

### Multi-Tenancy

0a. **[multi-tenancy.yaml](multi-tenancy.yaml)** - **Multi-tenancy setup** with both cluster models
   - Platform-managed DNS using ClusterBind9Provider (cluster-scoped)
   - Tenant-managed DNS using Bind9Cluster (namespace-scoped)
   - Complete RBAC setup for platform and application teams
   - Three namespaces: platform-dns, team-web, team-api
   - Shows both production (shared) and development (isolated) patterns
   - ServiceAccounts, Roles, RoleBindings, and ClusterRoleBindings
   - Example zones and records for both tenancy models

### Basic Setup

1. **[bind9-cluster.yaml](bind9-cluster.yaml)** - Basic Bind9Cluster configuration
   - Defines shared settings for BIND9 instances
   - Configures DNSSEC, query permissions, and zone transfers

2. **[bind9-instance.yaml](bind9-instance.yaml)** - Primary and secondary BIND9 instances
   - Shows how to create primary and secondary DNS servers
   - Demonstrates instance labeling and roles
   - Shows correct clusterRef usage

2a. **[bind9-cluster-custom-service.yaml](bind9-cluster-custom-service.yaml)** - Custom Service configuration
   - Shows how to customize Service spec for primary and secondary instances
   - Demonstrates NodePort for external DNS access
   - Demonstrates LoadBalancer for cloud environments
   - Shows partial spec merging with defaults

3. **[dns-zone.yaml](dns-zone.yaml)** - DNS zone definitions
   - Example zones: `example.com` and `internal.local`
   - Shows SOA record configuration
   - **Updated:** Now correctly references Bind9Cluster (not instance)

3a. **[zone-label-selector.yaml](zone-label-selector.yaml)** - **Zone selection with label selectors**
   - Demonstrates automatic zone discovery using `zonesFrom` field
   - Shows production/development cluster separation
   - Examples of `matchLabels` and `matchExpressions` selectors
   - Illustrates explicit `clusterRef` override behavior
   - Multi-selector and cluster-scoped provider examples
   - Self-healing zone assignment based on labels

4. **[dns-records.yaml](dns-records.yaml)** - Various DNS record types
   - A records (IPv4 addresses)
   - CNAME records (aliases)
   - MX records (mail servers)
   - TXT records (SPF, DMARC)

### Custom Configuration

5. **[custom-zones-configmap.yaml](custom-zones-configmap.yaml)** - Custom zones configuration
   - Shows how to provide a custom `named.conf.zones` file
   - Useful for pre-configured zones or legacy zone imports
   - Demonstrates the `namedConfZones` ConfigMapRef field

### Persistent Storage

6. **[storage-pvc.yaml](storage-pvc.yaml)** - PersistentVolumeClaim examples
   - Shared storage for cluster-level configuration
   - Instance-specific storage for primary/secondary
   - ReadWriteMany example for shared access

7. **[bind9-cluster-with-storage.yaml](bind9-cluster-with-storage.yaml)** - Complete storage setup
   - Bind9Cluster with persistent volumes
   - Instance-level storage overrides

## Quick Start

### Option 1: Deploy Complete Example (Recommended)

```bash
# Install CRDs
kubectl apply -k ../deploy/crds/

# Create namespace
kubectl create namespace dns-system

# Deploy complete working setup
kubectl apply -f complete-setup.yaml
```

This creates everything with correct clusterRef relationships.

### Option 2: Deploy Step-by-Step

```bash
# 1. Install CRDs
kubectl apply -k ../deploy/crds/

# 2. Create namespace
kubectl create namespace dns-system

# 3. Create cluster definition
kubectl apply -f bind9-cluster.yaml

# 4. Create instances (references cluster via clusterRef)
kubectl apply -f bind9-instance.yaml

# 5. Create DNS zones (references cluster via clusterRef)
kubectl apply -f dns-zone.yaml

# 6. Add DNS records (discovered by zones via label selectors)
kubectl apply -f dns-records.yaml
```

**IMPORTANT:** Ensure all clusterRef values match and records have proper labels:
```bash
# Verify the cluster name
kubectl get bind9cluster -n dns-system

# Verify instances reference the correct cluster
kubectl get bind9instance -n dns-system -o yaml | grep clusterRef

# Verify zones reference the correct cluster
kubectl get dnszone -n dns-system -o yaml | grep clusterRef

# Verify records are selected by zones (check DNSZone status)
kubectl get dnszone -n dns-system -o yaml | grep -A10 "status:"
```

### With Persistent Storage

```bash
# Step 1: Create PVCs first
kubectl apply -f storage-pvc.yaml

# Step 2: Deploy cluster with storage
kubectl apply -f bind9-cluster-with-storage.yaml

# Continue with zones and records...
```

## Validation

Before applying to a cluster, validate the manifests:

```bash
# Dry-run validation
kubectl apply --dry-run=client -f bind9-cluster.yaml
kubectl apply --dry-run=client -f bind9-instance.yaml
kubectl apply --dry-run=client -f dns-zone.yaml
kubectl apply --dry-run=client -f dns-records.yaml

# Or use the validation script for all examples
../scripts/validate-examples.sh
```

## Customization

These examples use placeholder values. Customize them for your environment:

- **Namespaces**: Change from `dns-system` to your namespace
- **IP Addresses**: Replace example IPs with your actual IPs
- **Zone Names**: Use your actual domain names
- **Storage Sizes**: Adjust PVC sizes based on your needs
- **Replicas**: Set appropriate replica counts for your HA requirements

## Notes

- All examples use the API group `bindy.firestoned.io/v1beta1`
- Email addresses in SOA records use `.` instead of `@` (e.g., `admin.example.com.`)
- DNS names in records must end with `.` (FQDN format)
- Records are associated with zones via label selectors (see `DNSZone.spec.recordsFrom`)
- The `DNSZone.status.records` field shows all discovered records for a zone

## See Also

- [Quickstart Guide](../docs/src/installation/quickstart.md)
- [API Reference](../docs/src/reference/api.md)
- [CRD Schemas](../deploy/crds/)
