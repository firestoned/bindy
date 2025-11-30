# DNS Record Specifications

Complete specifications for all DNS record types.

## Common Fields

All DNS record types share these common fields:

### zone / zoneRef
**Type**: string
**Required**: Exactly one of `zone` or `zoneRef` must be specified

Reference to the parent DNSZone resource. Use **one** of the following:

**`zone` field** - Matches against `DNSZone.spec.zoneName` (the actual DNS zone name):
```yaml
spec:
  zone: "example.com"  # Matches DNSZone with spec.zoneName: example.com
```

**`zoneRef` field** - Direct reference to `DNSZone.metadata.name` (the Kubernetes resource name, recommended for production):
```yaml
spec:
  zoneRef: "example-com"  # Matches DNSZone with metadata.name: example-com
```

**Important**: You must specify **exactly one** of `zone` or `zoneRef` - not both, not neither.

See [Referencing DNS Zones](../guide/records-guide.md#referencing-dns-zones) for detailed comparison and best practices.

### name
**Type**: string
**Required**: Yes

The record name within the zone.

```yaml
spec:
  name: "www"  # Creates www.example.com
  name: "@"    # Creates record at zone apex (example.com)
```

### ttl
**Type**: integer
**Required**: No
**Default**: Inherited from zone

Time To Live in seconds.

```yaml
spec:
  ttl: 300  # 5 minutes
```

---

## A Record (IPv4 Address)

Maps hostnames to IPv4 addresses.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example-com
  namespace: dns-system
spec:
  zoneRef: "example-com"
  name: "www"
  ipv4Address: "192.0.2.1"
  ttl: 300
```

### Fields

#### ipv4Address
**Type**: string
**Required**: Yes

IPv4 address in dotted decimal notation.

```yaml
spec:
  ipv4Address: "192.0.2.1"
```

### Example: Multiple A Records (Round Robin)

```yaml
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example-com-1
spec:
  zoneRef: "example-com"
  name: "www"
  ipv4Address: "192.0.2.1"
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example-com-2
spec:
  zoneRef: "example-com"
  name: "www"
  ipv4Address: "192.0.2.2"
```

---

## AAAA Record (IPv6 Address)

Maps hostnames to IPv6 addresses.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: AAAARecord
metadata:
  name: www-example-com-v6
  namespace: dns-system
spec:
  zoneRef: "example-com"
  name: "www"
  ipv6Address: "2001:db8::1"
  ttl: 300
```

### Fields

#### ipv6Address
**Type**: string
**Required**: Yes

IPv6 address in colon-separated hexadecimal notation.

```yaml
spec:
  ipv6Address: "2001:db8::1"
```

**Formats**:
- Full: "2001:0db8:0000:0000:0000:0000:0000:0001"
- Compressed: "2001:db8::1"

### Example: Dual Stack (IPv4 + IPv6)

```yaml
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-v4
spec:
  zoneRef: "example-com"
  name: "www"
  ipv4Address: "192.0.2.1"
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: AAAARecord
metadata:
  name: www-v6
spec:
  zoneRef: "example-com"
  name: "www"
  ipv6Address: "2001:db8::1"
```

---

## CNAME Record (Canonical Name)

Creates an alias from one hostname to another.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: CNAMERecord
metadata:
  name: www-alias
  namespace: dns-system
spec:
  zoneRef: "example-com"
  name: "www"
  target: "server.example.com."
  ttl: 3600
```

### Fields

#### target
**Type**: string
**Required**: Yes

Target hostname (FQDN recommended).

```yaml
spec:
  target: "server.example.com."
```

### Restrictions

- Cannot be created at zone apex (@)
- Cannot coexist with other record types for same name
- Target should be fully qualified (end with dot)

### Example: CDN Alias

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: CNAMERecord
metadata:
  name: cdn-alias
spec:
  zoneRef: "example-com"
  name: "cdn"
  target: "d123456.cloudfront.net."
```

---

## MX Record (Mail Exchange)

Specifies mail servers for the domain.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: MXRecord
metadata:
  name: mail-primary
  namespace: dns-system
spec:
  zoneRef: "example-com"
  name: "@"
  priority: 10
  mailServer: "mail.example.com."
  ttl: 3600
```

### Fields

#### priority
**Type**: integer
**Required**: Yes

Priority (preference) value. Lower values are preferred.

```yaml
spec:
  priority: 10  # Primary mail server
  priority: 20  # Backup mail server
```

#### mailServer
**Type**: string
**Required**: Yes

Hostname of mail server (FQDN recommended).

```yaml
spec:
  mailServer: "mail.example.com."
```

### Example: Primary and Backup Mail Servers

```yaml
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: MXRecord
metadata:
  name: mail-primary
spec:
  zoneRef: "example-com"
  name: "@"
  priority: 10
  mailServer: "mail1.example.com."
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: MXRecord
metadata:
  name: mail-backup
spec:
  zoneRef: "example-com"
  name: "@"
  priority: 20
  mailServer: "mail2.example.com."
```

---

## TXT Record (Text)

Stores arbitrary text data, commonly used for verification and policies.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: spf-record
  namespace: dns-system
spec:
  zoneRef: "example-com"
  name: "@"
  text:
    - "v=spf1 mx -all"
  ttl: 3600
```

### Fields

#### text
**Type**: array of strings
**Required**: Yes

Text values. Multiple strings are concatenated.

```yaml
spec:
  text:
    - "v=spf1 mx -all"
```

### Example: SPF, DKIM, and DMARC

```yaml
---
# SPF Record
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: spf
spec:
  zoneRef: "example-com"
  name: "@"
  text:
    - "v=spf1 mx include:_spf.google.com ~all"
---
# DKIM Record
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: dkim
spec:
  zoneRef: "example-com"
  name: "default._domainkey"
  text:
    - "v=DKIM1; k=rsa; p=MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC..."
---
# DMARC Record
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: dmarc
spec:
  zoneRef: "example-com"
  name: "_dmarc"
  text:
    - "v=DMARC1; p=quarantine; rua=mailto:dmarc@example.com"
```

---

## NS Record (Name Server)

Delegates a subdomain to different nameservers.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: NSRecord
metadata:
  name: subdomain-delegation
  namespace: dns-system
spec:
  zoneRef: "example-com"
  name: "subdomain"
  nameserver: "ns1.subdomain.example.com."
  ttl: 3600
```

### Fields

#### nameserver
**Type**: string
**Required**: Yes

Nameserver hostname (FQDN recommended).

```yaml
spec:
  nameserver: "ns1.subdomain.example.com."
```

### Example: Subdomain Delegation

```yaml
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: NSRecord
metadata:
  name: sub-ns1
spec:
  zoneRef: "example-com"
  name: "subdomain"
  nameserver: "ns1.subdomain.example.com."
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: NSRecord
metadata:
  name: sub-ns2
spec:
  zoneRef: "example-com"
  name: "subdomain"
  nameserver: "ns2.subdomain.example.com."
```

---

## SRV Record (Service)

Specifies location of services.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: sip-service
  namespace: dns-system
spec:
  zoneRef: "example-com"
  name: "_sip._tcp"
  priority: 10
  weight: 60
  port: 5060
  target: "sip.example.com."
  ttl: 3600
```

### Fields

#### priority
**Type**: integer
**Required**: Yes

Priority for target selection. Lower values are preferred.

```yaml
spec:
  priority: 10
```

#### weight
**Type**: integer
**Required**: Yes

Relative weight for same-priority targets.

```yaml
spec:
  weight: 60  # 60% of traffic
  weight: 40  # 40% of traffic
```

#### port
**Type**: integer
**Required**: Yes

Port number where service is available.

```yaml
spec:
  port: 5060
```

#### target
**Type**: string
**Required**: Yes

Hostname providing the service.

```yaml
spec:
  target: "sip.example.com."
```

### Example: Load Balanced Service

```yaml
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: srv-primary
spec:
  zoneRef: "example-com"
  name: "_service._tcp"
  priority: 10
  weight: 60
  port: 8080
  target: "server1.example.com."
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: srv-secondary
spec:
  zoneRef: "example-com"
  name: "_service._tcp"
  priority: 10
  weight: 40
  port: 8080
  target: "server2.example.com."
```

---

## CAA Record (Certificate Authority Authorization)

Restricts which CAs can issue certificates for the domain.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: CAARecord
metadata:
  name: caa-letsencrypt
  namespace: dns-system
spec:
  zoneRef: "example-com"
  name: "@"
  flags: 0
  tag: "issue"
  value: "letsencrypt.org"
  ttl: 3600
```

### Fields

#### flags
**Type**: integer
**Required**: Yes

Flags byte. Typically 0 (non-critical) or 128 (critical).

```yaml
spec:
  flags: 0
```

#### tag
**Type**: string
**Required**: Yes

Property tag.

**Valid Tags**:
- "issue" - Authorize CA to issue certificates
- "issuewild" - Authorize CA to issue wildcard certificates
- "iodef" - URL for violation reports

```yaml
spec:
  tag: "issue"
```

#### value
**Type**: string
**Required**: Yes

Property value (CA domain or URL).

```yaml
spec:
  value: "letsencrypt.org"
```

### Example: Multiple CAA Records

```yaml
---
# Allow Let's Encrypt for regular certs
apiVersion: bindy.firestoned.io/v1alpha1
kind: CAARecord
metadata:
  name: caa-issue
spec:
  zoneRef: "example-com"
  name: "@"
  flags: 0
  tag: "issue"
  value: "letsencrypt.org"
---
# Allow Let's Encrypt for wildcard certs
apiVersion: bindy.firestoned.io/v1alpha1
kind: CAARecord
metadata:
  name: caa-issuewild
spec:
  zoneRef: "example-com"
  name: "@"
  flags: 0
  tag: "issuewild"
  value: "letsencrypt.org"
---
# Violation reporting
apiVersion: bindy.firestoned.io/v1alpha1
kind: CAARecord
metadata:
  name: caa-iodef
spec:
  zoneRef: "example-com"
  name: "@"
  flags: 0
  tag: "iodef"
  value: "mailto:security@example.com"
```

---

## Related Resources

- [API Reference](./api.md)
- [DNSZone Specification](./dnszone-spec.md)
- [Examples](./examples.md)
- [DNS Records Guide](../guide/records-guide.md)
