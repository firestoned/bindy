# Architecture Overview

This guide explains the Bindy architecture, focusing on the dual-cluster model that enables multi-tenancy and flexible deployment patterns.

## Table of Contents

- [Architecture Principles](#architecture-principles)
- [Cluster Models](#cluster-models)
- [Resource Hierarchy](#resource-hierarchy)
- [Reconciliation Flow](#reconciliation-flow)
- [Multi-Tenancy Model](#multi-tenancy-model)
- [Namespace Isolation](#namespace-isolation)

## Architecture Principles

Bindy follows Kubernetes controller pattern best practices:

1. **Declarative Configuration**: Users declare desired state via CRDs, controllers reconcile to match
2. **Level-Based Reconciliation**: Controllers continuously ensure actual state matches desired state
3. **Status Subresources**: All CRDs expose status for observability
4. **Finalizers**: Proper cleanup of dependent resources before deletion
5. **Generation Tracking**: Reconcile only when spec changes (using `metadata.generation`)

## Cluster Models

Bindy provides two cluster models to support different organizational patterns:

### Namespace-Scoped Clusters (`Bind9Cluster`)

**Use Case**: Development teams manage their own DNS infrastructure within their namespace.

```mermaid
graph TB
    subgraph "Namespace: dev-team-alpha"
        Cluster[Bind9Cluster<br/>dev-team-dns]
        Zone1[DNSZone<br/>app.example.com]
        Zone2[DNSZone<br/>test.local]
        Record1[ARecord<br/>www]
        Record2[MXRecord<br/>mail]

        Cluster --> Zone1
        Cluster --> Zone2
        Zone1 --> Record1
        Zone2 --> Record2
    end

    style Cluster fill:#e1f5ff
    style Zone1 fill:#fff4e1
    style Zone2 fill:#fff4e1
    style Record1 fill:#f0f0f0
    style Record2 fill:#f0f0f0
```

**Characteristics:**
- Isolated to a single namespace
- Teams manage their own DNS independently
- RBAC scoped to namespace (Role/RoleBinding)
- Cannot be referenced from other namespaces

**YAML Example:**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: dev-team-dns
  namespace: dev-team-alpha
spec:
  version: "9.18"
  primary:
    replicas: 1
  secondary:
    replicas: 1
```

### Cluster-Scoped Clusters (`Bind9GlobalCluster`)

**Use Case**: Platform teams provide shared DNS infrastructure accessible from all namespaces.

```mermaid
graph TB
    subgraph "Cluster-Scoped (no namespace)"
        GlobalCluster[Bind9GlobalCluster<br/>shared-production-dns]
    end

    subgraph "Namespace: production"
        Zone1[DNSZone<br/>api.example.com]
        Record1[ARecord<br/>api]
    end

    subgraph "Namespace: staging"
        Zone2[DNSZone<br/>staging.example.com]
        Record2[ARecord<br/>app]
    end

    GlobalCluster -.-> Zone1
    GlobalCluster -.-> Zone2
    Zone1 --> Record1
    Zone2 --> Record2

    style GlobalCluster fill:#c8e6c9
    style Zone1 fill:#fff4e1
    style Zone2 fill:#fff4e1
    style Record1 fill:#f0f0f0
    style Record2 fill:#f0f0f0
```

**Characteristics:**
- Cluster-wide visibility (no namespace)
- Platform team manages centralized DNS
- RBAC requires ClusterRole/ClusterRoleBinding
- DNSZones in any namespace can reference it

**YAML Example:**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9GlobalCluster
metadata:
  name: shared-production-dns
  # No namespace - cluster-scoped resource
spec:
  version: "9.18"
  primary:
    replicas: 3
    service:
      type: LoadBalancer
  secondary:
    replicas: 2
```

## Resource Hierarchy

The complete resource hierarchy shows how components relate:

```mermaid
graph TD
    subgraph "Cluster-Scoped Resources"
        GlobalCluster[Bind9GlobalCluster]
    end

    subgraph "Namespace-Scoped Resources"
        Cluster[Bind9Cluster]
        Zone[DNSZone]
        Instance[Bind9Instance]
        Records[DNS Records<br/>A, AAAA, CNAME, MX, etc.]
    end

    GlobalCluster -.globalClusterRef.-> Zone
    Cluster --clusterRef--> Zone

    Cluster --cluster_ref--> Instance
    GlobalCluster -.cluster_ref.-> Instance

    Zone --> Records

    style GlobalCluster fill:#c8e6c9
    style Cluster fill:#e1f5ff
    style Zone fill:#fff4e1
    style Instance fill:#ffe1e1
    style Records fill:#f0f0f0
```

### Key Relationships

1. **DNSZone → Cluster References**:
   - `spec.clusterRef`: References namespace-scoped `Bind9Cluster` (same namespace)
   - `spec.globalClusterRef`: References cluster-scoped `Bind9GlobalCluster`
   - **Mutual Exclusivity**: Exactly one must be specified

2. **Bind9Instance → Cluster Reference**:
   - `spec.cluster_ref`: Can reference either `Bind9Cluster` or `Bind9GlobalCluster`
   - Controller auto-detects cluster type

3. **DNS Records → Zone Reference**:
   - `spec.zone`: Zone name lookup (searches in same namespace)
   - `spec.zoneRef`: Direct DNSZone resource name (same namespace)
   - **Namespace Isolation**: Records can ONLY reference zones in their own namespace

## Reconciliation Flow

### DNSZone Reconciliation

```mermaid
sequenceDiagram
    participant K8s as Kubernetes API
    participant Controller as DNSZone Controller
    participant Cluster as Bind9Cluster/GlobalCluster
    participant Instances as Bind9Instances
    participant BIND9 as BIND9 Pods

    K8s->>Controller: DNSZone created/updated
    Controller->>Controller: Check metadata.generation vs status.observedGeneration
    alt Spec unchanged
        Controller->>K8s: Skip reconciliation (status-only update)
    else Spec changed
        Controller->>Controller: Validate clusterRef XOR globalClusterRef
        Controller->>Cluster: Get cluster by clusterRef or globalClusterRef
        Controller->>Instances: List instances by cluster reference
        Controller->>BIND9: Update zone files via Bindcar API
        Controller->>K8s: Update status (observedGeneration, conditions)
    end
```

### Bind9GlobalCluster Reconciliation

```mermaid
sequenceDiagram
    participant K8s as Kubernetes API
    participant Controller as GlobalCluster Controller
    participant Instances as Bind9Instances (all namespaces)

    K8s->>Controller: Bind9GlobalCluster created/updated
    Controller->>Controller: Check generation changed
    Controller->>Instances: List all instances across all namespaces
    Controller->>Controller: Filter instances by cluster_ref
    Controller->>Controller: Calculate cluster status
    Note over Controller: - Count ready instances<br/>- Aggregate conditions<br/>- Format instance names as namespace/name
    Controller->>K8s: Update status with aggregated health
```

## Multi-Tenancy Model

Bindy supports multi-tenancy through two organizational patterns:

### Platform Team Pattern

Platform teams manage cluster-wide DNS infrastructure:

```mermaid
graph TB
    subgraph "Platform Team (ClusterRole)"
        PlatformAdmin[Platform Admin]
    end

    subgraph "Cluster-Scoped"
        GlobalCluster[Bind9GlobalCluster<br/>production-dns]
    end

    subgraph "Namespace: app-a"
        Zone1[DNSZone<br/>app-a.example.com]
        Instance1[Bind9Instance<br/>primary-app-a]
    end

    subgraph "Namespace: app-b"
        Zone2[DNSZone<br/>app-b.example.com]
        Instance2[Bind9Instance<br/>primary-app-b]
    end

    PlatformAdmin -->|manages| GlobalCluster
    GlobalCluster -.->|referenced by| Zone1
    GlobalCluster -.->|referenced by| Zone2
    GlobalCluster -->|references| Instance1
    GlobalCluster -->|references| Instance2

    style PlatformAdmin fill:#ff9800
    style GlobalCluster fill:#c8e6c9
    style Zone1 fill:#fff4e1
    style Zone2 fill:#fff4e1
```

**RBAC Setup:**
```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: platform-dns-admin
rules:
- apiGroups: ["bindy.firestoned.io"]
  resources: ["bind9globalclusters"]
  verbs: ["*"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: platform-team-dns
subjects:
- kind: Group
  name: platform-team
  apiGroup: rbac.authorization.k8s.io
roleRef:
  kind: ClusterRole
  name: platform-dns-admin
  apiGroup: rbac.authorization.k8s.io
```

### Development Team Pattern

Development teams manage namespace-scoped DNS:

```mermaid
graph TB
    subgraph "Namespace: dev-team-alpha (Role)"
        DevAdmin[Dev Team Admin]
        Cluster[Bind9Cluster<br/>dev-dns]
        Zone[DNSZone<br/>dev.example.com]
        Records[DNS Records]
        Instance[Bind9Instance]
    end

    DevAdmin -->|manages| Cluster
    DevAdmin -->|manages| Zone
    DevAdmin -->|manages| Records
    Cluster --> Instance
    Cluster --> Zone
    Zone --> Records

    style DevAdmin fill:#2196f3
    style Cluster fill:#e1f5ff
    style Zone fill:#fff4e1
```

**RBAC Setup:**
```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: dns-admin
  namespace: dev-team-alpha
rules:
- apiGroups: ["bindy.firestoned.io"]
  resources: ["bind9clusters", "dnszones", "arecords", "cnamerecords", "mxrecords", "txtrecords"]
  verbs: ["*"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: dev-team-dns
  namespace: dev-team-alpha
subjects:
- kind: Group
  name: dev-team-alpha
  apiGroup: rbac.authorization.k8s.io
roleRef:
  kind: Role
  name: dns-admin
  apiGroup: rbac.authorization.k8s.io
```

## Namespace Isolation

**Security Principle**: DNSZones and records are always namespace-scoped, even when referencing cluster-scoped resources.

```mermaid
graph TB
    subgraph "Cluster-Scoped"
        GlobalCluster[Bind9GlobalCluster<br/>shared-dns]
    end

    subgraph "Namespace: team-a"
        ZoneA[DNSZone<br/>team-a.example.com]
        RecordA[ARecord<br/>www]
    end

    subgraph "Namespace: team-b"
        ZoneB[DNSZone<br/>team-b.example.com]
        RecordB[ARecord<br/>api]
    end

    GlobalCluster -.-> ZoneA
    GlobalCluster -.-> ZoneB
    ZoneA --> RecordA
    ZoneB --> RecordB

    RecordA -.X|blocked|ZoneB
    RecordB -.X|blocked|ZoneA

    style GlobalCluster fill:#c8e6c9
    style ZoneA fill:#fff4e1
    style ZoneB fill:#fff4e1
```

**Isolation Rules:**

1. **Records can ONLY reference zones in their own namespace**
   - Controller uses `Api::namespaced()` to enforce this
   - Cross-namespace references are impossible

2. **DNSZones are namespace-scoped**
   - Even when referencing `Bind9GlobalCluster`
   - Each team manages their own zones

3. **RBAC controls zone management**
   - Platform team: ClusterRole for `Bind9GlobalCluster`
   - Dev teams: Role for `DNSZone` and records in their namespace

**Example - Record Isolation:**

```yaml
# team-a namespace
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www
  namespace: team-a
spec:
  zoneRef: team-a-zone  # ✅ References zone in same namespace
  name: www
  ipv4Address: "192.0.2.1"
---
# This would FAIL - cannot reference zone in another namespace
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www
  namespace: team-a
spec:
  zoneRef: team-b-zone  # ❌ References zone in team-b namespace - BLOCKED
  name: www
  ipv4Address: "192.0.2.1"
```

## Decision Tree: Choosing a Cluster Model

Use this decision tree to determine which cluster model fits your use case:

```mermaid
graph TD
    Start{Who manages<br/>DNS infrastructure?}
    Start -->|Platform Team| PlatformCheck{Shared across<br/>namespaces?}
    Start -->|Development Team| DevCheck{Isolated to<br/>namespace?}

    PlatformCheck -->|Yes| Global[Use Bind9GlobalCluster<br/>cluster-scoped]
    PlatformCheck -->|No| Cluster[Use Bind9Cluster<br/>namespace-scoped]

    DevCheck -->|Yes| Cluster
    DevCheck -->|No| Global

    Global --> GlobalDetails[✓ ClusterRole required<br/>✓ Accessible from all namespaces<br/>✓ Centralized management<br/>✓ Production workloads]

    Cluster --> ClusterDetails[✓ Role required namespace<br/>✓ Isolated to namespace<br/>✓ Team autonomy<br/>✓ Dev/test workloads]

    style Global fill:#c8e6c9
    style Cluster fill:#e1f5ff
    style GlobalDetails fill:#e8f5e9
    style ClusterDetails fill:#e3f2fd
```

## Next Steps

- [Multi-Tenancy Guide](multi-tenancy.md) - Detailed RBAC setup and examples
- [Choosing a Cluster Type](choosing-cluster-type.md) - Decision guide for cluster selection
- [Quickstart Guide](quickstart.md) - Getting started with both cluster types
