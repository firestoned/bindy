# Configuration

Configure the Bindy DNS operator and BIND9 instances for your environment.

## Operator Configuration

The Bindy operator is configured through environment variables set in the deployment.

See [Environment Variables](./env-vars.md) for details on all available configuration options.

### Kubernetes API Rate Limiting

The operator includes configurable rate limiting for Kubernetes API requests to prevent overwhelming the API server in large deployments.

**Environment Variables:**

- `BINDY_KUBE_QPS` - Sustained queries per second (default: `20.0`)
- `BINDY_KUBE_BURST` - Maximum burst requests (default: `30`)

**Default Values:**

The defaults match `kubectl` rate limits and are suitable for most deployments:

```yaml
env:
  - name: BINDY_KUBE_QPS
    value: "20.0"
  - name: BINDY_KUBE_BURST
    value: "30"
```

**Tuning for Large Deployments:**

For clusters with hundreds of resources, you may need to increase these limits:

```yaml
env:
  - name: BINDY_KUBE_QPS
    value: "50.0"    # Higher sustained rate
  - name: BINDY_KUBE_BURST
    value: "100"     # Larger burst allowance
```

**Symptoms of Rate Limiting:**

- HTTP 429 (Too Many Requests) errors in logs
- Slow reconciliation times
- Operator falling behind on resource updates

**Pagination:**

The operator automatically paginates Kubernetes API list operations to prevent memory issues and reduce API server load when working with large resource sets (e.g., 1000+ `DNSZone`s).

- **Page Size**: 100 items per page (configurable via `KUBE_LIST_PAGE_SIZE` constant)
- **Memory Usage**: Constant O(1) relative to total resource count
- **API Efficiency**: 1000 resources = 10 API calls

Pagination is automatically applied to:
- DNSZone discovery and record queries
- Bind9Cluster instance listings
- Bind9Instance pod health checks

**Automatic Retry with Exponential Backoff:**

The operator automatically retries transient Kubernetes API errors with exponential backoff:

- **Retryable Errors**: HTTP 429 (rate limiting), 5xx (server errors), network failures
- **Non-Retryable Errors**: 4xx client errors (except 429) - fail immediately
- **Initial Retry**: 100ms
- **Max Interval**: 30 seconds between retries
- **Max Duration**: 5 minutes total retry time
- **Backoff Multiplier**: 2.0 (exponential growth)
- **Randomization**: ±10% (prevents thundering herd)

Retry schedule (approximate):
1. 100ms
2. 200ms
3. 400ms
4. 800ms
5. 1.6s
6. 3.2s
7. 6.4s
8. 12.8s
9. 25.6s
10. 30s (then continues at 30s intervals until 5 minutes elapsed)

**Implementation Status:**

- ✅ Phase 1: Rate limiting configuration (QPS/Burst environment variables)
- ✅ Phase 2: Pagination for list operations
- ✅ Phase 3: Exponential backoff for retries
- ⏳ Phase 4: Tower middleware-based rate limiting (planned)

## BIND9 Instance Configuration

Configure BIND9 instances through the `Bind9Instance` custom resource:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
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
apiVersion: bindy.firestoned.io/v1beta1
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
    namedConfZones: "my-custom-zones"  # Optional: for zone definitions
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

**Zones Configuration File:**

If you need to provide a custom zones file (e.g., for pre-configured zones), create a ConfigMap with `named.conf.zones`:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-custom-zones
  namespace: dns-system
data:
  named.conf.zones: |
    // Zone definitions
    zone "example.com" {
      type primary;
      file "/etc/bind/zones/example.com.zone";
    };

    zone "internal.local" {
      type primary;
      file "/etc/bind/zones/internal.local.zone";
    };
```

Then reference it in your `Bind9Instance`:

```yaml
spec:
  configMapRefs:
    namedConfZones: "my-custom-zones"
```

**Default Behavior:**
- If `configMapRefs` is not specified, Bindy auto-generates configuration from the `config` block
- If custom ConfigMaps are provided, they take precedence
- The `namedConfZones` ConfigMap is optional - only include it if you need to pre-configure zones
- If no `namedConfZones` is provided, no zones file will be included (zones can be added dynamically via RNDC)

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

- [Environment Variables](./env-vars.md) - Operator configuration
- [RBAC Setup](./rbac.md) - Permissions and service accounts
- [Resource Limits](./resources.md) - CPU and memory configuration
