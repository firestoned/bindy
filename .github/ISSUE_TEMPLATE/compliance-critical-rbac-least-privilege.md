---
name: "[CRITICAL] Fix RBAC Least Privilege Violations"
about: Security - reduce overly permissive RBAC to minimum required permissions
title: '[Compliance C-2] Fix RBAC Permissions - Implement Least Privilege'
labels: compliance, critical, security, rbac, kubernetes
assignees: ''
---

## Severity: CRITICAL

**Compliance Frameworks:** PCI-DSS 7.1.2, SOX 404, Basel III Operational Risk

## Summary

ClusterRole grants overly broad permissions including `delete` on Secrets, ConfigMaps, and all CRDs. This violates the principle of least privilege and creates operational risk.

## Problem

**Location:** `deploy/rbac/role.yaml:1-84`

**Current State:**
```yaml
# Line 59-61: Secrets with delete permission
- apiGroups: [""]
  resources: ["secrets"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]  # ❌ delete is dangerous

# Line 55-57: ConfigMaps with delete permission
- apiGroups: [""]
  resources: ["configmaps"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]  # ❌ delete is dangerous

# Lines 8-44: All CRDs with delete permission (no scoping)
- apiGroups: ["bindy.firestoned.io"]
  resources: ["bind9instances", "dnszones", "*records"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]  # ❌ can delete ANY resource
```

**Risk:**
- Controller compromise could delete all secrets in cluster (data loss)
- Can delete DNS zones causing service disruption
- No scoping to controller-managed resources only
- Violates PCI-DSS least privilege requirement (7.1.2)
- Operational risk for Basel III compliance

**Impact:**
- ❌ **PCI-DSS 7.1.2:** Least privilege principle violated
- ❌ **SOX 404:** Change control - can delete production data without approval
- ❌ **Basel III:** Operational risk - controller compromise = infrastructure destruction
- ❌ **Blast Radius:** Single compromised pod can delete all DNS infrastructure

## Solution

### Phase 1: Remove Dangerous Permissions (Week 1)

1. **Remove `delete` permission from Secrets:**

```yaml
# deploy/rbac/role.yaml
- apiGroups: [""]
  resources: ["secrets"]
  verbs: ["get", "list", "watch"]  # ✅ Read-only for secrets
  # Removed: "create", "update", "patch", "delete"
```

**Rationale:** Controller only needs to READ RNDC secrets (generated externally). It should never delete secrets.

2. **Scope ConfigMap permissions to namespace:**

```yaml
# deploy/rbac/role.yaml
- apiGroups: [""]
  resources: ["configmaps"]
  verbs: ["get", "list", "watch", "create", "update", "patch"]
  # Removed: "delete"
  # Note: ConfigMaps are recreated on reconciliation, so delete not needed
```

### Phase 2: Implement Resource Scoping (Week 1-2)

3. **Scope CRD permissions using label selectors:**

Since Kubernetes RBAC doesn't support label selectors directly, implement controller-side validation:

```rust
// src/reconcilers/mod.rs

/// Validates that a resource is managed by this controller before allowing deletion
async fn validate_deletion_allowed(resource: &impl Resource) -> Result<bool> {
    // Check for managed-by label
    let labels = resource.metadata().labels.as_ref().ok_or_else(|| {
        anyhow!("Resource missing labels")
    })?;

    match labels.get("app.kubernetes.io/managed-by") {
        Some(manager) if manager == "bindy-controller" => Ok(true),
        _ => {
            warn!(
                resource = ?resource.metadata().name,
                "Attempted to delete resource not managed by bindy-controller"
            );
            Ok(false)
        }
    }
}
```

4. **Create separate admin role for destructive operations:**

```yaml
# deploy/rbac/role-admin.yaml (NEW FILE)
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-admin-role
  labels:
    app.kubernetes.io/name: bindy
    app.kubernetes.io/component: rbac
rules:
  # Admin-only permissions (manual use only, not bound to ServiceAccount)
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["bind9instances", "bind9clusters", "bind9globalclusters", "dnszones"]
    verbs: ["delete"]  # Only admins can delete via kubectl

  - apiGroups: [""]
    resources: ["secrets", "configmaps"]
    verbs: ["delete"]  # Only admins can delete
```

**Usage:**
```bash
# Admins bind this role manually when needed:
kubectl create rolebinding bindy-admin-binding \
  --clusterrole=bindy-admin-role \
  --user=admin@example.com \
  --namespace=dns-system
```

### Phase 3: Implement Namespace Scoping (Week 2)

5. **Change from ClusterRole to namespaced Role where possible:**

