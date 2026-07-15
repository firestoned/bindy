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
| `05-bindy-rndc-strict-policy.yaml` | `bindy-rndc-algorithm-strict` | **Recommended (default since bindcar 0.7.0).** Rejects `hmac-sha1` in addition to the already-rejected `hmac-md5`. bindcar 0.7.0 refuses both server-side, and the operator's RNDC parsers reject both at runtime; this policy moves the rejection to admission. |
| `06-bindy-rndc-strict-binding.yaml` | binding | Binds the strict RNDC policy. |
| `07-bindy-pod-shape-policy.yaml` | `bindy-pod-shape-validation` | Strict allow-list for `spec.volumes` / `spec.volumeMounts` on `Bind9Instance` / `Bind9Cluster` / `ClusterBind9Provider`. Closes audit finding F-001 (host-fs / foreign-Secret injection via tenant CR). |
| `08-bindy-pod-shape-binding.yaml` | binding | Binds the pod-shape policy with `validationActions: [Deny]`. |
| `09-bindy-dnssec-policy-policy.yaml` | `bindy-dnssec-policy-validation` | Safe-identifier check on DNSSEC policy names (`DNSZone spec.dnssecPolicy`, `Bind9Cluster`/`ClusterBind9Provider` `spec.global.dnssec.signing.policy`, `Bind9Instance spec.config.dnssec.signing.policy`) **and** safe-token check on the sibling signing params `algorithm`/`kskLifetime`/`zskLifetime` under `*.dnssec.signing`. Closes audit findings B-6 (injection via the quoted `dnssec-policy "<name>"` literal) and B-6b (injection via the unquoted lifetime/algorithm values in the `dnssec-policy { ... }` block). |
| `10-bindy-dnssec-policy-binding.yaml` | binding | Binds the DNSSEC-policy-name policy with `validationActions: [Deny]`. |
| `11-bindy-operator-workload-sa-policy.yaml` | `bindy-operator-workload-sa-validation` | Constrains workloads created **by the operator SA** (`system:serviceaccount:bindy-system:bindy`): the pod template `serviceAccountName` must be `bind9`. Compensating control for audit finding C2 (a compromised operator token could otherwise run Pods as any SA and escalate). Stop-gap until the operator runs namespace-scoped. |
| `12-bindy-operator-workload-sa-binding.yaml` | binding | Binds the operator-workload-SA policy with `validationActions: [Deny]`. |
| `13-bindy-record-value-policy.yaml` | `bindy-record-value-validation` | Defense-in-depth on DNS record target values (`CNAMERecord.spec.target`, `MXRecord.spec.mailServer`, `NSRecord.spec.nameserver`, `SRVRecord.spec.target`): must be an absolute FQDN (trailing dot), match `[A-Za-z0-9._-]`, ≤253 octets, no leading/consecutive dots. Mirrors bindcar 0.7.0's stricter record-value validation (migration §3) at the API server, since bindy sends most records over the RFC 2136 path rather than the bindcar HTTP API. |
| `14-bindy-record-value-binding.yaml` | binding | Binds the record-value policy with `validationActions: [Deny]`. |
| `15-bindy-image-provenance-policy.yaml` | `bindy-image-provenance-validation` | Registry/immutability allow-list on the image overrides tenants can set via CRs (`spec[.common].image.image`, `spec[.common].bindcarConfig.image`): bindcar from `ghcr.io/firestoned/`, BIND9 from ISC or a firestoned mirror, and every reference digest-pinned or a named non-`:latest` tag. CEL cannot verify Cosign signatures — pair with Kyverno / policy-controller for cryptographic admission (see `docs/src/security/signed-releases.md`). |
| `16-bindy-image-provenance-binding.yaml` | binding | Binds the image-provenance policy with `validationActions: [Deny]`. |

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
kubectl apply -f deploy/admission-policies/09-bindy-dnssec-policy-policy.yaml
kubectl apply -f deploy/admission-policies/10-bindy-dnssec-policy-binding.yaml

