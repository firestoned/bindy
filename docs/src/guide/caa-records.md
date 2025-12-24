# CAA Records (Certificate Authority Authorization)

CAA records specify which Certificate Authorities (CAs) are authorized to issue SSL/TLS certificates for your domain. This helps prevent unauthorized certificate issuance.

## Creating a CAA Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: letsencrypt-caa
  namespace: dns-system
spec:
  zoneRef: example-com  # References DNSZone metadata.name (recommended)
  name: "@"             # Apply to entire domain
  flags: 0              # Typically 0 (non-critical)
  tag: issue            # Tag: issue, issuewild, or iodef
  value: letsencrypt.org
  ttl: 3600
```

This authorizes Let's Encrypt to issue certificates for `example.com`.

**Note:** You can also use `zone: example.com` (matching `DNSZone.spec.zoneName`) instead of `zoneRef`. See [Referencing DNS Zones](./records-guide.md#referencing-dns-zones) for details.

## CAA Tags

### issue

Authorizes a CA to issue certificates for the domain:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-issue
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org  # Authorize Let's Encrypt
```

### issuewild

Authorizes a CA to issue wildcard certificates:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-wildcard
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issuewild
  value: letsencrypt.org  # Allow wildcard certificates
```

### iodef

Specifies URL/email for reporting policy violations:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-iodef-email
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: iodef
  value: mailto:security@example.com
---
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-iodef-url
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: iodef
  value: https://example.com/caa-report
```

## Common Configurations

### Let's Encrypt

```yaml
# Standard certificates
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-le-issue
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org
---
# Wildcard certificates
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-le-wildcard
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issuewild
  value: letsencrypt.org
```

### DigiCert

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-digicert
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issue
  value: digicert.com
```

### AWS Certificate Manager

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-aws
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issue
  value: amazon.com
---
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-aws-wildcard
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issuewild
  value: amazon.com
```

### Multiple CAs

Authorize multiple Certificate Authorities:

```yaml
# Let's Encrypt
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-letsencrypt
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org
---
# DigiCert
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-digicert
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issue
  value: digicert.com
```

### Deny All Issuance

Prevent any CA from issuing certificates:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-deny-all
spec:
  zoneRef: example-com
  name: "@"
  flags: 0
  tag: issue
  value: ";"  # Semicolon means no CA is authorized
```

## Flags

- **0** - Non-critical (default, recommended)
- **128** - Critical - CA MUST understand all CAA properties or refuse issuance

Most deployments use `flags: 0`.

## Subdomain CAA Records

Apply CAA policy to specific subdomains:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-staging
spec:
  zoneRef: example-com
  name: staging  # staging.example.com
  flags: 0
  tag: issue
  value: letsencrypt.org  # Only Let's Encrypt for staging
```

## Best Practices

1. **Start with permissive policies** - Allow your current CA before enforcing restrictions
2. **Test thoroughly** - Verify certificate renewal works after adding CAA
3. **Use iodef** - Configure reporting to catch unauthorized issuance attempts
4. **Document authorized CAs** - Maintain list of approved CAs in your security policy
5. **Regular audits** - Review CAA records periodically

## Certificate Authority Values

Common CA values for the `issue` and `issuewild` tags:

- Let's Encrypt: `letsencrypt.org`
- DigiCert: `digicert.com`
- AWS ACM: `amazon.com`
- GlobalSign: `globalsign.com`
- Sectigo (Comodo): `sectigo.com`
- GoDaddy: `godaddy.com`
- Google Trust Services: `pki.goog`

Check your CA's documentation for the correct value.

## Status Monitoring

```bash
kubectl get caarecord letsencrypt-caa -o yaml
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

### Test CAA records

```bash
# Query CAA records
dig CAA example.com

# Expected output
;; ANSWER SECTION:
example.com. 3600 IN CAA 0 issue "letsencrypt.org"
example.com. 3600 IN CAA 0 issuewild "letsencrypt.org"
```

### Certificate Issuance Failures

If certificate issuance fails after adding CAA:

1. Verify CA is authorized:
   ```bash
   dig CAA example.com
   ```

2. Check for typos in CA value

3. Ensure both `issue` and `issuewild` are configured if using wildcards

4. Test with online tools:
   - [SSLMate CAA Test](https://sslmate.com/caa/)
   - [DigiCert CAA Check](https://www.digicert.com/help/)

### Common Mistakes

- **Wrong CA value** - Each CA has a specific value (check their docs)
- **Missing issuewild** - Wildcard certificates need separate authorization
- **Critical flag** - Using `flags: 128` can cause issues if CA doesn't understand all tags

## Security Benefits

1. **Prevent unauthorized issuance** - CAs must check CAA before issuing
2. **Incident detection** - iodef tag provides violation notifications
3. **Defense in depth** - Additional layer beyond domain validation
4. **Compliance** - Many security standards recommend CAA records

## Next Steps

- [TXT Records](./txt-records.md) - Configure domain verification
- [DNS Records Overview](./records-guide.md) - Complete guide to all record types
- [Monitoring DNS](../operations/monitoring.md) - Monitor your DNS infrastructure
