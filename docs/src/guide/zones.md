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

Zones are deployed to Bind9Instances using label selectors:

```yaml
spec:
  instanceSelector:
    matchLabels:
      dns-role: primary
      environment: production
```

This deploys the zone to all instances matching both labels.

## SOA Record

Every primary zone requires an SOA (Start of Authority) record:

```yaml
spec:
  soaRecord:
    primaryNS: ns1.example.com.      # Primary nameserver
    adminEmail: admin@example.com    # Admin email (@ becomes .)
    serial: 2024010101               # Zone serial number
    refresh: 3600                    # Refresh interval
    retry: 600                       # Retry interval
    expire: 604800                   # Expiration time
    negativeTTL: 86400              # Negative caching TTL
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
kubectl describe dnszone example-com -n dns-system
```

Status conditions indicate:
- Whether the zone is ready
- Which instances are hosting the zone
- Any errors or warnings

## Common Operations

### Listing Zones

```bash
# List all zones
kubectl get dnszones -n dns-system

# Show zones with custom columns
kubectl get dnszones -n dns-system -o custom-columns=NAME:.metadata.name,ZONE:.spec.zoneName,TYPE:.spec.type
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
