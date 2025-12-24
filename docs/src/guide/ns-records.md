# NS Records (Name Server)

NS records delegate a subdomain to a different set of nameservers. This is essential for subdomain delegation and zone distribution.

## Creating an NS Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: subdomain-ns
  namespace: dns-system
spec:
  zoneRef: example-com  # References DNSZone metadata.name (recommended)
  name: sub              # Subdomain to delegate
  nameserver: ns1.subdomain-host.com.  # Must end with dot (FQDN)
  ttl: 3600
```

This delegates `sub.example.com` to `ns1.subdomain-host.com`.

**Note:** You can also use `zone: example.com` (matching `DNSZone.spec.zoneName`) instead of `zoneRef`. See [Referencing DNS Zones](./records-guide.md#referencing-dns-zones) for details.

## Subdomain Delegation

Delegate a subdomain to external nameservers:

```yaml
# Primary nameserver
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: dev-ns1
spec:
  zoneRef: example-com
  name: dev
  nameserver: ns1.hosting-provider.com.
---
# Secondary nameserver
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: dev-ns2
spec:
  zoneRef: example-com
  name: dev
  nameserver: ns2.hosting-provider.com.
```

Now `dev.example.com` is managed by the hosting provider's DNS servers.

## Common Use Cases

### Multi-Cloud Delegation

```yaml
# Delegate subdomain to AWS Route 53
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: aws-ns1
spec:
  zoneRef: example-com
  name: aws
  nameserver: ns-123.awsdns-12.com.
---
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: aws-ns2
spec:
  zoneRef: example-com
  name: aws
  nameserver: ns-456.awsdns-45.net.
```

### Environment Separation

```yaml
# Production environment
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: prod-ns1
spec:
  zoneRef: example-com
  name: prod
  nameserver: ns-prod1.example.com.
---
# Staging environment
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: staging-ns1
spec:
  zoneRef: example-com
  name: staging
  nameserver: ns-staging1.example.com.
```

## FQDN Requirement

**CRITICAL:** The `nameserver` field **MUST** end with a dot (`.`):

```yaml
# ✅ CORRECT
nameserver: ns1.example.com.

# ❌ WRONG
nameserver: ns1.example.com
```

## Glue Records

When delegating to nameservers within the delegated zone, you need glue records (A/AAAA):

```yaml
# NS delegation
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: sub-ns
spec:
  zoneRef: example-com
  name: sub
  nameserver: ns1.sub.example.com.  # Nameserver is within delegated zone
---
# Glue record (A record for the nameserver)
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: sub-ns-glue
spec:
  zoneRef: example-com
  name: ns1.sub
  ipv4Address: "203.0.113.10"
```

## Best Practices

1. **Use multiple NS records** - Always specify at least 2 nameservers for redundancy
2. **FQDNs only** - Always end nameserver values with a dot
3. **Match TTLs** - Use consistent TTLs across NS records for the same subdomain
4. **Glue records** - Provide A/AAAA records when NS points within delegated zone
5. **Test delegation** - Verify subdomain resolution after delegation

## Status Monitoring

```bash
kubectl get nsrecord subdomain-ns -o yaml
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

### Test NS delegation

```bash
# Query NS records
dig NS sub.example.com

# Test resolution through delegated nameservers
dig @ns1.subdomain-host.com www.sub.example.com
```

### Common Issues

- **Missing glue records** - Circular dependency if NS points within delegated zone
- **Wrong FQDN** - Missing trailing dot causes relative name
- **Single nameserver** - No redundancy if one server fails

## Next Steps

- [DNS Records Overview](./records-guide.md) - Complete guide to all record types
- [A Records](./a-records.md) - Create glue records for nameservers
- [Monitoring DNS](../operations/monitoring.md) - Monitor your DNS infrastructure
