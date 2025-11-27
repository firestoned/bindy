# API Reference

Complete reference for Bindy's Kubernetes Custom Resources.

## Custom Resource Definitions

Bindy provides the following CRDs:

### Core Resources

#### Bind9Instance
Manages BIND9 DNS server instances.

- **Group**: dns.firestoned.io
- **Version**: v1alpha1
- **Kind**: Bind9Instance
- **Plural**: bind9instances
- **Scope**: Namespaced

[Full specification →](./bind9instance-spec.md)

#### DNSZone
Defines DNS zones to be served by Bind9Instances.

- **Group**: dns.firestoned.io
- **Version**: v1alpha1
- **Kind**: DNSZone
- **Plural**: dnszones
- **Scope**: Namespaced

[Full specification →](./dnszone-spec.md)

### DNS Record Resources

All record resources share:
- **Group**: dns.firestoned.io
- **Version**: v1alpha1
- **Scope**: Namespaced

| Resource | Kind | Purpose |
|----------|------|---------|
| arecords | ARecord | IPv4 address records |
| aaaarecords | AAAARecord | IPv6 address records |
| cnamerecords | CNAMERecord | Canonical name (alias) records |
| mxrecords | MXRecord | Mail exchange records |
| txtrecords | TXTRecord | Text records |
| nsrecords | NSRecord | Name server records |
| srvrecords | SRVRecord | Service location records |
| caarecords | CAARecord | Certificate authority authorization |

[Full record specifications →](./record-specs.md)

## Common Patterns

### Labels and Selectors

All resources support standard Kubernetes labels and selectors:

```yaml
metadata:
  labels:
    app: my-app
    environment: production
```

DNSZone uses label selectors to target Bind9Instances:

```yaml
spec:
  instanceSelector:
    matchLabels:
      dns-role: primary
```

### Status Conditions

All resources implement standard status conditions:

- **Ready**: Resource is ready for use
- **Available**: Resource is available and serving traffic
- **Progressing**: Resource is being reconciled
- **Degraded**: Resource is partially functional
- **Failed**: Resource reconciliation failed

Example status:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      lastTransitionTime: "2024-01-15T10:30:00Z"
      reason: ReconcileSuccess
      message: "Resource reconciled successfully"
```

### Owner References

Bindy automatically sets owner references for resource cleanup:

```yaml
metadata:
  ownerReferences:
    - apiVersion: dns.firestoned.io/v1alpha1
      kind: DNSZone
      name: example-zone
      uid: abc123
      controller: true
```

## API Versions

### v1alpha1 (Current)

Current development version. API may change between releases.

**Stability**: Alpha - Breaking changes possible

**Deprecation Policy**: None yet established

### Future Versions

- **v1beta1**: Planned when API stabilizes
- **v1**: Planned for 1.0 release

## Validation

### Schema Validation

All CRDs include OpenAPI v3 schema validation:

- Required fields enforced
- Field types validated
- Pattern matching for strings
- Range validation for numbers

### Webhook Validation

Planned for future releases:

- Cross-field validation
- Business logic validation
- Default value injection

## API Conventions

### Naming

- Use DNS-compatible names (lowercase, numbers, hyphens)
- Maximum 253 characters for resource names
- Maximum 63 characters for DNS labels

### TTL Values

TTL (Time To Live) values in seconds:

- Minimum: 60 (1 minute)
- Maximum: 86400 (24 hours)
- Default: 3600 (1 hour)

### IP Addresses

- IPv4: Dotted decimal notation (192.0.2.1)
- IPv6: Colon-separated hexadecimal (2001:db8::1)
- CIDR notation supported for ACLs (10.0.0.0/8)

## Examples

See the [Examples](./examples.md) section for complete configuration examples.
