# Simplify Zone-Instance Relationship

**Date:** 2026-01-03
**Status:** Planning
**Impact:** Architecture simplification - eliminates bidirectional status writes

## Current Problem

The current design has **bidirectional status writes** between Bind9Instance and DNSZone:

1. Bind9Instance writes to `Bind9Instance.status.selectedZones[]`
2. Bind9Instance **also** writes to `DNSZone.status.bind9Instances[]`
3. DNSZone watches Bind9Instance and reads from **both** sources

This creates complexity:
- Two sources of truth for the same relationship
- Status writes across resource boundaries
- Unclear ownership of the relationship data
- Potential for inconsistency

## Proposed Solution

**Single Source of Truth**: `Bind9Instance.status.selectedZones[]`

### Bind9Instance Controller

**Reconciliation Logic:**
1. Discover zones matching `zonesFrom` selectors (using reflector store)
2. Write to `Bind9Instance.status.selectedZones[]` with structure:
   ```rust
   ZoneReference {
       api_version: "bindy.firestoned.io/v1beta1",
       kind: "DNSZone",
       name: "example-com",
       namespace: "dns-system",
       last_reconciled_at: Some("2026-01-03T03:00:00Z"), // or None if needs reconfiguration
   }
   ```
3. On **pod restart** or **spec change**: Set `last_reconciled_at = None` for ALL selected zones
   - This signals "zones need reconfiguration on this instance"

**Remove:**
- ❌ All code that writes to `DNSZone.status.bind9Instances[]`
- ❌ Cross-resource status updates

### DNSZone Controller

**Watch Mapper (NEW):**
```rust
.watches(bind9instance_api, semantic_watcher_config(), move |instance| {
    // Only reconcile zones that:
    // 1. Are in instance.status.selectedZones[]
    // 2. Have lastReconciledAt == None

    instance.status
        .as_ref()
        .map(|s| &s.selected_zones)
        .unwrap_or(&[])
        .iter()
        .filter(|zone_ref| zone_ref.last_reconciled_at.is_none())
        .map(|zone_ref| ObjectRef::new(&zone_ref.name).within(&zone_ref.namespace))
        .collect()
})
```

**Reconciler Logic:**
1. Get instances from:
   - `spec.bind9Instances[]` (explicit assignment) OR
   - Query reflector store for instances with this zone in `selectedZones[]`
2. Configure zone on each instance
3. **Update** `Bind9Instance.status.selectedZones[].lastReconciledAt` to current timestamp
   - Signals "zone successfully configured on this instance"

**Remove:**
- ❌ DNSZone.status.bind9Instances[] field (or deprecate it)
- ❌ Reading from DNSZone status for instance discovery

## Data Flow

### Normal Operation

```
1. Bind9Instance reconciles
   ↓
2. Discovers zones via zonesFrom
   ↓
3. Writes to Bind9Instance.status.selectedZones[]
   with lastReconciledAt = Some(timestamp)
   ↓
4. DNSZone watch mapper sees selectedZones[]
   ↓
5. lastReconciledAt != None → no reconciliation needed
```

### Pod Restart

```
1. Pod restarts
   ↓
2. Bind9Instance reconciles (via .owns() on Deployment)
   ↓
3. Sets lastReconciledAt = None for ALL selectedZones[]
   ↓
4. DNSZone watch mapper sees lastReconciledAt == None
   ↓
5. DNSZone reconciles → creates zone files on instance
   ↓
6. DNSZone updates Bind9Instance.status.selectedZones[].lastReconciledAt
```

### New Zone Created

```
1. DNSZone created with labels
   ↓
2. Bind9Instance watches DNSZone (semantic)
   ↓
3. Bind9Instance reconciles, discovers new zone
   ↓
4. Adds to selectedZones[] with lastReconciledAt = None
   ↓
5. DNSZone watch mapper sees lastReconciledAt == None
   ↓
6. DNSZone reconciles → creates zone on instance
   ↓
7. Updates lastReconciledAt to timestamp
```

## Benefits

✅ **Single source of truth**: `Bind9Instance.status.selectedZones[]` owns the relationship
✅ **No cross-resource status writes**: Bind9Instance only writes to itself
✅ **Clear ownership**: Instance owns the list, Zone acts on it
✅ **Explicit signals**: `lastReconciledAt == None` means "needs work"
✅ **Efficient**: Early return in watch mapper prevents unnecessary reconciliations
✅ **Simpler**: Eliminates bidirectional status synchronization

## Implementation Steps

### Phase 1: Update Bind9Instance Controller
- [ ] Simplify `update_instance_zone_status()` to only write to `Bind9Instance.status`
- [ ] Remove all code that writes to `DNSZone.status.bind9Instances[]`
- [ ] Add logic to set `lastReconciledAt = None` on pod restart/spec change
- [ ] Update tests

### Phase 2: Update DNSZone Controller
- [ ] Add watch on Bind9Instance with smart mapper
- [ ] Update `get_instances_from_zone()` to query reflector store
- [ ] Remove dependency on `DNSZone.status.bind9Instances[]`
- [ ] Update reconciler to write `lastReconciledAt` to Bind9Instance status
- [ ] Update tests

### Phase 3: Deprecate DNSZone.status.bind9Instances[]
- [ ] Mark field as deprecated in CRD schema
- [ ] Update documentation
- [ ] Add migration guide for users
- [ ] (Future) Remove field in next major version

## Testing

- [ ] Unit tests for watch mapper logic
- [ ] Integration test: Pod restart triggers zone reconfiguration
- [ ] Integration test: New zone added to instance via zonesFrom
- [ ] Integration test: Zone removed from instance
- [ ] Integration test: Multiple instances selecting same zone

## Risks

⚠️ **Breaking change**: Removes writes to `DNSZone.status.bind9Instances[]`
   - Mitigation: Keep field in CRD, just stop writing to it
   - Migration: Existing zones will work once instances reconcile

⚠️ **Reconciliation timing**: DNSZone must wait for Bind9Instance to reconcile first
   - Mitigation: Event-driven design ensures this happens automatically

## Success Criteria

- [ ] Zero writes to `DNSZone.status.bind9Instances[]`
- [ ] Pod restarts trigger zone reconfiguration within 30 seconds
- [ ] New zones discovered and configured within 30 seconds
- [ ] No reconciliation loops (verify with metrics)
- [ ] All integration tests passing
