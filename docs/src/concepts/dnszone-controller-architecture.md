# DNSZone Operator Architecture

This page documents the unified DNSZone operator architecture following the Phase 1-8 consolidation (January 2026).

## Overview

The DNSZone operator is responsible for:
1. Discovering Bind9Instances via `clusterRef` and/or `bind9InstancesFrom` label selectors
2. Synchronizing zone configuration to selected instances
3. Tracking per-instance synchronization status
4. Maintaining the Ready condition based on instance health

## Architecture Evolution

### Before: Dual Operator Architecture (Deprecated)

```mermaid
graph TB
    subgraph k8s["Kubernetes Cluster"]
        dnszone["DNSZone<br/>Resource"]
        instance["Bind9Instance<br/>Resource"]
        
        subgraph operators["Operators"]
            dnszoneCtrl["DNSZone Operator"]
            zonesyncCtrl["ZoneSync Operator"]
        end
        
        subgraph status["Status Fields"]
            s1["DNSZone.status.instances[]"]
            s2["DNSZone.status.syncStatus[]"]
            s3["Bind9Instance.status.selectedZones[]"]
        end
    end
    
    dnszone -->|watches| dnszoneCtrl
    instance -->|watches| zonesyncCtrl
    dnszoneCtrl -->|updates| s1
    zonesyncCtrl -->|updates| s2
    dnszoneCtrl -->|updates| s3
    s3 -->|triggers| zonesyncCtrl
    
    style operators fill:#ffcccc,stroke:#cc0000,stroke-width:2px
    style status fill:#ffffcc,stroke:#cccc00,stroke-width:2px
```

**Problems:**
- Two operators managing the same resource (DNSZone)
- Circular dependencies: DNSZone → Bind9Instance.status.selectedZones → ZoneSync → DNSZone
- Multiple status fields tracking the same information
- Complex event-driven architecture with multiple reconciliation paths
- ~915 lines of duplicate code

### After: Unified Operator Architecture (Current)

```mermaid
graph TB
    subgraph k8s["Kubernetes Cluster"]
        dnszone["DNSZone<br/>Resource"]
        instance["Bind9Instance<br/>Resource"]
        
        subgraph operators["Operators"]
            dnszoneCtrl["DNSZone Operator<br/>(Unified)"]
        end
        
        subgraph status["Status Fields"]
            s1["DNSZone.status.instances[]<br/>(Single Source of Truth)"]
        end
    end
    
    dnszone -->|watches| dnszoneCtrl
    instance -->|watches| dnszoneCtrl
    dnszoneCtrl -->|updates| s1
    dnszoneCtrl -->|calls bindcar API| instance
    
    style operators fill:#ccffcc,stroke:#00cc00,stroke-width:2px
    style status fill:#ccffff,stroke:#00cccc,stroke-width:2px
```

**Benefits:**
- Single operator with clear responsibility
- No circular dependencies
- One status field: `status.instances[]`
- Simplified reconciliation logic
- ~915 lines of code removed

## Instance Selection

DNSZones select Bind9Instances using three methods:

### Method 1: clusterRef (Simple Cluster Reference)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
spec:
  zoneName: example.com
  clusterRef: production-dns  # Selects ALL instances with this clusterRef
```

```mermaid
graph LR
    zone["DNSZone<br/>clusterRef: production-dns"]
    inst1["Bind9Instance 1<br/>clusterRef: production-dns<br/>✅ SELECTED"]
    inst2["Bind9Instance 2<br/>clusterRef: production-dns<br/>✅ SELECTED"]
    inst3["Bind9Instance 3<br/>clusterRef: staging-dns<br/>❌ NOT SELECTED"]
    
    zone -.->|matches| inst1
    zone -.->|matches| inst2
    zone -.->|no match| inst3
    
    style inst1 fill:#ccffcc,stroke:#00cc00,stroke-width:2px
    style inst2 fill:#ccffcc,stroke:#00cc00,stroke-width:2px
    style inst3 fill:#ffcccc,stroke:#cc0000,stroke-width:2px
```

### Method 2: bind9InstancesFrom (Label Selectors)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
spec:
  zoneName: example.com
  bind9InstancesFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/region: us-west-2
          bindy.firestoned.io/role: primary
```

```mermaid
graph LR
    zone["DNSZone<br/>selector:<br/>region=us-west-2<br/>role=primary"]
    inst1["Bind9Instance 1<br/>region=us-west-2<br/>role=primary<br/>✅ SELECTED"]
    inst2["Bind9Instance 2<br/>region=us-east-1<br/>role=primary<br/>❌ NOT SELECTED"]
    inst3["Bind9Instance 3<br/>region=us-west-2<br/>role=secondary<br/>❌ NOT SELECTED"]
    
    zone -.->|labels match| inst1
    zone -.->|region mismatch| inst2
    zone -.->|role mismatch| inst3
    
    style inst1 fill:#ccffcc,stroke:#00cc00,stroke-width:2px
    style inst2 fill:#ffcccc,stroke:#cc0000,stroke-width:2px
    style inst3 fill:#ffcccc,stroke:#cc0000,stroke-width:2px
```

### Method 3: Union (Both Together)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
spec:
  zoneName: example.com
  clusterRef: production-dns        # Instances from cluster
  bind9InstancesFrom:               # PLUS instances from selectors
    - selector:
        matchLabels:
          special-capability: geo-dns
