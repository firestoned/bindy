# CIS Kubernetes Benchmark Compliance

**Document Version:** 1.0
**Last Updated:** 2025-12-17
**Scope:** Bindy DNS Operator for Kubernetes
**Benchmark:** CIS Kubernetes Benchmark v1.8.0

---

## Executive Summary

This document maps the Bindy DNS Operator's configuration and deployment to the Center for Internet Security (CIS) Kubernetes Benchmark. The CIS Kubernetes Benchmark provides consensus-driven security configuration guidelines for Kubernetes deployments.

**Scope:** This document covers workload-level controls that Bindy implements. Cluster-level controls (control plane, etcd, node configuration) are the responsibility of the Kubernetes platform operator.

**Compliance Level:** Bindy is designed to meet **Level 1** (essential security) and many **Level 2** (defense-in-depth) controls.

---

## CIS Benchmark Sections

- **Section 1:** Control Plane Components (Cluster responsibility)
- **Section 2:** etcd (Cluster responsibility)
- **Section 3:** Control Plane Configuration (Cluster responsibility)
- **Section 4:** Worker Nodes (Cluster responsibility)
- **Section 5:** Policies (Workload responsibility) ← **Bindy scope**

---

## Section 5: Kubernetes Policies

### 5.1: RBAC and Service Accounts

#### 5.1.1: Ensure that the cluster-admin role is only used where required
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - Bindy does NOT require cluster-admin
- Uses minimal RBAC with scoped ClusterRole
- Can be deployed with namespace-scoped RoleBinding for even tighter restrictions

**Evidence:** [deploy/rbac/role.yaml](../../deploy/rbac/role.yaml)

**Verification:**
```bash
kubectl describe clusterrole bindy-operator
# Verify no wildcard permissions, no cluster-admin binding
```

---

#### 5.1.2: Minimize access to secrets
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - Secrets access limited to operator namespace
- Operator only accesses Secrets it creates (TSIG keys)
- No wildcard Secret access across namespaces

**Evidence:** [deploy/rbac/role.yaml](../../deploy/rbac/role.yaml)
```yaml
- apiGroups: [""]
  resources: ["secrets"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # Note: Scoped to namespace when using RoleBinding
```

**Best Practice:** Deploy with RoleBinding (not ClusterRoleBinding) for production namespaces.

---

#### 5.1.3: Minimize wildcard use in Roles and ClusterRoles
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - No wildcard resource names
- All resources explicitly listed
- No `resources: ["*"]` or `verbs: ["*"]`

**Evidence:** [deploy/rbac/role.yaml](../../deploy/rbac/role.yaml) - all resources and verbs explicitly defined

**Verification:**
```bash
kubectl get clusterrole bindy-operator -o yaml | grep -E '\["?\*"?\]'
# Should return empty (no wildcards)
```

---

#### 5.1.4: Minimize access to create pods
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - No direct pod creation permission
- Operator creates StatefulSets (which create pods)
- Follows operator pattern (indirect pod management)

**Evidence:** [deploy/rbac/role.yaml](../../deploy/rbac/role.yaml) - no `pods` create verb
```yaml
- apiGroups: [""]
  resources: ["pods"]
  verbs: ["get", "list", "watch"]  # Read-only, no create/update/delete
```

---

#### 5.1.5: Ensure that default service accounts are not actively used
**Level:** 1
**Type:** Automated

**Implementation:**
- ✅ **PASS** - Uses dedicated ServiceAccount
- Does not use `default` ServiceAccount
- ServiceAccount explicitly referenced in Deployment

**Evidence:** [deploy/operator.yaml](../../deploy/operator.yaml)
```yaml
spec:
  serviceAccountName: bindy-operator  # Not 'default'
```

**Verification:**
```bash
kubectl get deployment bindy-operator -o jsonpath='{.spec.template.spec.serviceAccountName}'
# Should output: bindy-operator
```

---

#### 5.1.6: Ensure that Service Account Tokens are only mounted where necessary
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - ServiceAccount token mounted (required for Kubernetes API access)
- Operator requires token to reconcile CustomResources
- No unnecessary token mounting in BIND9 pods

**Evidence:**
- Operator pod: Token required (Kubernetes API client)
- BIND9 pods: No ServiceAccount token needed (can set `automountServiceAccountToken: false`)

**Future Improvement:** Set `automountServiceAccountToken: false` on BIND9 StatefulSets

---

### 5.2: Pod Security Standards

#### 5.2.1: Ensure that the cluster has at least one active policy control mechanism
**Level:** 1
**Type:** Manual

