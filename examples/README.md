# Bindy Examples

This directory contains example YAML manifests for deploying and configuring Bindy resources.

## ⚠️ Important: Schema Validation

All examples in this directory MUST be valid and match the CRD schemas in `/deploy/crds/`.

After any CRD changes, run:

```bash
./scripts/validate-examples.sh
```

This ensures all examples can be successfully applied to a Kubernetes cluster.

## Examples Overview

### Basic Setup

1. **[bind9-cluster.yaml](bind9-cluster.yaml)** - Basic Bind9Cluster configuration
   - Defines shared settings for BIND9 instances
   - Configures DNSSEC, query permissions, and zone transfers

2. **[bind9-instance.yaml](bind9-instance.yaml)** - Primary and secondary BIND9 instances
   - Shows how to create primary and secondary DNS servers
   - Demonstrates instance labeling and roles

3. **[dns-zone.yaml](dns-zone.yaml)** - DNS zone definitions
   - Example zones: `example.com` and `internal.local`
   - Shows SOA record configuration

4. **[dns-records.yaml](dns-records.yaml)** - Various DNS record types
   - A records (IPv4 addresses)
   - CNAME records (aliases)
   - MX records (mail servers)
   - TXT records (SPF, DMARC)

### Persistent Storage

5. **[storage-pvc.yaml](storage-pvc.yaml)** - PersistentVolumeClaim examples
   - Shared storage for cluster-level configuration
   - Instance-specific storage for primary/secondary
   - ReadWriteMany example for shared access

6. **[bind9-cluster-with-storage.yaml](bind9-cluster-with-storage.yaml)** - Complete storage setup
   - Bind9Cluster with persistent volumes
   - Instance-level storage overrides

## Quick Start

### 1. Install CRDs

```bash
kubectl apply -k ../deploy/crds/
```

### 2. Create Namespace

```bash
kubectl create namespace dns-system
```

### 3. Deploy in Order

```bash
# Step 1: Create cluster definition
kubectl apply -f bind9-cluster.yaml

# Step 2: Create instances
kubectl apply -f bind9-instance.yaml

# Step 3: Create DNS zones
kubectl apply -f dns-zone.yaml

# Step 4: Add DNS records
kubectl apply -f dns-records.yaml
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

- All examples use the API group `bindy.firestoned.io/v1alpha1`
- Email addresses in SOA records use `.` instead of `@` (e.g., `admin.example.com.`)
- DNS names in records must end with `.` (FQDN format)
- Zone references use the metadata name, not the zone name (e.g., `example-com` not `example.com`)

## See Also

- [Quickstart Guide](../docs/src/installation/quickstart.md)
- [API Reference](../docs/src/reference/api.md)
- [CRD Schemas](../deploy/crds/)
