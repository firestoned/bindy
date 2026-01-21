# Operator High Availability

This guide covers deploying and operating the Bindy operator in high availability (HA) mode with leader election.

## Overview

Running multiple operator instances with leader election ensures:
- **Continuous operation** - If one operator fails, another takes over
- **Zero downtime** - Failover typically completes in ~15 seconds
- **Automatic recovery** - No manual intervention required
- **Production reliability** - Recommended for all production deployments

## Quick Start

Deploy 3 operator replicas with leader election:

```bash
# Update deployment to 3 replicas
kubectl scale deployment -n dns-system bindy --replicas=3

# Verify all replicas are running
kubectl get pods -n dns-system -l app=bindy

# Check which instance is the leader
kubectl get lease -n dns-system bindy-leader -o jsonpath='{.spec.holderIdentity}'
```

## Architecture

### Leader Election Model

```
┌──────────────┐         ┌──────────────┐         ┌──────────────┐
│  Operator    │         │  Operator    │         │  Operator    │
│  Instance 1  │         │  Instance 2  │         │  Instance 3  │
│   (Leader)   │         │  (Standby)   │         │  (Standby)   │
└──────┬───────┘         └──────┬───────┘         └──────┬───────┘
       │                        │                        │
       └────────────────────────┼────────────────────────┘
                                │
                        ┌───────▼────────┐
                        │ Kubernetes API │
                        │  Lease Object  │
                        │ (coordination) │
                        └────────────────┘
```

### How It Works

1. **All instances start** and attempt to acquire the lease
2. **One instance succeeds** and becomes the leader
3. **Leader starts reconciliation** of all resources
4. **Standby instances wait** and monitor the lease
5. **Leader renews lease** every 2 seconds (default)
6. **If leader fails**, standby instances detect it within ~15 seconds
7. **New leader elected** automatically from standby instances

## Configuration

### Environment Variables

Configure leader election behavior via environment variables:

| Variable | Default | Description | Recommended |
|----------|---------|-------------|-------------|
| `ENABLE_LEADER_ELECTION` | `true` | Enable/disable leader election | `true` (always) |
| `LEASE_NAME` | `bindy-leader` | Name of the Lease resource | `bindy-leader` |
| `LEASE_NAMESPACE` | `dns-system` | Namespace for Lease | Match operator namespace |
| `LEASE_DURATION_SECONDS` | `15` | How long leader holds lease | `15` (production) |
| `LEASE_RENEW_DEADLINE_SECONDS` | `10` | Leader must renew before this | `10` |
| `LEASE_RETRY_PERIOD_SECONDS` | `2` | Attempt acquisition frequency | `2` |
| `POD_NAME` | `$HOSTNAME` | Unique identity | Use `metadata.name` |

### Deployment Configuration

Example deployment with HA configuration:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: bindy
  namespace: dns-system
spec:
  replicas: 3  # Run 3 instances for HA
  selector:
    matchLabels:
      app: bindy
  template:
    metadata:
      labels:
        app: bindy
    spec:
      serviceAccountName: bindy
      # Spread pods across nodes
      affinity:
        podAntiAffinity:
          preferredDuringSchedulingIgnoredDuringExecution:
          - weight: 100
            podAffinityTerm:
              labelSelector:
                matchExpressions:
                - key: app
                  operator: In
                  values:
                  - bindy
              topologyKey: kubernetes.io/hostname
      containers:
      - name: operator
        image: ghcr.io/firestoned/bindy:latest
        env:
        # Leader election configuration
        - name: ENABLE_LEADER_ELECTION
          value: "true"
        - name: LEASE_NAME
          value: "bindy-leader"
        - name: LEASE_NAMESPACE
          value: "dns-system"
        - name: LEASE_DURATION_SECONDS
          value: "15"
        - name: LEASE_RENEW_DEADLINE_SECONDS
          value: "10"
        - name: LEASE_RETRY_PERIOD_SECONDS
          value: "2"
        - name: POD_NAME
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        resources:
          requests:
            cpu: 100m
            memory: 128Mi
          limits:
            cpu: 500m
            memory: 512Mi
```

### RBAC Requirements

Leader election requires `coordination.k8s.io/leases` permissions:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: bindy
  namespace: dns-system
rules:
# Leases for leader election (required)
- apiGroups: ["coordination.k8s.io"]
  resources: ["leases"]
  verbs: ["get", "create", "update", "patch"]
```

## Monitoring

### Check Current Leader

View which operator instance is currently the leader:

```bash
# Get leader identity
kubectl get lease -n dns-system bindy-leader \
  -o jsonpath='{.spec.holderIdentity}'

# Output: bindy-7d8f9c5b4d-x7k2m

# Verify that pod is running
kubectl get pod -n dns-system bindy-7d8f9c5b4d-x7k2m
```

### View Lease Details

Inspect the full lease object:

```bash
kubectl get lease -n dns-system bindy-leader -o yaml
```

