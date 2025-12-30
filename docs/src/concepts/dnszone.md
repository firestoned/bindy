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
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: my-dns-cluster  # References Bind9Instance name
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.  # Note: @ replaced with .
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
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

- `primaryNs` - Primary nameserver (must end with `.`)
- `adminEmail` - Zone administrator email (@ replaced with `.`, must end with `.`)
- `serial` - Zone serial number (typically YYYYMMDDNN format)
- `refresh` - Refresh interval in seconds (how often secondaries check for updates)
- `retry` - Retry interval in seconds (retry delay after failed refresh)
- `expire` - Expiry time in seconds (when to stop serving if primary unreachable)
- `negativeTtl` - Negative caching TTL (cache duration for NXDOMAIN responses)

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
apiVersion: bindy.firestoned.io/v1beta1
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

The controller reports zone status with granular condition types that provide real-time visibility into the reconciliation process.

### Status During Reconciliation

```yaml
# Phase 1: Configuring primary instances
status:
  conditions:
    - type: Progressing
      status: "True"
      reason: PrimaryReconciling
      message: "Configuring zone on primary instances"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 1

# Phase 2: Primary success, configuring secondaries
status:
  conditions:
    - type: Progressing
      status: "True"
      reason: SecondaryReconciling
      message: "Configured on 2 primary server(s), now configuring secondaries"
      lastTransitionTime: "2024-11-26T10:00:01Z"
  observedGeneration: 1
  secondaryIps:
    - "10.42.0.5"
    - "10.42.0.6"
```

### Status After Successful Reconciliation

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSucceeded
      message: "Configured on 2 primary server(s) and 3 secondary server(s)"
      lastTransitionTime: "2024-11-26T10:00:02Z"
  observedGeneration: 1
  recordCount: 5
  secondaryIps:
    - "10.42.0.5"
    - "10.42.0.6"
    - "10.42.0.7"
  records:
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: ARecord
      name: web-a-record
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: AAAARecord
      name: web-aaaa-record
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: CNAMERecord
      name: www-cname-record
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: MXRecord
      name: mail-mx-record
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: TXTRecord
      name: spf-txt-record
```

**Note:** The `records` field is **only available in v1beta1** and tracks all DNS records successfully associated with this zone. This field did not exist in the now-removed v1alpha1 API.

### Status After Partial Failure (Degraded)

```yaml
status:
  conditions:
    - type: Degraded
      status: "True"
      reason: SecondaryFailed
      message: "Configured on 2 primary server(s), but secondary configuration failed: connection timeout"
      lastTransitionTime: "2024-11-26T10:00:02Z"
  observedGeneration: 1
  recordCount: 5
  secondaryIps:
    - "10.42.0.5"
    - "10.42.0.6"
```

### Condition Types

DNSZone uses the following condition types:

- **Progressing** - Zone is being configured
  - `PrimaryReconciling`: Configuring on primary instances
  - `PrimaryReconciled`: Primary configuration successful
  - `SecondaryReconciling`: Configuring on secondary instances
  - `SecondaryReconciled`: Secondary configuration successful

- **Ready** - Zone fully configured and operational
  - `ReconcileSucceeded`: All primaries and secondaries configured successfully

- **Degraded** - Partial or complete failure
  - `PrimaryFailed`: Primary configuration failed (zone not functional)
  - `SecondaryFailed`: Secondary configuration failed (primaries work, but secondaries unavailable)

### Status Field: `records` (v1beta1 only)

The `status.records` field provides a real-time inventory of all DNS records successfully associated with this zone. This field is **only available in the v1beta1 API** and did not exist in the now-removed v1alpha1 API.

#### How It Works

When a DNS record (ARecord, AAAARecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, TXTRecord, or CAARecord) is successfully reconciled and added to the zone, the record reconciler automatically adds a reference to the `DNSZone.status.records` list.

Each record reference contains:
- `apiVersion` - The API version of the record resource (always `bindy.firestoned.io/v1beta1`)
- `kind` - The record type (e.g., `ARecord`, `CNAMERecord`, `MXRecord`)
- `name` - The name of the record resource in Kubernetes

#### Example

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
status:
  conditions:
    - type: Ready
      status: "True"
  records:
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: ARecord
      name: web-server
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: AAAARecord
      name: web-server-ipv6
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: CNAMERecord
      name: www-alias
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: MXRecord
      name: mail-server
```

#### Use Cases

1. **Operational Visibility** - Quickly see which records are managed by this zone:
   ```bash
   kubectl get dnszone example-com -o jsonpath='{.status.records[*].name}'
   # Output: web-server web-server-ipv6 www-alias mail-server
   ```

2. **Debugging** - Verify a record was successfully added to the zone:
   ```bash
   kubectl get dnszone example-com -o yaml | grep -A3 "kind: ARecord"
   # Output:
   # - apiVersion: bindy.firestoned.io/v1beta1
   #   kind: ARecord
   #   name: web-server
   ```

3. **Auditing** - Count how many records of each type are associated:
   ```bash
   kubectl get dnszone example-com -o json | jq '.status.records | group_by(.kind) | map({kind: .[0].kind, count: length})'
   # Output: [{"kind":"ARecord","count":1},{"kind":"AAAARecord","count":1},{"kind":"CNAMERecord","count":1},{"kind":"MXRecord","count":1}]
   ```

4. **Automation** - Build tools that react to zone record changes:
   ```bash
   # Watch for changes to the records list
   kubectl get dnszone example-com -o jsonpath='{.status.records}' --watch
   ```

#### Important Notes

- **v1beta1 Only**: This field did not exist in the now-removed v1alpha1 API.
- **Read-Only**: The `records` field is managed automatically by the controller. Do not manually edit it.
- **Eventually Consistent**: After creating a new record, it may take a few seconds for it to appear in the zone's `status.records` list.
- **Duplicate Prevention**: The controller automatically prevents duplicate record references from being added.
- **Serialization**: When the `records` field is empty, it is omitted from the YAML/JSON output to reduce clutter.

### Benefits of Granular Status

1. **Real-time visibility** - See which reconciliation phase is running
2. **Better debugging** - Know exactly which phase failed (primary vs secondary)
3. **Graceful degradation** - Secondary failures don't break the zone (primaries still work)
4. **Accurate counts** - Status shows exact number of configured servers
5. **Record inventory** - Track all DNS records associated with each zone (v1beta1 only)

## Use Cases

### Simple Zone

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: simple-com
spec:
  zoneName: simple.com
  clusterRef: primary-dns
  soaRecord:
    primaryNs: ns1.simple.com.
    adminEmail: admin.simple.com.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
```

### Production Zone with Custom TTL

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: api-example-com
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

## Next Steps

- [DNS Records](./records.md) - Add records to zones
- [RNDC-Based Architecture](./architecture-rndc.md) - Learn how RNDC protocol works
- [Bind9Instance](./bind9instance.md) - Learn about BIND9 instance resources
- [Creating Zones](../guide/creating-zones.md) - Zone management guide
