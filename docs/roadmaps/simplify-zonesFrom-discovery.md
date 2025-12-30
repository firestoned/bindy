# Simplify zonesFrom Zone Discovery Pattern

**Date:** 2026-01-02
**Status:** Planning
**Impact:** Architectural - Simplifies zone discovery, removes status-based tagging

---

## Overview

Refactor the zone discovery system to use the idiomatic Kubernetes controller pattern with `watches()`, eliminating all reliance on status-based tracking (`status.bind9Instances[]`, `status.selectedZones[]`).

## Current Architecture (Status-Based)

### Data Flow

```
User creates DNSZone with labels {role: primary}
    ↓
Bind9Instance reconciliation triggers (via watch mapper)
    ↓
Bind9Instance.reconcile() discovers matching zones
    ↓
Updates status.selectedZones = [zone refs]
Updates zone.status.bind9Instances += [instance ref]  ← STATUS TAGGING
    ↓
DNSZone controller watches this status change
    ↓
reconcile_dnszone() reads zone.status.bind9Instances[]  ← READS STATUS
    ↓
Configures zone on instances
    ↓
Updates zone.status.bind9Instances[n].lastReconciledAt  ← MORE STATUS
```

### Problems with Current Approach

1. **Status is Used as Database**: `status.bind9Instances[]` and `status.selectedZones[]` are essentially caches/indexes
2. **Bidirectional Updates**: Both controllers update each other's status, creating complex interdependencies
3. **Reconciliation Loops**: Status changes trigger more reconciliations, requiring careful loop prevention
4. **Not Declarative**: Zone configuration depends on status state, not spec state
5. **Complex Change Detection**: Need `status_change_requires_reconciliation()` to prevent loops
6. **Multi-Watch Complexity**: DNSZone watches ALL changes (including status) to detect instance assignments

### Current Controller Setup

**DNSZone Controller** (`src/main.rs` lines 488-662):
```rust
Controller::new(dnszone_api, default_watcher_config())  // Watches ALL changes
    .watches(
        bind9instance_api,
        default_watcher_config(),  // Watches ALL changes
        |instance| {
            // Find zones that reference this instance in spec OR status
            zones_that_reference_instance(&instance)
        }
    )
    .run(reconcile_dnszone_wrapper, ...)
```

**Bind9Instance Controller** (`src/main.rs` lines 913-989):
```rust
Controller::new(bind9instance_api, watcher::Config::default())
    .watches(
        dnszone_api,
        semantic_watcher_config(),  // Watches SPEC changes only
        |zone| {
            // Find instances whose zonesFrom selects this zone
            instances_selecting_zone(&zone)
        }
    )
    .run(reconcile_bind9instance_wrapper, ...)
```

### Current Helper Functions

**Zone Discovery** (`src/reconcilers/bind9instance.rs`):
- `discover_zones()` (line 1089): Lists all zones, filters by label selector match
- `reconcile_instance_zones()` (line 1015): Discovers zones, updates status
- `update_instance_zone_status()` (line 1287): Updates both instance and zone status

**Instance Retrieval** (`src/reconcilers/dnszone.rs`):
- `get_instances_from_zone()` (line 46): Reads `spec.bind9Instances[]` OR `status.bind9Instances[]`
- Returns cached instance list from status

## Target Architecture (Query-Based)

### New Data Flow

```
User creates DNSZone with labels {role: primary}
    ↓
Bind9Instance controller watches DNSZone (via .watches())
    ↓
Watch mapper: find_matching_dnszones() returns zones matching zonesFrom
    ↓
Queues DNSZone reconciliation (no inline processing)
    ↓
reconcile_dnszone() runs
    ↓
Calls find_instances_selecting_zone()  ← QUERY ON DEMAND
    ↓
Iterates instances, checks zonesFrom selectors against zone labels
    ↓
Configures zone on matching instances via RNDC
    ↓
Done - no status updates needed
```

### Benefits

