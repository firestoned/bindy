# Environment Variables

Configure Bindy using environment variables. See also the [CLI Reference](../reference/cli.md) for flags that can override these values.

---

## `bindy run` — Main Operator

### Kubernetes Client

| Variable | Default | Description |
|---|---|---|
| `BINDY_KUBE_QPS` | `20.0` | Sustained API server request rate (queries per second), enforced client-side ([ADR-0005](https://github.com/firestoned/bindy/blob/main/docs/adr/0005-client-side-kube-api-rate-limiting.md)). Requests over budget queue; they are not rejected. |
| `BINDY_KUBE_BURST` | `30` | Requests allowed to burst above the sustained rate. Invalid or non-positive values for either variable fall back to the default with a warning. |
| `BINDY_KUBE_REQUEST_TIMEOUT_SECS` | `30` | Deadline, in whole seconds, for each non-watch API request, covering the response headers and body ([ADR-0014](https://github.com/firestoned/bindy/blob/main/docs/adr/0014-bounded-kube-api-request-timeout.md)). A request on a stalled connection fails after this long and is retried with backoff. Watch streams are exempt. Invalid or zero values fall back to the default with a warning. |

### Namespace Scoping

| Variable | Default | Description |
|---|---|---|
| `BINDY_WATCH_NAMESPACES` | *(unset — cluster-wide)* | Comma-separated list of namespaces to watch, e.g. `tenant-a,tenant-b`. When set, the operator builds one watch and one controller **per namespace** (`Api::namespaced`) and needs only a `Role`/`RoleBinding` in each, instead of a cluster-wide `ClusterRoleBinding`. |

Leaving this unset is the default and keeps the operator cluster-wide, exactly as
before. Setting it is a deliberate opt-in that **also requires applying the namespaced
RBAC** — see [`deploy/operator/rbac/namespaced/README.md`](https://github.com/firestoned/bindy/blob/main/deploy/operator/rbac/namespaced/README.md).

!!! warning "The namespace list and the RBAC must match exactly"
    If the operator watches a namespace with no `Role`, it crash-loops on 403s. If a
    `Role` exists for a namespace the operator does not watch, that is a silent
    over-grant. Change both together.

**What this buys you.** With the cluster-wide `ClusterRoleBinding` removed, the
operator ServiceAccount can no longer read `Secrets` or create workloads outside the
watched namespaces. That closes the two findings behind the cluster-wide deployment:
a compromised operator token can no longer read every Secret in the cluster, nor
create a Deployment whose pod template names a privileged ServiceAccount.

**What it does not buy you.** A slim `ClusterRole` remains, granting
`get/list/watch` on `clusterbind9providers`. `ClusterBind9Provider` is the only bindy
kind with `scope: Cluster`, and a cluster-scoped object has no namespace to scope a
watch to. So this reduces cluster-wide access to a single read-only CRD; it does not
eliminate cluster-wide access entirely.

!!! note "Leader election is independent"
    The lease lives in `BINDY_LEASE_NAMESPACE` (default `bindy-system`), which is not
    required to be in the watched set. If it is not, apply the namespaced `Role` and
    `RoleBinding` there too, or grant a dedicated lease-only Role.

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
| `RUST_LOG` | `info` | Log level filter. Values: `trace`, `debug`, `info`, `warn`, `error`. Supports module-level filters: `bindy=debug,kube=warn`. `bindy::audit=info` is added unless the value names `bindy::audit` ([Secret Audit Log](secret-audit-log.md)). |
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
| `BINDY_SCOUT_DEFAULT_ZONE` | `--default-zone` | unset | Zone used when a source has no `bindy.firestoned.io/zone` annotation. Must be inside `BINDY_SCOUT_ALLOWED_ZONES` when that is set, or Scout fails at startup. |
| `BINDY_SCOUT_DEFAULT_IPS` | `--default-ips` | unset | Comma-separated IPv4 addresses used when no annotation or discovered address applies. Must be inside `BINDY_SCOUT_ALLOWED_IP_RANGES` when that is set, or Scout fails at startup. |
| `BINDY_SCOUT_GATEWAY_SERVICES` | `--gateway-service` | unset | Comma-separated `gatewayClass=namespace/name` or `gatewayClass=namespace/<label-selector>` map for Gateway API route IP resolution. A selector publishes every matching LoadBalancer Service (ADR-0028); multi-label selectors need the repeatable flag. |
| `BINDY_SCOUT_NAMESPACE_SELECTOR` | `--namespace-selector` | unset (every namespace) | Label selector a source's namespace must match. |
| `BINDY_SCOUT_ALLOWED_ZONES` | `--allowed-zones` | unset (every zone) | Comma-separated zones Scout may publish into; each entry also allows its subzones. A source naming any other zone publishes nothing and keeps the records it already had. `*` and entries that are not DNS names fail startup. Unset logs a startup warning ([ADR-0025](https://github.com/firestoned/bindy/blob/main/docs/adr/0025-scout-records-bound-to-cluster-and-zones.md)). |
| `BINDY_SCOUT_ALLOWED_IP_RANGES` | `--allowed-ip-ranges` | unset (every address) | Comma-separated IPv4 CIDR ranges (a bare address means `/32`). A source with any address outside them, annotated or discovered, publishes nothing and is retried every 30 seconds; a route's addresses discovered from its Gateway are the exception, dropped one by one with a warning (ADR-0028). A malformed range fails startup. Unset logs a startup warning. |
| `BINDY_SCOUT_REMOTE_SECRET` | none | unset | Name of the kubeconfig Secret from `bindy bootstrap mc`; Scout then writes to the bindy cluster. Mutually exclusive with `BINDY_SCOUT_REMOTE_ENDPOINT`. |
| `BINDY_SCOUT_REMOTE_SECRET_NAMESPACE` | none | `POD_NAMESPACE` | Namespace of `BINDY_SCOUT_REMOTE_SECRET`. |
| `BINDY_SCOUT_REMOTE_ENDPOINT` | — | Bindy cluster API URL for the endpoint remote mode (ADR-0008): Linkerd-mirrored meshed proxy, konnectivity, or the API server. Requires `BINDY_SCOUT_REMOTE_TOKEN_FILE`; mutually exclusive with `BINDY_SCOUT_REMOTE_SECRET`. |
| `BINDY_SCOUT_REMOTE_TOKEN_FILE` | — | Bearer-token file minted by the Bindy cluster; re-read on rotation. |
| `BINDY_SCOUT_REMOTE_CA_FILE` | — | Endpoint CA bundle (PEM); unset means webpki public roots. |
| `RUST_LOG` | none | `info` | Log level filter; `bindy::audit=info` is added unless the value names `bindy::audit`. |
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
            # What this cluster may publish (ADR-0025): its own zone and its
            # load-balancer range.
            - name: BINDY_SCOUT_ALLOWED_ZONES
              value: "prod.example.com"
            - name: BINDY_SCOUT_ALLOWED_IP_RANGES
              value: "10.20.0.0/24"
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
6. **Set `BINDY_SCOUT_ALLOWED_ZONES` and `BINDY_SCOUT_ALLOWED_IP_RANGES` on every Scout that serves tenants**, and in multi-cluster mode register each cluster's ServiceAccount with `bindy bootstrap mc --cluster-name --allowed-zones` ([ADR-0025](https://github.com/firestoned/bindy/blob/main/docs/adr/0025-scout-records-bound-to-cluster-and-zones.md)), so no tenant annotation or stolen token can publish outside the cluster's own zones.
