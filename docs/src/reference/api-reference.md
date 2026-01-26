# API Reference

Complete API reference documentation for the Bindy operator.

## Custom Resource Definitions (CRDs)

Detailed API documentation for all Custom Resources:

- **[Bind9Cluster API](../concepts/bind9cluster.md)** - DNS cluster within a single Kubernetes cluster
- **[ClusterBind9Provider API](../concepts/clusterbind9provider.md)** - Provider reference for cross-cluster operations
- **[Bind9Instance API](../concepts/bind9instance.md)** - Individual BIND9 DNS server instances
- **[DNSZone API](../concepts/dnszone.md)** - DNS zone management
- **[DNS Records API](../concepts/records.md)** - DNS record types (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)

## Full CRD Specifications

Auto-generated from Rust source code:

- **[Complete API Specification](api.md)** - Full CRD schemas with all fields and validation

## Operator API

For information about the Rust operator internals:

- **[Rustdoc API Documentation](../rustdoc.md)** - Complete Rust API documentation

## Status Conditions

All Custom Resources report status using Kubernetes standard conditions:

- **[Status Conditions Reference](status-conditions.md)** - Condition types, reasons, and meanings

## Examples

Practical examples of using the APIs:

- **[Examples Overview](examples.md)** - Collection of example manifests
- **[Simple Setup](examples-simple.md)** - Basic single-region setup
- **[Production Setup](examples-production.md)** - Multi-region HA setup
- **[Multi-Region Setup](examples-multi-region.md)** - Cross-cluster DNS

## Specifications by Resource

### Bind9Cluster

See [Bind9Cluster Spec](bind9cluster-spec.md) for detailed field documentation.

### Bind9Instance

See [Bind9Instance Spec](bind9instance-spec.md) for detailed field documentation.

### DNSZone

See [DNSZone Spec](dnszone-spec.md) for detailed field documentation.

### DNS Records

See [DNS Records Spec](record-specs.md) for all supported record types.

## API Versioning

Current API version: `v1beta1`

!!! note "Beta API"
    The API is currently in beta (`v1beta1`). Breaking changes may occur, but will be documented in the [Changelog](../changelog.md) with migration guides.

## Validation

All CRDs include OpenAPI v3 validation schemas. Invalid resources will be rejected by the Kubernetes API server before reaching the operator.

## Related Documentation

- [Basic Concepts](../concepts/index.md) - Understanding the resource model
- [User Guide](../guide/architecture.md) - Practical usage patterns
- [Troubleshooting](../operations/troubleshooting.md) - Common API issues
