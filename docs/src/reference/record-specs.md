# DNS Record Specifications

Complete reference for all DNS record types supported by Bindy.

## How Records Associate with Zones

DNS records are discovered by DNSZones using **Kubernetes label selectors**, similar to how Services select Pods or NetworkPolicies select endpoints. This declarative pattern allows flexible, dynamic record-to-zone associations without hardcoded references.

### Pattern: DNSZone with Selector

A `DNSZone` uses `recordsFrom` label selectors to discover DNS records:

```yaml
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
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
```

### Pattern: Record with Matching Labels

DNS records include labels in their metadata that match a DNSZone's selector:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    zone: example.com  # ✅ Matches DNSZone selector
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

When the DNSZone reconciler runs, it:
1. Evaluates the `recordsFrom` selector against all DNS records in the namespace
2. Includes matching records in the zone's BIND9 configuration
3. Updates `DNSZone.status.records` with references to associated records

See the [Label Selector Guide](../guide/label-selectors.md) for advanced selector patterns including `matchExpressions`, multi-environment zones, and dynamic record routing.

---

## Common Fields

All DNS record types share these fields in their **spec**:

### name
**Type**: string
**Required**: Yes

The record name within the zone.

- Use `@` for the zone apex (the domain itself)
- Use subdomain names like `www`, `api`, `mail`, `ftp`
- For services, use `_service._protocol` format (e.g., `_ldap._tcp`)

```yaml
spec:
  name: "www"  # Creates www.example.com
  name: "@"    # Creates record at zone apex (example.com)
  name: "api"  # Creates api.example.com
```

### ttl
**Type**: integer
**Required**: No
**Default**: Inherited from DNSZone

Time To Live in seconds (0 to 2,147,483,647). Controls how long DNS resolvers cache this record.

```yaml
spec:
  ttl: 300   # 5 minutes
  ttl: 3600  # 1 hour
  ttl: 86400 # 1 day
```

---

## Common Metadata

All DNS records should have appropriate labels in their metadata for DNSZone discovery.

### labels.zone
**Type**: string (in metadata.labels)
**Required**: Yes (for DNSZone to discover the record)

The zone name that matches a DNSZone's `recordsFrom` selector. This label is the standard way to associate records with zones.

```yaml
metadata:
  labels:
    zone: example.com  # Matches DNSZone with selector matchLabels: {zone: example.com}
```

You can use additional labels for advanced selector patterns:

```yaml
metadata:
  labels:
    zone: example.com
    environment: production
    app: podinfo
    team: platform
```

---

## A Record (IPv4 Address)

Maps hostnames to IPv4 addresses.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example-com
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

### Fields

#### ipv4Address
**Type**: string
**Required**: Yes

IPv4 address in dotted-decimal notation.

```yaml
spec:
  ipv4Address: "192.0.2.1"
```

Must be a valid IPv4 address (0-255 for each octet).

### Example: Multiple A Records (Round Robin)

Create multiple A records with the same name for DNS load balancing:

```yaml
---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example-com-1
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example-com-2
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.2"
  ttl: 300
```

DNS resolvers will receive both addresses and typically rotate between them.

---

## AAAA Record (IPv6 Address)

Maps hostnames to IPv6 addresses.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-example-com-v6
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv6Address: "2001:db8::1"
  ttl: 300
```

### Fields

#### ipv6Address
**Type**: string
**Required**: Yes

IPv6 address in standard colon-separated hexadecimal notation.

```yaml
spec:
  ipv6Address: "2001:db8::1"
```

**Supported Formats**:
- Full notation: `2001:0db8:0000:0000:0000:0000:0000:0001`
- Compressed notation: `2001:db8::1` (recommended)
- Loopback: `::1`
- Link-local: `fe80::1`

### Example: Dual Stack (IPv4 + IPv6)

Provide both IPv4 and IPv6 addresses for the same hostname:

```yaml
---
# IPv4 address
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-v4
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
---
# IPv6 address
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-v6
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv6Address: "2001:db8::1"
  ttl: 300
