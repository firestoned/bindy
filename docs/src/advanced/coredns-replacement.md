# Replacing CoreDNS with ClusterBind9Provider

`ClusterBind9Provider` provides a powerful alternative to CoreDNS for cluster-wide DNS infrastructure. This guide explores using Bindy as a CoreDNS replacement in Kubernetes clusters.

## Why Consider Replacing CoreDNS?

CoreDNS is the default DNS solution for Kubernetes, but you might want an alternative if you need:

- **Enterprise DNS Features**: Advanced BIND9 capabilities like DNSSEC, dynamic updates via RNDC, and comprehensive zone management
- **Centralized DNS Management**: Declarative DNS infrastructure managed via Kubernetes CRDs
- **GitOps-Ready DNS**: DNS configuration as code, versioned and auditable
- **Integration with Existing Infrastructure**: Organizations already using BIND9 for external DNS
- **Compliance Requirements**: Full audit trails, signed releases, and documented controls (SOX, NIST 800-53)
- **Advanced Zone Management**: Programmatic control over zones and records without editing configuration files

## Architecture Comparison

### CoreDNS (Default)

```
┌─────────────────────────────────────────┐
│ CoreDNS DaemonSet/Deployment            │
│ - Serves cluster.local queries          │
│ - Configured via ConfigMap              │
│ - Limited to Corefile syntax            │
└─────────────────────────────────────────┘
```

**Characteristics:**

- Simple, built-in solution
- ConfigMap-based configuration
- Limited declarative management
- Manual ConfigMap edits for changes

### Bindy with ClusterBind9Provider

```
┌──────────────────────────────────────────────────┐
│ ClusterBind9Provider (cluster-scoped)            │
│ - Cluster-wide DNS infrastructure                │
│ - Platform team managed                          │
└──────────────────────────────────────────────────┘
         │
         ├─ Creates → Bind9Cluster (per namespace)
         │            └─ Creates → Bind9Instance (BIND9 pods)
         │
         └─ Referenced by DNSZones (any namespace)
                       └─ Records (A, AAAA, CNAME, MX, TXT, etc.)
```

**Characteristics:**

- Declarative infrastructure-as-code
- GitOps-ready (all configuration in YAML)
- Dynamic updates via RNDC API (no restarts)
- Full DNSSEC support
- Programmatic record management
- Multi-tenancy with RBAC

## Use Cases

### 1. Platform DNS Service

Replace CoreDNS with a platform-managed DNS service accessible to all namespaces:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: platform-dns
  labels:
    app.kubernetes.io/component: dns
    app.kubernetes.io/part-of: platform-services
spec:
  version: "9.18"
  primary:
    replicas: 3  # HA for cluster DNS
    service:
      spec:
        type: ClusterIP
        clusterIP: 10.96.0.10  # Standard kube-dns ClusterIP
  secondary:
    replicas: 2
  global:
    recursion: true  # Important for cluster DNS
    allowQuery:
      - "0.0.0.0/0"
    forwarders:  # Forward external queries
      - "8.8.8.8"
      - "8.8.4.4"
```

**Benefits:**

- High availability with multiple replicas
- Declarative configuration (no ConfigMap editing)
- Version-controlled DNS infrastructure
- Gradual migration path from CoreDNS

### 2. Hybrid DNS Architecture

Use Bindy for application DNS while keeping CoreDNS for `cluster.local`:

```yaml
# CoreDNS continues handling cluster.local
# Bindy handles application-specific zones

apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: app-dns
spec:
  version: "9.18"
  primary:
    replicas: 2
  secondary:
    replicas: 1
---
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: internal-services
  namespace: platform
spec:
  zoneName: internal.example.com
  clusterProviderRef: app-dns
  soaRecord:
    primaryNs: ns1.internal.example.com.
    adminEmail: platform.example.com.
```

**Benefits:**

- Zero risk to existing cluster DNS
- Application teams get advanced DNS features
- Incremental adoption
- Clear separation of concerns

### 3. Service Mesh Integration

Provide DNS for service mesh configurations:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: mesh-dns
  labels:
    linkerd.io/control-plane-ns: linkerd
spec:
  version: "9.18"
  primary:
    replicas: 2
    service:
      annotations:
        linkerd.io/inject: enabled
  global:
    recursion: false  # Authoritative only
    allowQuery:
      - "10.0.0.0/8"  # Service mesh network
---
# Application teams create zones
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: api-zone
  namespace: api-team
spec:
  zoneName: api.mesh.local
  clusterProviderRef: mesh-dns
---
# Dynamic service records
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: api-v1
  namespace: api-team
  labels:
    zone: api.mesh.local  # Matches DNSZone selector
spec:
  name: v1
  ipv4Address: "10.0.1.100"
```

