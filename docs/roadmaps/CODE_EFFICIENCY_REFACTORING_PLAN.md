# Code Efficiency Refactoring Plan

**Date:** 2025-12-27 (Updated)
**Status:** Planned
**Impact:** CRITICAL - Reduces ~2,000+ lines of duplicate code, significantly improves maintainability

---

## Executive Summary

Comprehensive analysis of the Bindy operator codebase identified **significant code duplication and long functions** across multiple files:

- **`src/main.rs`**: ~1,200 lines of duplicate wrapper code
- **`src/reconcilers/`**: 8 record reconcilers with 95% identical code (~1,330 lines)
- **Large functions**: 15 functions exceeding 50-100 lines that need refactoring

**Total Impact:** ~2,000+ lines can be eliminated or refactored into smaller, focused functions.

This document outlines a phased refactoring plan to:
1. Eliminate code duplication (macros, generics, helpers)
2. Break down large functions (300+ lines) into smaller, testable units
3. Improve maintainability and readability across the codebase

---

##  Critical Issues (High ROI)

### 1. Consolidate Record Wrapper Functions

**Current State:** 8 nearly identical wrapper functions (~900 lines total)
- `reconcile_arecord_wrapper()`
- `reconcile_txtrecord_wrapper()`
- `reconcile_aaaarecord_wrapper()`
- `reconcile_cnamerecord_wrapper()`
- `reconcile_mxrecord_wrapper()`
- `reconcile_nsrecord_wrapper()`
- `reconcile_srvrecord_wrapper()`
- `reconcile_caarecord_wrapper()`

