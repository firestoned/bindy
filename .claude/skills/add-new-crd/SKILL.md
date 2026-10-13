---
name: add-new-crd
description: Full procedure for adding a new Custom Resource Definition to the operator: Rust struct, crdgen registration, regeneration, examples, docs, quality gate. A new DNS record CRD is its spec struct, one record-kind table row and its BIND9 data conversion, plus deploy/, examples/, docs/ (see the checklist inside).
---

# add-new-crd

## Core steps

1. Add the new `CustomResource` struct to `crates/bindy-api/src/crd.rs`:

   ```rust
   #[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
   #[kube(
       group = "bindy.firestoned.io",
       version = "v1beta1",
       kind = "MyNewResource",
       namespaced
   )]
   #[serde(rename_all = "camelCase")]
   pub struct MyNewResourceSpec {
       pub field_name: String,
   }
   ```

2. Register it in `crates/bindy-api/src/bin/crdgen.rs` (and `crates/bindy-api/src/bin/crddoc.rs`,
   and bootstrap's `build_all_crds`). A DNS record kind needs none of this: its
   table row registers it (see the checklist below).

   ```rust
   generate_crd::<MyNewResource>(output_dir)?; // writes <plural>.crd.yaml
   ```

3. Run the `regen-crds` skill.
4. Add examples to `examples/`.
5. Run the `validate-examples` skill.
6. Add documentation in `docs/src/`.
7. Run the `regen-api-docs` skill (LAST).
8. Run the `cargo-quality` skill.
9. Run the `update-changelog` skill.

Per ADD (`rules/architecture-driven-development.md`), a new CRD is
architecturally significant: ADR first, CALM model update
(`make calm-validate` + `make calm-docs`), threat-model pass after.

## Checklist for a new DNS record CRD (ADR-0020: the kinds are declared once)

Every per-kind list is generated from one table, so a record kind is added in
three source places:

1. `crates/bindy-api/src/crd.rs`: the spec struct (`#[kube(kind = "XRecord",
   status = "RecordStatus")]`, with `name: String` and `ttl: Option<i32>` like
   every other record spec).
2. `crates/bindy-api/src/record_kinds.rs`: one row in `for_each_record_kind!`,
   `(X, $crate::crd::XRecord, "XRecord", "bindy.firestoned.io/xrecord-finalizer")`.
   The variant name must be the hickory `RecordType` variant. The row
   generates `DNSRecordKind` and its conversions, the SDK `RecordKind` impl
   and `RECORD_KINDS` entry (watch, store, readiness wait), the record
   controller, the zone controller's watch, discovery and stale cleanup,
   the BIND9 replay dispatch, `crdgen`, `crddoc` and bootstrap's
   `build_all_crds`.
3. `crates/bindy-bind9/src/record_push.rs`: the kind's `ReconcilableRecord`
   impl, its one hand-written part: `add_to_bind9` (call the matching
   `Bind9Manager::add_*_record`) and, for address kinds, `display_addresses`.
   Without it nothing that iterates the table compiles. If BIND9 has no writer
   for the type yet, add it in `crates/bindy-bind9/src/bind9/` (`types.rs`,
   `mod.rs` manager method, `records/<type>.rs` + `_tests.rs`).

Tests: add the kind to the per-kind pins (`bindy-api` `record_kinds_tests.rs`,
`bindy-controller-sdk` `context_tests.rs::record_kind_pins`, `bindy-bind9`
`record_push_tests.rs::record_kind_pins`).

Non-src: `deploy/operator/rbac/{role,role-admin}.yaml`, admission policies 03+13, `deploy/kind-test.sh`, `examples/`, `docs/src/**` ("all N record types" counts!), `README.md`, `calm/bindy-control-plane.architecture.json`, `docs/mkdocs.yml` nav

**Gotchas:** grep tests for the new kind string BEFORE implementing: tests
use unknown kinds as placeholders and hardcode CRD counts
(`crates/bindy-bootstrap/src/bootstrap_tests.rs::test_build_all_crds_returns_thirteen`, bump it).

## Verification

`kubectl apply --dry-run=client -f deploy/operator/crds/<plural>.crd.yaml`
succeeds; API docs include the new resource; `cargo-quality` green.
