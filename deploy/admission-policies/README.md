# Bindy ValidatingAdmissionPolicies

CEL-based admission validation that rejects malformed bindy CRDs at the
Kubernetes API server **before** they reach etcd or the operator. This is
defense-in-depth on top of the in-process Rust validators in
`src/bind9_acl.rs` and the CRD JSONSchema enums.

## What's in this directory

| File | Policy | Purpose |
|------|--------|---------|
| `01-bindy-acl-policy.yaml` | `bindy-acl-validation` | Strict whitelist for every `allowQuery` / `allowTransfer` entry on `Bind9Cluster`, `Bind9Instance`, `ClusterBind9Provider`. Rejects the H1 named.conf injection shape. |
| `02-bindy-acl-binding.yaml` | binding | Binds the ACL policy with `validationActions: [Deny]`. |
| `03-bindy-zone-name-policy.yaml` | `bindy-zone-name-validation` | Defense-in-depth checks on `spec.zoneName` and per-record `spec.zoneRef.zoneName` / `spec.recordName` across all DNS resources. |
| `04-bindy-zone-name-binding.yaml` | binding | Binds the zone-name policy. |
| `05-bindy-rndc-strict-policy.yaml` | `bindy-rndc-algorithm-strict` | **Optional, opt-in.** Rejects `hmac-sha1` in addition to the already-rejected `hmac-md5`. |
| `06-bindy-rndc-strict-binding.yaml` | binding | Binds the strict RNDC policy. |
| `07-bindy-pod-shape-policy.yaml` | `bindy-pod-shape-validation` | Strict allow-list for `spec.volumes` / `spec.volumeMounts` on `Bind9Instance` / `Bind9Cluster` / `ClusterBind9Provider`. Closes audit finding F-001 (host-fs / foreign-Secret injection via tenant CR). |
| `08-bindy-pod-shape-binding.yaml` | binding | Binds the pod-shape policy with `validationActions: [Deny]`. |

F-003 (cross-namespace zone hijack) is enforced operator-side via the
`bindy.firestoned.io/allow-zone-namespaces` annotation on `Bind9Instance`
(see `src/reconcilers/dnszone/validation.rs::get_instances_from_zone`).
There is no admission policy for it because the gate's input — metadata
on the platform-owned target instance — isn't visible during `DNSZone`
admission.

## Requirements

- Kubernetes **1.30+** (`admissionregistration.k8s.io/v1` GA).
- For 1.28–1.29 use `admissionregistration.k8s.io/v1beta1` and ensure the
  `ValidatingAdmissionPolicy` feature gate is on.
- The policies use the CEL `optionals` library (`?` field access,
  `orValue`) which is enabled by default in `v1`.

## Install

```bash
# Apply core policies (ACL + zone names). These are safe to deploy
# alongside any existing bindy install.
kubectl apply -f deploy/admission-policies/01-bindy-acl-policy.yaml
kubectl apply -f deploy/admission-policies/02-bindy-acl-binding.yaml
kubectl apply -f deploy/admission-policies/03-bindy-zone-name-policy.yaml
kubectl apply -f deploy/admission-policies/04-bindy-zone-name-binding.yaml

# Pod-shape allow-list (closes F-001 — host-fs / foreign-Secret injection).
# Strongly recommended on any multi-tenant cluster.
kubectl apply -f deploy/admission-policies/07-bindy-pod-shape-policy.yaml
kubectl apply -f deploy/admission-policies/08-bindy-pod-shape-binding.yaml

# Optional: posture-strict RNDC (rejects hmac-sha1).
# Verify nothing in your cluster is using hmac-sha1 first.
kubectl apply -f deploy/admission-policies/05-bindy-rndc-strict-policy.yaml
kubectl apply -f deploy/admission-policies/06-bindy-rndc-strict-binding.yaml

# Or apply the whole directory at once:
kubectl apply -f deploy/admission-policies/
```

## Roll-out tip: `Audit` before `Deny`

To preview what the policies would block without breaking existing
resources, edit each binding and switch `validationActions` from `[Deny]`
to `[Audit]`. Failures will appear as `validation.policy.admission.k8s.io/<policy>`
keys in the API server audit log without rejecting the request:

```yaml
spec:
  validationActions:
    - Audit
```

After a soak window, switch back to `[Deny]`.

## Testing the policies

Each policy ships with a corresponding fixture pair under `tests/`:

```
tests/
├── accept-bind9cluster.yaml      # should pass admission
├── reject-bind9cluster-acl.yaml  # should be rejected by bindy-acl-validation
├── reject-dnszone-bad-name.yaml  # should be rejected by bindy-zone-name-validation
└── ...
```

Use `kubectl apply --dry-run=server` to verify (server-side dry-run runs
admission policies but does not persist):

```bash
# Should succeed
kubectl apply --dry-run=server -f deploy/admission-policies/tests/accept-bind9cluster.yaml

# Should fail with the policy's messageExpression
kubectl apply --dry-run=server -f deploy/admission-policies/tests/reject-bind9cluster-acl.yaml
```

A convenience Makefile target runs the full fixture suite:

```bash
make admission-policies-test
```

## What each policy catches

### `bindy-acl-validation`

The H1 audit finding showed that CRDs accepted arbitrary strings in
`allowQuery` / `allowTransfer` and `join("; ")`-ed them straight into the
`named.conf` `{ … }` block. A payload like

```yaml
allowQuery:
  - 'any; }; zone "evil" { type master; file "/etc/passwd"; }; acl x { any'
```

would close the ACL block and inject arbitrary BIND9 directives. This
policy:

1. Caps the list at 256 entries.
2. Caps each entry at 256 bytes.
3. Rejects entries containing `; { } " \\` or whitespace control chars
   with a message that names the offending entries.
4. Requires every entry to match the accepted-token whitelist:
   `any | none | localhost | localnets | <IPv4>[/0-32] | <IPv6>[/0-128]
   | key <name>`, each optionally `!`-negated.

### `bindy-zone-name-validation`

DNS names flow into BIND9 zone declarations and into RFC 2136 dynamic
updates. The CRD already enforces a regex pattern; this policy adds:

1. RFC 1035 length cap (253 octets).
2. Rejection of `; { } " \\` and whitespace.
3. Rejection of leading `.`, leading `-`, or `..`.
4. Rejection of bare-IPv4 zone names (a frequent misconfiguration).

### `bindy-rndc-algorithm-strict` (opt-in)

The CRD enum already rejects `hmac-md5` after the H4 fix. This policy
*also* rejects `hmac-sha1` for environments that need to enforce the
SOC 2 / FIPS 140-3 posture. Enable only after confirming no production
keys still use SHA-1.

## Audit annotations

`bindy-acl-validation` sets an audit annotation
(`bindy-acl-rejected-entries`) on every denied request, so security
teams can trace rejected admissions through the API server audit log
without having to reconstruct the offending payload.

## Removing the policies

```bash
kubectl delete -f deploy/admission-policies/
```

The bindy operator continues to function; the in-process Rust ACL
validator remains the last line of defense.
