# TXT Records (Text)

TXT records store arbitrary text data in DNS. They're commonly used for domain verification, email security (SPF, DKIM, DMARC), and other service configurations.

## Creating a TXT Record

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: verification-txt
  namespace: dns-system
spec:
  zoneRef: example-com  # References DNSZone metadata.name (recommended)
  name: "@"
  text: "v=spf1 include:_spf.example.com ~all"
  ttl: 3600
```

**Note:** You can also use `zone: example.com` (matching `DNSZone.spec.zoneName`) instead of `zoneRef`. See [Referencing DNS Zones](./records-guide.md#referencing-dns-zones) for details.

## Common Use Cases

### SPF (Sender Policy Framework)

Authorize mail servers to send email on behalf of your domain:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: spf-record
spec:
  zoneRef: example-com
  name: "@"
  text: "v=spf1 mx include:_spf.google.com ~all"
  ttl: 3600
```

Common SPF mechanisms:
- `mx` - Allow servers in MX records
- `a` - Allow A/AAAA records of domain
- `ip4:192.0.2.0/24` - Allow specific IPv4 range
- `include:domain.com` - Include another domain's SPF policy
- `~all` - Soft fail (recommended)
- `-all` - Hard fail (strict)

### DKIM (Domain Keys Identified Mail)

Publish DKIM public keys:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: dkim-selector
spec:
  zoneRef: example-com
  name: default._domainkey  # selector._domainkey format
  text: "v=DKIM1; k=rsa; p=MIGfMA0GCSqGSIb3DQEBA..."
  ttl: 3600
```

### DMARC (Domain-based Message Authentication)

Set email authentication policy:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: dmarc-policy
spec:
  zoneRef: example-com
  name: _dmarc
  text: "v=DMARC1; p=quarantine; rua=mailto:dmarc@example.com"
  ttl: 3600
```

DMARC policies:
- `p=none` - Monitor only (recommended for testing)
- `p=quarantine` - Treat failures as spam
- `p=reject` - Reject failures outright

### Domain Verification

Verify domain ownership for services:

```yaml
# Google verification
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: google-verification
spec:
  zoneRef: example-com
  name: "@"
  text: "google-site-verification=1234567890abcdef"
---
# Microsoft verification
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: ms-verification
spec:
  zoneRef: example-com
  name: "@"
  text: "MS=ms12345678"
```

### Service-Specific Records

#### Atlassian Domain Verification

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: atlassian-verify
spec:
  zoneRef: example-com
  name: "@"
  text: "atlassian-domain-verification=abc123"
```

#### Stripe Domain Verification

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: stripe-verify
spec:
  zoneRef: example-com
  name: "_stripe-verification"
  text: "stripe-verification=xyz789"
```

## Multiple TXT Values

Some records require multiple TXT strings. Create separate records:

```yaml
# SPF record
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: txt-spf
spec:
  zoneRef: example-com
  name: "@"
  text: "v=spf1 include:_spf.google.com ~all"
---
# Domain verification (same name, different value)
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: txt-verify
spec:
  zoneRef: example-com
  name: "@"
  text: "google-site-verification=abc123"
```

Both records will exist under the same DNS name.

## String Formatting

### Long Strings

DNS TXT records have a 255-character limit per string. For longer values, the DNS server automatically splits them:

```yaml
spec:
  text: "v=DKIM1; k=rsa; p=MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC..."  # Can be long
```

### Special Characters

Quote strings containing spaces or special characters:

```yaml
spec:
  text: "This string contains spaces"
  text: "key=value; another-key=another value"
```

## Best Practices

1. **Keep TTLs moderate** - 3600 (1 hour) is typical for TXT records
2. **Test before deploying** - Verify SPF/DKIM/DMARC records with online tools
3. **Monitor DMARC reports** - Set up `rua` and `ruf` addresses to receive reports
4. **Start with soft policies** - Use `~all` for SPF and `p=none` for DMARC initially
5. **Document record purposes** - Use clear resource names

## Status Monitoring

```bash
kubectl get txtrecord spf-record -o yaml
```

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSucceeded
      message: "Record configured on 3 endpoint(s)"
  observedGeneration: 1
```

## Troubleshooting

### Test TXT record

```bash
# Query TXT records
dig TXT example.com

# Test SPF
dig TXT example.com | grep spf

# Test DKIM
dig TXT default._domainkey.example.com

# Test DMARC
dig TXT _dmarc.example.com
```

### Online Validation Tools

- **SPF**: [mxtoolbox.com/spf.aspx](https://mxtoolbox.com/spf.aspx)
- **DKIM**: [mxtoolbox.com/dkim.aspx](https://mxtoolbox.com/dkim.aspx)
- **DMARC**: [mxtoolbox.com/dmarc.aspx](https://mxtoolbox.com/dmarc.aspx)

### Common Issues

- **SPF too long** - Limit DNS lookups to 10 (use `include` wisely)
- **DKIM not found** - Verify selector name matches mail server configuration
- **DMARC syntax error** - Validate with online tools before deploying

## Next Steps

- [MX Records](./mx-records.md) - Configure mail servers
- [DNS Records Overview](./records-guide.md) - Complete guide to all record types
- [Monitoring DNS](../operations/monitoring.md) - Monitor your DNS infrastructure
