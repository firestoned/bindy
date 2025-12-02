# BIND9 DNS Controller for Kubernetes

[![Main Branch CI/CD](https://github.com/firestoned/bindy/actions/workflows/main.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/main.yaml)
[![PR CI](https://github.com/firestoned/bindy/actions/workflows/pr.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/pr.yaml)
[![Integration Tests](https://github.com/firestoned/bindy/actions/workflows/integration.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/integration.yaml)
[![codecov](https://codecov.io/gh/firestoned/bindy/branch/main/graph/badge.svg)](https://codecov.io/gh/firestoned/bindy)

A high-performance Kubernetes operator written in Rust using kube-rs that manages BIND9 DNS infrastructure through Custom Resource Definitions (CRDs).

## Overview

Bindy is a cloud-native DNS controller that brings declarative DNS management to Kubernetes. It watches for DNS-related CRDs and automatically provisions, configures, and manages BIND9 DNS infrastructure using industry-standard RNDC (Remote Name Daemon Control) protocol for dynamic DNS updates.

## Key Features

- 🚀 **High Performance** - Native Rust with async/await and zero-copy operations
- 🏗️ **Cluster Management** - Manage logical DNS clusters with automatic instance provisioning
- 🔄 **Dynamic DNS Updates** - Real-time record updates via RNDC protocol
- 📝 **Multi-Record Types** - A, AAAA, CNAME, MX, TXT, NS, SRV, CAA records
- 🎯 **Declarative Configuration** - Manage DNS as Kubernetes resources with full GitOps support
- 🔒 **Security First** - Non-root containers, RBAC-ready, mTLS for RNDC communication
- 📊 **Full Observability** - Status tracking, resource annotations, Prometheus metrics
- 🏆 **High Availability** - Leader election support with automatic failover (~15s)
- 🔐 **DNSSEC Support** - Automated DNSSEC key management and zone signing
- 🎨 **Resource Tracking** - Automatic annotations linking records to clusters, instances, and zones

## Architecture

### Custom Resource Definitions (CRDs)

#### Infrastructure Resources

1. **Bind9Cluster** (`bind9clusters.bindy.firestoned.io`) - Logical DNS cluster definition
   - Manages multiple Bind9Instance resources automatically
   - Supports primary and secondary server roles
   - Global configuration inherited by all instances
   - Automatic scaling and self-healing

2. **Bind9Instance** (`bind9instances.bindy.firestoned.io`) - Individual BIND9 server deployment
   - Can be standalone or cluster-managed
   - Generates Kubernetes Deployment, Service, ConfigMap, and Secret
   - RNDC key generation and rotation
   - Customizable BIND9 configuration

#### DNS Management Resources

3. **DNSZone** (`dnszones.bindy.firestoned.io`) - DNS zone definition with SOA records
   - References Bind9Cluster for zone placement
   - Automatic SOA record generation
   - DNSSEC configuration support

#### DNS Record Types

4. **ARecord** (`arecords.bindy.firestoned.io`) - IPv4 address records
5. **AAAARecord** (`aaaarecords.bindy.firestoned.io`) - IPv6 address records
6. **TXTRecord** (`txtrecords.bindy.firestoned.io`) - Text records (SPF, DKIM, DMARC, etc.)
7. **CNAMERecord** (`cnamerecords.bindy.firestoned.io`) - Canonical name (alias) records
8. **MXRecord** (`mxrecords.bindy.firestoned.io`) - Mail exchanger records
9. **NSRecord** (`nsrecords.bindy.firestoned.io`) - Nameserver delegation records
10. **SRVRecord** (`srvrecords.bindy.firestoned.io`) - Service location records
11. **CAARecord** (`caarecords.bindy.firestoned.io`) - Certificate Authority Authorization records

### Controllers

The operator runs multiple reconcilers concurrently:
- **Bind9Cluster reconciler** - Manages DNS cluster lifecycle and instance provisioning
- **Bind9Instance reconciler** - Creates/updates BIND9 deployments, services, and configuration
- **DNSZone reconciler** - Manages DNS zones on target instances
- **Record reconcilers** - Add/update/delete DNS records via RNDC (A, AAAA, CNAME, TXT, MX, NS, SRV, CAA)

### Leader Election

For high availability, Bindy supports leader election using Kubernetes Lease API:
- Multiple controller replicas can run simultaneously
- Only one instance actively reconciles resources (leader)
- Automatic failover if leader becomes unavailable (~15 seconds)
- Non-leader instances stand by ready for immediate takeover

## Installation

### 1. Create Namespace

```bash
kubectl create namespace dns-system
```

### 2. Install CRDs

```bash
kubectl apply -f deploy/crds/
```

This will install all Custom Resource Definitions:
- `bind9clusters.bindy.firestoned.io`
- `bind9instances.bindy.firestoned.io`
- `dnszones.bindy.firestoned.io`
- `arecords.bindy.firestoned.io`
- `aaaarecords.bindy.firestoned.io`
- `cnamerecords.bindy.firestoned.io`
- `mxrecords.bindy.firestoned.io`
- `txtrecords.bindy.firestoned.io`
- `nsrecords.bindy.firestoned.io`
- `srvrecords.bindy.firestoned.io`
- `caarecords.bindy.firestoned.io`

### 3. Create RBAC

```bash
kubectl apply -f deploy/rbac/
```

### 4. Deploy Controller

```bash
kubectl apply -f deploy/operator/deployment.yaml
```

Wait for the controller to be ready:

```bash
kubectl wait --for=condition=available --timeout=300s deployment/bindy -n dns-system
```

### 5. Create a DNS Cluster

The easiest way to get started is with a Bind9Cluster, which automatically manages instances for you:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: my-dns-cluster
  namespace: dns-system
spec:
  # Global configuration inherited by all instances
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"

  # Primary servers
  primary:
    replicas: 2
    version: "9.18"
    config:
      dnssec:
        enabled: true
        validation: true

  # Secondary servers (optional)
  secondary:
    replicas: 2
    version: "9.18"
```

Apply the cluster:

```bash
kubectl apply -f my-dns-cluster.yaml
```

The controller will automatically create:
- 2 primary Bind9Instance resources (`primary-0`, `primary-1`)
- 2 secondary Bind9Instance resources (`secondary-0`, `secondary-1`)
- Kubernetes Deployments, Services, ConfigMaps, and Secrets for each instance
- RNDC keys for secure dynamic DNS updates

## Quick Start: Creating DNS Records

### 1. Create a DNS Zone

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: my-dns-cluster
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2025010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
```

### 2. Add DNS Records

```yaml
---
# A Record
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example-com
  namespace: dns-system
spec:
  zoneRef: example-com  # References DNSZone resource name
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300

---
# CNAME Record
apiVersion: bindy.firestoned.io/v1alpha1
kind: CNAMERecord
metadata:
  name: blog-example-com
  namespace: dns-system
spec:
  zoneRef: example-com
  name: blog
  target: www.example.com.
  ttl: 300

---
# MX Record
apiVersion: bindy.firestoned.io/v1alpha1
kind: MXRecord
metadata:
  name: mail-example-com
  namespace: dns-system
spec:
  zoneRef: example-com
  name: "@"
  priority: 10
  mailServer: mail.example.com.
  ttl: 3600

---
# TXT Record (SPF)
apiVersion: bindy.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: spf-example-com
  namespace: dns-system
spec:
  zoneRef: example-com
  name: "@"
  text:
    - "v=spf1 include:_spf.example.com ~all"
  ttl: 3600
```

Apply the records:

```bash
kubectl apply -f dns-records.yaml
```

### 3. Verify Records

Check that records were created successfully:

```bash
# View record status
kubectl get arecords,cnamerecords,mxrecords,txtrecords -n dns-system

# Check detailed status with annotations
kubectl describe arecord www-example-com -n dns-system
```

Each record will have annotations showing which cluster, instance, and zone it's associated with:
```yaml
metadata:
  annotations:
    bindy.firestoned.io/cluster: my-dns-cluster
    bindy.firestoned.io/instance: primary-0
    bindy.firestoned.io/zone: example.com
```

### 4. Test DNS Resolution

```bash
# Get the DNS service endpoint
kubectl get svc -n dns-system

# Test DNS query
dig @<service-ip> www.example.com A
```

## Advanced Configuration

### Standalone Bind9Instance

You can also create standalone instances (not managed by a cluster):

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: standalone-dns
  namespace: dns-system
spec:
  replicas: 1
  version: "9.18"
  role: primary
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

### Configuration Inheritance

When using Bind9Cluster, configuration follows this priority order:
1. **Instance-specific config** (highest priority)
2. **Role-specific config** (primary/secondary)
3. **Global config** (cluster-wide defaults)
4. **System defaults** (lowest priority)

Example:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"

  primary:
    replicas: 3
    allowTransfer:  # Primary-specific: allow transfers
      - "10.0.0.0/8"

  secondary:
    replicas: 2
    # Secondaries inherit global.allowQuery
    # Secondaries don't need allowTransfer
```

### Custom Volumes

Add persistent storage for zone data:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: persistent-dns
  namespace: dns-system
spec:
  primary:
    replicas: 2
    volumes:
      - name: zone-data
        persistentVolumeClaim:
          claimName: dns-zones-pvc
    volumeMounts:
      - name: zone-data
        mountPath: /var/lib/bind
```

## Development

### Prerequisites

- Rust 1.87+
- Cargo
- Docker (for building images)
- Kubernetes 1.27+

### Building Locally

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Building Docker Image

```bash
docker build -t bindy:latest .
```

### Project Structure

```
bindy/
├── Cargo.toml              # Rust dependencies
├── src/
│   ├── main.rs            # Entry point with leader election
│   ├── lib.rs             # Library exports
│   ├── crd.rs             # CRD type definitions
│   ├── bind9.rs           # BIND9 management and RNDC client
│   ├── bind9_resources.rs # Kubernetes resource generation
│   └── reconcilers/
│       ├── mod.rs         # Reconciler module
│       ├── bind9cluster.rs    # Bind9Cluster reconciler
│       ├── bind9instance.rs   # Bind9Instance reconciler
│       ├── dnszone.rs     # DNSZone reconciler
│       └── records.rs     # DNS record reconcilers
├── deploy/
│   ├── crds/              # Generated CRD YAML files
│   ├── rbac/              # RBAC manifests
│   └── operator/          # Operator deployment
├── examples/              # Example resource manifests
└── docs/                  # Documentation (mdBook)
```

### Generating CRDs

CRD YAML files are auto-generated from Rust types:

```bash
# Regenerate CRD YAML files from src/crd.rs
cargo run --bin crdgen

# Regenerate API documentation
cargo run --bin crddoc > docs/src/reference/api.md
```

**IMPORTANT:** Never edit the YAML files in `deploy/crds/` directly - they will be overwritten. Always edit `src/crd.rs` and regenerate.

### Key Dependencies

- **kube** (2.0) - Kubernetes client library
- **kube-lease-manager** (0.10) - Leader election support
- **tokio** (1.x) - Async runtime
- **serde** (1.x) - Serialization/deserialization
- **tracing** (0.1) - Structured logging
- **rndc** (0.1.3) - RNDC protocol client for dynamic DNS updates

## Configuration

The controller is configured via environment variables:

### Core Settings
- `RUST_LOG` - Log level (default: `info`, example: `debug`)
- `POD_NAME` - Pod name (auto-injected by Kubernetes)
- `POD_NAMESPACE` - Pod namespace (auto-injected by Kubernetes)

### Leader Election Settings
- `BINDY_ENABLE_LEADER_ELECTION` - Enable leader election (default: `true`)
- `BINDY_LEASE_NAME` - Lease resource name (default: `bindy-leader`)
- `BINDY_LEASE_DURATION_SECONDS` - Lease duration (default: `15`)
- `BINDY_LEASE_RENEW_DEADLINE_SECONDS` - Renew deadline (default: `10`)
- `BINDY_LEASE_RETRY_PERIOD_SECONDS` - Retry period (default: `2`)

## How It Works

### Bind9Cluster Reconciliation

1. Controller watches for Bind9Cluster resources
2. When a cluster is created/updated:
   - Calculates desired instance count from primary/secondary replicas
   - Creates/updates managed Bind9Instance resources
   - Applies global configuration to all instances
   - Handles scale-up (creates new instances) and scale-down (deletes excess instances)
   - Implements self-healing if child resources are deleted

### Bind9Instance Reconciliation

1. Controller watches for Bind9Instance resources
2. When an instance is created/updated:
   - Generates RNDC key for secure communication
   - Creates Kubernetes Secret with RNDC key
   - Generates BIND9 configuration (named.conf, options)
   - Creates ConfigMap with BIND9 configuration
   - Creates Deployment running BIND9
   - Creates Service for DNS access (UDP/TCP port 53, RNDC port 953)
   - Updates status with instance readiness

### DNSZone Reconciliation

1. Controller watches for DNSZone resources
2. When a zone is created/updated:
   - Looks up referenced Bind9Cluster
   - Finds available Bind9Instance from cluster
   - Connects to instance via RNDC
   - Creates zone with SOA record
   - Updates resource status

### DNS Record Reconciliation

1. Controller watches for record resources (A, AAAA, CNAME, TXT, MX, NS, SRV, CAA)
2. When a record is created/updated:
   - Looks up referenced DNSZone
   - Finds associated Bind9Cluster and instance
   - **Adds tracking annotations** to record resource:
     - `bindy.firestoned.io/cluster` - Bind9Cluster name
     - `bindy.firestoned.io/instance` - Bind9Instance being used
     - `bindy.firestoned.io/zone` - DNS zone name
   - Connects to BIND9 instance via RNDC
   - Adds/updates DNS record dynamically
   - Updates resource status with result

### Resource Cleanup

- **Finalizers** ensure proper cleanup before deletion
- **Cluster-managed instances**: Cluster reconciler handles deletion
- **Standalone instances**: Instance reconciler deletes all child resources (Deployment, Service, ConfigMap, Secret)
- **DNS records**: Removed from zone via RNDC before resource deletion

## Status Subresources

All resources include status subresources to track reconciliation state:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: RecordCreated
      message: A record www.example.com created successfully
      lastTransitionTime: 2025-01-01T00:00:00Z
  observedGeneration: 1
```

## RBAC

The controller requires the following permissions:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy
rules:
  # Bind9Cluster resources
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["bind9clusters", "bind9clusters/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # Bind9Instance resources
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["bind9instances", "bind9instances/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # DNSZone resources
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["dnszones", "dnszones/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # Record resources
  - apiGroups: ["bindy.firestoned.io"]
    resources:
      - "arecords"
      - "aaaarecords"
      - "txtrecords"
      - "cnamerecords"
      - "mxrecords"
      - "nsrecords"
      - "srvrecords"
      - "caarecords"
      - "*/status"
    verbs: ["get", "list", "watch", "update", "patch"]

  # Kubernetes core resources (for managing BIND9 deployments)
  - apiGroups: [""]
    resources: ["configmaps", "secrets", "services"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  - apiGroups: ["apps"]
    resources: ["deployments"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # Leader election
  - apiGroups: ["coordination.k8s.io"]
    resources: ["leases"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]

  # Events for logging
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["create", "patch"]
```

## Performance Characteristics

- **Startup Time** - <1 second
- **Memory Usage** - ~50-100MB per controller instance
- **Reconciliation Latency**:
  - Zone creation: <200ms
  - Record addition via RNDC: <100ms
  - Cluster provisioning: <5s (depending on image pull time)
- **Controller Overhead** - Negligible CPU when idle
- **Leader Election Failover** - ~15 seconds (configurable)

## Troubleshooting

### Check Controller Logs

```bash
kubectl logs -n dns-system -l app=bindy -f
```

### Verify Leader Election

```bash
# Check which pod is the current leader
kubectl get lease bindy-leader -n dns-system -o jsonpath='{.spec.holderIdentity}'

# View leader election logs
kubectl logs -n dns-system -l app=bindy | grep -i leader
```

### Verify CRDs are Installed

```bash
kubectl get crd | grep bindy.firestoned.io
```

### Check Resource Status

```bash
# Clusters
kubectl get bind9clusters -n dns-system -o wide
kubectl describe bind9cluster my-dns-cluster -n dns-system

# Instances
kubectl get bind9instances -n dns-system -o wide
kubectl describe bind9instance primary-0 -n dns-system

# Zones
kubectl get dnszones -n dns-system -o wide
kubectl describe dnszone example-com -n dns-system

# Records
kubectl get arecords,txtrecords,cnamerecords,mxrecords -n dns-system
kubectl describe arecord www-example-com -n dns-system
```

### Check Record Annotations

View which cluster, instance, and zone serve a record:

```bash
kubectl get arecord www-example-com -n dns-system -o jsonpath='{.metadata.annotations}'
```

### Verify BIND9 Configuration

```bash
# Get BIND9 pod name
kubectl get pods -n dns-system -l app.kubernetes.io/name=bind9

# View BIND9 configuration
kubectl exec -it <bind9-pod> -n dns-system -- cat /etc/bind/named.conf

# Test BIND9 configuration
kubectl exec -it <bind9-pod> -n dns-system -- named-checkconf /etc/bind/named.conf

# View zone file (if using file-based zones)
kubectl exec -it <bind9-pod> -n dns-system -- cat /etc/bind/zones/db.example.com

# Test DNS query
kubectl exec -it <bind9-pod> -n dns-system -- dig @localhost example.com SOA
```

### Check RNDC Communication

```bash
# Get RNDC key from secret
kubectl get secret <instance-name>-rndc-key -n dns-system -o jsonpath='{.data.rndc-key}' | base64 -d

# Test RNDC from within cluster
kubectl run -it --rm debug --image=nicolaka/netshoot --restart=Never -- \
  dig @primary-0.dns-system.svc.cluster.local example.com SOA
```

## Documentation

Complete documentation is available at [https://firestoned.github.io/bindy/](https://firestoned.github.io/bindy/)

To build and view documentation locally:

```bash
make docs-serve
# Navigate to http://localhost:3000
```

The documentation includes:
- **Installation Guide** - Step-by-step setup instructions
- **User Guide** - Creating DNS infrastructure, zones, and records
- **Operations** - Configuration, monitoring, and troubleshooting
- **Advanced Topics** - High availability, security, performance tuning
- **Developer Guide** - Contributing and development workflow
- **API Reference** - Complete CRD specifications and examples

## Contributing

Contributions are welcome! Please ensure:

1. Code compiles without warnings
2. Tests pass: `cargo test`
3. Format code: `cargo fmt`
4. Check with clippy: `cargo clippy -- -D warnings`
5. Update CHANGELOG.md with your changes
6. Regenerate CRDs if you modified `src/crd.rs`: `cargo run --bin crdgen`

See the [Contributing Guide](https://firestoned.github.io/bindy/development/contributing.html) for more details.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for full details.

**SPDX-License-Identifier:** MIT

**Copyright (c) 2025 Erick Bourgeois, firestoned**

All source code files include SPDX license identifiers for easy machine-readable license information.

### What This Means

- ✅ **Free to use** - Personal and commercial use permitted
- ✅ **Modify freely** - Create derivative works and modifications
- ✅ **Distribute** - Share the original or modified versions
- ✅ **Private use** - Use in proprietary/closed-source projects
- ⚠️ **No warranty** - Provided "as is" without warranty of any kind
- ℹ️ **Attribution** - Include copyright notice in substantial portions

For more information about the MIT License, visit [https://opensource.org/licenses/MIT](https://opensource.org/licenses/MIT)

## Support

For issues, questions, or suggestions:
- GitHub Issues: https://github.com/firestoned/bindy/issues
- GitHub Discussions: https://github.com/firestoned/bindy/discussions
