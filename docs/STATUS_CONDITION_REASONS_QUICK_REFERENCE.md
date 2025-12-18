# Status Condition Reasons - Quick Reference

**Last Updated:** 2025-01-18

## Key Distinction: Encompassing vs Child Conditions

### Encompassing Condition (`type: Ready`)
The overall health indicator for the resource.

**When to use:**
- The main `type: Ready` condition that shows overall resource health

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
Track individual child resource health.

**When to use:**
- Conditions for specific children (e.g., `Bind9Instance-0`, `Pod-1`)

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

## HTTP Error Code Mapping

| HTTP | Reason | Message Template |
|------|--------|------------------|
| Connection Error | `BindcarUnreachable` | "Cannot connect to Bindcar API" |
| 400 | `BindcarBadRequest` | "Invalid request to Bindcar API (400)" |
| 401, 403 | `BindcarAuthFailed` | "Bindcar authentication failed" |
| 404 | `ZoneNotFound` | "Zone or resource not found in BIND9 (404)" |
| 500 | `BindcarInternalError` | "Bindcar API internal error (500)" |
| 501 | `BindcarNotImplemented` | "Operation not supported by Bindcar (501)" |
| 502, 503, 504 | `GatewayError` | "Gateway error reaching Bindcar ({code})" |

## Complete Reason Reference

### Common (All Resources)

| Reason | When to Use | Condition Type |
|--------|-------------|----------------|
| `AllReady` | All children ready | Encompassing |
| `Ready` | Child is ready | Child |
| `PartiallyReady` | Some ready, some not | Both |
| `NotReady` | None ready | Both |
| `NoChildren` | No children found | Encompassing |
| `Progressing` | Creating/updating | Both |
| `ConfigurationValid` | Config validated | Any |
| `ConfigurationInvalid` | Config invalid | Any |

### Bind9Instance Specific

| Reason | When to Use | Condition Type |
|--------|-------------|----------------|
| `MinimumReplicasAvailable` | Min replicas met, not all | Encompassing |
| `ProgressDeadlineExceeded` | Deployment stuck | Encompassing |
| `RNDCAuthenticationFailed` | RNDC auth failed | Child |
| `BindcarUnreachable` | Can't connect to API | Child |
| `BindcarBadRequest` | HTTP 400 | Child |
| `BindcarAuthFailed` | HTTP 401/403 | Child |
| `ZoneNotFound` | HTTP 404 | Child |
| `BindcarInternalError` | HTTP 500 | Child |
| `BindcarNotImplemented` | HTTP 501 | Child |
| `GatewayError` | HTTP 502/503/504 | Child |
| `ZoneTransferComplete` | Secondary synced | Child |
| `ZoneTransferFailed` | Zone sync failed | Child |
| `PodsPending` | Waiting for schedule | Encompassing |
| `PodsCrashing` | CrashLoopBackOff | Encompassing |

### Bind9Cluster Specific

| Reason | When to Use | Condition Type |
|--------|-------------|----------------|
| `InstancesCreated` | All instances created | Encompassing |
| `InstancesScaling` | Scaling in progress | Encompassing |
| `InstancesPending` | Waiting for instances | Encompassing |

### Bind9GlobalCluster Specific

| Reason | When to Use | Condition Type |
|--------|-------------|----------------|
| `ClustersReady` | All clusters ready | Encompassing |
| `ClustersProgressing` | Some clusters not ready | Encompassing |

### Network/External

| Reason | When to Use | Condition Type |
|--------|-------------|----------------|
| `UpstreamUnreachable` | Can't reach external service | Child |

## Code Examples

### Setting Encompassing Condition

```rust
use crate::status_reasons::*;

// When all children are ready
Condition {
    r#type: CONDITION_TYPE_READY.to_string(),
    status: "True".to_string(),
    reason: Some(REASON_ALL_READY.to_string()),
    message: Some(format!("All {} instances are ready", total_count)),
    last_transition_time: Some(Utc::now().to_rfc3339()),
}

// When some children are ready
Condition {
    r#type: CONDITION_TYPE_READY.to_string(),
    status: "False".to_string(),
    reason: Some(REASON_PARTIALLY_READY.to_string()),
    message: Some(format!("{}/{} instances are ready", ready_count, total_count)),
    last_transition_time: Some(Utc::now().to_rfc3339()),
}
```

### Setting Child Condition

