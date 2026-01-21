# Bind9Instance Specification

Complete specification for the Bind9Instance Custom Resource Definition.

## Resource Definition

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: string
  namespace: string
  labels:
    key: value
spec:
  clusterRef: string          # References Bind9Cluster
  role: primary|secondary     # Required: Server role
  replicas: integer
  version: string             # Optional, overrides cluster version
  image:                      # Optional, overrides cluster image
    image: string
    imagePullPolicy: string
    imagePullSecrets: [string]
  configMapRefs:              # Optional, custom config files
    namedConf: string
    namedConfOptions: string
  global:                     # Optional, overrides cluster global config
    recursion: boolean
    allowQuery: [string]
    allowTransfer: [string]
    dnssec:
      enabled: boolean
      validation: boolean
    forwarders: [string]
    listenOn: [string]
    listenOnV6: [string]
  primaryServers: [string]    # Required for secondary role
```

## Spec Fields

### clusterRef
**Type**: string
**Required**: Yes

Name of the Bind9Cluster that this instance belongs to. The instance inherits cluster-level configuration (version, shared config, TSIG keys, ACLs) from the referenced cluster.

```yaml
spec:
  clusterRef: production-dns  # References Bind9Cluster named "production-dns"
```

**How It Works**:
- Instance inherits `version` from cluster unless overridden
- Instance inherits `global` config from cluster unless overridden
- Operator uses cluster TSIG keys for zone transfers
- Instance can override cluster settings with its own spec

### replicas
**Type**: integer
**Required**: No
**Default**: 1

Number of BIND9 pod replicas to run.

```yaml
spec:
  replicas: 3
```

**Best Practices**:
- Use 2+ replicas for high availability
- Use odd numbers (3, 5) for consensus-based systems
- Consider resource constraints when scaling

### version
**Type**: string
**Required**: No
**Default**: "9.18"

BIND9 version to deploy. Must match available Docker image tags.

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

Container image configuration for the BIND9 instance. Overrides cluster-level image configuration.

```yaml
spec:
  image:
    image: "my-registry.example.com/bind9:custom"
    imagePullPolicy: "Always"
    imagePullSecrets:
      - my-registry-secret
```

**How It Works**:
- If not specified, inherits from `Bind9Cluster.spec.image`
- If cluster doesn't specify, uses default image `internetsystemsconsortium/bind9:9.18`
- Instance-level configuration takes precedence over cluster configuration

#### image.image
**Type**: string
**Required**: No
**Default**: "internetsystemsconsortium/bind9:9.18"

Full container image reference including registry, repository, and tag.

```yaml
spec:
  image:
    image: "docker.io/internetsystemsconsortium/bind9:9.18"
```

**Examples**:
- Public registry: `"internetsystemsconsortium/bind9:9.18"`
- Private registry: `"my-registry.example.com/dns/bind9:custom"`
- With digest: `"bind9@sha256:abc123..."`

#### image.imagePullPolicy
**Type**: string
**Required**: No
**Default**: "IfNotPresent"

Kubernetes image pull policy.

```yaml
spec:
  image:
    imagePullPolicy: "Always"
```

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
      - gcr-pull-secret
```

**Setup**:
1. Create a docker-registry secret:
   ```bash
   kubectl create secret docker-registry my-registry-secret \
     --docker-server=my-registry.example.com \
     --docker-username=user \
     --docker-password=pass \
     --docker-email=email@example.com
   ```
2. Reference the secret name in `imagePullSecrets`

### configMapRefs
**Type**: object
**Required**: No

References to custom ConfigMaps containing BIND9 configuration files. Overrides cluster-level ConfigMap references.

```yaml
spec:
  configMapRefs:
    namedConf: "my-custom-named-conf"
    namedConfOptions: "my-custom-options"
```

**How It Works**:
- If specified, Bindy uses your custom ConfigMaps instead of auto-generating configuration
- If not specified, Bindy auto-generates ConfigMaps from the `config` block
- Instance-level references override cluster-level references
- You can specify one or both ConfigMaps

**Default Behavior**:
- If `configMapRefs` is not set, Bindy creates a ConfigMap named `<instance-name>-config`
- Auto-generated ConfigMap includes both `named.conf` and `named.conf.options`
- Configuration is built from the `config` block in the spec

#### configMapRefs.namedConf
**Type**: string
**Required**: No

Name of ConfigMap containing the main `named.conf` file.

```yaml
spec:
  configMapRefs:
    namedConf: "my-named-conf"
```

**ConfigMap Format**:
```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-named-conf
  namespace: dns-system
data:
  named.conf: |
    // Custom BIND9 configuration
    include "/etc/bind/named.conf.options";
    include "/etc/bind/zones/named.conf.zones";

    logging {
      channel custom_log {
        file "/var/log/named/queries.log" versions 3 size 5m;
        severity info;
      };
      category queries { custom_log; };
    };
```

**File Location**: The ConfigMap data must have a key `named.conf` which will be mounted at `/etc/bind/named.conf`

#### configMapRefs.namedConfOptions
**Type**: string
**Required**: No

Name of ConfigMap containing the `named.conf.options` file.

```yaml
spec:
  configMapRefs:
    namedConfOptions: "my-options"
```

