# Operand-namespace RBAC

Template for running Bind9Instances (operands) in a namespace other than the
operator's own (`bindy-system` by default). Each operand namespace needs two
grants that the default install only makes for `bindy-system`. Both are in
[`rbac.yaml`](rbac.yaml):

| Object | Scope | Why |
|---|---|---|
| Role + RoleBinding `bindy-secrets-writer` | Namespaced | The operator creates, rotates and deletes each instance's `<instance>-rndc-key` Secret in the instance's namespace. Its ClusterRole is read-only on Secrets (B-5 hardening). |
| ClusterRoleBinding `bindcar-tokenreview-<namespace>` | Cluster | The bindcar sidecar runs as the `bind9` ServiceAccount of its own namespace and validates the operator's token with a TokenReview. Binds the existing `bindcar-tokenreview` ClusterRole to that ServiceAccount. |

```sh
NS=team-dns
sed "s/REPLACE_NAMESPACE/$NS/g" rbac.yaml | kubectl apply -f -
```

Remove the same objects when the namespace stops hosting operands:

```sh
NS=team-dns
sed "s/REPLACE_NAMESPACE/$NS/g" rbac.yaml | kubectl delete -f -
```

The template is kept out of `deploy/operator/rbac/` itself because that
directory is applied non-recursively by `make deploy-rbac` and the test scripts,
and the `REPLACE_NAMESPACE` placeholder must not be applied as-is.

Full procedure, the alternative of adding a subject to the shared
`bindcar-tokenreview` ClusterRoleBinding, verification, failure symptoms and
cleanup: [`docs/src/operations/multi-namespace.md`](../../../../docs/src/operations/multi-namespace.md).
