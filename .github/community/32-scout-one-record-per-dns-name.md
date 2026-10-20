# 32: Scout publishes one record per DNS name (issue #556)

> **Goal.** Several sources of one Scout that resolve to the same zone and
> DNS name publish one `ARecord` with the complete address set, written as
> a complete, conditional replace, instead of one record each that
> overwrite one another.
>
> **Stop condition.** Every phase below is ✅, or recorded as a follow-up
> with a reason. Decided in
> [ADR-0026](../../docs/adr/0026-scout-one-record-per-dns-name.md).

**Status:** ✅ Done (2026-10-20): every phase passed, including the two-cluster kind proof on slate.
**Created:** 2026-10-20
**Owner:** Erick Bourgeois

## Why

Scout writes one `ARecord` per source host, and the records controller
treats each record as the owner of its RRset (ADR-0021). Three
`LoadBalancer` Services published at one name (issue #556: kube-API VIPs,
one Service per availability zone, all at `@`) produce three records that
replace each other's RRset, so BIND9 serves whichever reconciled last.

## Phases

| Phase | Scope | Status |
|---|---|---|
| 1 | Every IPv4 address of a load balancer status is published, not only the first (Service, Ingress, `--gateway-service` lookup) | ✅ `crates/bindy-scout/src/scout.rs` (`resolve_ips_from_service_lb_status`, `resolve_ips_from_lb_status`), `pipeline.rs` (`resolve_ips_from_gateway_service`) |
| 2 | ADR-0026 accepted; CALM multi-cluster model updated (`arecords`, `scout-a`, `scout-*-applies`) | ✅ `docs/adr/0026-scout-one-record-per-dns-name.md`, `calm/bindy-multi-cluster.architecture.json`, `docs/src/architecture/calm-multi-cluster.md` |
| 3 | Per-name record naming (cluster, zone, DNS name); contributor annotation and per-contributor labels; address set and lowest TTL derived from the map (Decisions 1, 2) | ✅ `crates/bindy-scout/src/scout.rs` (`dns_name_record_cr_name`, `name_identity_hash`), `crates/bindy-scout/src/contributors.rs` (`Contribution`, `render`, `derived_ips`, `derived_ttl`, `contributor_label`) |
| 4 | Conditional read-modify-write with `resourceVersion` precondition and retry on 409; leaving a name removes only the source's own entry; conditional delete when the map is empty (Decisions 3, 4) | ✅ `contributors.rs` (`plan_contribute`, `plan_withdraw`, `contribute`, `withdraw`; at most 5 attempts), `pipeline.rs` (`publish`, `withdraw_from_own_records`, `delete_source_records`); fake API server in `pipeline_tests.rs` now enforces preconditions and can lose a race |
| 5 | Handover from per-source records to per-name records, deleting the old record once the new one is served (Decision 5) | ✅ `pipeline.rs` (`publish`: per-source, legacy and previous-cluster records wait on `handover_step`; previous-cluster per-name records lose only this source's entry, `withdraw_from_other_cluster_record`, refusals skipped) |
| 6 | Docs (`docs/src/guide/scout.md` record names, labels, troubleshooting; migration guide incl. rollback), CHANGELOG, kind proof with three Services at `@` and source churn | ✅ Docs, examples, CHANGELOG, `tests/e2e/scout_test.sh` updated; new two-cluster suite `tests/e2e/scout_multi_cluster_test.sh` (`make e2e-scout-multi`) passed on slate 2026-10-20: #556 reproduced on the old Scout, upgrade with 0 empty answers in 160 probe samples per name, churn, mixed kinds/namespaces/TTLs, a 10-way race, Scout/operator/BIND9 kills, quiet operator and Scout. `e2e-scout` and `e2e-chaos` also pass |
| 7 | Full threat-model pass; header stamp bumped | ✅ `docs/src/security/threat-model.md` v1.29: T6, M-73, accepted risk 24 |

## Follow-ups

- Two owners of one name (two Scouts registered for one zone, or a
  hand-written `ARecord` next to a Scout one) stay last-writer-wins. With
  this roadmap, any second record for a name is a conflict; detecting it is
  roadmap 27 item 2 and needs its own ADR, which must not flag the overlap
  during a phase 5 handover.
