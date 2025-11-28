# Custom Resource Definitions

Bindy extends Kubernetes with these Custom Resource Definitions (CRDs).

## Infrastructure CRDs

### Bind9Cluster

Represents cluster-level configuration shared across multiple BIND9 instances.

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
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"
    dnssec:
      enabled: true
  tsigKeys:
    - name: transfer-key
      algorithm: hmac-sha256
      secret: "base64-encoded-secret"
```

Learn more: [Bind9Cluster concept documentation](./bind9cluster.md)

### Bind9Instance

Represents a BIND9 DNS server instance that references a Bind9Cluster.

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  clusterRef: production-dns  # References Bind9Cluster
  replicas: 2
```

[Learn more about Bind9Instance](./bind9instance.md)

## DNS CRDs

### DNSZone

Defines a DNS zone with SOA record and references a Bind9Instance.

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: primary-dns  # References Bind9Instance
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

[Learn more about DNSZone](./dnszone.md)

### DNS Record Types

Bindy supports all common DNS record types:

- **ARecord** - IPv4 addresses
- **AAAARecord** - IPv6 addresses
- **CNAMERecord** - Canonical name aliases
- **MXRecord** - Mail exchange
- **TXTRecord** - Text records (SPF, DKIM, etc.)
- **NSRecord** - Nameserver delegation
- **SRVRecord** - Service discovery
- **CAARecord** - Certificate authority authorization

[Learn more about DNS Records](./records.md)

## Resource Hierarchy

The three-tier resource model:

```
Bind9Cluster (cluster config)
    ↑
    │ referenced by clusterRef
    │
Bind9Instance (instance deployment)
    ↑
    │ referenced by clusterRef
    │
DNSZone (zone definition)
    ↑
    │ referenced by zone field
    │
DNS Records (A, CNAME, MX, etc.)
```

## Common Fields

All Bindy CRDs share these common fields:

### Metadata

```yaml
metadata:
  name: resource-name
  namespace: dns-system
  labels:
    key: value
  annotations:
    key: value
```

### Status Subresource

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Synchronized
      message: Resource is synchronized
      lastTransitionTime: "2024-01-01T00:00:00Z"
  observedGeneration: 1
```

## API Group and Versions

All Bindy CRDs belong to the `bindy.firestoned.io` API group:

- **Current version**: `v1alpha1`
- **API stability**: Alpha (subject to breaking changes)

## Next Steps

- [Bind9Instance Details](./bind9instance.md)
- [DNSZone Details](./dnszone.md)
- [DNS Record Details](./records.md)
- [RNDC-Based Architecture](./architecture-rndc.md)
- [API Reference](../reference/api.md)