Output:
```yaml
apiVersion: coordination.k8s.io/v1
kind: Lease
metadata:
  name: bindy-leader
  namespace: dns-system
spec:
  acquireTime: "2025-11-30T12:34:56Z"
  holderIdentity: bindy-7d8f9c5b4d-x7k2m
  leaseDurationSeconds: 15
  renewTime: "2025-11-30T12:35:10Z"
```

### Monitor Leadership Changes

Watch for leadership transitions:

```bash
# Watch lease changes
kubectl get lease -n dns-system bindy-leader -w

# Watch operator logs for leadership events
kubectl logs -n dns-system deployment/bindy -f | grep -i "leader\|lease"
```

### Leader Election Metrics

Key log messages indicating leader election status:

| Log Message | Meaning |
|-------------|---------|
| `Attempting to acquire lease bindy-leader` | Instance trying to become leader |
| `Lease acquired, this instance is now the leader` | Instance became leader |
| `Starting all operators` | Leader starting reconciliation |
| `Leadership lost! Stopping all operators...` | Leader detected lease loss |
| `Lease already held by <pod-name>` | Another instance is leader |

### Prometheus Metrics

Monitor leader election health (if metrics enabled):

```promql
# Number of leadership changes (should be low)
increase(bindy_leader_elections_total[1h])

# Time since last leadership change
time() - bindy_leader_election_timestamp

# Current leader status (1 = leader, 0 = standby)
bindy_is_leader
```

## Testing Failover

### Simulated Leader Failure

Test automatic failover by deleting the leader pod:

```bash
# 1. Find current leader
LEADER=$(kubectl get lease -n dns-system bindy-leader \
  -o jsonpath='{.spec.holderIdentity}')
echo "Current leader: $LEADER"

# 2. Delete leader pod
kubectl delete pod -n dns-system $LEADER

# 3. Watch for new leader (typically 10-20 seconds)
watch kubectl get lease -n dns-system bindy-leader

# 4. Verify DNS operations continue
kubectl get bind9instances -A
kubectl get dnszones -A
```

Expected behavior:
- Leader pod terminates
- Within ~15 seconds, standby instance acquires lease
- New leader starts reconciliation
- All DNS operations continue without user intervention

### Network Partition Test

Simulate network partition using NetworkPolicy:

```bash
# Block leader from API server
kubectl apply -f - <<EOF
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: block-leader
  namespace: dns-system
spec:
  podSelector:
    matchLabels:
      statefulset.kubernetes.io/pod-name: $LEADER
  policyTypes:
  - Egress
  egress: []  # Block all egress
EOF

# Wait for lease expiration (~15 seconds)
sleep 20

# Verify new leader elected
kubectl get lease -n dns-system bindy-leader

# Cleanup
kubectl delete networkpolicy -n dns-system block-leader
```

## Troubleshooting

### No Leader Elected

**Symptom:** No operator instance becomes leader, resources not reconciling

**Check:**
```bash
# Verify lease exists
kubectl get lease -n dns-system bindy-leader

# Check operator logs
kubectl logs -n dns-system deployment/bindy --all-containers=true

# Verify RBAC permissions
kubectl auth can-i get leases \
  --namespace=dns-system \
  --as=system:serviceaccount:dns-system:bindy
```

**Common causes:**
- Missing `coordination.k8s.io/leases` RBAC permissions
- Network issues preventing API server access
- `ENABLE_LEADER_ELECTION=false` set incorrectly

**Fix:**
```bash
# Ensure RBAC includes leases
kubectl apply -f deploy/rbac/role.yaml

# Restart operators
kubectl rollout restart deployment -n dns-system bindy
```

### Multiple Leaders (Split Brain)

**Symptom:** Multiple operators reconciling simultaneously, conflicts in logs

**This should NEVER happen** with proper leader election. If it does:

```bash
# Check if all operators use the same LEASE_NAME
kubectl get deployment -n dns-system bindy -o yaml | grep LEASE_NAME

# Delete and recreate lease to force re-election
kubectl delete lease -n dns-system bindy-leader

# Watch for single leader election
kubectl get lease -n dns-system bindy-leader -w
```

**Verify:**
```bash
# All operators should show the same lease holder
kubectl logs -n dns-system deployment/bindy --all-containers=true \
  | grep "holderIdentity"
```

### Frequent Leadership Changes

**Symptom:** Leader changes every few minutes, instability

**Check:**
```bash
# Monitor lease renewals
kubectl get lease -n dns-system bindy-leader -w

# Check operator resource usage (may be OOMKilled)
kubectl top pods -n dns-system -l app=bindy

# Check operator logs for crashes
kubectl logs -n dns-system deployment/bindy --previous
```

**Common causes:**
- Operator pods being OOMKilled (increase memory limits)
- Network latency to API server
- Node instability
- Too aggressive `LEASE_DURATION_SECONDS`

**Fix:**
```bash
# Increase lease duration for unstable environments
kubectl set env deployment/bindy \
  -n dns-system \
  LEASE_DURATION_SECONDS=30 \
  LEASE_RENEW_DEADLINE_SECONDS=20
```

