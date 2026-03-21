# RBAC Security Analysis: ClusterRole vs Role Feasibility

**Date:** 2026-03-06
**Status:** Analysis Complete
**Priority:** HIGH (Security Finding)
**Author:** Erick Bourgeois

## Executive Summary

**Trivy Finding:**
- **KSV-0041 (CRITICAL)**: ClusterRole 'bindy-role' has cluster-wide access to Secrets
- **KSV-0056 (HIGH)**: ClusterRole has broad access to networking resources

**Question:** Can we move from ClusterRole to namespace-scoped Role to reduce security risk?

**Short Answer:** **NO** - Not without breaking multi-tenancy and ClusterBind9Provider support.

**Long Answer:** The operator's architecture requires cluster-wide permissions for multi-tenancy. However, we can implement **defense-in-depth strategies** to mitigate the security risks while maintaining functionality.

---

## Current Architecture Analysis

### 1. Resource Scope

| Resource Type | Scope | Cross-Namespace Access |
|---|---|---|
| `ClusterBind9Provider` | Cluster-scoped | N/A (cluster-wide) |
| `Bind9Cluster` | Namespaced | Yes (watched across all namespaces) |
| `Bind9Instance` | Namespaced | Yes (watched across all namespaces) |
| `DNSZone` | Namespaced | Yes (watched across all namespaces) |
| DNS Records (A, AAAA, etc.) | Namespaced | Yes (watched across all namespaces) |
| Secrets (RNDC keys) | Namespaced | **No** (only accessed within namespace) |
| Services | Namespaced | **No** (only created/managed within namespace) |
| ConfigMaps | Namespaced | **No** (only created/managed within namespace) |
| Deployments | Namespaced | **No** (only created/managed within namespace) |

### 2. Multi-Tenancy Requirements

The operator is **explicitly designed for multi-tenancy**:

**Platform-Managed DNS:**
```yaml
# team-web namespace
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: web-zone
  namespace: team-web
spec:
  clusterProviderRef: production-dns  # References cluster-scoped provider
```

**Tenant-Managed DNS:**
```yaml
# team-api namespace
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: api-dns
  namespace: team-api
spec:
  primary:
    replicas: 1
```

The operator **MUST** watch resources across all namespaces to support both models.

### 3. Code Evidence

**Operator watches ALL namespaces:**
```rust
// src/main.rs:586-653
let cluster_providers_api: Api<ClusterBind9Provider> = Api::all(client.clone());
let clusters_api: Api<Bind9Cluster> = Api::all(client.clone());
let instances_api: Api<Bind9Instance> = Api::all(client.clone());
```

**Secrets accessed ONLY within namespace:**
```rust
// src/reconcilers/bind9instance/resources.rs:176
let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
secret_api.get(&secret_name).await.ok()
```

**Key Insight:** While the operator has **ClusterRole permissions** for Secrets, the code **ONLY accesses Secrets in the namespace** where the Bind9Instance exists. This is a code-level safeguard, but not enforced by RBAC.

---

## Why ClusterRole is Required

### 1. ClusterBind9Provider (Cluster-Scoped Resource)

```rust
// src/crd.rs:3044
#[kube(
    kind = "ClusterBind9Provider",
    // NOTE: No 'namespaced' attribute = cluster-scoped
)]
```

**Cluster-scoped resources REQUIRE ClusterRole permissions.** There is no way to manage them with a namespaced Role.

### 2. Multi-Tenancy Support

The operator must watch resources in **multiple namespaces simultaneously**:
- Team A creates DNSZone in `team-a` namespace
- Team B creates Bind9Cluster in `team-b` namespace
- Platform team manages ClusterBind9Provider (cluster-scoped)

**All must be reconciled by the same operator instance.**

### 3. Cross-Namespace Resource Discovery

**DNSZones** can reference:
- `clusterRef`: Bind9Cluster in the **same namespace**
- `clusterProviderRef`: ClusterBind9Provider (**cluster-scoped**)

The operator must be able to:
1. Watch DNSZone in any namespace
2. Resolve references to ClusterBind9Provider (cluster-scoped)
3. Resolve references to Bind9Cluster in the same namespace
4. Create child resources (Deployments, Services, Secrets) in the parent's namespace

