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

Two subcommands handle everything — namespace, CRDs, RBAC, and the Deployment — via server-side apply (idempotent, safe to re-run):

```bash
# Deploy the operator
bindy bootstrap operator

# Deploy Scout (optional — creates ARecords from annotated Ingresses)
bindy bootstrap scout
```

The image tag automatically matches the binary version (e.g. `ghcr.io/firestoned/bindy:v0.5.0`). Override with `--version` if needed.

**Air-gapped / private registry:**

```bash
bindy bootstrap operator --registry harbor.corp.internal/bindy-mirror
bindy bootstrap scout    --registry harbor.corp.internal/bindy-mirror
```

Use `--dry-run` to preview every resource as YAML before applying:

```bash
bindy bootstrap operator --dry-run
```

### <img src="../../images/scouty.svg" alt="Scouty the Scout Bee" width="40" style="vertical-align: middle; margin-right: 2px;"/> Deploying Scout

Scout is an optional, event-driven controller that watches `Ingress` resources and automatically creates `ARecord` CRs in the bindy namespace, no manual DNS management needed for application teams.

```bash
bindy bootstrap scout
```

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
