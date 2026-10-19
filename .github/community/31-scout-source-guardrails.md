# 31: Scout source guardrails (records bound to their cluster and zones)

> **Goal.** An edge cluster's Scout can publish only into the zones, and
> only with the cluster names and addresses, that it was set up for, even
> when a tenant writes any annotation or the Scout's token is stolen.
>
> **Stop condition.** Every phase below is ✅, or recorded as a follow-up
> with a reason. Decided in
> [ADR-0025](../../docs/adr/0025-scout-records-bound-to-cluster-and-zones.md).

**Status:** 🔶 In progress (phases 1 to 4 implemented; phase 5 open).
**Created:** 2026-10-19
**Owner:** Erick Bourgeois

## Why

Before ADR-0025 every check on what a Scout publishes ran inside the Scout.
The zone grant (`bindy.firestoned.io/allow-zone-namespaces`) names bare
namespaces, which are not unique across clusters; the `source-cluster` label
is self-declared; the bootstrap token can write any `ARecord` in its
namespace; and a `DNSZone` selector that omits the zone label takes every
Scout record. A tenant on cluster `foo`, or a compromised `foo` Scout, could
therefore publish into `bar.example.com`, and where both publish one name the
last writer wins.

## Phases

| Phase | Scope | Status |
|---|---|---|
| 1 | Scout `--allowed-zones` / `BINDY_SCOUT_ALLOWED_ZONES` (zone and subzones; `*` and non-DNS entries refused; `--default-zone` must be allowed) and `--allowed-ip-ranges` / `BINDY_SCOUT_ALLOWED_IP_RANGES` (IPv4 CIDRs; default IPs must be inside); `bindy bootstrap scout` templates both | ✅ `crates/bindy-scout/src/guardrails.rs`, `pipeline.rs` (`publish`), `scout.rs` (`ScoutConfig::from_lookup`), `crates/bindy-bootstrap/src/bootstrap.rs` (`build_scout_deployment`) |
| 2 | Registry ConfigMap `bindy-scout-sources` (`<sa>.clusters`, `<sa>.zones`), written by `bindy bootstrap mc --cluster-name --allowed-zones` under a per-account field manager, removed by `--revoke` | ✅ `bootstrap.rs` (`build_scout_source_registry`, `scout_source_field_manager`, `unregister_scout_source`) |
| 3 | Admission policy `bindy-scout-source-binding` (`deploy/admission-policies/21-*`, `22-*`): registered accounts' ARecord create, update and delete bound to their clusters and zones, old and new object; fixtures in `tests/` with real objects in `tests/setup/` | ✅ Verified on kind (Kubernetes 1.36) 2026-10-19: type checking clean, every fixture as expected, a refused delete answers HTTP 422, server-side apply denied the same way, unregistered accounts and a missing registry unaffected |
| 4 | DNSZone takes a Scout record only for the zone in its label; Scout skips a refused delete of another cluster's record (403 or 422) instead of retrying | ✅ `crates/bindy-controller-zone/src/dnszone/discovery.rs` (`scout_record_belongs_to_zone`), `crates/bindy-scout/src/pipeline.rs` (`delete_other_cluster_arecord`), `crates/bindy-controller-sdk/src/kube_errors.rs` (`is_refused`) |
| 5 | Live verification: two scouts on separate edge clusters against one bindy cluster, one registered for `foo`, one for `bar`, through `tests/e2e/scout_test.sh` (needs a Scout image built from this branch) | ⛔ Not started |

## Follow-ups

- **Duplicate names in a shared zone** stay last-writer-wins; tracked as
  roadmap [27](27-scout-followups.md) item 2. ADR-0025 makes sharing a zone
  an administrator's registration choice.
- **Address checks in admission.** Left out so the policy does not depend on
  CEL's IP and CIDR library; the address check runs in Scout, where the
  address is chosen. Revisit when the supported Kubernetes floor guarantees
  that library.
- **Registry tamper protection.** Anyone who can write ConfigMaps in the
  records namespace can edit the registry. Scout's Role cannot. A policy in
  the style of `17-bindy-configmap-integrity` could restrict it to the
  platform team.
