# Deploying Scout

!!! info "Two Deployment Modes"
    **Same-cluster mode** (default): Scout and the Bindy operator run in the same cluster. No extra configuration needed.

    **Multi-cluster mode**: Scout runs on workload clusters and writes to a dedicated **Queen Bee cluster** cluster running Bindy. Use `bindy bootstrap mc` to generate credentials, then set `BINDY_SCOUT_REMOTE_SECRET`. See [Multi-Cluster Setup](#multi-cluster-setup) below.

**Bindy Scout** is an optional companion controller that watches `Ingress` resources and `LoadBalancer` Services across your cluster and automatically creates `ARecord` CRs on behalf of application teams — without requiring them to have write access to the bindy namespace.

See the [Bindy Scout guide](../guide/scout.md) for the full conceptual overview, including the scout bee backstory, how record naming works, and the multi-cluster roadmap.

---

## Prerequisites

- The main Bindy operator must already be running ([Deploying Operator](./controller.md))
- CRDs must be installed (Scout uses the `ARecord` CRD)
- A `DNSZone` must exist for the zone Scout will write records into

---

## Install

The recommended way to install Scout is with the `bindy` CLI. This applies all resources via server-side apply and is safe to re-run:

```bash
bindy bootstrap scout
```

This creates:

- A `Namespace` (`bindy-system` by default)
- All 12 `CustomResourceDefinition` objects (same set as the operator — safe if already installed)
- A `ServiceAccount` (`bindy-scout`) in `bindy-system`
- A `ClusterRole` / `ClusterRoleBinding` for cluster-wide Ingress watch, LoadBalancer Service watch, DNSZone read, and Secret read
- A `Role` / `RoleBinding` for `ARecord` write access in `bindy-system`
- A `Deployment` running the `bindy scout` controller

!!! tip "Custom namespace or version"
    ```bash
    bindy bootstrap scout --namespace my-namespace --version v0.5.0
    ```

!!! tip "Air-gapped / private registry"
    ```bash
    bindy bootstrap scout --registry harbor.corp.internal/bindy-mirror
    ```
    This produces `harbor.corp.internal/bindy-mirror/bindy:<version>` instead of `ghcr.io/firestoned/bindy:<version>`. See the [CLI reference](../reference/cli.md#air-gapped-environments) for the full air-gapped workflow.

!!! tip "Preview before applying"
    ```bash
    bindy bootstrap scout --dry-run
    ```
    Prints every resource as YAML without connecting to the cluster.

---

## Configure

Scout requires one mandatory setting: the **logical cluster name** that is stamped on every `ARecord` it creates. Set it via environment variable or CLI flag.

=== "Environment variable"

    ```yaml
    env:
      - name: BINDY_SCOUT_CLUSTER_NAME
        value: "prod"
      - name: POD_NAMESPACE
        valueFrom:
          fieldRef:
            fieldPath: metadata.namespace
    ```

=== "CLI flag"

    ```yaml
    args: ["scout", "--cluster-name", "prod"]
    ```

!!! note "CLI takes precedence"
    When both `--cluster-name` and `BINDY_SCOUT_CLUSTER_NAME` are set, the CLI flag wins.

### Full configuration reference

| Variable | CLI flag | Default | Description |
|---|---|---|---|
| `BINDY_SCOUT_CLUSTER_NAME` | `--cluster-name` | — | **Required.** Logical cluster name stamped on all created `ARecord` labels. |
| `BINDY_SCOUT_NAMESPACE` | `--namespace` | `bindy-system` | Namespace where `ARecord` CRs are created. |
| `POD_NAMESPACE` | — | `default` | Scout's own namespace. Always excluded from Ingress watching. Inject via downward API. |
| `BINDY_SCOUT_EXCLUDE_NAMESPACES` | — | — | Comma-separated list of additional namespaces to skip. |
| `BINDY_SCOUT_DEFAULT_ZONE` | `--default-zone` | — | Default DNS zone when no `bindy.firestoned.io/zone` annotation is present. With `DEFAULT_IPS`, Ingresses only need `scout-enabled: "true"`. |
| `BINDY_SCOUT_DEFAULT_IPS` | `--default-ips` | — | Comma-separated default IP(s) used when no per-Ingress annotation or LB status IP is available. For shared-ingress topologies (e.g. Traefik). |
| `BINDY_SCOUT_REMOTE_SECRET` | — | — | **(Multi-cluster)** Name of a Secret in the local cluster containing a `kubeconfig` key for the Queen Bee cluster. When set, Scout writes `ARecord` CRs and validates zones on the remote Bindy cluster. See [Multi-Cluster Setup](#multi-cluster-setup) below. |
| `BINDY_SCOUT_REMOTE_SECRET_NAMESPACE` | — | Scout's own namespace | **(Multi-cluster)** Namespace of the `BINDY_SCOUT_REMOTE_SECRET`. Defaults to Scout's own namespace. |
| `RUST_LOG` | — | `info` | Log level: `trace`, `debug`, `info`, `warn`, `error`. |
| `RUST_LOG_FORMAT` | — | `text` | Log format: `text` or `json`. |

---

## Multi-Cluster Setup

In multi-cluster deployments, Scout runs on **workload clusters** and writes `ARecord` CRs to the dedicated **Queen Bee cluster** (where the Bindy operator lives). Use `bindy bootstrap mc` to generate the credentials.

### 1. Generate credentials on the Queen Bee cluster

```bash
# Run this against the Queen Bee cluster (with Queen Bee cluster KUBECONFIG active)
bindy bootstrap mc \
  --service-account bindy-scout-remote \
  --namespace bindy-system \
  | kubectl --context=<child-cluster> apply -f -
```

This single pipeline:
1. Creates a `ServiceAccount` + `Role` + `RoleBinding` on the **Queen Bee cluster** (minimal: ARecord CRUD + DNSZone read)
2. Generates a kubeconfig for that service account
3. Outputs a `bindy.firestoned.io/remote-kubeconfig` Secret and applies it to the **child cluster**

!!! tip "One SA per child cluster"
    Use a unique `--service-account` per child cluster for independent credential revocation:
    ```bash
    bindy bootstrap mc --service-account bindy-scout-remote-cluster-a \
      | kubectl --context=cluster-a apply -f -
    ```

### 2. Enable remote mode on the Scout Deployment

Add `BINDY_SCOUT_REMOTE_SECRET` to the Scout Deployment on the child cluster:

```yaml
env:
  - name: BINDY_SCOUT_REMOTE_SECRET
    value: "bindy-scout-remote-kubeconfig"
```

Scout will load the kubeconfig from that Secret and write all `ARecord` CRs to the Queen Bee cluster instead of the local cluster.

See the [Scout guide — Multi-Cluster Mode](../guide/scout.md#multi-cluster-mode-phase-2) for the full architecture diagram, per-cluster SA strategy, and RBAC details.

---

## Verify

Check that the Scout pod is running:

```bash
kubectl get pods -n bindy-system -l app.kubernetes.io/component=scout
```

Expected output:

```
NAME                           READY   STATUS    RESTARTS   AGE
bindy-scout-6d4b9f7c8d-r2kp4   1/1     Running   0          30s
```

Check Scout logs:

```bash
kubectl logs -n bindy-system -l app.kubernetes.io/component=scout -f
```

You should see Scout announce which namespaces it is watching and confirm it is ready.

---

## Test

Annotate an Ingress in any non-excluded namespace:

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: my-app
  namespace: my-app-ns
  annotations:
    bindy.firestoned.io/recordKind: "ARecord"
    bindy.firestoned.io/zone: "example.com"
spec:
  rules:
    - host: my-app.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: my-app
                port:
                  number: 80
```

Within seconds, Scout creates an `ARecord` in `bindy-system`:

```bash
kubectl get arecords -n bindy-system -l bindy.firestoned.io/managed-by=scout
```

---

## Next Steps

- [Bindy Scout Guide](../guide/scout.md) — full conceptual guide, annotations reference, record naming, and RBAC details
- [Environment Variables](../operations/env-vars.md) — complete Scout environment variable reference
