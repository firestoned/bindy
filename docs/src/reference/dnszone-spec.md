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
  nameServers:              # Optional (v0.4.0+)
    - hostname: string
      ipv4Address: string   # Optional
      ipv6Address: string   # Optional
  nameServerIps:            # DEPRECATED: use nameServers instead
    hostname: ipAddress
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

### nameServers
**Type**: array of objects
**Required**: No
**Since**: v0.4.0

List of authoritative nameservers for this zone. NS records are automatically generated for all entries, eliminating the need to create manual NSRecord CRs for zone-level nameservers.

```yaml
spec:
  nameServers:
    - hostname: ns2.example.com.
      ipv4Address: "192.0.2.2"
    - hostname: ns3.example.com.
      ipv4Address: "192.0.2.3"
      ipv6Address: "2001:db8::3"
    - hostname: ns4.external-provider.net.
```

**Automatic Record Generation**:
- **NS Records**: Auto-generated for ALL nameservers (including primary from `soaRecord.primaryNs`)
- **Glue Records**: A/AAAA records auto-generated for in-zone nameservers with IP addresses
- **No Manual CRs**: No need to create separate NSRecord resources for zone-level nameservers

**How It Works**:
1. Operator reads `soaRecord.primaryNs` (e.g., ns1.example.com)
2. Operator reads `nameServers` list (e.g., ns2, ns3, ns4)
3. During zone reconciliation, NS records are added via dynamic DNS update (RFC 2136)
4. Glue records (A/AAAA) are created for nameservers within the zone's domain

**Result**:
```
@ IN NS ns1.example.com.  ; From soaRecord.primaryNs
@ IN NS ns2.example.com.  ; From nameServers[0]
@ IN NS ns3.example.com.  ; From nameServers[1]
@ IN NS ns4.external-provider.net.  ; From nameServers[2]

; Glue records (only for in-zone nameservers with IPs)
ns2.example.com. IN A 192.0.2.2
ns3.example.com. IN A 192.0.2.3
ns3.example.com. IN AAAA 2001:db8::3
```

#### nameServers[].hostname
**Type**: string
**Required**: Yes

Fully qualified domain name of the nameserver.

```yaml
nameServers:
  - hostname: ns2.example.com.
```

**Requirements**:
- Must be a fully qualified domain name (FQDN)
- Must end with a dot (.)
- Can be in-zone (e.g., ns2.example.com for zone example.com) or out-of-zone (e.g., ns.external.net)

#### nameServers[].ipv4Address
**Type**: string
**Required**: No

IPv4 address for glue record generation.

```yaml
nameServers:
  - hostname: ns2.example.com.
    ipv4Address: "192.0.2.2"
```

**When Required**:
- Required when the nameserver is within the zone's own domain (in-zone nameserver)
- Optional for out-of-zone nameservers (no glue records needed)

**Format**:
- Must be a valid IPv4 address in dotted-decimal notation
- Example: "192.0.2.2", "10.0.0.1"

**Glue Record Generation**:
When provided for an in-zone nameserver, automatically generates:
```
ns2.example.com. IN A 192.0.2.2
```

#### nameServers[].ipv6Address
**Type**: string
**Required**: No

IPv6 address for glue record generation (AAAA record).

```yaml
nameServers:
  - hostname: ns2.example.com.
    ipv4Address: "192.0.2.2"
    ipv6Address: "2001:db8::2"
```

**Format**:
- Must be a valid IPv6 address
- Example: "2001:db8::2", "fd00::1"

**Glue Record Generation**:
When provided for an in-zone nameserver, automatically generates:
```
ns2.example.com. IN AAAA 2001:db8::2
```

**Dual-Stack Nameservers**:
You can provide both IPv4 and IPv6 addresses for dual-stack support:
```yaml
nameServers:
  - hostname: ns2.example.com.
    ipv4Address: "192.0.2.2"
    ipv6Address: "2001:db8::2"
```

This generates both A and AAAA glue records.

### nameServerIps (Deprecated)
**Type**: map of string to string
**Required**: No
**Deprecated**: Since v0.4.0, use `nameServers` instead

> **⚠️ DEPRECATED**: This field is deprecated and will be removed in v1.0.0. Use `nameServers` instead.

Old format for specifying nameserver IP addresses for glue records. The field name was misleading as it suggests only glue records, when it actually defines authoritative nameservers.

```yaml
# OLD FORMAT (deprecated)
spec:
  nameServerIps:
    ns2.example.com: "192.0.2.2"
    ns3.example.com: "192.0.2.3"
```

**Migration**: See [Migration Guide](../operations/migration-guide.md#migrating-from-nameserverips-to-nameservers) for how to migrate to `nameServers`.

**Backward Compatibility**: Existing zones using `nameServerIps` will continue to work with a deprecation warning in operator logs.

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

### Zone with Multiple Nameservers

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-multi-ns
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-dns
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2025012101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  # Multiple nameservers with automatic NS record generation
  nameServers:
    - hostname: ns2.example.com.
      ipv4Address: "192.0.2.2"
    - hostname: ns3.example.com.
      ipv4Address: "192.0.2.3"
      ipv6Address: "2001:db8::3"  # Dual-stack nameserver
    - hostname: ns4.external-provider.net.  # Out-of-zone NS
  ttl: 3600
```

**Result**: Automatically generates 4 NS records (ns1 from SOA + 3 from nameServers) and glue records for in-zone nameservers.

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
