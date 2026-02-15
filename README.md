# <img src="docs/src/images/bindy-the-bee.png" alt="Bindy the Bee" width="60" style="vertical-align: middle; margin-right: 5px;"/> Bindy - BIND9 DNS Operator for Kubernetes
### *Pronounced: "bined-ee" (like BIND + ee)*

## Project Status

[![License](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GitHub Release](https://img.shields.io/github/v/release/firestoned/bindy)](https://github.com/firestoned/bindy/releases/latest)
[![GitHub commits since latest release](https://img.shields.io/github/commits-since/firestoned/bindy/latest)](https://github.com/firestoned/bindy/commits/main)
[![Last Commit](https://img.shields.io/github/last-commit/firestoned/bindy)](https://github.com/firestoned/bindy/commits/main)

## CI/CD Status

[![Main Branch CI/CD](https://github.com/firestoned/bindy/actions/workflows/main.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/main.yaml)
[![Pull Request Checks](https://github.com/firestoned/bindy/actions/workflows/pr.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/pr.yaml)
[![Release Workflow](https://github.com/firestoned/bindy/actions/workflows/release.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/release.yaml)
[![Integration Tests](https://github.com/firestoned/bindy/actions/workflows/integration.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/integration.yaml)
[![Documentation](https://github.com/firestoned/bindy/actions/workflows/docs.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/docs.yaml)

## Code Quality

[![codecov](https://codecov.io/gh/firestoned/bindy/branch/main/graph/badge.svg)](https://codecov.io/gh/firestoned/bindy)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/firestoned/bindy/badge)](https://api.securityscorecards.dev/projects/github.com/firestoned/bindy)
[![Security Scan](https://github.com/firestoned/bindy/actions/workflows/security-scan.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/security-scan.yaml)

## Technology & Compatibility

[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Kubernetes](https://img.shields.io/badge/kubernetes-1.27+-326CE5.svg?logo=kubernetes&logoColor=white)](https://kubernetes.io)
[![BIND9](https://img.shields.io/badge/BIND9-DNS%20Server-blue)](https://www.isc.org/bind/)
[![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black)](https://www.linux.org/)
[![Docker](https://img.shields.io/badge/Docker-2496ED?logo=docker&logoColor=white)](https://www.docker.com/)

## Security & Compliance

[![SPDX](https://img.shields.io/badge/SPDX-License--Identifier-blue)](https://spdx.dev/)
[![SLSA 3](https://img.shields.io/badge/SLSA-Level%203-blue)](https://slsa.dev)
[![Cosign Signed](https://img.shields.io/badge/releases-signed-brightgreen.svg)](docs/security/SIGNED_RELEASES.md)
[![Commits Signed](https://img.shields.io/badge/commits-signed-brightgreen.svg)](CONTRIBUTING.md#commit-signing-requirements)
[![SBOM](https://img.shields.io/badge/SBOM-CycloneDX-orange)](https://cyclonedx.org/)
[![Trivy](https://img.shields.io/badge/Trivy-Security%20Scanning-blue)](https://trivy.dev/)

## Regulatory Compliance

[![SOX Controls](https://img.shields.io/badge/SOX-Controls%20Documented-purple)](docs/compliance/sox-controls.md)
[![NIST 800-53](https://img.shields.io/badge/NIST%20800--53-94%25%20Compliant-blue)](docs/compliance/nist-800-53.md)
[![CIS Kubernetes](https://img.shields.io/badge/CIS%20Kubernetes-Level%201%20(84%25)-green)](docs/compliance/cis-kubernetes.md)
[![FIPS 140-2](https://img.shields.io/badge/FIPS%20140--2-Compatible-blue)](docs/compliance/fips.md)

## Community & Support

[![Issues](https://img.shields.io/github/issues/firestoned/bindy)](https://github.com/firestoned/bindy/issues)
[![Pull Requests](https://img.shields.io/github/issues-pr/firestoned/bindy)](https://github.com/firestoned/bindy/pulls)
[![Contributors](https://img.shields.io/github/contributors/firestoned/bindy)](https://github.com/firestoned/bindy/graphs/contributors)
[![Stars](https://img.shields.io/github/stars/firestoned/bindy?style=social)](https://github.com/firestoned/bindy/stargazers)

---

**Declarative DNS management for Kubernetes.** Manage BIND9 infrastructure as code using Custom Resources. Built in Rust for high performance and security.

> **Built for Regulated Environments**: Full SOX, NIST 800-53, and CIS compliance documentation. Designed for banking and financial services.

---

## What is Bindy?

Bindy is a Kubernetes operator that manages BIND9 DNS infrastructure declaratively. Instead of manually configuring DNS servers, you define DNS resources in YAML and Bindy handles:
- Deploying and configuring BIND9 instances
- Creating DNS zones with SOA records
- Adding/updating DNS records dynamically via RNDC
- Platform-managed DNS accessible cluster-wide

## Quick Example

> **⚠️ Breaking Change from v0.2.x**: Records now use **label selectors** instead of `zoneRef`. Zones select records via `recordsFrom` using labels. See [Migration Guide](https://firestoned.github.io/bindy/operations/migration-guide.html) for upgrading from v0.2.x.

```yaml
# 1. Create a DNS cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: my-dns
  namespace: dns-system
spec:
  primary:
    replicas: 2
  secondary:
    replicas: 2

---
# 2. Create a zone with label selector
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: my-dns
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com

---
# 3. Add DNS records with matching labels
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
  namespace: dns-system
  labels:
    zone: example.com  # Selected by DNSZone
spec:
  name: www
  ipv4Address: "192.0.2.1"
```

Apply and you're done:
```bash
kubectl apply -f dns.yaml
dig @<dns-service-ip> www.example.com  # Works!
```

## Architecture

### Custom Resources (CRDs)

Bindy provides 4 types of resources:

#### **Infrastructure**
| Resource | Purpose |
|----------|---------|
| **Bind9Cluster** | Logical DNS cluster (manages multiple instances) |
| **ClusterBind9Provider** | Cluster-scoped DNS infrastructure (platform-managed) |
| **Bind9Instance** | Individual BIND9 server deployment |

#### **DNS Management**
| Resource | Purpose |
|----------|---------|
| **DNSZone** | DNS zone with SOA record |
| **ARecord** | IPv4 address (A) |
| **AAAARecord** | IPv6 address (AAAA) |
| **CNAMERecord** | Alias (CNAME) |
| **MXRecord** | Mail server (MX) |
| **TXTRecord** | Text data (TXT, SPF, DKIM) |
| **NSRecord** | Nameserver delegation (NS) |
| **SRVRecord** | Service location (SRV) |
| **CAARecord** | Certificate authority authorization (CAA) |

### How It Works

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
│ Bind9Cluster    │────▶│ Bind9Instance    │────▶│ BIND9 Pod   │
│ (Logical)       │     │ (Physical)       │     │ + Service   │
└─────────────────┘     └──────────────────┘     └─────────────┘
                              │
                              ▼
┌─────────────────┐     ┌──────────────────┐
│ DNSZone         │────▶│ Zone on BIND9    │
│ (example.com)   │     │ via RNDC         │
└─────────────────┘     └──────────────────┘
        │
        ▼
┌─────────────────┐     ┌──────────────────┐
│ ARecord         │────▶│ DNS Record       │
│ (www)           │     │ via RNDC         │
└─────────────────┘     └──────────────────┘
```

**Key Points:**
- **Bind9Cluster** creates and manages **Bind9Instance** resources automatically
- **Bind9Instance** generates Kubernetes Deployments, Services, ConfigMaps, and Secrets
- **DNSZone** creates the zone on target BIND9 instances
- **Record resources** add DNS records dynamically (no zone file edits!)

### Cluster-Scoped DNS with ClusterBind9Provider

For platform-managed DNS infrastructure accessible from all namespaces:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: shared-dns
  # No namespace - cluster-scoped resource
spec:
  version: "9.18"
  primary:
    replicas: 3
  secondary:
    replicas: 2
```

Application teams can then reference this global cluster from any namespace using `clusterProviderRef`. See [Multi-Tenancy Guide](https://firestoned.github.io/bindy/guide/multi-tenancy.html) for details.

## Installation

### 1. Install CRDs
```bash
kubectl create namespace dns-system
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml
```

### 2. Install RBAC
```bash
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/serviceaccount.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/role.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/rolebinding.yaml
```

### 3. Deploy Operator
```bash
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/operator/deployment.yaml
```

### 4. Verify
```bash
kubectl wait --for=condition=available --timeout=300s deployment/bind9-operator -n dns-system
```

That's it! Now create DNS resources.

**See the [Installation Guide](https://firestoned.github.io/bindy/installation/installation.html) for more options.**

## Usage Examples

### Simple DNS Cluster

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: simple-dns
  namespace: dns-system
spec:
  primary:
    replicas: 1
```

Creates 1 primary BIND9 instance. No secondaries needed for dev/test.

### Production DNS Cluster

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: prod-dns
  namespace: dns-system
spec:
  global:
    recursion: false
    allowQuery: ["0.0.0.0/0"]
    allowTransfer: ["10.0.0.0/8"]
  primary:
    replicas: 3
    config:
      dnssec:
        enabled: true
  secondary:
    replicas: 2
```

Creates 3 primaries + 2 secondaries with DNSSEC enabled and zone transfers configured.

### DNS Zone with Records

```yaml
# Zone with label selector
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: prod-dns
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin.example.com.
  ttl: 3600

---
# A Record
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300

---
# CNAME
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: blog
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: blog
  target: www.example.com.

---
# MX Record
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mail
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  priority: 10
  mailServer: mail.example.com.

---
# TXT (SPF)
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: spf
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  text:
    - "v=spf1 include:_spf.example.com ~all"
```

### Standalone Instance (Advanced)

Skip the cluster abstraction and create a single instance directly:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: standalone
  namespace: dns-system
spec:
  replicas: 1
  version: "9.18"
  role: primary
  config:
    recursion: false
    allowQuery: ["0.0.0.0/0"]
```

Useful for testing or when you need full control over a single instance.

## Key Features

✅ **Declarative** - Manage DNS as Kubernetes resources (GitOps-ready)
✅ **Dynamic Updates** - Records added via RNDC (no zone file restarts)
✅ **High Performance** - Written in Rust, minimal overhead
✅ **Cluster-Scoped** - ClusterBind9Provider for platform-managed DNS
✅ **DNSSEC** - Automatic key management and zone signing
✅ **High Availability** - Leader election, automatic failover
✅ **Compliance** - SOX, NIST 800-53, CIS documented
✅ **Secure** - Non-root containers, RBAC, signed releases

## Configuration

The operator is configured via environment variables in the deployment:

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (`debug`, `info`, `warn`, `error`) |
| `BINDY_ENABLE_LEADER_ELECTION` | `true` | Enable leader election for HA |
| `BINDY_LEASE_DURATION_SECONDS` | `15` | Lease duration |

See [deployment.yaml](deploy/operator/deployment.yaml) for all options.

## Monitoring

Check resource status:
```bash
# Clusters
kubectl get bind9clusters -n dns-system

# Instances
kubectl get bind9instances -n dns-system

# Zones
kubectl get dnszones -n dns-system

# Records
kubectl get arecords,cnamerecords,mxrecords,txtrecords -n dns-system
```

View detailed status:
```bash
kubectl describe arecord www -n dns-system
```

Output includes annotations showing cluster, instance, and zone:
```yaml
metadata:
  annotations:
    bindy.firestoned.io/cluster: prod-dns
    bindy.firestoned.io/instance: primary-0
    bindy.firestoned.io/zone: example.com
status:
  conditions:
    - type: Ready
      status: "True"
      reason: RecordCreated
      message: A record www.example.com created successfully
```

## Troubleshooting

**Operator logs:**
```bash
kubectl logs -n dns-system -l app=bindy -f
```

**Test DNS resolution:**
```bash
# Get service IP
kubectl get svc -n dns-system

# Query DNS
dig @<service-ip> www.example.com A
```

**Verify BIND9 config:**
```bash
# Find BIND9 pod
kubectl get pods -n dns-system -l app.kubernetes.io/name=bind9

# Check config
kubectl exec -it <pod> -n dns-system -- named-checkconf /etc/bind/named.conf
```

**Common issues:**
- Records not appearing? Check `kubectl describe <record>` for error status
- BIND9 not starting? Check RNDC key in Secret: `kubectl get secret -n dns-system`
- Cluster not creating instances? Check Bind9Cluster status: `kubectl describe bind9cluster`

## Documentation

📚 **Complete docs:** [https://firestoned.github.io/bindy/](https://firestoned.github.io/bindy/)

Includes:
- Installation guide
- Multi-tenancy patterns
- High availability setup
- DNSSEC configuration
- Compliance documentation (SOX, NIST, CIS)
- API reference

## Development

**Prerequisites:**
- Rust 1.85+
- Kubernetes 1.27+

**Build & Test:**
```bash
cargo build
cargo test
```

See the [Developer Guide](https://firestoned.github.io/bindy/development/setup.html) for detailed development instructions.

## Contributing

Contributions welcome! Please:
1. Sign commits with GPG/SSH (required for compliance)
2. Run `cargo fmt` and `cargo clippy`
3. Add tests for new features
4. Update CHANGELOG.md

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## Security

- **Signed Releases**: All releases signed with Cosign (keyless). [Verify releases →](docs/security/SIGNED_RELEASES.md)
- **SLSA Level 3**: Build provenance for supply chain security
- **SBOM**: CycloneDX SBOM included with every release
- **Vulnerability Scanning**: Daily `cargo audit` runs

Report security issues to: security@firestoned.io

## License

MIT License - see [LICENSE](LICENSE)

**Copyright (c) 2025 Erick Bourgeois, firestoned**

---

**Need help?**
- [GitHub Issues](https://github.com/firestoned/bindy/issues)
- [Documentation](https://firestoned.github.io/bindy/)
