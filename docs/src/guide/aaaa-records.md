# AAAA Records (IPv6)

AAAA records map domain names to IPv6 addresses. They are the IPv6 equivalent of A records.

## Creating an AAAA Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-example-ipv6
  namespace: dns-system
  labels:
    zone: example.com  # Used by DNSZone selector
spec:
  name: www
  ipv6Addresses:
    - "2001:db8::1"
  ttl: 300
```

This creates `www.example.com -> 2001:db8::1`.

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
kind: AAAARecord
metadata:
  name: www
  labels:
    zone: example.com  # ✅ Matches selector above
spec:
  name: www
  ipv6Addresses:
    - "2001:db8::1"
```

See [Label Selector Guide](./label-selectors.md) for advanced patterns.

## Root Record

For the zone apex (example.com):

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: root-example-ipv6
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  ipv6Addresses:
    - "2001:db8::1"
```

## Round-Robin DNS (Load Balancing)

**New in v0.3.4+**: A single AAAARecord can now manage multiple IPv6 addresses for round-robin DNS load balancing.

### Single Resource with Multiple IPs

Instead of creating multiple AAAARecord resources with the same name, use a single resource with multiple IPs:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-ipv6-load-balanced
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv6Addresses:
    - "2001:db8::1"
    - "2001:db8::2"
    - "2001:db8::3"
  ttl: 60  # Lower TTL for faster failover
```

This creates a single DNS RRset with three AAAA records:
```
www.example.com.  60  IN  AAAA  2001:db8::1
www.example.com.  60  IN  AAAA  2001:db8::2
www.example.com.  60  IN  AAAA  2001:db8::3
```

DNS resolvers will typically rotate through the addresses, providing basic load balancing.

### How Round-Robin Works

1. **DNS Query**: Client queries `www.example.com` (AAAA type)
2. **Response**: DNS server returns all IPv6 addresses in the RRset
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

## Dual-Stack Configuration

For dual-stack (IPv4 + IPv6) configuration, create both A and AAAA records:

```yaml
# IPv4
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-ipv4
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.1"
  ttl: 300
---
# IPv6
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-ipv6
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv6Addresses:
    - "2001:db8::1"
  ttl: 300
```

Clients will use IPv6 if available, falling back to IPv4 otherwise.

### Dual-Stack Round-Robin

You can combine round-robin for both IPv4 and IPv6:

```yaml
# IPv4 round-robin
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-ipv4-rr
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.1"
    - "192.0.2.2"
    - "192.0.2.3"
  ttl: 60
---
# IPv6 round-robin
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-ipv6-rr
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv6Addresses:
    - "2001:db8::1"
    - "2001:db8::2"
    - "2001:db8::3"
  ttl: 60
```

## IPv6 Address Formats

IPv6 addresses support various formats:

```yaml
# Full format
ipv6Addresses:
  - "2001:0db8:0000:0000:0000:0000:0000:0001"

# Compressed format (recommended)
ipv6Addresses:
  - "2001:db8::1"

# Link-local address
ipv6Addresses:
  - "fe80::1"

# Loopback
ipv6Addresses:
  - "::1"

# IPv4-mapped IPv6
ipv6Addresses:
  - "::ffff:192.0.2.1"
```

## Migration from Old Schema

If you have existing AAAARecords using the old `ipv6Address` field (singular), update them to use `ipv6Addresses` (plural array):

**Before** (deprecated):
```yaml
spec:
  name: www
  ipv6Address: "2001:db8::1"  # ❌ Old field (no longer supported)
```

**After** (current):
```yaml
spec:
  name: www
  ipv6Addresses:  # ✅ New array field
    - "2001:db8::1"
```

## Common Use Cases

### Load Balancing Web Servers

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-ipv6-prod
  namespace: dns-system
  labels:
    zone: example.com
    environment: production
spec:
  name: www
  ipv6Addresses:
    - "2001:db8:1::443"  # web-server-1
    - "2001:db8:2::443"  # web-server-2
    - "2001:db8:3::443"  # web-server-3
  ttl: 60
```

### API Endpoint

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: api-ipv6
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: api
  ipv6Addresses:
    - "2001:db8:2::443"
  ttl: 60  # Short TTL for faster updates
```

### Mail Server

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: mail-ipv6
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: mail
  ipv6Addresses:
    - "2001:db8:3::25"
  ttl: 3600
```

## Best Practices

1. **Use compressed format** - `2001:db8::1` instead of `2001:0db8:0000:0000:0000:0000:0000:0001`
2. **Dual-stack when possible** - Provide both A and AAAA records for compatibility
3. **Match TTLs** - Use the same TTL for A and AAAA records of the same name
4. **Test IPv6 connectivity** - Ensure your infrastructure supports IPv6 before advertising AAAA records
5. **Use arrays for all records** - Even single IPs should use array syntax for consistency

## Status and Conditions

Check record status with:

```bash
kubectl get aaaarecords -n dns-system
```

Output shows:
- **NAME**: Resource name
- **Name**: DNS name within zone
- **Addresses**: IPv6 addresses (shows first IP if multiple)
- **TTL**: Time to live
- **Ready**: Whether record is successfully configured in DNS

For detailed status:

```bash
kubectl describe aaaarecord www-ipv6 -n dns-system
```

## Troubleshooting

### Record Not Appearing in DNS

1. **Check DNSZone selector**: Ensure zone's `recordsFrom` selector matches record labels
2. **Check instance status**: Verify Bind9Instance is Ready
3. **Verify labels**: `kubectl get aaaarecord <name> -o yaml | grep -A 5 labels`
4. **Check zone selection**: `kubectl get dnszone <zone> -o yaml | grep -A 10 recordsFrom`

### Multiple IPv6 Addresses Not Working

1. **Verify CRD version**: Ensure CRDs are updated (v0.3.4+)
2. **Check array syntax**: Use YAML array format with `-` prefix
3. **Validate schema**: `kubectl apply --dry-run=client -f record.yaml`
4. **Query DNS directly**: `dig AAAA @<bind9-pod-ip> www.example.com`

### Invalid IPv6 address

The operator validates IPv6 addresses. Ensure your address is in valid format:
- Use compressed notation: `2001:db8::1`
- Do not mix uppercase/lowercase unnecessarily
- Ensure all segments are valid hexadecimal

## Related Documentation

- [A Records (IPv4)](./a-records.md)
- [CNAME Records](./cname-records.md)
- [Label Selectors](./label-selectors.md)
- [DNSZone Configuration](../concepts/dnszone.md)
- [Bind9 Integration](../concepts/architecture.md)
