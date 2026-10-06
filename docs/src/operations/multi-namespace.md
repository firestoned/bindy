# Operands in Other Namespaces

The default install assumes every `Bind9Instance` runs in the operator's own
namespace, `bindy-system`. Running instances (the *operands*) anywhere else
needs two extra grants per operand namespace. Neither is created automatically,
neither `bindy bootstrap operator` nor `kubectl apply -f deploy/operator/rbac/`
makes them, and each one fails in a different place when it is missing.

## When this applies

You need this page for every namespace, other than the operator's, that holds a
`Bind9Instance`. That includes instances you create yourself, instances a
`Bind9Cluster` creates in its own namespace, and the managed `Bind9Cluster` a
`ClusterBind9Provider` creates in each namespace that has an instance
referencing it.

It applies in both operator modes:

- **Cluster-wide** (`BINDY_WATCH_NAMESPACES` unset, the default): the
  `bindy-role` ClusterRole already lets the operator see the namespace, but it
  is read-only on Secrets.
- **Namespace-scoped** (`BINDY_WATCH_NAMESPACES` set): the operand namespace must
  also be in `BINDY_WATCH_NAMESPACES` and carry the per-namespace `bindy-role`
  Role from `deploy/operator/rbac/namespaced/` (see
  [Environment Variables](env-vars.md)). That Role is read-only on Secrets too,
  so the two grants below are still required.

The examples use `team-dns` as the operand namespace and assume the operator
runs as ServiceAccount `bindy` in `bindy-system`. Substitute your own names.

## The two grants

| Grant | Who is granted | What for | Missing it breaks |
|---|---|---|---|
| Role + RoleBinding `bindy-secrets-writer` in the operand namespace | operator SA `system:serviceaccount:bindy-system:bindy` | `create`, `update`, `patch`, `delete` on `secrets`: creating, rotating and deleting each instance's `<instance>-rndc-key` Secret | `Bind9Instance` reconcile: no BIND9 pods are created |
| ClusterRole `bindcar-tokenreview` bound to the operand namespace's `bind9` SA | `system:serviceaccount:team-dns:bind9` | `create` on `tokenreviews.authentication.k8s.io`: the bindcar sidecar validating the operator's bearer token | every bindcar API call: zones and records never reach BIND9 |

### Why they are needed

**Secrets (B-5 hardening).** The operator's ClusterRole grants only `get`,
`list` and `watch` on Secrets cluster-wide. The mutating verbs live in the
namespaced `bindy-secrets-writer` Role, which the shipped manifests
(`deploy/operator/rbac/secrets-role.yaml`, `secrets-rolebinding.yaml`) and
`bindy bootstrap operator` create in the operator namespace only. This is
deliberate: a compromised operator cannot write Secrets in namespaces you have
not opted in, such as `kube-system`. The operator writes each instance's RNDC
key Secret in the instance's own namespace, so every operand namespace has to
be opted in explicitly.

**TokenReview (bindcar Mode B).** Every BIND9 pod runs the bindcar API sidecar
(container `api`) as the `bind9` ServiceAccount of its own namespace. The
operator authenticates to bindcar with a `bindcar`-audience ServiceAccount
token, and bindcar validates it by creating a `TokenReview`. The shipped
`bindcar-tokenreview` ClusterRoleBinding names only
`system:serviceaccount:bindy-system:bind9`, so a sidecar in `team-dns` is
refused when it tries to create the TokenReview.

The sidecar's allow-list does not change per namespace: `BIND_ALLOWED_SERVICE_ACCOUNTS`
always names the operator SA (`system:serviceaccount:bindy-system:bindy`), which
the operator sets on every operand Deployment.

## Granting a namespace

### Option 1: the template (recommended)

`deploy/operator/rbac/operand-namespace/rbac.yaml` holds both grants for one
namespace, with `REPLACE_NAMESPACE` as the placeholder:

```bash
NS=team-dns
sed "s/REPLACE_NAMESPACE/$NS/g" deploy/operator/rbac/operand-namespace/rbac.yaml \
  | kubectl apply -f -
```

