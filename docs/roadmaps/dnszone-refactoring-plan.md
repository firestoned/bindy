# DNSZone Reconciler Refactoring Plan

**Status:** Planning - Not Started
**Priority:** Low (Future Work)
**Created:** 2026-01-09
**Author:** Erick Bourgeois

---

## Executive Summary

The `src/reconcilers/dnszone.rs` file has grown to 3,902 lines with 36 functions after Phase 1-3 cleanup (originally 5,305 lines). While functional, this large single-file structure makes the code harder to navigate, test, and maintain. This document proposes splitting it into a module structure organized by functional responsibility.

---

## Current State

### File Statistics (Post-Cleanup)
- **Total Lines:** 3,902 lines
- **Functions:** 36 functions
- **Structs:** 2 (`PodInfo`, `EndpointAddress`)
- **Complexity:** High - reconciliation, pod discovery, record management, cleanup all in one file

### Functional Areas Identified

The file contains 5 distinct functional areas:

1. **Zone Reconciliation** (Main logic)
   - `reconcile_dnszone()` - Main reconciler entry point
   - `add_dnszone()` - Add zone to primary instances
   - `add_dnszone_to_secondaries()` - Propagate to secondary instances
   - `delete_dnszone()` - Remove zone from all instances
   - `update_zone_reconciled_timestamp()` - Rate limiting

2. **Instance Selection & Filtering**
   - `get_instances_from_zone()` - Extract instance references from zone
   - `filter_instances_needing_reconciliation()` - Determine which instances need updates
   - `filter_primary_instances()` - Filter primary role instances
   - `filter_secondary_instances()` - Filter secondary role instances

3. **Pod & Endpoint Discovery**
   - `find_all_primary_pods()` - Find all primary pod IPs
   - `find_primary_ips_from_instances()` - Get primary IPs from instance list
   - `find_all_secondary_pods()` - Find all secondary pod IPs
   - `generate_nameserver_ips()` - Generate NS record IPs
   - `find_secondary_pod_ips_from_instances()` - Get secondary IPs from instances
   - `get_endpoint()` - Kubernetes endpoint discovery
   - `for_each_instance_endpoint()` - Iterator over instance endpoints
   - `for_each_primary_endpoint()` - Iterator over primary endpoints
   - `for_each_secondary_endpoint()` - Iterator over secondary endpoints
   - Structs: `PodInfo`, `EndpointAddress`

4. **DNS Record Management** (recordsFrom selector)
   - `reconcile_zone_records()` - Reconcile records matching selector
   - `tag_record_with_zone()` - Set status.zoneRef on matching records
   - `untag_record_from_zone()` - Clear status.zoneRef on non-matching records
   - `discover_a_records()` - Find ARecords matching selector
   - `discover_aaaa_records()` - Find AAAARecords matching selector
   - `discover_txt_records()` - Find TXTRecords matching selector
   - `discover_cname_records()` - Find CNAMERecords matching selector
   - `discover_mx_records()` - Find MXRecords matching selector
   - `discover_ns_records()` - Find NSRecords matching selector
   - `discover_srv_records()` - Find SRVRecords matching selector
   - `discover_caa_records()` - Find CAARecords matching selector
   - `find_zones_selecting_record()` - Reverse lookup: which zones select a record
   - `trigger_record_reconciliation()` - Force record re-reconciliation
   - `check_all_records_ready()` - Verify all records are ready
   - `check_record_ready()` - Check single record readiness

5. **Cleanup Operations**
   - `cleanup_deleted_instances()` - Remove references to deleted instances
   - `cleanup_stale_records()` - Remove orphaned record references

6. **Utilities**
   - `load_rndc_key()` - Load RNDC secret from Kubernetes

---

## Proposed Module Structure

```
src/reconcilers/dnszone/
├── mod.rs                       # Public API exports and module declarations
├── reconcile.rs                 # Main reconciliation logic
├── instances.rs                 # Instance selection and filtering
├── pod_discovery.rs             # Pod and endpoint discovery
├── record_selection.rs          # DNS record discovery and tagging (recordsFrom)
├── cleanup.rs                   # Cleanup operations
├── types.rs                     # Shared types (PodInfo, EndpointAddress)
└── utils.rs                     # Utility functions (load_rndc_key, etc.)
```

### File Breakdown

#### `mod.rs` (~50 lines)
- Module declarations
- Public API re-exports
- Module-level documentation

#### `reconcile.rs` (~800 lines)
**Responsibility:** Main zone reconciliation logic

**Functions:**
- `pub async fn reconcile_dnszone()` - Main entry point
- `pub async fn add_dnszone()` - Add zone to primary instances
- `pub async fn add_dnszone_to_secondaries()` - Propagate to secondaries
- `pub async fn delete_dnszone()` - Remove zone
- `fn update_zone_reconciled_timestamp()` - Rate limiting

