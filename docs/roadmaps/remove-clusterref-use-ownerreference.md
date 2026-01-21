# Remove clusterRef from Bind9Instance.spec - Use ownerReference Instead

**Status:** 📋 Planning
**Date:** 2026-01-02
**Author:** Erick Bourgeois
**Impact:** 🟡 Breaking Change - v1beta1 CRD Update (No Migration Required)

---

## Executive Summary

**Goal**: Remove the `clusterRef` field from `Bind9Instance.spec` and replace it with Kubernetes-native `ownerReference` to establish the instance-to-cluster relationship.

**Why**:
- **Kubernetes-Native**: ownerReference is the standard way to express resource ownership
- **Automatic Garbage Collection**: When a cluster is deleted, Kubernetes automatically deletes owned instances
- **Cleaner Architecture**: Eliminates redundant spec field
- **Better Lifecycle Management**: Proper parent-child relationship semantics
- **Prevents Orphaned Resources**: ownerReference ensures instances can't outlive their cluster

**Current State**: `Bind9Instance.spec.clusterRef` is a string field that must match the name of a `Bind9Cluster` or `ClusterBind9Provider`. All reconciler logic uses string comparison to find instances belonging to a cluster.

**Target State**:
- `Bind9Instance.metadata.ownerReferences` contains reference to parent cluster
- `Bind9Instance.status.clusterRef` **kept for reference** (provides human-readable cluster info)
- Reconciler logic uses ownerReference filtering to find instances

**Migration Strategy**: **No migration script** - Breaking change acceptable for early-stage v1beta1 project. Users must recreate instances.

---

## Current Architecture (Before Migration)

### Resource Relationship
```yaml
# Bind9Cluster (namespace-scoped)
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system

---
# Bind9Instance (namespace-scoped)
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: production-dns-primary
  namespace: dns-system
spec:
  clusterRef: production-dns  # ← String reference to cluster name
  role: primary
  replicas: 2
```

### Discovery Pattern
```rust
// Current: Filter instances by string comparison
let instances = instance_api.list(&ListParams::default()).await?;
let filtered: Vec<_> = instances
    .items
    .into_iter()
    .filter(|instance| instance.spec.cluster_ref == cluster_name)  // ← String match
    .collect();
```

### Problem: No Automatic Cleanup
If a `Bind9Cluster` is deleted:
1. Instances with `clusterRef: production-dns` remain (orphaned)
2. User must manually find and delete all instances
3. No cascading deletion
4. No automatic lifecycle management

---

## Target Architecture (After Migration)

### Resource Relationship
```yaml
# Bind9Cluster (namespace-scoped)
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
  uid: 12345-67890-abcdef  # ← Kubernetes assigns UID

---
# Bind9Instance (namespace-scoped)
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: production-dns-primary
  namespace: dns-system
  ownerReferences:  # ← Kubernetes-native ownership
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: Bind9Cluster
      name: production-dns
      uid: 12345-67890-abcdef
      operator: true
      blockOwnerDeletion: true
spec:
  # clusterRef removed!
  role: primary
  replicas: 2
status:
  # Keep clusterRef in status for human-readable reference
  clusterRef:
    kind: Bind9Cluster
    apiVersion: bindy.firestoned.io/v1beta1
    name: production-dns
    namespace: dns-system  # Empty for ClusterBind9Provider
```

### Discovery Pattern
```rust
// New: Filter instances by ownerReference
let instances = instance_api.list(&ListParams::default()).await?;
let filtered: Vec<_> = instances
    .items
    .into_iter()
    .filter(|instance| {
        instance
            .metadata
            .owner_references
            .as_ref()
            .map(|refs| {
                refs.iter().any(|r| {
                    r.kind == "Bind9Cluster" && r.name == cluster_name
                })
            })
            .unwrap_or(false)
    })
    .collect();
```

