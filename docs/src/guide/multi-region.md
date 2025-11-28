# Multi-Region Setup

Distribute your DNS infrastructure across multiple regions or availability zones for maximum availability and performance.

## Architecture Overview

A multi-region DNS setup typically includes:

- **Primary instances** in one or more regions
- **Secondary instances** in multiple geographic locations
- **Zone distribution** across all instances using label selectors

## Creating Regional Instances

### Primary in Region 1

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-us-east
  namespace: dns-system
  labels:
    dns-role: primary
    region: us-east-1
    environment: production
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"
    dnssec:
      enabled: true
```

### Secondary in Region 2

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-us-west
  namespace: dns-system
  labels:
    dns-role: secondary
    region: us-west-2
    environment: production
spec:
  replicas: 1
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

### Secondary in Region 3

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-eu-west
  namespace: dns-system
  labels:
    dns-role: secondary
    region: eu-west-1
    environment: production
spec:
  replicas: 1
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

## Distributing Zones Across Regions

Create zones that target all regions:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  type: primary
  instanceSelector:
    matchExpressions:
      - key: environment
        operator: In
        values:
          - production
      - key: dns-role
        operator: In
        values:
          - primary
          - secondary
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin@example.com
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
```

This zone will be deployed to all instances matching the selector (all production primary and secondary instances).

## Deployment Strategy

### Option 1: Primary-Secondary Model

- One region hosts primary instances
- All other regions host secondary instances
- Zone transfers flow from primary to secondaries

```
 Region 1 (us-east-1)         Region 2 (us-west-2)        Region 3 (eu-west-1)
┌─────────────────────┐      ┌─────────────────────┐     ┌─────────────────────┐
│  Primary Instances  │──────│ Secondary Instances │─────│ Secondary Instances │
│   (Master zones)    │      │  (Slave zones)      │     │  (Slave zones)      │
└─────────────────────┘      └─────────────────────┘     └─────────────────────┘
```

### Option 2: Multi-Primary Model

- Multiple regions host primary instances
- Different zones can have primaries in different regions
- Use careful labeling to route zones to appropriate primaries

## Network Considerations

### Zone Transfer Network

Ensure network connectivity for zone transfers:

- Primaries must reach secondaries on TCP port 53
- Use VPN, peering, or allow public transfer with IP restrictions

### Client Query Routing

Use one of:

- **GeoDNS** - Route clients to nearest regional instance
- **Anycast** - Same IP announced from multiple locations
- **Load Balancer** - Distribute across regional endpoints

## Failover Strategy

### Automatic Failover

Kubernetes handles pod-level failures automatically:

```yaml
spec:
  replicas: 2  # Multiple replicas for pod-level HA
```

### Regional Failover

For regional failures:

1. Clients automatically query secondary instances in other regions
2. Zone data remains available via zone transfers
3. Updates queue until primary region recovers

### Manual Failover

To manually promote a secondary to primary:

1. Update DNSZone to change primary servers
2. Update instance labels if needed
3. Verify zone transfers are working correctly

## Monitoring Multi-Region Setup

Check instance distribution:

```bash
# View all instances and their regions
kubectl get bind9instances -n dns-system -L region

# Check zone distribution
kubectl describe dnszone example-com -n dns-system
```

Monitor zone transfers:

```bash
# Check transfer logs on secondaries
kubectl logs -n dns-system -l dns-role=secondary | grep "transfer of"
```

## Best Practices

1. **Use Odd Number of Regions**: 3 or 5 regions for better quorum
2. **Distribute Replicas**: Spread replicas across availability zones
3. **Monitor Latency**: Watch zone transfer times between regions
4. **Test Failover**: Regularly test regional failover scenarios
5. **Automate Updates**: Use GitOps for consistent multi-region deployments

## Next Steps

- [Configure Monitoring](../operations/monitoring.md) for multi-region health
- [Set Up DNSSEC](./zone-config.md) across all regions
- [Implement Disaster Recovery](../operations/troubleshooting.md) procedures
