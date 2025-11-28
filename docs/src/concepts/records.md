# DNS Records

Bindy supports all common DNS record types as Custom Resources.

## Supported Record Types

- **ARecord** - IPv4 address mapping
- **AAAARecord** - IPv6 address mapping
- **CNAMERecord** - Canonical name (alias)
- **MXRecord** - Mail exchange
- **TXTRecord** - Text data
- **NSRecord** - Nameserver delegation
- **SRVRecord** - Service location
- **CAARecord** - Certificate authority authorization

## Common Fields

All DNS record types share these fields:

```yaml
metadata:
  name: record-name
  namespace: dns-system
spec:
  zone: zone-resource-name  # References DNSZone
  name: record-name          # DNS name (@ for zone apex)
  ttl: 300                   # Time to live (optional)
```

## ARecord (IPv4)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example
spec:
  zone: example-com
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

[Learn more about A Records](../guide/a-records.md)

## AAAARecord (IPv6)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: AAAARecord
metadata:
  name: www-example-ipv6
spec:
  zone: example-com
  name: www
  ipv6Address: "2001:db8::1"
  ttl: 300
```

[Learn more about AAAA Records](../guide/aaaa-records.md)

## CNAMERecord (Alias)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: CNAMERecord
metadata:
  name: blog-example
spec:
  zone: example-com
  name: blog
  target: www.example.com.
  ttl: 300
```

[Learn more about CNAME Records](../guide/cname-records.md)

## MXRecord (Mail Exchange)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: MXRecord
metadata:
  name: mail-example
spec:
  zone: example-com
  name: "@"
  priority: 10
  mailServer: mail.example.com.
  ttl: 3600
```

[Learn more about MX Records](../guide/mx-records.md)

## TXTRecord (Text)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: spf-example
spec:
  zone: example-com
  name: "@"
  text:
    - "v=spf1 include:_spf.example.com ~all"
  ttl: 3600
```

[Learn more about TXT Records](../guide/txt-records.md)

## NSRecord (Nameserver)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: NSRecord
metadata:
  name: delegate-subdomain
spec:
  zone: example-com
  name: subdomain
  nameserver: ns1.subdomain.example.com.
  ttl: 3600
```

[Learn more about NS Records](../guide/ns-records.md)

## SRVRecord (Service)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: sip-service
spec:
  zone: example-com
  name: _sip._tcp
  priority: 10
  weight: 60
  port: 5060
  target: sipserver.example.com.
  ttl: 3600
```

[Learn more about SRV Records](../guide/srv-records.md)

## CAARecord (Certificate Authority)

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: CAARecord
metadata:
  name: letsencrypt-caa
spec:
  zone: example-com
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org
  ttl: 3600
```

[Learn more about CAA Records](../guide/caa-records.md)

## Record Management

### Referencing Zones

All records reference a DNSZone via the `zone` field:

```yaml
spec:
  zone: example-com  # Must match DNSZone metadata.name
```

### Zone Apex Records

Use `@` for zone apex records:

```yaml
spec:
  name: "@"  # Represents the zone itself
```

### Subdomain Records

Use the subdomain name:

```yaml
spec:
  name: www        # www.example.com
  name: api.v2     # api.v2.example.com
```

## Next Steps

- [Managing DNS Records](../guide/records-guide.md) - Complete record management guide
- [A Records](../guide/a-records.md) - IPv4 address records
- [CNAME Records](../guide/cname-records.md) - Alias records
- [MX Records](../guide/mx-records.md) - Mail server records