**Benefits:**

- Service mesh can use DNS for routing
- Dynamic record updates without mesh operator changes
- Platform team manages DNS infrastructure
- Application teams manage their service records

## Migration Strategies

### Strategy 1: Parallel Deployment (Recommended)

Run Bindy alongside CoreDNS during migration:

1. **Deploy Bindy Cluster Provider**:
   ```yaml
   apiVersion: bindy.firestoned.io/v1beta1
   kind: ClusterBind9Provider
   metadata:
     name: platform-dns-migration
   spec:
     version: "9.18"
     primary:
       replicas: 2
       service:
         spec:
           type: ClusterIP  # Different IP from CoreDNS
     global:
       recursion: true
       forwarders:
         - "8.8.8.8"
   ```

2. **Test DNS Resolution**:
   ```bash
   # Get Bindy DNS service IP
   kubectl get svc -n dns-system -l app.kubernetes.io/name=bind9

   # Test queries
   dig @<bindy-service-ip> kubernetes.default.svc.cluster.local
   dig @<bindy-service-ip> google.com
   ```

3. **Gradually Migrate Applications**:
   Update pod specs to use Bindy DNS:
   ```yaml
   spec:
     dnsPolicy: None
     dnsConfig:
       nameservers:
         - <bindy-service-ip>
       searches:
         - default.svc.cluster.local
         - svc.cluster.local
         - cluster.local
   ```

4. **Switch Cluster Default** (final step):
   ```bash
   # Update kubelet DNS config
   # Change --cluster-dns to Bindy service IP
   # Rolling restart nodes
   ```

### Strategy 2: Zone-by-Zone Migration

Keep CoreDNS for cluster.local, migrate application zones:

1. **Keep CoreDNS for Cluster Services**:
   ```yaml
   # CoreDNS ConfigMap unchanged
   # Handles *.cluster.local, *.svc.cluster.local
   ```

2. **Create Application Zones in Bindy**:
   ```yaml
   apiVersion: bindy.firestoned.io/v1beta1
   kind: DNSZone
   metadata:
     name: apps-zone
     namespace: platform
   spec:
     zoneName: apps.example.com
     clusterProviderRef: platform-dns
   ```

3. **Configure Forwarding** (CoreDNS → Bindy):
   ```yaml
   # CoreDNS Corefile
   apps.example.com:53 {
     forward . <bindy-service-ip>
   }
   ```

**Benefits:**

- Zero risk to cluster stability
- Incremental testing
- Easy rollback
- Coexistence of both solutions

## Configuration for Cluster DNS

### Essential Settings

For cluster DNS replacement, configure these settings:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: cluster-dns
spec:
  version: "9.18"
  primary:
    replicas: 3  # HA requirement
    service:
      spec:
        type: ClusterIP
        clusterIP: 10.96.0.10  # kube-dns default
  global:
    # CRITICAL: Enable recursion for cluster DNS
    recursion: true

    # Allow queries from all pods
    allowQuery:
      - "0.0.0.0/0"

    # Forward external queries to upstream DNS
    forwarders:
      - "8.8.8.8"
      - "8.8.4.4"

    # Cluster.local zone configuration
    zones:
      - name: cluster.local
        type: forward
        forwarders:
          - "10.96.0.10"  # Forward to Bindy itself for cluster zones
```

### Recommended Zones

Create these zones for Kubernetes cluster DNS:

```yaml
---
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: cluster-local
  namespace: dns-system
spec:
  zoneName: cluster.local
  clusterProviderRef: cluster-dns
  soaRecord:
    primaryNs: ns1.cluster.local.
    adminEmail: dns-admin.cluster.local.
---
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: svc-cluster-local
  namespace: dns-system
spec:
  zoneName: svc.cluster.local
  clusterProviderRef: cluster-dns
  soaRecord:
    primaryNs: ns1.svc.cluster.local.
    adminEmail: dns-admin.svc.cluster.local.
```

## Advantages Over CoreDNS

### 1. Declarative Infrastructure

**CoreDNS:**
```yaml
# Manual ConfigMap editing
apiVersion: v1
kind: ConfigMap
metadata:
  name: coredns
data:
  Corefile: |
    .:53 {
        errors
        health
        # ... manual editing required
    }