```

Clients will prefer IPv6 when available and fall back to IPv4.

---

## CNAME Record (Canonical Name)

Creates an alias from one hostname to another.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: blog-alias
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: blog
  target: example.github.io.
  ttl: 3600
```

### Fields

#### target
**Type**: string
**Required**: Yes

Target hostname (canonical name). Should be a fully qualified domain name ending with a dot.

```yaml
spec:
  target: "server.example.com."
```

### Restrictions

- **Cannot be created at zone apex** - CNAME records cannot use `name: "@"`
- **Cannot coexist with other record types** - A CNAME is mutually exclusive with A, AAAA, MX, TXT, etc. for the same name
- **Target should be FQDN** - End with a dot to avoid ambiguity

### Example: CDN Alias

Alias your domain to a CDN distribution:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: cdn-alias
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: cdn
  target: d123456.cloudfront.net.
  ttl: 3600
```

---

## MX Record (Mail Exchange)

Specifies mail servers for the domain.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mail-primary
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  priority: 10
  mailServer: mail.example.com.
  ttl: 3600
```

### Fields

#### priority
**Type**: integer
**Required**: Yes
**Range**: 0-65535

Priority (preference) value. **Lower values are preferred** by mail servers.

```yaml
spec:
  priority: 10  # Primary mail server (higher priority)
  priority: 20  # Backup mail server (lower priority)
```

#### mailServer
**Type**: string
**Required**: Yes

Fully qualified domain name of the mail server. Should end with a dot.

```yaml
spec:
  mailServer: "mail.example.com."
```

### Example: Primary and Backup Mail Servers

Configure multiple MX records with different priorities:

```yaml
---
# Primary mail server (priority 10)
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mail-primary
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  priority: 10
  mailServer: mail1.example.com.
  ttl: 3600
---
# Backup mail server (priority 20)
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mail-backup
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  priority: 20
  mailServer: mail2.example.com.
  ttl: 3600
```

Mail delivery will try the primary server first, then fall back to the backup if the primary is unreachable.

---

## TXT Record (Text)

Stores arbitrary text data, commonly used for verification and policies.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: spf-record
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  text:
    - "v=spf1 mx -all"
  ttl: 3600
```

### Fields

#### text
**Type**: array of strings
**Required**: Yes

Text values. Each string can be up to 255 characters. Multiple strings are concatenated by DNS resolvers.

```yaml
spec:
  text:
    - "v=spf1 mx -all"
```

For long text values, split into multiple strings:

```yaml
spec:
  text:
    - "v=DKIM1; k=rsa; p=MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC..."
    - "continuation of the key material here..."
```

### Example: SPF, DKIM, and DMARC

Configure email authentication records:

```yaml
---
# SPF Record - Authorize mail servers
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: spf
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  text:
    - "v=spf1 mx include:_spf.google.com ~all"
  ttl: 3600
---
# DKIM Record - Email signing key
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: dkim
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "default._domainkey"
  text:
    - "v=DKIM1; k=rsa; p=MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC..."
  ttl: 3600
---
# DMARC Record - Email authentication policy
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: dmarc
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "_dmarc"
  text:
    - "v=DMARC1; p=quarantine; rua=mailto:dmarc@example.com"
  ttl: 3600
```

---

## NS Record (Name Server)

Delegates a subdomain to other nameservers.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: subdomain-delegation
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: subdomain
  nameserver: ns1.subdomain.example.com.
  ttl: 3600
```

### Fields

#### nameserver
**Type**: string
**Required**: Yes

Fully qualified domain name of the nameserver. Should end with a dot.

```yaml
spec:
  nameserver: "ns1.subdomain.example.com."
```

### Example: Subdomain Delegation

Delegate a subdomain to external nameservers:

```yaml
---
# First nameserver for subdomain
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: sub-ns1
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: subdomain
  nameserver: ns1.subdomain.example.com.
  ttl: 86400
---
# Second nameserver for subdomain
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: sub-ns2
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: subdomain
  nameserver: ns2.subdomain.example.com.
  ttl: 86400
```

Queries for `*.subdomain.example.com` will be delegated to the specified nameservers.

---

## SRV Record (Service)

