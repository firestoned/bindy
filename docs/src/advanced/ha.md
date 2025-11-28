# High Availability

Design and implement highly available DNS infrastructure with Bindy.

## Overview

High availability (HA) DNS ensures continuous DNS service even during:
- Pod failures
- Node failures
- Availability zone outages
- Regional outages
- Planned maintenance

## HA Architecture Components

### 1. Multiple Replicas

Run multiple replicas of each Bind9Instance:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
spec:
  replicas: 3  # Multiple replicas for pod-level HA
```

**Benefits:**
- Survives pod crashes
- Load distribution
- Zero-downtime updates

### 2. Multiple Instances

Deploy separate primary and secondary instances:

```yaml
# Primary instance
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  labels:
    dns-role: primary
spec:
  replicas: 2
---
# Secondary instance  
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-dns
  labels:
    dns-role: secondary
spec:
  replicas: 2
```

**Benefits:**
- Role separation
- Independent scaling
- Failover capability

### 3. Geographic Distribution

Deploy instances across multiple regions:

```yaml
# US East primary
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-us-east
  labels:
    dns-role: primary
    region: us-east-1
spec:
  replicas: 2
---
# US West secondary
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
# EU secondary
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

**Benefits:**
- Regional failure tolerance
- Lower latency for global users
- Regulatory compliance (data locality)

## HA Patterns

### Pattern 1: Active-Passive

One active primary, multiple passive secondaries:

```
┌──────────────┐         ┌──────────────┐         ┌──────────────┐
│   Primary    │────────▶│  Secondary   │────────▶│  Secondary   │
│  (Active)    │  AXFR   │  (Passive)   │  AXFR   │  (Passive)   │
│  us-east-1   │         │  us-west-2   │         │  eu-west-1   │
└──────────────┘         └──────────────┘         └──────────────┘
        │                        │                        │
        └────────────────────────┴────────────────────────┘
                        Clients query any
```

- Updates go to primary only
- Secondaries receive via zone transfer
- Clients query any available instance

### Pattern 2: Multi-Primary

Multiple primaries in different regions:

```
┌──────────────┐         ┌──────────────┐
│   Primary    │◀───────▶│   Primary    │
│  (zone-a)    │  Sync   │  (zone-b)    │
│  us-east-1   │         │  eu-west-1   │
└──────────────┘         └──────────────┘
```

- Different zones on different primaries
- Geographic distribution of updates
- Careful coordination required

### Pattern 3: Anycast

Same IP announced from multiple locations:

```
        Client Query (192.0.2.53)
                 │
         ┌───────┼───────┐
         ▼       ▼       ▼
      ┌────┐  ┌────┐  ┌────┐
      │DNS │  │DNS │  │DNS │
      │US  │  │EU  │  │APAC│
      └────┘  └────┘  └────┘
```

- Requires BGP routing
- Lowest latency routing
- Automatic failover

## Pod-Level HA

### Anti-Affinity

Spread pods across nodes:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: primary-dns
spec:
  replicas: 3
  template:
    spec:
      affinity:
        podAntiAffinity:
          preferredDuringSchedulingIgnoredDuringExecution:
          - weight: 100
            podAffinityTerm:
              labelSelector:
                matchExpressions:
                - key: instance
                  operator: In
                  values:
                  - primary-dns
              topologyKey: kubernetes.io/hostname
```

### Topology Spread

Distribute across availability zones:

```yaml
spec:
  topologySpreadConstraints:
  - maxSkew: 1
    topologyKey: topology.kubernetes.io/zone
    whenUnsatisfiable: DoNotSchedule
    labelSelector:
      matchLabels:
        instance: primary-dns
```

## Service-Level HA

### Liveness and Readiness Probes

Ensure only healthy pods serve traffic:

```yaml
livenessProbe:
  exec:
    command: ["dig", "@localhost", "version.bind", "txt", "chaos"]
  initialDelaySeconds: 30
  periodSeconds: 10
  
readinessProbe:
  exec:
    command: ["dig", "@localhost", "version.bind", "txt", "chaos"]
  initialDelaySeconds: 5
  periodSeconds: 5
```

### Pod Disruption Budgets

Limit concurrent disruptions:

```yaml
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: primary-dns-pdb
spec:
  minAvailable: 2
  selector:
    matchLabels:
      instance: primary-dns
```

## Monitoring HA

### Check Instance Distribution

```bash
# View instances across regions
kubectl get bind9instances -A -L region

# View pod distribution
kubectl get pods -n dns-system -o wide

# Check zone spread
kubectl get pods -n dns-system \
  -o custom-columns=NAME:.metadata.name,NODE:.spec.nodeName,ZONE:.spec.nodeSelector
```

### Test Failover

```bash
# Simulate pod failure
kubectl delete pod -n dns-system <pod-name>

# Verify automatic recovery
kubectl get pods -n dns-system -w

# Test DNS during failover
while true; do dig @$SERVICE_IP example.com +short; sleep 1; done
```

## Disaster Recovery

### Backup Strategy

```bash
# Regular backups of all CRDs
kubectl get bind9instances,dnszones,arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords \
  -A -o yaml > backup-$(date +%Y%m%d).yaml
```

### Recovery Procedures

1. **Single Pod Failure** - Kubernetes automatically recreates
2. **Instance Failure** - Clients fail over to other instances
3. **Regional Failure** - Zone data available from other regions
4. **Complete Loss** - Restore from backup

```bash
# Restore from backup
kubectl apply -f backup-20241126.yaml
```

## Best Practices

1. **Run 3+ Replicas** - Odd numbers for quorum
2. **Multi-AZ Deployment** - Spread across availability zones
3. **Geographic Redundancy** - At least 2 regions for critical zones
4. **Monitor Continuously** - Alert on degraded HA
5. **Test Failover** - Regular disaster recovery drills
6. **Automate Recovery** - Use Kubernetes self-healing
7. **Document Procedures** - Runbooks for incidents

## Next Steps

- [Zone Transfers](./zone-transfers.md) - Configure zone replication
- [Replication](./replication.md) - Multi-region replication strategies
- [Performance](./performance.md) - Optimize for high availability
