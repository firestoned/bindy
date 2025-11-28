# BIND9 DNS Controller for Kubernetes

[![Main Branch CI/CD](https://github.com/firestoned/bindy/actions/workflows/main.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/main.yaml)
[![PR CI](https://github.com/firestoned/bindy/actions/workflows/pr.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/pr.yaml)
[![Integration Tests](https://github.com/firestoned/bindy/actions/workflows/integration.yaml/badge.svg)](https://github.com/firestoned/bindy/actions/workflows/integration.yaml)
[![codecov](https://codecov.io/gh/firestoned/bindy/branch/main/graph/badge.svg)](https://codecov.io/gh/firestoned/bindy)

A high-performance Kubernetes controller written in Rust using kube-rs that manages BIND9 DNS infrastructure through Custom Resource Definitions (CRDs).

## Overview

This controller watches for DNS-related CRDs (DNSZone, ARecord, TXTRecord, CNAMERecord, etc.) and automatically generates BIND9 zone configurations. It replaces the previous Python-based operator with a more efficient, compiled Rust implementation.

## Key Features

- 🚀 **High Performance** - Native Rust with async/await and zero-copy operations
- 🏷️ **Label Selectors** - Target BIND9 instances using Kubernetes label selectors
- 📝 **Dynamic Zone Management** - Automatically create and manage DNS zones
- 🔄 **Multi-Record Types** - A, AAAA, CNAME, MX, TXT, NS, SRV, CAA records
- 🎯 **Declarative DNS** - Manage DNS as Kubernetes resources
- 🔒 **Security First** - Non-root containers, RBAC-ready
- 📊 **Status Tracking** - Full status subresources for all resources

## Architecture

### CRDs

1. **Bind9Instance** - Represents a BIND9 DNS server deployment
2. **DNSZone** - Defines a DNS zone with label-based instance targeting
3. **ARecord** - IPv4 address records
4. **AAAARecord** - IPv6 address records
5. **TXTRecord** - Text records (SPF, DKIM, DMARC, etc.)
6. **CNAMERecord** - Alias records
7. **MXRecord** - Mail exchanger records
8. **NSRecord** - Nameserver records
9. **SRVRecord** - Service records
10. **CAARecord** - Certification Authority Authorization records

### Controllers

The controller runs multiple reconcilers concurrently:
- DNSZone reconciler - Creates/updates zone files based on zone specs
- ARecord reconciler - Adds A records to zone files
- TXTRecord reconciler - Adds TXT records to zone files
- CNAMERecord reconciler - Adds CNAME records to zone files
- (Additional record type reconcilers can be easily added)

## Installation

### 1. Create Namespace

```bash
kubectl create namespace dns-system
```

### 2. Install CRDs

```bash
kubectl apply -k deploy/crds/
```

This will install all Custom Resource Definitions:
- `bind9instances.dns.firestoned.io`
- `dnszones.dns.firestoned.io`
- `arecords.dns.firestoned.io`
- `aaaarecords.dns.firestoned.io`
- `cnamerecords.dns.firestoned.io`
- `mxrecords.dns.firestoned.io`
- `txtrecords.dns.firestoned.io`
- `nsrecords.dns.firestoned.io`
- `srvrecords.dns.firestoned.io`
- `caarecords.dns.firestoned.io`

### 3. Create RBAC

```bash
kubectl apply -f deploy/rbac/
```

### 4. Deploy Controller

```bash
kubectl apply -f deploy/controller/deployment.yaml
```

Wait for the controller to be ready:

```bash
kubectl wait --for=condition=available --timeout=300s deployment/bind9-controller -n dns-system
```

### 5. Create Bind9Instance Resources

After the controller is running, create your BIND9 instances.

#### Primary Instance

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
      - "10.0.0.0/8"  # Allow transfers to secondary servers
```

#### Secondary Instance

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-dns
  namespace: dns-system
  labels:
    dns-role: secondary
    environment: production
    datacenter: us-west
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer: []  # Secondaries typically don't transfer to others
```

Apply the instances:

```bash
kubectl apply -f primary-instance.yaml
kubectl apply -f secondary-instance.yaml
```

## Primary and Secondary DNS Architecture

### How Zone Transfers Work

The controller manages zone configurations on both primary and secondary BIND9 instances. Here's how they communicate:

1. **Primary Instance** - Authoritative source for zone data
   - Hosts the master copy of DNS zones
   - Accepts dynamic updates (if configured)
   - Allows zone transfers to secondaries via `allowTransfer` ACL

2. **Secondary Instance** - Read-only replica
   - Receives zone data via AXFR/IXFR from primary
   - Provides redundancy and load distribution
   - Cannot accept dynamic updates

### Example: Multi-Region DNS Setup

```yaml
# Primary in US-East
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-us-east
  namespace: dns-system
  labels:
    dns-role: primary
    region: us-east
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.1.0.0/16"  # US-West secondary network
      - "10.2.0.0/16"  # EU secondary network

---
# Secondary in US-West
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-us-west
  namespace: dns-system
  labels:
    dns-role: secondary
    region: us-west
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"

---
# Secondary in EU
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-eu
  namespace: dns-system
  labels:
    dns-role: secondary
    region: eu-central
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

### Zone Distribution to Both Primary and Secondary

When you create a DNSZone, you can target both primary and secondary instances using label selectors:

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  type: primary
  # This selector matches BOTH primary and secondary instances
  instanceSelector:
    matchExpressions:
      - key: dns-role
        operator: In
        values:
          - primary
          - secondary
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin@example.com
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
```

The controller will:
1. Generate zone configuration for the primary instance
2. Generate zone configuration for all matched secondary instances
3. BIND9's built-in zone transfer mechanism handles data replication
4. Both primary and secondary can answer queries for the zone

### Target Only Primary Instances

To create zones only on primary servers:

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: internal-only
  namespace: dns-system
spec:
  zoneName: internal.local
  type: primary
  instanceSelector:
    matchLabels:
      dns-role: primary  # Only primaries
  soaRecord:
    primaryNS: ns1.internal.local.
    adminEmail: admin@internal.local
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
```

## Usage Examples

### Creating a Zone with Label Selector

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  type: primary
  # Target instances with matching labels
  instanceSelector:
    matchLabels:
      dns-role: primary
      environment: production
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin@example.com
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
```

### Advanced Label Selection with Expressions

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: internal-local
  namespace: dns-system
spec:
  zoneName: internal.local
  type: primary
  # Use label expressions for complex selectors
  instanceSelector:
    matchExpressions:
      - key: dns-role
        operator: In
        values:
          - primary
          - secondary
      - key: environment
        operator: In
        values:
          - production
          - staging
  soaRecord:
    primaryNS: ns1.internal.local.
    adminEmail: admin@internal.local
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
```

### Adding DNS Records

```yaml
# A Record
apiVersion: dns.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zone: example-com  # References DNSZone name
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300

---
# CNAME Record
apiVersion: dns.firestoned.io/v1alpha1
kind: CNAMERecord
metadata:
  name: blog-example
  namespace: dns-system
spec:
  zone: example-com
  name: blog
  target: www.example.com.
  ttl: 300

---
# TXT Record
apiVersion: dns.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: spf-example
  namespace: dns-system
spec:
  zone: example-com
  name: "@"
  text:
    - "v=spf1 include:_spf.example.com ~all"
  ttl: 3600

---
# MX Record
apiVersion: dns.firestoned.io/v1alpha1
kind: MXRecord
metadata:
  name: mail-example
  namespace: dns-system
spec:
  zone: example-com
  name: "@"
  priority: 10
  mailServer: mail.example.com.
  ttl: 3600
```

## Development

### Prerequisites

- Rust 1.70+
- Cargo
- Docker (for building images)
- Kubernetes 1.24+

### Building Locally

```bash
cd controller
cargo build
```

### Running Tests

```bash
cd controller
cargo test
```

### Building Docker Image

```bash
docker build -t bind9-controller:latest .
```

### Project Structure

```
controller/
├── Cargo.toml              # Rust dependencies
├── src/
│   ├── main.rs            # Entry point with controller loop
│   ├── lib.rs             # Library exports
│   ├── crd.rs             # CRD type definitions
│   ├── bind9.rs           # BIND9 zone file management
│   └── reconcilers/
│       ├── mod.rs         # Reconciler module
│       ├── dnszone.rs     # DNSZone reconciler
│       └── records.rs     # DNS record reconcilers
```

### Key Dependencies

- **kube-rs** - Kubernetes client library
- **tokio** - Async runtime
- **serde** - Serialization/deserialization
- **tracing** - Structured logging

## Configuration

The controller is configured via environment variables:

- `RUST_LOG` - Log level (default: `info`)
- `BIND9_ZONES_DIR` - Directory for zone files (default: `/etc/bind/zones`)

## How It Works

### DNSZone Reconciliation

1. Controller watches for DNSZone resources
2. When a zone is created/updated:
   - Extracts zone specification
   - Evaluates label selector against Bind9Instance resources
   - Creates zone file with SOA record
   - Updates resource status with matched instances count

### DNS Record Reconciliation

1. Controller watches for record resources (A, CNAME, TXT, etc.)
2. When a record is created/updated:
   - Validates zone reference exists
   - Appends record to zone file
   - Updates resource status

### Cleanup

- When resources are deleted, corresponding entries are removed from zone files
- Finalizers can be used to ensure clean deletion

## Status Subresources

All resources include status subresources to track reconciliation state:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Synchronized
      message: Zone created for 2 instances
      lastTransitionTime: 2024-01-01T00:00:00Z
  observedGeneration: 1
  recordCount: 5
```

## RBAC

The controller requires the following permissions:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bind9-controller
rules:
  # DNSZone resources
  - apiGroups: ["dns.firestoned.io"]
    resources: ["dnszones", "dnszones/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  
  # Record resources
  - apiGroups: ["dns.firestoned.io"]
    resources: ["arecords", "aaaarecords", "txtrecords", "cnamerecords", "mxrecords", "nsrecords", "srvrecords", "caarecords"]
    resources: ["*/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  
  # Bind9Instance resources
  - apiGroups: ["dns.firestoned.io"]
    resources: ["bind9instances", "bind9instances/status"]
    verbs: ["get", "list", "watch"]
```

## Performance Characteristics

- **Startup Time** - <1 second
- **Memory Usage** - ~50MB baseline
- **Zone Creation Latency** - <100ms per zone
- **Record Addition Latency** - <50ms per record
- **Controller Overhead** - Negligible CPU when idle

## Troubleshooting

### Check Controller Logs

```bash
kubectl logs -n dns-system -l app=bind9-controller -f
```

### Verify CRDs are Installed

```bash
kubectl get crd | grep dns.firestoned.io
```

### Check Resource Status

```bash
kubectl get dnszones -o wide
kubectl describe dnszone example-com
```

### Verify Zone Files

```bash
kubectl exec -it <bind9-pod> -- ls -la /etc/bind/zones/
kubectl exec -it <bind9-pod> -- cat /etc/bind/zones/db.example.com
```

## Migration from Python Operator

If migrating from the Python operator:

1. Backup existing zone files
2. Install new CRDs
3. Deploy Rust controller
4. Update Bind9Instance resources with labels
5. Update DNSZone resources to use `instanceSelector` instead of `bind9InstanceRef`
6. Verify zones are recreated in controller logs
7. Decommission Python operator

## Future Enhancements

- [ ] DNSSEC key management
- [ ] Automatic serial number increment
- [ ] Zone transfer synchronization
- [ ] DNS query statistics and monitoring
- [ ] Zone validation and testing
- [ ] Multi-cluster DNS federation

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
4. Check with clippy: `cargo clippy`

See the [Contributing Guide](https://firestoned.github.io/bindy/development/contributing.html) for more details.

## License

MIT - See LICENSE file for details

## Support

For issues, questions, or suggestions:
- GitHub Issues: https://github.com/firestoned/bindy/issues
- GitHub Discussions: https://github.com/firestoned/bindy/discussions
