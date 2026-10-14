# 0023: A `secretRef` RNDC key is mirrored into an operator-owned Secret

- **Status:** Accepted
- **Date:** 2026-10-14
- **Proposed:** 2026-10-14
- **Deciders:** Erick Bourgeois
- **Amended:** 2026-10-14 (Decision 1: the mirrored key is always named `bindy-operator`; `keyNameKey` is deprecated and ignored)
- **Related:** Extends the RNDC key modes of `rndcKey` (`Bind9Instance`, `Bind9Cluster`, `ClusterBind9Provider`); rollouts as in [ADR-0018](0018-staggered-bind9-rollouts.md); roadmap 30 follow-up

## Context

`rndcKey.secretRef` points an instance at a user-managed Secret. Its
`keyNameKey` and `secretKey` fields (defaults `key-name` and `secret`) say
which entries of that Secret hold the key name and the key. No code reads
them:

- The BIND9 pod mounts the referenced Secret at `/etc/bind/keys/` and
  `named.conf` includes `/etc/bind/keys/rndc.key`. Only the `rndc.key` entry
  is ever used by `named` and bindcar.
- The operator's key loader (`load_rndc_key`, used for every DNS UPDATE and
  bindcar call) reads the Secret the Deployment mounts and accepts either the
  fixed `key-name` / `algorithm` / `secret` entries or `rndc.key`.

A Secret that holds its key under custom entry names, with no `rndc.key`,
therefore never worked: the pod has no key file and the loader finds no key.
A volume mount can rename entries (`items`), but it cannot build an
`rndc.key` file out of two separate values.

There is a second defect in the same mode. When the user changes their
Secret, the loader reads the new key at once, but the running pods keep the
old one until something restarts them. The instance controller watches only
Secrets it owns, so nothing does. Every write then fails until the pods are
restarted by hand.

## Decision

1. **The operator reads the referenced Secret and builds the key from it.**
   In order:
   - if the Secret holds the `secretKey` entry, it gives the key; the
     algorithm is the Secret's `algorithm` entry when present, else
     `secretRef.algorithm`;
   - otherwise, if it holds `rndc.key`, that file is parsed (today's external
     format) for its algorithm and key;
   - otherwise the reconcile fails with a reason naming the entries it looked
     for.

   The key is validated as every RNDC key is (algorithm allow-list, no
   HMAC-MD5 or HMAC-SHA1) and must be non-empty standard base64.

   **Amended 2026-10-14:** the mirrored key is always named
   `bindy-operator`, whatever name the source gives it. `named.conf`'s
   `controls` block accepts only that name, and the file is rendered once in
   the cluster-shared ConfigMap for every instance of both roles, which may
   use different keys; the kind e2e showed a key under any other name
   crash-loops `named` (as an external `rndc.key` with another name always
   did). `keyNameKey` is therefore deprecated and ignored, and the only text
   the mirror takes from the source is the base64 key and the algorithm.
2. **The pods mount an operator-owned mirror.** The operator writes the key,
   in the operator's own four-entry format (`key-name`, `algorithm`,
   `secret`, `rndc.key`), to `{instance}-rndc-key`, owned by the instance and
   annotated `bindy.firestoned.io/rndc-source-secret: <name>`. Every pod
   mounts `{instance}-rndc-key` in every mode, so the loader and the pods
   read the same Secret in the same format, and the custom entry names are
   read in exactly one place.
3. **A change to the source Secret reaches the pods.** The instance
   controller watches the Secrets its instances reference and reconciles
   those instances when one changes. When the mirror's key differs from the
   source's, the operator rewrites the mirror, drops its cached key, and
   rolls the pods with the `bindy.firestoned.io/rndc-rotated-at` pod-template
   annotation, as an automatic rotation does.
4. **A mirror is never rotated and never mistaken for a generated key.** A
   Secret carrying the source annotation is skipped by automatic rotation;
   switching an instance from `secretRef` back to a generated key replaces
   the mirror with a fresh generated key. Any replacement of an existing
   key under `{instance}-rndc-key` (a mirror change, a mode switch, a
   generated key recreated after an algorithm change) drops the cached key
   and rolls the pods, since the pod template does not otherwise change.
5. **Fail fast, never destroy.**
   - A `secretRef` naming `{instance}-rndc-key` itself is refused (the
     operator would overwrite the user's Secret).
   - A referenced Secret that is missing or unreadable fails the reconcile
     and leaves the existing mirror in place, so running pods keep their key
     and DNS keeps working while the user fixes the reference.
   - A GET of the mirror that fails with anything but 404 fails the
     reconcile instead of being treated as "absent" (it is today, which
     deletes and regenerates a generated key).
6. **A `secretRef` must name a `bindy-` Secret.** The referenced Secret's
   name must start with `bindy-`, the allow-list prefix bug-021 introduced
   for DNSSEC key Secrets. Without it, anyone who can create a
   `Bind9Instance`, `Bind9Cluster` or `ClusterBind9Provider` can make the
   operator read any Secret in the namespace and place two of its entries
   in a Secret the BIND9 pods mount. The rule is enforced twice, as for
   DNSSEC keys: the `bindy-pod-shape-validation` admission policy rejects
   the resource at admission, and the instance reconciler refuses it (not
   Ready, no Secret written) where the policy is not installed.

## Consequences

- `secretKey` works as documented, and a Secret with only `rndc.key` keeps
  working, whatever key name it carries (it used to crash-loop `named` unless
  the name was `bindy-operator`). `keyNameKey` is deprecated: BIND9 sees
  every bindy RNDC key as `bindy-operator`, so an external tool signing with
  the same key must use that name.
- Editing a referenced Secret now rolls the instance's pods, instead of
  breaking every write until a manual restart.
- **Visible change:** instances on `secretRef` change their pod template
  once on upgrade (the volume now names `{instance}-rndc-key`) and roll once.
- The operator holds a copy of user key material in the instance's
  namespace. It already reads that Secret for every write, and it already
  writes generated keys to the same name in the same namespace with the same
  RBAC, so no grant changes; the copy is owned by the instance and deleted
  with it. The threat model records the copy and its lifetime.
- **Breaking:** a `secretRef` naming a Secret without the `bindy-` prefix
  is rejected at admission and refused by the reconciler. Such an instance
  keeps its running pods (nothing is deleted) but stops reconciling until
  the Secret is renamed or copied to a `bindy-` name. The migration guide
  records it. Together with the mirror, the pods no longer see every entry
  of an arbitrary Secret: they see only the two entries of a `bindy-` one
  that the key is built from.
- The key change rollout bypasses the ADR-0018 queue, as an automatic
  rotation does today: an instance with a stale key cannot answer writes,
  so waiting behind another instance only extends the outage.
- The inline `rndcKey.secret` mode is unchanged (still generated; out of
  scope).
- CALM gains the RNDC key Secrets: the user's source Secret and the
  operator-owned mirror the pods mount.
