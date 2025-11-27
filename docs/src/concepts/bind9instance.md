# Bind9Instance

The `Bind9Instance` resource represents a BIND9 DNS server deployment in Kubernetes.

## Overview

A Bind9Instance defines:
- Number of replicas
- BIND9 version
- Configuration options
- Network settings
- Labels for targeting

## Example

```yaml
apiVersion: dns.firestoned.io/v1alpha1
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

### Required Fields

- `spec.version` - BIND9 version to deploy

### Optional Fields

- `spec.replicas` - Number of BIND9 pods (default: 1)
- `spec.config` - BIND9 configuration options
  - `recursion` - Enable/disable recursion (default: false)
  - `allowQuery` - List of CIDR ranges allowed to query
  - `allowTransfer` - List of CIDR ranges allowed to transfer zones

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

## Next Steps

- [DNSZone](./dnszone.md) - Learn about DNS zones
- [Primary Instances](../guide/primary-instance.md) - Deploy primary DNS
- [Secondary Instances](../guide/secondary-instance.md) - Deploy secondary DNS
