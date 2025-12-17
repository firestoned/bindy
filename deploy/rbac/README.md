# Bindy RBAC Configuration

This directory contains Role-Based Access Control (RBAC) configurations for the Bindy DNS Controller.

## Overview

Bindy implements **least privilege** RBAC to comply with PCI-DSS 7.1.2, SOX 404, and Basel III requirements.

**Key Principles:**
- Controller ServiceAccount has **minimal permissions**
- **No delete permissions** on Secrets, ConfigMaps, or CRDs
- Separate **admin role** for destructive operations
- Secrets are **read-only** for the controller

---

## Roles

### 1. `bindy-role` (Controller Role)

**File:** [`role.yaml`](role.yaml)
**Type:** ClusterRole
**Bound To:** ServiceAccount `bindy` in `dns-system` namespace

**Purpose:** Minimal permissions required for controller operation.

**Permissions:**
- ✅ **Read/Write** on all Bindy CRDs (Bind9Instance, DNSZone, Records)
- ✅ **Read/Write** on Kubernetes resources (Deployments, Services, ConfigMaps, ServiceAccounts)
- ✅ **Read-only** on Secrets (PCI-DSS 7.1.2 compliance)
- ❌ **NO delete** on any resources (least privilege)

**Why No Delete?**
- **Security:** Controller compromise cannot delete infrastructure
- **Compliance:** PCI-DSS 7.1.2 requires least privilege
- **Safety:** Prevents accidental data loss
- **Audit:** Deletions require human administrator approval

**Apply:**
```bash
kubectl apply -f deploy/rbac/role.yaml
kubectl apply -f deploy/rbac/rolebinding.yaml
```

---

### 2. `bindy-admin-role` (Admin Role)

**File:** [`role-admin.yaml`](role-admin.yaml)
**Type:** ClusterRole
**Bound To:** **Human administrators only** (NOT ServiceAccount)

**Purpose:** Administrative permissions for destructive operations.

**Permissions:**
- ✅ **Delete** on all Bindy CRDs
- ✅ **Delete** on Kubernetes resources (Deployments, Services, ConfigMaps)
- ✅ **Delete** on Secrets (use with extreme caution)

**⚠️ WARNING:**
- **NEVER** bind this role to the controller ServiceAccount
- Only bind to **human administrators** who need delete permissions
- Use **temporary bindings** for specific tasks
- **Audit all usage** of this role

**Apply:**
```bash
# Create the role (but don't bind it yet)
kubectl apply -f deploy/rbac/role-admin.yaml

# Bind to a specific admin user (example)
kubectl create rolebinding bindy-admin-binding \
  --clusterrole=bindy-admin-role \
  --user=admin@example.com \
  --namespace=dns-system

# Or bind temporarily for a specific task
kubectl create rolebinding bindy-admin-temp \
  --clusterrole=bindy-admin-role \
  --user=$(kubectl config view --minify -o jsonpath='{.contexts[0].context.user}') \
  --namespace=dns-system

# Delete the binding after task completion
kubectl delete rolebinding bindy-admin-temp --namespace=dns-system
```

---

## Usage Examples

### Controller Operations (Normal)

The controller can create, update, and patch resources:

```bash
# Controller can create resources
kubectl apply -f examples/bind9-instance.yaml

# Controller can update resources
kubectl patch bind9instance example --type=merge -p '{"spec":{"replicas":3}}'

# Controller can read secrets
kubectl logs -n dns-system deployment/bindy | grep "Reading secret"
```

### Admin Operations (Destructive)

Admins must bind the admin role for deletions:

```bash
# 1. Bind admin role to your user
kubectl create rolebinding my-admin-binding \
  --clusterrole=bindy-admin-role \
  --user=$(kubectl config view --minify -o jsonpath='{.contexts[0].context.user}') \
  --namespace=dns-system

# 2. Perform admin operations
kubectl delete bind9instance example
kubectl delete dnszone example.com

# 3. Remove admin binding when done
kubectl delete rolebinding my-admin-binding --namespace=dns-system
```

