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
[![CodeQL](https://github.com/firestoned/bindy/actions/workflows/codeql.yml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/codeql.yml)
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

## Getting Started

### 1. Download the bindy binary

```bash
# Linux (amd64)
curl -Lo bindy https://github.com/firestoned/bindy/releases/latest/download/bindy-linux-amd64
chmod +x bindy && sudo mv bindy /usr/local/bin/

# macOS (arm64)
curl -Lo bindy https://github.com/firestoned/bindy/releases/latest/download/bindy-darwin-arm64
chmod +x bindy && sudo mv bindy /usr/local/bin/
```

### 2. Bootstrap the cluster

This creates the `bindy-system` namespace, installs CRDs, sets up RBAC, and deploys the operator — all in one command:

```bash
bindy bootstrap
```

The operator image tag matches the binary version (e.g. `ghcr.io/firestoned/bindy:v0.5.0`). Override with `--version` if needed.

### 3. Create a BIND9 instance, zone, and record

Save this as `dns.yaml`:

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

# Test it
dig @$(kubectl get svc -n bindy-system -o jsonpath='{.items[0].status.loadBalancer.ingress[0].ip}') www.example.com A
```

That's it. See the [full documentation](https://firestoned.github.io/bindy/) for HA setup, DNSSEC, multi-tenancy, and more.

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

## More Examples

### Simple DNS Cluster

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: simple-dns
  namespace: bindy-system
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
  namespace: bindy-system
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
  namespace: bindy-system
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
  namespace: bindy-system
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
  namespace: bindy-system
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
  namespace: bindy-system
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
  namespace: bindy-system
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
  namespace: bindy-system
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
kubectl get bind9clusters -n bindy-system

# Instances
kubectl get bind9instances -n bindy-system

# Zones
kubectl get dnszones -n bindy-system

# Records
kubectl get arecords,cnamerecords,mxrecords,txtrecords -n bindy-system
```

View detailed status:
```bash
kubectl describe arecord www -n bindy-system
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
kubectl logs -n bindy-system -l app=bindy -f
```

**Test DNS resolution:**
```bash
# Get service IP
kubectl get svc -n bindy-system

# Query DNS
dig @<service-ip> www.example.com A
```

**Verify BIND9 config:**
```bash
# Find BIND9 pod
kubectl get pods -n bindy-system -l app.kubernetes.io/name=bind9

# Check config
kubectl exec -it <pod> -n bindy-system -- named-checkconf /etc/bind/named.conf
```

**Common issues:**
- Records not appearing? Check `kubectl describe <record>` for error status
- BIND9 not starting? Check RNDC key in Secret: `kubectl get secret -n bindy-system`
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

**Security Scanning:**
```bash
make cargo-deny           # Check dependencies & licenses
make gitleaks             # Scan for secrets
make trivy-fs             # Scan filesystem for vulnerabilities
make trivy-k8s            # Scan Kubernetes manifests
make install-git-hooks    # Install pre-commit hooks
make security-scan-full   # Run all security scans
```

See the [Developer Guide](https://firestoned.github.io/bindy/development/setup.html) for detailed development instructions.

## Contributing

Contributions welcome! Please:
1. Sign commits with GPG/SSH (required for compliance)
2. Run `cargo fmt` and `cargo clippy`
3. Add tests for new features
4. Install git hooks: `make install-git-hooks` (prevents committing secrets)
5. Ensure security scans pass: `make security-scan-local`
4. Update CHANGELOG.md

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## Security

- **Signed Releases**: All releases signed with Cosign (keyless). [Verify releases →](docs/security/SIGNED_RELEASES.md)
- **SLSA Level 3**: Build provenance for supply chain security
- **SBOM**: CycloneDX SBOM included with every release
- **Multi-Layer Security Scanning**:
  - **CodeQL**: Static application security testing (SAST) for Rust code
  - **cargo-deny**: Dependency security, license compliance, and supply chain validation
  - **Gitleaks**: Pre-commit and CI secret scanning
  - **Trivy**: Container image and Kubernetes manifest vulnerability scanning
  - **Dependabot**: Automated dependency updates

Report security issues to: security@firestoned.io

## License

MIT License - see [LICENSE](LICENSE)

**Copyright (c) 2025 Erick Bourgeois, firestoned**

---

**Need help?**
- [GitHub Issues](https://github.com/firestoned/bindy/issues)
- [Documentation](https://firestoned.github.io/bindy/)