```

**Bindy:**
```yaml
# Infrastructure as code
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
# ... declarative specs
---
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
# ... versioned, reviewable YAML
```

### 2. Dynamic Updates

**CoreDNS:**

- Requires ConfigMap changes
- Requires pod restarts
- No programmatic API

**Bindy:**

- Dynamic record updates via RNDC
- Zero downtime changes
- Programmatic API (Kubernetes CRDs)

### 3. Multi-Tenancy

**CoreDNS:**

- Single shared ConfigMap
- No namespace isolation
- Platform team controls everything

**Bindy:**

- Platform team: Manages `ClusterBind9Provider`
- Application teams: Manage `DNSZone` and records in their namespace
- RBAC-enforced isolation

### 4. Enterprise Features

**Bindy Provides:**

- ✅ DNSSEC with automatic key management
- ✅ Zone transfers (AXFR/IXFR)
- ✅ Split-horizon DNS (views/ACLs)
- ✅ Audit logging for compliance
- ✅ SOA record management
- ✅ Full BIND9 feature set

**CoreDNS:**

- ❌ Limited DNSSEC support
- ❌ No zone transfers
- ❌ Basic view support
- ❌ Limited audit capabilities

## Operational Considerations

### Performance

**Memory Usage:**

- CoreDNS: ~30-50 MB per pod
- Bindy (BIND9): ~100-200 MB per pod
- Trade-off: More features, slightly higher resource use

**Query Performance:**

- Both handle 10K+ queries/sec per pod
- BIND9 excels at authoritative zones
- CoreDNS excels at simple forwarding

**Recommendation:** Use Bindy where you need advanced features; CoreDNS is lighter for simple forwarding.

### High Availability

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: ha-dns
spec:
  primary:
    replicas: 3  # Spread across zones
    affinity:
      podAntiAffinity:
        requiredDuringSchedulingIgnoredDuringExecution:
          - labelSelector:
              matchLabels:
                app.kubernetes.io/name: bind9
            topologyKey: kubernetes.io/hostname
  secondary:
    replicas: 2  # Read replicas for query load
```

### Monitoring

```bash
# Check DNS cluster status
kubectl get clusterbind9provider -o wide

# Check instance health
kubectl get bind9instances -n dns-system

# Query metrics (if Prometheus enabled)
kubectl port-forward -n dns-system svc/bindy-metrics 8080:8080
curl localhost:8080/metrics | grep bindy_
```

## Limitations

### Not Suitable For:

1. **Clusters requiring ultra-low resource usage**
   - CoreDNS is lighter for simple forwarding

2. **Simple forwarding-only scenarios**
   - CoreDNS is simpler if you don't need BIND9 features

3. **Rapid pod scaling (1000s/sec)**
   - CoreDNS has slightly faster startup time

### Well-Suited For:

1. **Enterprise environments** with compliance requirements
2. **Multi-tenant platforms** with RBAC requirements
3. **Complex DNS requirements** (DNSSEC, zone transfers, dynamic updates)
4. **GitOps workflows** where DNS is infrastructure-as-code
5. **Organizations standardizing on BIND9** across infrastructure

## Best Practices

### 1. Start with Hybrid Approach

Keep CoreDNS for `cluster.local`, add Bindy for application zones:

```yaml
# CoreDNS: cluster.local, svc.cluster.local
# Bindy: apps.example.com, internal.example.com
```

### 2. Use Health Checks

```yaml
spec:
  primary:
    livenessProbe:
      tcpSocket:
        port: 53
      initialDelaySeconds: 30
    readinessProbe:
      exec:
        command: ["/usr/bin/dig", "@127.0.0.1", "health.check.local"]
```

### 3. Enable Audit Logging

```yaml
spec:
  global:
    logging:
      channels:
        - name: audit_log
          file: /var/log/named/audit.log
          severity: info
      categories:
        - name: update
          channels: [audit_log]
```

### 4. Plan for Disaster Recovery

```bash
# Backup DNS zones
kubectl get dnszones -A -o yaml > dns-zones-backup.yaml

# Backup records
kubectl get arecords,cnamerecords,mxrecords -A -o yaml > dns-records-backup.yaml
```

## Conclusion

`ClusterBind9Provider` provides a powerful, enterprise-grade alternative to CoreDNS for Kubernetes clusters. While CoreDNS remains an excellent choice for simple forwarding scenarios, Bindy excels when you need:

- Declarative DNS infrastructure-as-code
- GitOps workflows for DNS management
- Multi-tenancy with namespace isolation
- Enterprise features (DNSSEC, zone transfers, dynamic updates)
- Compliance and audit requirements
- Integration with existing BIND9 infrastructure

**Recommendation:** Start with a hybrid approach—keep CoreDNS for cluster services, and adopt Bindy for application DNS zones. This provides a safe migration path with the ability to leverage advanced DNS features where needed.

## Next Steps

- [Multi-Tenancy Guide](../guide/multi-tenancy.md) - RBAC setup for platform and application teams
- [Choosing a Cluster Type](../guide/choosing-cluster-type.md) - When to use ClusterBind9Provider vs Bind9Cluster
- [High Availability](./ha.md) - HA configuration for production DNS
- [DNSSEC](./dnssec.md) - Enabling DNSSEC for secure DNS
