# 0006 — DNSSEC DS record extraction and status reporting

- **Status:** Accepted
- **Date:** 2026-09-27
- **Deciders:** Erick Bourgeois
- **Related:** Completes roadmap 07 Phase 5 (`.github/community/07-dnssec-zone-signing.md`)

## Context

DNSSEC zone signing (roadmap 07, Phases 1–4) is implemented: `dnssec-policy`
blocks are rendered into `named.conf`, key sources (Secret-backed,
auto-generated, PVC) are wired into the operand pod, and zones are created
with `dnssec-policy`/`inline-signing` via bindcar. What is missing is the last
hop of the chain of trust: the **DS (Delegation Signer) records** an operator
of the parent zone must publish. Today a user has to exec into a pod and run
`dnssec-dsfromkey` by hand; the `DNSZone.status.dnssec` struct
(`DNSSECStatus`) exists in the CRD but nothing populates it, and
`verify_zone_signed()` in `zone_ops.rs` has no callers.

DS records are pure derivations of the KSK DNSKEY: key tag (RFC 4034 App. B),
algorithm number, and a digest of `owner name || DNSKEY RDATA`. Three ways to
obtain them were considered:

1. **Query DNSKEY over DNS from the operator and compute DS locally.** The
   operator is already a DNS client of the operands (hickory-net over UDP
   5353 for record verification and RFC 2136 updates), and
   `hickory-proto`'s ring-backed `dnssec` feature (already enabled) provides
   `DNSKEY::calculate_key_tag()` and `DNSKEY::to_digest()`.
2. **Add a DS/DNSKEY endpoint to bindcar.** New API surface, a bindcar
   release and version bump, and a second copy of data the DNS plane already
   serves authoritatively.
3. **Exec `dnssec-dsfromkey` in the operand pod.** Requires `pods/exec` RBAC
   the operator deliberately does not have, and shells out for what is a
   ~20-line pure computation.

## Decision

Option 1. After a zone with a DNSSEC policy is configured on its primaries,
the DNSZone reconciler queries DNSKEY on the first configured endpoint (the
same endpoint NOTIFY already targets, port-swapped to the operand DNS port
5353), filters for KSKs (`zone_key && secure_entry_point && !revoke`),
computes the key tag and the **SHA-256** digest (digest type 2 — the RFC 8624
mandatory type; SHA-1 is not emitted), and publishes into
`DNSZone.status.dnssec`:

- `signed: true` with `dsRecords` in presentation format
  (`<zone>. IN DS <keytag> <algorithm> 2 <digest>`), `keyTag`, and the
  algorithm mnemonic, when KSK DNSKEYs are present;
- `signed: false` with empty `dsRecords` when the policy is applied but no
  DNSKEY exists yet (keys still generating) — the periodic requeue refreshes
  it;
- `dnssec: null` (cleared) when the zone has no effective DNSSEC policy or
  the policy is `"none"`.

A DNS query failure only logs a warning and leaves the previous status
untouched — DS reporting must never fail the reconcile, since the zone itself
is healthy. The status is merged through the existing `ZoneStatusUpdate`
builder so it rides the same guarded status patch (and its retry) as
conditions and records. A `DNSSEC` print column on the DNSZone CRD surfaces
`.status.dnssec.signed` in `kubectl get`.

`nextKeyRollover`/`lastKeyRollover` stay unpopulated: they require reading
BIND's key state files, which is bindcar API surface — recorded as a future
enhancement in roadmap 07, not silently guessed from policy lifetimes.

## Consequences

- The DNSSEC chain of trust becomes self-service: `kubectl get dnszone -o
  jsonpath='{.status.dnssec.dsRecords}'` yields exactly what the parent zone
  needs; no pod exec, no extra RBAC, no bindcar release.
- The operator gains one more in-cluster read path, operator → `named`
  UDP 5353 (modeled in CALM). It is unauthenticated but read-only public DNS
  data; DS records and key tags are public by design, and no key material
  ever reaches the status.
- DS records derive from a single primary endpoint. Primaries share keys via
  the common key volume, so per-endpoint divergence would indicate a broken
  deployment, not a reporting bug; cross-checking endpoints (or CDS/CDNSKEY
  RFC 7344/8078 automation) is a future enhancement.
- Automated DS publication to parent zones remains out of scope (roadmap 07
  open question 2: registrar-dependent, needs external credentials).
- If a KSK rolls over, the status refreshes only on the next reconcile of the
  zone; the requeue interval bounds the staleness of `dsRecords`. Accepted —
  KSK rollovers are rare (365d default) and BIND retains the old key through
  the parent-propagation window.
