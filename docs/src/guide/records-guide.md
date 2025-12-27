# Managing DNS Records

DNS records are the actual data in your zones - IP addresses, mail servers, text data, etc.

## Record Types

Bindy supports all common DNS record types:

- **A Records** - IPv4 addresses
- **AAAA Records** - IPv6 addresses
- **CNAME Records** - Canonical name (alias)
- **MX Records** - Mail exchange servers
- **TXT Records** - Text data (SPF, DKIM, DMARC, verification)
- **NS Records** - Nameserver delegation
- **SRV Records** - Service location
- **CAA Records** - Certificate authority authorization

## Record Structure

All records share common fields:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: <RecordType>
metadata:
  name: <unique-name>
  namespace: dns-system
  labels:
    zone: <zone-name>  # Used by DNSZone selector
spec:
  name: <record-name>  # Name within the zone
  ttl: <optional-ttl>  # Override zone default TTL
  # ... record-specific fields
```

## How Records Are Associated with Zones

DNS records are discovered by DNSZones using **label selectors**, similar to how Kubernetes Services select Pods.

### Label Selector Pattern

The DNSZone resource defines a `recordsFrom` field with label selectors. Any DNS record with matching labels will be included in the zone:

```yaml
# DNSZone defines the selector
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-dns
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com  # Selects records with this label
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
```

### Creating Records

Records are associated with zones by adding matching labels:

```yaml
# This record will be selected by the DNSZone above
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    zone: example.com  # ✅ Matches the DNSZone selector
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

The controller automatically discovers this record and adds it to the `example.com` zone.

### Why Label Selectors?

Label selectors provide:

1. **Flexibility** - Use any label structure that fits your organization
2. **Multi-zone records** - Same record can belong to multiple zones
3. **Dynamic selection** - Adding/removing labels automatically updates zones
4. **Kubernetes-native** - Familiar pattern for Kubernetes users
5. **Environment separation** - Use labels like `env: production` or `env: staging`

### Advanced Selector Patterns

See the [Label Selector Guide](./label-selectors.md) for advanced patterns including:
- Multi-label selectors
- Environment-based selection
- Team/ownership labels
- Wildcard matching

## Creating Records

After setting up your zone and labels, specify the record details:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: api-server
  namespace: dns-system
  labels:
    zone: example.com
    env: production
    team: platform
spec:
  name: api             # Creates api.example.com
  ipv4Address: "192.0.2.10"
  ttl: 300              # Optional - overrides zone default
```

## Next Steps

- [A Records](./a-records.md) - IPv4 addresses
- [AAAA Records](./aaaa-records.md) - IPv6 addresses
- [CNAME Records](./cname-records.md) - Aliases
- [MX Records](./mx-records.md) - Mail servers
- [TXT Records](./txt-records.md) - Text data
- [NS Records](./ns-records.md) - Delegation
- [SRV Records](./srv-records.md) - Services
- [CAA Records](./caa-records.md) - Certificate authority
- [Label Selector Guide](./label-selectors.md) - Advanced selector patterns
