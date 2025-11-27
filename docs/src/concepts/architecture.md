# Architecture Overview

This page provides a detailed overview of Bindy's architecture and design principles.

## High-Level Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    Kubernetes Cluster                        │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │            Custom Resource Definitions (CRDs)          │ │
│  │  • Bind9Instance  • DNSZone  • ARecord  • MXRecord ... │ │
│  └────────────────────────────────────────────────────────┘ │
│                             │                                │
│                             │ watches                        │
│                             ▼                                │
│  ┌────────────────────────────────────────────────────────┐ │
│  │              Bindy Controller (Rust)                   │ │
│  │                                                        │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐ │ │
│  │  │  Bind9Instance│  │   DNSZone    │  │   Records   │ │ │
│  │  │  Reconciler  │  │  Reconciler  │  │  Reconciler │ │ │
│  │  └──────────────┘  └──────────────┘  └─────────────┘ │ │
│  │                                                        │ │
│  │  ┌──────────────────────────────────────────────────┐ │ │
│  │  │         Zone File Generator                      │ │ │
│  │  └──────────────────────────────────────────────────┘ │ │
│  └────────────────────────────────────────────────────────┘ │
│                             │                                │
│                             │ configures                     │
│                             ▼                                │
│  ┌────────────────────────────────────────────────────────┐ │
│  │              BIND9 Instances                           │ │
│  │                                                        │ │
│  │  ┌──────────┐      ┌──────────┐      ┌──────────┐    │ │
│  │  │ Primary  │ AXFR │Secondary │ AXFR │Secondary │    │ │
│  │  │   DNS    │─────▶│   DNS    │─────▶│   DNS    │    │ │
│  │  │  (us-east)│     │ (us-west)│     │  (eu)    │    │ │
│  │  └──────────┘      └──────────┘      └──────────┘    │ │
│  └────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
                             │
                             │ DNS queries (UDP/TCP 53)
                             ▼
                    ┌─────────────────┐
                    │     Clients     │
                    │  • Apps         │
                    │  • Services     │
                    │  • External     │
                    └─────────────────┘
```

## Components

### Bindy Controller

The controller is written in Rust using the kube-rs library. It consists of:

#### 1. Reconcilers

Each reconciler handles a specific resource type:

- **Bind9Instance Reconciler** - Manages BIND9 instance lifecycle
  - Creates StatefulSets for BIND9 pods
  - Configures services and networking
  - Updates instance status

- **DNSZone Reconciler** - Manages DNS zones
  - Evaluates label selectors
  - Generates zone files
  - Updates zone configuration
  - Reports matched instances

- **Record Reconcilers** - Manage individual DNS records
  - One reconciler per record type (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)
  - Validates record specifications
  - Appends records to zone files
  - Updates record status

#### 2. Zone File Generator

Generates BIND9-compatible zone files from Kubernetes resources:

```rust
// Simplified example
pub fn generate_zone_file(zone: &DNSZone, records: Vec<DNSRecord>) -> String {
    let mut zone_file = String::new();

    // SOA record
    zone_file.push_str(&format_soa_record(&zone.spec.soa_record));

    // NS records
    for ns in &zone.spec.name_servers {
        zone_file.push_str(&format_ns_record(ns));
    }

    // Individual records
    for record in records {
        zone_file.push_str(&format_record(record));
    }

    zone_file
}
```

### Custom Resource Definitions (CRDs)

CRDs define the schema for DNS resources:

```yaml
apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: dnszones.dns.firestoned.io
spec:
  group: dns.firestoned.io
  names:
    kind: DNSZone
    plural: dnszones
  scope: Namespaced
  versions:
    - name: v1alpha1
      served: true
      storage: true
