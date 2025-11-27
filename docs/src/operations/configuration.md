# Configuration

Configure the Bindy DNS operator and BIND9 instances for your environment.

## Controller Configuration

The Bindy controller is configured through environment variables set in the deployment.

See [Environment Variables](./env-vars.md) for details on all available configuration options.

## BIND9 Instance Configuration

Configure BIND9 instances through the `Bind9Instance` custom resource:

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"
    dnssec:
      enabled: true
      validation: true
```

### Configuration Options

#### Recursion

Control whether the DNS server performs recursive queries:

```yaml
spec:
  config:
    recursion: false  # Disable for authoritative servers
```

For authoritative DNS servers, recursion should be disabled.

#### Query Access Control

Specify which networks can query the DNS server:

```yaml
spec:
  config:
    allowQuery:
      - "0.0.0.0/0"        # Allow from anywhere (public DNS)
      - "10.0.0.0/8"       # Private network only
      - "192.168.1.0/24"   # Specific subnet
```

#### Zone Transfer Access Control

Restrict zone transfers to authorized servers:

```yaml
spec:
  config:
    allowTransfer:
      - "10.0.1.0/24"      # Secondary DNS network
      - "192.168.100.5"    # Specific secondary server
```

#### DNSSEC Configuration

Enable DNSSEC signing and validation:

```yaml
spec:
  config:
    dnssec:
      enabled: true        # Enable DNSSEC signing
      validation: true     # Enable DNSSEC validation
```

## RBAC Configuration

Configure Role-Based Access Control for the operator.

See [RBAC](./rbac.md) for detailed RBAC setup.

## Resource Limits

Set CPU and memory limits for BIND9 pods.

See [Resource Limits](./resources.md) for resource configuration.

## Configuration Best Practices

1. **Separate Primary and Secondary** - Use different instances for primary and secondary roles
2. **Limit Zone Transfers** - Only allow transfers to known secondaries
3. **Enable DNSSEC** - Use DNSSEC for production zones
4. **Set Appropriate Replicas** - Use 2+ replicas for high availability
5. **Use Labels** - Organize instances with meaningful labels

## Next Steps

- [Environment Variables](./env-vars.md) - Controller configuration
- [RBAC Setup](./rbac.md) - Permissions and service accounts
- [Resource Limits](./resources.md) - CPU and memory configuration
