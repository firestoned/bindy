# SRV Records (Service Location)

SRV records specify the location of services, including hostname and port number. They're used for service discovery in protocols like SIP, XMPP, LDAP, and Minecraft.

## Creating an SRV Record

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: xmpp-server
  namespace: dns-system
spec:
  zoneRef: example-com  # References DNSZone metadata.name (recommended)
  service: xmpp-client  # Service name (without leading underscore)
  proto: tcp            # Protocol: tcp or udp
  name: "@"             # Domain (use @ for zone apex)
  priority: 10
  weight: 50
  port: 5222
  target: xmpp.example.com.  # Must end with dot (FQDN)
  ttl: 3600
```

This creates `_xmpp-client._tcp.example.com` pointing to `xmpp.example.com:5222`.

**Note:** You can also use `zone: example.com` (matching `DNSZone.spec.zoneName`) instead of `zoneRef`. See [Referencing DNS Zones](./records-guide.md#referencing-dns-zones) for details.

## SRV Record Format

The DNS name format is: `_service._proto.name.domain`

- **service**: Service name (e.g., `xmpp-client`, `sip`, `ldap`)
- **proto**: Protocol (`tcp` or `udp`)
- **name**: Subdomain or `@` for zone apex
- **priority**: Lower values are preferred (like MX records)
- **weight**: For load balancing among equal priorities (0-65535)
- **port**: Service port number
- **target**: Hostname providing the service (FQDN with trailing dot)

## Common Services

### XMPP (Jabber)

```yaml
# Client connections
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: xmpp-client
spec:
  zoneRef: example-com
  service: xmpp-client
  proto: tcp
  name: "@"
  priority: 5
  weight: 0
  port: 5222
  target: xmpp.example.com.
---
# Server-to-server
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: xmpp-server
spec:
  zoneRef: example-com
  service: xmpp-server
  proto: tcp
  name: "@"
  priority: 5
  weight: 0
  port: 5269
  target: xmpp.example.com.
```

### SIP (VoIP)

```yaml
# SIP over TCP
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: sip-tcp
spec:
  zoneRef: example-com
  service: sip
  proto: tcp
  name: "@"
  priority: 10
  weight: 50
  port: 5060
  target: sip.example.com.
---
# SIP over UDP
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: sip-udp
spec:
  zoneRef: example-com
  service: sip
  proto: udp
  name: "@"
  priority: 10
  weight: 50
  port: 5060
  target: sip.example.com.
```

### LDAP

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: ldap-service
spec:
  zoneRef: example-com
  service: ldap
  proto: tcp
  name: "@"
  priority: 0
  weight: 100
  port: 389
  target: ldap.example.com.
```

### Minecraft Server

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: minecraft
spec:
  zoneRef: example-com
  service: minecraft
  proto: tcp
  name: "@"
  priority: 0
  weight: 5
  port: 25565
  target: mc.example.com.
```

## Priority and Weight

### Failover with Priority

```yaml
# Primary server (priority 10)
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: sip-primary
spec:
  zoneRef: example-com
  service: sip
  proto: tcp
  name: "@"
  priority: 10
  weight: 0
  port: 5060
  target: sip1.example.com.
---
# Backup server (priority 20)
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: sip-backup
spec:
  zoneRef: example-com
  service: sip
  proto: tcp
  name: "@"
  priority: 20
  weight: 0
  port: 5060
  target: sip2.example.com.
```

### Load Balancing with Weight

```yaml
# Server 1 (weight 70 = 70% of traffic)
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: srv-1
spec:
  zoneRef: example-com
  service: xmpp-client
  proto: tcp
  name: "@"
  priority: 10
  weight: 70
  port: 5222
  target: xmpp1.example.com.
---
# Server 2 (weight 30 = 30% of traffic)
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: srv-2
spec:
  zoneRef: example-com
  service: xmpp-client
  proto: tcp
  name: "@"
  priority: 10
  weight: 30
  port: 5222
  target: xmpp2.example.com.
```

## FQDN Requirement

**CRITICAL:** The `target` field **MUST** end with a dot (`.`):

```yaml
# ✅ CORRECT
target: server.example.com.

# ❌ WRONG
target: server.example.com
```

## Required Supporting Records

SRV records need corresponding A/AAAA records for targets:

```yaml
# SRV record
apiVersion: bindy.firestoned.io/v1alpha1
kind: SRVRecord
metadata:
  name: service-srv
spec:
  zoneRef: example-com
  service: myservice
  proto: tcp
  name: "@"
  priority: 10
  weight: 0
  port: 8080
  target: server.example.com.
---
# A record for target
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: server
spec:
  zoneRef: example-com
  name: server
  ipv4Address: "203.0.113.50"
```

## Best Practices

1. **Always use FQDNs** - End `target` values with a dot
2. **Multiple servers** - Use priority/weight for redundancy and load balancing
3. **Match protocols** - Create both TCP and UDP records if service supports both
4. **Test clients** - Verify client applications can discover services via SRV
5. **Document services** - Clearly name resources for maintainability

## Status Monitoring

```bash
kubectl get srvrecord xmpp-server -o yaml
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

### Test SRV record

```bash
# Query SRV record
dig SRV _xmpp-client._tcp.example.com

# Expected output shows priority, weight, port, and target
;; ANSWER SECTION:
_xmpp-client._tcp.example.com. 3600 IN SRV 5 0 5222 xmpp.example.com.
```

### Common Issues

- **Service not auto-discovered** - Verify client supports SRV lookups
- **Missing A/AAAA for target** - Target hostname must resolve
- **Wrong service/proto names** - Must match what client expects (check docs)

## Next Steps

- [A Records](./a-records.md) - Create records for SRV targets
- [DNS Records Overview](./records-guide.md) - Complete guide to all record types
- [Monitoring DNS](../operations/monitoring.md) - Monitor your DNS infrastructure
