# Roadmap: Refactor Zone-Instance Association Architecture

**Date:** 2025-12-30
**Status:** 🚧 IN PROGRESS
**Impact:** 🔄 Major Architecture Refactor - Simplifies Ownership Model
**Author:** Erick Bourgeois

---

## Problem Statement

The current architecture has zones referencing **clusters** instead of **instances**, which creates confusion about ownership and responsibility:

### Current (Incorrect) Model:
```
DNSZone
  ├─ spec.clusterRef → Bind9Cluster
  ├─ spec.clusterProviderRef → ClusterBind9Provider
  └─ Problem: Zones don't know which instances actually serve them!

DNSZone reconciler
  └─ Queries cluster → finds instances → reconciles zones
  └─ Problem: Indirect, complex lookup logic
```

### Issues:
1. **Zones reference clusters** but are served by instances
2. **Clusters manage zones** when they should only manage instances
3. **Complex indirection**: Zone → Cluster → Instances
4. **Unclear ownership**: Who owns the zone-instance relationship?

---

## Correct Architecture

### Ownership Hierarchy:

```
ClusterBind9Provider (cluster-scoped, platform-only)
  ├─ .owns() → Bind9Cluster (in system namespace)
  ├─ Responsibility: Manage namespace-scoped clusters
  └─ Self-healing: Recreate clusters if deleted

Bind9Cluster (namespace-scoped, any team)
  ├─ .owns() → Bind9Instance (managed instances)
  ├─ Responsibility: Manage instances in namespace
  └─ Self-healing: Recreate instances if deleted

Bind9Instance (can be standalone or owned)
  ├─ .owns() → Deployment (BIND9 pod)
  ├─ .watches() → DNSZone (via zonesFrom selectors)
  ├─ Responsibility: Serve zones, manage BIND9 deployment
  ├─ Updates: dnszone.status.servedBy[] (instances serving zone)
  └─ Self-healing: Recreate deployment if deleted

DNSZone (namespace-scoped)
  ├─ spec.instances[] (explicit instance assignments) - OPTIONAL
  ├─ status.servedBy[] (instances serving via zonesFrom) - AUTO
  └─ NO cluster references!
```

### Key Principles:

1. **Direct Association**: Zones ↔ Instances (no cluster indirection)
2. **Clear Ownership**:
   - ClusterBind9Provider owns Bind9Cluster
   - Bind9Cluster owns Bind9Instance
   - Bind9Instance owns Deployment
   - NO resource owns DNSZone
3. **Responsibility**:
   - Clusters manage instances
   - Instances manage zones
   - Zones are passive (just configuration)

---

## Implementation Plan

### Phase 1: Update CRD Schemas ✅

**DNSZone CRD Changes:**
```yaml
spec:
  # REMOVE:
  # clusterRef: string
  # clusterProviderRef: string

  # ADD:
  instances:
    - name: primary-dns-0
      namespace: dns-system
    - name: secondary-dns-0
      namespace: dns-system

status:
  # REMOVE:
  # clusterRef: object

  # ADD:
  servedBy:
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: Bind9Instance
      name: primary-dns-0
      namespace: dns-system
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: Bind9Instance
      name: secondary-dns-0
      namespace: dns-system
```

**Files to Modify:**
- [ ] `src/crd.rs` - Update `DNSZoneSpec` and `DNSZoneStatus`
- [ ] Run `cargo run --bin crdgen` to regenerate CRD YAMLs
- [ ] Run `cargo run --bin crddoc` to regenerate API docs

### Phase 2: Update Bind9Instance Reconciler ✅

**Current Behavior:**
- Updates `instance.status.selectedZones` ✅ (keep this)
- Updates `dnszone.status.clusterRef` ❌ (replace with servedBy)

**New Behavior:**
- Updates `instance.status.selectedZones` (existing)
- Updates `dnszone.status.servedBy[]` array with instance reference

