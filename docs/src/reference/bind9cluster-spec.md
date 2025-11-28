# Bind9Cluster Specification

Complete specification for the Bind9Cluster Custom Resource Definition.

## Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: string
  namespace: string
spec:
  version: string              # Optional, BIND9 version
  image:                       # Optional, container image config
    image: string
    imagePullPolicy: string
    imagePullSecrets: [string]
  configMapRefs:               # Optional, custom config files
    namedConf: string
    namedConfOptions: string
  config:                      # Optional, shared BIND9 config
    recursion: boolean
    allowQuery: [string]
    allowTransfer: [string]
    dnssec:
      enabled: boolean
      validation: boolean
    forwarders: [string]
    listenOn: [string]
    listenOnV6: [string]
  tsigKeys: [TSIGKey]          # Optional, TSIG keys for zone transfers
  acls:                        # Optional, named ACLs
    name: [string]
```

## Overview

Bind9Cluster defines a logical grouping of BIND9 DNS server instances with shared configuration. It provides centralized management of BIND9 version, container images, and common settings across multiple instances.

**Key Features:**
- Shared version and image configuration
- Centralized BIND9 configuration
- TSIG key management for secure zone transfers
- Named ACLs for access control
- Cluster-wide status reporting

## Spec Fields

### version
**Type**: string
**Required**: No
**Default**: "9.18"

BIND9 version to deploy across all instances in the cluster unless overridden at the instance level.

```yaml
spec:
  version: "9.18"
```

**Supported Versions**:
- "9.16" - Older stable
- "9.18" - Current stable (recommended)
- "9.19" - Development

### image
**Type**: object
**Required**: No

Container image configuration shared by all instances in the cluster.

```yaml
spec:
  image:
    image: "internetsystemsconsortium/bind9:9.18"
    imagePullPolicy: "IfNotPresent"
    imagePullSecrets:
      - my-registry-secret
```

**How It Works**:
- Instances inherit image configuration from the cluster
- Instances can override with their own `image` config
- Simplifies managing container images across multiple instances

#### image.image
**Type**: string
**Required**: No
**Default**: "internetsystemsconsortium/bind9:9.18"

Full container image reference including registry, repository, and tag.

```yaml
spec:
  image:
    image: "my-registry.example.com/bind9:custom"
```

#### image.imagePullPolicy
**Type**: string
**Required**: No
**Default**: "IfNotPresent"

Kubernetes image pull policy.

**Valid Values**:
- `"Always"` - Always pull the image
- `"IfNotPresent"` - Pull only if not present locally (recommended)
- `"Never"` - Never pull, use local image only

#### image.imagePullSecrets
**Type**: array of strings
**Required**: No
**Default**: []

List of Kubernetes secret names for authenticating with private container registries.

```yaml
spec:
  image:
    imagePullSecrets:
      - docker-registry-secret
```

### configMapRefs
**Type**: object
**Required**: No

References to custom ConfigMaps containing BIND9 configuration files shared across the cluster.

```yaml
spec:
  configMapRefs:
    namedConf: "cluster-named-conf"
    namedConfOptions: "cluster-options"
```

**How It Works**:
- Cluster-level ConfigMaps apply to all instances
- Instances can override with their own ConfigMap references
- Useful for sharing common configuration

#### configMapRefs.namedConf
**Type**: string
**Required**: No

Name of ConfigMap containing the main `named.conf` file.

#### configMapRefs.namedConfOptions
**Type**: string
**Required**: No

Name of ConfigMap containing the `named.conf.options` file.

### config
**Type**: object
**Required**: No

Shared BIND9 configuration for all instances in the cluster.

```yaml
spec:
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.2.0/24"
    dnssec:
      enabled: true
      validation: auto
```

**How It Works**:
- All instances inherit cluster configuration
- Instances can override specific settings
- Changes propagate to all instances using cluster config

#### config.recursion
**Type**: boolean
**Required**: No
**Default**: false

Enable recursive DNS queries.

#### config.allowQuery
**Type**: array of strings
**Required**: No
**Default**: ["0.0.0.0/0"]

IP addresses or CIDR blocks allowed to query servers in this cluster.

#### config.allowTransfer
**Type**: array of strings
**Required**: No
**Default**: []

IP addresses or CIDR blocks allowed to perform zone transfers.

#### config.dnssec
**Type**: object
**Required**: No

DNSSEC configuration for the cluster.

##### config.dnssec.enabled
**Type**: boolean
**Required**: No
**Default**: false

Enable DNSSEC signing for zones.

##### config.dnssec.validation
**Type**: boolean
**Required**: No
**Default**: false

Enable DNSSEC validation for recursive queries.

#### config.forwarders
**Type**: array of strings
**Required**: No
**Default**: []

DNS servers to forward queries to (for recursive mode).

```yaml
spec:
  config:
    recursion: true
    forwarders:
      - "8.8.8.8"
      - "1.1.1.1"
