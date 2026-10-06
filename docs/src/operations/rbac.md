# RBAC (Role-Based Access Control)

Configure Kubernetes RBAC for the Bindy operator.

## Required Permissions

The Bindy operator needs permissions to:
- Manage Bind9Instance, DNSZone, and DNS record resources
- Create and manage Deployments, Services, ConfigMaps, and ServiceAccounts
- Update resource status fields
- Read BIND9 pods, and set the `bindy.firestoned.io/zones-loaded` condition on
  their status (the zones-loaded readiness gate, ADR-0017)
- Create events for logging

## ClusterRole

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-role
rules:
  # Bindy CRDs
  - apiGroups: ["bindy.firestoned.io"]
    resources:
      - "bind9instances"
      - "bind9instances/status"
      - "dnszones"
      - "dnszones/status"
      - "arecords"
      - "arecords/status"
      - "aaaarecords"
      - "aaaarecords/status"
      - "cnamerecords"
      - "cnamerecords/status"
      - "mxrecords"
      - "mxrecords/status"
      - "txtrecords"
      - "txtrecords/status"
      - "nsrecords"
      - "nsrecords/status"
      - "srvrecords"
      - "srvrecords/status"
      - "caarecords"
      - "caarecords/status"
      - "ptrrecords"
      - "ptrrecords/status"
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  
  # Kubernetes resources
  - apiGroups: ["apps"]
    resources: ["deployments"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  - apiGroups: [""]
    resources: ["services", "configmaps", "serviceaccounts"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # BIND9 pods: read-only, plus the zones-loaded readiness gate (ADR-0017)
  - apiGroups: [""]
    resources: ["pods", "endpoints"]
    verbs: ["get", "list", "watch"]

  - apiGroups: [""]
    resources: ["pods/status"]
    verbs: ["get", "patch"]

  - apiGroups: [""]
    resources: ["events"]
    verbs: ["create", "patch"]
```

This is an abridged view; `deploy/operator/rbac/role.yaml` is authoritative.

### Why `pods/status` patch

Every BIND9 pod lists `bindy.firestoned.io/zones-loaded` in
`spec.readinessGates`, so Kubernetes keeps it out of its Service until the
operator sets that condition `True`, which it does once the pod has every live
zone (and, on a primary, every record) loaded. The operator writes that one
condition with a strategic merge patch of `pods/status`; it has no write verb
on `pods`, so it cannot change a pod's spec or labels, or delete it.

Apply the updated role **with or before** an operator that uses the gate. An
operator without the grant cannot open the gate: new BIND9 pods stay not
Ready (the old pods keep serving) and the operator logs `403 Forbidden` on
`pods/status`.

## ServiceAccount

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: bindy
  namespace: bindy-system
```

## ClusterRoleBinding

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: bindy-rolebinding
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: bindy-role
subjects:
- kind: ServiceAccount
  name: bindy
  namespace: bindy-system
```

## Namespace-Scoped RBAC

For namespace-scoped deployments, use Role instead of ClusterRole:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: bindy-role
  namespace: bindy-system
rules:
  # Same rules as ClusterRole
  - apiGroups: ["bindy.firestoned.io"]
    resources: ["bind9instances", "dnszones", "*records"]
    verbs: ["*"]
  
  - apiGroups: ["apps"]
    resources: ["deployments"]
    verbs: ["*"]
  
  - apiGroups: [""]
    resources: ["services", "configmaps"]
    verbs: ["*"]

  # The zones-loaded readiness gate (ADR-0017)
  - apiGroups: [""]
    resources: ["pods/status"]
    verbs: ["get", "patch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: bindy-rolebinding
  namespace: bindy-system
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: bindy-role
subjects:
- kind: ServiceAccount
  name: bindy
  namespace: bindy-system
```

## bindcar TokenReview RBAC (Mode B)

Since the bindcar 0.7.x migration, the operator authenticates to the bindcar
sidecar HTTP API with a `bindcar`-audience ServiceAccount token, which the
sidecar validates by creating a Kubernetes **TokenReview**. The sidecar runs as
the operand `bind9` ServiceAccount, so that SA needs permission to create
tokenreviews. This is a purpose-built, least-privilege ClusterRole — it grants
**only** `create tokenreviews` (no `subjectaccessreviews`, no other verbs).

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindcar-tokenreview
rules:
  - apiGroups: ["authentication.k8s.io"]
    resources: ["tokenreviews"]
    verbs: ["create"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: bindcar-tokenreview
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: bindcar-tokenreview
subjects:
  - kind: ServiceAccount
    name: bind9
    namespace: bindy-system   # one subject per operand namespace
```

Ships as `deploy/operator/rbac/tokenreview-clusterrole.yaml` and
`tokenreview-clusterrolebinding.yaml` (applied by `make regression-test` and the
release manifests). For operand pods in namespaces other than `bindy-system`,
add one binding subject per namespace's `bind9` SA. Operand namespaces also need
their own copy of the `bindy-secrets-writer` Role and RoleBinding; see
[Operands in Other Namespaces](multi-namespace.md) for both grants and the
`deploy/operator/rbac/operand-namespace/` template. See the
[bindcar 0.7.x migration guide](migration-guide.md) for the full Mode B setup
(projected `audience: bindcar` token + `BIND_ALLOWED_SERVICE_ACCOUNTS`).

## Applying RBAC

### Install from Latest Release (Recommended)

```bash
# Apply all RBAC resources from latest release
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/serviceaccount.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/role.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/rolebinding.yaml

# Verify ServiceAccount
kubectl get serviceaccount bindy -n bindy-system

# Verify ClusterRole
kubectl get clusterrole bindy-role

# Verify ClusterRoleBinding
kubectl get clusterrolebinding bindy-rolebinding
```

### Install from Source

```bash
# Apply all RBAC resources from local files
kubectl apply -f deploy/rbac/
```

## Security Best Practices

1. **Least Privilege** - Only grant necessary permissions
2. **Namespace Scoping** - Use namespace-scoped roles when possible
3. **Separate ServiceAccounts** - Don't reuse default ServiceAccount
4. **Audit Regularly** - Review permissions periodically
5. **Use Pod Security Policies** - Restrict pod capabilities

## Troubleshooting RBAC

Check if operator has required permissions:

```bash
# Check what the ServiceAccount can do
kubectl auth can-i list dnszones \
  --as=system:serviceaccount:bindy-system:bindy

# The zones-loaded readiness gate needs pods/status patch in every operand
# namespace (ADR-0017)
kubectl auth can-i patch pods/status -n bindy-system \
  --as=system:serviceaccount:bindy-system:bindy

# Describe the ClusterRoleBinding
kubectl describe clusterrolebinding bindy-rolebinding

# Check operator logs for permission errors
kubectl logs -n bindy-system deployment/bindy | grep -i forbidden
```
