# AAAA Records (IPv6)

AAAA records map domain names to IPv6 addresses. They are the IPv6 equivalent of A records.

## Creating an AAAA Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-example-ipv6
  namespace: dns-system
spec:
  zoneRef: example-com  # References DNSZone metadata.name (recommended)
  name: www
  ipv6Address: "2001:db8::1"
  ttl: 300
```

This creates `www.example.com -> 2001:db8::1`.

**Note:** You can also use `zone: example.com` (matching `DNSZone.spec.zoneName`) instead of `zoneRef`. See [Referencing DNS Zones](./records-guide.md#referencing-dns-zones) for details on choosing between `zone` and `zoneRef`.

## Root Record

For the zone apex (example.com):

```yaml
spec:
  zoneRef: example-com
  name: "@"
  ipv6Address: "2001:db8::1"
```

## Multiple AAAA Records

Create multiple records for the same name for load balancing:

```bash
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-ipv6-1
spec:
  zoneRef: example-com
  name: www
  ipv6Address: "2001:db8::1"
---
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-ipv6-2
spec:
  zoneRef: example-com
  name: www
  ipv6Address: "2001:db8::2"
EOF
```

DNS clients will receive both addresses (round-robin load balancing).

## Dual-Stack Configuration

For dual-stack (IPv4 + IPv6) configuration, create both A and AAAA records:

```yaml
# IPv4
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-ipv4
spec:
  zoneRef: example-com
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
---
# IPv6
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-ipv6
spec:
  zoneRef: example-com
  name: www
  ipv6Address: "2001:db8::1"
  ttl: 300
```

Clients will use IPv6 if available, falling back to IPv4 otherwise.

## IPv6 Address Formats

IPv6 addresses support various formats:

```yaml
# Full format
ipv6Address: "2001:0db8:0000:0000:0000:0000:0000:0001"

# Compressed format (recommended)
ipv6Address: "2001:db8::1"

# Link-local address
ipv6Address: "fe80::1"

# Loopback
ipv6Address: "::1"

# IPv4-mapped IPv6
ipv6Address: "::ffff:192.0.2.1"
```

## Common Use Cases

### Web Server

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: web-ipv6
spec:
  zoneRef: example-com
  name: www
  ipv6Address: "2001:db8:1::443"
  ttl: 300
```

### API Endpoint

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: api-ipv6
spec:
  zoneRef: example-com
  name: api
  ipv6Address: "2001:db8:2::443"
  ttl: 60  # Short TTL for faster updates
```

### Mail Server

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: mail-ipv6
spec:
  zoneRef: example-com
  name: mail
  ipv6Address: "2001:db8:3::25"
  ttl: 3600
```

## Best Practices

1. **Use compressed format** - `2001:db8::1` instead of `2001:0db8:0000:0000:0000:0000:0000:0001`
2. **Dual-stack when possible** - Provide both A and AAAA records for compatibility
3. **Match TTLs** - Use the same TTL for A and AAAA records of the same name
4. **Test IPv6 connectivity** - Ensure your infrastructure supports IPv6 before advertising AAAA records

## Status Monitoring

Check the status of your AAAA record:

```bash
kubectl get aaaarecord www-ipv6 -o yaml
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

### Record not resolving

1. Check record status:
   ```bash
   kubectl get aaaarecord www-ipv6 -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}'
   ```

2. Verify zone exists:
   ```bash
   kubectl get dnszone example-com
   ```

3. Test DNS resolution:
   ```bash
   dig AAAA www.example.com @<dns-server-ip>
   ```

### Invalid IPv6 address

The controller validates IPv6 addresses. Ensure your address is in valid format:
- Use compressed notation: `2001:db8::1`
- Do not mix uppercase/lowercase unnecessarily
- Ensure all segments are valid hexadecimal

## Next Steps

- [DNS Records Overview](./records-guide.md) - Complete guide to all record types
- [MX Records](./mx-records.md) - Mail exchange records
- [TXT Records](./txt-records.md) - Text records for SPF, DKIM, etc.
- [Monitoring DNS](../operations/monitoring.md) - Monitor your DNS infrastructure
