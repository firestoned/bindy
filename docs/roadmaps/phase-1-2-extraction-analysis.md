# Phase 1.2 Extraction Analysis

**Date:** 2026-01-12
**Status:** In Progress
**Goal:** Reduce reconcile_dnszone() from ~471 lines to ~80-150 lines

## Current State Analysis

### reconcile_dnszone() - Lines 384-855 (~471 lines)

**Already Extracted (Phase 1.1):**
- ✅ `refetch_zone()` - Already in helpers.rs
- ✅ `handle_duplicate_zone()` - Already in helpers.rs
- ✅ `detect_spec_changes()` - Already in helpers.rs
- ✅ `detect_instance_changes()` - Already in helpers.rs

### Extraction Opportunities

#### 1. **bind9_config.rs** - BIND9 Configuration Orchestration (~147 lines)
**Lines:** 565-711
**Purpose:** Orchestrate primary and secondary zone configuration
**Functions to extract:**

```rust
/// Configure zone on all BIND9 instances (primary and secondary)
pub async fn configure_zone_on_instances(
    ctx: Arc<Context>,
    dnszone: &DNSZone,
    zone_manager: &Bind9Manager,
    status_updater: &mut DNSZoneStatusUpdater,
    instance_refs: &[InstanceReference],
    unreconciled_instances: &[InstanceReference],
) -> Result<(usize, usize)>
```

This function should encapsulate:
- Setting initial "Progressing" status
- Finding primary IPs
- Calling `add_dnszone()` for primaries
- Calling `add_dnszone_to_secondaries()` for secondaries
- Handling errors and updating status conditions

Returns `(primary_count, secondary_count)`

#### 2. **status_helpers.rs** - Status Calculation and Application (~60 lines)
**Lines:** 775-835
**Purpose:** Calculate and apply final status conditions
**Functions to extract:**

```rust
/// Calculate expected instance counts
pub async fn calculate_expected_instance_counts(
    client: &Client,
    instance_refs: &[InstanceReference],
) -> Result<(usize, usize)>

/// Determine final zone status and apply conditions
pub async fn finalize_zone_status(
    status_updater: &mut DNSZoneStatusUpdater,
    client: &Client,
    zone_name: &str,
    namespace: &str,
    name: &str,
    primary_count: usize,
    secondary_count: usize,
    expected_primary_count: usize,
    expected_secondary_count: usize,
    records_count: usize,
    generation: Option<i64>,
) -> Result<()>
```

This function should:
- Check if status_updater has degraded conditions
- Compare actual vs expected counts
- Set Ready or Degraded condition accordingly
- Call `status_updater.apply(client).await`

#### 3. **Extract record discovery orchestration** (~30 lines)
**Lines:** 713-750
**Purpose:** Coordinate record discovery and status updates
**Already partially extracted:**
- `discovery::reconcile_zone_records()` exists
- `discovery::check_all_records_ready()` exists
- Just needs thin wrapper for status updates

```rust
/// Discover and update status with DNS records
async fn discover_and_update_records(
    client: &Client,
    dnszone: &DNSZone,
    status_updater: &mut DNSZoneStatusUpdater,
) -> Result<usize>
```

## Extraction Strategy

### Order of Extraction:
1. **status_helpers.rs** (lowest risk, pure logic)
2. **Record discovery wrapper** (simple delegation)
3. **bind9_config.rs** (largest extraction, most complex)

### After Extraction:

**reconcile_dnszone() will be ~150-180 lines:**
```rust
pub async fn reconcile_dnszone(...) -> Result<()> {
    // Setup (40 lines)
    let client = ctx.client.clone();
    let namespace = dnszone.namespace().unwrap_or_default();
    let name = dnszone.name_any();
    info!("Reconciling DNSZone: {}/{}", namespace, name);

    // Refetch and validate (30 lines)
    let watch_event_instances = ...;
    let dnszone = refetch_zone(&client, &namespace, &name).await?;
    let mut status_updater = DNSZoneStatusUpdater::new(&dnszone);
    let instance_refs = validation::get_instances_from_zone(...)?;

    // Duplicate check (10 lines)
    if let Some(duplicate_info) = validation::check_for_duplicate_zones(...) {
        handle_duplicate_zone(...).await?;
        return Ok(());
    }

    // Change detection (20 lines)
    let (first_reconciliation, spec_changed) = detect_spec_changes(&dnszone);
    let instances_changed = detect_instance_changes(...);
    let unreconciled_instances = validation::filter_instances_needing_reconciliation(...);

    // Cleanup (20 lines)
    cleanup::cleanup_deleted_instances(...).await?;
    cleanup::cleanup_stale_records(...).await?;

    // BIND9 configuration (5 lines - extracted to bind9_config.rs)
    let (primary_count, secondary_count) = bind9_config::configure_zone_on_instances(
        ctx.clone(),
        &dnszone,
        zone_manager,
        &mut status_updater,
        &instance_refs,
        &unreconciled_instances,
    ).await?;

    // Record discovery (5 lines - extracted wrapper)
    let records_count = discover_and_update_records(
        &client,
        &dnszone,
        &mut status_updater,
    ).await?;

    // Check if records are ready (10 lines)
    if records_count > 0 {
        let all_records_ready = discovery::check_all_records_ready(...).await?;
        if all_records_ready {
            info!("All records ready, BIND9 will handle zone transfers");
        }
    }

    // Calculate expected counts (5 lines - extracted to status_helpers.rs)
    let (expected_primary_count, expected_secondary_count) =
        status_helpers::calculate_expected_instance_counts(&client, &instance_refs).await?;

    // Finalize status (5 lines - extracted to status_helpers.rs)
    status_helpers::finalize_zone_status(
        &mut status_updater,
        &client,
        &spec.zone_name,
        &namespace,
        &name,
        primary_count,
        secondary_count,
        expected_primary_count,
        expected_secondary_count,
        records_count,
        dnszone.metadata.generation,
    ).await?;

    // Trigger record reconciliation (10 lines)
    if !status_updater.has_degraded_condition() {
        if let Err(e) = discovery::trigger_record_reconciliation(...).await {
            warn!("Failed to trigger record reconciliation: {}", e);
        }
    }

    Ok(())
}
```

**Total:** ~160 lines (66% reduction from 471 lines)

## Success Metrics

- [x] Phase 1.1 complete: 60.7% reduction (4,174 → 1,639 lines)
- [ ] Phase 1.2 target: ~66% reduction in reconcile_dnszone() (471 → ~160 lines)
- [ ] All 594 tests still passing
- [ ] Zero clippy warnings
- [ ] No performance regression

## Notes

- Some helper functions already exist from Phase 1.1 (refetch_zone, handle_duplicate_zone, etc.)
- Main extraction targets: bind9_config.rs (147 lines), status calculation (60 lines)
- Extraction improves testability - status logic can be unit tested in isolation
- Configuration orchestration can be tested without full reconciliation loop
