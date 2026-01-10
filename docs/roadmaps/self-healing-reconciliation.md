# Self-Healing Reconciliation and Owner-Aware Deletion

**Date:** 2025-12-27
**Status:** Proposed
**Impact:** Critical - Ensures proper Kubernetes operator self-healing behavior
**Priority:** High
**Author:** Erick Bourgeois

## Problem Statement

The operator currently has **incomplete self-healing behavior** for managed resources:

1. **Missing drift detection in `Bind9Cluster`**: When a `Bind9Instance` is manually deleted, the `Bind9Cluster` controller does NOT recreate it automatically
2. **Slow drift detection**: When a `Deployment` is deleted, the `Bind9Instance` controller recreates it, but only after a 5-minute requeue delay (no immediate notification)
3. **No owner-aware deletion logic**: The system doesn't properly distinguish between:
   - **User-initiated deletion** (should propagate down the hierarchy via owner references)
   - **Accidental/external deletion** (should trigger self-healing to recreate the resource)

### Current Behavior

**Scenario 1: User deletes a `Deployment`**
- ❌ **Current**: `Bind9Instance` eventually recreates it (after 5 min requeue)
- ✅ **Expected**: `Bind9Instance` immediately detects and recreates it

**Scenario 2: User deletes a `Bind9Instance`**
- ❌ **Current**: `Bind9Instance` stays deleted, `Bind9Cluster` does nothing
- ✅ **Expected**: `Bind9Cluster` immediately detects and recreates it

**Scenario 3: User deletes a `Bind9Cluster`**
- ✅ **Current**: Kubernetes owner references cascade delete to `Bind9Instance` and `Deployment`
- ✅ **Expected**: Same behavior (correct)

### Root Cause

1. **No `.owns()` watch relationships**: Controllers don't get notified when owned resources are deleted
2. **Incomplete drift detection**: `Bind9Cluster` has no drift detection code (unlike `Bind9Instance`)
3. **Generation-based reconciliation skips drift checks**: Controllers only reconcile on spec changes, not when actual state diverges

## Ownership Hierarchy

```
ClusterBind9Provider (cluster-scoped)
  └─ owns → Bind9Cluster (namespace-scoped)
              └─ owns → Bind9Instance (namespace-scoped)
                          └─ owns → Deployment (namespace-scoped)
                          └─ owns → ConfigMap (namespace-scoped)
                          └─ owns → Service (namespace-scoped)
```

## Self-Healing Requirements

### Principle: Owner Recreates Deleted Children

When a child resource is deleted **WITHOUT** deleting its owner:
1. **Owner should detect** the deletion (via watch or drift detection)
2. **Owner should reconcile** and recreate the deleted child
3. **Deletion should only propagate down** (not up) via owner references

### Implementation Strategy

Each controller layer must implement:
1. **`.owns()` watches** - Get immediate notification when owned resources change
2. **Drift detection** - Check if actual state matches desired state (regardless of generation)
3. **Owner-aware deletion** - Only delete if the owner is being deleted (via `deletionTimestamp`)

## Proposed Solution

### Phase 1: Add `.owns()` Watch Relationships

**Goal:** Controllers get immediate notification when owned resources are deleted.

#### 1.1. `Bind9Instance` Controller watches `Deployment`

**File:** [src/main.rs](../../src/main.rs)

**Current code:**
```rust
async fn run_bind9instance_controller(client: Client) -> Result<()> {
    info!("Starting Bind9Instance controller");

    let api = Api::<Bind9Instance>::all(client.clone());

    Controller::new(api, default_watcher_config())
        .run(
            reconcile_bind9instance_wrapper,
            error_policy_instance,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}
```

**New code:**
```rust
async fn run_bind9instance_controller(client: Client) -> Result<()> {
    info!("Starting Bind9Instance controller");

    let api = Api::<Bind9Instance>::all(client.clone());
    let deployment_api = Api::<Deployment>::all(client.clone());

    Controller::new(api, default_watcher_config())
        .owns(deployment_api, default_watcher_config())  // ← ADD THIS
        .run(
            reconcile_bind9instance_wrapper,
            error_policy_instance,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}
```

**Impact:**
- When a `Deployment` is deleted, the owning `Bind9Instance` is **immediately reconciled**
- Reduces drift detection time from **5 minutes → ~1 second**

#### 1.2. `Bind9Cluster` Controller watches `Bind9Instance`