```

### BIND9 Instances

BIND9 servers managed by Bindy:

- Deployed as Kubernetes StatefulSets
- Configuration via ConfigMaps
- Zone files mounted from ConfigMaps or PVCs
- Support for primary and secondary architectures

## Data Flow

### Zone Creation Flow

1. **User creates DNSZone resource**
   ```bash
   kubectl apply -f dnszone.yaml
   ```

2. **Controller watches and receives event**
   ```rust
   // Watch stream receives create event
   stream.next().await
   ```

3. **DNSZone reconciler evaluates selector**
   ```rust
   // Find matching Bind9Instances
   let instances = find_matching_instances(&zone.spec.instance_selector).await?;
   ```

4. **Generate zone file for each instance**
   ```rust
   // Create zone configuration
   let zone_file = generate_zone_file(&zone, &records)?;
   ```

5. **Update BIND9 configuration**
   ```rust
   // Apply ConfigMap with zone file
   update_bind9_config(&instance, &zone_file).await?;
   ```

6. **Update DNSZone status**
   ```rust
   // Report success
   update_status(&zone, conditions, matched_instances).await?;
   ```

### Record Addition Flow

1. **User creates DNS record resource**
2. **Controller receives event**
3. **Record reconciler validates zone reference**
4. **Append record to existing zone file**
5. **Reload BIND9 configuration**
6. **Update record status**

## Concurrency Model

Bindy uses Rust's async/await with Tokio runtime:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // Spawn multiple reconcilers concurrently
    tokio::try_join!(
        run_bind9instance_controller(),
        run_dnszone_controller(),
        run_record_controllers(),
    )?;
    Ok(())
}
```

Benefits:
- **Concurrent reconciliation** - Multiple resources reconciled simultaneously
- **Non-blocking I/O** - Efficient API server communication
- **Low memory footprint** - Async tasks use minimal memory
- **High throughput** - Handle thousands of DNS records efficiently

## Resource Watching

The controller uses Kubernetes watch API with reflector caching:

```rust
let api: Api<DNSZone> = Api::all(client);
let watcher = watcher(api, ListParams::default());

// Reflector caches resources locally
let store = reflector::store::Writer::default();
let reader = store.as_reader();
let reflector = reflector(store, watcher);

// Process events
while let Some(event) = stream.try_next().await? {
    match event {
        Applied(zone) => reconcile_zone(zone).await?,
        Deleted(zone) => cleanup_zone(zone).await?,
        Restarted(_) => refresh_all().await?,
    }
}
```

## Error Handling

Multi-layer error handling strategy:

1. **Validation Errors** - Caught early, reported in status
2. **Reconciliation Errors** - Retried with exponential backoff
3. **Fatal Errors** - Logged and cause controller restart
4. **Status Reporting** - All errors visible in resource status

```rust
match reconcile_zone(&zone).await {
    Ok(_) => update_status(Ready, "Synchronized"),
    Err(e) => {
        log::error!("Failed to reconcile zone: {}", e);
        update_status(NotReady, e.to_string());
        // Requeue for retry
        Err(e)
    }
}
```

## Performance Optimizations

### 1. Incremental Updates
Only regenerate zone files when records change, not on every reconciliation.

### 2. Caching
Local cache of BIND9 instances to avoid repeated API calls.

### 3. Batch Processing
Group related updates to minimize BIND9 reloads.

### 4. Zero-Copy Operations
Use string slicing and references to avoid unnecessary allocations.

### 5. Compiled Binary
Rust compilation produces optimized native code with no runtime overhead.

## Security Architecture

### RBAC

Controller uses least-privilege service account:

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: bind9-controller
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bind9-controller
rules:
  - apiGroups: ["dns.firestoned.io"]
    resources: ["dnszones", "arecords", ...]
    verbs: ["get", "list", "watch", "update"]
```

### Non-Root Containers

Controller runs as non-root user:

```dockerfile
USER 65532:65532
```

### Network Policies

Limit controller network access:

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: bind9-controller
spec:
  podSelector:
    matchLabels:
      app: bind9-controller
  policyTypes:
    - Egress
  egress:
    - to:
        - namespaceSelector: {}
      ports:
        - protocol: TCP
          port: 443  # API server only
```

## Scalability

### Horizontal Scaling

Multiple controller replicas with leader election:

```rust
let lease_lock = LeaseLock::new(
    client,
    "dns-system",
    "bind9-controller-leader",
);

run_with_lease(lease_lock, reconcile_loop).await?;
```

### Resource Limits

Recommended production configuration:

```yaml
resources:
  requests:
    cpu: 100m
    memory: 128Mi
  limits:
    cpu: 500m
    memory: 512Mi
```

Can handle:
- **1000+** DNS zones
- **10,000+** DNS records
- **<100ms** average reconciliation time

## Next Steps

- [Custom Resource Definitions](./crds.md) - CRD specifications
- [Controller Design](../development/controller-design.md) - Implementation details
- [Performance Tuning](../advanced/performance.md) - Optimization strategies
