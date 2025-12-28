# Status Conditions Implementation Tracking

**Author:** Erick Bourgeois
**Created:** 2025-01-18
**Last Updated:** 2025-01-18

## Overview

This document tracks the implementation of hierarchical status condition tracking for Bindy resources. It serves as a checklist for completing the work outlined in [STATUS_CONDITIONS_DESIGN.md](STATUS_CONDITIONS_DESIGN.md).

## Phase 1: Foundation ✅ COMPLETED

### Files Created
- [x] `src/status_reasons.rs` - Standard condition reason constants
- [x] `STATUS_CONDITIONS_DESIGN.md` - Design document
- [x] `STATUS_CONDITIONS_IMPLEMENTATION.md` - This tracking document

### Code Changes
- [x] Added `status_reasons` module to `src/lib.rs`
- [x] Added 30+ standard condition reason constants
- [x] Added helper functions for child condition types
- [x] Added HTTP error code mapping constants
- [x] Added comprehensive documentation and examples

## Phase 2: CRD Updates (IN PROGRESS)

### Task: Update Bind9InstanceStatus Structure

**File:** `src/crd.rs` (lines 1755-1769)

**Current Status Structure:**
```rust
pub struct Bind9InstanceStatus {
    pub conditions: Vec<Condition>,
    pub observed_generation: Option<i64>,
    pub replicas: Option<i32>,
    pub ready_replicas: Option<i32>,
    pub service_address: Option<String>,
}
```

**✅ No changes needed!** The current structure already supports child conditions since `conditions` is a `Vec<Condition>`. We just need to populate it correctly in the reconciler.

### Task: Update Bind9ClusterStatus Structure

**File:** `src/crd.rs` (lines 1586-1608)

**Current Status Structure:**
```rust
pub struct Bind9ClusterStatus {
    pub conditions: Vec<Condition>,
    pub observed_generation: Option<i64>,
    pub instance_count: Option<i32>,
    pub ready_instances: Option<i32>,
    pub instances: Vec<String>,
}
```

**✅ No changes needed!** The current structure already supports child conditions.

### Verification
- [ ] Verify CRD status structures support multiple conditions
- [ ] Test that conditions vector can hold 10+ conditions without size issues
- [ ] Verify condition serialization to YAML is correct

## Phase 3: Reconciler Updates (TODO)

### Task 3.1: Update Bind9Instance Reconciler

**File:** `src/reconcilers/bind9instance.rs`

**Current Status Setting Location:** `update_status_from_deployment()` (lines 706-797)

**Changes Needed:**
1. **Add Pod-level condition tracking:**
   - Query Deployment for Pod status
   - Create one `Pod-{index}` condition per pod replica
   - Map pod conditions (Ready, PodScheduled, etc.) to our reasons
   - Track pod names in condition messages

2. **Add HTTP error code mapping:**
   - When calling Bindcar API, catch HTTP errors
   - Map status codes to condition reasons using new constants
   - Example: 404 → `REASON_ZONE_NOT_FOUND`

3. **Update overall Ready condition:**
   - Set `REASON_ALL_READY` when all pods ready
   - Set `REASON_PARTIALLY_READY` when some pods ready
   - Set `REASON_NOT_READY` when no pods ready
   - Set `REASON_BINDCAR_UNREACHABLE` when API calls fail

**Implementation Checklist:**
- [ ] Import status reasons: `use crate::status_reasons::*;`
- [ ] Query Pod list from Deployment
- [ ] Create pod-level conditions with `pod_condition_type(index)`
- [ ] Implement HTTP error code mapping function
- [ ] Update `update_status()` to accept multiple conditions
- [ ] Add integration with Bindcar API error handling
- [ ] Test with various failure scenarios

**Code Location:**
```rust
// src/reconcilers/bind9instance.rs

async fn update_status_from_deployment(
    client: &Client,
    namespace: &str,
    name: &str,
    instance: &Bind9Instance,
    expected_replicas: i32,
) -> Result<()> {
    // TODO: Add pod-level condition tracking here
    // 1. Get Deployment
    // 2. List Pods for this deployment
    // 3. Create Pod-{index} conditions
    // 4. Set overall Ready condition based on pod health
}
```

### Task 3.2: Update Bind9Cluster Reconciler

**File:** `src/reconcilers/bind9cluster.rs`

**Current Status Setting Location:** `update_status()` (lines 294-388)

