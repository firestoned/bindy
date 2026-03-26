# Deploying the Operator

The Bindy operator watches for DNS resources and manages BIND9 configurations.

## Prerequisites

- The `bindy` binary (same version as the image you want to deploy)
- `kubectl` configured with cluster access

## Installation

The recommended way to deploy the operator is with the `bindy` CLI. A single command handles namespace creation, CRD installation, RBAC, and the Deployment — all via server-side apply (idempotent, safe to re-run):

```bash
bindy bootstrap operator
```

This creates in order:

1. `Namespace/bindy-system`
2. All 12 CRDs (`bindy.firestoned.io/v1beta1`)
3. `ServiceAccount/bindy`
4. `ClusterRole/bindy-role` — operator permissions
5. `ClusterRole/bindy-admin-role` — admin/destructive permissions
6. `ClusterRoleBinding/bindy-rolebinding`
7. `Deployment/bindy`

!!! tip "Custom namespace or version"
    ```bash
    bindy bootstrap operator --namespace my-namespace --version v0.5.0
    ```

!!! tip "Air-gapped / private registry"
    ```bash
    bindy bootstrap operator --registry harbor.corp.internal/bindy-mirror
    ```
    This produces `harbor.corp.internal/bindy-mirror/bindy:<version>` instead of `ghcr.io/firestoned/bindy:<version>`. See the [CLI reference](../reference/cli.md#air-gapped-environments) for the full workflow.

!!! tip "Preview before applying"
    ```bash
    bindy bootstrap operator --dry-run
    ```

### Wait for Readiness

```bash
kubectl wait --for=condition=available --timeout=300s \
  deployment/bindy -n bindy-system
```

## Verify Deployment

Check operator pod status:

```bash
kubectl get pods -n bindy-system -l app=bind9-operator
```

Expected output:

```
NAME                                READY   STATUS    RESTARTS   AGE
bind9-operator-7d4b8c4f9b-x7k2m   1/1     Running   0          1m
```

Check operator logs:

```bash
kubectl logs -n bindy-system -l app=bind9-operator -f
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
   kubectl describe pod -n bindy-system -l app=bind9-operator
   ```

2. Check if CRDs are installed:
   ```bash
   kubectl get crd | grep bindy.firestoned.io
   ```

3. Check RBAC permissions:
   ```bash
   kubectl auth can-i list dnszones --as=system:serviceaccount:bindy-system:bind9-operator
   ```

### High Memory Usage

If the operator uses excessive memory:

1. Reduce log level: `RUST_LOG=warn`
2. Increase resource limits
3. Check for memory leaks in logs

## Next Steps

- [Step-by-Step Guide](./step-by-step.md) - Create your first DNS zone
- [Configuration](../operations/configuration.md) - Advanced configuration
- [Monitoring](../operations/monitoring.md) - Set up monitoring

---

## What's Next: Scout

Want application teams to get DNS records automatically from their `Ingress` resources — without needing write access to the bindy namespace? Deploy the optional **Bindy Scout** controller.

→ **[Deploying Scout](./scout.md)**