**ConfigMap Format**:
```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-options
  namespace: dns-system
data:
  named.conf.options: |
    options {
      directory "/var/cache/bind";
      recursion no;
      allow-query { any; };
      dnssec-validation auto;
    };
```

**File Location**: The ConfigMap data must have a key `named.conf.options` which will be mounted at `/etc/bind/named.conf.options`

**Examples**:

Using separate ConfigMaps for fine-grained control:
```yaml
spec:
  configMapRefs:
    namedConf: "prod-named-conf"
    namedConfOptions: "prod-options"
```

Using only custom options, auto-generating main config:
```yaml
spec:
  configMapRefs:
    namedConfOptions: "my-custom-options"
  # namedConf not specified - will be auto-generated
```

### global
**Type**: object
**Required**: No

BIND9 configuration options that override cluster-level global configuration.

#### global.recursion
**Type**: boolean
**Required**: No
**Default**: false

Enable recursive DNS queries. Should be `false` for authoritative servers.

```yaml
spec:
  global:
    recursion: false
```

**Warning**: Enabling recursion on public-facing authoritative servers is a security risk.

#### global.allowQuery
**Type**: array of strings
**Required**: No
**Default**: ["0.0.0.0/0"]

IP addresses or CIDR blocks allowed to query this server.

```yaml
spec:
  global:
    allowQuery:
      - "0.0.0.0/0"        # Allow all (public DNS)
      - "10.0.0.0/8"       # Private network
      - "192.168.1.0/24"   # Specific subnet
```

#### global.allowTransfer
**Type**: array of strings
**Required**: No
**Default**: []

IP addresses or CIDR blocks allowed to perform zone transfers (AXFR/IXFR).

```yaml
spec:
  global:
    allowTransfer:
      - "10.0.1.10"        # Specific secondary server
      - "10.0.1.11"        # Another secondary
```

**Security Note**: Restrict zone transfers to trusted secondary servers only.

#### global.dnssec
**Type**: object
**Required**: No

DNSSEC configuration for signing zones and validating responses.

##### global.dnssec.enabled
**Type**: boolean
**Required**: No
**Default**: false

Enable DNSSEC signing for zones.

```yaml
spec:
  global:
    dnssec:
      enabled: true
```

##### global.dnssec.validation
**Type**: boolean
**Required**: No
**Default**: false

Enable DNSSEC validation for recursive queries.

```yaml
spec:
  global:
    dnssec:
      enabled: true
      validation: true
```

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
      - "8.8.4.4"
```

#### global.listenOn
**Type**: array of strings
**Required**: No
**Default**: ["any"]

IPv4 addresses to listen on.

```yaml
spec:
  global:
    listenOn:
      - "any"              # All IPv4 interfaces
      - "10.0.1.10"        # Specific IP
```

#### global.listenOnV6
**Type**: array of strings
**Required**: No
**Default**: ["any"]

IPv6 addresses to listen on.

```yaml
spec:
  global:
    listenOnV6:
      - "any"              # All IPv6 interfaces
      - "2001:db8::1"      # Specific IPv6
```

## Status Fields

### conditions
**Type**: array of objects

Standard Kubernetes conditions indicating resource state.

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSuccess
      message: "Instance is ready"
      lastTransitionTime: "2024-01-15T10:30:00Z"
```

**Condition Types**:
- Ready - Instance is ready for use
- Available - Instance is serving DNS queries
- Progressing - Instance is being reconciled
- Degraded - Instance is partially functional
- Failed - Instance reconciliation failed

### observedGeneration
**Type**: integer

The generation of the resource that was last reconciled.

```yaml
status:
  observedGeneration: 5
```

### replicas
**Type**: integer

Total number of replicas configured.

```yaml
status:
  replicas: 3
```

### readyReplicas
**Type**: integer

Number of replicas that are ready and serving traffic.

```yaml
status:
  readyReplicas: 3
```

## Complete Example

### Primary DNS Instance

```yaml
# First create the Bind9Cluster
apiVersion: bindy.firestoned.io/v1beta1
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

---
# Then create the Bind9Instance referencing the cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
  labels:
    dns-role: primary
    environment: production
spec:
  clusterRef: production-dns  # References cluster above
  role: primary  # Required: primary or secondary
  replicas: 2
  # Inherits version and global config from cluster
```

### Secondary DNS Instance

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: secondary-dns
  namespace: dns-system
  labels:
    dns-role: secondary
    environment: production
spec:
  clusterRef: production-dns  # References same cluster as primary
  role: secondary  # Required: primary or secondary
  replicas: 2
  # Override global config for secondary role
  global:
    allowTransfer: []  # No zone transfers from secondary
    dnssec:
      enabled: false
      validation: true
```

### Recursive Resolver

```yaml
# Separate cluster for resolvers
apiVersion: bindy.firestoned.io/v1beta1
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
      - "1.1.1.1"
    dnssec:
      enabled: false
      validation: true

---
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: resolver
  namespace: dns-system
  labels:
    dns-role: resolver
spec:
  clusterRef: resolver-cluster
  role: primary  # Required: primary or secondary
  replicas: 3
  # Inherits recursive global config from cluster
```

## Related Resources

- [DNSZone Specification](./dnszone-spec.md)
- [Examples](./examples.md)
- [Configuration Guide](../operations/configuration.md)
