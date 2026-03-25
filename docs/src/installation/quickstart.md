# Quick Start

Get up and running with Bindy in four steps.

## 1. Download the bindy binary

```bash
# Linux (amd64)
curl -Lo bindy.tar.gz https://github.com/firestoned/bindy/releases/latest/download/bindy-linux-amd64.tar.gz
tar xzf bindy.tar.gz && chmod +x bindy && sudo mv bindy /usr/local/bin/

# Linux (arm64)
curl -Lo bindy.tar.gz https://github.com/firestoned/bindy/releases/latest/download/bindy-linux-arm64.tar.gz
tar xzf bindy.tar.gz && chmod +x bindy && sudo mv bindy /usr/local/bin/

# macOS (arm64 / Apple Silicon)
curl -Lo bindy.tar.gz https://github.com/firestoned/bindy/releases/latest/download/bindy-macos-arm64.tar.gz
tar xzf bindy.tar.gz && chmod +x bindy && sudo mv bindy /usr/local/bin/

# macOS (amd64 / Intel)
curl -Lo bindy.tar.gz https://github.com/firestoned/bindy/releases/latest/download/bindy-macos-amd64.tar.gz
tar xzf bindy.tar.gz && chmod +x bindy && sudo mv bindy /usr/local/bin/
```

## 2. Bootstrap the cluster

A single command creates the `bindy-system` namespace, installs all CRDs, sets up RBAC, and deploys the operator:

```bash
bindy bootstrap
```

The operator image tag matches the binary version automatically (e.g. `ghcr.io/firestoned/bindy:v0.5.0`). To override: `bindy bootstrap --version latest`.

## 3. Create a BIND9 instance, zone, and record

Save this as `dns.yaml` and apply it:

```yaml
# A single-primary BIND9 cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: my-dns
  namespace: bindy-system
spec:
  primary:
    replicas: 1

---
# A DNS zone that picks up records by label
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: bindy-system
spec:
  zoneName: example.com
  clusterRef: my-dns
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com

---
# An A record for www.example.com
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
  namespace: bindy-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.1"
```

```bash
kubectl apply -f dns.yaml
```

Test it:

```bash
# Get the DNS service IP
DNS_IP=$(kubectl get svc -n bindy-system -o jsonpath='{.items[0].status.loadBalancer.ingress[0].ip}')

# Query your record
dig @$DNS_IP www.example.com A
```

## Next Steps

- [Step-by-Step Guide](./step-by-step.md) - Detailed walkthrough with explanations
- [Creating Zones](../guide/creating-zones.md) - Zone configuration options
- [DNS Records](../guide/records-guide.md) - All supported record types
- [High Availability](../advanced/ha.md) - Multi-replica production setup
