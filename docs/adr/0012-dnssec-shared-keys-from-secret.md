# 0012: Shared DNSSEC keys from a Secret, copied into a writable key directory

- **Status:** Accepted
- **Date:** 2026-10-04
- **Deciders:** Erick Bourgeois
- **Related:** Amends roadmap 07 (`.github/community/07-dnssec-zone-signing.md`),
  Phase 2 "user-supplied keys"; builds on
  [ADR-0006](0006-dnssec-ds-record-status-reporting.md), whose DS records
  are only stable once every primary signs with the same KSK

## Context

`keysFrom.secretRef` on `spec.dnssec.signing` is meant to let an operator
supply the zone's DNSSEC keys from a Kubernetes Secret (Vault, External
Secrets, sealed-secrets), so the keys survive pod restarts and **every
primary signs with the same keys**. Two primaries that each serve the zone
directly, which is the shape of a cluster with `primary.replicas: 2` behind
two load-balancer addresses, publish a different DNSKEY RRset per pod when
each generates its own keys. A validating resolver then sees a DNSKEY set
that depends on which address it reached, and a parent-zone DS record can
match at most one of them.

The implementation as shipped does not work:

1. **The Secret is mounted directly at the key directory.** Kubernetes
   mounts Secret volumes read-only regardless of `readOnly: false` on the
   mount, and `named`'s `dnssec-policy` must write a `.state` file beside
   every key it manages. BIND either fails to manage the keys or generates
   new ones elsewhere.
2. **The documented key names cannot exist.** BIND names key files
   `K<zone>.+<alg>+<id>.key` / `.private` / `.state`. A Secret's data keys
   must match `[-._a-zA-Z0-9]+`, so `+` is refused by the API server and no
   Secret can hold a key under its BIND name.
3. **Rolling lifetimes diverge.** Even with identical starting keys, a
   policy with a finite `kskLifetime` / `zskLifetime` makes each `named`
   pre-publish and roll a successor on its own schedule, generating a
   different key in each pod. Shared keys only stay shared while no pod
   generates keys.

## Decision

1. **Copy, do not mount.** When `keysFrom.secretRef` is set, the pod gets:
   - the Secret as volume `dnssec-keys-source` (mode `0440`, readable
     through the pod's `fsGroup`), mounted **read-only and only** into an
     init container at `/etc/bind/dnssec-keys-source`;
   - a memory-backed `emptyDir` volume `dnssec-keys`, mounted at
     `/var/cache/bind/keys` (the `key-directory`) in both the init container
     and `named`, so the private key copies never reach the node's disk;
   - the init container `dnssec-keys-init`: the same BIND9 image, the same
     restricted security context as `named` plus a read-only root
     filesystem. It copies each key file into the key directory, restores
     its BIND name and sets it `0600`.

   `named` never mounts the Secret; it sees a writable copy and writes its
   `.state` files beside it.

2. **Key names in the Secret replace each `+` with `_`.** A data key named
   `K<zone>._<alg>_<id>.<ext>`, where `<alg>` is three digits, `<id>` five
   digits and `<ext>` one of `key`, `private`, `state`, is copied as
   `K<zone>.+<alg>+<id>.<ext>`. The match is anchored at the end of the
   name, so an underscore inside the zone name is untouched. Other data keys
   are skipped with a message on the init container's log. **A Secret with
   no key file fails the init container**, and so the pod: signing with no
   supplied key would make `named` generate its own, which is the
   divergence this ADR exists to remove.

3. **Shared keys require `unlimited` lifetimes.** With `secretRef` set,
   `kskLifetime` and `zskLifetime` must be unset (they default to
   `unlimited`) or `unlimited`. The CRD rejects anything else at admission
   (an `x-kubernetes-validations` rule on `DNSSECSigningConfig`), and
   policy rendering refuses it at runtime for a cluster with an older CRD.
   Key rotation becomes an operator action: generate the successor, add it
   to the Secret, roll the pods.

4. **Changing the init containers updates the Deployment.** The update
   path patches `initContainers` as a replace list, and an init container
   added or removed counts as drift, so switching an existing cluster to
   or from `secretRef` reaches its running pods.

## Consequences

- Two or more primaries serve one DNSKEY RRset, and the DS records in
  `DNSZone.status.dnssec` (ADR-0006) are the same whichever pod answered.
- The private key leaves the Secret only into a tmpfs `emptyDir` of the
  pod that signs with it, as a Secret volume would be. `named` has no access to the
  Secret volume itself, and the H2 control (the Secret name must start with
  `bindy-`) still applies to `secretRef`.
- **Automatic key rollover is off** for Secret-supplied keys. That is the
  price of a shared key set without coordination between pods; a
  coordinated rollover (one signer, others transferring the signed zone, or
  the operator generating successors into the Secret) is follow-up work.
- **Editing the Secret does not roll the pods.** The copy happens when a
  pod starts. The operator does not read the Secret (it has no RBAC for
  it), so after a rotation the user restarts the Deployment. A Secret
  content hash on the pod template is follow-up work.
- `persistentVolume` and `autoGenerate` are unchanged: each pod still
  generates and rolls its own keys. Those modes are only correct with a
  single signing primary.
- The CRD doc for `secretRef` is corrected to the `_` encoding. Any Secret
  written to the old documented form could never have been created, so
  nothing existing breaks.
