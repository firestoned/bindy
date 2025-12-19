# Enhanced Status Conditions Design

**Author:** Erick Bourgeois
**Date:** 2025-01-18
**Status:** Implementation In Progress

## Overview

This document describes the enhanced hierarchical status condition tracking system for Bindy resources. The design enables users to quickly identify which specific child resources are failing without needing to inspect multiple resources manually.

## Motivation

The current status implementation only tracks overall readiness at each resource level. When a resource reports `Ready: False`, users must manually inspect child resources to identify the specific component causing the failure. This is time-consuming and doesn't scale well in large deployments.

## Design Goals

1. **Hierarchical Tracking**: Each resource tracks the status of its direct children
2. **Quick Failure Identification**: A single `kubectl get` shows which specific child is failing
3. **Standard Reasons**: Use consistent, well-defined condition reasons across all resources
4. **Kubernetes Conventions**: Follow standard Kubernetes status condition patterns
5. **Backwards Compatible**: Don't break existing status consumers

## Status Hierarchy

```
Bind9GlobalCluster (cluster-scoped)
    ├─ Bind9Cluster (namespace: dns-system)
    │   ├─ Bind9Instance-0 (primary-0)
    │   │   ├─ Pod-0
    │   │   └─ Pod-1
    │   ├─ Bind9Instance-1 (primary-1)
    │   │   ├─ Pod-0
    │   │   └─ Pod-1
    │   └─ Bind9Instance-2 (secondary-0)
    │       ├─ Pod-0
    │       └─ Pod-1
    └─ Bind9Cluster (namespace: production)
        └─ ... (instances and pods)
```

## Condition Structure

### Condition Types

Each resource has:
1. **One encompassing condition**: `type: Ready` - Overall health indicator
2. **Multiple child conditions**: One per child resource (e.g., `Bind9Instance-0`, `Pod-1`)

### Condition Format

Following Kubernetes conventions, each condition has:

```yaml
type: string              # "Ready" or "Bind9Instance-0" or "Pod-1"
status: string            # "True", "False", or "Unknown"
reason: string            # CamelCase programmatic identifier (e.g., "AllReady")
message: string           # Human-readable explanation
lastTransitionTime: string # RFC3339 timestamp
```

## Resource-Specific Definitions

### Bind9GlobalCluster

**Definition of Ready**: When all referenced `Bind9Cluster` resources are Ready.

**Conditions**:
- `type: Ready` - Overall cluster health
- No child conditions (delegates to Bind9Cluster level for details)

**Example Status**:
```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: AllReady
      message: "All 2 clusters are ready"
      lastTransitionTime: "2025-01-18T10:30:00Z"
  observedGeneration: 5
  instanceCount: 2
  readyInstances: 2
  instances:
    - "dns-system/production-dns"
    - "production/production-dns"
```

**Reasons**:
- `AllReady` - All clusters are ready
- `PartiallyReady` - Some clusters ready, some not
- `NotReady` - No clusters are ready
- `NoChildren` - No clusters found
- `ClustersProgressing` - Clusters are being created/updated

### Bind9Cluster

**Definition of Ready**: When all `Bind9Instance` resources are Ready.

**Conditions**:
- `type: Ready` - Overall cluster health
- `type: Bind9Instance-{index}` - Per-instance health (one for each instance)

**Example Status**:
```yaml
status:
  conditions:
    - type: Ready
      status: "False"
      reason: PartiallyReady
      message: "2 of 3 instances are ready"
      lastTransitionTime: "2025-01-18T10:30:00Z"

    - type: Bind9Instance-0
      status: "True"
      reason: Ready
      message: "Instance production-dns-primary-0 is ready (2/2 pods)"
      lastTransitionTime: "2025-01-18T10:28:00Z"

    - type: Bind9Instance-1
      status: "True"
      reason: Ready
      message: "Instance production-dns-primary-1 is ready (2/2 pods)"
      lastTransitionTime: "2025-01-18T10:29:00Z"

    - type: Bind9Instance-2
      status: "False"
      reason: PartiallyReady
      message: "Instance production-dns-secondary-0 is progressing (1/2 pods)"
      lastTransitionTime: "2025-01-18T10:30:00Z"

  observedGeneration: 12
  instanceCount: 3
  readyInstances: 2
  instances:
    - "production-dns-primary-0"
    - "production-dns-primary-1"
    - "production-dns-secondary-0"
```

**Reasons**:
- `AllReady` - All instances are ready
- `PartiallyReady` - Some instances ready, some not
- `NotReady` - No instances are ready
- `NoChildren` - No instances found
- `InstancesCreated` - All instances created successfully
- `InstancesScaling` - Scaling instances up or down
- `InstancesPending` - Waiting for instances to be created

