# Migrating to Scout Source Guardrails (ADR-0025)

This guide takes an existing Scout installation, same-cluster or
multi-cluster, to the guardrails of
[ADR-0025](https://github.com/firestoned/bindy/blob/main/docs/adr/0025-scout-records-bound-to-cluster-and-zones.md):
each cluster's Scout publishes only into its own zones and address ranges,
and on the bindy cluster each Scout's ServiceAccount can write only records
for its own cluster names and zones.

Nothing here is required for an upgrade to keep working. Every guardrail is
opt-in, except one change in how a `DNSZone` selects Scout records
([step 1](#step-1-upgrade-the-operator-on-the-bindy-cluster)), which affects
only hand-made records.

## Why migrate

Before ADR-0025, every check on what a Scout publishes ran inside the Scout:

- the zone grant `bindy.firestoned.io/allow-zone-namespaces` names bare
  namespaces, and namespace names repeat across clusters, so a grant written
  for `team-a` on one cluster also covers `team-a` on every other cluster;
- the `bindy.firestoned.io/source-cluster` label is whatever the Scout was
  configured with;
- the token from `bindy bootstrap mc` can create, change and delete any
  `ARecord` in its namespace;
- a `DNSZone` selector that omits the zone label takes every Scout record.

So a tenant on cluster `foo`, or a compromised `foo` Scout, could publish
into `bar.example.com`, take over a name `bar` already publishes, or delete
`bar`'s records. See threat model items
[S4](../security/threat-model.md#s4-a-scout-claims-another-clusters-name) and
[T5](../security/threat-model.md#t5-cross-cluster-dns-record-injection-through-scout).

## What changes

| Change | Where | Default | Effect when enabled |
|---|---|---|---|
| `--allowed-zones` / `BINDY_SCOUT_ALLOWED_ZONES` | Scout, each cluster | unset: every zone, startup warning | A source naming any other zone publishes nothing; earlier records stay |
| `--allowed-ip-ranges` / `BINDY_SCOUT_ALLOWED_IP_RANGES` | Scout, each cluster | unset: every address, startup warning | A source with an address outside publishes nothing; retried every 30 s |
| `bindy-scout-sources` ConfigMap | bindy cluster, records namespace | absent | Registers each Scout ServiceAccount's cluster names and zones |
| `bindy-scout-source-binding` admission policy (`21`/`22`) | bindy cluster | not installed | Refuses a registered account's `ARecord` writes outside its registration |
| DNSZone selection of Scout records | operator | **always on** | A record labelled `managed-by: scout` is taken only by the zone named in its `zone` label |
| Refused deletes of another cluster's records | Scout | always on | Logged and skipped instead of retried |
| `bindy bootstrap mc --cluster-name --allowed-zones` | CLI | not passed | Writes the registry entry; `--revoke` removes it |
| `bindy bootstrap scout --allowed-zones --allowed-ip-ranges` | CLI | not passed | Templates the Scout settings |

There is no CRD change and no change to any Scout RBAC.

## Before you start

You need, for each cluster that runs Scout:

- its **cluster name** (`BINDY_SCOUT_CLUSTER_NAME` / `--cluster-name`);
- the **zones** it legitimately publishes into;
- its **load-balancer address ranges** (the addresses its Ingresses,
  Services and Gateways get);
- in multi-cluster mode, the **ServiceAccount** on the bindy cluster it
  writes as (the account behind its `BINDY_SCOUT_REMOTE_SECRET`).

Read them from each cluster:

```bash
# On each child (edge) cluster
kubectl get deploy bindy-scout -n bindy-system \
  -o jsonpath='{range .spec.template.spec.containers[0].env[*]}{.name}={.value}{"\n"}{end}' \
  | grep -E 'CLUSTER_NAME|REMOTE_SECRET|DEFAULT_ZONE|DEFAULT_IPS'
kubectl get deploy bindy-scout -n bindy-system -o jsonpath='{.spec.template.spec.containers[0].args}'

# The account behind the remote Secret is the user in its kubeconfig
kubectl get secret <remote-secret> -n bindy-system -o jsonpath='{.data.kubeconfig}' \
  | base64 -d | grep -A2 '^users:'
```

On the bindy cluster, list which cluster names and zones Scout records use
today. This is what each registration and allowlist must cover:

```bash
kubectl get arecords -n bindy-system -l bindy.firestoned.io/managed-by=scout \
  -o custom-columns='CLUSTER:.metadata.labels.bindy\.firestoned\.io/source-cluster,ZONE:.metadata.labels.bindy\.firestoned\.io/zone' \
  | sort | uniq -c
```

And list the Scout accounts `bootstrap mc` created:

```bash
kubectl get serviceaccounts -n bindy-system -l app.kubernetes.io/component=scout-remote
```

!!! warning "One account per cluster"
    If several child clusters share one account (for example the default
    `bindy-scout-remote`), that account can only be registered for all of
    their names and zones together, and the admission policy cannot tell
    them apart. Split it first: see
    [Splitting a shared account](#splitting-a-shared-account).

## Step 1: Upgrade the operator on the bindy cluster

From this release a `DNSZone` takes a record labelled
`bindy.firestoned.io/managed-by: scout` only when the record's
`bindy.firestoned.io/zone` label equals the zone's `spec.zoneName` (case
and a trailing dot ignored). Scout always writes the label with the zone it
authorized against, so **records Scout wrote are not affected**.

Before upgrading, find records that would stop being selected: Scout-labelled
records whose zone label is missing or differs from the zone serving them.

```bash
kubectl get arecords -A -l bindy.firestoned.io/managed-by=scout -o json | jq -r '
  .items[]
  | select((.metadata.labels["bindy.firestoned.io/zone"] // "" | ascii_downcase | rtrimstr("."))
           != (.status.zoneRef.zoneName // "" | ascii_downcase | rtrimstr(".")))
  | "\(.metadata.namespace)/\(.metadata.name) zone-label=\(.metadata.labels["bindy.firestoned.io/zone"] // "-") served-by=\(.status.zoneRef.zoneName // "-")"'
```

Expect no output, apart from records that are not served by any zone yet
(`served-by=-`), which this check cannot judge. Any other record listed was
not written by Scout (or was edited by hand). Fix its zone label, or remove `bindy.firestoned.io/managed-by:
scout` from it, before you upgrade.

After the upgrade, the operator logs every record it skips:

```bash
kubectl logs -n bindy-system deploy/bindy | grep 'labelled for another zone'
```

## Step 2: Register each Scout account (multi-cluster mode)

For each child cluster, re-run `bindy bootstrap mc` against the bindy cluster
with its **existing** `--service-account`, adding its cluster name and
zones. It re-applies the same ServiceAccount, Role and RoleBinding (no
change) and writes the account's keys into `bindy-scout-sources`. It also
writes the kubeconfig Secret file again, with the same token: you do not need
to re-apply it to the child cluster, so delete it.

```bash
bindy bootstrap mc \
  --service-account bindy-scout-cluster-a \
  --cluster-name cluster-a \
  --allowed-zones cluster-a.example.com,cluster-a.example.org \
  -o register-cluster-a.yaml
rm register-cluster-a.yaml
```

Check the registry:

```bash
kubectl get configmap bindy-scout-sources -n bindy-system -o jsonpath='{.data}' | jq .
```

```json
{
  "bindy-scout-cluster-a.clusters": "cluster-a",
  "bindy-scout-cluster-a.zones": "cluster-a.example.com,cluster-a.example.org"
}
```

Registering has no effect until the policy is installed in step 3, so this
step is safe on a live system. If you manage the bindy cluster with GitOps,
commit the ConfigMap instead (one pair of keys per account); see
`examples/scout-guardrails-bindy-cluster.yaml`.

!!! note "Zone entries cover subzones"
    `cluster-a.example.com` also allows `api.cluster-a.example.com`, but not
    `example.com` or `badcluster-a.example.com`. Register the narrowest
    zones each cluster really owns.

## Step 3: Install the admission policy on the bindy cluster

To see what the policy would refuse without refusing it, install the binding
with `Warn` and `Audit` first:

```bash
kubectl apply -f deploy/admission-policies/21-bindy-scout-source-binding-policy.yaml
cat <<'EOF' | kubectl apply -f -
apiVersion: admissionregistration.k8s.io/v1
kind: ValidatingAdmissionPolicyBinding
metadata:
  name: bindy-scout-source-binding-binding
spec:
  policyName: bindy-scout-source-binding
  paramRef:
    name: bindy-scout-sources
    parameterNotFoundAction: Allow
  validationActions:
    - Warn
    - Audit
EOF
```

Let every Scout run through its normal traffic (a Scout restart republishes
every source, which is a quick way to exercise them all), then look for
would-be denials in the API server audit log: each one carries the audit
annotation `validation.policy.admission.k8s.io/validation_failure` naming
the policy and the message. A `kubectl` request that would be denied prints
the message as a `Warning:` line. A would-be denial means the account's
registration is missing a cluster name or zone it really uses: fix the
registration (step 2) and repeat.

Then enforce it:

```bash
kubectl apply -f deploy/admission-policies/22-bindy-scout-source-binding-binding.yaml
```

Or install every policy with `make admission-policies-install`, or the
release's `admission-policies.yaml`.

Verify that a registered account is refused outside its registration. This
impersonates the account, so run it as a cluster admin:

```bash
cat <<'EOF' | kubectl apply --dry-run=server \
  --as=system:serviceaccount:bindy-system:bindy-scout-cluster-a -f -
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: guardrail-check
  namespace: bindy-system
  labels:
    bindy.firestoned.io/managed-by: scout
    bindy.firestoned.io/source-cluster: cluster-a
    bindy.firestoned.io/zone: cluster-b.example.com
spec:
  name: www
  ipv4Addresses: ["192.0.2.10"]
EOF
# Expected: ... denied request: ServiceAccount 'bindy-scout-cluster-a' may only
# publish into zones [cluster-a.example.com, ...], not 'cluster-b.example.com'
```

Change the zone label to one of the account's zones and the same command
reports `created (server dry run)`.

## Step 4: Upgrade Scout and set its allowlists on each cluster

Set the zones (the same ones as the account's registration) and the
load-balancer ranges. With `bindy bootstrap scout`:

```bash
bindy bootstrap scout \
  --cluster-name cluster-a \
  --remote-secret bindy-scout-cluster-a-kubeconfig \
  --allowed-zones cluster-a.example.com,cluster-a.example.org \
  --allowed-ip-ranges 10.20.0.0/24,10.20.8.0/24
```

Or, in your own manifests, add to the Scout container's `env`:

```yaml
- name: BINDY_SCOUT_ALLOWED_ZONES
  value: "cluster-a.example.com,cluster-a.example.org"
- name: BINDY_SCOUT_ALLOWED_IP_RANGES
  value: "10.20.0.0/24,10.20.8.0/24"
```

Scout checks both at startup. It exits with an error naming the setting if
an entry is malformed, if `*` is used, or if `--default-zone` or a
`--default-ips` address falls outside them. On a clean start it logs
`Publishing only into the allowed zones and their subzones` and no
allowlist warning.

Verify on the child cluster: a source naming a zone outside the list is
refused there, before anything reaches the bindy cluster.

```bash
kubectl create ingress guardrail-check -n <an opted-in namespace> \
  --rule='www.cluster-b.example.com/*=nothing:80' \
  --annotation=bindy.firestoned.io/scout-enabled=true \
  --annotation=bindy.firestoned.io/zone=cluster-b.example.com
kubectl logs -n bindy-system deploy/bindy-scout | grep guardrail-check
# Expected: Zone is not in this Scout's allowed zones (BINDY_SCOUT_ALLOWED_ZONES),
#           publishing nothing for this source
kubectl delete ingress guardrail-check -n <an opted-in namespace>
```

In same-cluster mode (Scout and the operator on one cluster) step 4 is the
whole migration: there is no remote account to register.

## Step 5: Record the result

- Every account listed by
  `kubectl get sa -n bindy-system -l app.kubernetes.io/component=scout-remote`
  has a `.clusters` and a `.zones` key in `bindy-scout-sources`.
- `bindy-scout-source-binding-binding` has `validationActions: [Deny]`.
- No Scout logs `No --allowed-zones` or `No --allowed-ip-ranges` at startup.
- `kubectl logs deploy/bindy | grep 'labelled for another zone'` is empty.

## Renaming a registered cluster

The registry lists the names an account may stamp, so a rename takes three
steps:

1. Register both names:
   `bindy bootstrap mc --service-account <sa> --cluster-name new,old --allowed-zones <zones> -o r.yaml && rm r.yaml`
2. Restart Scout with `--cluster-name new`. It republishes every record under
   the new name and deletes the old-name records once the new ones are
   served.
3. Register the new name alone:
   `bindy bootstrap mc --service-account <sa> --cluster-name new --allowed-zones <zones> -o r.yaml && rm r.yaml`

If the old name is dropped before step 2 finishes, the bindy cluster refuses
the cleanup. Scout logs `The bindy cluster refused deleting another
cluster's ARecord; leaving it` and does not retry. Delete those records by
hand (`kubectl delete arecords -n bindy-system -l bindy.firestoned.io/source-cluster=old`).

## Splitting a shared account

To give each child cluster its own account:

1. For each child cluster, create and register a new account, then deliver
   its kubeconfig:

    ```bash
    bindy bootstrap mc --service-account bindy-scout-cluster-a \
      --cluster-name cluster-a --allowed-zones cluster-a.example.com -o cluster-a.yaml
    kubectl --context=cluster-a apply -f cluster-a.yaml && rm cluster-a.yaml
    ```

2. Point that cluster's Scout at the new Secret
   (`BINDY_SCOUT_REMOTE_SECRET=bindy-scout-cluster-a-kubeconfig`, or
   `bindy bootstrap scout --remote-secret ...`). Record names come from the
   cluster name, not the account, so no record is renamed or republished.
3. Once every cluster has moved, revoke the shared account:
   `bindy bootstrap mc --revoke --service-account bindy-scout-remote`.

## Rolling back

Each step can be undone on its own:

| To undo | Do |
|---|---|
| The admission policy | `kubectl delete validatingadmissionpolicybinding bindy-scout-source-binding-binding` (accounts become unconstrained again; the registry can stay) |
| A registration | `bindy bootstrap mc --revoke ...` removes the account entirely; to keep the account, delete its two keys: `kubectl edit configmap bindy-scout-sources -n bindy-system` |
| Scout allowlists | Unset `BINDY_SCOUT_ALLOWED_ZONES` / `BINDY_SCOUT_ALLOWED_IP_RANGES` (or re-run `bootstrap scout` without the flags). Sources refused before are published on their next change, or immediately after the Scout restart |
| The zone-label rule | Roll the operator back to the previous release. The rule has no switch: fix the record labels instead where you can |

## Known limits

- Two clusters registered for the **same** zone can still publish the same
  name, and the last record written wins. Register a zone for more than one
  cluster only when they deliberately share it.
- The registry is an ordinary ConfigMap: whoever can write ConfigMaps in the
  records namespace can change it. Scout's own Role cannot.
- Addresses are checked by Scout, not by the admission policy, so a stolen
  token can still point a name in its own zones at any address.

These are recorded as accepted risk 23 in the
[threat model](../security/threat-model.md).

## See also

- [Scout guide: publishing limits](../guide/scout.md#publishing-limits-for-this-cluster-allowed-zones-allowed-ip-ranges)
  and [binding each cluster](../guide/scout.md#binding-each-cluster-to-its-names-and-zones)
- [CLI reference: `bindy bootstrap multi-cluster`](../reference/cli.md#bindy-bootstrap-multi-cluster-alias-mc)
- [Troubleshooting: Scout publishes nothing](troubleshooting.md#scout-publishes-nothing-for-a-source-or-a-scout-record-is-not-served)
- `examples/scout-guardrails-bindy-cluster.yaml`, `examples/scout-guardrails-edge-cluster.yaml`