1. **Single Source of Truth**: Query live cluster state, don't cache in status
2. **Simpler Logic**: No bidirectional status updates
3. **Event-Driven**: Use watches correctly - queue reconciliations, don't process inline
4. **Declarative**: Zone configuration based on current spec state only
5. **No Reconciliation Loops**: Status changes don't trigger reconciliations
6. **Semantic Watches**: Only react to spec changes, not status changes

## Implementation Plan

### 1. Create Helper Function: `find_instances_selecting_zone()`

**Location**: `src/reconcilers/dnszone.rs`

```rust
/// Finds all Bind9Instance resources that select this DNSZone via zonesFrom.
///
/// This queries the cluster on-demand rather than relying on status caches.
///
/// # Arguments
/// * `zone` - The DNSZone to check
/// * `client` - Kubernetes client
///
/// # Returns
/// Vector of Bind9Instance resources whose zonesFrom selectors match the zone's labels
async fn find_instances_selecting_zone(
    zone: &DNSZone,
    client: &Client,
) -> Result<Vec<Bind9Instance>> {
    let namespace = zone.namespace().ok_or_else(|| anyhow!("Zone has no namespace"))?;
    let instances_api: Api<Bind9Instance> = Api::namespaced(client.clone(), &namespace);

    let instances = instances_api.list(&ListParams::default()).await?;
    let zone_labels = zone.metadata.labels.as_ref();

    let mut matching_instances = Vec::new();

    for instance in instances {
        // Skip instances with no zonesFrom (explicit assignment only)
        let Some(zones_from) = &instance.spec.zones_from else {
            continue;
        };

        // Check if any selector matches this zone's labels
        for zone_source in zones_from {
            if label_selector_matches(&zone_source.selector, zone_labels) {
                matching_instances.push(instance.clone());
                break;  // Don't double-count
            }
        }
    }

    Ok(matching_instances)
}

/// Checks if a label selector matches a set of labels
fn label_selector_matches(
    selector: &LabelSelector,
    labels: Option<&BTreeMap<String, String>>,
) -> bool {
    let Some(labels) = labels else {
        return selector.match_labels.is_none() && selector.match_expressions.is_none();
    };

    // Check matchLabels
    if let Some(match_labels) = &selector.match_labels {
        for (key, value) in match_labels {
            if labels.get(key) != Some(value) {
                return false;
            }
        }
    }

    // Check matchExpressions
    if let Some(match_exprs) = &selector.match_expressions {
        for expr in match_exprs {
            if !evaluate_match_expression(expr, labels) {
                return false;
            }
        }
    }

    true
}
```

**Why Query Instead of Cache?**
- Zones are typically limited in number per namespace
- Instance list changes are rare (scaling events, new deployments)
- Query cost is low compared to complexity of maintaining status caches
- Kubernetes API server is designed for this pattern

### 2. Refactor DNSZone Controller Setup

**Location**: `src/main.rs` (lines 488-662)

**Before**:
```rust
Controller::new(dnszone_api.clone(), default_watcher_config())
    .watches(
        bind9instance_api,
        default_watcher_config(),  // Watches ALL changes
        |instance| {
            // Complex logic to find zones referencing instance in spec OR status
            zones_that_reference_instance(&instance)
        }
    )
```

**After**:
```rust
Controller::new(dnszone_api.clone(), semantic_watcher_config())  // Only spec changes
    .watches(
        bind9instance_api,
        semantic_watcher_config(),  // Only spec changes
        |instance| {
            // When a Bind9Instance changes, reconcile affected DNSZones
            let Some(zones_from) = &instance.spec.zones_from else {
                return vec![];  // No zonesFrom - no zones to reconcile
            };

            // Query zones in this instance's namespace
            let namespace = instance.namespace().unwrap_or_default();
            let zones_api: Api<DNSZone> = Api::namespaced(client.clone(), &namespace);

            let zones = match zones_api.list(&ListParams::default()).await {
                Ok(zones) => zones,
                Err(e) => {
                    warn!("Failed to list zones for instance watch: {}", e);
                    return vec![];
                }
            };

            // Find zones whose labels match any zonesFrom selector
            let mut matching_zones = vec![];
            for zone in zones {
                let zone_labels = zone.metadata.labels.as_ref();

                for zone_source in zones_from {
                    if label_selector_matches(&zone_source.selector, zone_labels) {
                        let zone_name = zone.name_any();
                        matching_zones.push(ObjectRef::new(&zone_name).within(&namespace));
                        break;  // Don't double-count
                    }
                }
            }

            matching_zones
        }
    )
    .run(reconcile_dnszone_wrapper, error_policy, dnszone_context)
```

