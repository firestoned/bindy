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
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary
  labels:
    dns-role: secondary
spec:
  replicas: 2
---
# Zone on Primary
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
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

#### Custom Container Image

Using a custom or private container image:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: custom-image-cluster
  namespace: dns-system
spec:
  # Default image for all instances in this cluster
  image:
    image: "my-registry.example.com/bind9:custom-9.18"
    imagePullPolicy: "Always"
    imagePullSecrets:
      - my-registry-secret
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: custom-dns
  namespace: dns-system
spec:
  clusterRef: custom-image-cluster
  replicas: 2
  # Instance inherits custom image from cluster
```

#### Instance-Specific Custom Image

Override cluster image for specific instance:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: prod-cluster
  namespace: dns-system
spec:
  image:
    image: "internetsystemsconsortium/bind9:9.18"
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: canary-dns
  namespace: dns-system
spec:
  clusterRef: prod-cluster
  replicas: 1
  # Override cluster image for canary testing
  image:
    image: "internetsystemsconsortium/bind9:9.19"
    imagePullPolicy: "Always"
```

#### Custom Configuration Files

Using custom ConfigMaps for BIND9 configuration:

```yaml
# Create custom ConfigMap
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-custom-named-conf
  namespace: dns-system
data:
  named.conf: |
    // Custom BIND9 configuration
    include "/etc/bind/named.conf.options";
    include "/etc/bind/zones/named.conf.zones";

    logging {
      channel query_log {
        file "/var/log/named/queries.log" versions 5 size 10m;
        severity info;
        print-time yes;
        print-category yes;
      };
      category queries { query_log; };
      category lame-servers { null; };
    };
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-custom-options
  namespace: dns-system
data:
  named.conf.options: |
    options {
      directory "/var/cache/bind";
      recursion no;
      allow-query { any; };
      allow-transfer { 10.0.2.0/24; };
      dnssec-validation auto;
      listen-on { any; };
      listen-on-v6 { any; };
      max-cache-size 256M;
      max-cache-ttl 3600;
    };
---
# Reference custom ConfigMaps
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: custom-config-dns
  namespace: dns-system
spec:
  replicas: 2
  configMapRefs:
    namedConf: "my-custom-named-conf"
    namedConfOptions: "my-custom-options"
```

#### Cluster-Level Custom ConfigMaps

Share custom configuration across all instances:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: shared-options
  namespace: dns-system
data:
  named.conf.options: |
    options {
      directory "/var/cache/bind";
      recursion no;
      allow-query { any; };
      dnssec-validation auto;
    };
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: shared-config-cluster
  namespace: dns-system
spec:
  configMapRefs:
    namedConfOptions: "shared-options"
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: instance-1
  namespace: dns-system
spec:
  clusterRef: shared-config-cluster
  replicas: 2
  # Inherits configMapRefs from cluster
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: instance-2
  namespace: dns-system
spec:
  clusterRef: shared-config-cluster
  replicas: 2
  # Also inherits same configMapRefs from cluster
```

#### Split Horizon DNS

```yaml
# Internal DNS
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
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
apiVersion: bindy.firestoned.io/v1alpha1
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
