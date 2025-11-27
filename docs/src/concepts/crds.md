# Custom Resource Definitions

Bindy extends Kubernetes with these Custom Resource Definitions (CRDs).

## Infrastructure CRDs

### Bind9Instance

Represents a BIND9 DNS server instance.

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  labels:
    dns-role: primary
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

[Learn more about Bind9Instance](./bind9instance.md)

## DNS CRDs

### DNSZone

Defines a DNS zone with SOA record and instance targeting.

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin@example.com
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

All Bindy CRDs belong to the `dns.firestoned.io` API group:

- **Current version**: `v1alpha1`
- **API stability**: Alpha (subject to breaking changes)

## Next Steps

- [Bind9Instance Details](./bind9instance.md)
- [DNSZone Details](./dnszone.md)
- [DNS Record Details](./records.md)
- [API Reference](../reference/api.md)
