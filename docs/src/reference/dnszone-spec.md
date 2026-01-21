# DNSZone Specification

Complete specification for the DNSZone Custom Resource Definition.

## Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: string
  namespace: string
spec:
  zoneName: string
  clusterRef: string        # References Bind9Cluster
  soaRecord:
    primaryNs: string
    adminEmail: string
    serial: integer
    refresh: integer
    retry: integer
    expire: integer
    negativeTtl: integer
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
- Maximum 253 characters
- Can be forward or reverse zone

**Examples**:
- "example.com"
- "subdomain.example.com"
- "1.0.10.in-addr.arpa" (reverse zone)

### clusterRef
**Type**: string
**Required**: Yes

Name of the Bind9Cluster that will manage this zone.

```yaml
spec:
  clusterRef: production-dns  # References Bind9Cluster named "production-dns"
```

**How It Works**:
- Operator finds Bind9Cluster with this name
- Discovers all Bind9Instance resources referencing this cluster
- Identifies primary instances for zone hosting
- Loads RNDC keys from cluster configuration
- Creates zone on primary instances using `rndc addzone` command
- Configures zone transfers to secondary instances

**Validation**:
- Referenced Bind9Cluster must exist in same namespace
- Operator validates reference at admission time

### soaRecord
**Type**: object
**Required**: Yes

Start of Authority record defining zone parameters.

```yaml
spec:
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "admin.example.com."  # Note: @ replaced with .
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
- Must end with a dot (.)
- Pattern: `^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*\.$`

#### soaRecord.adminEmail
**Type**: string
**Required**: Yes

Email address of zone administrator in DNS format.

```yaml
soaRecord:
  adminEmail: "admin.example.com."  # Represents admin@example.com
```

**Format**:
- Replace @ with . in email address
- Must end with a dot (.)
- Example: admin@example.com → admin.example.com.
- Pattern: `^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*\.$`

#### soaRecord.serial
**Type**: integer (64-bit)
**Required**: Yes
**Range**: 0 to 4,294,967,295

Zone serial number for change tracking.

```yaml
soaRecord:
  serial: 2024010101
```

**Best Practices**:
- Use format: YYYYMMDDnn (year, month, day, revision)
- Increment on every change
- Secondaries use this to detect updates

**Examples**:
- 2024010101 - January 1, 2024, first revision
- 2024010102 - January 1, 2024, second revision

#### soaRecord.refresh
**Type**: integer (32-bit)
**Required**: Yes
**Range**: 1 to 2,147,483,647

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
**Type**: integer (32-bit)
**Required**: Yes
**Range**: 1 to 2,147,483,647

How long (in seconds) to wait before retrying a failed refresh.

```yaml
soaRecord:
  retry: 600  # 10 minutes
```

**Best Practice**: Should be less than refresh value

#### soaRecord.expire
**Type**: integer (32-bit)
**Required**: Yes
**Range**: 1 to 2,147,483,647

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
**Type**: integer (32-bit)
**Required**: Yes
**Range**: 0 to 2,147,483,647

How long (in seconds) to cache negative responses (NXDOMAIN).

```yaml
soaRecord:
  negativeTtl: 86400  # 24 hours
```

**Typical Values**:
- 86400 (24 hours) - Standard
- 3600 (1 hour) - Shorter caching
- 300 (5 minutes) - Very short for dynamic zones

### ttl
**Type**: integer (32-bit)
**Required**: No
**Default**: 3600
**Range**: 0 to 2,147,483,647

Default Time To Live for records in this zone (in seconds).

```yaml
spec:
  ttl: 3600  # 1 hour
```

**Common Values**:
- 3600 (1 hour) - Standard
- 300 (5 minutes) - Frequently changing zones
- 86400 (24 hours) - Stable zones

## Status Fields

### conditions
**Type**: array of objects

Standard Kubernetes conditions.

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Synchronized
      message: "Zone created for cluster: primary-dns"
      lastTransitionTime: "2024-01-15T10:30:00Z"
```

**Condition Types**:
- **Ready** - Zone is created and serving
- **Synced** - Zone is synchronized with BIND9
- **Failed** - Zone creation or update failed

### observedGeneration
**Type**: integer

The generation last reconciled.

```yaml
status:
  observedGeneration: 3
```

### recordCount
**Type**: integer

Number of DNS records in this zone.

```yaml
status:
  recordCount: 42
```

## Complete Examples

### Simple Primary Zone

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: primary-dns
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
```

### Production Zone with Custom TTL

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: api-example-com
  namespace: dns-system
spec:
  zoneName: api.example.com
  clusterRef: production-dns
  ttl: 300  # 5 minute default TTL for faster updates
  soaRecord:
    primaryNs: ns1.api.example.com.
    adminEmail: ops.example.com.
    serial: 2024010101
    refresh: 1800   # Check every 30 minutes
    retry: 300      # Retry after 5 minutes
    expire: 604800
    negativeTtl: 300  # Short negative cache
```

### Reverse DNS Zone

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: reverse-zone
  namespace: dns-system
spec:
  zoneName: 1.0.10.in-addr.arpa
  clusterRef: primary-dns
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
```

### Multi-Region Setup

```yaml
# East Region Zone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-east
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: dns-east  # References east instance
  soaRecord:
    primaryNs: ns1.east.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400

---
# West Region Zone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-west
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: dns-west  # References west instance
  soaRecord:
    primaryNs: ns1.west.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
```

## Zone Creation Flow

When you create a DNSZone resource:

1. **Admission** - Kubernetes validates the resource schema
2. **Operator watches** - Bindy operator detects the new zone
3. **Cluster lookup** - Finds Bind9Cluster referenced by `clusterRef`
4. **Instance discovery** - Finds all Bind9Instance resources referencing the cluster
5. **Primary identification** - Identifies primary instances (with `role: primary`)
6. **RNDC key load** - Retrieves RNDC keys from cluster configuration
7. **RNDC connection** - Connects to primary instance pods via RNDC
8. **Zone creation** - Executes `rndc addzone {zoneName} ...` on primary instances
9. **Zone transfer setup** - Configures zone transfers to secondary instances
10. **Status update** - Updates DNSZone status to Ready

## Related Resources

- [Bind9Cluster Specification](./bind9cluster-spec.md)
- [Bind9Instance Specification](./bind9instance-spec.md)
- [Record Specifications](./record-specs.md)
- [Creating Zones Guide](../guide/creating-zones.md)
- [RNDC-Based Architecture](../concepts/architecture-rndc.md)
