# Replication

Implement multi-region DNS replication strategies for global availability.

## Replication Models

### Hub-and-Spoke

One central primary, multiple regional secondaries:

```
        Primary (us-east-1)
             │
      ┌──────┼──────┐
      ▼      ▼      ▼
  Secondary Secondary Secondary
  (us-west) (eu-west) (ap-south)
```

**Pros:** Simple, clear source of truth
**Cons:** Single point of failure, latency for distant regions

### Multi-Primary

Multiple primaries in different regions:

```
Primary A ◀─────▶ Primary B
(us-east)        (eu-west)
    │                │
    ▼                ▼
Secondary        Secondary
(us-west)        (ap-south)
```

**Pros:** Regional updates, better latency
**Cons:** Complex synchronization, conflict resolution

### Hierarchical

Tiered replication structure:

```
      Global Primary
           │
    ┌──────┼──────┐
    ▼      ▼      ▼
Regional   Regional  Regional
Primary    Primary   Primary
    │         │         │
    ▼         ▼         ▼
  Local     Local     Local
Secondary Secondary Secondary
```

**Pros:** Scales well, reduces global load
**Cons:** More complex, longer propagation time

## Configuration Examples

### Hub-and-Spoke Setup

```yaml
# Central Primary (us-east-1)
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: global-primary
  labels:
    dns-role: primary
    region: us-east-1
spec:
  replicas: 3
  config:
    allowTransfer:
      - "10.0.0.0/8"  # Allow all regional networks
---
# Regional Secondaries
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-us-west
  labels:
    dns-role: secondary
    region: us-west-2
spec:
  replicas: 2
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-eu-west
  labels:
    dns-role: secondary
    region: eu-west-1
spec:
  replicas: 2
```

## Replication Latency

### Measuring Propagation Time

```bash
# Update record on primary
kubectl apply -f new-record.yaml

# Check serial on primary
PRIMARY_SERIAL=$(kubectl exec -n dns-system deployment/global-primary -- \
  dig @localhost example.com SOA +short | awk '{print $3}')

# Wait and check secondary
SECONDARY_SERIAL=$(kubectl exec -n dns-system deployment/secondary-eu-west -- \
  dig @localhost example.com SOA +short | awk '{print $3}')

# Calculate lag
echo "Primary: $PRIMARY_SERIAL, Secondary: $SECONDARY_SERIAL"
```

### Optimizing Propagation

1. **Reduce refresh interval** - More frequent checks
2. **Enable NOTIFY** - Immediate notification of changes
3. **Use IXFR** - Faster incremental transfers
4. **Optimize network** - Low-latency connections between regions

## Conflict Resolution

When using multi-primary setups, handle conflicts:

### Prevention
- Separate zones per primary
- Use different subdomains per region
- Implement locking mechanism

### Detection
```bash
# Compare zones between primaries
diff <(kubectl exec deployment/primary-us -- cat /var/lib/bind/zones/example.com.zone) \
     <(kubectl exec deployment/primary-eu -- cat /var/lib/bind/zones/example.com.zone)
```

## Monitoring Replication

### Replication Dashboard

Monitor:
- Serial number sync status
- Replication lag per region
- Transfer success/failure rate
- Zone size and growth

### Alerts

Set up alerts for:
- Serial number drift > threshold
- Failed zone transfers
- Replication lag > SLA
- Network connectivity issues

## Best Practices

1. **Document topology** - Clear replication map
2. **Monitor lag** - Track propagation time
3. **Test failover** - Regular DR drills
4. **Use consistent serials** - YYYYMMDDnn format
5. **Automate updates** - GitOps for all regions
6. **Capacity planning** - Account for replication traffic

## Next Steps

- [High Availability](./ha.md) - HA architecture
- [Zone Transfers](./zone-transfers.md) - Transfer configuration
- [Performance](./performance.md) - Optimize replication performance