**File:** [src/main.rs](../../src/main.rs)

**Current code:**
```rust
async fn run_bind9cluster_controller(client: Client) -> Result<()> {
    info!("Starting Bind9Cluster controller");

    let api = Api::<Bind9Cluster>::all(client.clone());

    Controller::new(api, default_watcher_config())
        .run(
            reconcile_bind9cluster_wrapper,
            error_policy_cluster,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}
```

**New code:**
```rust
async fn run_bind9cluster_controller(client: Client) -> Result<()> {
    info!("Starting Bind9Cluster controller");

    let api = Api::<Bind9Cluster>::all(client.clone());
    let instance_api = Api::<Bind9Instance>::all(client.clone());

    Controller::new(api, default_watcher_config())
        .owns(instance_api, default_watcher_config())  // ← ADD THIS
        .run(
            reconcile_bind9cluster_wrapper,
            error_policy_cluster,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}
```

**Impact:**
- When a `Bind9Instance` is deleted, the owning `Bind9Cluster` is **immediately reconciled**
- Enables self-healing for accidentally deleted instances

#### 1.3. `ClusterBind9Provider` Controller watches `Bind9Cluster`

**File:** [src/main.rs](../../src/main.rs)

**Current code:**
```rust
async fn run_clusterbind9provider_controller(client: Client) -> Result<()> {
    info!("Starting ClusterBind9Provider controller");

    let api = Api::<ClusterBind9Provider>::all(client.clone());

    Controller::new(api, default_watcher_config())
        .run(
            reconcile_clusterbind9provider_wrapper,
            error_policy_clusterprovider,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}
```

**New code:**
```rust
async fn run_clusterbind9provider_controller(client: Client) -> Result<()> {
    info!("Starting ClusterBind9Provider controller");

    let api = Api::<ClusterBind9Provider>::all(client.clone());
    let cluster_api = Api::<Bind9Cluster>::all(client.clone());

    Controller::new(api, default_watcher_config())
        .owns(cluster_api, default_watcher_config())  // ← ADD THIS
        .run(
            reconcile_clusterbind9provider_wrapper,
            error_policy_clusterprovider,
            Arc::new(client),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}
```

**Note:** This change **complements** (not conflicts with) the `.watches()` optimization proposed in [CLUSTER_PROVIDER_RECONCILIATION_OPTIMIZATION.md](./CLUSTER_PROVIDER_RECONCILIATION_OPTIMIZATION.md).

**Difference between `.owns()` and `.watches()`:**
- **`.owns()`**: Watches resources owned by this controller (via `ownerReferences.controller = true`)
- **`.watches()`**: Watches related resources that may trigger reconciliation (e.g., instances referenced by `cluster_ref`)

Both are needed for complete behavior:
- `.owns(Bind9Cluster)` - Triggers when cluster is deleted/modified
- `.watches(Bind9Instance)` - Triggers when instance status changes (for faster status propagation)

---

### Phase 2: Add Drift Detection to `Bind9Cluster`

**Goal:** `Bind9Cluster` reconciler detects when actual instances don't match desired replicas.

**File:** [src/reconcilers/bind9cluster.rs](../../src/reconcilers/bind9cluster.rs)

**Current code (lines 99-118):**
```rust
// Only reconcile spec-related resources if spec changed
let spec_changed =
    crate::reconcilers::should_reconcile(current_generation, observed_generation);

if spec_changed {
    debug!(
        "Reconciliation needed: current_generation={:?}, observed_generation={:?}",
        current_generation, observed_generation
    );

    // Create or update shared cluster ConfigMap
    create_or_update_cluster_configmap(&client, &cluster).await?;

    // Reconcile managed instances (create/update as needed)
    reconcile_managed_instances(&client, &cluster).await?;
} else {
    debug!(
        "Spec unchanged (generation={:?}), skipping cluster resource updates",
        current_generation
    );
}
```

