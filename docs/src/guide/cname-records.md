# CNAME Records

CNAME (Canonical Name) records create aliases to other domain names.

## Creating a CNAME Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: blog-example-com
  namespace: dns-system
  labels:
    zone: example.com  # Used by DNSZone selector
spec:
  name: blog
  target: www.example.com.  # Must end with a dot
  ttl: 300
```

This creates `blog.example.com -> www.example.com`.

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
kind: CNAMERecord
metadata:
  name: blog
  labels:
    zone: example.com  # ✅ Matches selector above
spec:
  name: blog
  target: www.example.com.
```

See [Label Selector Guide](./label-selectors.md) for advanced patterns.

## Important CNAME Rules

### Target Must Be Fully Qualified

The `target` field must be a fully qualified domain name (FQDN) ending with a dot:

```yaml
# ✅ Correct
target: www.example.com.

# ❌ Incorrect - missing trailing dot
target: www.example.com
```

### No CNAME at Zone Apex

CNAME records **cannot** be created at the zone apex (`@`):

```yaml
# ❌ Not allowed - RFC 1034/1035 violation
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: apex-cname
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  target: www.example.com.
```

For the zone apex, use [A Records](./a-records.md) or [AAAA Records](./aaaa-records.md) instead.

### No Other Records for Same Name

If a CNAME exists for a name, no other record types can exist for that same name (RFC 1034):

```yaml
# ❌ Not allowed - www already has a CNAME
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: www-alias
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  target: server.example.com.
---
# ❌ This will conflict with the CNAME above
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-a-record
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www  # Same name as CNAME - not allowed
  ipv4Address: "192.0.2.1"
```

## Common Use Cases

### Aliasing to External Services

Point to external services like CDNs or cloud providers:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: cdn-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: cdn
  target: d111111abcdef8.cloudfront.net.
  ttl: 3600
```

### Subdomain Aliases

Create aliases for subdomains:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: shop-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: shop
  target: www.example.com.
  ttl: 300
```

This creates `shop.example.com -> www.example.com`.

### Internal Service Discovery

Point to internal Kubernetes services:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: cache-internal
  namespace: dns-system
  labels:
    zone: internal.local
spec:
  name: cache
  target: db.internal.local.
  ttl: 300
```

### www to Non-www Redirect

Create a www alias:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  target: example.com.
  ttl: 300
```

**Note:** This only works if `example.com` has an A or AAAA record, not another CNAME.

## Field Reference

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Record name within the zone (cannot be "@") |
| `target` | string | Yes | Target FQDN ending with a dot |
| `ttl` | integer | No | Time To Live in seconds (default: zone TTL) |

**Note:** Records are associated with zones using labels. Add a `zone` label in `metadata.labels` that matches the DNSZone's `recordsFrom` selector.

## TTL Behavior

If `ttl` is not specified, the zone's default TTL is used:

```yaml
# Uses zone default TTL
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: blog-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: blog
  target: www.example.com.
```

```yaml
# Explicit TTL override
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: blog-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: blog
  target: www.example.com.
  ttl: 600  # 10 minutes
```

## Troubleshooting

### CNAME Loop Detection

Avoid creating CNAME loops:

```yaml
# ❌ Creates a loop
# a.example.com -> b.example.com
# b.example.com -> a.example.com
---
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: cname-a
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: a
  target: b.example.com.
---
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: cname-b
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: b
  target: a.example.com.  # ❌ Loop!
```

### Missing Trailing Dot

If your CNAME doesn't resolve correctly, check for the trailing dot:

```bash
# Check the BIND9 zone file
kubectl exec -n dns-system bindy-primary-0 -- cat /etc/bind/zones/example.com.zone

# Should show:
# blog.example.com.  300  IN  CNAME  www.example.com.
```

If you see relative names, the target is missing the trailing dot:

```bind
# ❌ Wrong - becomes blog.example.com -> www.example.com.example.com
blog.example.com.  300  IN  CNAME  www.example.com
```

## See Also

- [A Records (IPv4)](./a-records.md)
- [AAAA Records (IPv6)](./aaaa-records.md)
- [DNS Records Guide](./records-guide.md)
- [DNSZone Custom Resource](./zones.md)