**Lines:** [src/main.rs:1009-1417](../src/main.rs#L1009-L1417) (~408 lines total for all 8 functions)

**Problem:** Each function contains identical logic for:
- Timing/metrics
- Status checking
- Error handling
- Requeue logic

Only differences:
- Record type
- KIND constant
- Display name

**Solution:**

Create a macro to generate all 8 wrappers:

```rust
// Add constants
const REQUEUE_WHEN_READY_SECS: u64 = 300;
const REQUEUE_WHEN_NOT_READY_SECS: u64 = 30;
const CONDITION_TYPE_READY: &str = "Ready";
const CONDITION_STATUS_TRUE: &str = "True";
const ERROR_TYPE_RECONCILE: &str = "reconcile_error";

// Add helper functions
fn is_resource_ready<T>(status: &Option<T>) -> bool
where
    T: AsRef<bindy::crd::RecordStatus>,
{
    status
        .as_ref()
        .map(|s| {
            s.as_ref()
                .conditions
                .first()
                .is_some_and(|condition| {
                    condition.r#type == CONDITION_TYPE_READY
                        && condition.status == CONDITION_STATUS_TRUE
                })
        })
        .unwrap_or(false)
}

fn requeue_based_on_readiness(is_ready: bool) -> Action {
    if is_ready {
        Action::requeue(Duration::from_secs(REQUEUE_WHEN_READY_SECS))
    } else {
        Action::requeue(Duration::from_secs(REQUEUE_WHEN_NOT_READY_SECS))
    }
}

// Add macro
macro_rules! generate_record_wrapper {
    ($wrapper_fn:ident, $record_type:ty, $reconcile_fn:ident, $kind_const:ident, $display_name:expr) => {
        async fn $wrapper_fn(
            record: Arc<$record_type>,
            ctx: Arc<(Client, Arc<Bind9Manager>)>,
        ) -> Result<Action, ReconcileError> {
            use bindy::constants::$kind_const;
            let start = std::time::Instant::now();
            let result = $reconcile_fn(ctx.0.clone(), (*record).clone()).await;
            let duration = start.elapsed();

            match result {
                Ok(()) => {
                    info!("Successfully reconciled {}: {}", $display_name, record.name_any());
                    metrics::record_reconciliation_success($kind_const, duration);

                    let namespace = record.namespace().unwrap_or_default();
                    let name = record.name_any();
                    let api: Api<$record_type> = Api::namespaced(ctx.0.clone(), &namespace);

                    let is_ready = if let Ok(updated_record) = api.get(&name).await {
                        is_resource_ready(&updated_record.status)
                    } else {
                        false
                    };

                    Ok(requeue_based_on_readiness(is_ready))
                }
                Err(e) => {
                    error!("Failed to reconcile {}: {}", $display_name, e);
                    metrics::record_reconciliation_error($kind_const, duration);
                    metrics::record_error($kind_const, ERROR_TYPE_RECONCILE);
                    Err(e.into())
                }
            }
        }
    };
}

// Generate all 8 wrappers
generate_record_wrapper!(reconcile_arecord_wrapper, ARecord, reconcile_a_record, KIND_A_RECORD, "ARecord");
generate_record_wrapper!(reconcile_txtrecord_wrapper, TXTRecord, reconcile_txt_record, KIND_TXT_RECORD, "TXTRecord");
generate_record_wrapper!(reconcile_aaaarecord_wrapper, AAAARecord, reconcile_aaaa_record, KIND_AAAA_RECORD, "AAAARecord");
generate_record_wrapper!(reconcile_cnamerecord_wrapper, CNAMERecord, reconcile_cname_record, KIND_CNAME_RECORD, "CNAMERecord");
generate_record_wrapper!(reconcile_mxrecord_wrapper, MXRecord, reconcile_mx_record, KIND_MX_RECORD, "MXRecord");
generate_record_wrapper!(reconcile_nsrecord_wrapper, NSRecord, reconcile_ns_record, KIND_NS_RECORD, "NSRecord");
generate_record_wrapper!(reconcile_srvrecord_wrapper, SRVRecord, reconcile_srv_record, KIND_SRV_RECORD, "SRVRecord");
generate_record_wrapper!(reconcile_caarecord_wrapper, CAARecord, reconcile_caa_record, KIND_CAA_RECORD, "CAARecord");
```

**Estimated Reduction:** ~900 lines → ~150 lines
**Testing:** All existing integration tests should pass unchanged

---

### 2. Remove Controller Setup Duplication

**Current State:** Controller setup duplicated in two locations

**Lines:**
- [src/main.rs:243-302](../src/main.rs#L243-L302) - In `run_controllers_without_leader_election()`
- [src/main.rs:381-440](../src/main.rs#L381-L440) - In `run_all_controllers()`

**Problem:** The same `tokio::select!` block with 12 controller error handlers exists twice

**Solution:** Delete lines 243-302 and replace with call to `run_all_controllers(client, bind9_manager).await`

**Estimated Reduction:** ~60 lines
**Testing:** Verify both leader election and non-leader-election modes work

---

### 3. Extract Ready Status Checking Helper

**Current State:** Pattern repeated 12 times

**Lines:** Scattered throughout wrapper functions

**Pattern:**
```rust
let is_ready = resource
    .status
    .as_ref()
    .and_then(|status| status.conditions.first())
    .is_some_and(|condition| condition.r#type == "Ready" && condition.status == "True");

if is_ready {
    Ok(Action::requeue(Duration::from_secs(300)))
} else {
    Ok(Action::requeue(Duration::from_secs(30)))
}
```

**Solution:** Already covered by #1 above (helper functions in macro)

**Estimated Reduction:** Included in ~900 line reduction from #1

---

## CRITICAL: Reconciler Function Refactoring

### 1A. Generic Record Reconciler (Eliminates 8 Duplicate Functions)

**Files:** `src/reconcilers/records.rs`
**Functions Affected:** 8 record reconcilers (Lines 134-299 per function, ~1,330 total lines)
- `reconcile_a_record()` (166 lines)
- `reconcile_aaaa_record()` (166 lines)
- `reconcile_txt_record()` (170 lines)
- `reconcile_cname_record()` (166 lines)
- `reconcile_mx_record()` (166 lines)
- `reconcile_ns_record()` (166 lines)
- `reconcile_srv_record()` (166 lines)
- `reconcile_caa_record()` (166 lines)

**Problem:** These 8 functions share **95% identical code**. The only differences are:
- Record type (ARecord vs TXTRecord vs etc.)
- Type-specific `add_*_record_to_zone()` call
- Field access (spec.ipv4_address vs spec.data)

**Each function follows the identical pattern:**
1. Check if record is selected by a zone
2. Calculate hash of record spec
3. Skip if data unchanged (hash match)
4. Get zone info from annotation
5. Add record to zone via BIND9
6. Update record status with results

**Solution:** Create a **generic reconciler** using trait abstraction:

```rust
// Define a trait for record types
pub trait DNSRecordType: Resource + Clone + Debug {
    type RecordSpec;

    fn spec(&self) -> &Self::RecordSpec;
    fn add_to_zone(client: Client, record: Self, zone_fqdn: String) -> Result<()>;
}

// Implement trait for each record type
impl DNSRecordType for ARecord {
    type RecordSpec = ARecordSpec;

    fn spec(&self) -> &Self::RecordSpec { &self.spec }
    fn add_to_zone(client: Client, record: Self, zone_fqdn: String) -> Result<()> {
        add_a_record_to_zone(client, record, zone_fqdn).await
    }
}

// Generic reconciler works for all types
pub async fn reconcile_dns_record<T: DNSRecordType>(
    client: Client,
    record: T,
) -> Result<()> {
    // Shared logic for all 8 record types
    let zone_fqdn = check_zone_selection(&client, &record).await?;

    if !should_update_record(&record).await? {
        return Ok(());  // Skip unchanged records
    }

    let (zone_name, cluster_ref) = get_zone_and_cluster_ref(&client, &record).await?;

    T::add_to_zone(client.clone(), record.clone(), zone_fqdn).await?;

    update_record_status(&client, &record, success_condition()).await?;

    Ok(())
}
```

**Extraction Steps:**

1. **Extract zone selection** → `check_zone_selection<T>()` (Lines 147-171 per function)
2. **Extract change detection** → `should_update_record<T>()` (Lines 173-206 per function)
3. **Extract zone info retrieval** → `get_zone_and_cluster_ref()` (Lines 219-242 per function)
4. **Extract status update** → Already exists as `update_record_status<T>()`
5. **Create trait for type-specific operations**
6. **Implement generic reconciler**

**Impact:**
- Eliminates ~800-1000 lines of duplicate code
- Bug fixes apply to all record types automatically
- Single test suite validates all record types
- Much easier to add new record types

**Estimated Effort:** 2-3 days

---

### 1B. `reconcile_dnszone()` - Break Into 5 Functions

**File:** [src/reconcilers/dnszone.rs:98-405](src/reconcilers/dnszone.rs#L98-L405)
**Current Lines:** 308
**Complexity:** Multiple nested match expressions, scattered status updates

**Problem:** This massive function handles **5 distinct phases**:
1. Extract spec and validate cluster references
2. Configure primary zone servers
3. Configure secondary zone servers
4. Discover DNS records via label selectors
5. Trigger zone transfers

**Solution:** Extract each phase into a focused function:

```rust
pub async fn reconcile_dnszone(
    client: Client,
    dnszone: Arc<DNSZone>,
    ctx: Arc<Context>,
) -> Result<Action> {
    let zone_name = extract_and_validate_spec(&dnszone)?;

    let primary_ips = configure_primary_zones(
        client.clone(), &dnszone, &ctx
    ).await?;

    let secondary_ips = configure_secondary_zones(
        client.clone(), &dnszone, &ctx
    ).await?;

    discover_and_tag_zone_records(
        client.clone(), &dnszone
    ).await?;

    ensure_zone_transfers(
        client.clone(), &dnszone, &primary_ips, &secondary_ips
    ).await?;

    Ok(Action::requeue(Duration::from_secs(RECONCILE_INTERVAL_SECS)))
}
```

**Extraction Targets:**

1. **`configure_primary_zones()`** (Lines 152-282)
   - Validates cluster references
   - Calls `add_dnszone()`
   - Handles failures with status updates
   - Returns primary IPs

2. **`configure_secondary_zones()`** (Lines 231-279)
   - Calls `add_dnszone_to_secondaries()`
   - Handles failures gracefully
   - Returns secondary IPs

3. **`discover_and_tag_zone_records()`** (Lines 284-320)
   - Calls `reconcile_zone_records()`
   - Handles discovery failures
   - Updates status conditions

4. **`ensure_zone_transfers()`** (Lines 322-368)
   - Checks if records are ready
   - Triggers zone transfers
   - Handles transfer failures

**Impact:**
- Main reconciler becomes 30-40 lines (clear orchestration)
- Each phase independently testable
- Status updates localized to each phase
- Much easier to modify individual phases

**Estimated Effort:** 2-3 days

---

### 1C. `build_options_conf()` - Break Into 5 Functions

**File:** [src/bind9_resources.rs:409-566](src/bind9_resources.rs#L409-L566)
**Current Lines:** 158
**Complexity:** 17 nested if/else blocks, duplicated priority resolution

**Problem:** Handles **4 config options** (recursion, allow-query, allow-transfer, dnssec-validate) with **3-level priority** (instance > role/global > defaults). Logic is deeply nested and duplicated.

**Solution:** Extract each configuration option:

```rust
pub fn build_options_conf(
    instance_name: &str,
    role: Bind9Role,
    global_config: &Bind9GlobalConfig,
    role_config: &Bind9RoleConfig,
    instance_config: Option<&Bind9InstanceConfig>,
) -> String {
    let recursion = build_recursion_setting(instance_config, global_config);
    let allow_query = build_allow_query_setting(instance_config, role_config, global_config);
    let allow_transfer = build_allow_transfer_setting(instance_config, role_config, global_config);
    let dnssec_validate = build_dnssec_setting(instance_config, global_config);

    format!("options {{\n{recursion}{allow_query}{allow_transfer}{dnssec_validate}}};\n")
}
```

**Extraction Targets:**

1. **`get_effective_config<T>()`** - Generic priority resolver
   - Handles instance > global > default pattern
   - Eliminates duplication

2. **`build_recursion_setting()`** (Lines 414-439)
3. **`build_allow_query_setting()`** (Lines 441-454)
4. **`build_allow_transfer_setting()`** (Lines 456-488)
5. **`build_dnssec_setting()`** (Lines 490-507)

**Impact:**
- Eliminates 17 nested if/else blocks
- Each config option independently testable
- Easy to add new configuration options
- Clear priority resolution

**Estimated Effort:** 1-2 days

---

### 1D. `reconcile_managed_instances()` - Break Into 4 Functions

**File:** [src/reconcilers/bind9cluster.rs:458-668](src/reconcilers/bind9cluster.rs#L458-L668)
**Current Lines:** 211
**Complexity:** Duplicated scaling logic for primary/secondary

**Problem:** Handles instance scaling for **both roles** with **nearly identical code**:
1. List and filter existing instances
2. Scale up primary instances (create)
3. Scale down primary instances (delete)
4. Scale up secondary instances (create)
5. Scale down secondary instances (delete)
6. Update existing instances

**Solution:**

```rust
pub async fn reconcile_managed_instances(
    client: Client,
    cluster: &Bind9Cluster,
    namespace: &str,
) -> Result<()> {
    let (primary_instances, secondary_instances) =
        list_managed_instances(client.clone(), cluster, namespace).await?;

    reconcile_primary_instances(
        client.clone(), cluster, namespace, primary_instances
    ).await?;

    reconcile_secondary_instances(
        client.clone(), cluster, namespace, secondary_instances
    ).await?;

    update_existing_managed_instances(
        client.clone(), cluster, namespace
    ).await?;

    ensure_managed_instance_resources(
        client.clone(), cluster, namespace
    ).await?;

    Ok(())
}
```

**Extraction Targets:**

1. **`list_managed_instances()`** (Lines 497-536)
2. **`reconcile_primary_instances()`** (Lines 548-588)
3. **`reconcile_secondary_instances()`** (Lines 590-631)
4. **`log_scaling_results()`** (Lines 633-651)

**Impact:**
- Eliminates duplicated scaling logic
- Clear separation between primary/secondary
- Easier to test role-specific scaling
- Better maintainability

**Estimated Effort:** 1-2 days

---

## Medium Priority Issues

### 4. Consolidate Error Policy Functions

**Current State:** 5 identical error policy functions

**Lines:** [src/main.rs:1418-1453](../src/main.rs#L1418-L1453)

**Functions:**
- `error_policy()`
- `error_policy_dnszone()`
- `error_policy_cluster()`
- `error_policy_clusterprovider()`
- `error_policy_instance()`

**Solution:** Use a single generic function or replace with closures

```rust
fn generic_error_policy<T: std::fmt::Debug, C>(
    _resource: Arc<T>,
    _err: &ReconcileError,
    _ctx: Arc<C>,
) -> Action {
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_DURATION_SECS))
}
```

**Estimated Reduction:** ~40 lines → ~8 lines

---

### 5. Extract Watcher Config Constant

**Current State:** `Config::default().any_semantic()` repeated 16 times

**Solution:**
```rust
// At module level
fn default_watcher_config() -> Config {
    Config::default().any_semantic()
}

// Use in controllers
let watcher_config = default_watcher_config();
```

**Estimated Reduction:** Minor, but improves consistency

---

### 6. Add String Constants (Per Project Guidelines)

**Current State:** Hardcoded strings violate `.claude/CLAUDE.md` guidelines

**Strings appearing 2+ times:**
- `"Ready"` - 12+ times (NOTE: Already added as `CONDITION_TYPE_READY` in #1)
- `"reconcile_error"` - 12 times (NOTE: Already added as `ERROR_TYPE_RECONCILE` in #1)

**Solution:** Already covered in #1 above

---

## Low Priority Issues

### 7. Remove Unused `dnszone_store` Field

**Lines:** [src/main.rs:518-519](../src/main.rs#L518-L519)

```rust
#[allow(dead_code)]
dnszone_store: kube::runtime::reflector::Store<DNSZone>,
```

**Solution:** Remove if truly unused, or document why it's kept

---

### 8. Add Duration Constants

**Current State:** Duration values hardcoded

**Solution:** Already added in #1:
```rust
pub const REQUEUE_WHEN_READY_SECS: u64 = 300;
pub const REQUEUE_WHEN_NOT_READY_SECS: u64 = 30;
```

---

## Implementation Plan

### Phase 1: High Impact Refactoring
1. **Consolidate record wrappers** (#1)
   - Add constants and helper functions
   - Add macro
   - Generate wrapper functions
   - Delete old implementations
   - Run `cargo test` to verify
   - Run integration tests

2. **Remove controller setup duplication** (#2)
   - Delete duplicate code in `run_controllers_without_leader_election`
   - Call `run_all_controllers` instead
   - Test both leader election modes

### Phase 2: Medium Impact Cleanup
3. **Consolidate error policies** (#4)
4. **Extract watcher config** (#5)

### Phase 3: Low Priority Cleanup
5. **Remove unused field** (#7)
6. **Documentation cleanup**

---

## Testing Strategy

For each phase:

1. **Unit Tests:**
   ```bash
   cargo test --lib
   ```

2. **Integration Tests:**
   ```bash
   cargo test --test simple_integration -- --ignored --test-threads=1 --nocapture
   ```

3. **Lint Checks:**
   ```bash
   cargo fmt
   cargo clippy -- -D warnings -W clippy::pedantic -A clippy::module_name_repetitions
   ```

4. **Functional Testing:**
   - Deploy to test cluster
   - Create all 8 record types
   - Verify reconciliation works
   - Check metrics/logs

---

## Success Criteria

- [ ] All tests pass (`cargo test`)
- [ ] Integration tests pass
- [ ] Clippy warnings resolved
- [ ] Code formatted (`cargo fmt`)
- [ ] Line count reduced by ~1,000 lines
- [ ] No functional regressions
- [ ] Metrics still work
- [ ] Error handling unchanged

---

## Rollback Plan

If issues arise:
```bash
git checkout src/main.rs
```

Commit each phase separately to allow selective rollback.

---

## Estimated Impact

| Metric | Before | After | Change |
|--------|--------|-------|--------|
| Lines of Code (main.rs) | ~1,753 | ~553 | **-1,200 lines (-68%)** |
| Record Wrappers | 8 × 51 lines | 8 × 1 macro call | -400 lines |
| Helper Functions | 0 | 2 | +50 lines |
| Macro | 0 | 1 | +40 lines |
| Error Policies | 5 × 8 lines | 1 × 8 lines | -32 lines |
| Maintainability | Low | High | Significant improvement |

---

## Related Documents

- [.claude/CLAUDE.md](../.claude/CLAUDE.md) - Project coding guidelines
- [Code Efficiency Analysis Report](../CHANGELOG.md) - Initial findings

---

## Appendix: Complete Function Inventory

### Long Functions Requiring Refactoring

| Priority | Function | File | Lines | Key Issue | Estimated Effort |
|----------|----------|------|-------|-----------|------------------|
| CRITICAL | `reconcile_*_record()` × 8 | records.rs | 166-170 ea | 95% duplicate code | 2-3 days |
| CRITICAL | `reconcile_dnszone()` | dnszone.rs | 308 | 5 distinct phases | 2-3 days |
| CRITICAL | `build_options_conf()` | bind9_resources.rs | 158 | 17 nested if/else | 1-2 days |
| CRITICAL | `reconcile_managed_instances()` | bind9cluster.rs | 211 | Duplicated scaling | 1-2 days |
| HIGH | `add_dnszone_to_secondaries()` | dnszone.rs | 161 | Double nested loop | 1 day |
| HIGH | `update_record_status<T>()` | records.rs | 151 | Complex merging | 1 day |
| HIGH | `add_dnszone()` | dnszone.rs | 128 | Mixed concerns | 0.5 day |
| MEDIUM | `find_all_primary_pods()` | dnszone.rs | 119 | Deep nesting | 1 day |
| MEDIUM | `for_each_primary_endpoint()` | dnszone.rs | 86 | Generic iteration | 1 day |
| MEDIUM | `check_all_records_ready()` | dnszone.rs | 63 | Large match stmt | 0.5 day |
| MODERATE | `create_managed_instance_with_owner()` | bind9cluster.rs | 144 | Metadata building | 1 day |
| MODERATE | `build_pod_spec()` | bind9_resources.rs | 126 | Container building | 1 day |
| MODERATE | `build_api_sidecar_container()` | bind9_resources.rs | 113 | Mixed concerns | 0.5 day |
| MODERATE | `build_volume_mounts()` | bind9_resources.rs | 101 | Config cascade | 0.5 day |
| MODERATE | `find_all_secondary_pods()` | dnszone.rs | 107 | Pod iteration | 0.5 day |

**Total:** 15 long functions + 8 wrapper functions = 23 functions requiring refactoring
**Total Lines:** ~2,300 lines in reconcilers + ~1,200 lines in main.rs = **~3,500 lines**
**Estimated Effort:** 15-20 days total

### Code Duplication Summary

| Category | Files | Duplicate Lines | Reduction Potential |
|----------|-------|-----------------|---------------------|
| Record reconcilers | `src/reconcilers/records.rs` | ~1,330 | ~800-1000 lines |
| Record wrappers | `src/main.rs` | ~900 | ~750 lines |
| Error policies | `src/main.rs` | ~40 | ~32 lines |
| Configuration builders | `src/bind9_resources.rs` | ~158 | ~100 lines |
| Instance scaling | `src/reconcilers/bind9cluster.rs` | ~211 | ~150 lines |
| **TOTAL** | | **~2,639 lines** | **~1,832-2,032 lines** |

---

## Author

Erick Bourgeois

## Status

- **Planned:** 2025-12-27 (Updated with comprehensive analysis)
- **Implementation:** Pending approval
- **Completion:** TBD
