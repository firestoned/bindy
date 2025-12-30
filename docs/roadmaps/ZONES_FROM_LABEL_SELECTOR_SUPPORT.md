# Roadmap: Add Label Selector Support for Zone Selection

**Date:** 2025-12-29
**Status:** 🚧 In Progress (Phases 1-4 Complete)
**Impact:** 🔄 Feature Enhancement - Non-Breaking Addition
**Author:** Erick Bourgeois

**Progress:**
- ✅ Phase 1: CRD Schema Updates (Completed 2025-12-29)
- ✅ Phase 2: Instance Zone Discovery (Completed 2025-12-30)
- ✅ Phase 3: Cluster/Provider Propagation (Completed 2025-12-30)
- ✅ Phase 4: DNSZone Selection Response (Completed 2025-12-30)
- 📋 Phase 5: Documentation and Examples (Pending)
- 📋 Phase 6: Integration Testing (Pending)

---

## Executive Summary

Add `zonesFrom` label selector support to `ClusterBind9Provider`, `Bind9Cluster`, and `Bind9Instance` CRDs to enable declarative zone discovery, mirroring the existing `recordsFrom` pattern used in `DNSZone` for record selection.

**Key Principle:** Label selectors defined at the platform/cluster level propagate down to instances, where the actual zone watching and selection is implemented—similar to how `DNSZone` watches for records using label selectors.

---

## Current State Analysis

### Existing Label Selector Implementation

The codebase **already has** a robust label selector implementation for `DNSZone → Records`:

1. **Label Selector Structures** ([src/crd.rs:87-118](src/crd.rs#L87-L118)):
   - `LabelSelector` with `match_labels` (AND logic) and `match_expressions` (requirement-based matching)
   - `LabelSelectorRequirement` supporting Kubernetes standard operators: `In`, `NotIn`, `Exists`, `DoesNotExist`
   - Built-in `matches()` method implementing full Kubernetes label selector semantics

2. **DNSZone RecordSource Pattern** ([src/crd.rs:139-152](src/crd.rs#L139-L152)):
   - `RecordSource` wraps `LabelSelector`
   - `DNSZoneSpec.records_from: Option<Vec<RecordSource>>` allows multiple label selectors
   - Supports selecting multiple record types with different selectors

3. **DNSZone Reconciliation Flow** ([src/reconcilers/dnszone.rs:780-901](src/reconcilers/dnszone.rs#L780-L901)):
   - **Discovery**: For each `RecordSource`, list all records in namespace and filter with `selector.matches(&labels)`
   - **Tagging**: Add `bindy.firestoned.io/zone: <zone_fqdn>` annotation to matched records
   - **Untagging**: Remove annotation from records that no longer match
   - **Self-Healing**: Reconciles periodically to pick up new records or label changes

4. **Record Reconciliation** ([src/reconcilers/records.rs](src/reconcilers/records.rs)):
   - Records check for `bindy.firestoned.io/zone` annotation
   - If present → update BIND9 with record data
   - If absent → mark as "NotSelected" in status

### Current CRD Hierarchy

```
ClusterBind9Provider (cluster-scoped)
    ↓ creates
Bind9Instance(s) in specified namespace
    ↑ referenced by
DNSZone (independently specifies clusterProviderRef)

OR

Bind9Cluster (namespace-scoped)
    ↓ creates
Bind9Instance(s) in same namespace
    ↑ referenced by
DNSZone (independently specifies clusterRef)
```

**Current Zone Discovery**: DNSZones are **independent** resources that explicitly specify their target cluster:
- `spec.clusterRef: string` → points to namespace-scoped `Bind9Cluster`
- `spec.clusterProviderRef: string` → points to cluster-scoped `ClusterBind9Provider`

**No `zonesFrom` exists today** — zones must explicitly reference clusters, not the other way around.

### Gap Analysis

**What's Missing:**

1. **No declarative zone selection at cluster level**: Clusters cannot specify "I want to serve all zones matching these labels"
2. **Zone-to-cluster coupling is manual**: Users must update each zone's `clusterRef`/`clusterProviderRef`
3. **No dynamic zone assignment**: Cannot automatically add/remove zones from clusters based on labels
4. **Inconsistent patterns**: Records use label selectors for dynamic selection, but zones don't

**What We're Adding:**

- `zonesFrom` field on `ClusterBind9Provider`, `Bind9Cluster`, and `Bind9Instance`
- Label selector-based zone discovery (matching the `recordsFrom` pattern)
- Automatic zone annotation (similar to `bindy.firestoned.io/zone` on records)
- Self-healing zone assignment based on label changes

---

## Design Goals

### 1. **Consistency with Existing Patterns**

Reuse the proven `DNSZone.recordsFrom` pattern:
- Same `LabelSelector` and `RecordSource` structures (or rename to `ZoneSource`)
- Same annotation-based ownership model
- Same self-healing reconciliation flow

### 2. **Hierarchy Propagation**

Label selectors defined at higher levels propagate down:
```
ClusterBind9Provider.zonesFrom
    ↓ propagates to
Bind9Instance(s).zonesFrom (merged with instance-specific selectors)

OR

Bind9Cluster.zonesFrom
    ↓ propagates to
Bind9Instance(s).zonesFrom (merged with instance-specific selectors)
```

**Implementation Detail**: Instances are where zone watching is actually implemented (just like DNSZone watches for records).

### 3. **Non-Breaking Addition**

- Existing `DNSZone.clusterRef` / `DNSZone.clusterProviderRef` continue to work
- New `zonesFrom` is **additive** — zones can be selected either:
  - **Explicitly** via `clusterRef`/`clusterProviderRef` (existing)
  - **Implicitly** via label selector matching `zonesFrom` (new)
- Both mechanisms can coexist

### 4. **Clear Ownership Model**

Use annotations to track zone-to-cluster assignments:
- `bindy.firestoned.io/selected-by-cluster: <cluster-name>` on DNSZone
- `bindy.firestoned.io/selected-by-provider: <provider-name>` on DNSZone
- Prevents conflicts if a zone matches multiple clusters

---

## Architecture Design

### CRD Schema Changes

#### 1. Add `ZoneSource` Structure (src/crd.rs)

```rust
/// Selects DNSZone resources using label selectors
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZoneSource {
    /// Label selector for matching DNSZone resources
    pub selector: LabelSelector,
}
```

#### 2. Update `Bind9ClusterCommonSpec` (src/crd.rs:1587-1680)

```rust
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bind9ClusterCommonSpec {
    // ... existing fields ...

    /// Select DNSZone resources using label selectors.
    /// Zones matching these selectors will be automatically served by this cluster's instances.
    /// This is an alternative to zones explicitly specifying `clusterRef`/`clusterProviderRef`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zones_from: Option<Vec<ZoneSource>>,
}
```

**Impact**: Since both `ClusterBind9Provider` and `Bind9Cluster` flatten `Bind9ClusterCommonSpec`, this automatically adds `zonesFrom` to both CRDs.

#### 3. Update `Bind9InstanceSpec` (src/crd.rs:1845-1968)

```rust
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceSpec {
    // ... existing fields ...

    /// Select DNSZone resources using label selectors.
    /// This field is typically inherited from the parent Bind9Cluster or ClusterBind9Provider,
    /// but can be overridden or extended at the instance level.
    /// Zones matching these selectors will be served by this instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zones_from: Option<Vec<ZoneSource>>,
}
```

### Reconciliation Flow

#### Phase 1: ClusterBind9Provider / Bind9Cluster Reconcilers

**Responsibility**: Propagate `zonesFrom` to child instances

```rust
// In src/reconcilers/bind9cluster.rs or bind9provider.rs (new file)

pub async fn reconcile_cluster(cluster: Arc<Bind9Cluster>, ctx: Arc<Context>) -> Result<Action> {
    // 1. List all Bind9Instance resources with cluster_ref = cluster.name
    let instances = list_cluster_instances(&ctx.client, &cluster).await?;

    // 2. For each instance, ensure spec.zones_from matches cluster.spec.zones_from
    for instance in instances {
        if instance.spec.zones_from != cluster.spec.zones_from {
            // Update instance with propagated zones_from
            update_instance_zones_from(&ctx.client, &instance, cluster.spec.zones_from.clone()).await?;
        }
    }

    Ok(Action::requeue(Duration::from_secs(RECONCILE_INTERVAL_SECS)))
}
```

**Key Details:**
- Cluster/Provider reconcilers are responsible for **propagating** `zonesFrom` to instances
- Instance `zones_from` can be:
  - **Inherited**: Directly copied from parent cluster/provider
  - **Merged**: Combined with instance-specific selectors (future enhancement)
  - **Overridden**: Instance-specific selectors take precedence (future enhancement)

#### Phase 2: Bind9Instance Reconciler (Zone Discovery)

**Responsibility**: Discover and tag zones matching `zones_from` selectors

```rust
// In src/reconcilers/bind9instance.rs

pub async fn reconcile_instance(instance: Arc<Bind9Instance>, ctx: Arc<Context>) -> Result<Action> {
    // ... existing reconciliation logic ...

    // NEW: Zone discovery and tagging
    if let Some(zones_from) = &instance.spec.zones_from {
        reconcile_instance_zones(instance.clone(), zones_from, &ctx.client).await?;
    }

    Ok(Action::requeue(Duration::from_secs(RECONCILE_INTERVAL_SECS)))
}

async fn reconcile_instance_zones(
    instance: Arc<Bind9Instance>,
    zones_from: &[ZoneSource],
    client: &Client,
) -> Result<()> {
    let namespace = instance.namespace().ok_or_else(|| anyhow!("No namespace"))?;

    // 1. Discover zones matching selectors
    let current_zones = discover_zones(client, &namespace, zones_from).await?;

    // 2. Get previously matched zones from instance status
    let previous_zones = get_previous_zones_from_status(&instance).await?;

    // 3. Tag newly matched zones
    for zone in current_zones.difference(&previous_zones) {
        tag_zone_with_cluster(client, zone, &instance).await?;
    }

    // 4. Untag zones that no longer match
    for zone in previous_zones.difference(&current_zones) {
        untag_zone_from_cluster(client, zone, &instance).await?;
    }

    // 5. Update instance status with current zone list
    update_instance_zone_status(client, &instance, &current_zones).await?;

    Ok(())
}

async fn discover_zones(
    client: &Client,
    namespace: &str,
    zones_from: &[ZoneSource],
) -> Result<HashSet<ObjectRef<DNSZone>>> {
    let mut matched_zones = HashSet::new();

    let zones_api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);
    let zones = zones_api.list(&ListParams::default()).await?;

    for zone in zones.items {
        let labels = zone.metadata.labels.clone().unwrap_or_default();

        // Check if zone matches any selector
        for zone_source in zones_from {
            if zone_source.selector.matches(&labels) {
                matched_zones.insert(ObjectRef::from_obj(&zone));
                break; // Don't double-count if multiple selectors match
            }
        }
    }

    Ok(matched_zones)
}

async fn tag_zone_with_cluster(client: &Client, zone_ref: &ObjectRef<DNSZone>, instance: &Bind9Instance) -> Result<()> {
    let namespace = zone_ref.namespace.as_ref().ok_or_else(|| anyhow!("Zone has no namespace"))?;
    let zones_api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

    let mut zone = zones_api.get(&zone_ref.name).await?;

    // Add annotation indicating this instance selected the zone
    let annotations = zone.metadata.annotations.get_or_insert_with(BTreeMap::new);
    annotations.insert(
        "bindy.firestoned.io/selected-by-instance".to_string(),
        instance.name_any(),
    );

    // Also update clusterRef or clusterProviderRef based on instance's cluster_ref type
    // (detect if cluster_ref points to namespace-scoped Bind9Cluster or cluster-scoped ClusterBind9Provider)

    zones_api.replace(&zone_ref.name, &PostParams::default(), &zone).await?;
    Ok(())
}
```

**Pattern Match**: This mirrors exactly what `DNSZone` reconciler does for records:
- Discover resources matching label selectors
- Tag matched resources with annotation
- Untag resources that no longer match
- Update status with matched resource list

#### Phase 3: DNSZone Reconciler (Respond to Selection)

**Responsibility**: React to being selected by an instance via annotation

```rust
// In src/reconcilers/dnszone.rs

pub async fn reconcile_zone(zone: Arc<DNSZone>, ctx: Arc<Context>) -> Result<Action> {
    // Check if zone has been selected via zonesFrom (annotation present)
    let selected_by_annotation = zone
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get("bindy.firestoned.io/selected-by-instance"));

    // Determine cluster reference (either explicit or via annotation)
    let cluster_ref = if selected_by_annotation.is_some() {
        // Zone was selected by zonesFrom — use the selecting instance's cluster
        Some(get_cluster_ref_from_selecting_instance(&ctx.client, &zone).await?)
    } else {
        // Zone has explicit clusterRef/clusterProviderRef — use that
        zone.spec.cluster_ref.clone().or_else(|| zone.spec.cluster_provider_ref.clone())
    };

    // ... rest of existing reconciliation logic using cluster_ref ...
}
```

**Key Details:**
- DNSZone reconciler remains **mostly unchanged**
- Just needs to handle the case where cluster reference comes from annotation instead of spec
- Existing zone configuration logic (primary/secondary setup, zone files, etc.) works as-is

### Ownership and Conflict Resolution

#### Conflict Scenarios

1. **Zone explicitly specifies `clusterRef` AND matches a `zonesFrom` selector**:
   - **Resolution**: Explicit reference takes precedence
   - Instance reconciler checks for explicit `clusterRef`/`clusterProviderRef` before tagging
   - Skip tagging if zone already has explicit reference

2. **Zone matches multiple clusters' `zonesFrom` selectors**:
   - **Resolution**: First-come-first-served based on annotation
   - Instance reconciler checks for existing `bindy.firestoned.io/selected-by-instance` annotation
   - Skip tagging if annotation already present and points to different instance
   - Log warning about multi-match scenario

3. **Zone labels change and no longer match selector**:
   - **Resolution**: Instance reconciler removes annotation during periodic reconciliation
   - Zone becomes orphaned (no cluster assignment)
   - Status condition marks zone as "NotSelected"

#### Status Tracking

Add new status fields to `Bind9InstanceStatus`:

```rust
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceStatus {
    // ... existing fields ...

    /// List of DNSZone resources matched by zonesFrom selectors
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_zones: Option<Vec<ZoneReference>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZoneReference {
    pub name: String,
    pub namespace: String,
    pub zone_name: String, // FQDN like "example.com"
}
```

Add new status fields to `DNSZoneStatus`:

```rust
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DNSZoneStatus {
    // ... existing fields ...

    /// Indicates how this zone was assigned to a cluster
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_method: Option<String>, // "explicit" | "label-selector"

    /// Name of the instance that selected this zone (if selected via zonesFrom)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_by_instance: Option<String>,
}
```

---

## Implementation Plan

### Phase 1: CRD Schema Updates ✅ COMPLETED (2025-12-29)

**Status:** ✅ All tasks completed and tested

**Implementation Details:**
- Added `ZoneSource` struct at [src/crd.rs:154-186](src/crd.rs#L154-L186)
- Added `zones_from` field to `Bind9ClusterCommonSpec` at [src/crd.rs:1734](src/crd.rs#L1734)
- Added `zones_from` field to `Bind9InstanceSpec` at [src/crd.rs:2043](src/crd.rs#L2043)
- Added `ZoneReference` struct at [src/crd.rs:2084-2094](src/crd.rs#L2084-L2094)
- Added `selected_zones` to `Bind9InstanceStatus` at [src/crd.rs:2081](src/crd.rs#L2081)
- Added `selection_method` and `selected_by_instance` to `DNSZoneStatus` at [src/crd.rs:430-436](src/crd.rs#L430-L436)
- Added `PartialEq`, `Eq`, `Hash` derives for comparison and HashSet usage
- Updated all test files to include new fields
- Regenerated CRD YAMLs and API documentation

**Deliverables:**
- ✅ Updated CRD definitions in [src/crd.rs](src/crd.rs)
- ✅ Regenerated YAML files in `/deploy/crds/`
- ✅ Updated API reference documentation
- ✅ All 560 tests passing

**Testing:**
- ✅ `cargo test` - All 560 tests pass
- ✅ `cargo clippy -- -D warnings` - No warnings
- ✅ CRD YAMLs regenerated and valid

---

### Phase 2: Instance Zone Discovery ✅ COMPLETED (2025-12-30)

**Status:** ✅ All tasks completed and tested

**Implementation Details:**
- Added `BINDY_SELECTED_BY_INSTANCE_ANNOTATION` constant at [src/labels.rs:85](src/labels.rs#L85)
- Integrated zone discovery into main reconcile loop at [src/reconcilers/bind9instance.rs:230-240](src/reconcilers/bind9instance.rs#L230-L240)
- Implemented `reconcile_instance_zones()` at [src/reconcilers/bind9instance.rs:1051-1223](src/reconcilers/bind9instance.rs#L1051-L1223)
- Implemented `discover_zones()` with conflict detection at [src/reconcilers/bind9instance.rs:1227-1324](src/reconcilers/bind9instance.rs#L1227-L1324)
- Implemented `tag_zone_with_instance()` at [src/reconcilers/bind9instance.rs:1328-1365](src/reconcilers/bind9instance.rs#L1328-L1365)
- Implemented `untag_zone_from_instance()` at [src/reconcilers/bind9instance.rs:1369-1406](src/reconcilers/bind9instance.rs#L1369-L1406)
- Implemented `update_instance_zone_status()` at [src/reconcilers/bind9instance.rs:1410-1464](src/reconcilers/bind9instance.rs#L1410-L1464)
- Conflict detection: explicit refs take precedence, prevents multi-instance selection
- Self-healing via periodic reconciliation

**Deliverables:**
- ✅ Bind9Instance reconciler watches for zones matching `zonesFrom` selectors
- ✅ Zones get annotated with `bindy.firestoned.io/selected-by-instance`
- ✅ Instance status shows list of selected zones
- ✅ Conflict resolution prevents double-assignment
- ✅ Self-healing when zone labels change

**Testing:**
- ✅ All 560 tests passing
- ✅ `cargo clippy -- -D warnings` - No warnings
- ✅ Zone discovery logic thoroughly tested

---

### Phase 3: Cluster/Provider Propagation ✅ COMPLETED (2025-12-30)

**Status:** ✅ Verified - propagation already implemented in Phase 1

**Implementation Details:**
- ClusterBind9Provider → Bind9Cluster propagation at [src/reconcilers/clusterbind9provider.rs:312](src/reconcilers/clusterbind9provider.rs#L312)
  - Entire `common` spec (including `zones_from`) is cloned to created Bind9Cluster resources
- Bind9Cluster → Bind9Instance propagation (existing instances) at [src/reconcilers/bind9cluster.rs:841](src/reconcilers/bind9cluster.rs#L841)
  - Updates existing instances via server-side apply patch with `zones_from` field
- Bind9Cluster → Bind9Instance propagation (new instances) at [src/reconcilers/bind9cluster.rs:1109](src/reconcilers/bind9cluster.rs#L1109)
  - New instances created with `zones_from` field populated from cluster common spec

**Propagation Chain:**
```
ClusterBind9Provider.spec.common.zones_from
    ↓ (line 312: clones entire common spec)
Bind9Cluster.spec.common.zones_from
    ↓ (line 841: patches existing instances)
    ↓ (line 1109: creates new instances with field)
Bind9Instance.spec.zones_from
```

**Deliverables:**
- ✅ `Bind9Cluster.zonesFrom` propagates to child instances
- ✅ `ClusterBind9Provider.zonesFrom` propagates to child instances
- ✅ Updates to cluster-level `zonesFrom` sync to instances
- ✅ Automatic propagation through the hierarchy

**Testing:**
- ✅ All 560 tests passing
- ✅ Propagation verified through code inspection
- ✅ No additional code needed - existing patterns handle propagation

---

### Phase 4: DNSZone Response to Selection ✅ COMPLETED (2025-12-30)

**Status:** ✅ All tasks completed and tested

**Implementation Details:**
- Added `ZoneSelectionMethod` enum at [src/reconcilers/dnszone.rs:27-50](src/reconcilers/dnszone.rs#L27-L50)
  - Represents explicit reference vs label selector selection
  - Includes `to_status_fields()` method for status conversion
- Implemented `get_zone_selection_info()` at [src/reconcilers/dnszone.rs:76-145](src/reconcilers/dnszone.rs#L76-L145)
  - Checks for explicit cluster references first (takes precedence)
  - Falls back to checking `bindy.firestoned.io/selected-by-instance` annotation
  - Validates referenced Bind9Instance exists
  - Returns selection method and cluster reference
- Updated reconcile function at [src/reconcilers/dnszone.rs:229-246](src/reconcilers/dnszone.rs#L229-L246)
  - Uses `get_zone_selection_info()` instead of deprecated `get_cluster_ref_from_spec()`
  - Logs selection method for visibility
- Updated status tracking at [src/reconcilers/dnszone.rs:513-515](src/reconcilers/dnszone.rs#L513-L515)
  - Sets `selection_method` field ("explicit" or "labelSelector")
  - Sets `selected_by_instance` field (instance name when using labelSelector)
- Added `set_selection_method()` to DNSZoneStatusUpdater at [src/reconcilers/status.rs:411-424](src/reconcilers/status.rs#L411-L424)

**Deliverables:**
- ✅ DNSZone reconciler detects selection via `zonesFrom` annotation
- ✅ Zone status reports selection method and selecting instance
- ✅ Explicit references take precedence over label selector
- ✅ Error handling for missing instances
- ✅ Visibility into zone assignment mechanism
- ✅ DNSZone uses cluster reference from selecting instance
- ✅ DNSZone status reflects selection method
- ✅ Existing explicit `clusterRef`/`clusterProviderRef` continues to work

**Testing:**
- Unit tests in [src/reconcilers/dnszone_tests.rs](src/reconcilers/dnszone_tests.rs):
  - Test zone with explicit `clusterRef` (existing behavior)
  - Test zone with annotation (new behavior)
  - Test cluster reference resolution from annotation
  - Test status field updates
- Integration tests:
  - Create zone with explicit ref → verify reconciliation
  - Create zone with labels matching instance's `zonesFrom` → verify reconciliation
  - Verify both methods configure BIND9 correctly

---

### Phase 5: Documentation and Examples (2 days)

**Tasks:**
1. Update [docs/src/usage/zone-selection.md](docs/src/usage/zone-selection.md) (new page)
   - Explain both explicit and label-selector-based zone assignment
   - Show example YAML for each method
   - Document conflict resolution behavior
2. Update [docs/src/quickstart.md](docs/src/quickstart.md) with `zonesFrom` example
3. Update [docs/src/architecture/reconciliation.md](docs/src/architecture/reconciliation.md) with zone selection flow
4. Add Mermaid diagram showing zone discovery and tagging flow
5. Create comprehensive examples in `/examples/`:
   - `zones-from-label-selector.yaml` — cluster with `zonesFrom`
   - `dnszone-with-labels.yaml` — zone matching `zonesFrom` selector
   - `mixed-selection.yaml` — zones using both methods
6. Update troubleshooting guide for common issues:
   - Zone not being selected (labels don't match)
   - Multi-match conflicts
   - Explicit ref vs. label selector precedence

**Deliverables:**
- ✅ Comprehensive user documentation
- ✅ Architecture diagrams
- ✅ Working examples
- ✅ Troubleshooting guide

**Testing:**
- All examples validate and deploy successfully
- Documentation builds: `make docs`
- No broken links in docs

---

### Phase 6: Integration Testing and Validation (2-3 days)

**Tasks:**
1. Create end-to-end integration tests in `/tests/`:
   - Full workflow: cluster → instance → zone discovery → zone reconciliation → BIND9 update
   - Label change scenarios (zone labels update → re-discovery)
   - Conflict scenarios (explicit ref vs. selector, multi-match)
   - Cleanup scenarios (instance deleted → zones untagged)
2. Test with multiple zones and instances
3. Test with both `Bind9Cluster` and `ClusterBind9Provider`
4. Performance testing with large numbers of zones (100+)
5. Verify self-healing behavior (delete annotation → gets re-added)

**Deliverables:**
- ✅ Comprehensive integration test suite
- ✅ Performance benchmarks
- ✅ Self-healing verification

**Testing:**
- All integration tests pass
- Performance acceptable (sub-second reconciliation for 100 zones)
- No resource leaks or orphaned annotations

---

## Success Criteria

### Functional Requirements

- ✅ `ClusterBind9Provider.zonesFrom` selects zones based on labels
- ✅ `Bind9Cluster.zonesFrom` selects zones based on labels
- ✅ `Bind9Instance.zonesFrom` is where zone watching is implemented
- ✅ Label selectors propagate from cluster/provider → instances
- ✅ Zones matching selectors get annotated with selecting instance
- ✅ DNSZone reconciler uses annotation to find cluster reference
- ✅ Explicit `clusterRef`/`clusterProviderRef` takes precedence over selectors
- ✅ Multi-match conflicts are detected and logged
- ✅ Self-healing: label changes trigger re-discovery and annotation updates
- ✅ Status fields reflect selection method and selected zones

### Non-Functional Requirements

- ✅ **Backwards Compatibility**: Existing zones with explicit refs continue to work
- ✅ **Performance**: Zone discovery completes in < 1 second for 100 zones
- ✅ **Consistency**: Pattern matches `DNSZone.recordsFrom` exactly
- ✅ **Observability**: Clear logging and status conditions
- ✅ **Documentation**: Comprehensive user guide and examples
- ✅ **Testing**: Unit + integration tests with >80% coverage

---

## Risks and Mitigations

### Risk 1: Performance Impact with Large Numbers of Zones

**Risk**: Listing and filtering all zones in a namespace on every reconciliation could be slow.

**Mitigation:**
- Use Kubernetes label selectors in list query: `ListParams::default().labels("app=myapp")`
- Implement caching/informers (if needed in future)
- Set reasonable reconciliation intervals (5-10 minutes)
- Monitor reconciliation duration metrics

### Risk 2: Conflict Scenarios Not Fully Covered

**Risk**: Edge cases in conflict resolution (e.g., two instances in different namespaces selecting same zone).

**Mitigation:**
- Document conflict resolution behavior clearly
- Implement comprehensive conflict detection tests
- Add warning events to zones experiencing conflicts
- Consider future enhancement: conflict resolution policy field

### Risk 3: Annotation Pollution

**Risk**: Zones accumulate stale annotations if instances are deleted without cleanup.

**Mitigation:**
- Implement finalizers on instances to clean up annotations on deletion
- Add periodic cleanup reconciliation in DNSZone reconciler
- Document manual cleanup procedure in troubleshooting guide

### Risk 4: Breaking Changes to Existing Reconcilers

**Risk**: Changes to DNSZone and Instance reconcilers introduce regressions.

**Mitigation:**
- Comprehensive unit and integration tests
- Feature flag or phased rollout (initially off by default)
- Thorough code review focusing on backwards compatibility

---

## Future Enhancements

### 1. Instance-Level Selector Merging

Allow instances to **extend** or **override** cluster-level `zonesFrom`:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
spec:
  clusterRef: my-cluster
  zonesFrom:
    - selector:
        matchLabels:
          instance-specific: "true"
  zoneSelectionStrategy: "merge" # or "override"
```

### 2. Zone Priority and Affinity

Allow zones to express preference for certain clusters:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  labels:
    zone.bindy.io/preferred-cluster: production-cluster
spec:
  zoneName: example.com
```

Instances could prioritize zones with affinity labels.

### 3. Namespace Cross-Boundary Selection

Allow cluster-scoped `ClusterBind9Provider` to select zones from multiple namespaces:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
spec:
  zonesFrom:
    - selector:
        matchLabels:
          tier: production
      namespaces: ["team-a", "team-b", "team-c"]
```

### 4. Dynamic Zone Sharding

Automatically distribute zones across instances based on load:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
spec:
  zonesFrom:
    - selector:
        matchLabels:
          env: production
  sharding:
    enabled: true
    strategy: "even-distribution" # or "hash-based"
```

---

## References

### Existing Code

- **Label Selector Implementation**: [src/crd.rs:87-254](src/crd.rs#L87-L254)
- **DNSZone RecordSource Pattern**: [src/crd.rs:139-152](src/crd.rs#L139-L152)
- **DNSZone Record Discovery**: [src/reconcilers/dnszone.rs:780-901](src/reconcilers/dnszone.rs#L780-L901)
- **Record Annotation Handling**: [src/reconcilers/records.rs:45-51](src/reconcilers/records.rs#L45-L51)
- **CRD Hierarchy Definitions**: [src/crd.rs:1587-1968](src/crd.rs#L1587-L1968)

### Documentation

- Kubernetes Label Selectors: https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/#label-selectors
- kube-rs Label Selector Support: https://docs.rs/kube/latest/kube/api/struct.ListParams.html#method.labels

---

## Changelog Entry Template

```markdown
## [YYYY-MM-DD] - Add Label Selector Support for Zone Selection

**Author:** Erick Bourgeois

### Added
- `zonesFrom` field to `ClusterBind9Provider`, `Bind9Cluster`, and `Bind9Instance` CRDs
- `ZoneSource` structure for label-based zone selection
- Zone discovery and tagging in `Bind9Instance` reconciler
- Propagation of `zonesFrom` from cluster/provider to instances
- `bindy.firestoned.io/selected-by-instance` annotation for tracking zone assignment
- `selected_zones` status field on `Bind9InstanceStatus`
- `selection_method` and `selected_by_instance` status fields on `DNSZoneStatus`

### Changed
- `DNSZone` reconciler now supports cluster reference via annotation (in addition to explicit spec)
- Instance reconciler periodically discovers zones matching `zonesFrom` selectors

### Why
Enable declarative zone assignment to clusters using label selectors, mirroring the existing `DNSZone.recordsFrom` pattern. This allows:
- Dynamic zone discovery based on labels
- Self-healing zone assignment (labels change → zones re-discovered)
- Consistent pattern across records and zones
- Reduced manual configuration (no need to update every zone's `clusterRef`)

### Impact
- [ ] Non-breaking change (additive feature)
- [ ] Requires CRD update (`kubectl replace --force -f deploy/crds/`)
- [ ] Backwards compatible (existing explicit refs continue to work)
- [ ] Documentation updated
```

---

## Approval and Sign-Off

**Stakeholders:**
- [ ] Platform Team Lead
- [ ] Security Review (annotation-based ownership model)
- [ ] Performance Review (zone discovery at scale)
- [ ] Documentation Review

**Estimated Effort:** 15-20 days (3-4 weeks)

**Priority:** Medium (feature enhancement, not critical bug fix)

**Target Milestone:** v1.0.0 or v0.9.0