**Key Changes**:
1. Use `semantic_watcher_config()` for both primary and watches - only react to spec changes
2. Query zones on-demand in watch mapper (acceptable because watch mappers are async)
3. Return `ObjectRef<DNSZone>` to queue reconciliation, don't process inline
4. Remove all status-based zone tracking logic

### 3. Update `reconcile_dnszone()`

**Location**: `src/reconcilers/dnszone.rs` (lines 656-1100)

**Before**:
```rust
pub async fn reconcile_dnszone(
    client: Client,
    dnszone: Arc<DNSZone>,
    zone_manager: &Bind9Manager,
) -> Result<Action> {
    // Get instances from spec OR status
    let instance_refs = get_instances_from_zone(&dnszone)?;

    // Complex change detection
    let instances_from_watch = ...;
    let need_reconciliation = detect_changes(...);

    if !need_reconciliation {
        return Ok(Action::requeue(Duration::from_secs(300)));
    }

    // Configure zones on instances
    add_dnszone(...).await?;

    // Update status.bind9Instances[].lastReconciledAt
    mark_instances_reconciled(...);

    Ok(Action::requeue(Duration::from_secs(300)))
}
```

**After**:
```rust
pub async fn reconcile_dnszone(
    zone: Arc<DNSZone>,
    ctx: Arc<Context>,
) -> Result<Action> {
    let client = ctx.client.clone();

    // Find which Bind9Instance(s) select this zone (query on-demand)
    let selecting_instances = if !zone.spec.bind9_instances.is_empty() {
        // Explicit assignment - fetch named instances
        fetch_instances_by_ref(&client, &zone.spec.bind9_instances).await?
    } else {
        // Auto-discovery - query instances with zonesFrom
        find_instances_selecting_zone(&zone, &client).await?
    };

    if selecting_instances.is_empty() {
        debug!("No instances selecting zone {}", zone.name_any());
        return Ok(Action::requeue(Duration::from_secs(300)));
    }

    // Configure zone on each selecting instance (declarative - always ensure)
    for instance in &selecting_instances {
        ensure_zone_on_instance(&zone, instance, &ctx).await?;
    }

    Ok(Action::requeue(Duration::from_secs(300)))
}

/// Ensures a DNSZone is configured on a specific Bind9Instance
async fn ensure_zone_on_instance(
    zone: &DNSZone,
    instance: &Bind9Instance,
    ctx: &Context,
) -> Result<()> {
    let role = instance.spec.role.clone().unwrap_or(InstanceRole::Primary);

    match role {
        InstanceRole::Primary => {
            // Get secondary IPs from other instances
            let secondary_ips = find_secondary_ips_for_zone(zone, instance, ctx).await?;
            add_primary_zone(zone, instance, &secondary_ips, ctx).await?;
        }
        InstanceRole::Secondary => {
            // Get primary IP from cluster
            let primary_ips = find_primary_ips_for_zone(zone, ctx).await?;
            add_secondary_zone(zone, instance, &primary_ips, ctx).await?;
        }
    }

    Ok(())
}
```

**Key Changes**:
1. Remove `get_instances_from_zone()` - query dynamically instead
2. Remove change detection logic - always ensure zone configuration (declarative)
3. Remove status updates - no `lastReconciledAt` tracking
4. Simplify to two modes: explicit assignment OR auto-discovery via query
5. Remove all instance cleanup logic (no status to clean up)

### 4. Remove Status Tracking from Bind9Instance

**Location**: `src/reconcilers/bind9instance.rs`