**Files to Modify:**
- [ ] `src/reconcilers/bind9instance.rs` - Update zone status patching logic

### Phase 3: Update DNSZone Reconciler ✅

**Current Behavior:**
- Reads `spec.clusterRef` or `spec.clusterProviderRef`
- Queries cluster to find instances
- Reconciles zones to instances

**New Behavior:**
- Priority 1: Read `spec.instances[]` (explicit assignment)
- Priority 2: Read `status.servedBy[]` (auto-assigned via zonesFrom)
- Directly reconcile to those instances (no cluster lookup)

**Files to Modify:**
- [ ] `src/reconcilers/dnszone.rs` - Replace `get_zone_selection_info()` and `get_cluster_ref_from_zone()`
- [ ] `src/reconcilers/dnszone.rs` - Update `add_dnszone()`, `add_dnszone_to_secondaries()`, `delete_dnszone()`

### Phase 4: Update Helper Functions ✅

**Functions to Update/Remove:**
- [ ] `find_all_secondary_pod_ips()` - Change to accept instance list instead of cluster ref
- [ ] `get_cluster_ref_from_zone()` - Replace with `get_instances_from_zone()`
- [ ] `get_zone_selection_info()` - Replace with `get_zone_instances()`

### Phase 5: Update Examples and Documentation ✅

**Files to Update:**
- [ ] `examples/dns-zone.yaml` - Remove clusterRef, add instances
- [ ] `examples/complete-setup.yaml` - Update zone definitions
- [ ] `docs/src/` - Update all documentation references
- [ ] `CHANGELOG.md` - Document breaking change

### Phase 6: Testing ✅

- [ ] Update unit tests for new CRD schema
- [ ] Test explicit instance assignment (`spec.instances[]`)
- [ ] Test auto-assignment via zonesFrom (`status.servedBy[]`)
- [ ] Test zone reconciliation with multiple instances
- [ ] Test backward compatibility (deprecated fields)

---

## Migration Strategy

### Backward Compatibility:

1. **Keep deprecated fields** in CRD for one release:
   - `spec.clusterRef` (deprecated)
   - `spec.clusterProviderRef` (deprecated)
   - Mark with `x-kubernetes-deprecated: true`

2. **Migration logic**:
   - If `spec.clusterRef` is set, automatically populate `spec.instances[]` from cluster instances
   - Warn users to migrate to new schema
   - Remove deprecated fields in next major version

### Migration Path:

```yaml
# Old (deprecated)
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
spec:
  clusterRef: production-dns

# New (recommended)
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
spec:
  instances:
    - name: primary-dns-0
      namespace: dns-system
    - name: secondary-dns-0
      namespace: dns-system
```

---

## Benefits

1. ✅ **Direct Association**: Zones know exactly which instances serve them
2. ✅ **Simpler Logic**: No cluster indirection or complex lookups
3. ✅ **Clear Ownership**: Each resource has one responsibility
4. ✅ **Flexible**: Zones can reference instances across namespaces if needed
5. ✅ **Intuitive**: Matches mental model (instances serve zones, not clusters)
6. ✅ **Multi-instance**: Zones naturally support multiple instances

---

## Breaking Changes

- ❌ `DNSZone.spec.clusterRef` → `DNSZone.spec.instances[]`
- ❌ `DNSZone.spec.clusterProviderRef` → removed
- ❌ `DNSZone.status.clusterRef` → `DNSZone.status.servedBy[]`

**Impact**: Major version bump required (v1alpha1 → v1beta1 or v1)

---

## Next Steps

1. ⏳ Phase 1: Update CRD schemas
2. ⏳ Phase 2: Update Bind9Instance reconciler
3. ⏳ Phase 3: Update DNSZone reconciler
4. ⏳ Phase 4: Update helper functions
5. ⏳ Phase 5: Update examples and documentation
6. ⏳ Phase 6: Testing and validation