### Benefit: Automatic Cleanup
If a `Bind9Cluster` is deleted:
1. Kubernetes automatically deletes all owned `Bind9Instance` resources
2. No orphaned instances
3. Proper cascading deletion
4. Lifecycle tied to parent cluster

---

## Migration Strategy: Breaking Change - No Migration Script

**Decision**: This is a **breaking change** in v1beta1. Users must recreate resources.

**Why No Migration Script?**
1. Project is early-stage (v1beta1) with limited production deployments
2. Simpler to document "delete and recreate" than build/maintain migration tooling
3. CRD is already v1beta1 - breaking changes are acceptable
4. Clean slate allows faster iteration

**Upgrade Approach**:
1. ✅ Document the breaking change clearly in CHANGELOG.md
2. ✅ Provide simple upgrade instructions (delete old, apply new)
3. ✅ Update all examples and documentation
4. ✅ Add validation to reject instances without ownerReference

**User Upgrade Path**:
```bash
# 1. (Optional) Save current configurations for reference
kubectl get bind9instance -A -o yaml > backup-instances.yaml

# 2. Delete old instances (clusters will recreate them automatically)
kubectl delete bind9instance --all -A

# 3. Update CRDs
kubectl replace --force -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml

# 4. Upgrade operator
kubectl set image deployment/bindy-operator \
  bindy=ghcr.io/firestoned/bindy:latest \
  -n bindy-system

# 5. Verify clusters recreate instances with ownerReferences
kubectl get bind9instance -A
kubectl get bind9instance <name> -o jsonpath='{.metadata.ownerReferences}'
```

---

## Implementation Plan

### Phase 1: Helper Functions and Core Logic

#### 1.1: Create Helper Functions for ownerReference Filtering

**Purpose**: Centralize the logic for finding instances by ownerReference

**Implementation**:
```rust
// src/reconcilers/helpers.rs (new file)

use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::ListParams;
use kube::{Api, Client, ResourceExt};
use crate::crd::Bind9Instance;
use anyhow::Result;

/// Find all Bind9Instance resources owned by a specific cluster
pub async fn find_instances_by_owner(
    client: &Client,
    namespace: &str,
    cluster_kind: &str,
    cluster_name: &str,
) -> Result<Vec<Bind9Instance>> {
    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let instances = api.list(&ListParams::default()).await?;

    Ok(instances
        .into_iter()
        .filter(|instance| {
            instance
                .owner_references()
                .iter()
                .any(|owner| owner.kind == cluster_kind && owner.name == cluster_name)
        })
        .collect())
}

/// Find all Bind9Instance resources owned by any cluster (cluster-scoped provider)
pub async fn find_instances_by_provider(
    client: &Client,
    provider_name: &str,
) -> Result<Vec<Bind9Instance>> {
    let api: Api<Bind9Instance> = Api::all(client.clone());
    let instances = api.list(&ListParams::default()).await?;

    Ok(instances
        .into_iter()
        .filter(|instance| {
            instance
                .owner_references()
                .iter()
                .any(|owner| owner.kind == "ClusterBind9Provider" && owner.name == provider_name)
        })
        .collect())
}

/// Check if an instance is owned by a specific cluster
pub fn is_owned_by_cluster(instance: &Bind9Instance, cluster_name: &str) -> bool {
    instance
        .owner_references()
        .iter()
        .any(|owner| {
            (owner.kind == "Bind9Cluster" || owner.kind == "ClusterBind9Provider")
                && owner.name == cluster_name
        })
}

/// Get the cluster reference from an instance's ownerReferences
pub fn get_cluster_from_owner_refs(instance: &Bind9Instance) -> Option<(&str, &str)> {
    instance
        .owner_references()
        .iter()
        .find(|owner| owner.kind == "Bind9Cluster" || owner.kind == "ClusterBind9Provider")
        .map(|owner| (owner.kind.as_str(), owner.name.as_str()))
}
```

**Files to Create**:
- `src/reconcilers/helpers.rs` - Centralized helper functions
- `src/reconcilers/helpers_tests.rs` - Unit tests for helpers

