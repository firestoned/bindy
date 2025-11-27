# Access Control

Configure fine-grained access control for DNS queries and zone transfers.

## Query Access Control

Restrict who can query your DNS servers:

### Public DNS (Allow All)

```yaml
spec:
  config:
    allowQuery:
      - "0.0.0.0/0"  # IPv4 - anyone
      - "::/0"       # IPv6 - anyone
```

### Internal DNS (Restricted)

```yaml
spec:
  config:
    allowQuery:
      - "10.0.0.0/8"      # RFC1918 private
      - "172.16.0.0/12"   # RFC1918 private
      - "192.168.0.0/16"  # RFC1918 private
```

### Specific Networks

```yaml
spec:
  config:
    allowQuery:
      - "192.168.1.0/24"   # Office network
      - "10.100.0.0/16"    # VPN network
      - "172.20.5.10"      # Specific host
```

## Zone Transfer Access Control

Restrict zone transfers to authorized servers:

```yaml
spec:
  config:
    allowTransfer:
      - "10.0.1.0/24"      # Secondary DNS subnet
      - "192.168.100.5"    # Specific secondary
      - "192.168.100.6"    # Another secondary
```

### Block All Transfers

```yaml
spec:
  config:
    allowTransfer: []  # No transfers allowed
```

## ACL Best Practices

1. **Default Deny** - Start restrictive, open as needed
2. **Use CIDR Blocks** - More maintainable than individual IPs
3. **Document ACLs** - Note why each entry exists
4. **Regular Review** - Remove obsolete entries
5. **Test Changes** - Verify before production

## Network Policies

Kubernetes NetworkPolicies add another layer:

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: dns-ingress
  namespace: dns-system
spec:
  podSelector:
    matchLabels:
      app: bind9
  policyTypes:
  - Ingress
  ingress:
  - from:
    - namespaceSelector: {}  # Allow from all namespaces
    ports:
    - protocol: UDP
      port: 53
    - protocol: TCP
      port: 53
```

## Testing Access Control

```bash
# From allowed network (should work)
dig @$SERVICE_IP example.com

# From blocked network (should timeout or refuse)
dig @$SERVICE_IP example.com
# ;; communications error: connection timed out

# Test zone transfer restriction
dig @$SERVICE_IP example.com AXFR
# Transfer should fail if not in allowTransfer list
```

## Next Steps

- [Security](./security.md) - Overall security
- [DNSSEC](./dnssec.md) - Cryptographic validation
