# Installing CRDs

Custom Resource Definitions (CRDs) extend Kubernetes with new resource types for DNS management.

## What are CRDs?

CRDs define the schema for custom resources in Kubernetes. Bindy uses CRDs to represent:

- BIND9 clusters (cluster-level configuration)
- BIND9 instances (individual DNS server deployments)
- DNS zones
- DNS records (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)

## Installation

Install all Bindy CRDs:

```bash
kubectl apply -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/crds/
```

Or install from local files:

```bash
cd bindy
kubectl apply -f deploy/crds/
```

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

- [Deploy the Controller](./controller.md)
- [Quick Start Guide](./quickstart.md)
