# DNSZone

The `DNSZone` resource defines a DNS zone with its SOA record and targets BIND9 instances using label selectors.

## Overview

A DNSZone represents:
- Zone name (e.g., example.com)
- SOA (Start of Authority) record
- Instance selector for targeting BIND9 instances
- Default TTL for records

## Example

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  type: primary
  instanceSelector:
    matchLabels:
      dns-role: primary
      environment: production
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin@example.com
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
      message: "Zone created for 2 instances"
  observedGeneration: 1
  matchedInstances: 2
```

## Specification

### Required Fields

- `spec.zoneName` - The DNS zone name (e.g., example.com)
- `spec.instanceSelector` - Label selector for targeting BIND9 instances
- `spec.soaRecord` - Start of Authority record configuration

### SOA Record Fields

- `primaryNS` - Primary nameserver (must end with .)
- `adminEmail` - Zone administrator email
- `serial` - Zone serial number
- `refresh` - Refresh interval in seconds
- `retry` - Retry interval in seconds
- `expire` - Expiry time in seconds
- `negativeTTL` - Negative caching TTL

### Optional Fields

- `spec.type` - Zone type (primary or secondary, default: primary)
- `spec.ttl` - Default TTL for records (default: 3600)

## Instance Selectors

Target specific BIND9 instances using label selectors:

### Match Labels

```yaml
instanceSelector:
  matchLabels:
    dns-role: primary
    datacenter: us-east
```

### Match Expressions

```yaml
instanceSelector:
  matchExpressions:
    - key: dns-role
      operator: In
      values:
        - primary
        - secondary
    - key: environment
      operator: NotIn
      values:
        - development
```

## Status

The controller reports zone status:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Synchronized
      message: "Zone created for 2 instances"
  matchedInstances: 2
  recordCount: 5
```

## Use Cases

### Simple Zone

```yaml
spec:
  zoneName: simple.com
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNS: ns1.simple.com.
    adminEmail: admin@simple.com
```

### Multi-Region Zone

```yaml
spec:
  zoneName: global.com
  instanceSelector:
    matchExpressions:
      - key: dns-role
        operator: In
        values:
          - primary
          - secondary
  soaRecord:
    primaryNS: ns1.global.com.
    adminEmail: admin@global.com
```

## Next Steps

- [DNS Records](./records.md) - Add records to zones
- [Label Selectors](../guide/label-selectors.md) - Advanced targeting
- [Creating Zones](../guide/creating-zones.md) - Zone management guide
