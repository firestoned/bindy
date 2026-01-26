# Creating Zones

Learn how to create DNS zones in Bindy and deploy them to BIND9 instances.

## Zone Architecture

> **Architecture**: In Bindy, **zones select instances** (not the other way around). A `DNSZone` resource declares which `Bind9Instance` resources should serve it.

Zones in Bindy follow a three-tier model:

1. **Bind9Cluster** - Cluster-level configuration (version, shared config, TSIG keys)
2. **Bind9Instance** - Individual BIND9 server deployment (references a cluster)
3. **DNSZone** - DNS zone (selects instances via `clusterRef` or `bind9InstancesFrom`)

## Prerequisites

Before creating a zone, ensure you have:

1. A Bind9Cluster resource deployed
2. A Bind9Instance resource deployed (referencing the cluster)
3. The instance is ready and running

## Creating a Primary Zone

First, ensure you have a cluster and instance:

```yaml
# Step 1: Create a Bind9Cluster (if not already created)
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

---
# Step 2: Create a Bind9Instance (if not already created)
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  clusterRef: production-dns  # References the Bind9Cluster above
  role: primary
  replicas: 1

---
# Step 3: Create the DNSZone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-dns  # Selects all instances with spec.clusterRef: production-dns
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.  # Note: @ replaced with .
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
```

## Instance Selection Methods

There are three ways to deploy a zone to instances:

### Method 1: Cluster Reference (Simplest)

Reference a cluster, and the zone will be served by all instances in that cluster:

```yaml
spec:
  clusterRef: production-dns  # Matches instances with spec.clusterRef: production-dns
```

### Method 2: Label Selectors (Most Flexible)

Use label selectors to choose instances based on labels:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
  labels:
    zone: example.com
spec:
  zoneName: example.com
  bind9InstancesFrom:
    - selector:
        matchLabels:
          dns-role: primary
          environment: production
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
  ttl: 3600
```

This deploys the zone to all instances with **both** labels.

### Method 3: Combined (Most Control)

Use both methods - the zone will be served by the UNION of instances from both:

```yaml
spec:
  clusterRef: production-dns
  bind9InstancesFrom:
    - selector:
        matchLabels:
          region: us-west
```

For detailed guidance, see [Zone Selection Guide](./zone-selection.md).

## How It Works

When you create a DNSZone:

1. **Zone selects instances** - Operator evaluates `clusterRef` and/or `bind9InstancesFrom` to select instances
2. **Deduplicates instances** - If using both methods, duplicates are removed (UID-based)
3. **For each selected instance**:
   - Retrieves bindcar HTTP API endpoint for the instance
   - Sends zone configuration via `POST /api/v1/zones`
   - Bindcar updates BIND9 configuration and reloads
   - Updates instance status in `status.bind9Instances[]`
4. **Computes status** - Sets `bind9InstancesCount` from array length
5. **Sets conditions** - Updates Ready/Progressing/Degraded conditions

## Verifying Zone Creation

Check the zone status:

```bash
kubectl get dnszones -n dns-system
```

Expected output:

```
NAME          ZONE           RECORDS  INSTANCES  TTL   READY  AGE
example-com   example.com    0        1          3600  True   30s
```

The **Instances** column shows how many `Bind9Instance` resources are serving the zone.

View detailed status:

```bash
kubectl describe dnszone example-com -n dns-system
```

Expected status output:

```yaml
Status:
  Bind9 Instances:
    Name:       primary-dns
    Namespace:  dns-system
    Status:     Configured
    Message:    Zone synchronized successfully
  Bind9 Instances Count:  1
  Conditions:
    Type:    Ready
    Status:  True
    Reason:  InstancesSynchronized
    Message: Zone configured on 1 instance
  Record Count:  0
```

Key status fields:
- **bind9InstancesCount**: Number of instances serving the zone
- **bind9Instances[]**: List of instances with their sync status
- **recordCount**: Number of DNS records in the zone
- **conditions**: Ready/Progressing/Degraded status

## Next Steps

- [Add DNS Records](./records-guide.md) to your zone
- [Configure Zone Transfers](../advanced/zone-transfers.md) for secondaries
- [Learn about Communication Protocols](../concepts/architecture-protocols.md) - RNDC and HTTP API
