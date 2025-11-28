# DNSZone

The `DNSZone` resource defines a DNS zone with its SOA record and references a specific BIND9 cluster.

## Overview

A DNSZone represents:
- Zone name (e.g., example.com)
- SOA (Start of Authority) record
- Cluster reference to a Bind9Instance
- Default TTL for records

The zone is created on the referenced BIND9 cluster using the RNDC protocol.

## Example

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: my-dns-cluster  # References Bind9Instance name
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin.example.com.  # Note: @ replaced with .
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Synchronized
      message: "Zone created for cluster: my-dns-cluster"
  observedGeneration: 1
```

## Specification

### Required Fields

- `spec.zoneName` - The DNS zone name (e.g., example.com)
- `spec.clusterRef` - Name of the Bind9Instance to host this zone
- `spec.soaRecord` - Start of Authority record configuration

### SOA Record Fields

- `primaryNS` - Primary nameserver (must end with `.`)
- `adminEmail` - Zone administrator email (@ replaced with `.`, must end with `.`)
- `serial` - Zone serial number (typically YYYYMMDDNN format)
- `refresh` - Refresh interval in seconds (how often secondaries check for updates)
- `retry` - Retry interval in seconds (retry delay after failed refresh)
- `expire` - Expiry time in seconds (when to stop serving if primary unreachable)
- `negativeTTL` - Negative caching TTL (cache duration for NXDOMAIN responses)

### Optional Fields

- `spec.ttl` - Default TTL for records in seconds (default: 3600)

## How Zones Are Created

When you create a DNSZone resource:

1. **Controller discovers pods** - Finds BIND9 pods with label `instance={clusterRef}`
2. **Loads RNDC key** - Retrieves Secret named `{clusterRef}-rndc-key`
3. **Connects via RNDC** - Establishes connection to `{clusterRef}.{namespace}.svc.cluster.local:953`
4. **Executes addzone** - Runs `rndc addzone` command with zone configuration
5. **BIND9 creates zone** - BIND9 creates the zone file and starts serving the zone
6. **Updates status** - Controller updates DNSZone status to Ready

## Cluster References

Zones reference a specific BIND9 cluster by name:

```yaml
spec:
  clusterRef: my-dns-cluster
```

This references a Bind9Instance resource:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: my-dns-cluster  # Referenced by DNSZone
  namespace: dns-system
spec:
  role: primary
  replicas: 2
```

### RNDC Key Discovery

The controller automatically finds the RNDC key using the cluster reference:

```
DNSZone.spec.clusterRef = "my-dns-cluster"
    ↓
Secret name = "my-dns-cluster-rndc-key"
    ↓
RNDC authentication to: my-dns-cluster.dns-system.svc.cluster.local:953
```

## Status

The controller reports zone status:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Synchronized
      message: "Zone created for cluster: my-dns-cluster"
  observedGeneration: 1
  recordCount: 5
```

Possible status conditions:

- **Ready/True** - Zone created and serving on BIND9 cluster
- **Ready/False** - Zone creation failed or RNDC error
- **Ready/Unknown** - Controller hasn't reconciled yet

## Use Cases

### Simple Zone

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: simple-com
spec:
  zoneName: simple.com
  clusterRef: primary-dns
  soaRecord:
    primaryNS: ns1.simple.com.
    adminEmail: admin.simple.com.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
```

### Production Zone with Custom TTL

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: api-example-com
spec:
  zoneName: api.example.com
  clusterRef: production-dns
  ttl: 300  # 5 minute default TTL for faster updates
  soaRecord:
    primaryNS: ns1.api.example.com.
    adminEmail: ops.example.com.
    serial: 2024010101
    refresh: 1800   # Check every 30 minutes
    retry: 300      # Retry after 5 minutes
    expire: 604800
    negativeTTL: 300  # Short negative cache
```

## Next Steps

- [DNS Records](./records.md) - Add records to zones
- [RNDC-Based Architecture](./architecture-rndc.md) - Learn how RNDC protocol works
- [Bind9Instance](./bind9instance.md) - Learn about BIND9 instance resources
- [Creating Zones](../guide/creating-zones.md) - Zone management guide
