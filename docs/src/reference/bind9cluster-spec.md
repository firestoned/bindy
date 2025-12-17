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
  global:                      # Optional, global BIND9 config for all instances
    recursion: boolean
    allowQuery: [string]       # ⚠️ NO DEFAULT - must be explicitly set
    allowTransfer: [string]    # ⚠️ NO DEFAULT - must be explicitly set
    dnssec:
      enabled: boolean
      validation: boolean
    forwarders: [string]
    listenOn: [string]
    listenOnV6: [string]
  rndcSecretRefs: [RndcSecretRef]  # Optional, refs to Secrets with RNDC/TSIG keys
  acls:                        # Optional, named ACLs
    name: [string]
  volumes: [Volume]            # Optional, Kubernetes volumes
  volumeMounts: [VolumeMount]  # Optional, volume mount specifications
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

### global
**Type**: object
**Required**: No

Global BIND9 configuration shared across all instances in the cluster.

> **⚠️ Warning**: There are NO defaults for `allowQuery` and `allowTransfer`. If not specified, BIND9's default behavior applies (no queries or transfers allowed). Always explicitly configure these fields for your security requirements.

```yaml
spec:
  global:
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
- All instances inherit global configuration
- Instances can override specific settings
- Role-specific configuration (primary/secondary) can override global settings
- Changes propagate to all instances using global config

#### global.recursion
**Type**: boolean
**Required**: No
**Default**: false

Enable recursive DNS queries.

#### global.allowQuery
**Type**: array of strings
**Required**: No
**Default**: None (BIND9 default: no queries allowed)

IP addresses or CIDR blocks allowed to query servers in this cluster.

> **⚠️ Warning**: No default value is provided. You must explicitly configure this field or queries will be denied.

#### global.allowTransfer
**Type**: array of strings
**Required**: No
**Default**: None (BIND9 default: no transfers allowed)

IP addresses or CIDR blocks allowed to perform zone transfers.

> **⚠️ Warning**: No default value is provided. You must explicitly configure this field or zone transfers will be denied.

#### global.dnssec
**Type**: object
**Required**: No

DNSSEC configuration for the cluster.

##### global.dnssec.enabled
**Type**: boolean
**Required**: No
**Default**: false

Enable DNSSEC signing for zones.

##### global.dnssec.validation
**Type**: boolean
**Required**: No
**Default**: false

Enable DNSSEC validation for recursive queries.

#### global.forwarders
**Type**: array of strings
**Required**: No
**Default**: []

DNS servers to forward queries to (for recursive mode).

```yaml
spec:
  global:
    recursion: true
    forwarders:
      - "8.8.8.8"
      - "1.1.1.1"
```

#### global.listenOn
**Type**: array of strings
**Required**: No
**Default**: ["any"]

IPv4 addresses to listen on.

#### global.listenOnV6
**Type**: array of strings
**Required**: No
**Default**: ["any"]

IPv6 addresses to listen on.

### rndcSecretRefs
**Type**: array of RndcSecretRef objects
**Required**: No
**Default**: []

References to Kubernetes Secrets containing RNDC/TSIG keys for authenticated zone transfers and RNDC communication.

```yaml
# 1. Create Secret with credentials
apiVersion: v1
kind: Secret
metadata:
  name: transfer-key-secret
type: Opaque
stringData:
  key-name: transfer-key
  secret: base64-encoded-hmac-key

---
# 2. Reference in Bind9Cluster
spec:
  rndcSecretRefs:
    - name: transfer-key-secret
      algorithm: hmac-sha256  # Algorithm specified in CRD
