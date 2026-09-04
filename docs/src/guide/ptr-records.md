# PTR Records (Reverse DNS)

PTR (Pointer) records map an IP address back to a canonical hostname. They live in reverse zones (`in-addr.arpa` for IPv4, `ip6.arpa` for IPv6) and are the reverse counterpart of A/AAAA records.

## Creating a PTR Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: PTRRecord
metadata:
  name: host-10-ptr
  namespace: bindy-system
  labels:
    zone: 0.168.192.in-addr.arpa  # Used by DNSZone selector
spec:
  name: "10"                      # Host portion within the reverse zone
  target: host10.example.com.     # Must end with dot (FQDN)
  ttl: 3600
```

This creates `10.0.168.192.in-addr.arpa. PTR host10.example.com.` — the reverse mapping for `192.168.0.10`.

## Reverse Zones

PTR records must be published in a reverse `DNSZone`. For the `192.168.0.0/24` network, the reverse zone is `0.168.192.in-addr.arpa` (octets reversed):

```yaml
# Reverse DNSZone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: 0-168-192-in-addr-arpa
  namespace: bindy-system
spec:
  zoneName: 0.168.192.in-addr.arpa
  clusterRef: production-dns
  recordsFrom:
    - selector:
        matchLabels:
          zone: 0.168.192.in-addr.arpa  # Selects all records with this label
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
---
# PTR record that will be selected
apiVersion: bindy.firestoned.io/v1beta1
kind: PTRRecord
metadata:
  name: host-10-ptr
  namespace: bindy-system
  labels:
    zone: 0.168.192.in-addr.arpa  # ✅ Matches selector above
spec:
  name: "10"
  target: host10.example.com.
```

For IPv6, the reverse zone uses nibbles of the address under `ip6.arpa` (e.g. `8.b.d.0.1.0.0.2.ip6.arpa` for `2001:db8::/32`).

See [Label Selector Guide](./label-selectors.md) for advanced patterns.

## Record Name Format

The `name` field is the host portion of the reversed address within the reverse zone:

| IP address | Reverse zone | `spec.name` |
| ---------- | ------------ | ----------- |
| `192.168.0.10` | `0.168.192.in-addr.arpa` | `"10"` |
| `192.168.0.10` | `168.192.in-addr.arpa` | `"10.0"` |
| `10.0.0.1` | `10.in-addr.arpa` | `"1.0.0"` |

**Note:** Quote numeric `name` values (`"10"`, not `10`) so YAML treats them as strings.

## FQDN Requirement

**CRITICAL:** The `target` field **MUST** end with a dot (`.`):

```yaml
# ✅ CORRECT
target: host10.example.com.

# ❌ WRONG
target: host10.example.com
```

## Common Use Cases

### Reverse records for forward A records

Keep PTR records in sync with your forward zone entries:

```yaml
# Forward: host10.example.com -> 192.168.0.10 (in the example.com zone)
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: host-10
  namespace: bindy-system
  labels:
    zone: example.com
spec:
  name: host10
  ipv4Addresses:
    - "192.168.0.10"
---
# Reverse: 192.168.0.10 -> host10.example.com. (in the reverse zone)
apiVersion: bindy.firestoned.io/v1beta1
kind: PTRRecord
metadata:
  name: host-10-ptr
  namespace: bindy-system
  labels:
    zone: 0.168.192.in-addr.arpa
spec:
  name: "10"
  target: host10.example.com.
```

### Mail server reverse DNS

Many mail providers require PTR records matching the SMTP banner hostname:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: PTRRecord
metadata:
  name: mail-ptr
  namespace: bindy-system
  labels:
    zone: 0.168.192.in-addr.arpa
spec:
  name: "25"
  target: mail.example.com.
```

## Best Practices

1. **Pair with A/AAAA records** - Every PTR record should have a matching forward record pointing back (forward-confirmed reverse DNS)
2. **FQDNs only** - Always end target values with a dot
3. **Quote numeric names** - Use `"10"` instead of `10` so the name stays a string
4. **One PTR per address** - Publish a single canonical hostname per IP address
5. **Consistent TTLs** - Match the TTL of the corresponding forward record

## Status Monitoring

```bash
kubectl get ptrrecord host-10-ptr -o yaml
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

### Test reverse resolution

```bash
# Query the PTR record directly
dig -x 192.168.0.10

# Verify against the reverse zone
dig PTR 10.0.168.192.in-addr.arpa
```

### Common Issues

- **Wrong reverse zone** - The octets of the network portion must be reversed (`192.168.0.0/24` → `0.168.192.in-addr.arpa`)
- **Wrong FQDN** - Missing trailing dot causes a relative name
- **Unquoted name** - `name: 10` (integer) fails schema validation; use `name: "10"`

## Next Steps

- [DNS Records Overview](./records-guide.md) - Complete guide to all record types
- [A Records](./a-records.md) - Create the matching forward records
- [Monitoring DNS](../operations/monitoring.md) - Monitor your DNS infrastructure
