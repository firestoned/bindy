# Scout follow-ups: metrics, conflict detection, AAAA, bootstrap parity

> **Status:** 🔶 In progress: item 2 done 2026-10-23 (ADR-0027); items 1,
> 3, 4 and 5 open. Collects the items that survived roadmap
> [12](12-scout-ingress-controller.md)'s closure (its open-questions list)
> plus small parity gaps, so nothing dangles inside a ✅ doc.

---

**Created:** 2026-09-28
**Author:** Erick Bourgeois

## Items

1. **Scout Prometheus metrics** (roadmap 12 Q4). Scout exposes no metrics
   today. Wanted: `scout_sources_watched`, `scout_arecords_{created,deleted}_total`,
   `scout_errors_total`, remote-client health — reusing the operator's
   registry conventions (`bindy_firestoned_io_` namespace).
2. ✅ **Done 2026-10-23 ([ADR-0027](../../docs/adr/0027-scout-name-conflicts-detected-not-overwritten.md)).**
   Two clusters' Scout records for one name are a conflict: the record that
   published first keeps the name, the other is `Ready=False`,
   `RRsetConflict`; zone replay and the finalizer re-push follow the same
   rule, the held-back record takes over when the holder leaves, and a rename
   with both names registered is not a conflict. Proven on kind
   (`make e2e-scout-guardrails`, G7 and G8). Original item:
   **Cross-cluster conflict detection** (roadmap 12 Q1). Two clusters
   creating an ARecord for the same host is last-write-wins today. Wanted:
   conflict surfaced as a status condition (likely bindy-side, keyed on the
   `source-cluster` label). Since roadmap [31](31-scout-source-guardrails.md)
   (ADR-0025), with each cluster's account registered and the policy
   installed, two clusters can only collide in a zone an administrator
   registered for both; this item covers that remaining case. Since
   roadmap [32](32-scout-one-record-per-dns-name.md) (ADR-0026), sources of one
   Scout that share a name write one record, so any second record for a name
   is a conflict.
3. **AAAARecord support** (roadmap 12 Q2). Same pattern as ARecord for IPv6.
   Touches the full record-CRD checklist in the `add-new-crd` skill only if
   a new source annotation is wanted; AAAARecord CRD itself already exists.
4. **`bindy bootstrap scout` parity for the endpoint mode** (ADR-0008
   consequence): template `BINDY_SCOUT_REMOTE_ENDPOINT` / `_TOKEN_FILE` /
   `_CA_FILE` and the credential volume into the generated Deployment.
5. **Live Linkerd multicluster verification** of the endpoint mode
   (roadmap 12 Phase 3's remaining box) — needs the two-cluster staging
   environment.
