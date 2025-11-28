# Configuration

Configure the Bindy DNS operator and BIND9 instances for your environment.

## Controller Configuration

The Bindy controller is configured through environment variables set in the deployment.

See [Environment Variables](./env-vars.md) for details on all available configuration options.

## BIND9 Instance Configuration

Configure BIND9 instances through the `Bind9Instance` custom resource:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  clusterRef: my-cluster
  role: primary
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

#### Container Image Configuration

Customize the BIND9 container image and pull configuration:

```yaml
spec:
  # At instance level (overrides cluster)
  image:
    image: "my-registry.example.com/bind9:custom"
    imagePullPolicy: "Always"
    imagePullSecrets:
      - my-registry-secret
```

Or configure at the cluster level for all instances:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: my-cluster
spec:
  # Default image configuration for all instances
  image:
    image: "internetsystemsconsortium/bind9:9.18"
    imagePullPolicy: "IfNotPresent"
    imagePullSecrets:
      - shared-pull-secret
```

**Fields:**
- `image`: Full container image reference (e.g., `registry/image:tag`)
- `imagePullPolicy`: `Always`, `IfNotPresent`, or `Never`
- `imagePullSecrets`: List of secret names for private registries

#### Custom Configuration Files

Use custom ConfigMaps for BIND9 configuration:

```yaml
spec:
  # Reference custom ConfigMaps
  configMapRefs:
    namedConf: "my-custom-named-conf"
    namedConfOptions: "my-custom-options"
```

Create your custom ConfigMap:

```yaml
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
      channel custom_log {
        file "/var/log/named/queries.log" versions 3 size 5m;
        severity info;
      };
      category queries { custom_log; };
    };
```

**Default Behavior:**
- If `configMapRefs` is not specified, Bindy auto-generates configuration from the `config` block
- If custom ConfigMaps are provided, they take precedence
- Default ConfigMap template is available in `deploy/configs/default-bind9-config.yaml`

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