# Record target-value validation (bindcar 0.7.0 §3: FQDN trailing dot + charset
# on CNAME/MX/NS/SRV targets). Safe to deploy alongside any existing install.
kubectl apply -f deploy/admission-policies/13-bindy-record-value-policy.yaml
kubectl apply -f deploy/admission-policies/14-bindy-record-value-binding.yaml

# Pod-shape allow-list (closes F-001 — host-fs / foreign-Secret injection;
# also closes C1 mountPath traversal and H2 DNSSEC keysFrom foreign-Secret).
# Strongly recommended on any multi-tenant cluster.
kubectl apply -f deploy/admission-policies/07-bindy-pod-shape-policy.yaml
kubectl apply -f deploy/admission-policies/08-bindy-pod-shape-binding.yaml

# Operator-workload SA guard (compensating control for C2 — blocks the
# operator token from running Pods as any SA other than 'bind9').
# Recommended wherever the operator runs cluster-wide RBAC.
kubectl apply -f deploy/admission-policies/11-bindy-operator-workload-sa-policy.yaml
kubectl apply -f deploy/admission-policies/12-bindy-operator-workload-sa-binding.yaml

# Recommended (default since bindcar 0.7.0): posture-strict RNDC (rejects
# hmac-sha1 in addition to hmac-md5). bindcar 0.7.0 rejects hmac-sha1 anyway, so
# this only moves the rejection earlier (to admission). Verify nothing in your
# cluster still uses hmac-sha1 before applying.
kubectl apply -f deploy/admission-policies/05-bindy-rndc-strict-policy.yaml
kubectl apply -f deploy/admission-policies/06-bindy-rndc-strict-binding.yaml

# Or apply the whole directory at once:
kubectl apply -f deploy/admission-policies/
```

### Single-file install (releases)

Each release publishes a combined `admission-policies.yaml` that bundles **all
fourteen** documents (every policy + binding, including the RNDC-strict and
record-value policies). Apply it in one shot:

```bash
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/admission-policies.yaml
```

> ⚠️ The bundle includes the RNDC-strict policy (`05`/`06`), which rejects
> `hmac-sha1` RNDC keys. Verify nothing in your cluster still uses `hmac-sha1`
> before applying, or install the individual files above instead.

Regenerate the bundle locally with `make admission-policies-yaml` (also run as
part of `make release-manifests`).

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

### `bindy-rndc-algorithm-strict` (recommended, default since bindcar 0.7.0)

The CRD enum already rejects `hmac-md5` after the H4 fix. This policy
*also* rejects `hmac-sha1` for environments that need to enforce the
SOC 2 / FIPS 140-3 posture. bindcar 0.7.0 refuses `hmac-sha1` server-side
and the operator's RNDC secret parsers reject it at runtime, so this is
now recommended by default; enable only after confirming no production
keys still use SHA-1.

### `bindy-record-value-validation`

bindcar 0.7.0 tightened validation of DNS record values (migration §3):
`CNAME` / `NS` / `PTR` / `MX` targets must be fully-qualified and end with
a trailing dot, and no value may contain control characters. Because bindy
sends most records over the RFC 2136 dynamic-update path (not the bindcar
HTTP API), bindcar's own validation does not always see these fields, so a
target such as `example.com` (no trailing dot) would be silently treated as
relative to the zone origin. This policy enforces the same contract on
`CNAMERecord.spec.target`, `MXRecord.spec.mailServer`,
`NSRecord.spec.nameserver`, and `SRVRecord.spec.target`:

1. Must end with `.` (absolute FQDN).
2. Character set restricted to `[A-Za-z0-9._-]` (rejects whitespace,
   control chars, and `;`, `{`, `}`, `"`, `\`, `$`).
3. RFC 1035 length cap (253 octets).
4. No leading `.` and no consecutive dots `..`.

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