**New code:**
```rust
// Only reconcile spec-related resources if spec changed OR drift detected
let spec_changed =
    crate::reconcilers::should_reconcile(current_generation, observed_generation);

// DRIFT DETECTION: Check if managed instances match desired state
let drift_detected = if !spec_changed {
    detect_instance_drift(&client, &cluster, &namespace, &name).await?
} else {
    false
};

if spec_changed || drift_detected {
    if drift_detected {
        info!(
            "Spec unchanged but instance drift detected for cluster {}/{}",
            namespace, name
        );
    } else {
        debug!(
            "Reconciliation needed: current_generation={:?}, observed_generation={:?}",
            current_generation, observed_generation
        );
    }

    // Create or update shared cluster ConfigMap
    create_or_update_cluster_configmap(&client, &cluster).await?;

    // Reconcile managed instances (create/update as needed)
    reconcile_managed_instances(&client, &cluster).await?;
} else {
    debug!(
        "Spec unchanged (generation={:?}) and no drift detected, skipping resource reconciliation",
        current_generation
    );
}
```

**New function to add:**
```rust
/// Detects if the actual managed instances match the desired replica counts.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `cluster` - The Bind9Cluster to check
/// * `namespace` - Cluster namespace
/// * `name` - Cluster name
///
/// # Returns
///
/// * `Ok(true)` - Drift detected (instances don't match desired state)
/// * `Ok(false)` - No drift (instances match desired state)
/// * `Err(_)` - Failed to check drift
async fn detect_instance_drift(
    client: &Client,
    cluster: &Bind9Cluster,
    namespace: &str,
    name: &str,
) -> Result<bool> {
    use crate::labels::{BINDY_CLUSTER_LABEL, BINDY_MANAGED_BY_LABEL, MANAGED_BY_BIND9_CLUSTER};

    // Get desired replica counts from spec
    let desired_primary = cluster
        .spec
        .common
        .primary
        .as_ref()
        .and_then(|p| p.replicas)
        .unwrap_or(0);

    let desired_secondary = cluster
        .spec
        .common
        .secondary
        .as_ref()
        .and_then(|s| s.replicas)
        .unwrap_or(0);

    // List existing managed instances
    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let instances = api.list(&ListParams::default()).await?;

    // Filter for managed instances of this cluster
    let managed_instances: Vec<_> = instances
        .items
        .into_iter()
        .filter(|instance| {
            instance.metadata.labels.as_ref().is_some_and(|labels| {
                labels.get(BINDY_MANAGED_BY_LABEL) == Some(&MANAGED_BY_BIND9_CLUSTER.to_string())
                    && labels.get(BINDY_CLUSTER_LABEL) == Some(&name.to_string())
            })
        })
        .collect();

    // Count by role
    let actual_primary = managed_instances
        .iter()
        .filter(|i| i.spec.role == ServerRole::Primary)
        .count();

    let actual_secondary = managed_instances
        .iter()
        .filter(|i| i.spec.role == ServerRole::Secondary)
        .count();

    // Drift detected if counts don't match
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let drift = actual_primary != desired_primary as usize
        || actual_secondary != desired_secondary as usize;

    if drift {
        info!(
            "Instance drift detected for cluster {}/{}: desired (primary={}, secondary={}), actual (primary={}, secondary={})",
            namespace, name, desired_primary, desired_secondary, actual_primary, actual_secondary
        );
    }

    Ok(drift)
}
```

**Location:** Add this function before `reconcile_managed_instances()` in [src/reconcilers/bind9cluster.rs](../../src/reconcilers/bind9cluster.rs)

**Impact:**
- `Bind9Cluster` now **always** checks if instances match desired state
- Missing instances are **automatically recreated**
- Works even when spec hasn't changed (generation-independent)

---

### Phase 3: Improve `Bind9Instance` Drift Detection

**Goal:** Clarify and enhance the existing drift detection in `Bind9Instance`.

**File:** [src/reconcilers/bind9instance.rs](../../src/reconcilers/bind9instance.rs)

**Current code is already good** (lines 159-184), but we should enhance the logging:

**Modify line 183:**
```rust
// OLD
info!("Spec unchanged but Deployment missing - drift detected, reconciling resources");

// NEW
info!(
    "Drift detected for Bind9Instance {}/{}: Deployment missing, will recreate",
    namespace, name
);
```

**Impact:**
- Clearer logs for debugging
- Consistency with other drift detection messages

---

### Phase 4: Owner-Aware Deletion Logic

**Goal:** Ensure resources are only deleted when their owner is being deleted (not when manually recreated).

This is **already implemented correctly** via Kubernetes owner references with `blockOwnerDeletion: true`:

```rust
// Example from bind9cluster.rs:539-546
let owner_ref = k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference {
    api_version: API_GROUP_VERSION.to_string(),
    kind: KIND_BIND9_CLUSTER.to_string(),
    name: cluster_name.clone(),
    uid: cluster.metadata.uid.clone().unwrap_or_default(),
    controller: Some(true),
    block_owner_deletion: Some(true),  // ← Prevents deletion of owner while children exist
};
```

**How it works:**
1. When a **child** is deleted (e.g., `Deployment`), owner is NOT deleted → Controller recreates child
2. When an **owner** is deleted (e.g., `Bind9Instance`), Kubernetes:
   - Sets `deletionTimestamp` on owner
   - Calls finalizer cleanup
   - Cascades deletion to all children (via `ownerReferences`)
3. `blockOwnerDeletion: true` ensures owner can't be deleted until all children are gone

**No changes needed** - current implementation is correct!

---

## Testing Plan

### Unit Tests

1. **Test drift detection in `Bind9Cluster`:**
   ```rust
   #[tokio::test]
   async fn test_detect_instance_drift() {
       // Setup: Cluster wants 2 primary, 1 secondary
       // Actual: Only 2 primary exist
       // Expected: drift_detected = true
   }
   ```

2. **Test `.owns()` watch triggers reconciliation:**
   ```rust
   #[tokio::test]
   async fn test_deployment_deletion_triggers_reconciliation() {
       // Setup: Create Bind9Instance and Deployment
       // Action: Delete Deployment
       // Expected: Bind9Instance reconciler is called
   }
   ```

### Integration Tests

1. **Test `Deployment` self-healing:**
   ```bash
   # Create Bind9Instance
   kubectl apply -f bind9instance.yaml

   # Wait for Deployment to exist
   kubectl wait --for=condition=Available deployment/my-instance

   # Delete Deployment
   kubectl delete deployment my-instance

   # Verify Deployment is recreated within 5 seconds
   timeout 5 kubectl wait --for=condition=Available deployment/my-instance
   ```

2. **Test `Bind9Instance` self-healing:**
   ```bash
   # Create Bind9Cluster with replicas=2
   kubectl apply -f bind9cluster.yaml

   # Wait for instances to exist
   kubectl wait --for=condition=Ready bind9instance my-cluster-primary-0
   kubectl wait --for=condition=Ready bind9instance my-cluster-primary-1

   # Delete one instance
   kubectl delete bind9instance my-cluster-primary-1

   # Verify instance is recreated within 5 seconds
   timeout 5 kubectl wait --for=condition=Ready bind9instance my-cluster-primary-1
   ```

3. **Test owner deletion cascades correctly:**
   ```bash
   # Create full hierarchy
   kubectl apply -f clusterbind9provider.yaml

   # Wait for all resources
   kubectl wait --for=condition=Ready clusterbind9provider production-dns

   # Delete ClusterBind9Provider
   kubectl delete clusterbind9provider production-dns

   # Verify all children are deleted (cascade)
   kubectl get bind9cluster production-dns --should-fail
   kubectl get bind9instance -l cluster=production-dns --should-be-empty
   kubectl get deployment -l cluster=production-dns --should-be-empty
   ```

---

## Performance Impact

### Expected Improvements

| Scenario | Before | After | Improvement |
|----------|--------|-------|-------------|
| `Deployment` deleted | 5 min (requeue) | ~1 second (watch) | **99.7% faster** |
| `Bind9Instance` deleted | Never recreated | ~1 second (watch + drift) | **∞ improvement** |
| `Bind9Cluster` deleted | Correct (cascade) | Correct (no change) | N/A |

### API Server Load

**Before:**
- Periodic LIST operations every 5 minutes per ready instance
- Periodic LIST operations every 30 seconds per not-ready instance

**After:**
- One-time WATCH connections per controller (persistent)
- Drift detection runs during each reconciliation (no extra API calls)
- **Net reduction in API load** due to fewer polling operations

---

## Implementation Checklist

### Phase 1: Add `.owns()` Watches
- [ ] Add `.owns(Deployment)` to `Bind9Instance` controller
- [ ] Add `.owns(Bind9Instance)` to `Bind9Cluster` controller
- [ ] Add `.owns(Bind9Cluster)` to `ClusterBind9Provider` controller
- [ ] Add `use k8s_openapi::api::apps::v1::Deployment` imports
- [ ] Update `CHANGELOG.md` with watch additions
- [ ] Test that watches trigger reconciliation