**Changes Needed:**
1. **Add instance-level condition tracking:**
   - For each `Bind9Instance` in cluster, create `Bind9Instance-{index}` condition
   - Copy status from instance's Ready condition
   - Include instance replica counts in message

2. **Update overall Ready condition:**
   - Use `REASON_ALL_READY` when all instances ready
   - Use `REASON_PARTIALLY_READY` when some ready
   - Use `REASON_INSTANCES_CREATED` after creating new instances
   - Use `REASON_INSTANCES_SCALING` during scale operations

**Implementation Checklist:**
- [ ] Import status reasons: `use crate::status_reasons::*;`
- [ ] Track instance index in management labels
- [ ] Create instance-level conditions with `bind9_instance_condition_type(index)`
- [ ] Copy instance Ready condition to cluster child condition
- [ ] Update `calculate_cluster_status()` to return all conditions
- [ ] Update status patch to include child conditions
- [ ] Test with multiple instances in various states

**Code Location:**
```rust
// src/reconcilers/bind9cluster.rs

fn calculate_cluster_status(
    instances: &[Bind9Instance],
    namespace: &str,
    name: &str,
) -> (i32, i32, Vec<String>, &'static str, String) {
    // TODO: Return Vec<Condition> instead of just status/message
    // Include Bind9Instance-{index} conditions
}
```

### Task 3.3: Update Bind9GlobalCluster Reconciler

**File:** `src/reconcilers/bind9globalcluster.rs`

**Current Status Setting Location:** `calculate_cluster_status()` (lines 474-541)

**Changes Needed:**
1. **Decide on child tracking strategy:**
   - Option A: Track `Bind9Cluster-{index}` conditions (NOT Bind9Instance directly)
   - Option B: Don't track children (rely on Bind9Cluster level)
   - **Recommendation:** Option B - GlobalCluster just shows "2 of 3 clusters ready"

2. **Update overall Ready condition:**
   - Use `REASON_CLUSTERS_READY` when all clusters ready
   - Use `REASON_CLUSTERS_PROGRESSING` when some clusters ready

**Implementation Checklist:**
- [ ] Import status reasons: `use crate::status_reasons::*;`
- [ ] Update `calculate_cluster_status()` to use new reasons
- [ ] Decide if cluster-level child tracking is needed
- [ ] Update status patch logic if child conditions added

**Code Location:**
```rust
// src/reconcilers/bind9globalcluster.rs

pub fn calculate_cluster_status(
    instances: &[Bind9Instance],
    generation: Option<i64>,
) -> Bind9ClusterStatus {
    // TODO: Use REASON_CLUSTERS_READY, REASON_CLUSTERS_PROGRESSING
}
```

### Task 3.4: Add HTTP Error Mapping Utility

**File:** `src/bind9.rs` or create new `src/http_errors.rs`

**Changes Needed:**
1. Create helper function to map HTTP status codes to condition reasons
2. Add error context with specific failure reasons

**Implementation Checklist:**
- [ ] Create `map_http_error_to_reason()` function
- [ ] Handle all HTTP codes: 400, 401, 403, 404, 500, 501, 502, 503, 504
- [ ] Add connection error handling
- [ ] Return tuple of `(reason, message)`
- [ ] Add unit tests for all HTTP codes

**Example Implementation:**
```rust
// src/http_errors.rs

use crate::status_reasons::*;

pub fn map_http_error_to_reason(status: u16) -> (&'static str, String) {
    match status {
        400 => (REASON_BINDCAR_BAD_REQUEST, "Invalid request to Bindcar API (400)".into()),
        401 | 403 => (REASON_BINDCAR_AUTH_FAILED, "Bindcar authentication failed".into()),
        404 => (REASON_ZONE_NOT_FOUND, "Zone or resource not found in BIND9 (404)".into()),
        500 => (REASON_BINDCAR_INTERNAL_ERROR, "Bindcar API internal error (500)".into()),
        501 => (REASON_BINDCAR_NOT_IMPLEMENTED, "Operation not supported by Bindcar (501)".into()),
        502 | 503 | 504 => (REASON_GATEWAY_ERROR, format!("Gateway error reaching Bindcar ({})", status)),
        _ => (REASON_BINDCAR_UNREACHABLE, format!("Unexpected HTTP error from Bindcar ({})", status)),
    }
}

pub fn map_connection_error() -> (&'static str, String) {
    (REASON_BINDCAR_UNREACHABLE, "Cannot connect to Bindcar API".into())
}
```

## Phase 4: Testing (TODO)

### Task 4.1: Add Unit Tests

