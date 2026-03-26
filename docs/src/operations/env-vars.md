# Environment Variables

Configure Bindy using environment variables. See also the [CLI Reference](../reference/cli.md) for flags that can override these values.

---

## `bindy run` — Main Operator

### Kubernetes Client

| Variable | Default | Description |
|---|---|---|
| `BINDY_KUBE_QPS` | `50.0` | API server request rate (queries per second). |
| `BINDY_KUBE_BURST` | `100` | API server burst cap above QPS. |

### Leader Election

| Variable | Default | Description |
|---|---|---|
| `BINDY_ENABLE_LEADER_ELECTION` | `true` | Set to `false` to disable. Only disable for local development — production deployments should always use leader election. |
| `BINDY_LEASE_NAME` | `bindy-leader` | Name of the Kubernetes `Lease` object. |
| `BINDY_LEASE_NAMESPACE` | `$POD_NAMESPACE` or `bindy-system` | Namespace where the `Lease` object lives. |
| `BINDY_LEASE_DURATION_SECONDS` | `15` | How long a lease is held before it expires. A new leader cannot be elected until this expires. |
| `BINDY_LEASE_RENEW_DEADLINE_SECONDS` | `10` | Deadline within which the current leader must renew its lease. |
| `BINDY_LEASE_RETRY_PERIOD_SECONDS` | `2` | How often non-leaders attempt to acquire the lease. |
| `POD_NAME` | — | Leader election identity. Falls back to `$HOSTNAME`, then a random string. Inject via the Kubernetes downward API. |
| `POD_NAMESPACE` | — | Pod namespace. Injected by Kubernetes downward API. Used as fallback lease namespace. |

### Metrics

| Variable | Default | Description |
|---|---|---|
| `BINDY_METRICS_BIND_ADDRESS` | `0.0.0.0` | Bind address for the Prometheus scrape endpoint. |
| `BINDY_METRICS_PORT` | `8080` | Port for the Prometheus scrape endpoint. |
| `BINDY_METRICS_PATH` | `/metrics` | HTTP path for Prometheus scraping. |

### Logging

| Variable | Default | Description |
|---|---|---|
| `RUST_LOG` | `info` | Log level filter. Values: `trace`, `debug`, `info`, `warn`, `error`. Supports module-level filters: `bindy=debug,kube=warn`. |
| `RUST_LOG_FORMAT` | `text` | Log output format. `text` = compact human-readable. `json` = structured for Loki, ELK, Splunk, etc. |

**Example JSON log entry:**

```json
{
  "timestamp": "2025-11-30T10:00:00.123456Z",
  "level": "INFO",
  "message": "Starting BIND9 DNS Operator",
  "file": "main.rs",
  "line": 80,
  "threadName": "bindy-run"
}
```

### Example Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: bindy
  namespace: bindy-system
spec:
  replicas: 2
  selector:
    matchLabels:
      app.kubernetes.io/name: bindy
      app.kubernetes.io/component: operator
  template:
    metadata:
      labels:
        app.kubernetes.io/name: bindy
        app.kubernetes.io/component: operator
    spec:
      serviceAccountName: bindy
      containers:
        - name: operator
          image: ghcr.io/firestoned/bindy:latest
          args: ["run"]
          env:
            - name: RUST_LOG
              value: "info"
            - name: RUST_LOG_FORMAT
              value: "json"
            - name: POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: BINDY_ENABLE_LEADER_ELECTION
              value: "true"
            - name: BINDY_LEASE_NAME
              value: "bindy-leader"
```

---

## `bindy scout` — Scout Controller

Scout can be configured via CLI flags or environment variables. CLI flags take precedence.
See [Bindy Scout](../guide/scout.md) for the full conceptual guide.

| Variable | CLI flag | Default | Description |
|---|---|---|---|
| `BINDY_SCOUT_CLUSTER_NAME` | `--cluster-name` | — | **Required.** Logical cluster name stamped on all created `ARecord` labels. If changed and Scout is restarted, stale `ARecord` CRs from the old name are deleted automatically on the next reconcile. |
| `BINDY_SCOUT_NAMESPACE` | `--namespace` | `bindy-system` | Namespace where `ARecord` CRs are created. |
| `POD_NAMESPACE` | — | `default` | Scout's own namespace. Always excluded from Ingress watching. Inject via Kubernetes downward API. |
| `BINDY_SCOUT_EXCLUDE_NAMESPACES` | — | — | Comma-separated list of additional namespaces to exclude from Ingress watching. |
| `RUST_LOG` | — | `info` | Log level filter. |
| `RUST_LOG_FORMAT` | — | `text` | Log format (`text` or `json`). |

### Example Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: bindy-scout
  namespace: bindy-system
spec:
  replicas: 1
  selector:
    matchLabels:
      app.kubernetes.io/name: bindy
      app.kubernetes.io/component: scout
  template:
    metadata:
      labels:
        app.kubernetes.io/name: bindy
        app.kubernetes.io/component: scout
    spec:
      serviceAccountName: bindy-scout
      containers:
        - name: scout
          image: ghcr.io/firestoned/bindy:latest
          args: ["scout"]
          env:
            - name: BINDY_SCOUT_CLUSTER_NAME
              value: "prod"
            - name: BINDY_SCOUT_NAMESPACE
              value: "bindy-system"
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: BINDY_SCOUT_EXCLUDE_NAMESPACES
              value: "kube-system,kube-public,kube-node-lease"
            - name: RUST_LOG
              value: "info"
            - name: RUST_LOG_FORMAT
              value: "json"
```

---

## Best Practices

1. **Use `info` in production** — balances visibility and noise. Raise to `debug` temporarily for troubleshooting.
2. **Use `json` format in production** — enables structured log parsing in Loki, ELK, and Splunk.
3. **Always enable leader election in production** — prevents split-brain when running multiple replicas.
4. **Inject `POD_NAME` and `POD_NAMESPACE` via downward API** — ensures unique leader identities and correct lease placement.
5. **Set `BINDY_SCOUT_EXCLUDE_NAMESPACES`** — skip namespaces that will never have Scout-annotated Ingresses to reduce unnecessary watch events.