---

## Verification

### Test Controller Permissions

Verify the controller has **minimum required** permissions:

```bash
# Controller can read/write CRDs
kubectl auth can-i get bind9instances \
  --as=system:serviceaccount:dns-system:bindy
# Expected: yes

kubectl auth can-i update bind9instances \
  --as=system:serviceaccount:dns-system:bindy
# Expected: yes

# Controller CANNOT delete CRDs
kubectl auth can-i delete bind9instances \
  --as=system:serviceaccount:dns-system:bindy
# Expected: no

# Controller can ONLY READ secrets
kubectl auth can-i get secrets \
  --as=system:serviceaccount:dns-system:bindy
# Expected: yes

kubectl auth can-i delete secrets \
  --as=system:serviceaccount:dns-system:bindy
# Expected: no

kubectl auth can-i update secrets \
  --as=system:serviceaccount:dns-system:bindy
# Expected: no
```

### Test Admin Permissions

Verify admin role has delete permissions:

```bash
# Assume you've bound the admin role to your user

# Admin can delete CRDs
kubectl auth can-i delete bind9instances
# Expected: yes

# Admin can delete secrets
kubectl auth can-i delete secrets --namespace=dns-system
# Expected: yes
```

---

## Compliance

### PCI-DSS 7.1.2 - Least Privilege

**Requirement:** Restrict access to system components and cardholder data to only those individuals whose job requires such access.

**Implementation:**
- Controller has **read-only** access to Secrets
- Controller **cannot delete** any resources
- Destructive operations require **separate admin role**
- Admin role **not bound to ServiceAccount**

**Evidence:**
- `deploy/rbac/role.yaml` - Minimal controller permissions
- `kubectl auth can-i` test results (see Verification section)
- Audit logs showing no unauthorized deletions

---

### SOX 404 - Change Control

**Requirement:** IT General Controls for change management require separation of duties and approval for destructive operations.

**Implementation:**
- Controller automation has **no delete permissions**
- Deletions require **human administrator** with separate role
- Two-person review for RBAC changes (PR process)
- Audit trail via Kubernetes RBAC logs

**Evidence:**
- Separate `bindy-role` and `bindy-admin-role`
- GitHub PR approval for RBAC changes
- Kubernetes audit logs for admin role bindings

---

### Basel III - Operational Risk

**Requirement:** Minimize operational risk from system compromise or insider threats.

**Implementation:**
- **Blast radius reduction**: Compromised controller cannot delete infrastructure
- **Defense in depth**: Multiple layers (no delete + read-only secrets + owner references)
- **Audit trail**: All destructive operations require admin role binding (logged)

**Evidence:**
- RBAC configuration limiting controller permissions
- Testing showing controller compromise limited impact
- Incident response plan for controller compromise

---

## Migration from Previous RBAC

**⚠️ BREAKING CHANGE:** Controller no longer has delete permissions.

### What Changed

| Resource | Before | After | Impact |
|----------|--------|-------|--------|
| Secrets | Read/Write/Delete | **Read-only** | Controller cannot modify/delete secrets |
| ConfigMaps | Read/Write/Delete | Read/Write | Controller cannot delete ConfigMaps |
| CRDs | Read/Write/Delete | Read/Write | Controller cannot delete CRDs |
| Deployments | Read/Write/Delete | Read/Write | Controller cannot delete Deployments |

### Migration Steps

1. **Backup current RBAC:**
   ```bash
   kubectl get clusterrole bindy-role -o yaml > role-backup.yaml
   ```

2. **Apply new RBAC:**
   ```bash
   kubectl apply -f deploy/rbac/role.yaml
   kubectl apply -f deploy/rbac/role-admin.yaml
   ```

3. **Verify permissions:**
   ```bash
   # Run verification tests (see Verification section)
   kubectl auth can-i delete secrets --as=system:serviceaccount:dns-system:bindy
   # Should return: no
   ```

