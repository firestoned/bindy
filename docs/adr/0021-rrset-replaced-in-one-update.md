# 0021: An RRset is replaced in one DNS UPDATE

- **Status:** Accepted
- **Date:** 2026-10-13
- **Proposed:** 2026-10-13
- **Deciders:** Erick Bourgeois
- **Related:** Builds on [ADR-0015](0015-bounded-api-cost-of-dns-writes.md) (DNS writes and their failure semantics); companion to [ADR-0020](0020-record-kinds-declared-once.md)

## Context

The operator writes a record to BIND9 with RFC 2136 dynamic updates, signed
with the instance's TSIG key (the CALM relationship "DNS UPDATE (RFC 2136,
TSIG)"). Eight of the nine record writers (`bindy-bind9/src/bind9/records/`)
do the same steps by hand:

1. query the current RRset and compare it with the spec (`should_update_record`);
2. send one UPDATE that deletes the RRset;
3. send one UPDATE per value that appends it (A/AAAA send N + 1 messages).

Each writer repeats that code (the writers and their shared helpers are
1,581 non-test lines), and the
split into several messages has three costs:

- **Not atomic.** Between the delete and the last append the name has no data,
  or only part of it. A resolver asking in that window gets NXDOMAIN or a
  partial answer; a failure in the middle leaves the RRset partly rebuilt
  until the next reconcile (PR 1 moved parsing ahead of the delete, but a
  rejected append still strands the RRset).
- **A compare by text.** Most writers compare `existing.to_string() == target`.
  A CNAME, NS, PTR, MX or SRV target that differs only in case or a trailing
  dot never compares equal, so the record is deleted and recreated on every
  reconcile, and each recreation opens the window above.
- **Inconsistent validation.** Some writers reject out-of-range values (SRV),
  others coerce them (MX priority), and each validates at a different step.

hickory-proto 0.26.3 exposes the UPDATE message builder (`Message`,
`add_zone`, `add_update`), and the TSIG-signed client sends any message. RFC 2136
section 3.4 processes a message's update section in order, and section 3.7
requires the server to apply it atomically: other clients see all of it or
none of it.

## Decision

1. **One writer.** `write_rrset(zone, name, record_type, ttl, desired:
   Vec<RData>, server, key)` replaces an RRset with exactly `desired` (the
   message built by `replace_rrset_message`, sent by `send_rrset_update`), in
   **one** UPDATE message: the zone section, then a delete of the RRset (class ANY, RFC 2136
   2.5.2), then one add per value (2.5.1). BIND9 applies it atomically: a
   query sees the old RRset or the new one, never an empty or partial one.
2. **Validate before the network.** Each kind converts its spec to `RData`
   (ADR-0020) and rejects an invalid value then, before any query or update,
   with the same rule for every kind.
3. **Compare by value.** The "is an update needed" check compares the current
   RRset with `desired` as values: names are normalised (case, trailing dot)
   before comparing, the TTL is compared, and order does not matter. An RRset
   that already matches sends nothing.
4. **The same failure semantics as today** (ADR-0015): a rejected UPDATE
   (any response other than `NOERROR`) fails the write and the reconcile
   retries; an empty `desired` set deletes the RRset; TSIG keys, servers and
   the coverage rules are unchanged.

## Consequences

- No window in which a replaced name answers NXDOMAIN or a partial RRset;
  the eight writers collapse into one plus a conversion per kind.
- Records whose targets differ from BIND9's form only in case or trailing dot
  stop being rewritten on every reconcile (fewer UPDATEs, quieter logs).
- The UPDATE message is built from hickory's public builder, not a hand-rolled
  encoder (the reason hickory was chosen over `domain`, cerebrum decision
  2026-05-02); a hickory upgrade that changes the builder is caught by the
  writer's tests.
- A message carrying many values is larger than one append, and hickory's UDP
  client has no TCP fallback. The writer measures the encoded UPDATE (plus a
  reserve for the TSIG record) and sends it over TCP when it would exceed the
  EDNS payload (`update_transport`); smaller UPDATEs stay on UDP. Both are
  TSIG-signed, and the pod network policy already allows TCP and UDP on the
  DNS port.
- No CRD, CALM or RBAC change: the relationship is the same RFC 2136 UPDATE
  with TSIG, now one message per RRset instead of N + 1. The threat model
  records the narrower tampering window (T1).
