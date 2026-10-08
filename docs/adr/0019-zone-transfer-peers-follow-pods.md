# 0019: Zone transfer peers follow the zone's pods

- **Status:** Accepted
- **Date:** 2026-10-07
- **Proposed:** 2026-10-07
- **Deciders:** Erick Bourgeois
- **Related:** Amends [ADR-0017](0017-zones-loaded-readiness-gate.md) (the gate loads a new secondary only after the primaries allow it, and waits for its transfer while another pod serves the zone), builds on [ADR-0016](0016-event-driven-reconciliation.md) (every wake is a watch event or a backoff retry) and [ADR-0015](0015-bounded-api-cost-of-dns-writes.md) (stores instead of per-reconcile LISTs)

## Context

A zone served by primaries and secondaries carries two lists that name pods
by IP:

- on every primary, `allow-transfer` (the secondaries allowed to AXFR the
  zone) and `also-notify` (where the primary sends NOTIFY when the serial
  moves);
- on every secondary, `primaries` (where it transfers the zone from).

bindy wrote both lists only when it **created** the zone on a pod. Once a
zone existed on a pod, every later reconcile saw `zone_exists` and skipped
it. So the lists froze at the IPs of the moment the zone was created.

Found on a real cluster with v0.8.0-rc.7 (one `Bind9Cluster`, two primaries
and one secondary, one replica each, zones on `emptyDir`): after a few
rollouts and pod deletions the secondary held **no copy** of the zone.

- The primaries still had `allow-transfer { <old secondary IP>/32; }` and
  `also-notify { <old secondary IP> port 5353; }`, so the current
  secondary's AXFR was `denied`.
- The secondary had `primaries { <two stale primary IPs>; <two current
  primary IPs>; }`. The peer lists came from a pod LIST filtered on
  `phase == Running`, which includes terminating pods: a secondary zone
  created during a rollout got the old and the new primaries, and the old
  ones then failed `host unreachable` forever.
- The zone reported `Ready=True` "configured on 2 primary and 1 secondary
  instance(s)" while the secondary served nothing.
- bindcar answers `GET /api/v1/zones/{zone}/status` for a configured but
  unloaded zone with HTTP 500 (its log reads `RNDC command 'zonestatus'
  failed: zone not loaded`; the response body is masked to a generic
  message). The operator treated that 500 as retryable and spent two minutes
  per reconcile retrying it, then tried a `POST` and got 409.

Nothing re-triggered the existing update path: `add_primary_zone` treats a
409 as success and PATCHes `allow-transfer` / `also-notify`, but it is only
reached when the existence check fails. And that PATCH could not have
worked: bindcar 0.9.0's `PATCH /api/v1/zones/{zone}` parses `alsoNotify`
entries as bare IP addresses (so the port-qualified `<ip>:5353` form bindy
sends is a 400), re-reads the zone with `rndc showzone`, keeps an
`also-notify { <ip> port 5353; }` it cannot parse as an opaque option, and
has no field for a secondary's `primaries` at all.

The ADR-0017 gate made the window worse in one respect: it loads a new
secondary pod and admits it without waiting for the transfer, but the
primaries' `allow-transfer` does not name the new pod yet, so the transfer
is denied and the pod goes Ready with nothing to serve.

## Decision

