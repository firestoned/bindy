# ADR-0003: Pod Placement and Zone Spreading

## Status

Accepted

## Context

The Deployments the operator builds (`build_deployment` in
`src/bind9_resources.rs`) set no scheduling constraints at all — no
`affinity`, no `nodeSelector`, no `topologySpreadConstraints`. On a multi-zone
cluster the scheduler is therefore free to place every primary DNS Pod in a
single zone. A zone outage then takes out all authoritative DNS at once,
defeating the point of running multiple primaries. This is issue #467.

### The complication: a DNS server is not an anonymous replica

The obvious fix — one `topologySpreadConstraint` on the pod template — does
not work here, because of how Bindy models nameservers.

A zone's `NS` records name the *individual* servers authoritative for it, and
resolvers query those names directly. Each nameserver therefore needs a stable
identity and its own address. Bindy gives every server its own
`Bind9Instance`, Deployment, Service, and RNDC key, and
`create_managed_instance_with_owner`
(`src/reconcilers/bind9cluster/instances.rs`) hardcodes `replicas: 1` on each
generated instance.

So `Bind9Cluster.spec.primary.replicas: 3` produces **three single-Pod
Deployments**, not one three-Pod Deployment.

A `topologySpreadConstraint` balances the set of Pods matched by its
`labelSelector`, bucketed by `topologyKey`. A selector matching only the local
Deployment's Pods would describe a set of size one — always trivially balanced
— so the constraint would be satisfied by any placement and all three
primaries could still land in one zone. The constraint would appear to work
and do nothing.

Two further constraints shaped the design:

- **`spec.selector` is immutable.** The label set built by
  `build_labels_from_instance` fed both the Deployment's `spec.selector` and
  its pod template. Any new label added there would change the selector, which
  Kubernetes rejects, wedging the reconciler on every Deployment that already
  exists.
- **The update path was an allow-list.** `deployment_needs_update`
  (`src/reconcilers/bind9instance/resources.rs`) compared only replicas and
  the bindcar container, and the strategic-merge patch below it sent only
  those fields. Any new pod-spec field would have been written once at
  creation and never reconciled again.

## Decision

### 1. A `placement.spread` API, scoped to topology spreading only

```yaml
placement:
  spread:
    - topologyKey: topology.kubernetes.io/zone
      maxSkew: 1
      whenUnsatisfiable: DoNotSchedule
      minDomains: 3
      scope: Role
```

Settable at two levels, resolved whole-block, most specific first:

1. `Bind9Instance.spec.placement`
2. `spec.primary.placement` / `spec.secondary.placement`

Resolution is whole-block rather than field-by-field so that "what will
actually be scheduled" is answerable by reading one block. A silently
half-inherited scheduling rule is a bad failure mode: a Pod in the wrong place
is invisible until the outage it was supposed to prevent.

### 2. `scope` generates the label selector

`scope` is the field that makes spreading work at all:

| Scope | Generated selector | Balanced set |
|---|---|---|
| `Role` (default, cluster-managed) | `bindy.firestoned.io/cluster` + `role` | all primaries of the cluster |
| `Cluster` | `bindy.firestoned.io/cluster` | every DNS Pod of the cluster |
| `Instance` (default, standalone) | the Deployment's own labels | that instance's replicas |

Users never write the selector themselves. It depends on the operator's
internal pod labels, which are not part of the public contract, and a wrong
selector fails silently rather than loudly.

This required adding `bindy.firestoned.io/cluster` to the pod labels. Because
`spec.selector` is immutable, the single label map was split in two:
`build_labels_from_instance` stays frozen and owns the selector;
`build_pod_labels_from_instance` is a superset applied to the pod template
only. Kubernetes requires the selector to *match* the template, not to equal
it, so a template may carry extra labels.

### 3. The default applies to primaries only, and is soft

With no `placement` anywhere, a primary role with two or more servers gets one
`ScheduleAnyway`, `maxSkew: 1` constraint over
`topology.kubernetes.io/zone`. Secondaries and single-server roles get
nothing.

