# Summary

[Introduction](./introduction.md)

# Getting Started

- [Installation](./installation/installation.md)
  - [Prerequisites](./installation/prerequisites.md)
  - [Quick Start](./installation/quickstart.md)
  - [Installing CRDs](./installation/crds.md)
  - [Deploying the Controller](./installation/controller.md)
- [Basic Concepts](./concepts/concepts.md)
  - [Architecture Overview](./concepts/architecture.md)
  - [Technical Architecture](./concepts/architecture-technical.md)
  - [Custom Resource Definitions](./concepts/crds.md)
  - [Bind9Instance](./concepts/bind9instance.md)
  - [DNSZone](./concepts/dnszone.md)
  - [DNS Records](./concepts/records.md)

# User Guide

- [Creating DNS Infrastructure](./guide/infrastructure.md)
  - [Primary DNS Instances](./guide/primary-instance.md)
  - [Secondary DNS Instances](./guide/secondary-instance.md)
  - [Multi-Region Setup](./guide/multi-region.md)
- [Managing DNS Zones](./guide/zones.md)
  - [Creating Zones](./guide/creating-zones.md)
  - [Label Selectors](./guide/label-selectors.md)
  - [Zone Configuration](./guide/zone-config.md)
- [Managing DNS Records](./guide/records-guide.md)
  - [A Records (IPv4)](./guide/a-records.md)
  - [AAAA Records (IPv6)](./guide/aaaa-records.md)
  - [CNAME Records](./guide/cname-records.md)
  - [MX Records](./guide/mx-records.md)
  - [TXT Records](./guide/txt-records.md)
  - [NS Records](./guide/ns-records.md)
  - [SRV Records](./guide/srv-records.md)
  - [CAA Records](./guide/caa-records.md)

# Operations

- [Configuration](./operations/configuration.md)
  - [Environment Variables](./operations/env-vars.md)
  - [RBAC](./operations/rbac.md)
  - [Resource Limits](./operations/resources.md)
- [Monitoring](./operations/monitoring.md)
  - [Status Conditions](./operations/status.md)
  - [Logging](./operations/logging.md)
  - [Metrics](./operations/metrics.md)
- [Troubleshooting](./operations/troubleshooting.md)
  - [Common Issues](./operations/common-issues.md)
  - [Debugging](./operations/debugging.md)
  - [FAQ](./operations/faq.md)

# Advanced Topics

- [High Availability](./advanced/ha.md)
  - [Zone Transfers](./advanced/zone-transfers.md)
  - [Replication](./advanced/replication.md)
- [Security](./advanced/security.md)
  - [DNSSEC](./advanced/dnssec.md)
  - [Access Control](./advanced/access-control.md)
- [Performance](./advanced/performance.md)
  - [Tuning](./advanced/tuning.md)
  - [Benchmarking](./advanced/benchmarking.md)
- [Integration](./advanced/integration.md)
  - [External DNS](./advanced/external-dns.md)
  - [Service Discovery](./advanced/service-discovery.md)

# Developer Guide

- [Development Setup](./development/setup.md)
  - [Building from Source](./development/building.md)
  - [Running Tests](./development/testing.md)
  - [Testing Guide](./development/testing-guide.md)
  - [Test Coverage](./development/test-coverage.md)
  - [Development Workflow](./development/workflow.md)
  - [GitHub Pages Setup](./development/github-pages-setup.md)
- [Architecture Deep Dive](./development/architecture-deep-dive.md)
  - [Controller Design](./development/controller-design.md)
  - [Reconciliation Logic](./development/reconciliation.md)
  - [BIND9 Integration](./development/bind9-integration.md)
- [Contributing](./development/contributing.md)
  - [Code Style](./development/code-style.md)
  - [Testing Guidelines](./development/testing-guidelines.md)
  - [Pull Request Process](./development/pr-process.md)

# Reference

- [API Reference](./reference/api.md)
  - [Bind9Instance Spec](./reference/bind9instance-spec.md)
  - [DNSZone Spec](./reference/dnszone-spec.md)
  - [Record Specs](./reference/record-specs.md)
  - [Status Conditions](./reference/status-conditions.md)
- [Examples](./reference/examples.md)
  - [Simple Setup](./reference/examples-simple.md)
  - [Production Setup](./reference/examples-production.md)
  - [Multi-Region Setup](./reference/examples-multi-region.md)

---

[API Documentation (rustdoc)](./rustdoc.md)
[Changelog](./changelog.md)
[License](./license.md)