---

### Phase 2: CRD Changes

#### 2.1: Update `src/crd.rs`

**Changes**:
1. **Remove `cluster_ref` from `Bind9InstanceSpec`** (breaking change)
2. **Keep `cluster_ref` in `Bind9InstanceStatus`** for human-readable reference
3. Update print columns to show cluster from ownerReference

```rust
// src/crd.rs

#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "Bind9Instance",
    namespaced,
    status = "Bind9InstanceStatus",
    printcolumn = r#"{"name":"Role","type":"string","jsonPath":".spec.role"}"#,
    printcolumn = r#"{"name":"Replicas","type":"integer","jsonPath":".spec.replicas"}"#,
    // NEW: Show cluster from ownerReference
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".metadata.ownerReferences[?(@.operator==true)].name"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type=='Ready')].status"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceSpec {
    // REMOVED: cluster_ref field

    /// The role of this instance (Primary or Secondary)
    pub role: ServerRole,

    /// Number of pod replicas for this instance
    #[serde(default = "default_replicas")]
    pub replicas: i32,

    // ... rest of fields
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceStatus {
    /// Reference to the cluster this instance belongs to.
    /// Populated from metadata.ownerReferences for human-readable reference.
    ///
    /// For namespace-scoped `Bind9Cluster`, includes namespace.
    /// For cluster-scoped `ClusterBind9Provider`, namespace will be empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_ref: Option<ClusterReference>,  // KEPT for reference

    /// Conditions represent the latest available observations of the instance's state
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,

    // ... rest of fields
}

// ClusterReference type KEPT (used in status)
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClusterReference {
    /// The kind of the cluster resource (Bind9Cluster or ClusterBind9Provider)
    pub kind: String,

    /// The API version of the cluster resource
    pub api_version: String,

    /// The name of the cluster resource
    pub name: String,

    /// The namespace of the cluster (empty for cluster-scoped providers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}
```

#### 2.2: Regenerate CRD YAML Files

```bash
# Regenerate CRDs
cargo run --bin crdgen

# Verify generated files
kubectl apply --dry-run=client -f deploy/crds/bind9instances.crd.yaml
```

---

### Phase 3: Reconciler Updates

#### 3.1: Update `Bind9Cluster` Reconciler

**File**: `src/reconcilers/bind9cluster.rs`

**Changes**:
1. Set `ownerReference` when creating instances
2. Use helper functions to find instances by owner
3. **Populate `status.clusterRef`** from ownerReference

```rust
// src/reconcilers/bind9cluster.rs

use crate::reconcilers::helpers::{find_instances_by_owner, is_owned_by_cluster};
use crate::crd::ClusterReference;

async fn reconcile_bind9cluster(
    cluster: Arc<Bind9Cluster>,
    ctx: Arc<Context>,
) -> Result<Action> {
    // ...

    // OLD: Filter by cluster_ref string comparison
    // let filtered: Vec<_> = instances
    //     .items
    //     .into_iter()
    //     .filter(|instance| instance.spec.cluster_ref == cluster_name)
    //     .collect();

    // NEW: Use helper to find owned instances
    let instances = find_instances_by_owner(
        &ctx.client,
        &namespace,
        "Bind9Cluster",
        &cluster_name,
    ).await?;

    // ...
}

async fn create_instance(
    client: &Client,
    cluster: &Bind9Cluster,
    instance_name: &str,
    role: ServerRole,
) -> Result<()> {
    let cluster_name = cluster.name_any();
    let cluster_uid = cluster.metadata.uid.as_ref()
        .ok_or_else(|| anyhow!("Cluster missing UID"))?;
    let namespace = cluster.namespace()
        .ok_or_else(|| anyhow!("Cluster missing namespace"))?;

    let instance = Bind9Instance {
        metadata: ObjectMeta {
            name: Some(instance_name.to_string()),
            namespace: Some(namespace.clone()),
            // NEW: Set ownerReference
            owner_references: Some(vec![OwnerReference {
                api_version: "bindy.firestoned.io/v1beta1".to_string(),
                kind: "Bind9Cluster".to_string(),
                name: cluster_name.clone(),
                uid: cluster_uid.clone(),
                operator: Some(true),
                block_owner_deletion: Some(true),
            }]),
            ..Default::default()
        },
        spec: Bind9InstanceSpec {
            // cluster_ref REMOVED!
            role,
            replicas: 2,
            // ... rest of spec
        },
        status: None,
    };

    let api: Api<Bind9Instance> = Api::namespaced(client.clone(), &namespace);
    api.create(&PostParams::default(), &instance).await?;

    Ok(())
}
```

