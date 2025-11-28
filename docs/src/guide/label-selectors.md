# Cluster References

Bindy uses direct cluster references instead of label selectors for targeting DNS zones to BIND9 instances.

## Overview

In Bindy's three-tier architecture, resources reference each other directly by name:

```
Bind9Cluster ← clusterRef ← Bind9Instance ← clusterRef ← DNSZone ← zone ← DNS Records
```

This provides:
- **Explicit targeting** - Clear, direct references instead of label matching
- **Simpler configuration** - No complex selector logic
- **Better validation** - References can be validated at admission time
- **Easier troubleshooting** - Direct relationships are easier to understand

## Cluster Reference Model

### DNSZone References Bind9Instance

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: primary-dns  # Direct reference to Bind9Instance name
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin.example.com.
```

### Bind9Instance References Bind9Cluster

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  clusterRef: production-dns  # Direct reference to Bind9Cluster name
  replicas: 2
```

### Bind9Cluster (Top-Level)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  version: "9.18"
  config:
    recursion: false
```

## How References Work

When you create a DNSZone with `clusterRef: primary-dns`:

1. **Controller finds the Bind9Instance** - Looks up `Bind9Instance` named `primary-dns`
2. **Discovers pods** - Finds pods with label `instance=primary-dns`
3. **Loads RNDC key** - Retrieves Secret named `primary-dns-rndc-key`
4. **Connects via RNDC** - Connects to `primary-dns.{namespace}.svc.cluster.local:953`
5. **Creates zone** - Executes `rndc addzone` command

## Example: Multi-Region Setup

### East Region

```yaml
# East Cluster
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: dns-cluster-east
  namespace: dns-system
spec:
  version: "9.18"

---
# East Instance
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: dns-east
  namespace: dns-system
spec:
  clusterRef: dns-cluster-east
  replicas: 2

---
# Zone on East Instance
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com-east
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: dns-east  # Targets east instance
```

### West Region

```yaml
# West Cluster
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: dns-cluster-west
  namespace: dns-system
spec:
  version: "9.18"

---
# West Instance
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: dns-west
  namespace: dns-system
spec:
  clusterRef: dns-cluster-west
  replicas: 2

---
# Zone on West Instance
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com-west
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: dns-west  # Targets west instance
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
  clusterRef: primary-dns  # Direct reference to instance name
```

## Next Steps

- [Creating Zones](./creating-zones.md) - Learn how to create zones with cluster references
- [Multi-Region Setup](./multi-region.md) - Deploy zones across multiple regions
- [RNDC-Based Architecture](../concepts/architecture-rndc.md) - Understand the RNDC protocol
