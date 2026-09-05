# Installing CRDs

Custom Resource Definitions (CRDs) extend Kubernetes with new resource types for DNS management.

## What are CRDs?

CRDs define the schema for custom resources in Kubernetes. Bindy uses CRDs to represent:

- BIND9 clusters (cluster-level configuration)
- BIND9 instances (individual DNS server deployments)
- DNS zones
- DNS records (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)

## Installation

### Install from Release (Recommended)

Install all Bindy CRDs from the latest release:

```bash
kubectl apply --server-side -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml
```

Or install a specific version:

```bash
kubectl apply --server-side -f https://github.com/firestoned/bindy/releases/download/v0.5.0/crds.yaml
```

### Install from Source

Install from local files:

```bash
cd bindy
kubectl apply --server-side -f deploy/operator/crds/
```

### `--server-side` is required, not optional

The `Bind9Cluster`, `ClusterBind9Provider`, and `Bind9Instance` CRDs are large
— several hundred kilobytes each — because they embed full Kubernetes schemas
for the passthrough fields they accept (`service.spec`, `volumes`,
`volumeMounts`, and so on).

Plain `kubectl apply` is a *client-side* apply: it stores a copy of the whole
manifest in the `kubectl.kubernetes.io/last-applied-configuration` annotation,
and annotations are capped at 256 KB. Applying these CRDs client-side fails
outright:

```text
The CustomResourceDefinition "bind9clusters.bindy.firestoned.io" is invalid:
metadata.annotations: Too long: may not be more than 262144 bytes
```

`--server-side` tracks ownership in `metadata.managedFields` instead of that
annotation, so the limit does not apply. It also works for **upgrades**, which
is why it is preferred over `kubectl create`: `create` succeeds on a fresh
cluster but fails with `AlreadyExists` when the CRDs are already installed.

If you have previously installed these CRDs client-side, the first server-side
apply may report a conflict. Resolve it once with:

```bash
kubectl apply --server-side --force-conflicts -f deploy/operator/crds/
```

### Updating Existing CRDs

Server-side apply handles updates too — the same command used to install:

```bash
kubectl apply --server-side -f deploy/operator/crds/
```

Do **not** use `kubectl replace --force` for this. It deletes and recreates
each CRD, and deleting a CRD cascades to every custom resource defined by it —
so it would take all your `Bind9Cluster`, `DNSZone`, and record objects with
it. It was previously recommended here as a way around the annotation size
limit; `--server-side` solves that without the data loss.

## Verify Installation

Check that all CRDs are installed:

```bash
kubectl get crd | grep bindy.firestoned.io
```

Expected output:

```
aaaarecords.bindy.firestoned.io         2024-01-01T00:00:00Z
arecords.bindy.firestoned.io            2024-01-01T00:00:00Z
bind9clusters.bindy.firestoned.io       2024-01-01T00:00:00Z
bind9instances.bindy.firestoned.io      2024-01-01T00:00:00Z
caarecords.bindy.firestoned.io          2024-01-01T00:00:00Z
cnamerecords.bindy.firestoned.io        2024-01-01T00:00:00Z
dnszones.bindy.firestoned.io            2024-01-01T00:00:00Z
mxrecords.bindy.firestoned.io           2024-01-01T00:00:00Z
nsrecords.bindy.firestoned.io           2024-01-01T00:00:00Z
ptrrecords.bindy.firestoned.io          2024-01-01T00:00:00Z
srvrecords.bindy.firestoned.io          2024-01-01T00:00:00Z
txtrecords.bindy.firestoned.io          2024-01-01T00:00:00Z
```

## CRD Details

For detailed specifications of each CRD, see:

- [Bind9Instance Spec](../reference/bind9instance-spec.md)
- [DNSZone Spec](../reference/dnszone-spec.md)
- [Record Specs](../reference/record-specs.md)

## Next Steps

- [Deploy the Operator](./controller.md)
- [Step-by-Step Guide](./step-by-step.md)