---

## Alternative Approaches (Why They Don't Work)

### Option 1: Namespace-Scoped Deployment (❌ Not Viable)

**Approach:** Deploy one operator instance per namespace with a namespaced Role.

**Why it fails:**
1. **Breaks ClusterBind9Provider**: Cluster-scoped resources cannot be managed by namespace-scoped Roles
2. **Loses multi-tenancy**: Each operator instance can only see its own namespace
3. **Operational complexity**: N operator deployments for N namespaces
4. **Resource overhead**: Multiple operator pods, multiple leader elections
5. **Breaking change**: Removes core platform DNS functionality

**Verdict:** Not feasible without a complete redesign.

---

### Option 2: Aggregated ClusterRoles (⚠️ Partial Improvement)

**Approach:** Split permissions into multiple ClusterRoles and aggregate them.

```yaml
# CRD management ClusterRole
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-crds
rules:
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["*"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

---
# Kubernetes resources ClusterRole
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-k8s-resources
rules:
  - apiGroups: ["", "apps"]
    resources: ["deployments", "services", "configmaps", "secrets", "serviceaccounts"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

---
# Aggregated ClusterRole
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-role
  labels:
    rbac.bindy.io/aggregate: "true"
aggregationRule:
  clusterRoleSelectors:
    - matchLabels:
        rbac.bindy.io/component: "bindy"
rules: []  # Rules are auto-populated by aggregation
```

**Benefits:**
- Better separation of concerns
- Easier to audit and understand
- Can be extended by cluster admins

**Limitations:**
- Still cluster-wide permissions
- Doesn't solve the Trivy finding (still ClusterRole)
- Adds complexity

**Verdict:** Improves maintainability but doesn't address the security concern.

---

### Option 3: Hybrid ClusterRole + Namespace Roles (⚠️ Complex)

**Approach:** Use ClusterRole for CRDs, namespace-specific Roles for Secrets.

