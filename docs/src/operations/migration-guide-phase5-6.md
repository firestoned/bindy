# Migration Guide: Phase 5-6 (DNSZone Consolidation)

Guide for migrating from multiple zone selection methods to the unified label selector approach.

!!! warning "Breaking Changes"
    Phases 5-6 of the DNSZone consolidation involved breaking API changes. This guide helps you migrate existing resources.

## Overview

The DNSZone API has been simplified to use a single, unified label selector approach for identifying target Bind9Clusters and Bind9Instances, replacing the previous multiple methods (`clusterRef`, `nameserverIPs`, `nameservers`).

## What Changed

### Old API (Pre-v0.3.0)

Multiple ways to specify DNS servers:

```yaml
apiVersion: dns.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com

  # Option 1: Direct cluster reference
  clusterRef:
    name: my-cluster
    namespace: dns-system

  # Option 2: IP addresses
  nameserverIPs:
    - 10.0.0.10
    - 10.0.0.11

  # Option 3: Nameserver details
  nameservers:
    - name: ns1
      ipAddress: 10.0.0.10
```

### New API (v0.3.0+)

Unified label selector approach:

```yaml
apiVersion: dns.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com

  # Single unified method: label selectors
  bind9ClusterSelector:
    matchLabels:
      environment: production
      region: us-west

  # Or select specific instances
  bind9InstanceSelector:
    matchLabels:
      role: primary
```

## Migration Steps

### Step 1: Inventory Existing DNSZones

```bash
# List all DNSZones using old API
kubectl get dnszones -A -o yaml > dnszones-backup.yaml

# Count zones by selection method
kubectl get dnszones -A -o json | jq -r '.items[] |
  if .spec.clusterRef then "clusterRef"
  elif .spec.nameserverIPs then "nameserverIPs"
  elif .spec.nameservers then "nameservers"
  else "unknown" end' | sort | uniq -c
```

### Step 2: Add Labels to Clusters/Instances

Before migrating DNSZones, ensure all Bind9Clusters and Bind9Instances have appropriate labels:

```bash
# Add labels to existing clusters
kubectl label bind9cluster my-cluster \
  environment=production \
  region=us-west \
  -n dns-system

# Add labels to existing instances
kubectl label bind9instance my-instance \
  role=primary \
  zone=example-com \
  -n dns-system
```

### Step 3: Migrate DNSZone Definitions

#### From `clusterRef`

```yaml
# OLD
spec:
  clusterRef:
    name: production-cluster
    namespace: dns-system

# NEW - Add labels to the cluster first, then use selector
spec:
  bind9ClusterSelector:
    matchLabels:
      cluster-name: production-cluster  # Add this label to the cluster
```

#### From `nameserverIPs`

```yaml
# OLD
spec:
  nameserverIPs:
    - 10.0.0.10
    - 10.0.0.11

# NEW - Label instances with their IPs, then use selector
spec:
  bind9InstanceSelector:
    matchLabels:
      ip-group: primary-nameservers  # Add this label to instances
```

#### From `nameservers`

```yaml
# OLD
spec:
  nameservers:
    - name: ns1.example.com
      ipAddress: 10.0.0.10
    - name: ns2.example.com
      ipAddress: 10.0.0.11

# NEW - Label instances, then use selector
spec:
  bind9InstanceSelector:
    matchLabels:
      nameserver-set: example-com  # Add this label to instances
```

### Step 4: Apply Updated Definitions

```bash
# Apply updated DNSZone (one at a time for safety)
kubectl apply -f updated-dnszone.yaml

# Verify zone was reconciled
kubectl get dnszone example-com -o yaml | grep -A10 status
```

### Step 5: Verify DNS Configuration

```bash
# Check that BIND9 instances have correct zone configuration
kubectl exec -it <bind9-pod> -n dns-system -- \
  named-checkconf /etc/bind/named.conf

# Verify zone file exists
kubectl exec -it <bind9-pod> -n dns-system -- \
  ls -l /etc/bind/zones/

# Test DNS resolution
dig @<instance-ip> example.com SOA
```

## Label Selector Best Practices

### Environment-Based Selection

```yaml
spec:
  bind9ClusterSelector:
    matchLabels:
      environment: production
      region: us-west
```

### Role-Based Selection

```yaml
spec:
  bind9InstanceSelector:
    matchLabels:
      role: primary
      zone-type: authoritative
```

### Combined Selectors

```yaml
spec:
  # Select clusters in production
  bind9ClusterSelector:
    matchLabels:
      environment: production

  # Then select primary instances within those clusters
  bind9InstanceSelector:
    matchLabels:
      role: primary
```

## Troubleshooting

### Zone Not Reconciling

Check if selectors match any resources:

```bash
# List clusters matching selector
kubectl get bind9cluster -A \
  -l environment=production,region=us-west

# List instances matching selector
kubectl get bind9instance -A \
  -l role=primary
```

### Multiple Matches

If selectors match more than expected:

```bash
# Add more specific labels to narrow selection
kubectl label bind9cluster my-cluster zone-group=example --overwrite
```

### Migration Validation

```bash
# Compare old and new configurations
diff -u old-dnszone.yaml new-dnszone.yaml

# Check operator logs for reconciliation
kubectl logs -n dns-system deployment/bindy-operator -f
```

## Rollback Procedure

If migration causes issues:

```bash
# Restore from backup
kubectl apply -f dnszones-backup.yaml

# Or revert individual zone
kubectl replace -f old-dnszone.yaml --force
```

!!! warning "Downgrade Not Supported"
    Once migrated to v0.3.0+, downgrading to v0.2.x requires manual intervention as the old fields are no longer in the CRD schema.

## API Deprecation Timeline

| Version | Status | `clusterRef` | `nameserverIPs` | `nameservers` | `*Selector` |
|---------|--------|--------------|-----------------|---------------|-------------|
| v0.2.x  | Old    | ✅ Supported | ✅ Supported    | ✅ Supported  | ❌ Not available |
| v0.3.0  | Current| ⚠️  Removed   | ⚠️  Removed     | ⚠️  Removed   | ✅ Supported |
| v1.0.0  | Future | ❌ Removed   | ❌ Removed      | ❌ Removed    | ✅ Supported |

## Related Documentation

- [Zone Selection Guide](../guide/zone-selection.md) - Using label selectors effectively
- [Label Selectors](../guide/label-selectors.md) - Understanding Kubernetes label selectors
- [DNSZone Troubleshooting](dnszone-migration-troubleshooting.md) - Common migration issues
- [DNSZone API Reference](../reference/dnszone-spec.md) - Complete API specification
- [Changelog](https://github.com/firestoned/bindy/blob/main/CHANGELOG.md) - Full migration history

## Support

For migration assistance:
- Open an issue on [GitHub](https://github.com/firestoned/bindy/issues)
- Check [Common Issues](common-issues.md)
- Review [Troubleshooting Guide](troubleshooting.md)