**Dependencies:**
- Uses `instances::*` for instance selection
- Uses `pod_discovery::*` for endpoint discovery
- Uses `record_selection::*` for record management
- Uses `cleanup::*` for cleanup operations

#### `instances.rs` (~400 lines)
**Responsibility:** Instance selection and filtering logic

**Functions:**
- `pub fn get_instances_from_zone()` - Extract instance refs from zone
- `fn filter_instances_needing_reconciliation()` - Determine which need updates
- `pub async fn filter_primary_instances()` - Filter primary role
- `pub async fn filter_secondary_instances()` - Filter secondary role

**Dependencies:**
- Uses Kubernetes client to fetch instance status
- Pure business logic, minimal external deps

#### `pod_discovery.rs` (~1000 lines)
**Responsibility:** Kubernetes pod and endpoint discovery

**Functions:**
- `pub async fn find_all_primary_pods()` - Find primary pod IPs
- `async fn find_primary_ips_from_instances()` - Get primary IPs
- `async fn find_all_secondary_pods()` - Find secondary pod IPs
- `pub async fn find_secondary_pod_ips_from_instances()` - Get secondary IPs
- `pub async fn generate_nameserver_ips()` - Generate NS IPs
- `pub async fn get_endpoint()` - Kubernetes endpoint lookup
- `pub async fn for_each_instance_endpoint()` - Endpoint iterator
- `pub async fn for_each_primary_endpoint()` - Primary endpoint iterator
- `pub async fn for_each_secondary_endpoint()` - Secondary endpoint iterator

**Types:**
- `pub struct PodInfo` - Pod metadata
- `pub struct EndpointAddress` - Endpoint info (IP + port)

**Dependencies:**
- Heavy Kubernetes API usage (Pod API, Service API)
- Returns results to reconcile.rs

#### `record_selection.rs` (~1200 lines)
**Responsibility:** DNS record discovery and tagging based on `recordsFrom` selector

**Functions:**
- `async fn reconcile_zone_records()` - Main record reconciliation
- `async fn tag_record_with_zone()` - Set status.zoneRef
- `async fn untag_record_from_zone()` - Clear status.zoneRef
- `async fn discover_a_records()` - Find matching ARecords
- `async fn discover_aaaa_records()` - Find matching AAAARecords
- `async fn discover_txt_records()` - Find matching TXTRecords
- `async fn discover_cname_records()` - Find matching CNAMERecords
- `async fn discover_mx_records()` - Find matching MXRecords
- `async fn discover_ns_records()` - Find matching NSRecords
- `async fn discover_srv_records()` - Find matching SRVRecords
- `async fn discover_caa_records()` - Find matching CAARecords
- `pub async fn find_zones_selecting_record()` - Reverse lookup
- `async fn trigger_record_reconciliation()` - Force record update
- `async fn check_all_records_ready()` - Verify readiness
- `async fn check_record_ready()` - Check single record

**Dependencies:**
- Uses record APIs (ARecord, AAAARecord, etc.)
- Uses label selectors from zone spec
- Called by reconcile.rs

#### `cleanup.rs` (~300 lines)
**Responsibility:** Cleanup of stale data

**Functions:**
- `pub async fn cleanup_deleted_instances()` - Remove deleted instance refs
- `pub async fn cleanup_stale_records()` - Remove orphaned records

**Dependencies:**
- Uses Kubernetes client
- Called periodically by reconcile.rs

#### `types.rs` (~50 lines)
**Responsibility:** Shared types used across the module

**Types:**
- `pub struct PodInfo` - Pod information
- `pub struct EndpointAddress` - Endpoint information

**Dependencies:**
- None (pure data structures)

#### `utils.rs` (~100 lines)
**Responsibility:** Utility functions

**Functions:**
- `async fn load_rndc_key()` - Load RNDC secret
- (Future: other utility functions)

**Dependencies:**
- Kubernetes Secret API

---

## Implementation Strategy

### Phase 1: Preparation (1-2 hours)
1. Create new directory: `src/reconcilers/dnszone/`
2. Create empty files for each module
3. Set up `mod.rs` with module declarations
4. Add comprehensive test suite for current behavior (integration tests)

### Phase 2: Extract Types (30 minutes)
1. Move `PodInfo` and `EndpointAddress` to `types.rs`
2. Update imports in main file
3. Verify compilation
4. Run tests

### Phase 3: Extract Utilities (30 minutes)
1. Move `load_rndc_key()` to `utils.rs`
2. Update imports
3. Verify compilation
4. Run tests

### Phase 4: Extract Pod Discovery (2-3 hours)
1. Move pod/endpoint discovery functions to `pod_discovery.rs`
2. Make types public where needed
3. Update imports in reconcile logic
4. Verify compilation
5. Run tests