```yaml
# deploy/rbac/role-namespaced.yaml (NEW FILE)
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: bindy-role
  namespace: dns-system  # ✅ Scoped to dns-system namespace only
rules:
  # Same rules as ClusterRole, but scoped to namespace
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["bind9instances", "bind9instances/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # No delete
  # ... other rules ...
```

**Note:** `Bind9GlobalCluster` is cluster-scoped, so it still needs ClusterRole. Use separate roles:
- `bindy-role` (Role) - for namespaced resources
- `bindy-global-role` (ClusterRole) - only for `Bind9GlobalCluster`

### Phase 4: Add Audit Logging (Week 2-3)

6. **Add RBAC action logging:**

```rust
// src/reconcilers/mod.rs

/// Logs all resource deletions for audit trail
async fn delete_with_audit<K: Resource>(
    api: &Api<K>,
    name: &str,
    reason: &str,
) -> Result<()> {
    info!(
        resource_type = std::any::type_name::<K>(),
        resource_name = name,
        reason = reason,
        action = "DELETE",
        "Deleting resource (audit log)"
    );

    api.delete(name, &DeleteParams::default()).await?;

    // Record metric
    metrics::RESOURCE_DELETIONS_TOTAL
        .with_label_values(&[std::any::type_name::<K>(), "success"])
        .inc();

    Ok(())
}
```

## Updated RBAC Configuration

### Minimal Controller Role

```yaml
# deploy/rbac/role.yaml (UPDATED)
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-role
rules:
  # Bind9Instance and Bind9Cluster resources - NO DELETE
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["bind9instances", "bind9instances/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  - apiGroups: ["bindy.firestoned.io"]
    resources: ["bind9clusters", "bind9clusters/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  # Bind9GlobalCluster - cluster-scoped, NO DELETE
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["bind9globalclusters", "bind9globalclusters/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  # DNSZone resources - NO DELETE
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["dnszones", "dnszones/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  # Record resources - NO DELETE
  - apiGroups: ["bindy.firestoned.io"]
    resources:
      - "arecords"
      - "arecords/status"
      - "aaaarecords"
      - "aaaarecords/status"
      - "txtrecords"
      - "txtrecords/status"
      - "cnamerecords"
      - "cnamerecords/status"
      - "mxrecords"
      - "mxrecords/status"
      - "nsrecords"
      - "nsrecords/status"
      - "srvrecords"
      - "srvrecords/status"
      - "caarecords"
      - "caarecords/status"
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  # Kubernetes resources for managing BIND9 infrastructure
  - apiGroups: ["apps"]
    resources: ["deployments"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  - apiGroups: [""]
    resources: ["services"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  - apiGroups: [""]
    resources: ["configmaps"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  - apiGroups: [""]
    resources: ["secrets"]
    verbs: ["get", "list", "watch"]  # ✅ READ-ONLY for secrets

  - apiGroups: [""]
    resources: ["serviceaccounts"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]  # ✅ No delete

  - apiGroups: [""]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]  # ✅ Read-only

  - apiGroups: [""]
    resources: ["endpoints"]
    verbs: ["get", "list", "watch"]  # ✅ Read-only

  # Events for logging (no delete needed)
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["create", "patch"]

  # Leases for leader election (no delete needed)
  - apiGroups: ["coordination.k8s.io"]
    resources: ["leases"]
    verbs: ["get", "create", "update", "patch"]
```

## Testing Plan

### Functional Testing

1. **Test controller can still reconcile:**
   ```bash
   # Deploy with new RBAC
   kubectl apply -f deploy/rbac/role.yaml
   kubectl apply -f deploy/rbac/rolebinding.yaml

   # Create Bind9Instance
   kubectl apply -f examples/bind9-instance.yaml

   # Verify reconciliation succeeds
   kubectl wait --for=condition=Ready bind9instance/example --timeout=120s
   kubectl get bind9instance example -o yaml
   ```

2. **Test secret read-only access:**
   ```bash
   # Controller should read secrets
   kubectl logs -n dns-system deployment/bindy | grep "Reading secret"

   # But should NOT delete secrets (test manually via kubectl exec if needed)
   ```

3. **Test deletion protection:**
   ```bash
   # Admin can still delete via kubectl
   kubectl delete bind9instance example  # Should work

   # But verify controller doesn't auto-delete on conflicts
   # (Check controller logs for any "delete" operations)
   kubectl logs -n dns-system deployment/bindy | grep -i "delet"
   ```

### Security Testing

1. **Verify least privilege:**
   ```bash
   # Check what controller can do
   kubectl auth can-i delete secrets \
     --as=system:serviceaccount:dns-system:bindy
   # Should return "no"

   kubectl auth can-i delete dnszones \
     --as=system:serviceaccount:dns-system:bindy
   # Should return "no"

   kubectl auth can-i update dnszones \
     --as=system:serviceaccount:dns-system:bindy
   # Should return "yes"
   ```