**Files to Update**:
- `src/reconcilers/bind9cluster.rs` - Update instance creation and filtering
- `src/reconcilers/bind9cluster_tests.rs` - Update test fixtures

#### 3.2: Update `ClusterBind9Provider` Reconciler

**File**: `src/reconcilers/clusterbind9provider.rs`

**Changes**:
1. Set `ownerReference` when creating instances (cross-namespace allowed for cluster-scoped)
2. Use helper functions to find instances by provider

```rust
// src/reconcilers/clusterbind9provider.rs

use crate::reconcilers::helpers::find_instances_by_provider;

async fn reconcile_provider(
    provider: Arc<ClusterBind9Provider>,
    ctx: Arc<Context>,
) -> Result<Action> {
    // ...

    // OLD: Filter by cluster_ref string comparison
    // let instances: Vec<_> = all_instances
    //     .items
    //     .into_iter()
    //     .filter(|inst| inst.spec.cluster_ref == provider_name)
    //     .collect();

    // NEW: Use helper to find owned instances (all namespaces)
    let instances = find_instances_by_provider(&ctx.client, &provider_name).await?;

    // ...
}
```

**Note on Cross-Namespace ownerReference**:
- Kubernetes allows cluster-scoped resources to own namespace-scoped resources
- `ClusterBind9Provider` (cluster-scoped) can own `Bind9Instance` (namespace-scoped)
- This is standard Kubernetes behavior (e.g., ClusterRole owns RoleBinding)

**Files to Update**:
- `src/reconcilers/clusterbind9provider.rs` - Update instance filtering
- `src/reconcilers/clusterbind9provider_tests.rs` - Update test fixtures

#### 3.3: Update `Bind9Instance` Reconciler

**File**: `src/reconcilers/bind9instance.rs`

**Changes**:
1. **Populate `status.clusterRef`** from ownerReference
2. Validate that instance has ownerReference set
3. Add admission validation (reconciler check)

```rust
// src/reconcilers/bind9instance.rs

use crate::reconcilers::helpers::get_cluster_from_owner_refs;
use crate::crd::ClusterReference;

async fn reconcile_bind9instance(
    instance: Arc<Bind9Instance>,
    ctx: Arc<Context>,
) -> Result<Action> {
    let instance_name = instance.name_any();
    let namespace = instance.namespace().unwrap_or_default();

    // VALIDATION: Ensure ownerReference exists
    let (cluster_kind, cluster_name) = get_cluster_from_owner_refs(&instance)
        .ok_or_else(|| anyhow!(
            "Bind9Instance '{}' missing ownerReference to Bind9Cluster or ClusterBind9Provider",
            instance_name
        ))?;

    info!(
        "Reconciling instance '{}' owned by {}/{}",
        instance_name, cluster_kind, cluster_name
    );

    // Populate status.clusterRef from ownerReference for human-readable reference
    let cluster_ref = Some(ClusterReference {
        kind: cluster_kind.to_string(),
        api_version: "bindy.firestoned.io/v1beta1".to_string(),
        name: cluster_name.to_string(),
        namespace: if cluster_kind == "Bind9Cluster" {
            Some(namespace.clone())
        } else {
            None  // ClusterBind9Provider is cluster-scoped
        },
    });

    // Update status with cluster reference
    let mut status = instance.status.clone().unwrap_or_default();
    status.cluster_ref = cluster_ref;

    // ... rest of reconciliation

    // Update status
    let api: Api<Bind9Instance> = Api::namespaced(ctx.client.clone(), &namespace);
    api.replace_status(&instance_name, &PostParams::default(), serde_json::to_vec(&Bind9Instance {
        metadata: instance.metadata.clone(),
        spec: instance.spec.clone(),
        status: Some(status),
    })?).await?;

    Ok(Action::requeue(Duration::from_secs(300)))
}
```