**Files to Update:**
- `src/reconcilers/bind9instance_tests.rs`
- `src/reconcilers/bind9cluster_tests.rs`
- `src/reconcilers/bind9globalcluster_tests.rs`
- `src/status_reasons_tests.rs` (already has basic tests)

**Test Cases Needed:**

#### Bind9Instance Tests
- [ ] Test pod-level condition creation
- [ ] Test all HTTP error code mappings
- [ ] Test encompassing condition uses `REASON_ALL_READY` when all pods ready
- [ ] Test encompassing condition uses `REASON_PARTIALLY_READY` when 1/2 pods ready
- [ ] Test encompassing condition uses `REASON_NOT_READY` when 0 pods ready
- [ ] Test child Pod-0 condition uses `REASON_READY` when pod is ready
- [ ] Test child Pod-1 condition uses specific failure reason (e.g., `REASON_BINDCAR_UNREACHABLE`)
- [ ] Test `REASON_PODS_PENDING` when pods not scheduled
- [ ] Test `REASON_BINDCAR_UNREACHABLE` on connection error
- [ ] Test condition message includes pod names

#### Bind9Cluster Tests
- [ ] Test instance-level condition creation
- [ ] Test encompassing condition uses `REASON_ALL_READY` when all instances ready
- [ ] Test encompassing condition uses `REASON_PARTIALLY_READY` when 2/3 instances ready
- [ ] Test child Bind9Instance-0 condition uses `REASON_READY` when instance is ready
- [ ] Test child Bind9Instance-2 condition uses `REASON_PARTIALLY_READY` when instance has partial pods
- [ ] Test `REASON_INSTANCES_CREATED` after creating instances
- [ ] Test `REASON_INSTANCES_SCALING` during scale up/down
- [ ] Test condition message includes instance names

#### Bind9GlobalCluster Tests
- [ ] Test `REASON_CLUSTERS_READY` when all clusters ready
- [ ] Test `REASON_CLUSTERS_PROGRESSING` when some clusters progressing
- [ ] Test condition message includes cluster names

### Task 4.2: Add Integration Tests

