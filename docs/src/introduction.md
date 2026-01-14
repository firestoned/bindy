# Introduction
### *Pronounced: "bined-ee" (like BIND + ee)*

[![Main Branch CI/CD](https://github.com/firestoned/bindy/actions/workflows/main.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/main.yaml)
[![PR CI](https://github.com/firestoned/bindy/actions/workflows/pr.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/pr.yaml)
[![Integration Tests](https://github.com/firestoned/bindy/actions/workflows/integration.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/integration.yaml)
[![codecov](https://codecov.io/gh/firestoned/bindy/branch/main/graph/badge.svg)](https://codecov.io/gh/firestoned/bindy)

**Bindy** is a high-performance Kubernetes controller written in Rust that manages BIND9 DNS infrastructure through Custom Resource Definitions (CRDs). It enables you to manage DNS zones and records as native Kubernetes resources, bringing the declarative Kubernetes paradigm to DNS management.

## What is Bindy?

Bindy watches for DNS-related Custom Resources in your Kubernetes cluster and automatically generates and manages BIND9 zone configurations. It replaces traditional manual DNS management with a declarative, GitOps-friendly approach.

### Key Features

- **High Performance** - Native Rust implementation with async/await and zero-copy operations
- **RNDC Protocol** - Native BIND9 management via Remote Name Daemon Control (RNDC) with TSIG authentication
- **Label Selectors** - Target specific BIND9 instances using Kubernetes label selectors
- **Dynamic Zone Management** - Automatically create and manage DNS zones using RNDC commands
- **Multi-Record Types** - Support for A, AAAA, CNAME, MX, TXT, NS, SRV, and CAA records
- **Declarative DNS** - Manage DNS as Kubernetes resources with full GitOps support
- **Security First** - TSIG-authenticated RNDC communication, non-root containers, RBAC-ready
- **Status Tracking** - Complete status subresources for all resources
- **Primary/Secondary Support** - Built-in support for primary and secondary DNS architectures with zone transfers

## Why Bindy?

Traditional DNS management involves:
- Manual editing of zone files
- SSH access to DNS servers
- No audit trail or version control
- Difficult disaster recovery
- Complex multi-region setups

Bindy transforms this by:
- Managing DNS as Kubernetes resources
- Full GitOps workflow support
- Native RNDC protocol for direct BIND9 control
- Built-in audit trail via Kubernetes events
- Simple disaster recovery (backup your CRDs)
- Seamless multi-region DNS distribution with zone transfers

## Who Should Use Bindy?

Bindy is ideal for:
- **Platform Engineers** building internal DNS infrastructure
- **DevOps Teams** managing DNS alongside their Kubernetes workloads
- **SREs** requiring automated, auditable DNS management
- **Organizations** running self-hosted BIND9 DNS servers
- **Multi-region Deployments** needing distributed DNS infrastructure

## Quick Example

Here's how simple it is to create a DNS zone with records:

```yaml
# Create a DNS zone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin@example.com
    serial: 2024010101
  ttl: 3600

---
# Add an A record
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
spec:
  zone: example-com
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

Apply it to your cluster:

```bash
kubectl apply -f dns-config.yaml
```

Bindy automatically:
1. Finds matching BIND9 instances using pod discovery
2. Connects to BIND9 via RNDC protocol (port 9530)
3. Creates zones and records using native RNDC commands
4. Tracks status and conditions in real-time

## Next Steps

- [Installation](./installation/installation.md) - Get started with Bindy
- [Quick Start](./installation/quickstart.md) - Deploy your first DNS zone
- [RNDC-Based Architecture](./concepts/architecture-rndc.md) - Learn about the RNDC protocol architecture
- [Architecture Overview](./concepts/architecture.md) - Understand how Bindy works
- [API Reference](./reference/api.md) - Complete API documentation

## Performance Characteristics

- **Startup Time**: <1 second
- **Memory Usage**: ~50MB baseline
- **Zone Creation Latency**: <500ms per zone (via RNDC)
- **Record Addition Latency**: <200ms per record (via RNDC)
- **RNDC Command Execution**: <100ms typical
- **Controller Overhead**: Negligible CPU when idle

## Project Status

Bindy is actively developed and used in production environments. The project follows semantic versioning and maintains backward compatibility within major versions.

Current version: **v0.1.0**

## Support & Community

- **GitHub Issues**: [Report bugs or request features](https://github.com/firestoned/bindy/issues)
- **GitHub Discussions**: [Ask questions and share ideas](https://github.com/firestoned/bindy/discussions)
- **Documentation**: You're reading it!

## License

Bindy is open-source software licensed under the [MIT License](./license.md).