**Soft, not hard**: a hard default turns a single-zone cluster — or the zone
outage this feature exists to guard against — into `Pending` DNS Pods, trading
degraded availability for a total outage.

**Primaries only** for two reasons. A primary is authoritative and holds the
writable copy of a zone; a secondary can be rebuilt from a primary at any
time, so losing every primary to one zone is the failure that matters. And
defaulting secondaries would silently change where already-running secondary
Pods are scheduled the moment an operator is upgraded — an unannounced
scheduling policy change on a live cluster. Secondaries opt in explicitly via
`secondary.placement.spread`.

### 4. No `nodeSelector`, `tolerations`, or `affinity` passthrough

An earlier revision of this work accepted the full Kubernetes `Affinity`,
`Toleration`, and node-selector types at four API levels, as an escape hatch.
That was reverted. See Consequences.

### 5. Validation is structural where it can be

The generated CRD schema carries `maxItems: 8` on `spread`, a label-key
`pattern` on `topologyKey`, `minimum`/`maximum` on the numeric fields, enums
on `scope` / `whenUnsatisfiable` / the node policies, and an
`x-kubernetes-validations` CEL rule for the `minDomains` ⇒ `DoNotSchedule`
pairing. All of it is emitted from `src/crd.rs` via `schemars`, so it needs no
hand-editing of generated YAML and no optional admission policy to be
enforced.

`src/placement.rs::validate_placement` remains as a reconcile-time backstop
for clusters running an older CRD revision, and covers the one rule a
structural schema cannot express cheaply: uniqueness of
`(topologyKey, whenUnsatisfiable)` across rules.

#### `topologyKey` limits, measured rather than assumed

`topologyKey` must be a valid Kubernetes qualified name. The exact bounds were
verified against a live API server rather than inferred, because two of them
are easy to get wrong:

- **The maximum length is 317, not 316.** A qualified name is a prefix of up to
  253 characters, a `/`, and a name of up to 63 — so a maximal key is 317
  characters and is accepted. An earlier revision capped at 316 and rejected it.
- **There is no 63-character cap on an individual DNS label.** RFC 1123 caps a
  DNS label at 63 octets, but Kubernetes' `IsDNS1123Subdomain` bounds only the
  253-character total and the charset. A key such as `<64 a's>/zone` is
  accepted by the API server both as a node label and as a `topologyKey`.
  Bindy therefore does not reject it: refusing input the platform accepts would
  be a worse failure than mirroring the platform's own looseness.
- **The 253-character prefix cap is enforced by CEL, not by the pattern.** CRD
  `pattern` is RE2, which has no lookahead, so a length bound on a
  variable-length prefix cannot be expressed in the regex. The rule
  `self.topologyKey.indexOf('/') <= 253` covers it (`indexOf` returns the
  prefix length, or `-1` when there is no prefix).

The generated schema is itself asserted by unit tests
(`generated_schema_*` in `src/placement_tests.rs`), so dropping a `#[schemars]`
attribute fails CI rather than silently moving enforcement from admission time
to reconcile time.

### 6. Cluster-level config is resolved live, and instances are woken by a watch

Role-level `placement` is resolved against the live cluster object when the
Deployment is built, rather than copied down into every managed
`Bind9Instance` spec. That avoids rewriting N instance specs on every
cluster-level edit, but means an instance is not otherwise notified. The
`Bind9Instance` controller therefore watches `Bind9Cluster` and
`ClusterBind9Provider` (`src/main.rs`), mapping a cluster event to its
instances. Measured propagation went from ~240s (the 5-minute requeue) to
sub-second.

## Alternatives considered

### `spec.spreadAcrossZones: true`

Suggested in issue #467. Rejected: a boolean cannot express a custom topology
key (clusters that do not label nodes with `topology.kubernetes.io/zone`),
soft versus hard, `minDomains`, or — decisively — the scope question, which is
the difference between a working constraint and a no-op.

### Pod anti-affinity instead of topology spread

