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

### RUST_LOG_FORMAT

Control logging output format:

```yaml
env:
  - name: RUST_LOG_FORMAT
    value: "text"  # Options: text, json
```

**Formats:**
- `text` - Human-readable compact text format (default)
- `json` - Structured JSON format for log aggregation tools

**Use JSON format for:**
- Kubernetes production deployments
- Log aggregation systems (Loki, ELK, Splunk)
- Centralized logging and monitoring
- Automated log parsing and analysis

**Example JSON output:**
```json
{
  "timestamp": "2025-11-30T10:00:00.123456Z",
  "level": "INFO",
  "message": "Starting BIND9 DNS Controller",
  "file": "main.rs",
  "line": 80,
  "threadName": "bindy-controller"
}
```

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
        - name: RUST_LOG_FORMAT
          value: "json"
        - name: NAMESPACE
          valueFrom:
            fieldRef:
              fieldPath: metadata.namespace
```

## Best Practices

1. **Use info level in production** - Balance between visibility and noise
2. **Enable debug for troubleshooting** - Temporarily increase to debug level
3. **Use JSON format in production** - Enable structured logging for better log aggregation
4. **Use text format for development** - More readable for local debugging
5. **Set reconcile interval appropriately** - Don't set too low to avoid API pressure
6. **Use namespace scoping** - Scope to specific namespace if not managing cluster-wide DNS