```

```mermaid
graph TB
    zone["DNSZone<br/>clusterRef: production-dns<br/>+ selector: special-capability=geo-dns"]
    
    subgraph cluster["From clusterRef"]
        inst1["Instance 1<br/>clusterRef: production-dns"]
        inst2["Instance 2<br/>clusterRef: production-dns"]
    end
    
    subgraph labels["From bind9InstancesFrom"]
        inst3["Instance 3<br/>special-capability: geo-dns"]
        inst4["Instance 4<br/>special-capability: geo-dns"]
    end
    
    subgraph result["Result: UNION (Deduplicated)"]
        r1["Instance 1"]
        r2["Instance 2"]
        r3["Instance 3"]
        r4["Instance 4"]
    end
    
    inst1 --> r1
    inst2 --> r2
    inst3 --> r3
    inst4 --> r4
    
    style cluster fill:#ccffcc,stroke:#00cc00,stroke-width:2px
    style labels fill:#ccccff,stroke:#0000cc,stroke-width:2px
    style result fill:#ffffcc,stroke:#cccc00,stroke-width:2px
```

## Reconciliation Flow

```mermaid
sequenceDiagram
    participant K8s as Kubernetes API
    participant Ctrl as DNSZone Operator
    participant Store as Instance Store
    participant Bindcar as Bindcar HTTP API
    
    Note over K8s,Bindcar: 1. Watch Event Triggers Reconciliation
    K8s->>Ctrl: DNSZone changed
    
    Note over Ctrl,Store: 2. Get Target Instances
    Ctrl->>Store: Get instances matching clusterRef
    Store-->>Ctrl: Instances A, B
    Ctrl->>Store: Get instances matching selectors
    Store-->>Ctrl: Instances B, C
    Note over Ctrl: Compute UNION: [A, B, C]
    
    Note over Ctrl: 3. Get Current Status
    Note over Ctrl: Current: [A:Configured, D:Configured]
    
    Note over Ctrl: 4. Compute Diff
    Note over Ctrl: Added: [B, C]<br/>Removed: [D]<br/>Existing: [A]
    
    Note over Ctrl,Bindcar: 5. Process Changes
    
    Ctrl->>Ctrl: Mark B, C as Claimed
    Ctrl->>Ctrl: Mark D as Unclaimed
    
    Note over Ctrl: A is Configured - skip
    
    Ctrl->>Bindcar: add_zones(zone, instance B)
    alt Success
        Bindcar-->>Ctrl: 200 OK
        Ctrl->>Ctrl: B status = Configured
    else Failure
        Bindcar-->>Ctrl: 500 Error
        Ctrl->>Ctrl: B status = Failed
    end
    
    Ctrl->>Bindcar: add_zones(zone, instance C)
    Bindcar-->>Ctrl: 200 OK
    Ctrl->>Ctrl: C status = Configured
    
    Note over Ctrl: 6. Update Status
    Note over Ctrl: New status: [A:Configured, B:Configured, C:Configured]
    Ctrl->>K8s: Update DNSZone.status.instances[]
    Ctrl->>K8s: Set Ready=True (all configured)
```

## Status Lifecycle

Each instance in `status.instances[]` goes through this lifecycle:

```mermaid
stateDiagram-v2
    [*] --> Claimed: Instance added to target
    Claimed --> Configured: add_zones() succeeds
    Claimed --> Failed: add_zones() fails
    Failed --> Configured: Retry succeeds
    Configured --> Configured: No changes needed
    Configured --> Unclaimed: Instance removed from target
    Failed --> Unclaimed: Instance removed from target
    Unclaimed --> [*]: Status entry removed
```

### Status Enum Values

- **Claimed**: Instance discovered, zone sync pending
- **Configured**: Zone successfully synchronized to instance
- **Failed**: Zone synchronization failed (error in `message` field)
- **Unclaimed**: Instance no longer selected (will be removed)

## Status Schema

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
status:
  instances:
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: Bind9Instance
      name: primary-dns-0
      namespace: dns-system
      status: Configured         # Claimed | Configured | Failed | Unclaimed
      lastReconciledAt: "2026-01-06T10:00:00Z"
      message: null              # Error message for Failed status

    - apiVersion: bindy.firestoned.io/v1beta1
      kind: Bind9Instance
      name: secondary-dns-0
      namespace: dns-system
      status: Failed
      lastReconciledAt: "2026-01-06T10:00:05Z"
      message: "HTTP 500: bindcar API unavailable"
  
  conditions:
    - type: Ready
      status: "False"  # True only when ALL instances are Configured
      reason: PartialFailure
      message: "1/2 instances configured"
      lastTransitionTime: "2026-01-06T10:00:05Z"
```

## Breaking Changes from Previous Architecture

### Removed Fields

**DNSZone Status:**
- ❌ `status.syncStatus[]` - Replaced by `status.instances[]`
- ❌ `status.syncedInstancesCount` - Computed from `status.instances[]`
- ❌ `status.totalInstancesCount` - Computed from `status.instances[]`

**Bind9Instance Status:**
- ❌ `status.selectedZones[]` - No longer tracks reverse references
- ❌ `status.selectedZoneCount` - No longer needed

### Migration Path

1. **Before upgrade**: Export all DNSZone resources
   ```bash
   kubectl get dnszones -A -o yaml > dnszones-backup.yaml
   ```

2. **Upgrade**: Apply new CRDs
   ```bash
   kubectl replace --force -f deploy/crds/
   ```

3. **After upgrade**: DNSZone operator repopulates `status.instances[]` automatically

4. **Cleanup**: Old status fields are ignored and garbage collected

## See Also

- [DNSZone Operator Consolidation Roadmap](../../roadmaps/dnszone-consolidation-roadmap.md)
- [Integration Test Plan](../../roadmaps/integration-test-plan.md)
- [API Reference](../reference/api.md)
