# Creating Zones

Learn how to create DNS zones in Bindy using the RNDC protocol.

## Zone Architecture

Zones in Bindy follow a three-tier model:

1. **Bind9Cluster** - Cluster-level configuration (version, shared config, TSIG keys)
2. **Bind9Instance** - Individual BIND9 server deployment (references a cluster)
3. **DNSZone** - DNS zone (references an instance via `clusterRef`)

## Prerequisites

Before creating a zone, ensure you have:

1. A Bind9Cluster resource deployed
2. A Bind9Instance resource deployed (referencing the cluster)
3. The instance is ready and running

## Creating a Primary Zone

First, ensure you have a cluster and instance:

```yaml
# Step 1: Create a Bind9Cluster (if not already created)
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"

---
# Step 2: Create a Bind9Instance (if not already created)
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  clusterRef: production-dns  # References the Bind9Cluster above
  replicas: 1

---
# Step 3: Create the DNSZone
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: primary-dns  # References the Bind9Instance above
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin.example.com.  # Note: @ replaced with .
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
```

## How It Works

When you create a DNSZone:

1. **Controller discovers pods** - Finds BIND9 pods with label `instance=primary-dns`
2. **Loads RNDC key** - Retrieves Secret named `primary-dns-rndc-key`
3. **Connects via RNDC** - Establishes connection to `primary-dns.dns-system.svc.cluster.local:953`
4. **Executes addzone** - Runs `rndc addzone example.com` command
5. **BIND9 creates zone** - BIND9 creates the zone and starts serving it
6. **Updates status** - Controller updates DNSZone status to Ready

## Verifying Zone Creation

Check the zone status:

```bash
kubectl get dnszones -n dns-system
kubectl describe dnszone example-com -n dns-system
```

Expected output:

```
Name:         example-com
Namespace:    dns-system
Labels:       <none>
Annotations:  <none>
API Version:  bindy.firestoned.io/v1alpha1
Kind:         DNSZone
Spec:
  Cluster Ref:  primary-dns
  Zone Name:    example.com
Status:
  Conditions:
    Type:    Ready
    Status:  True
    Reason:  Synchronized
    Message: Zone created for cluster: primary-dns
```

## Next Steps

- [Add DNS Records](./records-guide.md) to your zone
- [Configure Zone Transfers](../advanced/zone-transfers.md) for secondaries
- [Learn about the RNDC Protocol](../concepts/architecture-rndc.md)
