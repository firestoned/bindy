# Deploying the Operator

The Bindy operator watches for DNS resources and manages BIND9 configurations.

## Prerequisites

Before deploying the operator:

1. [CRDs must be installed](./crds.md)
2. RBAC must be configured
3. Namespace must exist (`dns-system` recommended)

## Installation

### Create Namespace

```bash
kubectl create namespace dns-system
```

### Install RBAC

```bash
kubectl apply -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/rbac/
```

This creates:
- ServiceAccount for the operator
- ClusterRole with required permissions
- ClusterRoleBinding to bind them together

### Deploy Operator

```bash
kubectl apply -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/operator/deployment.yaml
```

### Wait for Readiness

```bash
kubectl wait --for=condition=available --timeout=300s \
  deployment/bind9-operator -n dns-system
```

## Verify Deployment

Check operator pod status:

```bash
kubectl get pods -n dns-system -l app=bind9-operator
```

Expected output:

```
NAME                                READY   STATUS    RESTARTS   AGE
bind9-operator-7d4b8c4f9b-x7k2m   1/1     Running   0          1m
```

Check operator logs:

```bash
kubectl logs -n dns-system -l app=bind9-operator -f
```

You should see:

```
{"timestamp":"2024-01-01T00:00:00Z","level":"INFO","message":"Starting Bindy operator"}
{"timestamp":"2024-01-01T00:00:01Z","level":"INFO","message":"Watching DNSZone resources"}
{"timestamp":"2024-01-01T00:00:01Z","level":"INFO","message":"Watching DNS record resources"}
```

## Configuration

### Environment Variables

Configure the operator via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (error, warn, info, debug, trace) |
| `BIND9_ZONES_DIR` | `/etc/bind/zones` | Directory for zone files |
| `RECONCILE_INTERVAL` | `300` | Reconciliation interval in seconds |

Edit the deployment to customize:

```yaml
env:
  - name: RUST_LOG
    value: "debug"
  - name: BIND9_ZONES_DIR
    value: "/var/lib/bind/zones"
```

### Resource Limits

For production, set appropriate resource limits:

```yaml
resources:
  requests:
    cpu: 100m
    memory: 128Mi
  limits:
    cpu: 500m
    memory: 512Mi
```

### High Availability

Run multiple replicas with leader election:

```yaml
spec:
  replicas: 3
```

## Troubleshooting

### Operator Not Starting

1. Check pod events:
   ```bash
   kubectl describe pod -n dns-system -l app=bind9-operator
   ```

2. Check if CRDs are installed:
   ```bash
   kubectl get crd | grep bindy.firestoned.io
   ```

3. Check RBAC permissions:
   ```bash
   kubectl auth can-i list dnszones --as=system:serviceaccount:dns-system:bind9-operator
   ```

### High Memory Usage

If the operator uses excessive memory:

1. Reduce log level: `RUST_LOG=warn`
2. Increase resource limits
3. Check for memory leaks in logs

## Next Steps

- [Quick Start Guide](./quickstart.md) - Create your first DNS zone
- [Configuration](../operations/configuration.md) - Advanced configuration
- [Monitoring](../operations/monitoring.md) - Set up monitoring