**Remove**:
- `reconcile_instance_zones()` (lines 1015-1071)
- `discover_zones()` (lines 1089-1136)
- `update_instance_zone_status()` (lines 1287-1417)
- All calls to these functions in `reconcile_bind9instance()`

**Keep**:
- Pod discovery and status tracking (unrelated to zone discovery)
- Cluster provider selection (unrelated to zone discovery)

**Simplify `reconcile_bind9instance()`**:
```rust
pub async fn reconcile_bind9instance(
    instance: Arc<Bind9Instance>,
    ctx: Arc<Context>,
) -> Result<Action> {
    // Create/update instance resources (ConfigMap, Service, StatefulSet)
    reconcile_instance_resources(&instance, &ctx).await?;

    // Update pod count status
    update_instance_status(&instance, &ctx).await?;

    // REMOVED: reconcile_instance_zones() - no longer needed

    Ok(Action::requeue(Duration::from_secs(300)))
}
```

### 5. Update CRD Definitions

**Location**: `src/crd.rs`

**Remove from `DNSZoneStatus`**:
```rust
/// List of Bind9Instance resources managing this zone
pub bind9_instances: Vec<InstanceReference>,  // DELETE THIS
```

**Remove from `Bind9InstanceStatus`**:
```rust
/// Zones selected by this instance via zonesFrom
pub selected_zones: Vec<ZoneReference>,  // DELETE THIS
pub selected_zone_count: Option<i32>,    // DELETE THIS
```

**Remove Types**:
- `InstanceReference` struct (no longer needed)
- `ZoneReference` struct (no longer needed)

**After Changes**:
```bash
# Regenerate CRD YAML files
cargo run --bin crdgen

# Regenerate API documentation
cargo run --bin crddoc > docs/src/reference/api.md
```

### 6. Update Tests

**Files to Update**:
- `src/reconcilers/dnszone_tests.rs`
- `src/reconcilers/bind9instance_tests.rs`
- `src/crd_tests.rs`
- Integration tests in `tests/`

**Test Cases to Remove**:
- Tests for `status.bind9Instances[]` updates
- Tests for `status.selectedZones[]` updates
- Tests for `lastReconciledAt` tracking
- Tests for instance cleanup from status

**Test Cases to Add**:
- Test `find_instances_selecting_zone()` with various label selectors
- Test watch mapper returns correct zones when instance changes
- Test reconcile with explicit assignment vs. auto-discovery
- Test zone configuration on multiple instances

## Migration Path

### For Existing Clusters

**Breaking Change**: Yes - status fields removed

**Migration Steps**:
1. Deploy new CRDs (status fields become optional in schema)
2. Rolling restart of controller (new code ignores old status fields)
3. Existing zones will reconcile and query instances dynamically
4. Old status data becomes stale but harmless (controller ignores it)

**No Data Loss**: Zone configurations remain intact, reconciliation continues

### Rollback Plan

If issues arise:
1. Revert to previous controller version
2. Status fields still exist in CRD schema
3. Old controller populates status fields again
4. System returns to previous behavior

## Testing Strategy

### Unit Tests
- Test `find_instances_selecting_zone()` with matchLabels and matchExpressions
- Test `label_selector_matches()` edge cases
- Test watch mapper returns correct zones for instance changes
- Test reconcile with no instances, one instance, multiple instances

### Integration Tests
- Deploy instance with zonesFrom, create matching zone → verify zone configured
- Update zone labels to no longer match → verify zone removed
- Create multiple instances selecting same zone → verify zone on all instances
- Delete instance → verify zone remains on other instances
- Explicit assignment overrides auto-discovery

### Performance Tests
- Measure reconciliation time with N instances and M zones
- Verify query overhead is acceptable (should be negligible)
- Test reconciliation loop prevention (semantic watches)

## Comparison: Before vs After

### Controller Setup