### Bind9Instance

**Definition of Ready**: When all desired replicas (pods) are up and running.

**Conditions**:
- `type: Ready` - Overall instance health
- `type: Pod-{index}` - Per-pod health (one for each pod replica)

**Example Status**:
```yaml
status:
  conditions:
    - type: Ready
      status: "False"
      reason: PartiallyReady
      message: "1 of 2 pods are ready"
      lastTransitionTime: "2025-01-18T10:30:00Z"

    - type: Pod-0
      status: "True"
      reason: Ready
      message: "Pod production-dns-primary-0-7d9f8c6b5-abc12 is ready"
      lastTransitionTime: "2025-01-18T10:28:00Z"

    - type: Pod-1
      status: "False"
      reason: BindcarUnreachable
      message: "Pod production-dns-primary-0-7d9f8c6b5-xyz34 cannot connect to Bindcar API"
      lastTransitionTime: "2025-01-18T10:30:00Z"

  observedGeneration: 8
  replicas: 2
  readyReplicas: 1
  serviceAddress: "10.96.0.10"
```

**Reasons**:
- `AllReady` - All pods are ready
- `PartiallyReady` - Some pods ready, some not
- `NotReady` - No pods are ready
- `MinimumReplicasAvailable` - Minimum replicas met but not all
- `ProgressDeadlineExceeded` - Deployment failed to progress
- `RNDCAuthenticationFailed` - RNDC authentication failed
- `BindcarUnreachable` - Cannot connect to Bindcar API
- `ZoneTransferComplete` - Secondary completed zone transfer
- `ZoneTransferFailed` - Zone transfer failed
- `PodsPending` - Pods waiting to be scheduled
- `PodsCrashing` - Pods are in CrashLoopBackOff
- `UpstreamUnreachable` - Cannot reach primary servers (for secondaries)

## HTTP Error Code Mapping

When interacting with the Bindcar API, HTTP status codes are mapped to specific condition reasons:

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

### Implementation Example

```rust
use reqwest::StatusCode;
use crate::status_reasons::*;

fn map_http_error_to_reason(status: StatusCode) -> (&'static str, &'static str) {
    match status {
        StatusCode::BAD_REQUEST => (REASON_BINDCAR_BAD_REQUEST, "Invalid request to Bindcar API"),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => (REASON_BINDCAR_AUTH_FAILED, "Bindcar authentication failed"),
        StatusCode::NOT_FOUND => (REASON_ZONE_NOT_FOUND, "Zone or resource not found in BIND9"),
        StatusCode::INTERNAL_SERVER_ERROR => (REASON_BINDCAR_INTERNAL_ERROR, "Bindcar API internal error"),
        StatusCode::NOT_IMPLEMENTED => (REASON_BINDCAR_NOT_IMPLEMENTED, "Operation not supported by Bindcar"),
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT =>
            (REASON_GATEWAY_ERROR, "Gateway error reaching Bindcar"),
        _ => (REASON_BINDCAR_UNREACHABLE, "Unexpected HTTP error from Bindcar"),
    }
}
```

## Standard Condition Reasons

See [src/status_reasons.rs](src/status_reasons.rs) for the complete list of standard condition reasons and their definitions.

### Common Reasons (All Resources)

| Reason | Usage | Meaning |
|--------|-------|---------|
| `AllReady` | Encompassing `type: Ready` condition | All child resources are ready |
| `Ready` | Child conditions (e.g., `Bind9Instance-0`, `Pod-1`) | This specific child is ready |
| `PartiallyReady` | Encompassing or child conditions | Some but not all sub-resources are ready |
| `NotReady` | Encompassing or child conditions | No resources are ready |
| `NoChildren` | Encompassing condition | No child resources found |
| `Progressing` | Encompassing or child conditions | Resources being created/updated |
| `ConfigurationValid` | Any condition | Configuration validated successfully |
| `ConfigurationInvalid` | Any condition | Configuration validation failed |

**Key Distinction:**
- **Encompassing condition** (`type: Ready`): Use `AllReady`, `PartiallyReady`, `NotReady`
- **Child conditions** (`type: Bind9Instance-0`, `type: Pod-1`): Use `Ready`, `PartiallyReady`, `NotReady`, or specific failure reasons

### Instance-Specific Reasons