### Leader Not Reconciling

**Symptom:** Leader elected but resources not reconciling

**Check:**
```bash
# Verify leader pod is running
LEADER=$(kubectl get lease -n dns-system bindy-leader \
  -o jsonpath='{.spec.holderIdentity}')
kubectl get pod -n dns-system $LEADER

# Check leader logs
kubectl logs -n dns-system $LEADER -f

# Look for operator startup messages
kubectl logs -n dns-system $LEADER | grep "Starting.*operator"
```

**Common causes:**
- Leader pod stuck in initialization
- Operator panic after acquiring leadership
- Resource limits preventing reconciliation

### Operator Disabled Leader Election

**Symptom:** `ENABLE_LEADER_ELECTION=false` but multiple replicas running

**This will cause conflicts!** Either:

**Option 1: Enable leader election (recommended)**
```bash
kubectl set env deployment/bindy \
  -n dns-system \
  ENABLE_LEADER_ELECTION=true
```

**Option 2: Scale to single replica**
```bash
kubectl scale deployment -n dns-system bindy --replicas=1
```

## Best Practices

### Production Deployment

1. **Always enable leader election** - Set `ENABLE_LEADER_ELECTION=true`
2. **Run 3 replicas** - Provides redundancy with minimal overhead
3. **Use pod anti-affinity** - Spread pods across nodes
4. **Set resource limits** - Prevent resource starvation
5. **Monitor lease health** - Alert on frequent leadership changes
6. **Test failover regularly** - Validate HA configuration works

### Recommended Configuration

```yaml
spec:
  replicas: 3  # Optimal for most deployments

  # Spread across nodes
  affinity:
    podAntiAffinity:
      preferredDuringSchedulingIgnoredDuringExecution:
      - weight: 100
        podAffinityTerm:
          labelSelector:
            matchLabels:
              app: bindy
          topologyKey: kubernetes.io/hostname

  containers:
  - name: operator
    env:
    # Production settings
    - name: ENABLE_LEADER_ELECTION
      value: "true"
    - name: LEASE_DURATION_SECONDS
      value: "15"
    - name: LEASE_RENEW_DEADLINE_SECONDS
      value: "10"
    - name: LEASE_RETRY_PERIOD_SECONDS
      value: "2"

    # Production resource limits
    resources:
      requests:
        cpu: 100m
        memory: 128Mi
      limits:
        cpu: 500m
        memory: 512Mi
```

### Multi-AZ Deployment

For cloud environments, spread across availability zones:

```yaml
spec:
  replicas: 3

  topologySpreadConstraints:
  - maxSkew: 1
    topologyKey: topology.kubernetes.io/zone
    whenUnsatisfiable: DoNotSchedule
    labelSelector:
      matchLabels:
        app: bindy
```

### Cost Optimization

For development/staging with HA:

```yaml
spec:
  replicas: 2  # Minimal HA

  containers:
  - name: operator
    env:
    - name: LEASE_DURATION_SECONDS
      value: "30"  # Longer duration = less API calls

    resources:
      requests:
        cpu: 50m      # Lower for non-production
        memory: 64Mi
      limits:
        cpu: 200m
        memory: 256Mi
```

## Performance Impact

Leader election overhead:

| Metric | Impact |
|--------|--------|
| **CPU** | <1% increase per standby instance |
| **Memory** | <5MB increase per instance |
| **Network** | 1 API call every 2 seconds (leader renews lease) |
| **Failover time** | ~15 seconds (configurable) |
| **Reconciliation** | No impact (only leader reconciles) |

## Advanced Topics

### Custom Lease Namespace

Deploy operator in one namespace, lease in another:

```yaml
env:
- name: LEASE_NAMESPACE
  value: "kube-system"  # Centralized lease storage
```

Requires cross-namespace RBAC:
```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: bindy-lease
  namespace: kube-system
rules:
- apiGroups: ["coordination.k8s.io"]
  resources: ["leases"]
  verbs: ["get", "create", "update", "patch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: bindy-lease
  namespace: kube-system
subjects:
- kind: ServiceAccount
  name: bindy
  namespace: dns-system
roleRef:
  kind: Role
  name: bindy-lease
  apiGroup: rbac.authorization.k8s.io
```

### Graceful Shutdown

Operator handles SIGTERM gracefully:

1. Receives SIGTERM signal
2. Stops renewing lease
3. Waits for standby to acquire lease
4. Shuts down operators
5. Exits cleanly

### Debugging Leader Election

Enable debug logging:

```bash
kubectl set env deployment/bindy \
  -n dns-system \
  RUST_LOG=debug

# Watch detailed logs
kubectl logs -n dns-system deployment/bindy -f
```

## See Also

- [High Availability](../advanced/ha.md) - DNS instance HA strategies
- [Monitoring](./monitoring.md) - Monitoring operator health
- [Troubleshooting](./troubleshooting.md) - General troubleshooting guide
- [RBAC](./rbac.md) - Role-Based Access Control configuration
