# DNSZone Specification

Complete specification for the DNSZone Custom Resource Definition.

## Resource Definition

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: string
  namespace: string
spec:
  zoneName: string
  zoneType: string
  instanceSelector:
    matchLabels:
      key: value
    matchExpressions:
      - key: string
        operator: string
        values: [string]
  soaRecord:
    primaryNs: string
    adminEmail: string
    serial: integer
    refresh: integer
    retry: integer
    expire: integer
    negativeTtl: integer
  secondaryConfig:
    primaryServers: [string]
    tsigKey: string
  ttl: integer
```

## Spec Fields

### zoneName
**Type**: string
**Required**: Yes

The DNS zone name (domain name).

```yaml
spec:
  zoneName: "example.com"
```

**Requirements**:
- Must be a valid DNS domain name
- Should end with a dot for absolute names
- Maximum 253 characters

**Examples**:
- "example.com"
- "subdomain.example.com"
- "10.in-addr.arpa" (reverse zone)

### zoneType
**Type**: string
**Required**: No
**Default**: "primary"

Type of DNS zone.

```yaml
spec:
  zoneType: "primary"
```

**Valid Values**:
- "primary" - Master zone (authoritative data)
- "secondary" - Slave zone (transfers from primary)

### instanceSelector
**Type**: object
**Required**: Yes

Label selector to target Bind9Instance resources.

#### instanceSelector.matchLabels
**Type**: map of key-value pairs
**Required**: No

Exact label matches.

```yaml
spec:
  instanceSelector:
    matchLabels:
      dns-role: primary
      environment: production
```

#### instanceSelector.matchExpressions
**Type**: array of objects
**Required**: No

Advanced label selection expressions.

```yaml
spec:
  instanceSelector:
    matchExpressions:
      - key: dns-role
        operator: In
        values: [primary, secondary]
      - key: environment
        operator: NotIn
        values: [development]
```

**Operators**:
- In - Label value must be in list
- NotIn - Label value must not be in list
- Exists - Label key must exist
- DoesNotExist - Label key must not exist

### soaRecord
**Type**: object
**Required**: Yes for primary zones, No for secondary zones

Start of Authority record defining zone parameters.

```yaml
spec:
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "admin@example.com"
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
```

#### soaRecord.primaryNs
**Type**: string
**Required**: Yes

Primary nameserver for the zone.

```yaml
soaRecord:
  primaryNs: "ns1.example.com."
```

**Requirements**:
- Must be a fully qualified domain name (FQDN)
- Should end with a dot

#### soaRecord.adminEmail
**Type**: string
**Required**: Yes

Email address of zone administrator.

```yaml
soaRecord:
  adminEmail: "admin@example.com"
```

**Format**: Standard email address (@ is converted to . in zone file)

#### soaRecord.serial
**Type**: integer
**Required**: Yes

Zone serial number for change tracking.

```yaml
soaRecord:
  serial: 2024010101
```

**Best Practices**:
- Use format: YYYYMMDDnn (year, month, day, revision)
- Increment on every change
- Must be 32-bit unsigned integer

#### soaRecord.refresh
**Type**: integer
**Required**: Yes

How often (in seconds) secondary servers should check for updates.

```yaml
soaRecord:
  refresh: 3600  # 1 hour
```

**Typical Values**:
- 3600 (1 hour) - Standard
- 7200 (2 hours) - Less frequent updates
- 900 (15 minutes) - Frequent updates

#### soaRecord.retry
**Type**: integer
**Required**: Yes

How long (in seconds) to wait before retrying a failed refresh.

```yaml
soaRecord:
  retry: 600  # 10 minutes
```

**Best Practice**: Should be less than refresh value

#### soaRecord.expire
**Type**: integer
**Required**: Yes

How long (in seconds) secondary servers should keep serving zone data after primary becomes unreachable.

```yaml
soaRecord:
  expire: 604800  # 1 week
```

**Typical Values**:
- 604800 (1 week) - Standard
- 1209600 (2 weeks) - Extended
- 86400 (1 day) - Short-lived zones

#### soaRecord.negativeTtl
**Type**: integer
**Required**: Yes

How long (in seconds) to cache negative responses (NXDOMAIN).

```yaml
soaRecord:
  negativeTtl: 86400  # 24 hours
```

**Typical Values**:
- 86400 (24 hours) - Standard
- 3600 (1 hour) - Shorter caching
- 300 (5 minutes) - Very short for dynamic zones

### secondaryConfig
**Type**: object
**Required**: Yes for secondary zones, No for primary zones

Configuration for secondary (slave) zones.

```yaml
spec:
  zoneType: "secondary"
  secondaryConfig:
    primaryServers:
      - "10.0.1.10"
      - "10.0.1.11"
    tsigKey: "transfer-key"
```

#### secondaryConfig.primaryServers
**Type**: array of strings
**Required**: Yes

IP addresses of primary servers to transfer from.

```yaml
secondaryConfig:
  primaryServers:
    - "10.0.1.10"
    - "10.0.1.11"
```

#### secondaryConfig.tsigKey
**Type**: string
**Required**: No

TSIG key name for authenticated zone transfers.

```yaml
secondaryConfig:
  tsigKey: "transfer-key"
```

### ttl
**Type**: integer
**Required**: No
**Default**: 3600

Default Time To Live for records in this zone (in seconds).

```yaml
spec:
  ttl: 3600  # 1 hour
```

## Status Fields

### conditions
**Type**: array of objects

Standard Kubernetes conditions.

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSuccess
      message: "Zone configured successfully"
      lastTransitionTime: "2024-01-15T10:30:00Z"
```

### observedGeneration
**Type**: integer

The generation last reconciled.

### recordCount
**Type**: integer

Number of DNS records in this zone.

```yaml
status:
  recordCount: 42
```

## Complete Examples

### Primary Zone

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: "example.com"
  zoneType: "primary"
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "admin@example.com"
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
```

### Secondary Zone

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com-secondary
  namespace: dns-system
spec:
  zoneName: "example.com"
  zoneType: "secondary"
  instanceSelector:
    matchLabels:
      dns-role: secondary
  secondaryConfig:
    primaryServers:
      - "10.0.1.10"
      - "10.0.1.11"
    tsigKey: "transfer-key"
  ttl: 3600
```

### Zone with Match Expressions

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: multi-instance-zone
  namespace: dns-system
spec:
  zoneName: "example.com"
  zoneType: "primary"
  instanceSelector:
    matchExpressions:
      - key: dns-role
        operator: In
        values: [primary, multi-primary]
      - key: region
        operator: Exists
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "admin@example.com"
    serial: 2024010101
    refresh: 900
    retry: 300
    expire: 604800
    negativeTtl: 300
  ttl: 300
```

### Reverse DNS Zone

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: reverse-zone
  namespace: dns-system
spec:
  zoneName: "1.0.10.in-addr.arpa"
  zoneType: "primary"
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "admin@example.com"
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
```

## Related Resources

- [Bind9Instance Specification](./bind9instance-spec.md)
- [Record Specifications](./record-specs.md)
- [Zone Management Guide](../guide/zones.md)
