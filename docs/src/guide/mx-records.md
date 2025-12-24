# MX Records (Mail Exchange)

MX records specify the mail servers responsible for accepting email on behalf of a domain. Each MX record includes a priority value that determines the order in which mail servers are contacted.

## Creating an MX Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mail-example
  namespace: dns-system
spec:
  zoneRef: example-com  # References DNSZone metadata.name (recommended)
  name: "@"             # Zone apex - mail for @example.com
  priority: 10
  mailServer: mail.example.com.  # Must end with a dot (FQDN)
  ttl: 3600
```

This configures mail delivery for `example.com` to `mail.example.com` with priority 10.

**Note:** You can also use `zone: example.com` (matching `DNSZone.spec.zoneName`) instead of `zoneRef`. See [Referencing DNS Zones](./records-guide.md#referencing-dns-zones) for details on choosing between `zone` and `zoneRef`.

## FQDN Requirement

**CRITICAL:** The `mailServer` field **MUST** end with a dot (`.`) to indicate a fully qualified domain name (FQDN).

```yaml
# ✅ CORRECT
mailServer: mail.example.com.

# ❌ WRONG - will be treated as relative to zone
mailServer: mail.example.com
```

## Priority Values

Lower priority values are preferred. Mail servers with the lowest priority are contacted first.

### Single Mail Server

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-primary
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: mail.example.com.
```

### Multiple Mail Servers (Failover)

```yaml
# Primary mail server (lowest priority)
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-primary
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: mail1.example.com.
  ttl: 3600
---
# Backup mail server
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-backup
spec:
  zoneRef: example-com
  name: "@"
  priority: 20
  mailServer: mail2.example.com.
  ttl: 3600
```

Sending servers will try `mail1.example.com` first (priority 10), falling back to `mail2.example.com` (priority 20) if the primary is unavailable.

### Load Balancing

Equal priority values enable round-robin load balancing:

```yaml
# Server 1
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-1
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: mail1.example.com.
---
# Server 2 (same priority)
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-2
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: mail2.example.com.
```

Both servers share the load equally.

## Subdomain Mail

Configure mail for a subdomain:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: support-mail
spec:
  zoneRef: example-com
  name: support  # Email: user@support.example.com
  priority: 10
  mailServer: mail-support.example.com.
```

## Common Configurations

### Google Workspace (formerly G Suite)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-google-1
spec:
  zoneRef: example-com
  name: "@"
  priority: 1
  mailServer: aspmx.l.google.com.
  ttl: 3600
---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-google-2
spec:
  zoneRef: example-com
  name: "@"
  priority: 5
  mailServer: alt1.aspmx.l.google.com.
---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-google-3
spec:
  zoneRef: example-com
  name: "@"
  priority: 5
  mailServer: alt2.aspmx.l.google.com.
---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-google-4
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: alt3.aspmx.l.google.com.
---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-google-5
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: alt4.aspmx.l.google.com.
```

### Microsoft 365

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-microsoft
spec:
  zoneRef: example-com
  name: "@"
  priority: 0
  mailServer: example-com.mail.protection.outlook.com.  # Replace 'example-com' with your domain
  ttl: 3600
```

### Self-Hosted Mail Server

```yaml
# Primary MX
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-primary
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: mail.example.com.
---
# Corresponding A record for mail server
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: mail-server
spec:
  zoneRef: example-com
  name: mail
  ipv4Address: "203.0.113.10"
```

## Best Practices

1. **Always use FQDNs** - End `mailServer` values with a dot (`.`)
2. **Set appropriate TTLs** - Use longer TTLs (3600-86400) for stable mail configurations
3. **Configure backups** - Use multiple MX records with different priorities for redundancy
4. **Test mail delivery** - Verify mail flow after DNS changes
5. **Coordinate with SPF/DKIM** - Update TXT records when adding mail servers

## Required Supporting Records

MX records need corresponding A/AAAA records for the mail servers:

```yaml
# MX record points to mail.example.com
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-main
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: mail.example.com.
---
# A record for mail.example.com
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: mail-server-ipv4
spec:
  zoneRef: example-com
  name: mail
  ipv4Address: "203.0.113.10"
---
# AAAA record for IPv6
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: mail-server-ipv6
spec:
  zoneRef: example-com
  name: mail
  ipv6Address: "2001:db8::10"
```

## Status Monitoring

Check the status of your MX record:

```bash
kubectl get mxrecord mx-primary -o yaml
```

Look for the `status.conditions` field:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSucceeded
      message: "Record configured on 3 endpoint(s)"
      lastTransitionTime: "2024-11-26T10:00:01Z"
  observedGeneration: 1
```

## Troubleshooting

### Mail not being delivered

1. Check MX record status:
   ```bash
   kubectl get mxrecord mx-primary -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}'
   ```

2. Verify DNS propagation:
   ```bash
   dig MX example.com @<dns-server-ip>
   ```

3. Test from external servers:
   ```bash
   nslookup -type=MX example.com 8.8.8.8
   ```

4. Check mail server A/AAAA records exist:
   ```bash
   dig A mail.example.com
   ```

### Common Mistakes

- **Missing trailing dot** - `mail.example.com` instead of `mail.example.com.`
- **No A/AAAA record** - MX points to a hostname that doesn't resolve
- **Wrong priority** - Higher priority when you meant lower (remember: lower = preferred)
- **Relative vs absolute** - Without trailing dot, name is treated as relative to zone

## Testing Mail Configuration

### Test MX lookup

```bash
# Query MX records
dig MX example.com

# Expected output shows priority and mail server
;; ANSWER SECTION:
example.com.  3600  IN  MX  10 mail.example.com.
example.com.  3600  IN  MX  20 mail2.example.com.
```

### Test mail server connectivity

```bash
# Test SMTP connection
telnet mail.example.com 25

# Or using openssl for TLS
openssl s_client -starttls smtp -connect mail.example.com:25
```

## Next Steps

- [TXT Records](./txt-records.md) - Configure SPF, DKIM, DMARC for mail authentication
- [A Records](./a-records.md) - Create A records for mail servers
- [DNS Records Overview](./records-guide.md) - Complete guide to all record types
- [Monitoring DNS](../operations/monitoring.md) - Monitor your DNS infrastructure
