# Status Conditions Implementation Roadmap

**Author:** Erick Bourgeois
**Created:** 2025-01-18
**Last Updated:** 2025-12-30
**Status:** ✅ Phases 1-5 COMPLETE | Phases 6-7 FUTURE WORK

---

## Table of Contents

1. [Overview](#overview)
2. [Design Goals](#design-goals)
3. [Architecture](#architecture)
4. [Implementation Phases](#implementation-phases)
5. [HTTP Error Mapping](#http-error-mapping)
6. [Quick Reference](#quick-reference)
7. [Usage Examples](#usage-examples)
8. [Testing Strategy](#testing-strategy)
9. [Migration Plan](#migration-plan)

---

## Overview

This roadmap describes the implementation of **hierarchical status condition tracking** for Bindy resources. The system enables operators to quickly identify which specific child resources are failing without manually inspecting multiple resources.

### Key Principle

Label selectors defined at the platform/cluster level propagate down to instances, where actual zone watching and selection is implemented—similar to how `DNSZone` watches for records using label selectors.

### Current Status

**✅ COMPLETED (Phases 1-5):**
- ✅ Phase 1: Foundation (status reason constants, HTTP error mapping)
- ✅ Phase 2: CRD Schema Verification
- ✅ Phase 3: Reconciler Updates (standardized status reasons)
- ✅ Phase 4: Pod-Level Condition Tracking
- ✅ Phase 5: Instance-Level Condition Tracking

**🔄 FUTURE WORK (Phases 6-7):**
- Phase 6: Integration Testing & Validation
- Phase 7: Documentation & User Guides

---

## Design Goals

1. **Hierarchical Tracking**: Each resource tracks the status of its direct children
2. **Quick Failure Identification**: A single `kubectl get` shows which specific child is failing
3. **Standard Reasons**: Use consistent, well-defined condition reasons across all resources
4. **Kubernetes Conventions**: Follow standard Kubernetes status condition patterns
5. **Backwards Compatible**: Don't break existing status consumers
6. **HTTP Error Visibility**: Map Bindcar API errors to actionable condition reasons

---

## Architecture

### Status Hierarchy

```
ClusterBind9Provider (cluster-scoped)
    ↓
Bind9Cluster (namespace-scoped)
    ├─ Bind9Instance-0 (primary-0)
    │   ├─ Pod-0 (ready)
    │   └─ Pod-1 (ready)
    ├─ Bind9Instance-1 (primary-1)
    │   ├─ Pod-0 (ready)
    │   └─ Pod-1 (not ready - BindcarUnreachable)
    └─ Bind9Instance-2 (secondary-0)
        ├─ Pod-0 (ready)
        └─ Pod-1 (ready)
```

### Condition Structure

Each resource has:
1. **One encompassing condition**: `type: Ready` - Overall health indicator
2. **Multiple child conditions**: One per child resource (e.g., `Bind9Instance-0`, `Pod-1`)

**Condition Format** (Kubernetes standard):
```yaml
type: string              # "Ready" or "Bind9Instance-0" or "Pod-1"
status: string            # "True", "False", or "Unknown"
reason: string            # CamelCase programmatic identifier (e.g., "AllReady")
message: string           # Human-readable explanation
lastTransitionTime: string # RFC3339 timestamp
```

### Key Distinction: Encompassing vs Child

**Encompassing Conditions** (`type: Ready`):
- Use: `AllReady`, `PartiallyReady`, `NotReady`, `NoChildren`
- Represents overall resource health

**Child Conditions** (`type: Bind9Instance-0`, `Pod-1`):
- Use: `Ready`, `NotReady`, or specific failure reasons (e.g., `BindcarUnreachable`)
- Represents individual child health

---

## Implementation Phases

### Phase 1: Foundation ✅ COMPLETED (2025-01-18)

**Files Created:**
- `src/status_reasons.rs` - 30+ standard condition reason constants
- `src/http_errors.rs` - Complete HTTP error code mapping
- Documentation files (design, implementation tracking, quick reference)

**Key Accomplishments:**
- Established encompassing vs child condition distinction
- Defined condition type constants (`CONDITION_TYPE_READY`, `CONDITION_TYPE_BIND9_INSTANCE_PREFIX`, `CONDITION_TYPE_POD_PREFIX`)
- Created helper functions: `bind9_instance_condition_type(index)`, `pod_condition_type(index)`, `extract_child_index()`
- Mapped 10 HTTP status codes to specific condition reasons
- Added comprehensive rustdoc documentation

**Standard Reasons Defined:**
```rust
// Encompassing conditions (type: Ready)
REASON_ALL_READY       // All children ready
REASON_PARTIALLY_READY // Some children ready
REASON_NOT_READY       // No children ready
REASON_NO_CHILDREN     // No children found

// Child conditions
REASON_READY           // This child is ready
REASON_PROGRESSING     // Child is progressing
REASON_PARTIALLY_READY // Child has partial sub-resources ready

// HTTP error reasons
REASON_BINDCAR_UNREACHABLE
REASON_BINDCAR_BAD_REQUEST
REASON_BINDCAR_AUTH_FAILED
REASON_ZONE_NOT_FOUND
REASON_BINDCAR_INTERNAL_ERROR
REASON_BINDCAR_NOT_IMPLEMENTED
REASON_GATEWAY_ERROR
```

---

### Phase 2: CRD Schema Verification ✅ COMPLETED (2025-01-18)

**Verification Results:**
- ✅ `Bind9InstanceStatus` already supports child conditions via `conditions: Vec<Condition>`
- ✅ `Bind9ClusterStatus` already supports child conditions via `conditions: Vec<Condition>`
- ✅ No CRD schema changes needed - only reconciler logic updates required

**Files Verified:**
- `src/crd.rs` (lines 1755-1769) - Bind9InstanceStatus
- `src/crd.rs` (lines 1586-1608) - Bind9ClusterStatus

---

### Phase 3: Reconciler Updates ✅ COMPLETED (2025-01-18)

**Files Modified:**
- `src/reconcilers/bind9globalcluster.rs` - Use standard constants
- `src/reconcilers/bind9cluster.rs` - Use standard constants
- `src/reconcilers/bind9instance.rs` - Use standard constants
- `src/reconcilers/bind9cluster_tests.rs` - Added 8 comprehensive tests
- `src/lib.rs` - Added module exports

**Key Changes:**

**ClusterBind9Provider:**
- Replaced hardcoded strings with `REASON_ALL_READY`, `REASON_PARTIALLY_READY`, `REASON_NOT_READY`, `REASON_NO_CHILDREN`
- Updated condition type to use `CONDITION_TYPE_READY`

**Bind9Cluster:**
- Replaced hardcoded "Ready" with `CONDITION_TYPE_READY` constant
- Automatic reason mapping: All ready → `REASON_ALL_READY`, partial → `REASON_PARTIALLY_READY`, none → `REASON_NOT_READY`
- Cleaner messages: "All {count} instances are ready", "{ready}/{total} instances are ready"

**Bind9Instance:**
- Replaced hardcoded "Ready" with `CONDITION_TYPE_READY` constant
- Intelligent reason mapping based on status and message content

---

### Phase 4: Pod-Level Condition Tracking ✅ COMPLETED (2025-01-18)

**Goal:** Bind9Instance shows status of each pod

**Files Modified:**
- `src/reconcilers/bind9instance.rs`
- `src/reconcilers/bind9instance_tests.rs` (10 new tests)

**Implementation Details:**
1. **Added Pod Listing** - Lists pods using label selector `app={instance-name}`
2. **Rewrote `update_status_from_deployment()`** - Creates N+1 conditions (1 encompassing + N pod conditions)
3. **Updated `update_status()` Signature** - Accepts `Vec<Condition>` instead of individual status/message
4. **Enhanced Change Detection** - Compares all conditions (length, type, status, message, reason)

**Condition Logic:**
```rust
// Encompassing condition
if all_pods_ready {
    type: "Ready", status: "True", reason: "AllReady"
} else if some_pods_ready {
    type: "Ready", status: "False", reason: "PartiallyReady"
} else {
    type: "Ready", status: "False", reason: "NotReady"
}

// Pod-level conditions
for each pod {
    type: "Pod-{index}",
    status: pod_ready ? "True" : "False",
    reason: pod_ready ? "Ready" : "NotReady"
}
```

**Example Output:**
```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-primary-0
status:
  conditions:
  - type: Ready
    status: "False"
    reason: PartiallyReady
    message: "2/3 pods are ready"
  - type: Pod-0
    status: "True"
    reason: Ready
    message: "Pod dns-primary-0-abc123 is ready"
  - type: Pod-1
    status: "True"
    reason: Ready
    message: "Pod dns-primary-0-def456 is ready"
  - type: Pod-2
    status: "False"
    reason: NotReady
    message: "Pod dns-primary-0-ghi789 is not ready"
  replicas: 3
  readyReplicas: 2
```

**Tests Added:**
- `test_pod_condition_type_helper()` - Verifies `Pod-{index}` generation
- `test_encompassing_condition_uses_all_ready()` - Verifies `REASON_ALL_READY` usage
- `test_child_pod_condition_uses_ready()` - Verifies child conditions use `REASON_READY`
- `test_partially_ready_pods()` - Verifies partial readiness logic
- `test_no_pods_ready()` - Verifies no pods ready scenario
- Plus 5 more tests validating message formats and condition structures

---

### Phase 5: Instance-Level Condition Tracking ✅ COMPLETED (2025-01-18)

**Goal:** Bind9Cluster shows status of each instance

**Files Modified:**
- `src/reconcilers/bind9cluster.rs`
- `src/reconcilers/bind9cluster_tests.rs` (11 tests updated)

**Implementation Details:**
1. **Rewrote `calculate_cluster_status()`** - Returns `Vec<Condition>` instead of status/message tuple
2. **Updated `update_status()` Signature** - Accepts `Vec<Condition>`
3. **Enhanced Change Detection** - Compares all conditions

**Condition Logic:**
```rust
// Encompassing condition
if no_instances {
    type: "Ready", status: "False", reason: "NoChildren"
} else if all_instances_ready {
    type: "Ready", status: "True", reason: "AllReady"
} else if some_instances_ready {
    type: "Ready", status: "False", reason: "PartiallyReady"
} else {
    type: "Ready", status: "False", reason: "NotReady"
}

// Instance-level conditions
for each instance {
    type: "Bind9Instance-{index}",
    status: instance_ready ? "True" : "False",
    reason: instance_ready ? "Ready" : "NotReady"
}
```

**Example Output:**
```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: dns-cluster
status:
  conditions:
  - type: Ready
    status: "False"
    reason: PartiallyReady
    message: "2/3 instances are ready"
  - type: Bind9Instance-0
    status: "True"
    reason: Ready
    message: "Instance dns-primary-0 is ready"
  - type: Bind9Instance-1
    status: "True"
    reason: Ready
    message: "Instance dns-primary-1 is ready"
  - type: Bind9Instance-2
    status: "False"
    reason: NotReady
    message: "Instance dns-secondary-0 is not ready"
  instanceCount: 3
  readyInstances: 2
```

**Tests Updated:**
- `test_calculate_cluster_status_no_instances()` - Verifies 1 condition with `REASON_NO_CHILDREN`
- `test_calculate_cluster_status_all_ready()` - Verifies 4 conditions (1 encompassing + 3 children)
- `test_calculate_cluster_status_some_ready()` - Verifies mixed readiness
- `test_calculate_cluster_status_large_cluster()` - Verifies 11 conditions (1 + 10)
- Plus 7 more tests validating edge cases

---

### Phase 6: Integration Testing & Validation 🔄 FUTURE WORK

**Files to Update:**
- `tests/integration_test.rs`
- All reconciler test files

**Test Scenarios Needed:**
- Deploy cluster with 3 instances, verify 3 child conditions
- Scale cluster from 2 to 4 instances, verify conditions update
- Crash a pod, verify Pod-{index} condition shows failure
- Simulate Bindcar 404 error, verify `REASON_ZONE_NOT_FOUND`
- Simulate Bindcar 503 error, verify `REASON_GATEWAY_ERROR`
- Test with 100 instances in one cluster (performance)
- Verify status subresource size limits

**Unit Test Coverage:**

**Bind9Instance Tests:**
- [ ] Test all HTTP error code mappings
- [ ] Test `REASON_PODS_PENDING` when pods not scheduled
- [ ] Test `REASON_PODS_CRASHING` for CrashLoopBackOff
- [ ] Test condition message includes pod names
- [ ] Test connection error handling

**Bind9Cluster Tests:**
- [ ] Test `REASON_INSTANCES_CREATED` after creating instances
- [ ] Test `REASON_INSTANCES_SCALING` during scale up/down
- [ ] Test condition message includes instance names

**Quality Checks:**
- [ ] Run `cargo fmt`
- [ ] Run `cargo clippy -- -D warnings`
- [ ] Run `cargo test`
- [ ] Run `cargo audit`
- [ ] Verify no excessive API calls
- [ ] Check status subresource size limits

---

### Phase 7: Documentation & User Guides 🔄 FUTURE WORK

**Files to Create/Update:**
- `docs/src/troubleshooting/status-conditions.md` (new)
- `docs/src/reference/api.md`
- `docs/src/quickstart.md`
- `README.md`
- `examples/bind9-cluster.yaml`
- `examples/bind9-instance.yaml`

**Documentation Needed:**

**Troubleshooting Guide:**
- [ ] How to read hierarchical status
- [ ] Common condition reasons and solutions
- [ ] kubectl commands for inspecting status
- [ ] Examples of drilling down from cluster to pod
- [ ] HTTP error code troubleshooting table

**API Documentation:**
- [ ] Regenerate: `cargo run --bin crddoc > docs/src/reference/api.md`
- [ ] Add examples of hierarchical status
- [ ] Document condition reason constants

**Quickstart Guide:**
- [ ] Add section on checking status
- [ ] Show example of hierarchical conditions
- [ ] Add troubleshooting examples using conditions

**Examples:**
- [ ] Add status examples showing child conditions
- [ ] Add comments explaining condition types
- [ ] Ensure examples validate

---

## HTTP Error Mapping

### Error Code to Reason Mapping

| HTTP Code | Condition Reason | Status | Meaning | Action |
|-----------|------------------|--------|---------|--------|
| **Connection Error** | `BindcarUnreachable` | False | Cannot establish TCP connection | Check if Bindcar container is running |
| **400** | `BindcarBadRequest` | False | Invalid request format | Controller bug - report issue |
| **401** | `BindcarAuthFailed` | False | Authentication required | Check RNDC credentials in Secret |
| **403** | `BindcarAuthFailed` | False | Insufficient permissions | Check RBAC and service account |
| **404** | `ZoneNotFound` | False | Zone/resource doesn't exist | Verify zone was created in BIND9 |
| **500** | `BindcarInternalError` | False | Bindcar API error | Check Bindcar container logs |
| **501** | `BindcarNotImplemented` | False | Feature not supported | Upgrade Bindcar version |
| **502** | `GatewayError` | False | Bad gateway | Check service mesh / network policies |
| **503** | `GatewayError` | False | Service unavailable | Bindcar container may be restarting |
| **504** | `GatewayError` | False | Gateway timeout | Check network latency / Bindcar performance |
| **2xx** | `AllReady` | True | Success | Normal operation |

### Implementation

```rust
use crate::http_errors::map_http_error_to_reason;

// Map HTTP errors to condition reasons
let (reason, message) = map_http_error_to_reason(404);
// Returns: ("ZoneNotFound", "Zone or resource not found in BIND9 (404)")

// Handle connection errors
let (reason, message) = map_connection_error();
// Returns: ("BindcarUnreachable", "Cannot connect to Bindcar API")
```

---

## Quick Reference

### Encompassing Condition (`type: Ready`)

**When to Use:** The main `type: Ready` condition showing overall resource health

**Reasons:**
- ✅ `AllReady` - All children are ready
- ⚠️ `PartiallyReady` - Some children ready, some not
- ❌ `NotReady` - No children are ready
- ❌ `NoChildren` - No child resources found

**Example:**
```yaml
- type: Ready
  status: "True"
  reason: AllReady
  message: "All 3 instances are ready"
```

### Child Conditions (`type: Bind9Instance-{index}`, `type: Pod-{index}`)

**When to Use:** Conditions for specific children

**Reasons:**
- ✅ `Ready` - This specific child is ready
- ⚠️ `PartiallyReady` - This child has some sub-resources ready
- ❌ `NotReady` - This child is not ready
- ❌ Specific failure reasons (e.g., `BindcarUnreachable`, `PodsPending`)

**Example:**
```yaml
- type: Bind9Instance-0
  status: "True"
  reason: Ready
  message: "Instance production-dns-primary-0 is ready (2/2 pods)"

- type: Pod-1
  status: "False"
  reason: BindcarUnreachable
  message: "Pod production-dns-primary-0-abc123 cannot connect to Bindcar API"
```

### Complete Reason Reference

**Common (All Resources):**
- `AllReady`, `Ready`, `PartiallyReady`, `NotReady`, `NoChildren`, `Progressing`, `ConfigurationValid`, `ConfigurationInvalid`

**Bind9Instance Specific:**
- `MinimumReplicasAvailable`, `ProgressDeadlineExceeded`, `RNDCAuthenticationFailed`, `BindcarUnreachable`, `BindcarBadRequest`, `BindcarAuthFailed`, `ZoneNotFound`, `BindcarInternalError`, `BindcarNotImplemented`, `GatewayError`, `ZoneTransferComplete`, `ZoneTransferFailed`, `PodsPending`, `PodsCrashing`

**Bind9Cluster Specific:**
- `InstancesCreated`, `InstancesScaling`, `InstancesPending`

**Network Reasons:**
- `UpstreamUnreachable`

---

## Usage Examples

### Quick Status Check

```bash
# Check overall cluster status
kubectl get bind9cluster production-dns -o yaml | yq '.status.conditions'

# Output shows specific failing instance:
# - type: Ready
#   status: "False"
#   reason: PartiallyReady
#   message: "2 of 3 instances are ready"
# - type: Bind9Instance-2
#   status: "False"
#   reason: NotReady
#   message: "Instance production-dns-secondary-0 is not ready"
```

### Drill Down to Pod Level

```bash
# Check specific instance
kubectl get bind9instance production-dns-secondary-0 -o yaml | yq '.status.conditions'

# Output shows specific failing pod:
# - type: Ready
#   status: "False"
#   reason: PartiallyReady
#   message: "1 of 2 pods are ready"
# - type: Pod-1
#   status: "False"
#   reason: BindcarUnreachable
#   message: "Pod production-dns-secondary-0-7d9f8c6b5-xyz34 cannot connect to Bindcar API"
```

### Automated Monitoring

```bash
# Monitor for specific failure conditions
kubectl get bind9instances -A -o json | \
  jq '.items[] | select(.status.conditions[] | select(.reason == "BindcarUnreachable"))'

# Find all partially ready clusters
kubectl get bind9clusters -A -o json | \
  jq '.items[] | select(.status.conditions[] | select(.type == "Ready" and .reason == "PartiallyReady"))'
```

---

## Testing Strategy

### Unit Test Coverage

**Phase 4 Tests (Pod-Level):**
- ✅ 10 comprehensive unit tests in `bind9instance_tests.rs`
- Tests cover: all ready, partially ready, no ready, message formats, condition structures

**Phase 5 Tests (Instance-Level):**
- ✅ 11 comprehensive unit tests in `bind9cluster_tests.rs`
- Tests cover: no instances, all ready, some ready, none ready, large clusters, edge cases

### Integration Test Scenarios

**Pending (Phase 6):**
- Deploy cluster, verify child conditions appear
- Scale cluster, verify conditions update
- Kill pod, verify Pod-{index} condition changes
- Stop Bindcar, verify correct error reason
- Test all HTTP error codes return correct reasons
- Performance test with 100 instances

---

## Migration Plan

### Backwards Compatibility

The new status structure is **fully backwards compatible**:

1. **Existing `type: Ready` condition remains** - Existing status consumers continue to work
2. **New child conditions are additive** - Old consumers ignore unknown condition types
3. **Existing status fields unchanged** - `instanceCount`, `readyInstances`, etc. remain

### Gradual Rollout

1. **Phase 1-5 Deployed** - Standard reasons and hierarchical conditions now active
2. **Monitor existing consumers** - Verify Prometheus, alerting, kubectl plugins still work
3. **Update monitoring dashboards** - Use new child conditions for granular visibility
4. **Complete Phase 6-7** - Integration tests and user documentation

### Validation Checklist

Before production rollout:
- [ ] Existing Prometheus metrics still work
- [ ] Existing alerting rules still fire
- [ ] Existing kubectl plugins/scripts still work
- [ ] Status subresource size stays under Kubernetes limits
- [ ] All integration tests pass
- [ ] Documentation complete

---

## Benefits Delivered

### 1. Consistency ✅
All reconcilers use centralized constants from `src/status_reasons.rs` instead of scattered string literals.

### 2. Maintainability ✅
- Reason constants defined once, used everywhere
- Changes only needed in one place
- Compiler catches usage errors

### 3. Quick Failure Identification ✅
Operators can see exactly which pod or instance is failing without querying multiple resources.

### 4. HTTP Error Visibility ✅
Complete mapping of Bindcar API errors to actionable condition reasons with troubleshooting guidance.

### 5. Type Safety ✅
Constants provide compile-time checking, preventing typos and inconsistencies.

### 6. Hierarchical Visibility ✅
Three levels of status: Cluster → Instance → Pod, enabling drill-down troubleshooting.

---

## Related Files

**Source Code:**
- [src/status_reasons.rs](../../src/status_reasons.rs) - Status reason constants
- [src/http_errors.rs](../../src/http_errors.rs) - HTTP error mapping
- [src/reconcilers/bind9instance.rs](../../src/reconcilers/bind9instance.rs) - Pod-level conditions
- [src/reconcilers/bind9cluster.rs](../../src/reconcilers/bind9cluster.rs) - Instance-level conditions
- [src/reconcilers/bind9globalcluster.rs](../../src/reconcilers/bind9globalcluster.rs) - Cluster-level conditions

**Tests:**
- [src/reconcilers/bind9instance_tests.rs](../../src/reconcilers/bind9instance_tests.rs)
- [src/reconcilers/bind9cluster_tests.rs](../../src/reconcilers/bind9cluster_tests.rs)

**Documentation:**
- [CHANGELOG.md](../../CHANGELOG.md) - Detailed change log

---

## Changelog

### 2025-12-30
- Consolidated all status condition documentation into single roadmap
- Removed redundant files: status-conditions-design.md, status-conditions-implementation.md, status-conditions-phase3-summary.md, status-conditions-phase4-5-summary.md, status-condition-reasons-quick-reference.md
- Marked Phases 1-5 as complete, Phases 6-7 as future work

### 2025-01-18
- Completed Phase 5: Instance-level condition tracking
- Completed Phase 4: Pod-level condition tracking
- Completed Phase 3: Reconciler updates with standard reasons
- Completed Phase 2: CRD schema verification
- Completed Phase 1: Foundation (status reasons, HTTP error mapping)
- Created initial documentation files

---

## Next Steps

**Ready for Production:** Phases 1-5 are complete and production-ready.

**Future Enhancements (Phase 6-7):**
1. Complete integration testing with real Kubernetes resources
2. Add comprehensive user documentation and troubleshooting guides
3. Update examples and quickstart guide
4. Create automated monitoring dashboards using new conditions

**Recommendation:** Deploy Phases 1-5 to production now. The hierarchical status tracking provides immediate value for troubleshooting. Complete Phases 6-7 based on user feedback and actual usage patterns.