Specifies the location of services.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: SRVRecord
metadata:
  name: ldap-service
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: _ldap._tcp
  priority: 10
  weight: 60
  port: 389
  target: ldap.example.com.
  ttl: 3600
```

### Fields

#### priority
**Type**: integer
**Required**: Yes
**Range**: 0-65535

Priority for target selection. **Lower values are preferred**.

```yaml
spec:
  priority: 10  # Higher priority
  priority: 20  # Lower priority (fallback)
```

#### weight
**Type**: integer
**Required**: Yes
**Range**: 0-65535

Relative weight for same-priority targets. **Higher values receive more traffic**.

```yaml
spec:
  weight: 60  # ~60% of traffic
  weight: 40  # ~40% of traffic
```

#### port
**Type**: integer
**Required**: Yes
**Range**: 0-65535

TCP or UDP port where the service is available.

```yaml
spec:
  port: 389   # LDAP
  port: 5060  # SIP
  port: 8080  # HTTP alternate
```

#### target
**Type**: string
**Required**: Yes

Fully qualified domain name of the target host. Should end with a dot.

Use `"."` to indicate "service not available at this domain".

```yaml
spec:
  target: "ldap.example.com."
```

### Example: Load Balanced Service

Distribute traffic across multiple servers:

```yaml
---
# Primary server (60% of traffic)
apiVersion: bindy.firestoned.io/v1beta1
kind: SRVRecord
metadata:
  name: srv-primary
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: _service._tcp
  priority: 10
  weight: 60
  port: 8080
  target: server1.example.com.
  ttl: 3600
---
# Secondary server (40% of traffic)
apiVersion: bindy.firestoned.io/v1beta1
kind: SRVRecord
metadata:
  name: srv-secondary
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: _service._tcp
  priority: 10
  weight: 40
  port: 8080
  target: server2.example.com.
  ttl: 3600
```

---

## CAA Record (Certificate Authority Authorization)

Restricts which certificate authorities can issue certificates for the domain.

### Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-letsencrypt
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org
  ttl: 3600
```

### Fields

#### flags
**Type**: integer
**Required**: Yes
**Range**: 0-255

Flags byte. Use:
- `0` - Non-critical (CAs may ignore unknown tags)
- `128` - Critical (CAs must understand the tag or refuse issuance)

```yaml
spec:
  flags: 0    # Non-critical
  flags: 128  # Critical
```

#### tag
**Type**: string
**Required**: Yes

Property tag specifying the CAA record type.

**Valid Tags**:
- `"issue"` - Authorize CA to issue certificates
- `"issuewild"` - Authorize CA to issue wildcard certificates
- `"iodef"` - URL or email for violation reports

```yaml
spec:
  tag: "issue"      # Regular certificates
  tag: "issuewild"  # Wildcard certificates
  tag: "iodef"      # Violation reporting
```

#### value
**Type**: string
**Required**: Yes

Property value (format depends on the tag).

- For `"issue"` / `"issuewild"`: CA domain name (e.g., `"letsencrypt.org"`)
- For `"iodef"`: `mailto:` or `https:` URL for reports

```yaml
spec:
  value: "letsencrypt.org"
  value: "mailto:security@example.com"
  value: "https://ca.example.com/report"
```

### Example: Multiple CAA Records

Configure complete CAA policy:

```yaml
---
# Allow Let's Encrypt for regular certificates
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-issue
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org
  ttl: 86400
---
# Allow Let's Encrypt for wildcard certificates
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-issuewild
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  flags: 0
  tag: issuewild
  value: letsencrypt.org
  ttl: 86400
---
# Violation reporting
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-iodef
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  flags: 0
  tag: iodef
  value: mailto:security@example.com
  ttl: 86400
```

---

## Related Resources

- [API Reference](./api.md) - Complete CRD API documentation
- [DNSZone Specification](./dnszone-spec.md) - DNSZone CRD reference
- [Label Selector Guide](../guide/label-selectors.md) - Advanced selector patterns
- [DNS Records Guide](../guide/records-guide.md) - Creating and managing records
- [Examples](./examples.md) - Real-world configuration examples
