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

## bindcar Sidecar (operand pods)

These variables configure the **bindcar** HTTP API sidecar (`api` container) that
the operator injects into every BIND9 operand pod. The operator sets them
automatically from the `Bind9Instance` / `Bind9Cluster` spec — you normally do
**not** set them yourself. They are documented here because they define the
sidecar's authentication contract (bindcar 0.7.x "Mode B" / Kubernetes
TokenReview).

| Variable | Set by operator to | Purpose |
|----------|--------------------|---------|
| `BIND_TOKEN_AUDIENCES` | `bindcar` | Audience that the caller's ServiceAccount token must carry. bindcar validates the token's `status.audiences` (from the TokenReview response) against this. The operator presents a token minted with `audience: bindcar` (projected at `/var/run/secrets/bindcar/token`). |
| `BIND_ALLOWED_SERVICE_ACCOUNTS` | `system:serviceaccount:<operator-ns>:bindy` | Allow-list of ServiceAccounts permitted to call the API. This names the **operator** SA (the caller), not the operand `bind9` SA. The operator namespace comes from `POD_NAMESPACE` (fallback `bindy-system`). |
| `RNDC_SECRET` / `RNDC_ALGORITHM` | from the RNDC key Secret | RNDC credential the sidecar uses to talk to `named`. `RNDC_ALGORITHM` is SHA-2 only (`hmac-md5`/`hmac-sha1` are rejected). |
| `BIND_ZONE_DIR` | `/var/cache/bind` | Zone file directory. |
| `TMPDIR` | `/tmp` | Writable scratch dir (memory-backed `emptyDir`) for bindcar's `0600` TSIG key file — required because the sidecar runs with `readOnlyRootFilesystem: true` under Pod Security Admission `restricted`. |
| `API_PORT` | `8080` (or `bindcarConfig.port`) | HTTP API listen port. |

> ⚠️ **Reserved — do not override via `bindcarConfig.envVars`.** The variables
> above (plus `BIND_API_TOKEN`, `DISABLE_AUTH`, `BIND_ALLOW_ANY_SERVICEACCOUNT`,
> `BIND_ALLOWED_NAMESPACES`, and any `KUBE_*`) are security-critical. As of
> bindcar 0.7.2, setting `BIND_API_TOKEN` *disables* TokenReview, so injecting
> these through user `envVars` can weaken or bypass authentication. See the
> [migration guide](migration-guide.md).

For the full Mode B setup (projected token, `bindcar-tokenreview` RBAC, PSA), see
the [bindcar 0.7.x migration guide](migration-guide.md) and [RBAC](rbac.md).

---

## Best Practices

1. **Use `info` in production** — balances visibility and noise. Raise to `debug` temporarily for troubleshooting.
2. **Use `json` format in production** — enables structured log parsing in Loki, ELK, and Splunk.
3. **Always enable leader election in production** — prevents split-brain when running multiple replicas.
4. **Inject `POD_NAME` and `POD_NAMESPACE` via downward API** — ensures unique leader identities and correct lease placement.
5. **Set `BINDY_SCOUT_EXCLUDE_NAMESPACES`** — skip namespaces that will never have Scout-annotated Ingresses to reduce unnecessary watch events.
