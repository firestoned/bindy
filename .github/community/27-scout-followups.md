# Scout follow-ups: metrics, conflict detection, AAAA, bootstrap parity

> **Status:** ⛔ Not started. Collects the items that survived roadmap
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
2. **Cross-cluster conflict detection** (roadmap 12 Q1). Two clusters
   creating an ARecord for the same host is last-write-wins today. Wanted:
   conflict surfaced as a status condition (likely bindy-side, keyed on the
   `source-cluster` label).
3. **AAAARecord support** (roadmap 12 Q2). Same pattern as ARecord for IPv6.
   Touches the full record-CRD checklist in the `add-new-crd` skill only if
   a new source annotation is wanted; AAAARecord CRD itself already exists.
4. **`bindy bootstrap scout` parity for the endpoint mode** (ADR-0008
   consequence): template `BINDY_SCOUT_REMOTE_ENDPOINT` / `_TOKEN_FILE` /
   `_CA_FILE` and the credential volume into the generated Deployment.
5. **Live Linkerd multicluster verification** of the endpoint mode
   (roadmap 12 Phase 3's remaining box) — needs the two-cluster staging
   environment.