| Reason | Resource | Meaning | HTTP Code |
|--------|----------|---------|-----------|
| `MinimumReplicasAvailable` | Bind9Instance | Minimum replicas available but not all | N/A |
| `ProgressDeadlineExceeded` | Bind9Instance | Deployment failed to progress | N/A |
| `RNDCAuthenticationFailed` | Bind9Instance | RNDC authentication failed | N/A |
| `BindcarUnreachable` | Bind9Instance | Cannot connect to Bindcar API | Connection Error |
| `BindcarBadRequest` | Bind9Instance | Invalid request sent to Bindcar | 400 |
| `BindcarAuthFailed` | Bind9Instance | Bindcar authentication/authorization failed | 401, 403 |
| `ZoneNotFound` | Bind9Instance | Zone or resource not found in BIND9 | 404 |
| `BindcarInternalError` | Bind9Instance | Bindcar API internal error | 500 |
| `BindcarNotImplemented` | Bind9Instance | Bindcar API feature not implemented | 501 |
| `GatewayError` | Bind9Instance | Gateway cannot reach Bindcar pod | 502, 503, 504 |
| `ZoneTransferComplete` | Bind9Instance | Zone transfer completed successfully | N/A |
| `ZoneTransferFailed` | Bind9Instance | Zone transfer failed | N/A |
| `PodsPending` | Bind9Instance | Pods waiting to be scheduled | N/A |
| `PodsCrashing` | Bind9Instance | Pods in CrashLoopBackOff | N/A |

### Cluster-Specific Reasons

| Reason | Resource | Meaning |
|--------|----------|---------|
| `InstancesCreated` | Bind9Cluster | All instances created |
| `InstancesScaling` | Bind9Cluster | Scaling instances |
| `InstancesPending` | Bind9Cluster | Waiting for instances |

### Network Reasons

| Reason | Resource | Meaning |
|--------|----------|---------|
| `UpstreamUnreachable` | Any | Cannot reach upstream services |

## Implementation Plan

### Phase 1: Foundation (COMPLETED)
- [x] Create `status_reasons.rs` module with standard reasons
- [x] Define helper functions for child condition types
- [x] Add comprehensive documentation

### Phase 2: CRD Updates (IN PROGRESS)
- [ ] Update `Bind9InstanceStatus` to support pod-level conditions
- [ ] Update `Bind9ClusterStatus` to support instance-level conditions
- [ ] Regenerate CRD YAML files

### Phase 3: Reconciler Updates
- [ ] Update `bind9instance` reconciler to track pod status
- [ ] Update `bind9cluster` reconciler to track instance status
- [ ] Update `bind9globalcluster` reconciler to track cluster status
- [ ] Use standard reasons from `status_reasons.rs`

### Phase 4: Testing
- [ ] Add unit tests for new status structures
- [ ] Add integration tests for hierarchical status
- [ ] Update existing tests for new condition reasons

### Phase 5: Documentation
- [ ] Update API documentation with new status examples
- [ ] Add troubleshooting guide using condition reasons
- [ ] Update quickstart with status inspection examples

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
#   reason: PartiallyReady
#   message: "Instance production-dns-secondary-0 is progressing (1/2 pods)"
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
```

## Migration Plan

### Backwards Compatibility

The new status structure is **fully backwards compatible**:

1. **Existing `type: Ready` condition remains** - Existing status consumers will continue to work
2. **New child conditions are additive** - Old consumers ignore unknown condition types
3. **Existing status fields unchanged** - `instanceCount`, `readyInstances`, etc. remain

### Gradual Rollout

1. **Phase 1**: Deploy updated controller with new status structure
2. **Phase 2**: Monitor for any issues with existing consumers
3. **Phase 3**: Update monitoring dashboards to use new child conditions
4. **Phase 4**: Update documentation and examples

### Validation

Before rollout, validate:
- [ ] Existing Prometheus metrics still work
- [ ] Existing alerting rules still fire
- [ ] Existing kubectl plugins/scripts still work
- [ ] Status subresource size stays under Kubernetes limits

## Future Enhancements

### Potential Additions

1. **DNSZone Status Tracking**
   - Track which Bind9Instances have the zone loaded
   - Show zone serial numbers per instance
   - Track zone transfer status

2. **Health Scores**
   - Calculate numeric health scores (0-100)
   - Aggregate scores up the hierarchy
   - Use for automated scaling decisions

3. **Event Correlation**
   - Link conditions to Kubernetes Events
   - Show recent events in condition messages
   - Track condition change frequency

4. **Metrics Export**
   - Export condition counts as Prometheus metrics
   - Track time-to-ready for new resources
   - Alert on specific condition reasons

## References

- [Kubernetes API Conventions - Conditions](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md#typical-status-properties)
- [Kubernetes Conditions Best Practices](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md#conditions)
- [BIND9 Documentation](https://bind9.readthedocs.io/)

## Changelog

### 2025-01-18
- Initial design document created
- Implemented `status_reasons.rs` module with standard reasons
- Added helper functions for child condition type generation
