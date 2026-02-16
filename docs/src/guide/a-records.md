# A Records (IPv4)

A records map domain names to IPv4 addresses.

## Creating an A Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    zone: example.com  # Used by DNSZone selector
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.1"
  ttl: 300
```

This creates `www.example.com -> 192.0.2.1`.

## How Records Are Associated with Zones

Records are discovered by DNSZones using label selectors. The DNSZone must have a `recordsFrom` selector that matches the record's labels:

```yaml
# DNSZone with selector
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com
  clusterRef: production-dns
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com  # Selects all records with this label
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
---
# Record that will be selected
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
  labels:
    zone: example.com  # ✅ Matches selector above
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.1"
```

See [Label Selector Guide](./label-selectors.md) for advanced patterns.

## Root Record

For the zone apex (example.com):

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: root-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  ipv4Addresses:
    - "192.0.2.1"
```

## Round-Robin DNS (Load Balancing)

**New in v0.3.4+**: A single ARecord can now manage multiple IP addresses for round-robin DNS load balancing.

### Single Resource with Multiple IPs

Instead of creating multiple ARecord resources with the same name, use a single resource with multiple IPs:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-load-balanced
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.1"
    - "192.0.2.2"
    - "192.0.2.3"
  ttl: 60  # Lower TTL for faster failover
```

This creates a single DNS RRset with three A records:
```
www.example.com.  60  IN  A  192.0.2.1
www.example.com.  60  IN  A  192.0.2.2
www.example.com.  60  IN  A  192.0.2.3
```

DNS resolvers will typically rotate through the addresses, providing basic load balancing.

### How Round-Robin Works

1. **DNS Query**: Client queries `www.example.com`
2. **Response**: DNS server returns all IP addresses in the RRset
3. **Client Selection**: Most clients rotate through IPs or select randomly
4. **Load Distribution**: Traffic spreads across backend servers

### Advantages

- **Simpler Management**: One Kubernetes resource per DNS name
- **Atomic Updates**: All IPs updated together in a single DNS transaction
- **Declarative**: Specify desired IPs; Bindy ensures DNS matches
- **Order Independent**: IP order in YAML doesn't matter

### When to Use

- **Stateless applications** where any backend can serve any request
- **Basic load balancing** without health checks
- **Small to medium scale** (typically 2-10 backends)

### When NOT to Use

- **Stateful sessions** (use sticky sessions / session affinity instead)
- **Health-aware routing** (use a proper load balancer like Linkerd)
- **Large scale** (>10 backends - DNS response size limits apply)
- **Precise traffic control** (use traffic splitting in service mesh)

For production workloads, consider using [Linkerd](https://linkerd.io) for sophisticated load balancing with health checks, retries, and circuit breaking.

## Migration from Old Schema

If you have existing ARecords using the old `ipv4Address` field (singular), update them to use `ipv4Addresses` (plural array):

**Before** (deprecated):
```yaml
spec:
  name: www
  ipv4Address: "192.0.2.1"  # ❌ Old field (no longer supported)
```

**After** (current):
```yaml
spec:
  name: www
  ipv4Addresses:  # ✅ New array field
    - "192.0.2.1"
```

## Status and Conditions

Check record status with:

```bash
kubectl get arecords -n dns-system
```

Output shows:
- **NAME**: Resource name
- **Name**: DNS name within zone
- **Addresses**: IP addresses (shows first IP if multiple)
- **TTL**: Time to live
- **Ready**: Whether record is successfully configured in DNS

For detailed status:

```bash
kubectl describe arecord www-example -n dns-system
```

## Common Patterns

### Load Balancing Web Servers

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-prod
  namespace: dns-system
  labels:
    zone: example.com
    environment: production
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.10"  # web-server-1
    - "192.0.2.11"  # web-server-2
    - "192.0.2.12"  # web-server-3
  ttl: 60
```

### Development Environment

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: dev-api
  namespace: dns-system
  labels:
    zone: dev.example.com
    environment: development
spec:
  name: api
  ipv4Addresses:
    - "192.168.1.100"
  ttl: 60  # Short TTL for rapid iteration
```

### Zone Apex with Multiple IPs

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: root-load-balanced
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  ipv4Addresses:
    - "192.0.2.1"
    - "192.0.2.2"
  ttl: 300
```

## Troubleshooting

### Record Not Appearing in DNS

1. **Check DNSZone selector**: Ensure zone's `recordsFrom` selector matches record labels
2. **Check instance status**: Verify Bind9Instance is Ready
3. **Verify labels**: `kubectl get arecord <name> -o yaml | grep -A 5 labels`
4. **Check zone selection**: `kubectl get dnszone <zone> -o yaml | grep -A 10 recordsFrom`

### Multiple IP Addresses Not Working

1. **Verify CRD version**: Ensure CRDs are updated (v0.3.4+)
2. **Check array syntax**: Use YAML array format with `-` prefix
3. **Validate schema**: `kubectl apply --dry-run=client -f record.yaml`
4. **Query DNS directly**: `dig @<bind9-pod-ip> www.example.com`

### DNS Updates Not Propagating

1. **Check record status**: `kubectl describe arecord <name>`
2. **Verify TSIG key**: Ensure Bind9Instance has valid rndc key
3. **Check pod logs**: `kubectl logs -n dns-system <bind9-pod>`
4. **Test DNS update**: Use `nsupdate` manually to verify BIND9 accepts updates

## Related Documentation

- [AAAA Records (IPv6)](./aaaa-records.md)
- [CNAME Records](./cname-records.md)
- [Label Selectors](./label-selectors.md)
- [DNSZone Configuration](../concepts/dnszone.md)
- [Bind9 Integration](../concepts/architecture.md)