**Problems:**
1. **Operator needs consistent permissions**: Can't have different permissions per namespace
2. **ServiceAccount bound to single Role**: ClusterRoleBinding OR RoleBinding, not both (you can have multiple, but it's complex)
3. **Dynamic namespace discovery**: Operator doesn't know which namespaces exist ahead of time
4. **Deployment complexity**: Must create RoleBindings in every namespace where operator might work

**Example:**
```yaml
# ClusterRole for CRDs
kind: ClusterRole
metadata:
  name: bindy-crds
rules:
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["*"]
    verbs: ["*"]

---
# Role for Secrets (per namespace)
kind: Role
metadata:
  name: bindy-secrets
  namespace: team-web  # Must be created in every namespace!
rules:
  - apiGroups: [""]
    resources: ["secrets"]
    verbs: ["get", "list", "watch", "create", "patch", "delete"]

---
# ClusterRoleBinding
kind: ClusterRoleBinding
metadata:
  name: bindy-crds
roleRef:
  kind: ClusterRole
  name: bindy-crds
subjects:
  - kind: ServiceAccount
    name: bindy
    namespace: bindy-system

---
# RoleBinding (per namespace)
kind: RoleBinding
metadata:
  name: bindy-secrets
  namespace: team-web  # Must be created in every namespace!
roleRef:
  kind: Role
  name: bindy-secrets
subjects:
  - kind: ServiceAccount
    name: bindy
    namespace: bindy-system
```

**Operational Challenges:**
- Must pre-create Role + RoleBinding in every namespace
- Operator can't auto-discover new namespaces
- Team creates namespace → operator can't create Bind9Cluster until admin adds RoleBinding
- Breaks self-service multi-tenancy

**Verdict:** Technically possible but operationally impractical.

---

## Recommended Mitigation Strategies

Since **ClusterRole is architecturally required**, we should implement **defense-in-depth** security measures:

### 1. Minimize Permissions (Principle of Least Privilege)

**Current:**
```yaml
- apiGroups: [""]
  resources: ["secrets"]
  verbs: ["get", "list", "watch", "create", "patch", "delete"]
```

**Improvement:** Already minimal. Removed `update` verb (patch is sufficient for annotations).

✅ **Already implemented**: Line 132 in `deploy/rbac/role.yaml`

---

### 2. Audit Secret Access Patterns

**Add structured logging for all Secret operations:**

```rust
// Log every Secret access with context
info!(
    secret_name = %secret_name,
    namespace = %namespace,
    instance_name = %instance_name,
    operation = "get",
    "Accessing RNDC Secret"
);
```

**Benefits:**
- Audit trail for compliance
- Detect anomalous access patterns
- Helps with forensics if compromise occurs

---

### 3. Add Runtime Secret Access Validation

**Implement namespace isolation check:**

```rust
// src/reconcilers/bind9instance/resources.rs
async fn get_rndc_secret(
    client: &Client,
    namespace: &str,
    secret_name: &str,
    instance_namespace: &str,
) -> Result<Secret> {
    // CRITICAL: Validate namespace isolation
    if namespace != instance_namespace {
        return Err(anyhow!(
            "Security violation: Attempted cross-namespace Secret access. \
             Secret namespace: {}, Instance namespace: {}",
            namespace,
            instance_namespace
        ));
    }

    let secret_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    secret_api.get(secret_name).await.map_err(Into::into)
}
```

**Benefits:**
- Code-level enforcement of namespace isolation
- Fails fast on programming errors
- Defense against future refactoring mistakes

---

### 4. Network Policies for Operator Pod

**Restrict operator pod network access:**

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: bindy-operator-netpol
  namespace: bindy-system
spec:
  podSelector:
    matchLabels:
      app: bindy
  policyTypes:
    - Ingress
    - Egress
  ingress:
    - from:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: bindy-system
      ports:
        - protocol: TCP
          port: 8080  # Metrics
  egress:
    # Allow Kubernetes API server
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: kube-system
      ports:
        - protocol: TCP
          port: 443
    # Allow DNS
    - to:
        - namespaceSelector: {}
      ports:
        - protocol: UDP
          port: 53
```

**Benefits:**
- Limits blast radius if operator is compromised
- Prevents lateral movement
- Kubernetes best practice

---

### 5. OPA/Gatekeeper Policies

**Add policy enforcement for Secret access:**

```rego
# Gatekeeper ConstraintTemplate
package k8ssecretaccess

violation[{"msg": msg}] {
  input.review.kind.kind == "Secret"
  input.review.operation == "GET"
  input.review.userInfo.username == "system:serviceaccount:bindy-system:bindy"
  input.review.namespace != input.review.object.metadata.namespace
  msg := sprintf("bindy operator attempted cross-namespace Secret access: %v", [input.review.object.metadata.name])
}
```

**Benefits:**
- Policy-as-code enforcement
- Cluster-wide audit and enforcement
- Independent of operator code

---

### 6. Pod Security Standards

**Ensure operator pod is hardened:**

```yaml
# Already implemented in deploy/operator/deployment.yaml
securityContext:
  allowPrivilegeEscalation: false
  capabilities:
    drop:
      - ALL
  readOnlyRootFilesystem: true
  runAsNonRoot: true
  runAsUser: 65534  # nobody
```

✅ **Already implemented**: Lines 68-74 in `deploy/operator/deployment.yaml`

---

### 7. Documentation and Training

**Add prominent security documentation:**

1. **Threat Model**: Document that operator has cluster-wide Secret access
2. **Mitigation Measures**: List all defense-in-depth controls
3. **Audit Procedures**: How to audit Secret access logs
4. **Incident Response**: What to do if operator is compromised

---

## Trivy Finding Remediation

**For KSV-0041 (CRITICAL):** ClusterRole has cluster-wide Secret access

**Remediation Options:**

### Option A: Accept Risk (Document + Mitigate)

Create `.trivy/policies/ksv-0041-exception.rego`:

```rego
package builtin.kubernetes.KSV041

# Exception for bindy operator: Requires cluster-wide Secret access for multi-tenancy
exception[msg] {
  input.metadata.name == "bindy-role"
  msg := "bindy operator requires cluster-wide Secret access for RNDC key management across namespaces. Risk mitigated by: (1) Code-level namespace isolation, (2) Runtime validation, (3) Audit logging, (4) Network policies"
}
```

**Justification Document:** `docs/security/ksv-0041-justification.md`

---

### Option B: Use Trivy Ignore with Explanation

```yaml
# deploy/rbac/role.yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-role
  annotations:
    trivy.ignore.ksv-0041: |
      Multi-tenancy requires cluster-wide permissions. Operator code enforces
      namespace isolation for Secret access. See docs/security/rbac-security-analysis.md
rules:
  - apiGroups: [""]
    resources: ["secrets"]
    verbs: ["get", "list", "watch", "create", "patch", "delete"]
```

---

### Option C: Architectural Redesign (Future)

**Long-term:** Consider a different architecture:

1. **Split operator into platform and tenant components:**
   - **Platform Operator**: Manages ClusterBind9Provider (ClusterRole)
   - **Tenant Operator**: Manages Bind9Cluster per namespace (Role)

2. **Benefits:**
   - Platform operator has minimal permissions (CRDs only, no Secrets)
   - Tenant operators have namespace-scoped permissions
   - Better security isolation

3. **Challenges:**
   - Breaking change for users
   - Increased operational complexity
   - Two deployments to manage

**Verdict:** Worth exploring for v2.0 but not feasible for current release.

---

## Recommendations (Prioritized)

### Immediate (Security Hardening)

1. ✅ **Keep ClusterRole**: Required for multi-tenancy
2. ✅ **Document justification**: Create `docs/security/ksv-0041-justification.md`
3. ⬜ **Add runtime validation**: Namespace isolation check in Secret access code
4. ⬜ **Add audit logging**: Structured logs for all Secret operations
5. ⬜ **Add Trivy exception**: Document risk acceptance with mitigation measures

### Short-term (Defense in Depth)

6. ⬜ **Network policies**: Restrict operator pod network access
7. ⬜ **OPA/Gatekeeper policies**: Policy enforcement for Secret access
8. ⬜ **Security documentation**: Threat model and incident response

### Long-term (Architectural)

9. ⬜ **Explore split operator model**: Platform vs Tenant operators (v2.0)
10. ⬜ **Aggregated ClusterRoles**: Better RBAC granularity

---

## Conclusion

**Trivy Finding: ACCEPT WITH MITIGATION**

The ClusterRole with cluster-wide Secret access is **architecturally necessary** for Bindy's multi-tenancy design. However, we can implement **defense-in-depth security controls** to mitigate the risk:

1. **Code-level enforcement**: Secrets only accessed within parent namespace
2. **Runtime validation**: Guard clauses prevent cross-namespace access
3. **Audit logging**: All Secret operations logged for forensics
4. **Network policies**: Limit operator pod network access
5. **Policy enforcement**: OPA/Gatekeeper rules for additional safety
6. **Documentation**: Transparent security model for users

**The risk is acceptable given:**
- Multi-tenancy is a core feature
- ClusterBind9Provider requires cluster-scoped permissions
- Multiple layers of defense protect against misuse
- Operator code enforces namespace isolation

**Next Steps:**
1. Implement runtime validation (Priority 1)
2. Add audit logging (Priority 1)
3. Document security justification (Priority 1)
4. Add Trivy exception with explanation (Priority 2)
5. Implement network policies (Priority 2)

---

## Related Files

- `deploy/rbac/role.yaml` - Current ClusterRole definition
- `deploy/rbac/rolebinding.yaml` - ClusterRoleBinding
- `src/reconcilers/bind9instance/resources.rs` - Secret access code
- `examples/multi-tenancy.yaml` - Multi-tenancy example
- `docs/src/guide/multi-tenancy.md` - Multi-tenancy guide

## References

- [Kubernetes RBAC Authorization](https://kubernetes.io/docs/reference/access-authn-authz/rbac/)
- [Pod Security Standards](https://kubernetes.io/docs/concepts/security/pod-security-standards/)
- [Trivy KSV-0041](https://avd.aquasec.com/misconfig/ksv-0041)
- [Defense in Depth](https://en.wikipedia.org/wiki/Defense_in_depth_(computing))