2. **Test admin role separation:**
   ```bash
   # Admin user can delete
   kubectl auth can-i delete bind9instances \
     --as=admin@example.com
   # Should return "yes" (if bound to admin role)

   # Controller ServiceAccount cannot delete
   kubectl auth can-i delete bind9instances \
     --as=system:serviceaccount:dns-system:bindy
   # Should return "no"
   ```

## Documentation Updates

Required documentation:

1. **docs/operations/rbac.md** - Update RBAC documentation with new permissions
2. **docs/advanced/security.md** - Document least privilege implementation
3. **deploy/rbac/README.md** - Explain role separation (controller vs admin)
4. **CHANGELOG.md** - Document RBAC changes as breaking change
5. **docs/operations/admin-tasks.md** (NEW) - Document admin role usage for deletions

## Success Criteria

- [ ] `delete` permission removed from Secrets
- [ ] `delete` permission removed from ConfigMaps
- [ ] `delete` permission removed from all CRDs in controller role
- [ ] Separate admin role created for destructive operations
- [ ] Controller can still reconcile all resources (functional testing passes)
- [ ] `kubectl auth can-i` tests confirm least privilege
- [ ] Audit logging added for resource deletions
- [ ] Documentation updated
- [ ] All integration tests pass with new RBAC

## Migration Plan

**Week 1:**
- Day 1: Create new RBAC files (don't deploy yet)
- Day 2-3: Test new RBAC in staging/dev cluster
- Day 4-5: Update documentation

**Week 2:**
- Day 1: Deploy new RBAC to staging
- Day 2-3: Monitor for any permission errors in logs
- Day 4-5: Fix any issues found

**Week 3:**
- Day 1: Deploy to production (during maintenance window)
- Day 2-5: Monitor production for 1 week

**Rollback Plan:**
If issues found:
```bash
# Rollback to old RBAC
kubectl apply -f deploy/rbac/role.yaml.backup
kubectl rollout restart deployment/bindy -n dns-system
```

## Breaking Changes

⚠️ **BREAKING CHANGE:** Controller will no longer automatically delete resources.

**Impact:**
- Resource cleanup must be done manually via `kubectl delete`
- Orphaned resources (ConfigMaps, Services) may remain if parent is deleted
- Admins must bind admin role for bulk deletions

**Mitigation:**
- Document manual cleanup procedures
- Add finalizers to ensure cleanup via owner references
- Provide kubectl commands for bulk deletions

**Document in CHANGELOG.md:**
```markdown
## [YYYY-MM-DD] - BREAKING: RBAC Permissions Reduced

**BREAKING CHANGE - Security Enhancement:**

Controller RBAC permissions reduced to implement least privilege principle:

**Removed Permissions:**
- ❌ `delete` on Secrets (now read-only)
- ❌ `delete` on ConfigMaps
- ❌ `delete` on all CRDs (Bind9Instance, DNSZone, etc.)

**Impact:**
- Controller no longer auto-deletes resources
- Manual cleanup required for orphaned resources
- Admins must use `bindy-admin-role` for destructive operations

**Migration:**
1. Apply new RBAC: `kubectl apply -f deploy/rbac/`
2. For admin operations: `kubectl create rolebinding ... --clusterrole=bindy-admin-role`
3. See docs/operations/rbac.md for details

**Compliance:** Required for PCI-DSS 7.1.2 and SOX 404 compliance.
```

## Compliance Attestation

Once complete, update compliance documentation:

**File:** `docs/compliance/CONTROLS.md`
```markdown
### PCI-DSS 7.1.2 - Least Privilege

**Control:** Access rights limited to least privileges necessary

**Implementation:**
- Controller RBAC grants minimum permissions required
- No `delete` permission on Secrets, ConfigMaps, or CRDs
- Separate admin role for destructive operations
- Namespace scoping where possible

**Evidence:**
- `deploy/rbac/role.yaml` - Minimal controller permissions
- `kubectl auth can-i` test results
- Audit logs showing no unauthorized deletions

**Status:** ✅ Implemented (YYYY-MM-DD)
```

## Related Issues

- Related: #TBD (Audit logging for secret access)
- Blocks: PCI-DSS compliance certification
- Blocks: SOX 404 compliance certification

## References

- PCI-DSS v4.0: Requirement 7.1.2 - Access control systems restrict access
- Kubernetes RBAC: https://kubernetes.io/docs/reference/access-authn-authz/rbac/
- Principle of Least Privilege: https://csrc.nist.gov/glossary/term/least_privilege
- SOX 404: IT General Controls - Access Control
