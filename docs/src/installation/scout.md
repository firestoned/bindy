# Deploying Scout

!!! info "Same-Cluster Mode is the Default"
    Without additional configuration, Scout and the Bindy operator must run in the **same Kubernetes cluster**. For cross-cluster deployments, set `BINDY_SCOUT_REMOTE_SECRET` to a Secret containing a kubeconfig for the remote Bindy cluster. See the [Scout guide](../guide/scout.md) for details.

**Bindy Scout** is an optional companion controller that watches `Ingress` resources across your cluster and automatically creates `ARecord` CRs on behalf of application teams — without requiring them to have write access to the bindy namespace.

See the [Bindy Scout guide](../guide/scout.md) for the full conceptual overview, including the scout bee backstory, how record naming works, and the multi-cluster roadmap.

---

## Prerequisites

- The main Bindy operator must already be running ([Deploying Operator](./controller.md))
- CRDs must be installed (Scout uses the `ARecord` CRD)
- A `DNSZone` must exist for the zone Scout will write records into

---

## Install

Deploy Scout from the latest release:

```bash
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/scout.yaml
```

!!! tip "Specific version"
    To pin to a specific release instead of `latest`:
    ```bash
    kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/scout.yaml
    ```

This creates:

- A `ServiceAccount` (`bindy-scout`) in `bindy-system`
- A `ClusterRole` / `ClusterRoleBinding` for cluster-wide Ingress and DNSZone read access
- A `Role` / `RoleBinding` for `ARecord` write access in `bindy-system`
- A `Deployment` running the `bindy scout` controller

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
    args: ["scout", "--bind9-cluster-name", "prod"]
    ```

!!! note "CLI takes precedence"
    When both `--bind9-cluster-name` and `BINDY_SCOUT_CLUSTER_NAME` are set, the CLI flag wins.

### Full configuration reference

| Variable | CLI flag | Default | Description |
|---|---|---|---|
| `BINDY_SCOUT_CLUSTER_NAME` | `--bind9-cluster-name` | — | **Required.** Logical cluster name stamped on all created `ARecord` labels. |
| `BINDY_SCOUT_NAMESPACE` | `--namespace` | `bindy-system` | Namespace where `ARecord` CRs are created. |
| `POD_NAMESPACE` | — | `default` | Scout's own namespace. Always excluded from Ingress watching. Inject via downward API. |
| `BINDY_SCOUT_EXCLUDE_NAMESPACES` | — | — | Comma-separated list of additional namespaces to skip. |
| `BINDY_SCOUT_DEFAULT_ZONE` | `--default-zone` | — | Default DNS zone when no `bindy.firestoned.io/zone` annotation is present. With `DEFAULT_IPS`, Ingresses only need `scout-enabled: "true"`. |
| `BINDY_SCOUT_DEFAULT_IPS` | `--default-ips` | — | Comma-separated default IP(s) used when no per-Ingress annotation or LB status IP is available. For shared-ingress topologies (e.g. Traefik). |
| `RUST_LOG` | — | `info` | Log level: `trace`, `debug`, `info`, `warn`, `error`. |
| `RUST_LOG_FORMAT` | — | `text` | Log format: `text` or `json`. |

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
