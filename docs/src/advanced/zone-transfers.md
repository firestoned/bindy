# Zone Transfers

Configure and optimize DNS zone transfers between primary and secondary instances.

## Overview

Zone transfers replicate DNS zone data from primary to secondary servers using AXFR (full transfer) or IXFR (incremental transfer).

## Configuring Zone Transfers

### Primary Instance Setup

Allow zone transfers to secondary servers:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
spec:
  config:
    allowTransfer:
      - "10.0.0.0/8"        # Secondary network
      - "192.168.100.0/24"  # Specific secondary subnet
```

### Secondary Instance Setup

Configure secondary zones to transfer from primary:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com-secondary
spec:
  zoneName: example.com
  type: secondary
  instanceSelector:
    matchLabels:
      dns-role: secondary
  secondaryConfig:
    primaryServers:
      - "10.0.1.10"  # Primary DNS server IP
      - "10.0.1.11"  # Backup primary IP
```

## Transfer Types

### Full Transfer (AXFR)

Transfers entire zone:

- Used for initial zone load
- Triggered manually or when IXFR unavailable
- More bandwidth intensive

### Incremental Transfer (IXFR)

Transfers only changes since last serial:

- More efficient for large zones
- Requires serial number tracking
- Automatically used when available

## Transfer Triggers

### NOTIFY Messages

Primary sends NOTIFY when zone changes:

```
Primary Updates Zone
      │
      ├──NOTIFY──▶ Secondary 1
      ├──NOTIFY──▶ Secondary 2
      └──NOTIFY──▶ Secondary 3
      
Secondaries initiate IXFR/AXFR
```

### Refresh Timer

Secondary checks for updates periodically:

```yaml
soaRecord:
  refresh: 3600  # Check every hour
  retry: 600     # Retry after 10 minutes if failed
```

### Manual Trigger

Force zone transfer:

```bash
# On secondary pod
kubectl exec -n dns-system deployment/secondary-dns -- \
  rndc retransfer example.com
```

## Monitoring Zone Transfers

### Check Transfer Status

```bash
# View transfer logs
kubectl logs -n dns-system -l dns-role=secondary | grep "transfer of"

# Successful transfer
# transfer of 'example.com/IN' from 10.0.1.10#53: Transfer completed: 1 messages, 42 records

# Check zone status
kubectl exec -n dns-system deployment/secondary-dns -- \
  rndc zonestatus example.com
```

### Verify Serial Numbers

```bash
# Primary serial
kubectl exec -n dns-system deployment/primary-dns -- \
  dig @localhost example.com SOA +short | awk '{print $3}'

# Secondary serial  
kubectl exec -n dns-system deployment/secondary-dns -- \
  dig @localhost example.com SOA +short | awk '{print $3}'

# Should match when in sync
```

## Transfer Performance

### Optimize Transfer Speed

1. **Use IXFR** - Only transfer changes
2. **Increase Bandwidth** - Adequate network resources
3. **Compress Transfers** - Enable BIND9 compression
4. **Parallel Transfers** - Multiple zones transfer concurrently

### Transfer Limits

Configure maximum concurrent transfers:

```yaml
# In BIND9 config (future enhancement)
options {
  transfers-in 10;   # Max incoming transfers
  transfers-out 10;  # Max outgoing transfers
};
```

## Security

### Access Control

Restrict transfers by IP:

```yaml
spec:
  config:
    allowTransfer:
      - "10.0.0.0/8"  # Only this network
```

### TSIG Authentication

Use TSIG keys for authenticated transfers:

```yaml
# 1. Create a Kubernetes Secret with RNDC/TSIG credentials
apiVersion: v1
kind: Secret
metadata:
  name: transfer-key-secret
  namespace: dns-system
type: Opaque
stringData:
  key-name: transfer-key
  secret: K2xkajflkajsdf09asdfjlaksjdf==  # base64-encoded HMAC key

---
# 2. Reference the secret in Bind9Cluster
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  rndcSecretRefs:
    - name: transfer-key-secret
      algorithm: hmac-sha256  # Algorithm for this key
```

The secret will be used for authenticated zone transfers between primary and secondary servers.

## Troubleshooting

### Transfer Failures

**Check network connectivity:**
```bash
kubectl exec -n dns-system deployment/secondary-dns -- \
  nc -zv primary-dns-service 53
```

**Test manual transfer:**
```bash
kubectl exec -n dns-system deployment/secondary-dns -- \
  dig @primary-dns-service example.com AXFR
```

**Check ACLs:**
```bash
kubectl get bind9instance primary-dns -o jsonpath='{.spec.config.allowTransfer}'
```

### Slow Transfers

**Check zone size:**
```bash
kubectl exec -n dns-system deployment/primary-dns -- \
  wc -l /var/lib/bind/zones/example.com.zone
```

**Monitor transfer time:**
```bash
kubectl logs -n dns-system -l dns-role=secondary | \
  grep "transfer of" | grep "msecs"
```

### Transfer Lag

**Check refresh interval:**
```bash
kubectl get dnszone example-com -o jsonpath='{.spec.soaRecord.refresh}'
```

**Force immediate transfer:**
```bash
kubectl exec -n dns-system deployment/secondary-dns -- \
  rndc retransfer example.com
```

## Best Practices

1. **Use IXFR** - More efficient than full transfers
2. **Set Appropriate Refresh** - Balance freshness vs load
3. **Monitor Serial Numbers** - Detect sync issues
4. **Secure Transfers** - Use ACLs and TSIG
5. **Test Failover** - Verify secondaries work when primary fails
6. **Log Transfers** - Monitor for failures
7. **Geographic Distribution** - Secondaries in different regions

## Example: Complete Setup

```yaml
# Primary Instance
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  labels:
    dns-role: primary
spec:
  replicas: 2
  config:
    allowTransfer:
      - "10.0.0.0/8"
---
# Primary Zone
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com-primary
spec:
  zoneName: example.com
  type: primary
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin@example.com
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
---
# Secondary Instance  
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-dns
  labels:
    dns-role: secondary
spec:
  replicas: 2
---
# Secondary Zone
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com-secondary
spec:
  zoneName: example.com
  type: secondary
  instanceSelector:
    matchLabels:
      dns-role: secondary
  secondaryConfig:
    primaryServers:
      - "primary-dns-service.dns-system.svc.cluster.local"
```

## Next Steps

- [Replication](./replication.md) - Multi-region replication strategies
- [High Availability](./ha.md) - HA architecture
- [Performance](./performance.md) - Optimize zone transfer performance
