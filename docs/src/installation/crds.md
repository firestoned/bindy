# Installing CRDs

Custom Resource Definitions (CRDs) extend Kubernetes with new resource types for DNS management.

## What are CRDs?

CRDs define the schema for custom resources in Kubernetes. Bindy uses CRDs to represent:

- BIND9 instances
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
kubectl get crd | grep dns.firestoned.io
```

Expected output:

```
aaaarecords.dns.firestoned.io         2024-01-01T00:00:00Z
arecords.dns.firestoned.io            2024-01-01T00:00:00Z
bind9instances.dns.firestoned.io      2024-01-01T00:00:00Z
caarecords.dns.firestoned.io          2024-01-01T00:00:00Z
cnamerecords.dns.firestoned.io        2024-01-01T00:00:00Z
dnszones.dns.firestoned.io            2024-01-01T00:00:00Z
mxrecords.dns.firestoned.io           2024-01-01T00:00:00Z
nsrecords.dns.firestoned.io           2024-01-01T00:00:00Z
srvrecords.dns.firestoned.io          2024-01-01T00:00:00Z
txtrecords.dns.firestoned.io          2024-01-01T00:00:00Z
```

## CRD Details

For detailed specifications of each CRD, see:

- [Bind9Instance Spec](../reference/bind9instance-spec.md)
- [DNSZone Spec](../reference/dnszone-spec.md)
- [Record Specs](../reference/record-specs.md)

## Next Steps

- [Deploy the Controller](./controller.md)
- [Quick Start Guide](./quickstart.md)
