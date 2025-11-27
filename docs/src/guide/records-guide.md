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
apiVersion: dns.firestoned.io/v1alpha1
kind: <RecordType>
metadata:
  name: <unique-name>
  namespace: dns-system
spec:
  zone: <zone-resource-name>  # References a DNSZone resource
  name: <record-name>          # Name within the zone
  ttl: <optional-ttl>          # Override zone default TTL
  # ... record-specific fields
```

## Creating Records

Records must reference an existing DNSZone:

```yaml
spec:
  zone: example-com  # Must match a DNSZone resource name
  name: www          # Creates www.example.com
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
