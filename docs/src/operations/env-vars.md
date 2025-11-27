# Environment Variables

Configure the Bindy controller using environment variables.

## Controller Environment Variables

### RUST_LOG

Control logging level:

```yaml
env:
  - name: RUST_LOG
    value: "info"  # Options: error, warn, info, debug, trace
```

**Levels:**
- `error` - Only errors
- `warn` - Warnings and errors
- `info` - Informational messages (default)
- `debug` - Detailed debugging
- `trace` - Very detailed tracing

### RECONCILE_INTERVAL

Set how often to reconcile resources (in seconds):

```yaml
env:
  - name: RECONCILE_INTERVAL
    value: "300"  # 5 minutes
```

### NAMESPACE

Limit operator to specific namespace:

```yaml
env:
  - name: NAMESPACE
    valueFrom:
      fieldRef:
        fieldPath: metadata.namespace
```

Omit to watch all namespaces (requires ClusterRole).

## Example Deployment Configuration

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: bindy
  namespace: dns-system
spec:
  replicas: 1
  selector:
    matchLabels:
      app: bindy
  template:
    metadata:
      labels:
        app: bindy
    spec:
      serviceAccountName: bindy
      containers:
      - name: controller
        image: ghcr.io/firestoned/bindy:latest
        env:
        - name: RUST_LOG
          value: "info"
        - name: NAMESPACE
          valueFrom:
            fieldRef:
              fieldPath: metadata.namespace
```

## Best Practices

1. **Use info level in production** - Balance between visibility and noise
2. **Enable debug for troubleshooting** - Temporarily increase to debug level
3. **Set reconcile interval appropriately** - Don't set too low to avoid API pressure
4. **Use namespace scoping** - Scope to specific namespace if not managing cluster-wide DNS
