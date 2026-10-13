# 0020: Record kinds are declared once

- **Status:** Accepted
- **Date:** 2026-10-13
- **Proposed:** 2026-10-13
- **Deciders:** Erick Bourgeois
- **Related:** Builds on [ADR-0009](0009-workspace-crate-split-and-shared-watch-layer.md) (the crate split and the shared `RecordKind` registry); companion to [ADR-0021](0021-rrset-replaced-in-one-update.md) (how a record kind's data reaches BIND9)

## Context

bindy serves nine DNS record kinds (`ARecord`, `AAAARecord`, `CNAMERecord`,
`TXTRecord`, `MXRecord`, `NSRecord`, `SRVRecord`, `CAARecord`, `PTRRecord`).
Each is a CRD and a set of facts the operator needs about it: its kind string,
its finalizer, its hickory `RecordType`, its spec's record name and TTL, its
status, and how its spec becomes DNS data.

Those facts are written out by hand, once per kind, in four crates:

- `bindy-api`: the `DNSRecordKind` enum and its `as_str` / `all` /
  `to_hickory_record_type` / `TryFrom` matches, the `FINALIZER_*` constants,
  and the lists in the `crdgen` and `crddoc` binaries;
- `bindy-controller-sdk`: the `record_kind!` line and the `RECORD_KINDS`
  registry entry (the `RecordKind` trait);
- `bindy-bind9`: a `*RecordOp` struct that copies spec fields, its
  `RecordOperation` impl, the kind's `ReconcilableRecord` impl (seven
  methods), and an arm of the replay dispatch;
- `bindy-controller-records`: a `DnsRecordType` impl (`record_impls.rs`,
  174 lines for nine kinds) and a line in `controller()`;
- plus the zone controller's per-kind watch and discovery lines and
  bootstrap's CRD list.

Three traits (`RecordKind`, `ReconcilableRecord`, `DnsRecordType`) describe the
same nine types with overlapping methods: `KIND` / `record_type_name()` /
`RECORD_TYPE_STR`, `record_status()` / `get_status()` / `status()`,
`record_type_hickory()` / `hickory_record_type()`. The cerebrum checklist for
adding `PTRRecord` (issue #475) listed about 20 edits across those files, and
the copies have already disagreed: the rejection-cooldown key was built from
`record_type_name()` in one place and `KIND` in another, matching only because
every kind's name happens to be "X" + "Record".

## Decision

1. **One table.** A single `for_each_record_kind!` declaration
   (`crates/bindy-api/src/record_kinds.rs`) lists each kind once, with the
   facts that differ between kinds: the variant (the hickory `RecordType`
   name), the CRD type, the kind string and the finalizer:

   ```text
   (A, ARecord, "ARecord", "bindy.firestoned.io/arecord-finalizer"),
   (AAAA, AAAARecord, "AAAARecord", "bindy.firestoned.io/aaaarecord-finalizer"),
   ...
   ```

   It generates the `DNSRecordKind` enum and its methods (including
   `finalizer()`, which replaces the `FINALIZER_*` constants), the `RecordKind`
   impls and registry, the replay dispatch, and the lists the binaries and
   controllers iterate. Three helpers built on it (`each_record_kind!`,
   `match_record_kind!`, `record_kind_array!`) replace every per-kind list or
   match elsewhere.
2. **One trait for what a kind is.** `RecordKind` carries the kind
   (`DNS_KIND`, from which the kind string, finalizer and hickory type
   follow), the spec name and TTL, and the status accessor.
   `DnsRecordType` and the `*RecordOp` structs are deleted; `ReconcilableRecord`
   keeps only what is genuinely per kind.
3. **Only the data conversion stays hand-written.** Each kind implements one
   function from its spec to the DNS data it publishes (the `RData` set, see
   ADR-0021) and, where it has one, its display form for status. Nothing else
   about a kind is written outside the table.
4. **Behaviour does not change.** Kind strings, finalizer names, CRD schemas,
   status shapes and the DNS data written are identical; tests pin each kind's
   facts before the table replaces the hand-written copies. (The generated API
   reference lists the kinds in the table's order.)

## Consequences

- Adding a record kind becomes: its CRD struct in `crd.rs`, one table row, its
  data conversion, its tests, and the non-code checklist (RBAC, admission
  policies, examples, docs). The `add-new-crd` skill and the cerebrum checklist
  shrink accordingly.
- The table is a macro: a reader follows one declaration to find every
  generated item, and the macro's expansion is covered by the per-kind tests
  rather than read line by line. This is the accepted cost.
- Rules out adding a kind by hand-writing impls outside the table; a kind
  missing from the table is missing everywhere, which the per-kind tests catch.
- No CRD, CALM or RBAC change.
