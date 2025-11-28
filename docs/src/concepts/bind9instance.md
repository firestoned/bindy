# Bind9Instance

The `Bind9Instance` resource represents a BIND9 DNS server deployment in Kubernetes.

## Overview

A Bind9Instance defines:
- Number of replicas
- BIND9 version and container image
- Configuration options (or custom ConfigMap references)
- Network settings
- Labels for targeting
- Optional cluster reference for inheriting shared configuration

## Example

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
  labels:
    dns-role: primary
    environment: production
    datacenter: us-east
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Running
      message: "2 replicas running"
  readyReplicas: 2
  currentVersion: "9.18"
```

## Specification

### Optional Fields

All fields are optional. If no `clusterRef` is specified, default values are used.

- `spec.clusterRef` - Reference to a Bind9Cluster for inheriting shared configuration
- `spec.replicas` - Number of BIND9 pods (default: 1)
- `spec.version` - BIND9 version to deploy (default: "9.18", or inherit from cluster)
- `spec.image` - Container image configuration (inherits from cluster if not specified)
  - `image` - Full container image reference
  - `imagePullPolicy` - Image pull policy (Always, IfNotPresent, Never)
  - `imagePullSecrets` - List of secret names for private registries
- `spec.configMapRefs` - Custom ConfigMap references (inherits from cluster if not specified)
  - `namedConf` - ConfigMap name containing named.conf
  - `namedConfOptions` - ConfigMap name containing named.conf.options
- `spec.config` - BIND9 configuration options (inherits from cluster if not specified)
  - `recursion` - Enable/disable recursion (default: false)
  - `allowQuery` - List of CIDR ranges allowed to query
  - `allowTransfer` - List of CIDR ranges allowed to transfer zones
  - `dnssec` - DNSSEC configuration
  - `forwarders` - DNS forwarders
  - `listenOn` - IPv4 addresses to listen on
  - `listenOnV6` - IPv6 addresses to listen on

### Configuration Inheritance

When a Bind9Instance references a Bind9Cluster via `clusterRef`:
1. Instance-level settings take precedence
2. If not specified at instance level, cluster settings are used
3. If not specified at cluster level, defaults are used

## Labels and Selectors

Labels on Bind9Instance resources are used by DNSZone resources to target specific instances:

```yaml
# Instance with labels
metadata:
  labels:
    dns-role: primary
    region: us-east
    environment: production

# Zone selecting this instance
spec:
  instanceSelector:
    matchLabels:
      dns-role: primary
      region: us-east
```

## Status

The controller updates status to reflect the instance state:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Running
  readyReplicas: 2
  currentVersion: "9.18"
```

## Use Cases

### Primary DNS Instance

```yaml
metadata:
  labels:
    dns-role: primary
spec:
  replicas: 2
  config:
    allowTransfer:
      - "10.0.0.0/8"  # Allow secondaries to transfer
```

### Secondary DNS Instance

```yaml
metadata:
  labels:
    dns-role: secondary
spec:
  replicas: 2
  config:
    allowTransfer: []  # No transfers from secondary
```

### Instance with Custom Image

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: custom-image-dns
  namespace: dns-system
spec:
  replicas: 2
  image:
    image: "my-registry.example.com/bind9:9.18-patched"
    imagePullPolicy: "Always"
    imagePullSecrets:
      - my-registry-secret
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

### Instance with Custom ConfigMaps

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: custom-dns-config
  namespace: dns-system
data:
  named.conf.options: |
    options {
      directory "/var/cache/bind";
      recursion no;
      allow-query { any; };

      # Custom rate limiting
      rate-limit {
        responses-per-second 10;
      };
    };
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: custom-config-dns
  namespace: dns-system
spec:
  replicas: 2
  configMapRefs:
    namedConfOptions: "custom-dns-config"
```

### Instance Inheriting from Cluster

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: prod-cluster
  namespace: dns-system
spec:
  version: "9.18"
  image:
    image: "internetsystemsconsortium/bind9:9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: prod-instance-1
  namespace: dns-system
spec:
  clusterRef: prod-cluster
  replicas: 2
  # Inherits version, image, and config from cluster
```

### Canary Instance with Override

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: canary-instance
  namespace: dns-system
spec:
  clusterRef: prod-cluster  # Inherits most settings from cluster
  replicas: 1
  # Override image for canary testing
  image:
    image: "internetsystemsconsortium/bind9:9.19-beta"
    imagePullPolicy: "Always"
```

## Next Steps

- [DNSZone](./dnszone.md) - Learn about DNS zones
- [Primary Instances](../guide/primary-instance.md) - Deploy primary DNS
- [Secondary Instances](../guide/secondary-instance.md) - Deploy secondary DNS