```

**How It Works**:
- RNDC/TSIG keys authenticate zone transfers and RNDC commands
- Keys stored securely in Kubernetes Secrets
- Algorithm specified in CRD for type safety
- Keys are shared across all instances in the cluster

**RndcSecretRef Fields**:
- `name` (string, required) - Name of the Kubernetes Secret
- `algorithm` (RndcAlgorithm, optional) - HMAC algorithm (defaults to hmac-sha256)
  - Supported: `hmac-md5`, `hmac-sha1`, `hmac-sha224`, `hmac-sha256`, `hmac-sha384`, `hmac-sha512`
- `keyNameKey` (string, optional) - Key in secret for key name (defaults to "key-name")
- `secretKey` (string, optional) - Key in secret for secret value (defaults to "secret")

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
  global:
    allowQuery:
      - "acl:internal"
    allowTransfer:
      - "acl:trusted"
```

### volumes
**Type**: array of Kubernetes Volume objects
**Required**: No
**Default**: []

Kubernetes volumes that can be mounted by instances in this cluster.

```yaml
spec:
  volumes:
    - name: zone-data
      persistentVolumeClaim:
        claimName: dns-zone-pvc
    - name: config-override
      configMap:
        name: custom-bind-config
```

**How It Works**:
- Volumes defined at cluster level are inherited by all instances
- Instances can override with their own volumes
- Common use cases include:
  - PersistentVolumeClaims for zone data persistence
  - ConfigMaps for custom configuration files
  - Secrets for sensitive data like TSIG keys
  - EmptyDir for temporary storage

**Volume Types**:
Supports all Kubernetes volume types including:
- `persistentVolumeClaim` - Persistent storage for zone data
- `configMap` - Configuration files
- `secret` - Sensitive data
- `emptyDir` - Temporary storage
- `hostPath` - Host directory (use with caution)
- `nfs` - Network file system

### volumeMounts
**Type**: array of Kubernetes VolumeMount objects
**Required**: No
**Default**: []

Volume mount specifications that define where volumes should be mounted in containers.

```yaml
spec:
  volumes:
    - name: zone-data
      persistentVolumeClaim:
        claimName: dns-zone-pvc
  volumeMounts:
    - name: zone-data
      mountPath: /var/lib/bind
      readOnly: false
```

**How It Works**:
- Volume mounts must reference volumes defined in the `volumes` field
- Each mount specifies the volume name and where to mount it
- Instances inherit cluster-level volume mounts unless overridden
- Mounts are applied to the BIND9 container

**VolumeMount Fields**:
- `name` (string, required) - Volume name to mount (must match a volume)
- `mountPath` (string, required) - Path in container where volume is mounted
- `readOnly` (boolean, optional) - Mount as read-only (default: false)
- `subPath` (string, optional) - Sub-path within the volume

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
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.2.0/24"
    dnssec:
      enabled: true
      validation: auto
  rndcSecretRefs:
    - name: transfer-key-secret
      algorithm: hmac-sha256
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
  global:
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
  global:
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
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "acl:secondary-servers"
    dnssec:
      enabled: true
  rndcSecretRefs:
    - name: us-east-transfer-secret
      algorithm: hmac-sha256
    - name: us-west-transfer-secret
      algorithm: hmac-sha256
    - name: eu-transfer-secret
      algorithm: hmac-sha512  # Different algorithm for EU
  acls:
    secondary-servers:
      - "10.1.0.0/24"  # US East
      - "10.2.0.0/24"  # US West
      - "10.3.0.0/24"  # EU
    monitoring:
      - "10.0.10.0/24"
```

### Cluster with Persistent Storage

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: persistent-dns
  namespace: dns-system
spec:
  version: "9.18"
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    dnssec:
      enabled: true
  # Define persistent volume for zone data
  volumes:
    - name: zone-data
      persistentVolumeClaim:
        claimName: bind-zone-storage
  volumeMounts:
    - name: zone-data
      mountPath: /var/lib/bind
      readOnly: false
```

**Prerequisites**: Create a PersistentVolumeClaim first:

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: bind-zone-storage
  namespace: dns-system
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 10Gi
  storageClassName: fast-ssd
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