4. **Test functionality:**
   ```bash
   # Create a test instance
   kubectl apply -f examples/bind9-instance.yaml

   # Verify controller can reconcile
   kubectl wait --for=condition=Ready bind9instance/example --timeout=120s

   # Verify status updated
   kubectl get bind9instance example -o yaml
   ```

5. **Handle orphaned resources:**
   If you need to clean up orphaned resources (ConfigMaps, Services, etc.):
   ```bash
   # Bind admin role temporarily
   kubectl create rolebinding cleanup-binding \
     --clusterrole=bindy-admin-role \
     --user=$(kubectl config view --minify -o jsonpath='{.contexts[0].context.user}') \
     --namespace=dns-system

   # Delete orphaned resources
   kubectl delete configmap orphaned-cm --namespace=dns-system

   # Remove admin binding
   kubectl delete rolebinding cleanup-binding --namespace=dns-system
   ```

### Rollback

If issues occur, rollback to previous RBAC:

```bash
kubectl apply -f role-backup.yaml
kubectl rollout restart deployment/bindy -n dns-system
```

---

## Troubleshooting

### Error: "secrets is forbidden"

**Symptom:** Controller logs show permission errors accessing secrets.

**Cause:** RoleBinding not applied or ServiceAccount mismatch.

**Solution:**
```bash
# Verify RoleBinding exists
kubectl get rolebinding bindy-binding -n dns-system

# Verify it references correct ServiceAccount
kubectl get rolebinding bindy-binding -n dns-system -o yaml

# Reapply if needed
kubectl apply -f deploy/rbac/rolebinding.yaml
```

### Error: "cannot delete bind9instance"

**Symptom:** `kubectl delete bind9instance` fails with permission error.

**Cause:** You don't have the admin role bound.

**Solution:**
```bash
# Bind admin role to your user
kubectl create rolebinding my-admin \
  --clusterrole=bindy-admin-role \
  --user=$(kubectl config view --minify -o jsonpath='{.contexts[0].context.user}') \
  --namespace=dns-system

# Try delete again
kubectl delete bind9instance example

# Remove binding when done
kubectl delete rolebinding my-admin --namespace=dns-system
```

### Controller Cannot Create Resources

**Symptom:** Controller fails to create Deployments/Services/ConfigMaps.

**Cause:** RBAC may be missing create/update/patch permissions.

**Solution:**
```bash
# Verify controller has create permission
kubectl auth can-i create deployments \
  --as=system:serviceaccount:dns-system:bindy \
  --namespace=dns-system

# If "no", reapply role
kubectl apply -f deploy/rbac/role.yaml
```

---

## Security Best Practices

1. **Never bind admin role to ServiceAccount:**
   ```bash
   # ❌ NEVER DO THIS
   kubectl create clusterrolebinding bad-binding \
     --clusterrole=bindy-admin-role \
     --serviceaccount=dns-system:bindy
   ```

2. **Use temporary admin bindings:**
   ```bash
   # ✅ GOOD: Temporary binding for specific task
   kubectl create rolebinding temp-admin \
     --clusterrole=bindy-admin-role \
     --user=$USER \
     --namespace=dns-system

   # Perform admin task
   kubectl delete bind9instance example

   # Immediately remove binding
   kubectl delete rolebinding temp-admin --namespace=dns-system
   ```

3. **Audit admin role usage:**
   ```bash
   # List all bindings of admin role
   kubectl get rolebindings,clusterrolebindings --all-namespaces \
     -o json | jq '.items[] | select(.roleRef.name=="bindy-admin-role")'
   ```

4. **Enable Kubernetes audit logging:**
   - Log all RBAC changes
   - Log all secret access
   - Alert on admin role bindings

---

## References

- [Kubernetes RBAC Documentation](https://kubernetes.io/docs/reference/access-authn-authz/rbac/)
- [PCI-DSS Requirement 7.1.2](https://www.pcisecuritystandards.org/)
- [NIST Least Privilege Principle](https://csrc.nist.gov/glossary/term/least_privilege)
- [Bindy Security Policy](../../SECURITY.md)