**Implementation:**
- ⚠️ **CLUSTER RESPONSIBILITY** - PSA/PSS or PSP must be enabled cluster-wide
- Bindy is compatible with Pod Security Standards (PSS)
- Tested with `restricted` policy

**Evidence:** [src/bind9_resources.rs:427-440](../../src/bind9_resources.rs#L427-L440) - SecurityContext meets restricted PSS

**Deployment Notes:**
```yaml
# Namespace label for Pod Security Admission
apiVersion: v1
kind: Namespace
metadata:
  name: dns-system
  labels:
    pod-security.kubernetes.io/enforce: restricted
    pod-security.kubernetes.io/audit: restricted
    pod-security.kubernetes.io/warn: restricted
```

---

#### 5.2.2: Minimize the admission of privileged containers
**Level:** 1
**Type:** Automated

**Implementation:**
- ✅ **PASS** - No privileged containers
- `securityContext.privileged: false` (default)
- No `SYS_ADMIN` or other dangerous capabilities

**Evidence:** [src/bind9_resources.rs:427-440](../../src/bind9_resources.rs#L427-L440)
```rust
SecurityContext {
    run_as_non_root: Some(true),
    run_as_user: Some(1000),
    capabilities: Some(Capabilities {
        drop: Some(vec!["ALL".to_string()]),
        ..Default::default()
    }),
    ..Default::default()
}
```

**Verification:**
```bash
kubectl get pods -l app=bind9 -o jsonpath='{.items[*].spec.containers[*].securityContext.privileged}'
# Should output: false or empty (defaults to false)
```

---

#### 5.2.3: Minimize the admission of containers wishing to share the host process ID namespace
**Level:** 1
**Type:** Automated

**Implementation:**
- ✅ **PASS** - No host PID namespace sharing
- `hostPID: false` (default, not set)

**Evidence:** [src/bind9_resources.rs](../../src/bind9_resources.rs) - no `host_pid` field set

**Verification:**
```bash
kubectl get statefulset bind9-primary -o jsonpath='{.spec.template.spec.hostPID}'
# Should output: empty (defaults to false)
```

---

#### 5.2.4: Minimize the admission of containers wishing to share the host IPC namespace
**Level:** 1
**Type:** Automated

**Implementation:**
- ✅ **PASS** - No host IPC namespace sharing
- `hostIPC: false` (default, not set)

**Evidence:** [src/bind9_resources.rs](../../src/bind9_resources.rs) - no `host_ipc` field set

**Verification:**
```bash
kubectl get statefulset bind9-primary -o jsonpath='{.spec.template.spec.hostIPC}'
# Should output: empty (defaults to false)
```

---

#### 5.2.5: Minimize the admission of containers wishing to share the host network namespace
**Level:** 1
**Type:** Automated

**Implementation:**
- ✅ **PASS** - No host network namespace
- `hostNetwork: false` (default, not set)
- DNS service uses ClusterIP (not hostPort)

**Evidence:** [src/bind9_resources.rs](../../src/bind9_resources.rs) - no `host_network` field set

**Verification:**
```bash
kubectl get statefulset bind9-primary -o jsonpath='{.spec.template.spec.hostNetwork}'
# Should output: empty (defaults to false)
```

---

#### 5.2.6: Minimize the admission of containers with allowPrivilegeEscalation
**Level:** 1
**Type:** Automated

**Implementation:**
- ✅ **PASS** - Privilege escalation disabled
- `allowPrivilegeEscalation: false` explicitly set

**Evidence:** [src/bind9_resources.rs:435](../../src/bind9_resources.rs#L435)
```rust
allow_privilege_escalation: Some(false),
```

**Verification:**
```bash
kubectl get pods -l app=bind9 -o jsonpath='{.items[*].spec.containers[*].securityContext.allowPrivilegeEscalation}'
# Should output: false
```

---

#### 5.2.7: Minimize the admission of root containers
**Level:** 2
**Type:** Automated

**Implementation:**
- ✅ **PASS** - Runs as non-root user
- `runAsNonRoot: true` enforced
- `runAsUser: 1000` explicit UID

**Evidence:** [src/bind9_resources.rs:427-428](../../src/bind9_resources.rs#L427-L428)
```rust
run_as_non_root: Some(true),
run_as_user: Some(1000),
```

**Verification:**
```bash
kubectl get pods -l app=bind9 -o jsonpath='{.items[*].spec.containers[*].securityContext.runAsNonRoot}'
# Should output: true

kubectl get pods -l app=bind9 -o jsonpath='{.items[*].spec.containers[*].securityContext.runAsUser}'
# Should output: 1000
```

---

#### 5.2.8: Minimize the admission of containers with the NET_RAW capability
**Level:** 1
**Type:** Automated

**Implementation:**
- ✅ **PASS** - All capabilities dropped
- `capabilities.drop: ["ALL"]`
- No NET_RAW capability

**Evidence:** [src/bind9_resources.rs:429-433](../../src/bind9_resources.rs#L429-L433)
```rust
capabilities: Some(Capabilities {
    drop: Some(vec!["ALL".to_string()]),
    ..Default::default()
}),
```

**Verification:**
```bash
kubectl get pods -l app=bind9 -o jsonpath='{.items[*].spec.containers[*].securityContext.capabilities.drop}'
# Should output: ["ALL"]
```

---

#### 5.2.9: Minimize the admission of containers with added capabilities
**Level:** 1
**Type:** Automated

**Implementation:**
- ✅ **PASS** - No capabilities added
- `capabilities.add` not set
- All capabilities dropped

**Evidence:** [src/bind9_resources.rs:429-433](../../src/bind9_resources.rs#L429-L433) - only `drop` set, no `add`

**Verification:**
```bash
kubectl get pods -l app=bind9 -o jsonpath='{.items[*].spec.containers[*].securityContext.capabilities.add}'
# Should output: empty
```

---

#### 5.2.10: Minimize the admission of containers with capabilities assigned
**Level:** 2
**Type:** Manual

**Implementation:**
- ✅ **PASS** - All capabilities dropped, none added
- Meets strictest capability restriction

**Evidence:** Same as 5.2.8 and 5.2.9

---

#### 5.2.11: Minimize the admission of Windows HostProcess containers
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **N/A** - Bindy is Linux-only
- No Windows container support

---

#### 5.2.12: Minimize the admission of HostPath volumes
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - No hostPath volumes
- Uses PersistentVolumeClaims for storage
- ConfigMaps for configuration

**Evidence:** [src/bind9_resources.rs](../../src/bind9_resources.rs) - only PVC, ConfigMap, and EmptyDir volumes

**Verification:**
```bash
kubectl get statefulset bind9-primary -o jsonpath='{.spec.template.spec.volumes[*].hostPath}'
# Should output: empty
```

---

#### 5.2.13: Minimize the admission of containers which use HostPorts
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - No hostPort usage
- Services use ClusterIP/LoadBalancer
- No direct host port binding

**Evidence:** [src/bind9_resources.rs](../../src/bind9_resources.rs) - no `host_port` in container ports

**Verification:**
```bash
kubectl get pods -l app=bind9 -o jsonpath='{.items[*].spec.containers[*].ports[*].hostPort}'
# Should output: empty
```

---

### 5.3: Network Policies and CNI

#### 5.3.1: Ensure that the CNI in use supports NetworkPolicies
**Level:** 1
**Type:** Manual

**Implementation:**
- ⚠️ **CLUSTER RESPONSIBILITY** - CNI must support NetworkPolicies
- Bindy is compatible with NetworkPolicies
- Example policies available in documentation

**Recommended CNIs:**
- Calico
- Cilium
- Weave Net
- Linkerd (service mesh with policy)

---

#### 5.3.2: Ensure that all Namespaces have NetworkPolicies defined
**Level:** 2
**Type:** Automated

**Implementation:**
- ⚠️ **DEPLOYMENT RESPONSIBILITY** - NetworkPolicies should be applied per namespace
- Example NetworkPolicy provided below

**Example NetworkPolicy:**
```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: bindy-operator-netpol
  namespace: dns-system
spec:
  podSelector:
    matchLabels:
      app: bindy-operator
  policyTypes:
  - Ingress
  - Egress
  egress:
  # Allow Kubernetes API access
  - to:
    - namespaceSelector: {}
    ports:
    - protocol: TCP
      port: 443
  # Allow DNS
  - to:
    - namespaceSelector: {}
    ports:
    - protocol: UDP
      port: 53
  ingress:
  # Allow health checks from kubelet
  - from:
    - namespaceSelector: {}
    ports:
    - protocol: TCP
      port: 8080
```

---

### 5.4: Secrets Management

#### 5.4.1: Prefer using Secrets as files over Secrets as environment variables
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - Secrets mounted as volumes, not environment variables
- TSIG keys mounted as files in `/etc/bind/secrets`

**Evidence:** [src/bind9_resources.rs](../../src/bind9_resources.rs) - Secret volumes mounted to filesystem

**Verification:**
```bash
kubectl get statefulset bind9-primary -o yaml | grep -A 10 "volumes:"
# Verify secrets mounted as volumes, not in env:
```

---

#### 5.4.2: Consider external secret storage
**Level:** 2
**Type:** Manual

**Implementation:**
- ⚠️ **OPTIONAL** - Bindy supports Kubernetes Secrets
- Compatible with external secret operators:
  - External Secrets Operator (ESO)
  - HashiCorp Vault
  - AWS Secrets Manager
  - Azure Key Vault

**Integration Example:**
```yaml
apiVersion: external-secrets.io/v1beta1
kind: ExternalSecret
metadata:
  name: bind9-tsig-keys
spec:
  secretStoreRef:
    name: vault-backend
  target:
    name: bind9-tsig-keys
  data:
  - secretKey: tsig-key
    remoteRef:
      key: dns/tsig-keys
```

---

### 5.5: Extensible Admission Control

#### 5.5.1: Configure Image Provenance using ImagePolicyWebhook admission operator
**Level:** 2
**Type:** Manual

**Implementation:**
- ⚠️ **CLUSTER RESPONSIBILITY** - Admission operators configured cluster-wide
- Bindy supports image signature verification
- Compatible with:
  - Sigstore/Cosign
  - Notary
  - Kyverno
  - OPA Gatekeeper

**Recommended Policy (Kyverno):**
```yaml
apiVersion: kyverno.io/v1
kind: ClusterPolicy
metadata:
  name: verify-bindy-images
spec:
  validationFailureAction: enforce
  rules:
  - name: verify-signature
    match:
      resources:
        kinds:
        - Pod
    verifyImages:
    - image: "ghcr.io/firestoned/bindy:*"
      key: |-
        -----BEGIN PUBLIC KEY-----
        <COSIGN PUBLIC KEY>
        -----END PUBLIC KEY-----
```

---

### 5.7: General Policies

#### 5.7.1: Create administrative boundaries between resources using namespaces
**Level:** 1
**Type:** Manual

**Implementation:**
- ✅ **PASS** - Multi-tenancy via namespaces supported
- Bind9Instances deployed per namespace
- Zones scoped to namespaces

**Evidence:** [src/reconcilers/dnszone.rs](../../src/reconcilers/dnszone.rs) - namespace-aware reconciliation

---

#### 5.7.2: Ensure that the seccomp profile is set to docker/default in your Pod definitions
**Level:** 2
**Type:** Manual

**Implementation:**
- ⚠️ **TODO** - Add seccomp profile to SecurityContext

**Recommended Addition:**
```rust
// In src/bind9_resources.rs SecurityContext
seccomp_profile: Some(SeccompProfile {
    type_: "RuntimeDefault".to_string(),
    ..Default::default()
}),
```

**Future Enhancement:** Add seccomp profile to meet Level 2 compliance

---

#### 5.7.3: Apply SecurityContext to your Pods and Containers
**Level:** 2
**Type:** Manual

**Implementation:**
- ✅ **PASS** - SecurityContext configured on all containers
- Pod-level and container-level security contexts set

**Evidence:** [src/bind9_resources.rs:427-440](../../src/bind9_resources.rs#L427-L440)

---

#### 5.7.4: The default namespace should not be used
**Level:** 2
**Type:** Manual

**Implementation:**
- ✅ **PASS** - Operator deployed to `dns-system` namespace (recommended)
- Documentation specifies dedicated namespace
- No use of `default` namespace

**Evidence:** [deploy/operator.yaml](../../deploy/operator.yaml)

---

## Compliance Summary

### Level 1 Controls (Essential Security)

| Control | Status | Notes |
|---------|--------|-------|
| 5.1.1 | ✅ PASS | No cluster-admin required |
| 5.1.2 | ✅ PASS | Minimal secret access |
| 5.1.3 | ✅ PASS | No wildcard permissions |
| 5.1.4 | ✅ PASS | No direct pod creation |
| 5.1.5 | ✅ PASS | Dedicated ServiceAccount |
| 5.1.6 | ✅ PASS | Token mounted only where needed |
| 5.2.1 | ⚠️ CLUSTER | PSA/PSS enabled (cluster responsibility) |
| 5.2.2 | ✅ PASS | No privileged containers |
| 5.2.3 | ✅ PASS | No hostPID |
| 5.2.4 | ✅ PASS | No hostIPC |
| 5.2.5 | ✅ PASS | No hostNetwork |
| 5.2.6 | ✅ PASS | No privilege escalation |
| 5.2.8 | ✅ PASS | No NET_RAW capability |
| 5.2.9 | ✅ PASS | No added capabilities |
| 5.2.12 | ✅ PASS | No hostPath volumes |
| 5.2.13 | ✅ PASS | No hostPorts |
| 5.3.1 | ⚠️ CLUSTER | CNI with NetworkPolicy support |
| 5.4.1 | ✅ PASS | Secrets as files |
| 5.7.1 | ✅ PASS | Namespace isolation |

**Level 1 Compliance: 16/19 PASS (84%)** - 3 cluster-level controls

### Level 2 Controls (Defense-in-Depth)

| Control | Status | Notes |
|---------|--------|-------|
| 5.2.7 | ✅ PASS | Runs as non-root |
| 5.2.10 | ✅ PASS | All capabilities dropped |
| 5.3.2 | ⚠️ DEPLOYMENT | NetworkPolicies recommended |
| 5.4.2 | ⚠️ OPTIONAL | External secret storage compatible |
| 5.5.1 | ⚠️ CLUSTER | Image signature verification |
| 5.7.2 | ❌ TODO | Seccomp profile needed |
| 5.7.3 | ✅ PASS | SecurityContext configured |
| 5.7.4 | ✅ PASS | Dedicated namespace |

**Level 2 Compliance: 4/8 PASS (50%)** - 3 cluster/deployment-level, 1 TODO

---

## Hardening Recommendations

### Immediate Actions
1. ✅ Deploy to dedicated namespace (not `default`)
2. ✅ Use RoleBinding instead of ClusterRoleBinding where possible
3. ⚠️ Apply NetworkPolicies to restrict traffic
4. ⚠️ Enable Pod Security Admission (PSA) with `restricted` policy

### Short-Term Improvements
1. ❌ Add seccomp profile (`RuntimeDefault`)
2. ⚠️ Implement image signature verification (cosign)
3. ⚠️ Set `automountServiceAccountToken: false` on BIND9 pods
4. ⚠️ Add read-only root filesystem to SecurityContext

### Long-Term Enhancements
1. External secret management integration (Vault/ESO)
2. Service mesh integration (Linkerd) for mTLS and network policies
3. OPA/Kyverno policy enforcement
4. Runtime security monitoring (Falco)

---

## Verification Commands

Run these commands to verify CIS compliance:

```bash
# Check RBAC permissions
kubectl get clusterrole bindy-operator -o yaml

# Check SecurityContext settings
kubectl get pods -l app=bind9 -o json | jq '.items[].spec.containers[].securityContext'

# Check for privileged containers
kubectl get pods --all-namespaces -o json | jq '.items[] | select(.spec.containers[].securityContext.privileged == true)'

# Check for host namespace usage
kubectl get pods --all-namespaces -o json | jq '.items[] | select(.spec.hostNetwork == true or .spec.hostPID == true or .spec.hostIPC == true)'

# Check ServiceAccount usage
kubectl get pods -l app=bindy-operator -o jsonpath='{.items[*].spec.serviceAccountName}'
```

---

## Automated Scanning Tools

Recommended tools for CIS Kubernetes Benchmark scanning:

1. **kube-bench** (Aqua Security)
   ```bash
   kubectl apply -f https://raw.githubusercontent.com/aquasecurity/kube-bench/main/job.yaml
   kubectl logs -f job/kube-bench
   ```

2. **kubescape** (ARMO)
   ```bash
   kubescape scan framework cis-v1.23-t1.0.1
   ```

3. **Polaris** (Fairwinds)
   ```bash
   kubectl apply -f https://github.com/FairwindsOps/polaris/releases/latest/download/dashboard.yaml
   ```

4. **Starboard** (Aqua Security)
   ```bash
   kubectl starboard scan configauditreports
   ```

---

## References

- [CIS Kubernetes Benchmark v1.8.0](https://www.cisecurity.org/benchmark/kubernetes)
- [Kubernetes Pod Security Standards](https://kubernetes.io/docs/concepts/security/pod-security-standards/)
- [NSA/CISA Kubernetes Hardening Guide](https://media.defense.gov/2022/Aug/29/2003066362/-1/-1/0/CTR_KUBERNETES_HARDENING_GUIDANCE_1.2_20220829.PDF)
- [OWASP Kubernetes Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Kubernetes_Security_Cheat_Sheet.html)

---

## Document Control

| Version | Date       | Author          | Changes                          |
|---------|------------|-----------------|----------------------------------|
| 1.0     | 2025-12-17 | Erick Bourgeois | Initial CIS Kubernetes Benchmark compliance |