```rust
use crate::status_reasons::*;

// When a specific child is ready
Condition {
    r#type: bind9_instance_condition_type(0), // "Bind9Instance-0"
    status: "True".to_string(),
    reason: Some(REASON_READY.to_string()), // NOT AllReady!
    message: Some(format!("Instance {} is ready (2/2 pods)", instance_name)),
    last_transition_time: Some(Utc::now().to_rfc3339()),
}

// When a specific child has a failure
Condition {
    r#type: pod_condition_type(1), // "Pod-1"
    status: "False".to_string(),
    reason: Some(REASON_BINDCAR_UNREACHABLE.to_string()),
    message: Some(format!("Pod {} cannot connect to Bindcar API", pod_name)),
    last_transition_time: Some(Utc::now().to_rfc3339()),
}
```

### Mapping HTTP Errors

```rust
use crate::status_reasons::*;

fn map_http_error(status_code: u16) -> (&'static str, String) {
    match status_code {
        400 => (REASON_BINDCAR_BAD_REQUEST, "Invalid request to Bindcar API (400)".into()),
        401 | 403 => (REASON_BINDCAR_AUTH_FAILED, "Bindcar authentication failed".into()),
        404 => (REASON_ZONE_NOT_FOUND, "Zone or resource not found in BIND9 (404)".into()),
        500 => (REASON_BINDCAR_INTERNAL_ERROR, "Bindcar API internal error (500)".into()),
        501 => (REASON_BINDCAR_NOT_IMPLEMENTED, "Operation not supported by Bindcar (501)".into()),
        502 | 503 | 504 => (REASON_GATEWAY_ERROR, format!("Gateway error reaching Bindcar ({})", status_code)),
        _ => (REASON_BINDCAR_UNREACHABLE, format!("Unexpected HTTP error ({})", status_code)),
    }
}
```

## Message Format Templates

### Encompassing Condition Messages

```rust
// All ready
format!("All {} {} are ready", count, resource_type)
// Examples: "All 3 instances are ready", "All 2 pods are ready"

// Partially ready
format!("{}/{} {} are ready", ready, total, resource_type)
// Examples: "2/3 instances are ready", "1/2 pods are ready"

// Not ready
format!("No {} are ready", resource_type)
// Examples: "No instances are ready", "No pods are ready"

// No children
format!("No {} found for this {}", child_type, parent_type)
// Examples: "No instances found for this cluster"
```

### Child Condition Messages

```rust
// Child ready
format!("{} {} is ready ({})", resource_type, name, details)
// Examples: "Instance my-cluster-primary-0 is ready (2/2 pods)"
//           "Pod my-cluster-primary-0-abc123 is ready"

// Child progressing
format!("{} {} is progressing ({})", resource_type, name, details)
// Examples: "Instance my-cluster-secondary-0 is progressing (1/2 pods)"

// Child failure
format!("{} {} {}", resource_type, name, failure_description)
// Examples: "Pod my-cluster-primary-0-xyz456 cannot connect to Bindcar API"
//           "Instance my-cluster-primary-1 has no ready pods"
```

## Kubectl Examples

### View All Conditions

```bash
kubectl get bind9cluster production-dns -o yaml | yq '.status.conditions'
```

### Check Specific Child

```bash
# Find which instance is failing
kubectl get bind9cluster production-dns -o yaml | \
  yq '.status.conditions[] | select(.status == "False" and .type != "Ready")'
```

### Monitor for Specific Reasons

```bash
# Find all instances with Bindcar issues
kubectl get bind9instances -A -o json | \
  jq '.items[] | select(.status.conditions[]?.reason | contains("Bindcar"))'
```

## Testing Checklist

When implementing, verify these scenarios:

### Encompassing Condition Tests
- [ ] `AllReady` when all children ready
- [ ] `PartiallyReady` when some ready
- [ ] `NotReady` when none ready
- [ ] `NoChildren` when no children exist

### Child Condition Tests
- [ ] `Ready` for healthy children
- [ ] `PartiallyReady` for children with partial sub-resources
- [ ] Specific failure reasons (HTTP codes, pod issues, etc.)
- [ ] Condition messages include resource names
- [ ] Condition messages include counts/details

## Related Files

- [src/status_reasons.rs](src/status_reasons.rs) - Constant definitions
- [STATUS_CONDITIONS_DESIGN.md](STATUS_CONDITIONS_DESIGN.md) - Full design doc
- [STATUS_CONDITIONS_IMPLEMENTATION.md](STATUS_CONDITIONS_IMPLEMENTATION.md) - Implementation tracking
