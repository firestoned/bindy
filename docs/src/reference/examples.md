# Configuration Examples

Complete configuration examples for common Bindy deployment scenarios.

## Overview

This section provides ready-to-use YAML configurations for various deployment scenarios:

- [Simple Setup](./examples-simple.md) - Single instance, single zone
- [Production Setup](./examples-production.md) - HA, monitoring, backups
- [Multi-Region Setup](./examples-multi-region.md) - Geographic distribution

## Quick Reference

### Minimal Configuration

Minimal viable configuration for testing:

```yaml
# Bind9Instance
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: dns
  namespace: dns-system
  labels:
    dns-role: primary
spec:
  replicas: 1
---
# DNSZone
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: "example.com"
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "admin@example.com"
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
---
# A Record
apiVersion: dns.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www
  namespace: dns-system
spec:
  zone: "example-com"
  name: "www"
  ipv4Address: "192.0.2.1"
```

### Common Patterns

#### Primary/Secondary Setup

```yaml
# Primary
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary
  labels:
    dns-role: primary
spec:
  replicas: 2
  config:
    allowTransfer:
      - "10.0.2.0/24"  # Secondary network
---
# Secondary
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary
  labels:
    dns-role: secondary
spec:
  replicas: 2
---
# Zone on Primary
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-primary
spec:
  zoneName: "example.com"
  zoneType: "primary"
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "admin@example.com"
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
---
# Zone on Secondary
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-secondary
spec:
  zoneName: "example.com"
  zoneType: "secondary"
  instanceSelector:
    matchLabels:
      dns-role: secondary
  secondaryConfig:
    primaryServers:
      - "10.0.1.10"
      - "10.0.1.11"
```

#### DNSSEC Enabled

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: dnssec-instance
spec:
  replicas: 2
  config:
    dnssec:
      enabled: true
      validation: true
```

#### Split Horizon DNS

```yaml
# Internal DNS
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: internal-dns
  labels:
    dns-view: internal
spec:
  config:
    allowQuery:
      - "10.0.0.0/8"
---
# External DNS
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: external-dns
  labels:
    dns-view: external
spec:
  config:
    allowQuery:
      - "0.0.0.0/0"
```

## Resource Organization

### Namespace Structure

**Recommended namespace organization**:

```yaml
# Separate namespaces by environment
dns-system-prod      # Production DNS
dns-system-staging   # Staging DNS
dns-system-dev       # Development DNS
```

### Label Strategy

**Recommended labels**:

```yaml
metadata:
  labels:
    # Core labels
    app.kubernetes.io/name: bindy
    app.kubernetes.io/component: dns-server
    app.kubernetes.io/part-of: dns-infrastructure

    # Custom labels
    dns-role: primary              # primary, secondary, resolver
    environment: production         # production, staging, dev
    region: us-east-1              # Geographic region
    zone-type: authoritative       # authoritative, recursive
```

### Naming Conventions

**Recommended naming**:

```yaml
# Bind9Instance: <role>-<region>
name: primary-us-east-1

# DNSZone: <domain-with-dashes>
name: example-com

# Records: <name>-<type>-<identifier>
name: www-a-record
name: mail-mx-primary
```

## Testing Configurations

### Local Development (kind/minikube)

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: dev-dns
  namespace: dns-system
spec:
  replicas: 1
  config:
    recursion: true
    forwarders:
      - "8.8.8.8"
    allowQuery:
      - "0.0.0.0/0"
```

### CI/CD Testing

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: ci-dns
  namespace: ci-testing
  labels:
    ci-test: "true"
spec:
  replicas: 1
  config:
    recursion: false
    allowQuery:
      - "10.0.0.0/8"
```

## Troubleshooting Examples

### Debug Configuration

Enable verbose logging:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: bindy-config
data:
  RUST_LOG: "debug"
  RECONCILE_INTERVAL: "60"
```

### Dry Run Testing

Test configuration without applying:

```bash
kubectl apply --dry-run=client -f dns-config.yaml
kubectl apply --dry-run=server -f dns-config.yaml
```

### Validation

Validate resources:

```bash
# Check instance status
kubectl get bind9instances -A

# Check zone status
kubectl get dnszones -A

# Check all DNS records
kubectl get arecords,aaaarecords,cnamerecords,mxrecords,txtrecords -A
```

## Complete Examples

For complete, production-ready configurations see:

- [Simple Setup](./examples-simple.md) - Complete single-instance setup
- [Production Setup](./examples-production.md) - Full production configuration with HA
- [Multi-Region Setup](./examples-multi-region.md) - Multi-region deployment

## Related Resources

- [API Reference](./api.md)
- [Bind9Instance Specification](./bind9instance-spec.md)
- [DNSZone Specification](./dnszone-spec.md)
- [Record Specifications](./record-specs.md)
- [Quick Start Guide](../installation/quickstart.md)
