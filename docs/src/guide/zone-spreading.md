# Zone Spreading and Pod Placement

Distribute DNS servers across availability zones — or racks, or any other
failure domain your cluster labels nodes with — so a single domain outage
cannot take out every nameserver at once.

## Why this needs its own feature

In most Kubernetes workloads, `replicas: 3` means one Deployment with three
interchangeable Pods behind one Service. Spreading them is a one-line
`topologySpreadConstraint`.

DNS does not work that way. A zone's `NS` records name the *individual*
servers authoritative for it, and resolvers query those names directly. Each
nameserver therefore needs a stable identity and its own address. Bindy models
that by giving every server its own `Bind9Instance`, its own Deployment, its
own Service, and its own RNDC key.

So a cluster like this:

```yaml
spec:
  primary:
    replicas: 3
```

creates **three `Bind9Instance` resources, each backed by a single-Pod
Deployment** — not one three-Pod Deployment.

That distinction is the whole problem. A `topologySpreadConstraint` balances
the set of Pods matched by its `labelSelector`, bucketed by the value of
`topologyKey` on their node. If the constraint selected only its own
Deployment's Pods, the set would have exactly one member — always trivially
balanced — so the constraint would be satisfied by *any* placement and all
three primaries could still land in the same zone.

Bindy solves this with [`scope`](#scope), which generates a selector matching
the sibling instances rather than just the local Deployment. You never write
that selector yourself: it depends on the operator's internal Pod labels, and
getting it wrong fails silently rather than loudly.

## The default

**You usually need no configuration at all.** With two or more **primaries**,
the operator applies a soft zone spread on its own:

```yaml
topologySpreadConstraints:
  - topologyKey: topology.kubernetes.io/zone
    maxSkew: 1
    whenUnsatisfiable: ScheduleAnyway
    labelSelector:
      matchLabels:
        bindy.firestoned.io/cluster: my-dns
        bindy.firestoned.io/role: primary
```

A single-primary cluster gets no constraint, because balancing one Pod is a
no-op.

The default is deliberately **soft** (`ScheduleAnyway`). A hard constraint on a
single-zone cluster — or during the very zone outage this feature guards
against — leaves DNS Pods `Pending`, trading degraded availability for a total
outage. The scheduler still fills zones evenly whenever it can.

### Secondaries have no default

The automatic spread covers primaries only. A primary is authoritative and
holds the writable copy of a zone; a secondary can be rebuilt from a primary at
any time, so losing every primary to one zone outage is the failure that
matters. Just as importantly, defaulting secondaries would silently change
where already-running secondary Pods are scheduled the moment you upgrade the
operator, which is not a decision an operator should make on your behalf.

Secondaries opt in explicitly:

```yaml
spec:
  secondary:
    replicas: 3
    placement:
      spread:
        - topologyKey: topology.kubernetes.io/zone
          maxSkew: 1
          whenUnsatisfiable: ScheduleAnyway
```

## Configuring placement

`placement` can be set at two levels:

| Level | Field | Applies to |
|---|---|---|
| Instance | `Bind9Instance.spec.placement` | one DNS server |
| Role | `spec.primary.placement`, `spec.secondary.placement` on `Bind9Cluster` / `ClusterBind9Provider` | every server of that role |

Resolution is **whole-block**: the more specific level wins outright, and the
other is not merged into it. That keeps "what will actually be scheduled"
answerable by reading one block instead of mentally combining two —
half-inherited scheduling rules are a bad failure mode, because a Pod in the
wrong place is invisible until the outage.

### Spread rules

```yaml
spec:
  primary:
    replicas: 3
    placement:
      spread:
        - topologyKey: topology.kubernetes.io/zone
          maxSkew: 1
          whenUnsatisfiable: DoNotSchedule
          minDomains: 3
          scope: Role
```

| Field | Default | Meaning |
|---|---|---|
| `topologyKey` | *(required)* | Node label defining the failure domain. A Kubernetes qualified name: optional ≤253-char prefix, `/`, ≤63-char name — 317 overall |
| `maxSkew` | `1` | Largest permitted Pod-count difference between domains (1–100) |
| `whenUnsatisfiable` | `ScheduleAnyway` | `DoNotSchedule` for a hard guarantee |
| `scope` | `Role` (cluster-managed) / `Instance` (standalone) | Which Pods are balanced |
| `minDomains` | unset | Fail if fewer domains exist (1–1000). Requires `DoNotSchedule` |
| `nodeAffinityPolicy` | `Honor` | Count only nodes this Pod could land on |
| `nodeTaintsPolicy` | `Ignore` | Whether node taints exclude nodes from counting |

Each rule becomes one `topologySpreadConstraint`; at most 8 per Pod.

Three states, and the difference matters:

- **`spread` absent** — the operator applies its default (primaries only).
- **`spread: []`** — explicitly no constraints. This is the opt-out.
- **`spread: [...]`** — exactly these rules, and no default. This is also how
  secondaries opt in.

`spread` accepts at most 8 rules; the CRD schema rejects more at admission.

### `topologyKey` — any node label works

This is the answer to "our cluster doesn't use `topology.kubernetes.io/zone`."
Any node label is a valid failure domain:

```yaml
spread:
  # On-prem: the real failure domain is the rack.
  - topologyKey: failure-domain.acme.io/rack
    maxSkew: 1
    whenUnsatisfiable: DoNotSchedule
  # Then prefer separate hosts within a rack.
  - topologyKey: kubernetes.io/hostname
    maxSkew: 1
    whenUnsatisfiable: ScheduleAnyway
```

Other useful keys: `topology.kubernetes.io/region` for multi-region clusters,
`karpenter.sh/capacity-type` to balance spot against on-demand capacity.

### `scope`

Decides which Pods the scheduler counts per domain.

| Scope | Selector | Use when |
|---|---|---|
| `Role` | `cluster` + `role` | **Default for cluster-managed instances.** All primaries of the cluster balance against each other |
| `Cluster` | `cluster` | Total DNS footprint per zone matters more than per-role balance. Set it on both role blocks so every Pod carries it |
| `Instance` | the Deployment's own labels | **Default for a standalone `Bind9Instance`** with `replicas > 1` |

`Role` and `Cluster` need an owning cluster — a standalone instance's Pods
carry no cluster label — so they fall back to `Instance` with a warning.

### What `placement` deliberately does not cover

`placement` accepts topology spreading and nothing else. There is no
`nodeSelector`, `tolerations`, or `affinity` passthrough.

Two reasons. Embedding the full Kubernetes `Affinity` and `Toleration` schemas
added roughly 450KB to the generated CRDs — more than the rest of the API
combined. And those three fields are precisely the primitives needed to
schedule a Pod onto a control-plane node; since DNS Pods carry operator-issued
credentials, accepting them would have meant shipping an allow-list validator
and an admission policy to guard a risk that simply does not exist if the
fields are not accepted.

`placement` therefore cannot influence *which* nodes are eligible for a DNS
Pod, only how Pods are balanced across the ones that already are. See
[ADR-0003](https://github.com/firestoned/bindy/blob/main/docs/adr/0003-pod-placement-and-zone-spreading.md).

If you need DNS pinned to a particular node pool today, set `nodeSelector` or
`tolerations` on the generated Deployment out-of-band, or open an issue
describing the case — a small typed API for node tiering can be added without
re-opening this design.

## Validation

Most invalid input is rejected by the API server at admission, from the CRD
schema itself — no optional admission policy required:

| Rule | Enforced by |
|---|---|
| at most 8 spread rules | `maxItems` |
| `topologyKey` is a valid label key, 1–317 chars | `pattern`, `minLength`, `maxLength` |
| `topologyKey` prefix (before `/`) ≤ 253 chars | `x-kubernetes-validations` (CEL) |
| `maxSkew` between 1 and 100 | `minimum` / `maximum` |
| `minDomains` between 1 and 1000 | `minimum` / `maximum` |
| `scope`, `whenUnsatisfiable`, node policies | `enum` |
| `minDomains` only with `whenUnsatisfiable: DoNotSchedule` | `x-kubernetes-validations` (CEL) |

For example:

```console
$ kubectl apply -f bad.yaml
The Bind9Cluster "my-dns" is invalid: spec.primary.placement.spread[0]:
Invalid value: "object": minDomains is only valid together with
whenUnsatisfiable: DoNotSchedule
```

The reconciler re-validates as a backstop — for clusters still running an older
CRD revision — and additionally rejects two rules sharing the same
`(topologyKey, whenUnsatisfiable)` pair, which Kubernetes requires to be unique
and which a structural schema cannot express cheaply. Those surface as a
`Ready=False` condition naming the offending field.

## Verifying it works

The trap here is that a passing spread proves nothing on its own: the
scheduler's default scoring often distributes Pods anyway. Check the
**selector**, not the outcome.

```console
$ kubectl get deploy my-dns-primary-0 \
    -o jsonpath='{.spec.template.spec.topologySpreadConstraints[0].labelSelector.matchLabels}'
{"bindy.firestoned.io/cluster":"my-dns","bindy.firestoned.io/role":"primary"}
```

If that selector names only the instance (`app.kubernetes.io/instance`), it is
balancing a set of one and doing nothing. Confirm the set is the size you
expect:

```console
$ kubectl get pods -l bindy.firestoned.io/cluster=my-dns,bindy.firestoned.io/role=primary
NAME                                  READY   STATUS    RESTARTS   AGE
my-dns-primary-0-6955f99d86-8pp67     2/2     Running   0          3m
my-dns-primary-1-5db9449df9-8z5lx     2/2     Running   0          3m
my-dns-primary-2-77cd7f747b-6skcf     2/2     Running   0          3m
```

And which zones they occupy:

```console
$ kubectl get pods -l bindy.firestoned.io/cluster=my-dns -o wide
```

For a full local verification — including proving a hard constraint is really
enforced — see [Testing on a multi-zone cluster](#testing-on-a-multi-zone-cluster).

## Testing on a multi-zone cluster

kind nodes are containers on one host, so there are no real failure domains.
That does not matter: the scheduler buckets nodes purely by node label, so
labelling three workers reproduces multi-zone scheduling exactly.

```bash
kind create cluster --config deploy/kind-config-multizone.yaml
kubectl apply --server-side -f deploy/operator/crds/
kubectl create namespace bindy-system
kubectl apply -f deploy/operator/rbac/
# Run the operator (in-cluster, or locally against the kind context)
POD_NAMESPACE=bindy-system BINDY_ENABLE_LEADER_ELECTION=false cargo run -- run

# In another shell:
./tests/zone_spread_test.sh
```

The script asserts the selector spans siblings, that `spec.selector` stays
free of the cluster label, that placement changes converge onto existing
Deployments, and — the decisive one — that with two of three zones cordoned
the extra primaries go `Pending` citing topology spread constraints. That last
check can only pass if the cross-instance selector is correct.

## Troubleshooting

**Pods are `Pending` with `didn't match pod topology spread constraints`.**
A hard constraint cannot be satisfied: fewer eligible domains than replicas,
or a `minDomains` floor that is not met. Either add capacity in another
domain, or relax the rule to `whenUnsatisfiable: ScheduleAnyway`.

**Pods all land in one zone anyway.** Check the constraint's selector as
above. Also confirm your nodes actually carry the topology label —
`kubectl get nodes -L topology.kubernetes.io/zone`. A `topologyKey` no node
has means one domain, and one domain is always balanced.

**A `placement` change hasn't taken effect.** The `Bind9Instance` controller
watches `Bind9Cluster` and `ClusterBind9Provider`, so cluster-level edits
normally reach Deployments within seconds. If not, check the instance's
conditions: an invalid block is rejected with `Ready=False` naming the
offending field.

**Note on `kubectl patch --type=merge`.** JSON merge patch does not delete
keys you omit. To clear an entire placement block, send it explicitly as
`null`:

```bash
kubectl patch bind9cluster my-dns --type=merge \
  -p '{"spec":{"primary":{"placement":null}}}'
```

## Related

- [ADR-0003](https://github.com/firestoned/bindy/blob/main/docs/adr/0003-pod-placement-and-zone-spreading.md) — the design and what was deliberately left out
- [Multi-Region Setup](multi-region.md) — spanning regions, not just zones
- [`examples/zone-spreading.yaml`](https://github.com/firestoned/bindy/blob/main/examples/zone-spreading.yaml)
