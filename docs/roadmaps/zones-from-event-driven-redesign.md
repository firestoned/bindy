# Roadmap: Redesign zonesFrom to Use Event-Driven Watches

**Date:** 2025-12-30
**Status:** ✅ COMPLETE
**Impact:** 🔄 Architecture Refactor - Improves Controller Efficiency
**Author:** Erick Bourgeois

---

## Problem Statement

The current `zonesFrom` implementation (completed 2025-12-30) uses an **annotation-based** approach where:
1. Bind9Instance reconciler runs zone discovery on every reconciliation loop
2. Discovers zones matching label selectors by listing all DNSZones
3. Tags matching zones with `bindy.firestoned.io/selected-by-instance` annotation
4. DNSZone reconciler reads that annotation to determine which instance selected it

**Issues with this approach:**
- ❌ **Not event-driven**: Relies on periodic reconciliation to discover zones
- ❌ **Inefficient**: Lists all DNSZones on every Bind9Instance reconciliation
- ❌ **Annotation overhead**: Adds unnecessary annotations for control flow
- ❌ **Inconsistent pattern**: Records use label selectors without annotations

## Correct Kubernetes Controller Pattern

The proper approach mirrors how `DNSZone.recordsFrom` should work:

### Phase 1: Bind9Instance Watches DNSZones

**Bind9Instance controller** should use `.watches()` to observe DNSZone resources:

```rust
async fn run_bind9instance_controller(client: Client) -> Result<()> {
    let instance_api = Api::<Bind9Instance>::all(client.clone());
    let deployment_api = Api::<Deployment>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    Controller::new(instance_api, default_watcher_config())
        .owns(deployment_api, default_watcher_config())
        .watches(
            dnszone_api,
            default_watcher_config(),
            |dnszone| {
                // Map DNSZone changes to Bind9Instance reconciliation
                // Return the instances that might select this zone
                find_instances_for_zone(dnszone)
            }
        )
        .run(reconcile_bind9instance_wrapper, error_policy, Arc::new(client))
        .await;

    Ok(())
}
```

**How it works:**
1. When a DNSZone is created/updated/deleted, the watch mapper runs
2. The mapper returns a list of Bind9Instances that have `zonesFrom` selectors
3. Those Bind9Instances reconcile and update their `status.selectedZones` list
4. DNSZone reconciler queries Bind9Instance status to see if selected

### Phase 2: Remove Annotation-Based Selection

**Remove:**
- `bindy.firestoned.io/selected-by-instance` annotation
- `tag_zone_with_instance()` function
- `untag_zone_from_instance()` function
- Zone annotation checks in `get_zone_selection_info()`

**Keep:**
- `discover_zones()` function (renamed to `find_matching_zones()`)
- `update_instance_zone_status()` function
- `Bind9Instance.status.selectedZones` field

### Phase 3: DNSZone Queries Instance Status

In `get_zone_selection_info()`, instead of checking annotations:

```rust
pub async fn get_zone_selection_info(
    client: &Client,
    dnszone: &DNSZone,
) -> Result<(String, bool, ZoneSelectionMethod)> {
    // ... existing clusterRef/clusterProviderRef logic ...

    // If no explicit ref, check if any Bind9Instance has selected this zone
    let namespace = dnszone.namespace().ok_or_else(|| anyhow!("No namespace"))?;
    let instances: Api<Bind9Instance> = Api::namespaced(client.clone(), &namespace);

    for instance in instances.list(&ListParams::default()).await?.items {
        if let Some(status) = &instance.status {
            for zone_ref in &status.selected_zones {
                if zone_ref.name == dnszone.name_any() {
                    return Ok((
                        instance.name_any(),
                        false,
                        ZoneSelectionMethod::LabelSelector(instance.name_any()),
                    ));
                }
            }
        }
    }

    Err(anyhow!("DNSZone has no explicit ref and is not selected by any instance"))
}
```

---

## Implementation Plan

### Step 1: Add DNSZone Watch to Bind9Instance Controller

**File:** `src/main.rs`

1. Update `run_bind9instance_controller()` to add `.watches()` for DNSZone
2. Create watch mapper function that returns instances matching the zone's labels
3. Challenge: Mapper must be **synchronous** but needs to know instance `zonesFrom` selectors

**Solution:** Use in-memory cache/reflector:
```rust
// Build a reflector for Bind9Instances to enable sync lookup
let instance_store = reflector::store::Writer::default();
let instance_reflector = reflector(instance_store.as_reader(), instance_api.clone());

// Watch mapper can now synchronously query the cache
let mapper = |dnszone: &DNSZone| {
    let instances = instance_store.state();
    instances.into_iter()
        .filter(|inst| {
            inst.spec.zones_from.as_ref()
                .map(|selectors| selectors.iter().any(|s| s.selector.matches(&dnszone.labels)))
                .unwrap_or(false)
        })
        .map(|inst| ObjectRef::from_obj(&*inst))
        .collect::<Vec<_>>()
};
```

### Step 2: Remove Annotation-Based Code

**File:** `src/reconcilers/bind9instance.rs`

1. Remove `tag_zone_with_instance()` function
2. Remove `untag_zone_from_instance()` function
3. Remove annotation tagging/untagging from `reconcile_instance_zones()`
4. Keep `discover_zones()` but remove annotation checks
5. Keep `update_instance_zone_status()` to update `status.selectedZones`

### Step 3: Update DNSZone Selection Logic

**File:** `src/reconcilers/dnszone.rs`

1. Remove annotation-based selection from `get_zone_selection_info()`
2. Add logic to query Bind9Instance status to check if zone is selected
3. Ensure DNSZone reconciler uses the status-based selection method

### Step 4: Update Tests and Documentation

1. Update unit tests to remove annotation expectations
2. Update integration tests to verify event-driven behavior
3. Update documentation to reflect the new architecture
4. Update examples to show label-based selection without annotations

---

## Benefits

✅ **Event-driven**: Instances reconcile immediately when zones change
✅ **Efficient**: No periodic polling or full zone list scans
✅ **Cleaner**: No annotations used for control flow
✅ **Consistent**: Matches the intended `recordsFrom` pattern
✅ **Kubernetes-native**: Uses standard controller patterns

---

## Migration Path

Since `zonesFrom` is brand new (just implemented 2025-12-30), there are likely no production users yet. This is the **perfect time** to fix the architecture before it's adopted.

**Breaking change:** The `bindy.firestoned.io/selected-by-instance` annotation will be removed. Any existing DNSZones with this annotation will need to be reprocessed, but this happens automatically on controller restart.

---

## Next Steps

1. ✅ Create this roadmap document
2. ✅ Implement Step 1: Add DNSZone watch with reflector
3. ✅ Implement Step 2: Remove annotation code
4. ✅ Implement Step 3: Update DNSZone selection logic
5. ✅ Fix: Remove conflict prevention - zones can be selected by multiple instances
6. ✅ Fix: Use instance.spec.clusterRef instead of instance name
7. ⏳ Test end-to-end with examples
8. ✅ Update CHANGELOG.md with architecture improvement
