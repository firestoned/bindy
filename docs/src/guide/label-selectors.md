# Cluster References

Bindy uses direct cluster references instead of label selectors for targeting DNS zones to BIND9 instances.

## Overview

In Bindy's three-tier architecture, resources reference each other directly by name:

```
Bind9Cluster ← clusterRef ← Bind9Instance
       ↑
   clusterRef ← DNSZone ← zoneRef ← DNS Records
```

This provides:
- **Explicit targeting** - Clear, direct references instead of label matching
- **Simpler configuration** - No complex selector logic
- **Better validation** - References can be validated at admission time
- **Easier troubleshooting** - Direct relationships are easier to understand

## Cluster Reference Model

### Bind9Cluster (Top-Level)

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
```

### Bind9Instance References Bind9Cluster

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  clusterRef: production-dns  # Direct reference to Bind9Cluster name
  role: primary  # Required: primary or secondary
  replicas: 2
```

### DNSZone References Bind9Cluster

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-dns  # Direct reference to Bind9Cluster name
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
```

## How References Work

When you create a DNSZone with `clusterRef: production-dns`:

1. **Controller finds the Bind9Cluster** - Looks up `Bind9Cluster` named `production-dns`
2. **Discovers instances** - Finds all `Bind9Instance` resources referencing this cluster
3. **Identifies primaries** - Selects instances with `role: primary`
4. **Loads RNDC keys** - Retrieves RNDC keys from cluster configuration
5. **Connects via RNDC** - Connects to primary instance pods via RNDC
6. **Creates zone** - Executes `rndc addzone` command on primary instances

## Example: Multi-Region Setup

### East Region

```yaml
# East Cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: dns-cluster-east
  namespace: dns-system
spec:
  version: "9.18"

---
# East Instance
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-east
  namespace: dns-system
spec:
  clusterRef: dns-cluster-east
  role: primary  # Required: primary or secondary
  replicas: 2

---
# Zone on East Cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-east
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: dns-cluster-east  # Targets east cluster
```

### West Region

```yaml
# West Cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: dns-cluster-west
  namespace: dns-system
spec:
  version: "9.18"

---
# West Instance
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-west
  namespace: dns-system
spec:
  clusterRef: dns-cluster-west
  role: primary  # Required: primary or secondary
  replicas: 2

---
# Zone on West Cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-west
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: dns-cluster-west  # Targets west cluster
```

## Benefits Over Label Selectors

### Simpler Configuration

**Old approach (label selectors)**:
```yaml
# Had to set labels on instance
labels:
  dns-role: primary
  region: us-east

# Had to use selector in zone
instanceSelector:
  matchLabels:
    dns-role: primary
    region: us-east
```

**New approach (cluster references)**:
```yaml
# Just reference by name
clusterRef: primary-dns
```

### Better Validation

- References can be validated at admission time
- Typos are caught immediately
- No ambiguity about which instance will host the zone

### Clearer Relationships

```bash
# See exactly which instance hosts a zone
kubectl get dnszone example-com -o jsonpath='{.spec.clusterRef}'

# See which cluster an instance belongs to
kubectl get bind9instance primary-dns -o jsonpath='{.spec.clusterRef}'
```

## Migrating from Label Selectors

If you have old DNSZone resources using `instanceSelector`, migrate them:

**Before:**
```yaml
spec:
  zoneName: example.com
  instanceSelector:
    matchLabels:
      dns-role: primary
```

**After:**
```yaml
spec:
  zoneName: example.com
  clusterRef: production-dns  # Direct reference to cluster name
```

## Next Steps

- [Creating Zones](./creating-zones.md) - Learn how to create zones with cluster references
- [Multi-Region Setup](./multi-region.md) - Deploy zones across multiple regions
- [RNDC-Based Architecture](../concepts/architecture-rndc.md) - Understand the RNDC protocol