**Files to Update**:
- `src/reconcilers/bind9instance.rs` - Add validation, populate status.clusterRef
- `src/reconcilers/bind9instance_tests.rs` - Update test fixtures with ownerReferences

#### 3.4: Update DNSZone Reconciler

**File**: `src/reconcilers/dnszone.rs`

**Changes**:
1. Update `find_all_primary_pods()` to use ownerReference filtering
2. Update `find_all_secondary_pods()` to use ownerReference filtering
3. Update all callsites

```rust
// src/reconcilers/dnszone.rs

use crate::reconcilers::helpers::{find_instances_by_owner, find_instances_by_provider};

pub async fn find_all_primary_pods(
    client: &Client,
    namespace: &str,
    cluster_kind: &str,  // "Bind9Cluster" or "ClusterBind9Provider"
    cluster_name: &str,
) -> Result<Vec<PodInfo>> {
    let mut primary_pods = Vec::new();

    // NEW: Use helper to find owned instances
    let instances = if cluster_kind == "ClusterBind9Provider" {
        find_instances_by_provider(client, cluster_name).await?
    } else {
        find_instances_by_owner(client, namespace, cluster_kind, cluster_name).await?
    };

    // Filter for primary instances
    let primary_instances: Vec<_> = instances
        .into_iter()
        .filter(|inst| inst.spec.role == ServerRole::Primary)
        .collect();

    // Find pods for each primary instance (same logic as before)
    for instance in primary_instances {
        let instance_name = instance.name_any();
        let instance_ns = instance.namespace().unwrap_or_default();

        let label_selector = format!("app=bind9,instance={}", instance_name);
        let lp = ListParams::default().labels(&label_selector);

        let pod_api: Api<Pod> = Api::namespaced(client.clone(), &instance_ns);
        let pods = pod_api.list(&lp).await?;

        for pod in pods {
            if let Some(pod_ip) = pod.status.and_then(|s| s.pod_ip) {
                primary_pods.push(PodInfo {
                    name: pod.metadata.name.unwrap_or_default(),
                    namespace: instance_ns.clone(),
                    ip: pod_ip,
                });
            }
        }
    }

    Ok(primary_pods)
}

// Similar changes for find_all_secondary_pods()
pub async fn find_all_secondary_pods(
    client: &Client,
    namespace: &str,
    cluster_kind: &str,
    cluster_name: &str,
) -> Result<Vec<PodInfo>> {
    // Same pattern as find_all_primary_pods() but filter for ServerRole::Secondary
}
```

**Callsite Updates**:
All functions that currently extract cluster info from zone need to get cluster kind:

```rust
// Get cluster reference from zone's selected instances
let cluster_kind = /* determine from first instance's ownerReference */;
let cluster_name = /* extract from ownerReference */;

let primary_pods = find_all_primary_pods(
    &client,
    &namespace,
    cluster_kind,
    cluster_name,
).await?;
```

**Files to Update**:
- `src/reconcilers/dnszone.rs` - Update pod discovery functions
- `src/reconcilers/dnszone_tests.rs` - Update test fixtures
- `src/reconcilers/records.rs` - Update callsites if needed
- `src/reconcilers/records_tests.rs` - Update test fixtures

---

### Phase 4: Examples and Documentation