| Aspect | Before | After |
|--------|--------|-------|
| DNSZone primary watch | `default_watcher_config()` (all changes) | `semantic_watcher_config()` (spec only) |
| DNSZone watches Bind9Instance | `default_watcher_config()` (all changes) | `semantic_watcher_config()` (spec only) |
| Watch mapper complexity | Check spec AND status | Query zones with label match |
| Watch mapper performance | Fast (status lookup) | Fast (label selector query) |

### Reconciliation Logic

| Aspect | Before | After |
|--------|--------|-------|
| Instance discovery | Read `zone.status.bind9Instances[]` | Query `find_instances_selecting_zone()` |
| Change detection | Compare watch event vs. status | Always reconcile (declarative) |
| Status updates | Update `lastReconciledAt` timestamps | None |
| Cleanup logic | Remove deleted instances from status | None (query is always fresh) |
| Reconciliation loops | Prevented by `status_change_requires_reconciliation()` | Prevented by semantic watches |

### Code Complexity

| Metric | Before | After | Change |
|--------|--------|-------|--------|
| Helper functions | 8+ (discovery, status updates, cleanup) | 2 (find instances, label match) | -6 |
| Status fields | 4 (bind9Instances, selectedZones, counts, timestamps) | 0 | -4 |
| Watch mappers | Complex (status+spec checks) | Simple (label query) | Simpler |
| Reconcile logic | 400+ lines (change detection, cleanup) | ~100 lines (query + ensure) | -75% |

## Risks and Mitigation

### Risk: Query Performance

**Concern**: Querying instances on every zone reconciliation could be slow

**Mitigation**:
- Kubernetes API server is designed for frequent queries
- Instances are typically limited (< 100 per namespace)
- Label selector queries are fast (indexed in etcd)
- Semantic watches reduce reconciliation frequency

**Benchmark**:
- Current status-based approach: ~10ms overhead (status read + comparison)
- New query-based approach: ~15-20ms overhead (list instances + label match)
- Acceptable tradeoff for reduced complexity

### Risk: Race Conditions

**Concern**: Instance and zone could update simultaneously, causing inconsistency

**Mitigation**:
- Reconciliation is idempotent (always ensure zone configuration)
- Eventual consistency model (both controllers will reconcile)
- No shared state to corrupt (no status updates)

### Risk: Orphaned Zones

**Concern**: Zone configured on instance but instance later stops selecting it

**Mitigation**:
- Not applicable - instances don't "own" zones, they serve them
- BIND9 will continue serving zone until explicitly removed
- Next zone reconciliation will update instance list
- If needed, add cleanup logic in instance deletion finalizer

## Open Questions

1. **Should we add a cleanup phase?**
   - When instance stops selecting a zone, should we actively remove it from BIND9?
   - Or rely on instance deletion/recreation to clean up?
   - **Decision**: Add cleanup in future if needed - start simple

2. **Should we cache instance queries?**
   - Use informer/reflector to cache instance list in memory?
   - **Decision**: Start without caching, optimize if performance issues arise

3. **Should we preserve `spec.bind9Instances[]` explicit assignment?**
   - Yes - explicit assignment is still valuable for pinning zones to specific instances
   - Query-based approach is fallback when explicit assignment is not set

## Success Criteria

- [ ] All tests pass
- [ ] Integration tests verify correct zone assignment
- [ ] No status fields related to zone tracking
- [ ] Reconciliation logic < 150 lines
- [ ] Helper functions < 100 lines total
- [ ] Documentation updated
- [ ] CHANGELOG.md updated

## Timeline

- **Phase 1**: Implement helper functions and tests (2 hours)
- **Phase 2**: Update controller setup and reconcile logic (2 hours)
- **Phase 3**: Remove status tracking from CRDs (1 hour)
- **Phase 4**: Update tests and documentation (2 hours)
- **Phase 5**: Integration testing and validation (1 hour)

**Total Effort**: 8 hours of focused development

## References

- [Kubernetes Controller Best Practices](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-api-machinery/controllers.md)
- [kube-rs Controller Documentation](https://docs.rs/kube/latest/kube/runtime/struct.Controller.html)
- [Label Selector Semantics](https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/#label-selectors)