```

#### config.listenOn
**Type**: array of strings
**Required**: No
**Default**: ["any"]

IPv4 addresses to listen on.

#### config.listenOnV6
**Type**: array of strings
**Required**: No
**Default**: ["any"]

IPv6 addresses to listen on.

### tsigKeys
**Type**: array of TSIGKey objects
**Required**: No
**Default**: []

TSIG (Transaction Signature) keys for authenticated zone transfers between primary and secondary servers.

```yaml
spec:
  tsigKeys:
    - name: transfer-key
      algorithm: hmac-sha256
      secret: base64-encoded-secret
```

**How It Works**:
- TSIG keys authenticate zone transfers
- Keys are shared across all instances
- Used for secure primary-to-secondary replication

**TSIGKey Fields**:
- `name` (string, required) - Key identifier
- `algorithm` (string, required) - HMAC algorithm (e.g., "hmac-sha256")
- `secret` (string, required) - Base64-encoded shared secret

### acls
**Type**: object (map of string arrays)
**Required**: No
**Default**: {}

Named Access Control Lists that can be referenced in instance configurations.

```yaml
spec:
  acls:
    internal:
      - "10.0.0.0/8"
      - "172.16.0.0/12"
    trusted:
      - "192.168.1.0/24"
    external:
      - "0.0.0.0/0"
```

**How It Works**:
- Define ACLs once at cluster level
- Reference by name in instance configurations
- Simplifies managing access control across instances

**Usage Example**:
```yaml
# In Bind9Instance
spec:
  config:
    allowQuery:
      - "acl:internal"
    allowTransfer:
      - "acl:trusted"
```

## Status Fields

### conditions
**Type**: array of objects

Standard Kubernetes conditions indicating cluster state.

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: AllInstancesReady
      message: "All 3 instances are ready"
      lastTransitionTime: "2024-01-15T10:30:00Z"
```

**Condition Types**:
- **Ready** - Cluster is ready (all instances operational)
- **Degraded** - Some instances are not ready
- **Progressing** - Cluster is being reconciled

### observedGeneration
**Type**: integer

The generation of the resource that was last reconciled.

```yaml
status:
  observedGeneration: 5
```

### instanceCount
**Type**: integer

Total number of Bind9Instance resources referencing this cluster.

```yaml
status:
  instanceCount: 3
```

### readyInstances
**Type**: integer

Number of instances that are ready and serving traffic.

```yaml
status:
  readyInstances: 3
```

## Complete Examples

### Basic Production Cluster

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
      - "10.0.2.0/24"
    dnssec:
      enabled: true
      validation: auto
  tsigKeys:
    - name: transfer-key
      algorithm: hmac-sha256
      secret: "K9x7..." # Base64-encoded
```

### Cluster with Custom Image

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: custom-dns
  namespace: dns-system
spec:
  version: "9.18"
  image:
    image: "my-registry.example.com/bind9:hardened"
    imagePullPolicy: "Always"
    imagePullSecrets:
      - my-registry-secret
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

### Recursive Resolver Cluster

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: resolver-cluster
  namespace: dns-system
spec:
  version: "9.18"
  config:
    recursion: true
    allowQuery:
      - "10.0.0.0/8"  # Internal network only
    forwarders:
      - "8.8.8.8"
      - "8.8.4.4"
      - "1.1.1.1"
    dnssec:
      enabled: false
      validation: true
  acls:
    internal:
      - "10.0.0.0/8"
      - "172.16.0.0/12"
      - "192.168.0.0/16"
```

### Multi-Region Cluster with ACLs

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: global-dns
  namespace: dns-system
spec:
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "acl:secondary-servers"
    dnssec:
      enabled: true
  tsigKeys:
    - name: us-east-transfer
      algorithm: hmac-sha256
      secret: "east-secret-base64"
    - name: us-west-transfer
      algorithm: hmac-sha256
      secret: "west-secret-base64"
    - name: eu-transfer
      algorithm: hmac-sha256
      secret: "eu-secret-base64"
  acls:
    secondary-servers:
      - "10.1.0.0/24"  # US East
      - "10.2.0.0/24"  # US West
      - "10.3.0.0/24"  # EU
    monitoring:
      - "10.0.10.0/24"
```

## Cluster Hierarchy

```
Bind9Cluster
    ├── Defines shared configuration
    ├── Manages TSIG keys
    ├── Defines ACLs
    └── Referenced by one or more Bind9Instances
            ├── Instance inherits cluster config
            ├── Instance can override cluster settings
            └── Instance uses cluster TSIG keys
```

## Configuration Inheritance

When a Bind9Instance references a Bind9Cluster:

1. **Version** - Instance inherits cluster version unless it specifies its own
2. **Image** - Instance inherits cluster image config unless it specifies its own
3. **Config** - Instance inherits cluster config unless it specifies its own
4. **TSIG Keys** - Instance uses cluster TSIG keys for zone transfers
5. **ACLs** - Instance can reference cluster ACLs by name

**Override Priority**: Instance-level config > Cluster-level config > Default values

## Related Resources

- [Bind9Instance Specification](./bind9instance-spec.md) - Individual DNS server instances
- [DNSZone Specification](./dnszone-spec.md) - DNS zones managed by instances
- [Examples](./examples.md) - Complete configuration examples