### Phase 2: Drift Detection for `Bind9Cluster`
- [ ] Implement `detect_instance_drift()` function
- [ ] Add drift check to reconciliation logic
- [ ] Add info-level logs when drift detected
- [ ] Test drift detection with unit tests
- [ ] Test self-healing in integration tests
- [ ] Update `CHANGELOG.md` with drift detection

### Phase 3: Improve `Bind9Instance` Drift Logging
- [ ] Update drift detection log message
- [ ] Ensure consistency across all drift detection
- [ ] Update `CHANGELOG.md` with logging improvements

### Phase 4: Testing
- [ ] Write unit tests for drift detection
- [ ] Write integration tests for self-healing
- [ ] Write integration tests for cascade deletion
- [ ] Add tests to CI/CD pipeline
- [ ] Document test scenarios

### Phase 5: Documentation
- [ ] Update architecture diagrams showing watch relationships
- [ ] Document self-healing behavior in user guide
- [ ] Add troubleshooting section for drift scenarios
- [ ] Update API documentation if needed
- [ ] Run `make docs` to rebuild documentation

### Final Steps
- [ ] Run `cargo fmt` to format code
- [ ] Run `cargo clippy -- -D warnings` and fix all warnings
- [ ] Run `cargo test` and ensure all tests pass
- [ ] Manually test self-healing scenarios in Kind cluster
- [ ] Update `CHANGELOG.md` with complete summary
- [ ] Create PR with detailed description

---

## Compatibility with Other Roadmaps

### [CLUSTER_PROVIDER_RECONCILIATION_OPTIMIZATION.md](./CLUSTER_PROVIDER_RECONCILIATION_OPTIMIZATION.md)

**Relationship:** **Complementary** (both should be implemented)

- **Optimization roadmap adds**: `.watches(Bind9Instance)` for faster status propagation
- **This roadmap adds**: `.owns(Bind9Cluster)` for detecting cluster deletion
- **Together they provide**:
  - `.owns(Bind9Cluster)` - Immediate notification when cluster is deleted/modified
  - `.watches(Bind9Instance)` - Immediate notification when instance status changes
  - Complete event-driven architecture

**No conflicts** - both changes work together perfectly!

---

## References

- **Current Implementation:**
  - [src/main.rs:847-862](../../src/main.rs#L847-L862) - `Bind9Instance` controller setup
  - [src/main.rs:709-724](../../src/main.rs#L709-L724) - `Bind9Cluster` controller setup
  - [src/main.rs:822-838](../../src/main.rs#L822-L838) - `ClusterBind9Provider` controller setup
  - [src/reconcilers/bind9instance.rs:159-184](../../src/reconcilers/bind9instance.rs#L159-L184) - Existing drift detection
  - [src/reconcilers/bind9cluster.rs:99-118](../../src/reconcilers/bind9cluster.rs#L99-L118) - Missing drift detection

- **kube-rs Documentation:**
  - [Controller.owns()](https://docs.rs/kube/latest/kube/runtime/struct.Controller.html#method.owns)
  - [Controller.watches()](https://docs.rs/kube/latest/kube/runtime/struct.Controller.html#method.watches)
  - [Owner References](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/)

- **Kubernetes Documentation:**
  - [Garbage Collection](https://kubernetes.io/docs/concepts/architecture/garbage-collection/)
  - [Finalizers](https://kubernetes.io/docs/concepts/overview/working-with-objects/finalizers/)

---

## Success Criteria

✅ **Self-healing works for all layers:**
1. Delete `Deployment` → `Bind9Instance` recreates it within 5 seconds
2. Delete `Bind9Instance` → `Bind9Cluster` recreates it within 5 seconds
3. Delete `Bind9Cluster` → `ClusterBind9Provider` recreates it within 5 seconds

✅ **Cascade deletion still works:**
1. Delete `ClusterBind9Provider` → All `Bind9Cluster`, `Bind9Instance`, `Deployment` are deleted
2. Delete `Bind9Cluster` → All `Bind9Instance` and `Deployment` are deleted
3. Delete `Bind9Instance` → All `Deployment`, `ConfigMap`, `Service` are deleted

✅ **All tests pass:**
1. Unit tests for drift detection pass
2. Integration tests for self-healing pass
3. Integration tests for cascade deletion pass
4. `cargo clippy` has no warnings
5. `cargo test` all pass