1. **The peer sets are computed from the Pod store, on every zone
   reconcile.** `bindy_bind9::peers::desired_transfer_peers`:
   - `primaries` (a secondary's transfer sources): the IPs of the zone's
     primary instances' pods that are not terminating, have an IP, are
     `Running`, and are admitted by the zones-loaded gate (or carry no gate).
     A primary still loading its zones is not a transfer source: a forced
     retransfer from it would copy a partial zone.
   - `secondaries` (the primaries' `allow-transfer`): the IPs of the zone's
     secondary instances' pods that are not terminating, have an IP and are
     `Running`, gated or not: a new secondary must be allowed to transfer
     before it can load.
   - `notify` (the primaries' `also-notify`): the `ClusterIP` of each
     secondary instance's Service, bare (BIND's default port 53, which the
     Service maps to `named`'s 5353).
   Each list is sorted and de-duplicated. The two pod LISTs per reconcile
   (`find_primary_ips_from_instances`, `find_secondary_pod_ips_from_instances`)
   are replaced by store reads; the Service ClusterIPs cost one GET per
   secondary instance per reconcile.

2. **The last peer sets pushed are recorded in `status.transferPeers`**
   (`primaries`, `secondaries`, `notify`). A reconcile compares the desired
   sets with the recorded ones:
   - `secondaries` or `notify` changed: every primary endpoint that has the
     zone is PATCHed with `allowTransfer` (bare IPs) and `alsoNotify` (the
     ClusterIPs). A zone created before this ADR keeps an `also-notify` that
     bindcar 0.9.0 cannot rewrite; for it the PATCH is retried with
     `allowTransfer` only, which is enough for transfers, and its
     `also-notify` is corrected when its pod is next replaced.
   - `primaries` changed: every secondary endpoint that has the zone has it
     **replaced**: `DELETE`, then `POST` with exactly the desired primaries,
     then `retransfer`. bindcar 0.9.0 cannot change a secondary's
     `primaries` in place; a secondary holds no data that is not on the
     primaries, so a replace loses nothing.
   - Nothing changed: no extra bindcar call; a reconcile with unchanged peers
     is a no-op for peer configuration.
   The new sets are recorded only when every endpoint accepted them; a
   failure leaves the old sets in status, sets `Degraded`, and the backoff
   retry pushes again. A zone with no admitted primary leaves its
   secondaries untouched until one is admitted (its `Endpoints` event wakes
   the zone).
   Primaries are refreshed before secondaries, so a secondary is never
   pointed at transfer sources that do not allow it yet.

3. **The gate refreshes the primaries before it loads a secondary**
   (amends ADR-0017 decision 2). For a new secondary pod, the gate PATCHes
   every primary endpoint's `allow-transfer` and `also-notify` to the
   desired sets (which include the new pod) before creating the zone on it.
   After the zone is created and `retransfer` issued, the gate asks the pod
   for the zone's status: a zone not loaded yet is a load failure. As for
   any failed load, it blocks the gate only while another Ready pod of the
   instance serves the zone (a rollout with `maxSurge` 1: the old pod keeps
   serving until the new one has transferred), and is retried with the
   per-object backoff; with no such pod the new pod is admitted and the zone
   controller finishes the job.

4. **Status is truthful about secondaries.** After configuring the
   secondaries, a zone that is configured on a secondary endpoint but not
   loaded there makes that instance `Failed` in `status.bind9Instances` and
   the zone `Degraded` / `Ready=False` with reason `SecondaryNotLoaded` and a
   message naming the instance and endpoint. The backoff retries it (the
   transfer completes inside BIND9, which raises no Kubernetes event).

5. **"Zone not loaded" is a state, not a server fault.** bindcar 0.9.0 maps
   every rndc failure of `zonestatus` other than "not found" to HTTP 500, and
   masks every 5xx body to `{"error":"Internal server error"}`: the rndc text
   `zone not loaded` reaches only bindcar's own log, so it cannot be matched
   in the response. Instead (`zone_ops::zone_presence`):
   - the status check retries a 5xx only for `ZONE_STATUS_RETRY_BUDGET`
     (2 s), not the two minutes of the generic bindcar retry;
   - on a 5xx, the zone's SOA is asked of `named` on the same pod (its DNS
     port). `SERVFAIL` there means the zone is configured but has no data
     (`ZonePresence::NotLoaded`); an authoritative answer means it is loaded
     (bindcar failed for another reason); anything else (or no answer) keeps
     the 500 an error, retried by the controller's backoff.
   - A zone whose status check fails with a 5xx is still deletable: the
     DELETE decides (the zone finalizer and the decision 2 replace depend on
     it).
   - An endpoint whose zone state cannot be read at all (a pod that is
     gone, a sidecar restarting) is a failed endpoint of this reconcile,
     retried with the zone's backoff. It is no longer sent a `POST` that
     rides the two-minute bindcar retry against a dead address, which held
     the zone's reconcile (and with it a deletion finalizer and the tagging
     of new records) for minutes; found by the chaos e2e suite.
   Follow-ups for bindcar, none of which this ADR waits for: report an
   unloaded zone as a typed state rather than a masked 500; accept
   `primaries` in the zone PATCH; parse and render port-qualified
   `also-notify` entries.

## Consequences

- After a pod is replaced, the zone's peer lists follow within one zone
  reconcile, woken by the instance's `Endpoints` change. Stale IPs are
  dropped, not accumulated.
- **Security.** A stale `allow-transfer` entry is an IP the cluster may hand
  to an unrelated pod, which could then AXFR the zone. `allow-transfer` now
  names exactly the zone's current secondary pods.
- **A secondary briefly drops a zone when its primaries change.** Replacing
  a secondary zone leaves it configured but unloaded until the AXFR
  completes (milliseconds in-cluster for a small zone). A rolling update of
  one primary changes the primaries set twice (the new pod admitted, the old
  one terminating), so each secondary replaces each zone twice per primary
  rollout. The primaries keep serving throughout. With ADR-0018 only one
  instance of a cluster rolls at a time.
- **Upgrade.** A zone reconciled for the first time by this version has no
  `status.transferPeers`, so every primary is PATCHed once and every
  secondary zone is replaced once. That is also what repairs clusters that
  hit the rc.7 bug.
- **NOTIFY reaches one replica of a multi-replica secondary instance** per
  message, through the Service. The other replicas refresh on the zone's
  SOA `refresh` timer and on the zone reconcile's `retransfer`. A Service
  without a ClusterIP (headless) gets no NOTIFY; its secondaries rely on the
  same refresh paths. NOTIFY through the ClusterIP keeps the primary pod's
  source address only where kube-proxy does not masquerade pod-to-Service
  traffic (the default); with `masqueradeAll` the secondary refuses it.
- **Zones created before this ADR** keep a port-qualified `also-notify`
  that bindcar 0.9.0 cannot rewrite in place; transfers still work (their
  `allow-transfer` is rewritten) and NOTIFY to a replaced secondary is lost
  until the primary pod is next replaced. Rolling the BIND9 pods once after
  the upgrade moves every zone to the new form.
- **CRD.** `DNSZone.status.transferPeers` is new (status only, additive).
- **CALM.** The relationship between the operator and the bindcar sidecars
  is unchanged; the zone-transfer relationship between primaries and
  secondaries now targets the secondary Service for NOTIFY.
- **Threat model.** The allow-transfer exactness above, and the operator
  deleting and re-creating secondary zones (availability), are recorded.
