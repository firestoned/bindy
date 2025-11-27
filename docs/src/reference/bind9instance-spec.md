# Bind9Instance Specification

Complete specification for the Bind9Instance Custom Resource Definition.

## Resource Definition

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: string
  namespace: string
  labels:
    key: value
spec:
  replicas: integer
  version: string
  config:
    recursion: boolean
    allowQuery: [string]
    allowTransfer: [string]
    dnssec:
      enabled: boolean
      validation: boolean
    forwarders: [string]
    listenOn: [string]
    listenOnV6: [string]
```

## Spec Fields

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

### config
**Type**: object
**Required**: No

BIND9 configuration options.

#### config.recursion
**Type**: boolean
**Required**: No
**Default**: false

Enable recursive DNS queries. Should be `false` for authoritative servers.

```yaml
spec:
  config:
    recursion: false
```

**Warning**: Enabling recursion on public-facing authoritative servers is a security risk.

#### config.allowQuery
**Type**: array of strings
**Required**: No
**Default**: ["0.0.0.0/0"]

IP addresses or CIDR blocks allowed to query this server.

```yaml
spec:
  config:
    allowQuery:
      - "0.0.0.0/0"        # Allow all (public DNS)
      - "10.0.0.0/8"       # Private network
      - "192.168.1.0/24"   # Specific subnet
```

#### config.allowTransfer
**Type**: array of strings
**Required**: No
**Default**: []

IP addresses or CIDR blocks allowed to perform zone transfers (AXFR/IXFR).

```yaml
spec:
  config:
    allowTransfer:
      - "10.0.1.10"        # Specific secondary server
      - "10.0.1.11"        # Another secondary
```

**Security Note**: Restrict zone transfers to trusted secondary servers only.

#### config.dnssec
**Type**: object
**Required**: No

DNSSEC configuration for signing zones and validating responses.

##### config.dnssec.enabled
**Type**: boolean
**Required**: No
**Default**: false

Enable DNSSEC signing for zones.

```yaml
spec:
  config:
    dnssec:
      enabled: true
```

##### config.dnssec.validation
**Type**: boolean
**Required**: No
**Default**: false

Enable DNSSEC validation for recursive queries.

```yaml
spec:
  config:
    dnssec:
      enabled: true
      validation: true
```

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
      - "8.8.4.4"
```

#### config.listenOn
**Type**: array of strings
**Required**: No
**Default**: ["any"]

IPv4 addresses to listen on.

```yaml
spec:
  config:
    listenOn:
      - "any"              # All IPv4 interfaces
      - "10.0.1.10"        # Specific IP
```

#### config.listenOnV6
**Type**: array of strings
**Required**: No
**Default**: ["any"]

IPv6 addresses to listen on.

```yaml
spec:
  config:
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
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
  labels:
    dns-role: primary
    environment: production
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.2.0/24"
    dnssec:
      enabled: true
      validation: false
    listenOn:
      - "any"
    listenOnV6:
      - "any"
```

### Secondary DNS Instance

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-dns
  namespace: dns-system
  labels:
    dns-role: secondary
    environment: production
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    # No allowTransfer for secondary
    dnssec:
      enabled: false
      validation: true
```

### Recursive Resolver

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: resolver
  namespace: dns-system
  labels:
    dns-role: resolver
spec:
  replicas: 3
  version: "9.18"
  config:
    recursion: true
    allowQuery:
      - "10.0.0.0/8"        # Internal network only
    forwarders:
      - "8.8.8.8"
      - "1.1.1.1"
    dnssec:
      enabled: false
      validation: true
```

## Related Resources

- [DNSZone Specification](./dnszone-spec.md)
- [Examples](./examples.md)
- [Configuration Guide](../operations/configuration.md)