#### 4.1: Update All Examples

**Files to Update**:
- `examples/bind9-instance.yaml`
- `examples/bind9-cluster-with-storage.yaml`
- `examples/complete-setup.yaml`
- `examples/custom-zones-configmap.yaml`
- `examples/multi-tenancy.yaml`
- `examples/zone-label-selector.yaml`
- `examples/README.md`

**Example Update** (`examples/bind9-instance.yaml`):
```yaml
# Before
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: production-dns-primary
  namespace: dns-system
spec:
  clusterRef: production-dns  # ← REMOVE THIS
  role: primary
  replicas: 2

# After
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: production-dns-primary
  namespace: dns-system
  # ownerReferences are set automatically by the Bind9Cluster operator
  # when it creates instances. DO NOT set manually unless creating standalone.
spec:
  role: primary
  replicas: 2
```

**Update `examples/README.md`**:
- Remove sections explaining `clusterRef` relationships
- Add section explaining ownerReference is set automatically by cluster operators
- Update resource hierarchy diagram showing ownerReference arrows

#### 4.2: Update Documentation

**Files to Update**:
- `docs/src/reference/bind9instance-spec.md` - Remove clusterRef field
- `docs/src/reference/bind9instance-status.md` - Document clusterRef is populated from ownerReference
- `docs/src/concepts/architecture.md` - Update architecture diagrams with ownerReference
- `docs/src/concepts/bind9instance.md` - Explain ownerReference relationship
- `docs/src/guide/zone-selection.md` - Update cluster discovery explanation
- `docs/src/guide/multi-tenancy.md` - Update multi-tenancy examples

**New Documentation**:
- `docs/src/concepts/resource-ownership.md` - Explain ownerReference model and automatic garbage collection

#### 4.3: Update CHANGELOG.md

