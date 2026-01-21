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
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml
```

Or install a specific version:

```bash
kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/crds.yaml
```

This is the **recommended method** as it:
- Installs all CRDs in a single command
- Uses stable, tagged releases
- Avoids GitHub's annotation size limits for large CRDs

### Install from Source

Install all Bindy CRDs from the main branch:

```bash
kubectl create -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/crds/
```

Or install from local files:

```bash
cd bindy
kubectl create -f deploy/crds/
```

**Important**: Use `kubectl create` instead of `kubectl apply` to avoid the 256KB annotation size limit that can occur with large CRDs like `Bind9Instance`.

### Updating Existing CRDs

To update CRDs that are already installed:

```bash
kubectl replace --force -f deploy/crds/
```

The `--force` flag deletes and recreates the CRDs, which is necessary to avoid annotation size limits.

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
srvrecords.bindy.firestoned.io          2024-01-01T00:00:00Z
txtrecords.bindy.firestoned.io          2024-01-01T00:00:00Z
```

## CRD Details

For detailed specifications of each CRD, see:

- [Bind9Instance Spec](../reference/bind9instance-spec.md)
- [DNSZone Spec](../reference/dnszone-spec.md)
- [Record Specs](../reference/record-specs.md)

## Next Steps

- [Deploy the Operator](./operator.md)
- [Quick Start Guide](./quickstart.md)