**File:** `tests/integration_test.rs` (create if doesn't exist)

**Test Scenarios:**
- [ ] Deploy cluster with 3 instances, verify 3 child conditions
- [ ] Scale cluster from 2 to 4 instances, verify conditions update
- [ ] Crash a pod, verify Pod-{index} condition shows failure
- [ ] Simulate Bindcar 404 error, verify `REASON_ZONE_NOT_FOUND`
- [ ] Simulate Bindcar 503 error, verify `REASON_GATEWAY_ERROR`

## Phase 5: Documentation (TODO)

### Task 5.1: Update API Documentation

**File:** `docs/src/reference/api.md`

**Changes Needed:**
- [ ] Regenerate with `cargo run --bin crddoc > docs/src/reference/api.md`
- [ ] Verify condition reason constants are documented
- [ ] Add examples of hierarchical status

### Task 5.2: Add Troubleshooting Guide

**File:** `docs/src/troubleshooting/status-conditions.md` (create new)

**Content Needed:**
- [ ] How to read hierarchical status
- [ ] Common condition reasons and solutions
- [ ] kubectl commands for inspecting status
- [ ] Examples of drilling down from cluster to pod
- [ ] HTTP error code troubleshooting table

### Task 5.3: Update Quickstart Guide

**File:** `docs/src/quickstart.md`

**Changes Needed:**
- [ ] Add section on checking status
- [ ] Show example of hierarchical conditions
- [ ] Add troubleshooting examples using conditions

### Task 5.4: Update README

**File:** `README.md`

**Changes Needed:**
- [ ] Mention enhanced status tracking in features
- [ ] Add example showing child conditions
- [ ] Link to troubleshooting guide

## Phase 6: Regenerate CRDs (TODO)

### Task 6.1: Regenerate CRD YAML Files

**Commands:**
```bash
cargo run --bin crdgen
cargo run --bin crddoc > docs/src/reference/api.md
```

**Files Updated:**
- [ ] `deploy/crds/bind9instances.crd.yaml`
- [ ] `deploy/crds/bind9clusters.crd.yaml`
- [ ] `deploy/crds/bind9globalclusters.crd.yaml`
- [ ] `docs/src/reference/api.md`

**Validation:**
- [ ] Validate CRD YAML: `kubectl apply --dry-run=client -f deploy/crds/`
- [ ] Check CRD size (must be under 1MB)
- [ ] Verify printcolumns still work
- [ ] Test applying CRDs to cluster

### Task 6.2: Update Examples

**Files to Update:**
- [ ] `examples/bind9-cluster.yaml`
- [ ] `examples/bind9-instance.yaml`
- [ ] `examples/bind9-globalcluster.yaml`

**Changes:**
- [ ] Add status examples showing child conditions
- [ ] Add comments explaining condition types
- [ ] Ensure examples validate

## Phase 7: Final Quality Checks (TODO)

### Code Quality
- [ ] Run `cargo fmt`
- [ ] Run `cargo clippy -- -D warnings`
- [ ] Run `cargo test`
- [ ] Run `cargo audit`

### Documentation Quality
- [ ] Build docs: `make docs`
- [ ] Verify no broken links
- [ ] Check all code examples compile
- [ ] Review examples validate with kubectl

### Performance Testing
- [ ] Test with 100 instances in one cluster
- [ ] Verify status update performance
- [ ] Check status subresource size limits
- [ ] Ensure no excessive API calls

### Manual Testing Scenarios
- [ ] Create cluster, verify child conditions appear
- [ ] Scale cluster, verify conditions update
- [ ] Kill pod, verify Pod-{index} condition changes
- [ ] Stop Bindcar, verify correct error reason
- [ ] Test all HTTP error codes return correct reasons

## Completion Checklist

Before marking this work as complete:

- [ ] All Phase 2 tasks completed
- [ ] All Phase 3 tasks completed
- [ ] All Phase 4 tests passing
- [ ] All Phase 5 documentation updated
- [ ] All Phase 6 CRDs regenerated
- [ ] All Phase 7 quality checks passed
- [ ] Code review completed
- [ ] User acceptance testing completed
- [ ] CHANGELOG.md updated with changes

## Related Documents

- [STATUS_CONDITIONS_DESIGN.md](STATUS_CONDITIONS_DESIGN.md) - Design specification
- [src/status_reasons.rs](src/status_reasons.rs) - Reason constants implementation
- [CLAUDE.md](../.claude/CLAUDE.md) - Project coding standards

## Notes

### HTTP Error Code Mapping Priority

The following HTTP codes MUST be handled with specific condition reasons:

1. **400 Bad Request** → `BindcarBadRequest` (controller bug)
2. **401 Unauthorized** → `BindcarAuthFailed` (credentials issue)
3. **403 Forbidden** → `BindcarAuthFailed` (permissions issue)
4. **404 Not Found** → `ZoneNotFound` (zone doesn't exist)
5. **500 Internal Server Error** → `BindcarInternalError` (Bindcar bug)
6. **501 Not Implemented** → `BindcarNotImplemented` (feature missing)
7. **502 Bad Gateway** → `GatewayError` (network/mesh issue)
8. **503 Service Unavailable** → `GatewayError` (Bindcar restarting)
9. **504 Gateway Timeout** → `GatewayError` (network latency)
10. **Connection Error** → `BindcarUnreachable` (pod not running)

### Condition Message Formats

Follow these message format conventions:

**Encompassing Condition (`type: Ready`):**
- **All Ready**: "All {count} {children} are ready"
- **Partially Ready**: "{ready}/{total} {children} are ready"
- **Not Ready**: "No {children} are ready"

**Child Conditions (`type: Bind9Instance-{index}`, `type: Pod-{index}`):**
- **Ready**: "{Child} {name} is ready ({details})"
- **Partially Ready**: "{Child} {name} is progressing ({details})"
- **Specific Failure**: "{Child} {name} {failure description}"

Examples:

**Encompassing conditions:**
- "All 3 instances are ready" (reason: `AllReady`)
- "2 of 3 instances are ready" (reason: `PartiallyReady`)
- "No instances are ready" (reason: `NotReady`)

**Child conditions:**
- "Instance production-dns-primary-0 is ready (2/2 pods)" (reason: `Ready`)
- "Instance production-dns-secondary-0 is progressing (1/2 pods)" (reason: `PartiallyReady`)
- "Pod production-dns-primary-0-7d9f8c6b5-abc12 is ready" (reason: `Ready`)
- "Pod production-dns-primary-0-7d9f8c6b5-xyz34 cannot connect to Bindcar API" (reason: `BindcarUnreachable`)

### Index Tracking

Child conditions use zero-based indexing:
- First instance: `Bind9Instance-0`
- First pod: `Pod-0`

The index should come from the `BINDY_INSTANCE_INDEX_ANNOTATION` annotation on managed instances.