```markdown
## [v1beta1 - BREAKING] - 2026-01-XX

**Author:** Erick Bourgeois

### BREAKING CHANGES

#### Removed: `Bind9Instance.spec.clusterRef`

**What Changed**: The `clusterRef` field has been removed from `Bind9Instance.spec`. Instances now use Kubernetes-native `ownerReference` to establish the relationship with their parent cluster.

**Migration Required**: YES - You must delete and recreate instances.

**Upgrade Steps**:
1. (Optional) Backup instances: `kubectl get bind9instance -A -o yaml > backup.yaml`
2. Delete instances: `kubectl delete bind9instance --all -A`
3. Update CRDs: `kubectl replace --force -f deploy/crds/`
4. Upgrade operator: `kubectl set image deployment/bindy-operator bindy=latest`
5. Clusters will automatically recreate instances with ownerReferences

**Why**: This change provides:
- Automatic garbage collection of instances when clusters are deleted
- Proper Kubernetes-native resource ownership semantics
- Cleaner architecture aligned with Kubernetes best practices
- Prevents orphaned instances

**Impact**:
- [x] Breaking change - requires instance recreation
- [x] CRD schema change
- [ ] Config change only
- [ ] Documentation only

### Changed

- `src/crd.rs`: Removed `cluster_ref` from `Bind9InstanceSpec`
- `src/crd.rs`: Kept `cluster_ref` in `Bind9InstanceStatus` (populated from ownerReference for reference)
- `src/reconcilers/bind9cluster.rs`: Set `ownerReference` when creating instances
- `src/reconcilers/bind9cluster.rs`: Use `find_instances_by_owner()` helper
- `src/reconcilers/clusterbind9provider.rs`: Use `find_instances_by_provider()` helper
- `src/reconcilers/bind9instance.rs`: Populate `status.clusterRef` from ownerReference
- `src/reconcilers/bind9instance.rs`: Add validation for ownerReference presence
- `src/reconcilers/dnszone.rs`: Update `find_all_primary_pods()` to use ownerReference filtering
- `src/reconcilers/dnszone.rs`: Update `find_all_secondary_pods()` to use ownerReference filtering
- `deploy/crds/bind9instances.crd.yaml`: Regenerated without `spec.clusterRef` field
- `examples/*.yaml`: Removed `clusterRef` from all examples
- `docs/src/`: Updated all documentation to reflect ownerReference model

### Added

- `src/reconcilers/helpers.rs`: Centralized helper functions for ownerReference filtering
  - `find_instances_by_owner()` - Find instances by namespace-scoped cluster
  - `find_instances_by_provider()` - Find instances by cluster-scoped provider
  - `is_owned_by_cluster()` - Check if instance is owned by cluster
  - `get_cluster_from_owner_refs()` - Extract cluster info from ownerReferences
- `src/reconcilers/helpers_tests.rs`: Unit tests for helper functions
- `docs/src/concepts/resource-ownership.md`: Documentation on ownerReference model

### Why

The `clusterRef` string field was a custom pattern that duplicated functionality already provided by Kubernetes' built-in `ownerReference` mechanism. Using `ownerReference` provides automatic garbage collection, proper lifecycle management, and aligns with Kubernetes best practices.

This change simplifies the codebase, improves resource cleanup, and provides a more intuitive ownership model for users familiar with Kubernetes.
```

---

### Phase 5: Testing

#### 5.1: Unit Tests

**Files to Update**:
- `src/reconcilers/bind9cluster_tests.rs`
- `src/reconcilers/bind9instance_tests.rs`
- `src/reconcilers/clusterbind9provider_tests.rs`
- `src/reconcilers/dnszone_tests.rs`

**Test Fixtures**: Update all test instances to include ownerReferences

```rust
// src/reconcilers/bind9instance_tests.rs

fn create_test_instance(cluster_name: &str, cluster_uid: &str) -> Bind9Instance {
    Bind9Instance {
        metadata: ObjectMeta {
            name: Some("test-instance".into()),
            namespace: Some("default".into()),
            uid: Some("instance-uid-123".into()),
            // NEW: Include ownerReference in test fixtures
            owner_references: Some(vec![OwnerReference {
                api_version: "bindy.firestoned.io/v1beta1".into(),
                kind: "Bind9Cluster".into(),
                name: cluster_name.into(),
                uid: cluster_uid.into(),
                operator: Some(true),
                block_owner_deletion: Some(true),
            }]),
            ..Default::default()
        },
        spec: Bind9InstanceSpec {
            // cluster_ref removed!
            role: ServerRole::Primary,
            replicas: 2,
            // ... rest of spec
        },
        status: None,
    }
}
```

**New Tests**:
- Test instance creation sets ownerReference correctly
- Test filtering instances by ownerReference
- Test validation rejects instances without ownerReference
- Test `status.clusterRef` is populated from ownerReference

#### 5.2: Integration Tests

**Files to Update**:
- `tests/simple_integration.rs`
- `tests/multi_tenancy_integration.rs`

**Test Scenarios**:
1. Create cluster → instances created with ownerReference
2. Delete cluster → instances automatically deleted (garbage collection)
3. Instance without ownerReference → rejected by reconciler
4. Cross-namespace ownership (ClusterBind9Provider)
5. `status.clusterRef` populated correctly

---

### Phase 6: Validation and Release

#### 6.1: Pre-Release Checklist

- [ ] All unit tests pass: `cargo test`
- [ ] All integration tests pass: `make kind-integration-test`
- [ ] CRD YAML files regenerated: `cargo run --bin crdgen`
- [ ] API documentation regenerated: `cargo run --bin crddoc > docs/src/reference/api.md`
- [ ] All examples validate: `./scripts/validate-examples.sh`
- [ ] Documentation built successfully: `make docs`
- [ ] CHANGELOG.md updated with breaking changes
- [ ] `cargo fmt`, `cargo clippy`, `cargo test` all pass

#### 6.2: Release Notes

**GitHub Release Notes**:
```markdown
# Breaking Changes in v1beta1

## ⚠️ Removed: Bind9Instance.spec.clusterRef

The `clusterRef` field has been removed from `Bind9Instance.spec`. Instances now use Kubernetes-native `ownerReference`.

### Why This Change?
- Automatic garbage collection when clusters are deleted
- Kubernetes-native ownership semantics
- Prevents orphaned instances

### How to Upgrade

1. **Backup** (optional):
   ```bash
   kubectl get bind9instance -A -o yaml > backup.yaml
   ```

2. **Delete instances** (clusters will recreate them):
   ```bash
   kubectl delete bind9instance --all -A
   ```

3. **Update CRDs**:
   ```bash
   kubectl replace --force -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml
   ```

4. **Upgrade operator**:
   ```bash
   kubectl set image deployment/bindy-operator \
     bindy=ghcr.io/firestoned/bindy:latest \
     -n bindy-system
   ```

5. **Verify**:
   ```bash
   kubectl get bind9instance -A
   kubectl get bind9instance <name> -o jsonpath='{.metadata.ownerReferences}'
   ```

See [CHANGELOG.md](./CHANGELOG.md) for full details.
```

---

## Risk Assessment

### High Risk
- **Existing deployments break without upgrade steps**: Mitigated by clear documentation
- **User confusion about breaking change**: Mitigated by detailed upgrade guide

### Medium Risk
- **Cross-namespace ownership confusion**: Document clearly that cluster-scoped providers work
- **Users unaware of automatic garbage collection**: Document clearly with warnings

### Low Risk
- **Performance impact**: ownerReference filtering is as fast as string comparison
- **Kubernetes version compatibility**: ownerReference is core Kubernetes (v1.0+)

---

## Success Criteria

- [ ] All reconcilers use ownerReference filtering correctly
- [ ] Automatic garbage collection works (delete cluster → instances deleted)
- [ ] `status.clusterRef` populated correctly from ownerReference
- [ ] All examples and documentation updated
- [ ] Integration tests pass with new architecture
- [ ] Users can upgrade with clear, step-by-step guide

---

## Timeline Estimate

- **Phase 1 (Helper Functions)**: 0.5 days
  - Helper functions: 0.5 days

- **Phase 2 (CRD Changes)**: 0.5 days
  - Update `src/crd.rs`: 0.25 days
  - Regenerate CRDs: 0.25 days

- **Phase 3 (Reconciler Updates)**: 2-3 days
  - Bind9Cluster reconciler: 0.5 days
  - ClusterBind9Provider reconciler: 0.5 days
  - Bind9Instance reconciler: 0.5 days
  - DNSZone reconciler: 1-1.5 days

- **Phase 4 (Examples/Docs)**: 1-2 days
  - Update examples: 0.5 days
  - Update documentation: 0.5-1 day
  - Update CHANGELOG: 0.5 days

- **Phase 5 (Testing)**: 2-3 days
  - Unit tests: 1-1.5 days
  - Integration tests: 1-1.5 days

- **Phase 6 (Validation/Release)**: 0.5 days
  - Final testing: 0.25 days
  - Release prep: 0.25 days

**Total**: 7-10 days (1.5-2 weeks)

---

## Open Questions

1. **Should we warn users about automatic garbage collection?**
   - Add annotation to clusters like `bindy.firestoned.io/cascade-delete: "true"`?
   - Or just document it clearly?

2. **Should we add admission webhook for validation?**
   - Webhook could reject instances without ownerReference
   - Adds deployment complexity
   - Alternative: Reconciler validation (current plan)

3. **How to handle manual instance creation?**
   - Should users be allowed to create instances manually without ownerReference?
   - Or should reconciler validation require ownerReference always?

---

## Related Documents

- `docs/roadmaps/simplify-zone-instance-relationship.md` - Original discussion
- `examples/README.md` - Example explanations (to be updated)
- `docs/src/concepts/resource-ownership.md` - ownerReference documentation (to be created)
