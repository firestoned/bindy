# Bind9Cluster

The `Bind9Cluster` resource represents a logical DNS cluster - a collection of related BIND9 instances with shared configuration.

## Overview

A Bind9Cluster defines cluster-level configuration that can be inherited by multiple Bind9Instance resources:
- Shared BIND9 version and container image
- Common configuration (recursion, ACLs, etc.)
- Custom ConfigMap references for BIND9 configuration files
- TSIG keys for authenticated zone transfers
- Access Control Lists (ACLs)

## Example

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"
  rndcSecretRefs:
    - name: transfer-key
      algorithm: hmac-sha256
      secret: "base64-encoded-secret"
  acls:
    internal:
      - "10.0.0.0/8"
      - "172.16.0.0/12"
    external:
      - "0.0.0.0/0"
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ClusterConfigured
      message: "Cluster configured successfully"
  instanceCount: 4
  readyInstances: 4
```

## Specification

### Optional Fields

- `spec.version` - BIND9 version for all instances in the cluster
- `spec.image` - Container image configuration for all instances
  - `image` - Full container image reference (registry/repo:tag)
  - `imagePullPolicy` - Image pull policy (Always, IfNotPresent, Never)
  - `imagePullSecrets` - List of secret names for private registries
- `spec.configMapRefs` - Custom ConfigMap references for BIND9 configuration
  - `namedConf` - Name of ConfigMap containing named.conf
  - `namedConfOptions` - Name of ConfigMap containing named.conf.options
- `spec.config` - Shared BIND9 configuration
  - `recursion` - Enable/disable recursion globally
  - `allowQuery` - List of CIDR ranges allowed to query
  - `allowTransfer` - List of CIDR ranges allowed zone transfers
  - `dnssec` - DNSSEC configuration
  - `forwarders` - DNS forwarders
  - `listenOn` - IPv4 addresses to listen on
  - `listenOnV6` - IPv6 addresses to listen on
- `spec.tsigKeys` - TSIG keys for authenticated zone transfers
  - `name` - Key name
  - `algorithm` - HMAC algorithm (hmac-sha256, hmac-sha512, etc.)
  - `secret` - Base64-encoded shared secret
- `spec.acls` - Named ACL definitions that instances can reference

## Cluster vs Instance

The relationship between Bind9Cluster and Bind9Instance:

```yaml
# Cluster defines shared configuration
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: prod-cluster
spec:
  version: "9.18"
  config:
    recursion: false
  acls:
    internal:
      - "10.0.0.0/8"

---
# Instance references the cluster
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  labels:
    cluster: prod-cluster
    dns-role: primary
spec:
  clusterRef: prod-cluster
  replicas: 2
  # Instance-specific config can override cluster defaults
  config:
    allowQuery:
      - acl:internal  # Reference the cluster's ACL
```

## TSIG Keys

TSIG (Transaction SIGnature) keys provide authenticated zone transfers:

```yaml
spec:
  rndcSecretRefs:
    - name: primary-secondary-key
      algorithm: hmac-sha256
      secret: "K8x...base64...=="
    - name: backup-key
      algorithm: hmac-sha512
      secret: "L9y...base64...=="
```

These keys are used by:
- Primary instances for authenticated zone transfers to secondaries
- Secondary instances to authenticate when requesting zone transfers
- Dynamic DNS updates (if enabled)

## Access Control Lists (ACLs)

ACLs define reusable network access policies:

```yaml
spec:
  acls:
    # Internal networks
    internal:
      - "10.0.0.0/8"
      - "172.16.0.0/12"
      - "192.168.0.0/16"

    # External clients
    external:
      - "0.0.0.0/0"

    # Secondary DNS servers
    secondaries:
      - "10.0.1.10"
      - "10.0.2.10"
      - "10.0.3.10"
```

Instances can then reference these ACLs:

```yaml
# In Bind9Instance spec
config:
  allowQuery:
    - acl:external
  allowTransfer:
    - acl:secondaries
```

## Status

The controller updates status to reflect cluster state:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ClusterConfigured
      message: "Cluster configured with 4 instances"
  instanceCount: 4      # Total instances in cluster
  readyInstances: 4     # Instances reporting ready
  observedGeneration: 1
```

## Use Cases

### Multi-Region DNS Cluster

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: global-dns
spec:
  version: "9.18"
  config:
    recursion: false
    dnssec:
      enabled: true
      validation: true
  rndcSecretRefs:
    - name: region-sync-key
      algorithm: hmac-sha256
      secret: "..."
  acls:
    us-east:
      - "10.1.0.0/16"
    us-west:
      - "10.2.0.0/16"
    eu-west:
      - "10.3.0.0/16"
```

### Development Cluster

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: dev-dns
  namespace: dns-system
spec:
  version: "9.18"
  config:
    recursion: true  # Allow recursion for dev
    allowQuery:
      - "0.0.0.0/0"
    forwarders:
      - "8.8.8.8"
      - "8.8.4.4"
  acls:
    dev-team:
      - "192.168.1.0/24"
```

### Custom Image Cluster

Use a custom container image across all instances:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: custom-image-cluster
  namespace: dns-system
spec:
  version: "9.18"
  # Custom image with organization-specific patches
  image:
    image: "my-registry.example.com/bind9:9.18-custom"
    imagePullPolicy: "IfNotPresent"
    imagePullSecrets:
      - docker-registry-secret
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

All Bind9Instances referencing this cluster will inherit the custom image configuration unless they override it.

### Custom ConfigMap Cluster

Share custom BIND9 configuration files across all instances:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: shared-bind9-options
  namespace: dns-system
data:
  named.conf.options: |
    options {
      directory "/var/cache/bind";
      recursion no;
      allow-query { any; };
      allow-transfer { 10.0.2.0/24; };
      dnssec-validation auto;

      # Custom logging
      querylog yes;

      # Rate limiting
      rate-limit {
        responses-per-second 10;
        window 5;
      };
    };
---
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: custom-config-cluster
  namespace: dns-system
spec:
  version: "9.18"
  configMapRefs:
    namedConfOptions: "shared-bind9-options"
```

All instances in this cluster will use the custom configuration, while named.conf is auto-generated.

## Best Practices

1. **One cluster per environment** - Separate clusters for production, staging, development
2. **Consistent TSIG keys** - Use the same keys across all instances in a cluster
3. **Version pinning** - Specify exact BIND9 versions to avoid unexpected updates
4. **ACL organization** - Define ACLs at cluster level for consistency
5. **DNSSEC** - Enable DNSSEC at the cluster level for all zones
6. **Image management** - Define container images at cluster level for consistency; override at instance level only for canary testing
7. **ConfigMap strategy** - Use cluster-level ConfigMaps for shared configuration; use instance-level ConfigMaps for instance-specific customizations
8. **Image pull secrets** - Configure imagePullSecrets at cluster level to avoid duplicating secrets across instances

## Next Steps

- [Bind9Instance](./bind9instance.md) - Learn about DNS instances
- [DNSZone](./dnszone.md) - Learn about DNS zones
- [Multi-Region Setup](../guide/multi-region.md) - Deploy across multiple regions
