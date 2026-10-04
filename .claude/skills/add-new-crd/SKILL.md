---
name: add-new-crd
description: Full procedure for adding a new Custom Resource Definition to the operator — Rust struct, crdgen registration, regeneration, examples, docs, quality gate. A new DNS record CRD touches ~20 src files plus deploy/, examples/, docs/ (see the checklist inside).
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

2. Register it in `crates/bindy-api/src/bin/crdgen.rs` (and `crates/bindy-api/src/bin/crddoc.rs`):

   ```rust
   generate_crd::<MyNewResource>("mynewresources.crd.yaml", output_dir)?;
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

## Checklist for a new DNS record CRD (mirrors SRVRecord; ~20 src files)

Learned from PTRRecord (#475):

- `crates/bindy-api/src/crd.rs`: `DNSRecordKind` enum + `as_str`/`all`/`to_hickory_record_type`/`TryFrom` + `UnknownDNSRecordKind` error msg + spec struct
- `crates/bindy-controller-sdk/src/context.rs`: a `RecordRef` variant (plus its `name`/`namespace`/`record_type` arms), a `record_kind!(Type, "Kind", Variant)` line, and an `ops::<Type>()` entry in `RECORD_KINDS` (that one list drives the WatchSet registration, the store and selector matching)
- `crates/bindy/src/record_impls.rs`, `crates/bindy-api/src/constants.rs` (`KIND_*`), `crates/bindy-api/src/labels.rs` (`FINALIZER_*`)
- `crates/bindy/src/bind9/types.rs` (`*RecordData`), `crates/bindy/src/bind9/mod.rs` (manager method), `crates/bindy/src/bind9/records/{mod.rs,<type>.rs,<type>_tests.rs}`
- `crates/bindy/src/reconcilers/records/{mod.rs,types.rs}`, `crates/bindy/src/reconcilers/mod.rs`, `crates/bindy/src/reconcilers/dnszone/{discovery.rs,cleanup.rs}`
- `crates/bindy/src/main.rs`: select arm + DNSZone `.watches_stream` (registration and stores come from `RECORD_KINDS`)
- `crates/bindy/src/bootstrap.rs` (`build_all_crds`), `crates/bindy-api/src/bin/{crdgen,crddoc}.rs`
- Non-src: `deploy/operator/rbac/{role,role-admin}.yaml`, admission policies 03+13, `deploy/kind-test.sh`, `examples/`, `docs/src/**` ("all N record types" counts!), `README.md`, `calm/bindy-control-plane.architecture.json`, `docs/mkdocs.yml` nav

**Gotchas:** grep tests for the new kind string BEFORE implementing — tests
use unknown kinds as placeholders and hardcode CRD counts
(`bootstrap_tests.rs::test_build_all_crds_returns_twelve` — bump it).

## Verification

`kubectl apply --dry-run=client -f deploy/operator/crds/<plural>.crd.yaml`
succeeds; API docs include the new resource; `cargo-quality` green.
