# RBAC (Role-Based Access Control)

Configure Kubernetes RBAC for the Bindy operator.

## Required Permissions

The Bindy operator needs permissions to:
- Manage Bind9Instance, DNSZone, and DNS record resources
- Create and manage Deployments, Services, ConfigMaps, and ServiceAccounts
- Update resource status fields
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
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  
  # Kubernetes resources
  - apiGroups: ["apps"]
    resources: ["deployments"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  - apiGroups: [""]
    resources: ["services", "configmaps", "serviceaccounts"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  - apiGroups: [""]
    resources: ["events"]
    verbs: ["create", "patch"]
```

## ServiceAccount

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: bindy
  namespace: dns-system
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
  namespace: dns-system
```

## Namespace-Scoped RBAC

For namespace-scoped deployments, use Role instead of ClusterRole:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: bindy-role
  namespace: dns-system
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
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: bindy-rolebinding
  namespace: dns-system
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: bindy-role
subjects:
- kind: ServiceAccount
  name: bindy
  namespace: dns-system
```

## Applying RBAC

```bash
# Apply all RBAC resources
kubectl apply -f deploy/rbac/

# Verify ServiceAccount
kubectl get serviceaccount bindy -n dns-system

# Verify ClusterRole
kubectl get clusterrole bindy-role

# Verify ClusterRoleBinding
kubectl get clusterrolebinding bindy-rolebinding
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
  --as=system:serviceaccount:dns-system:bindy

# Describe the ClusterRoleBinding
kubectl describe clusterrolebinding bindy-rolebinding

# Check operator logs for permission errors
kubectl logs -n dns-system deployment/bindy | grep -i forbidden
```
