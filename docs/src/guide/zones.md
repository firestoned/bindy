# Managing DNS Zones

DNS zones are the containers for DNS records. In Bindy, zones are defined using the `DNSZone` custom resource.

## Zone Types

### Primary Zones

Primary (master) zones contain the authoritative data:

- Zone data is created and managed on the primary
- Changes are made by creating/updating DNS record resources
- Can be transferred to secondary servers

### Secondary Zones

Secondary (slave) zones receive data from primary servers:

- Zone data is received via AXFR/IXFR transfers
- Read-only - cannot be modified directly
- Automatically updated when primary changes

## Zone Lifecycle

1. **Create Bind9Instance** resources to host zones
2. **Create DNSZone** resource with instance selector
3. **Add DNS records** (A, CNAME, MX, etc.)
4. **Monitor status** to ensure zone is active

## Instance Selection

> **Architecture**: Zones select instances (not the other way around). A `DNSZone` declares which `Bind9Instance` resources should serve it.

Zones are deployed to `Bind9Instance` resources using one of three methods:

### Method 1: Cluster Reference

Reference a cluster, and the zone will be served by all instances in that cluster:

```yaml
spec:
  clusterRef: production-dns  # Matches instances with spec.clusterRef: production-dns
```

### Method 2: Label Selectors

Use label selectors to choose instances based on labels:

```yaml
spec:
  bind9InstancesFrom:
    - selector:
        matchLabels:
          dns-role: primary
          environment: production
```

### Method 3: Combined

Use both methods - the zone will be served by the UNION of instances from both:

```yaml
spec:
  clusterRef: production-dns
  bind9InstancesFrom:
    - selector:
        matchLabels:
          region: us-west
```

For detailed guidance on instance selection, see [Zone Selection Guide](./zone-selection.md).

## SOA Record

Every primary zone requires an SOA (Start of Authority) record:

```yaml
spec:
  soaRecord:
    primaryNs: ns1.example.com.      # Primary nameserver
    adminEmail: admin@example.com    # Admin email (@ becomes .)
    serial: 2024010101               # Zone serial number
    refresh: 3600                    # Refresh interval
    retry: 600                       # Retry interval
    expire: 604800                   # Expiration time
    negativeTtl: 86400              # Negative caching TTL
```

## Zone Configuration

### TTL (Time To Live)

Set the default TTL for records in the zone:

```yaml
spec:
  ttl: 3600  # 1 hour default TTL
```

Individual records can override this with their own TTL values.

## Zone Status

Check zone status:

```bash
kubectl get dnszone -n dns-system
```

Example output:

```
NAME          ZONE           RECORDS  INSTANCES  TTL   READY  AGE
example-com   example.com    5        3          3600  True   10m
api-zone      api.example    12       2          1800  True   5m
```

The **Instances** column shows how many `Bind9Instance` resources are serving the zone.

### Status Fields

View detailed status information:

```bash
kubectl describe dnszone example-com -n dns-system
```

Key status fields:

#### bind9InstancesCount

Shows how many instances are serving the zone:

```yaml
status:
  bind9InstancesCount: 3  # Zone is on 3 instances
```

This field is automatically computed from the `bind9Instances` array length.

#### bind9Instances

Lists each instance serving the zone with its status:

```yaml
status:
  bind9Instances:
    - name: primary-west
      namespace: dns-system
      status: Configured
      message: "Zone synchronized successfully"
    - name: primary-east
      namespace: dns-system
      status: Configured
      message: "Zone synchronized successfully"
    - name: primary-central
      namespace: dns-system
      status: Configured
      message: "Zone synchronized successfully"
```

Possible instance statuses:
- **Claimed**: Instance selected, synchronization pending
- **Configured**: Zone successfully configured on instance
- **Failed**: Synchronization failed (check message for details)

#### recordCount

Shows how many DNS records are selected by the zone:

```yaml
status:
  recordCount: 5  # Zone has 5 DNS records
```

#### conditions

Standard Kubernetes status conditions:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: InstancesSynchronized
      message: "Zone configured on 3 instances"
```

Possible conditions:
- **Ready**: Zone is ready and serving DNS queries
- **Progressing**: Zone synchronization in progress
- **Degraded**: Some instances failed, but zone is partially operational

## Common Operations

### Listing Zones

```bash
# List all zones
kubectl get dnszones -n dns-system

# Show zones with custom columns
kubectl get dnszones -n dns-system -o custom-columns=\
NAME:.metadata.name,\
ZONE:.spec.zoneName,\
RECORDS:.status.recordCount,\
INSTANCES:.status.bind9InstancesCount,\
READY:.status.conditions[?(@.type=='Ready')].status
```

Example output:

```
NAME          ZONE           RECORDS  INSTANCES  READY
example-com   example.com    5        3          True
api-zone      api.example    12       2          True
dev-zone      dev.local      8        1          True
```

### Viewing Zone Details

```bash
kubectl describe dnszone example-com -n dns-system
```

### Updating Zones

Edit the zone configuration:

```bash
kubectl edit dnszone example-com -n dns-system
```

Or apply an updated YAML file:

```bash
kubectl apply -f zone.yaml
```

### Deleting Zones

```bash
kubectl delete dnszone example-com -n dns-system
```

This removes the zone from all instances but doesn't delete the instance itself.

## Next Steps

- [Create Primary Zones](./creating-zones.md)
- [Understanding Label Selectors](./label-selectors.md)
- [Zone Configuration Options](./zone-config.md)
- [Add DNS Records](./records-guide.md)