It is the following YAML. The Role and RoleBinding mirror
`deploy/operator/rbac/secrets-role.yaml` and `secrets-rolebinding.yaml` with only
the namespace changed:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: bindy-secrets-writer
  namespace: team-dns
  labels:
    app.kubernetes.io/name: bindy
    app.kubernetes.io/component: rbac
rules:
  - apiGroups: [""]
    resources: ["secrets"]
    verbs: ["create", "update", "patch", "delete"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: bindy-secrets-writer
  namespace: team-dns
  labels:
    app.kubernetes.io/name: bindy
    app.kubernetes.io/component: rbac
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: bindy-secrets-writer
subjects:
  - kind: ServiceAccount
    name: bindy               # the operator SA
    namespace: bindy-system   # the operator namespace
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: bindcar-tokenreview-team-dns
  labels:
    app.kubernetes.io/name: bindy
    app.kubernetes.io/part-of: bindy
    app.kubernetes.io/component: rbac
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: bindcar-tokenreview   # shipped in tokenreview-clusterrole.yaml
subjects:
  - kind: ServiceAccount
    name: bind9               # the operand SA bindcar runs as
    namespace: team-dns
```

The template binds the existing `bindcar-tokenreview` ClusterRole with a
**separate ClusterRoleBinding per namespace** rather than editing the shared
`bindcar-tokenreview` binding. `subjects` is replaced as a whole on apply, so
re-applying `deploy/operator/rbac/tokenreview-clusterrolebinding.yaml` or
re-running `bindy bootstrap operator` resets the shared binding to its single
`bindy-system` subject and silently drops any namespace you added to it. A
per-namespace binding survives both, and is one object to delete when the
namespace goes away. This is the right shape for GitOps too: commit one rendered
copy of the template per operand namespace.

### Option 2: add a subject to the shared binding

If you prefer a single `bindcar-tokenreview` ClusterRoleBinding, create the
`bindy-secrets-writer` Role and RoleBinding as above, then append a subject with
a JSON patch:

```bash
NS=team-dns
kubectl patch clusterrolebinding bindcar-tokenreview --type=json -p \
  "[{\"op\":\"add\",\"path\":\"/subjects/-\",\"value\":{\"kind\":\"ServiceAccount\",\"name\":\"bind9\",\"namespace\":\"$NS\"}}]"
```

Declaratively, the binding then lists one subject per operand namespace:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: bindcar-tokenreview
  labels:
    app.kubernetes.io/name: bindy
    app.kubernetes.io/part-of: bindy
    app.kubernetes.io/component: rbac
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: bindcar-tokenreview
subjects:
  - kind: ServiceAccount
    name: bind9
    namespace: bindy-system   # the operator namespace, from the default install
  - kind: ServiceAccount
    name: bind9
    namespace: team-dns       # one entry per operand namespace
```

!!! warning "Re-applying the shipped binding drops patched subjects"
    With this option, own the full subject list in one place (your GitOps
    repository) and never re-apply `deploy/operator/rbac/tokenreview-clusterrolebinding.yaml`
    or re-run `bindy bootstrap operator` without re-adding the operand
    namespaces afterwards.

## Verifying

Run these before creating the first instance in the namespace. Both must print
`yes`:

```bash
NS=team-dns

# 1. The operator may write Secrets in the operand namespace.
kubectl auth can-i create secrets -n "$NS" \
  --as=system:serviceaccount:bindy-system:bindy

# 2. The operand namespace's bind9 SA may create TokenReviews.
kubectl auth can-i create tokenreviews.authentication.k8s.io \
  --as=system:serviceaccount:"$NS":bind9
```

Check `update` and `delete` on secrets the same way if you have customised the
Role; RNDC key rotation uses `update` and instance deletion uses `delete`.

The second check works before the `bind9` ServiceAccount exists: the operator
creates that ServiceAccount itself, as the first step of reconciling the first
instance in the namespace.

## Symptoms when a grant is missing

### Missing `bindy-secrets-writer`: no BIND9 pods

The operator creates the `bind9` ServiceAccount, then fails creating the RNDC
key Secret, before any ConfigMap, Deployment or Service exists. The instance
reports:

```bash
kubectl get bind9instance -n team-dns <name> \
  -o jsonpath='{.status.conditions[?(@.type=="Ready")]}'
```

- `status`: `False`
- `reason`: `NotReady`
- `message`: starts with `Failed to create resources: ApiError: secrets is forbidden:`
  and continues `User "system:serviceaccount:bindy-system:bindy" cannot create
  resource "secrets" in API group "" in the namespace "team-dns": Forbidden`

The operator log carries the same text after
`Failed to create/update resources for team-dns/<name>:`, and the reconcile is
retried with backoff until the grant is added. No restart is needed: the next
retry succeeds once the Role and RoleBinding exist.

### Missing TokenReview subject: pods run, zones never load

The BIND9 pods start and answer queries, but every bindcar call from the
operator is refused, so zones and records never reach BIND9 and `DNSZone`s do
not become Ready.

- **Operator log:** `HTTP API request failed` with `status=401 Unauthorized`
  and `error={"error":"Unauthorized"}`; zone errors contain
  `HTTP 401 Unauthorized: {"error":"Unauthorized"}`. A 401 is not retried as a
  transient error.
- **bindcar log** (the real cause; bindcar deliberately returns a generic
  `Unauthorized` to the caller):

```bash
kubectl logs -n team-dns <bind9-pod> -c api | grep -i tokenreview
```

shows `TokenReview API call failed:` followed by
`tokenreviews.authentication.k8s.io is forbidden: User "system:serviceaccount:team-dns:bind9" cannot create resource "tokenreviews" in API group "authentication.k8s.io" at the cluster scope`,
and `Token validation failed: Failed to validate token with Kubernetes API: ...`.

Adding the subject or binding fixes it without restarting anything: bindcar
creates a TokenReview per request, and the operator's next reconcile succeeds.

For the other causes of a bindcar 401 (audience mismatch, wrong allow-list), see
[Common Issues](common-issues.md#operator-gets-http-401-from-the-bindcar-api).

## Removing a namespace

Once a namespace no longer hosts operands, remove its grants so the operator and
the namespace's `bind9` SA keep no access they do not use.

1. Delete the namespace's `Bind9Instance`s (and any `Bind9Cluster`) **first**,
   while the grant still exists. The instance finalizer deletes the
   `<instance>-rndc-key` Secret; without `delete` on secrets that step only logs
   a warning, and the Secret is left to Kubernetes garbage collection through
   its owner reference.
2. Remove the grants.

With the template (option 1):

```bash
NS=team-dns
sed "s/REPLACE_NAMESPACE/$NS/g" deploy/operator/rbac/operand-namespace/rbac.yaml \
  | kubectl delete -f -
```

With a subject on the shared binding (option 2), find the subject's index and
remove it. The `test` op makes the patch fail instead of removing the wrong
entry if the list changed in between:

```bash
NS=team-dns
IDX=$(kubectl get clusterrolebinding bindcar-tokenreview \
  -o jsonpath='{range .subjects[*]}{.namespace}{"\n"}{end}' | grep -nx "$NS" | cut -d: -f1)
IDX=$((IDX - 1))
kubectl patch clusterrolebinding bindcar-tokenreview --type=json -p \
  "[{\"op\":\"test\",\"path\":\"/subjects/$IDX/namespace\",\"value\":\"$NS\"},{\"op\":\"remove\",\"path\":\"/subjects/$IDX\"}]"

kubectl delete rolebinding,role bindy-secrets-writer -n "$NS"
```

Deleting the namespace itself removes the namespaced Role and RoleBinding, but
**not** a cluster-scoped binding or subject. A leftover subject means a
namespace later recreated with the same name gets TokenReview access for its
`bind9` SA without anyone granting it, so always remove the cluster-scoped half
explicitly.

In namespace-scoped mode, also drop the namespace from `BINDY_WATCH_NAMESPACES`
and delete its `bindy-role` Role and RoleBinding, changing both together (see
[Environment Variables](env-vars.md)).

## See also

- [RBAC](rbac.md): the operator's ClusterRole and the `bindcar-tokenreview` ClusterRole
- [Environment Variables](env-vars.md): `BINDY_WATCH_NAMESPACES`
- [Multi-Tenancy Guide](../guide/multi-tenancy.md): tenancy models that put instances in team namespaces
- `deploy/operator/rbac/operand-namespace/`: the template used above
