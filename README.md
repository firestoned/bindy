# BIND9 DNS Operator for Kubernetes

A Kubernetes operator built with Kopf that manages BIND9 DNS infrastructure through Custom Resource Definitions (CRDs).

## Features

- 🚀 **Declarative DNS Management** - Manage DNS infrastructure as Kubernetes resources
- 🔄 **Full DNS Record Support** - A, AAAA, CNAME, MX, TXT, NS, SRV, PTR, CAA, NAPTR records
- 🎯 **Automatic Configuration** - Generates BIND9 configs from CRDs
- 📊 **Health Monitoring** - Built-in health checks and status reporting
- 🔒 **Secure by Default** - Non-root containers, RBAC, security contexts
- 🎨 **Production Ready** - Error handling, retries, status conditions

## Quick Start

### Prerequisites

- Kubernetes cluster (1.24+)
- kubectl configured
- Poetry installed (for development)

### Installation

1. **Install CRDs:**
```bash
kubectl apply -f deploy/crds/dns-crds.yaml
```

2. **Create namespace and RBAC:**
```bash
kubectl create namespace dns-system
kubectl apply -f deploy/rbac/
```

3. **Deploy operator:**
```bash
kubectl apply -f deploy/operator/
```

### Usage Examples

**Create a BIND9 Instance:**
```yaml
apiVersion: dns.example.com/v1alpha1
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
```

**Create a DNS Zone:**
```yaml
apiVersion: dns.example.com/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  type: master
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

**Add DNS Records:**
```yaml
apiVersion: dns.example.com/v1alpha1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zone: example-com
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
---
apiVersion: dns.example.com/v1alpha1
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

### Setup Development Environment

1. **Clone repository:**
```bash
git clone https://github.com/firestoned/bindy.git
cd bindy
```

2. **Install dependencies:**
```bash
poetry install
```

3. **Run tests:**
```bash
poetry run pytest
```

4. **Run operator locally:**
```bash
poetry run bind9-operator
```

### Project Structure

```
bindy/
├── operator/              # Main operator code
│   ├── handlers/         # Resource handlers
│   ├── config/           # Configuration generation
│   ├── kubernetes/       # K8s resource management
│   └── utils/            # Utilities
├── deploy/               # Kubernetes manifests
│   ├── crds/            # Custom Resource Definitions
│   ├── rbac/            # RBAC resources
│   └── operator/        # Operator deployment
└── tests/               # Test suite
```

### Building

**Build Docker image:**
```bash
make docker-build
```

**Push to registry:**
```bash
make docker-push
```

## Configuration

### Environment Variables

- `OPERATOR_NAMESPACE` - Namespace to watch (optional, watches all if not set)
- `KOPF_