### Phase 5: Extract Record Selection (2-3 hours)
1. Move record management functions to `record_selection.rs`
2. Update imports
3. Verify compilation
4. Run tests

### Phase 6: Extract Cleanup (1 hour)
1. Move cleanup functions to `cleanup.rs`
2. Update imports
3. Verify compilation
4. Run tests

### Phase 7: Extract Instance Logic (1-2 hours)
1. Move instance selection/filtering to `instances.rs`
2. Update imports
3. Verify compilation
4. Run tests

### Phase 8: Finalize Reconcile (1 hour)
1. Rename `dnszone.rs` to `dnszone/reconcile.rs`
2. Keep only main reconciliation functions
3. Update all imports across project
4. Verify compilation
5. Run full test suite
6. Run integration tests

### Phase 9: Documentation (1 hour)
1. Add module-level docs to each file
2. Update architecture documentation
3. Update CHANGELOG.md
4. Update contributor guide

---

## Testing Strategy

### Before Refactoring
1. Ensure 100% test coverage for public functions
2. Run integration tests to establish baseline
3. Document current behavior

### During Refactoring
1. After each phase, verify:
   - `cargo fmt` passes
   - `cargo clippy` passes (no warnings)
   - `cargo test` passes (all tests)
   - Integration tests pass
2. No behavior changes - only code organization

### After Refactoring
1. Verify identical behavior with integration tests
2. Check that public API remains unchanged
3. Verify performance characteristics unchanged

---

## Benefits of Refactoring

### Code Organization
- **Easier Navigation**: Find functions by functional area
- **Clear Boundaries**: Each module has one responsibility
- **Better IDE Support**: Smaller files load faster in editors

### Maintainability
- **Isolated Changes**: Changes to pod discovery don't affect record selection
- **Easier Reviews**: Smaller files are easier to review in PRs
- **Reduced Merge Conflicts**: Changes less likely to conflict

### Testing
- **Focused Tests**: Test pod discovery separately from reconciliation
- **Unit Test Isolation**: Mock dependencies more easily
- **Better Coverage**: Easier to identify untested code paths

### Onboarding
- **Clearer Architecture**: New contributors understand structure faster
- **Documentation**: Each module documents its purpose
- **Examples**: Easier to provide examples for specific areas

---

## Risks and Mitigation

### Risk: Breaking Changes
**Mitigation:**
- Keep all public APIs unchanged
- Use re-exports in `mod.rs`
- Comprehensive test suite before starting

### Risk: Import Cycles
**Mitigation:**
- Plan dependency graph before moving code
- Use types.rs for shared types
- Keep reconcile.rs as the orchestrator (top level)

### Risk: Regressions
**Mitigation:**
- Move code incrementally (one module at a time)
- Run tests after each phase
- Integration tests verify end-to-end behavior

### Risk: Time Investment
**Mitigation:**
- Refactoring is low priority (Phase 4)
- Can be done incrementally over multiple releases
- Provides long-term benefits

---

## Timeline (Estimated)

- **Total Effort:** 10-15 hours
- **Can be split across:** Multiple PRs/releases
- **Target Start:** After v0.3.0 release
- **Target Completion:** v0.4.0 or v1.0.0

---

## Success Criteria

- [ ] All 36 functions moved to appropriate modules
- [ ] Zero behavior changes (verified by integration tests)
- [ ] All tests pass (37+ unit tests, all integration tests)
- [ ] cargo clippy passes with zero warnings
- [ ] Documentation updated
- [ ] Public API remains unchanged (backward compatible)
- [ ] Code easier to navigate (verified by code review)

---

## Alternatives Considered

### Alternative 1: Keep Single File
**Pros:**
- No refactoring effort
- No risk of breaking changes

**Cons:**
- File continues to grow
- Harder to maintain and navigate
- Merge conflicts more likely

**Decision:** Rejected - file is already too large

### Alternative 2: Refactor AND Redesign
**Pros:**
- Could improve architecture while refactoring

**Cons:**
- Much higher risk
- Behavior changes hard to verify
- Would require extensive testing

**Decision:** Rejected - refactor first, redesign later if needed

### Alternative 3: Incremental Extraction (Chosen)
**Pros:**
- Low risk (one module at a time)
- Easy to verify correctness
- Can be paused/resumed

**Cons:**
- Takes longer overall

**Decision:** Accepted - safest approach

---

## Related Documents

- [Code Cleanup Analysis](./code-cleanup-analysis.md) - Phase 1-3 cleanup
- [DNSZone Controller Architecture](../src/concepts/dnszone-controller-architecture.md)
- [Integration Test Plan](./integration-test-plan.md)

---

## Notes

- This refactoring is purely organizational - no logic changes
- All public APIs remain unchanged
- Follow TDD workflow from CLAUDE.md
- Each phase should be a separate commit
- Consider making this a separate PR per phase for easier review