`preferredDuringSchedulingIgnoredDuringExecution` keyed on zone. Rejected:
anti-affinity can only express "not together", which stops being useful as
soon as there are more replicas than domains. Topology spread degrades
gracefully, supports `maxSkew` and `minDomains`, and is the modern, cheaper
scheduler plugin.

### Full pod-spec passthrough (`affinity` / `tolerations` / `nodeSelector`)

Implemented first, then removed on review. Rejected on three grounds:

- **CRD size.** The embedded Kubernetes schemas added ~451KB across the three
  cluster CRDs (`bind9clusters` 275KB → 469KB). Removing them brings the
  feature's total cost to ~48KB.
- **Security surface.** Those three fields are exactly the primitives a
  namespace tenant needs to schedule a Pod carrying operator-issued
  credentials onto a control-plane node. Accepting them required an
  allow-list validator plus a `ValidatingAdmissionPolicy` mirroring it —
  mitigating a threat model that simply does not exist if the fields are not
  accepted. Not accepting them deletes the problem instead of guarding it.
- **Scope.** Issue #467 asked for zone spreading. Node tiering is a separate
  request that deserves its own design, and can be added later as a small
  typed API without re-litigating this one.

Users who genuinely need raw affinity or tolerations today can set them on the
generated Deployment out-of-band, or open an issue describing the case.

### Unstructured passthrough (`x-kubernetes-preserve-unknown-fields`)

Would have kept the escape hatch at near-zero CRD cost. Rejected: it
sacrifices all schema validation and `kubectl explain`, and does nothing about
the control-plane security surface, which was the more serious of the two
problems.

## Consequences

### Positive

- Multi-primary clusters spread across zones by default, with no
  configuration.
- Any node label works as a failure domain, so on-prem clusters can spread
  across racks or cells.
- The feature costs ~48KB of CRD schema and adds no admission policy.
- No new privilege-escalation surface: `placement` cannot influence *which*
  nodes are eligible, only how Pods are balanced across the ones that already
  are.
- Bad input is rejected at admission by the API server, not discovered several
  reconciles later.

### Negative

- The three cluster CRDs grow by ~19KB each. They already exceeded the 256KB
  `last-applied-configuration` annotation limit on `main` (275KB), so
  client-side `kubectl apply` was already failing; installation docs, the
  `crdgen` output, and the combined `crds.yaml` header now state
  `--server-side` explicitly.
- Adding the `bindy.firestoned.io/cluster` pod label changes the pod template
  hash, so every DNS Pod restarts once on the upgrade that introduces it.
  Restarts are staggered across instances, so a zone stays served, but this
  belongs in release notes. There is still no PodDisruptionBudget in the repo
  (tracked separately).
- `scope` is a Bindy-specific concept users must learn. It is documented in
  `docs/src/guide/zone-spreading.md`, and the default is correct for both the
  cluster-managed and standalone shapes, so most users never set it.
- Node tiering (`nodeSelector` / `tolerations`) is not available through the
  CRD. This is a deliberate deferral, not an oversight.

## Verification

`tests/zone_spread_test.sh` runs against the three-zone kind cluster defined in
`deploy/kind-config-multizone.yaml`. The load-bearing assertion is not "Pods
landed in different zones" — the scheduler's default scoring often achieves
that on its own, so a passing spread proves nothing. It asserts instead that
the generated selector matches all sibling primaries, that `spec.selector`
stays free of the cluster label, that placement changes (including removals)
converge onto existing Deployments, that Deployments predating the feature are
repaired in place, and — decisively — that with two of three zones cordoned
the extra primaries go `Pending` citing topology spread constraints. That last
check can only pass if the cross-instance selector is correct.

## References

- Issue [#467](https://github.com/firestoned/bindy/issues/467)
- `src/placement.rs`, `src/crd.rs` (`PlacementConfig`, `SpreadRule`, `SpreadScope`)
- `docs/src/guide/zone-spreading.md`
- [Kubernetes: Pod Topology Spread Constraints](https://kubernetes.io/docs/concepts/scheduling-eviction/topology-spread-constraints/)
