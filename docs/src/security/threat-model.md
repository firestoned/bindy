# Threat Model - Bindy DNS Operator

**Version:** 1.28
**Last Updated:** 2026-10-19
**Owner:** Security Team
**Compliance:** SOX 404, PCI-DSS 6.4.1, Basel III Cyber Risk

> Last full pass 2026-10-19, against ADR-0001 ... ADR-0025 (ADR-0006 as amended;
> ADR-0009 as amended 2026-10-05, fully implemented; ADR-0013 stages 1 to 3,
> ADR-0014, ADR-0015, ADR-0016, ADR-0017 as amended and corrected 2026-10-07
> and by ADR-0019, ADR-0018 as amended 2026-10-07, ADR-0019, ADR-0020,
> ADR-0021, ADR-0022, ADR-0023, ADR-0024 as amended and ADR-0025 decisions 1
> to 6 implemented).
>
> **Revision note (v1.28):** Full pass for ADR-0025 (Scout records bound to
> their cluster and zones). New threats **S4** (a Scout claims another
> cluster's name) and **T5** (a tenant or a compromised Scout on one edge
> cluster publishes into, rewrites or deletes another cluster's records). New
> controls: Scout's `--allowed-zones` and `--allowed-ip-ranges` (**M-69**);
> the `bindy-scout-sources` registry and the `bindy-scout-source-binding`
> admission policy, which bind a registered ServiceAccount's ARecord writes,
> old and new object, to its cluster names and zones (**M-70**, verified on
> kind 1.36); a DNSZone takes a Scout record only for the zone in its label
> (**M-71**); Scout skips a refused delete of another cluster's record
> instead of retrying (**M-72**). Accepted risk **23** records what stays
> open: duplicate names in a zone registered for two clusters, accounts not
> yet registered, the registry's ConfigMap write surface, and no address
> check in admission. One new asset (the registry) and no new RBAC grant on
> any Scout role; `bindy bootstrap mc` additionally writes one ConfigMap with
> a per-account field manager. Sections changed: components (5), assets,
> Boundary 6, S4, T5, attack surface 7, Scenario 6, M-69 to M-72, accepted
> risk 23, controls summary. Corrected on the way: the assets table still
> gave the Scout token cluster-wide `secrets:get` and listed "all cluster
> Secrets (via Scout)" as exposed, both closed by M-25 on 2026-07-19; the
> controls summary counted 16 admission policies where there are 11 (10
> before this change). Reviewed with no change: Boundaries 1 to 5, S1 to S3,
> T1 to T4, R1, R2, I1 to I5, D1 to D6, E1 to E4, attack surfaces 1 to 6,
> Scenarios 1 to 5.
>
> **Revision note (v1.27):** Full pass for ADR-0024 (every Secret operation
> emits one structured audit event). bindy now records, on the tracing target
> `bindy::audit`, one event per Secret create, replace, patch, delete or read
> of key material, with operation, outcome, purpose, reason, actor and the
> object served, and never key material or an API error message (**M-67**,
> R2: Secret access attributable with *why*, joined to the API server audit
> log by namespace, name and time). All Secret access goes through one
> `SecretApi`, and a test fails the build when other code names an
> `Api<Secret>`. The same change narrows what the operator reads (**M-68**,
> I1): existence, rotation and mirror-staleness checks read metadata only,
> the ADR-0023 mirror records its source's `resourceVersion`, and the
> instance controller's Secret watch is a metadata watch, so the operator no
> longer receives the data of every Secret in the namespaces it watches. The
> planned M-12 (secret access audit trail, H-3) is done. Accepted risk
> **22** records that the events are only as durable as the log pipeline
> they are routed to, and that `bindy::audit=off` turns them off. Sections
> changed: components (1, 4, 5), Boundary 3, R1, R2, I1, attack surface 1,
> scenarios 1 and 4, existing and planned mitigations (M-12, M-67, M-68),
> high residual risk 2, accepted risk 22 and the controls summary. No new
> actor, trust boundary, RBAC grant or dependency (the events go to stdout
> like every log line; `get`, `list` and `watch` cover metadata reads).
> Other sections re-walked, unchanged.
>
> **Revision note (v1.26):** Full pass for ADR-0023 (a `rndcKey.secretRef`
> RNDC key is mirrored into an operator-owned Secret). The operator now reads
> the key from the referenced Secret (its `secretKey` entry, or its
> `rndc.key` file) and always names it `bindy-operator`, the only name the
> cluster-shared `named.conf` control channel accepts, and writes it to `{instance}-rndc-key`, which is the
> only RNDC Secret any BIND9 pod mounts; the referenced Secret is never
> mounted. A change to the source rewrites the mirror and rolls the pods
> (before, the operator switched keys while the pods kept the old one and
> every write failed until a manual restart). A missing source keeps the
> mirror, a failed GET no longer counts as "absent", and a key that is not
> standard base64 (and so could inject configuration into `rndc.key`) is
> refused; the source's key name is never written (**M-65**).
> An RNDC `secretRef` / `rndcSecretRef` must name a `bindy-` Secret, enforced
> at admission (`bindy-pod-shape-validation` rule 5) and by the reconciler
> (**M-66**, E2: the confused deputy through which anyone able to create a
> `Bind9Instance`, `Bind9Cluster` or `ClusterBind9Provider` had any Secret of
> the namespace mounted into the operand pod). Accepted risk **21** records
> the mirror as a second copy of user key material and the key-change rollout
> that bypasses the ADR-0018 queue. Sections changed: components (4), assets,
> Boundary 4, T3, I1, D6, E2, attack surfaces 3 and 5, M-58, M-65, M-66,
> accepted risk 21 and the controls summary. No new actor, trust boundary,
> RBAC grant or dependency: the operator already read Secrets cluster-wide
> and wrote `{instance}-rndc-key`; the Secret watch it already ran now also
> maps an unowned source Secret to the instances that reference it, from the
> instance and cluster stores. The reconciler's user-volume allow-list
> (`validate_user_volumes`, F-001) is now a true allow-list: a volume must set
> exactly one source and that one must be `emptyDir`, `secret`, `configMap`
> or `persistentVolumeClaim`. It used to return on the first allowed source,
> so an allowed source set together with one on no deny list (`image`) passed.
> The `bindy-pod-shape-validation` rule 1 still accepts a volume once an
> allowed source is present (CEL over a typed object cannot enumerate its
> fields), so for a combined source the reconciler is the control. Other
> sections re-walked, unchanged.
>
> **Revision note (v1.25):** Full pass for ADR-0022 (Scout's five source
> kinds share one reconcile pipeline). Accepted risk **20** is **closed**:
> every Scout `ARecord` carries a `bindy.firestoned.io/source-kind` label that
> every Scout selector includes, and its name carries a hash of the
> (cluster, kind, namespace, name) source, so same-named sources of different
> kinds no longer overwrite or prune each other's records and `team-a`/`web`
> no longer collides with `team`/`a-web` (**M-63**). The pipeline also rejects
> non-IPv4 status addresses, invalid TTL annotations and malformed startup
> configuration, and fails the reconcile on a Gateway or Service lookup error
> instead of hiding it. On the operator side, the record finalizer no longer
> deletes an RRset another live record in the zone still declares; it
> re-pushes the survivor instead, and a rename does the same for the old
> name (**M-64**, T1: a name shared by two records stayed off the air after
> one was deleted). The upgrade rename is gated: Scout deletes a legacy or
> previous-cluster record only once its replacement is bound to the zone and
> `Ready` (**M-63**). Sections changed: component 5, the T1 mitigations, M-59
> (evidence moved to `pipeline.rs`), M-63, M-64 and accepted risk 20. No new
> actor, asset, trust boundary, RBAC grant or dependency; other sections
> re-walked, unchanged.
>
> **Revision note (v1.24):** Full pass for ADR-0020 (record kinds declared
> once) and ADR-0021 (an RRset replaced in one DNS UPDATE). ADR-0020 changes
> no component, boundary, RBAC grant or data: the same nine kinds, finalizers
> and CRDs, now generated from one table. ADR-0021 narrows T1: a record write
> used to delete the RRset and append each value in its own message, so the
> name answered NXDOMAIN or a partial RRset between them, and a CNAME, NS,
> PTR, MX or SRV target differing only in case or trailing dot was rewritten
> on every reconcile, reopening that window. Each RRset is now replaced in one
> TSIG-signed UPDATE that BIND9 applies atomically (RFC 2136 section 3.7),
> values are validated before any network call, and the RRset is compared by
> value (**M-62**). An UPDATE too large for UDP goes over TCP on the DNS port
> the pod network policy already allows. No new actor, asset, trust boundary
> or dependency; other sections re-walked, unchanged.
>
> **Revision note (v1.23):** `bindy bootstrap mc` no longer prints the
> remote kubeconfig Secret, and with it the ServiceAccount token, to stdout:
> it writes it to a file readable only by its owner and refuses `-` (**M-61**). The
> I1 rows, Attack Surface 7 and Scenario 6 were re-walked; the Scout RBAC and
> the Secret's contents are unchanged. The dead-code removal and the
> `serde_yaml` to `serde-saphyr` move of the same day change no component,
> boundary or control (the generated CRDs are semantically identical).
>
> **Revision note (v1.22):** Full pass after a code review of every crate for
> paths that proceeded on failure. No new ADR, component, actor, asset, trust
> boundary, RBAC grant or dependency; the fixes tighten existing controls.
> (1) **P2-7 gap on TCPRoute.** Scout validated a tenant's
> `bindy.firestoned.io/record-name` override for Ingress, Service, HTTPRoute
> and TLSRoute but not TCPRoute, whose only record name is that override
> (Attack Surface 7; **M-59**). (2) **Errors treated as success.** A record
> finalizer let go when a lookup failed, the instance finalizer when a delete
> was refused, a primary or secondary dropped out of the target set when its
> role could not be read, pods rolled to an instance-only spec when the parent
> could not be read, a truncated LIST was used as complete, a failed lookup
> counted as zero expected instances (a possible false `Ready=True`), and
> controllers could reconcile against caches that had not listed yet (the
> zones-loaded gate could replay an empty record list onto a new pod): all now
> stop and retry (**M-56**, new accepted risk **19**). (3) **Record writes.**
> An invalid A/AAAA entry used to wipe the live RRset before failing, and a
> refused RRset delete was ignored (**M-57**). (4) **RNDC key.** The operator
> read `{instance}-rndc-key` even for an instance on `rndcKey.secretRef`; it
> now reads the Secret the pods mount (**M-58**). (5) **bindcar transport.**
> The TLS CA bundle could be read from `default` instead of the instance's
> namespace (S: a bundle planted there), and a failed client build fell back
> to a client without timeouts (**M-60**). (6) **Stale Scout records** for
> removed hosts, Services that left `LoadBalancer`, and old cluster names
> (**M-59**); new accepted risk **20** records the cross-kind name collision
> left in Scout. Other sections re-walked, unchanged.
>
> **Revision note (v1.21):** Amendments to ADR-0016 (a zone waiting only on
> a secondary's zone transfer is rechecked every 3 s, at most 20 times, then on
> the backoff) and ADR-0017 (readiness probes start after 2 s and run every
> 2 s). Both shorten recovery (D6: a replaced pod is back in its Service about
> 10 s sooner; a failing pod leaves it after 6 s instead of 15 s; status
> follows a recovered secondary within seconds) and are bounded (D2: at most
> 20 fast rechecks per transfer wait; readiness probes are a local TCP connect
> and HTTP call and restart nothing). No new component, actor, asset, trust
> boundary, RBAC grant or dependency; other sections re-walked, unchanged.
>
> **Revision note (v1.20):** Full pass for ADR-0019 (zone transfer peers
> follow the zone's pods) and two operator defects found by the new chaos
> e2e suite (`tests/e2e/chaos_test.sh`). (1) **Stale transfer peers.** On
> v0.8.0-rc.7 the primaries' `allow-transfer` and `also-notify` and the
> secondaries' `primaries` were written only when a zone was created on a
> pod, so after pod replacements a secondary held no copy of the zone while
> the zone reported `Ready=True`; worse for **I2**, a stale `allow-transfer`
> entry is an IP the cluster can hand to an unrelated pod, which could then
> AXFR the zone. New **M-52**: the peers are computed from the Pod store,
> recorded in `DNSZone.status.transferPeers`, and pushed when they change
> (a primary's ACL rewritten in place, a secondary zone replaced); a
> secondary that has the zone configured but not loaded makes the zone
> `Degraded` (`SecondaryNotLoaded`). New accepted risk **17** records what
> bindcar 0.9.0 leaves (a secondary briefly drops a zone while it is
> replaced; NOTIFY goes to the secondary Service; zones created before
> ADR-0019 keep their old `also-notify` until their pod is replaced).
> (2) **Leader lease renewed with 2 s to spare, on the shared client.** The
> lease's renewal grace was the retry period (2 s) and its requests went
> through the controllers' rate limiter with the 30 s deadline of ADR-0014,
> so one slow renewal lost the lease while the old leader kept leading for
> up to 30 s beside the new one. New **M-53**: the lease has its own client
> (no shared rate limit, an 8 s deadline) and renews one retry period after
> each renewal; accepted risk **10** revised with the bound. (3) **A removed
> bindcar env var stayed on the pods** (the Deployment patch merged `env` by
> name): a configuration removal never took effect, which matters for any
> setting removed for security reasons; **M-54**, the env list is replaced
> whole like the other owned lists. Two further availability defects of
> existing controls, no new threat: a `Bind9Cluster` / `ClusterBind9Provider`
> was not woken by instances that only reference it through `clusterRef`
> (status left at "3/4 instances are ready", ADR-0016 amended), and a zone
> reconcile could spend two minutes POSTing to a pod that was gone (the zone
> now fails that endpoint fast and retries with backoff). (4) **A deleted
> record served forever** (ADR-0015 amended, decision 7): the record
> finalizer skipped a primary whose bindcar or `named` container was
> restarting (the pod and its `emptyDir` zone data survive) and discovery
> then dropped the record on trust, so the primary and, by transfer, the
> secondary kept answering for a name removed from the spec while the zone
> reported Ready. That defeats the operator's control of what is published
> (**T1**, integrity of served data). New **M-55**: a write or deletion is
> done only once every pod that holds the zone was reached; the zone stays
> `Degraded` (`RecordDeletionPending`) meanwhile. Accepted risk **18**: a pod
> that never becomes ready again blocks record and zone deletion until it is
> deleted. No new RBAC grant (Services, pods and
> leases were already readable or writable by the operator), actor or trust
> boundary; one new network use of an existing path (the operator's SOA probe
> to a pod's DNS port, as for DS extraction), one new CRD status field. All
> other sections re-walked unchanged.
>
> **Revision note (v1.19):** Full pass for two findings from rolling
> v0.8.0-rc.6 on a real cluster. (1) **Correction to M-50.** The operator
> does set a terminating pod's gate `False` (`PodTerminating`) at the
> deletion event, but the pod's `Ready` condition followed only on the
> kubelet's next status sync, about 18 s later, past the 10 s preStop drain;
> the EndpointSlice `serving` flag follows `Ready`, so the flip does not
> move traffic before `named` exits, as v1.18 claimed. What kept the
> handover short was the new pod being Ready first and ADR-0018's
> staggering keeping a zone's other nameservers up: 2 of 136 probe queries
> lost over a staggered rollout of three instances, against 9 to 12 s of
> total outage on both `LoadBalancer` IPs in rc.5. M-50, the **D6**
> mitigations and accepted risk **16** are reworded; the likelihood rating
> is unchanged because it rested on M-51 as much as M-50. (2) **M-51 no-op
> loop fixed** (ADR-0018 decision 8). The drift check compared the bindcar
> container's `resources` with `!=`, and the API server stores an absent
> value as `{}`, so every reconcile saw a pod-template change; with the
> rollout queue that became a hot loop (about two no-op Deployment PATCHes
> a second, instances left `RolloutQueued` after every rollout had
> finished). The drift check now compares every owned field semantically,
> and a template patch that bumps no generation is remembered as a known
> no-op so it cannot repeat. This was an availability defect of an existing
> control (self-inflicted API load, misleading status), not a new threat:
> no new actor, asset, boundary, RBAC grant, CRD field or dependency. All
> other sections re-walked unchanged.
>
> **Revision note (v1.18):** Full pass for the ADR-0017 amendment (decision
> 6, hand over at the start of termination) and ADR-0018 (staggered BIND9
> rollouts), found rolling v0.8.0-rc.5 on a real cluster: a configuration
> change rolled all three nameservers of a live zone at once, and with
> MetalLB layer-2 `LoadBalancer` Services on `externalTrafficPolicy: Local`
> every query to both primaries' IPs timed out for 9 to 12 s, because each
> old pod stayed `Ready` (endpoint `serving`) from the moment `named` exited
> until its readiness probe failed. **D6** is broadened from "an empty pod
> admitted" to availability during rollouts and gains **M-50** (the operator
> closes a terminating pod's zones-loaded gate at once, so traffic moves
> before `named` exits) and **M-51** (a pod-template change is applied only
> while no instance sharing a zone or a cluster is mid-rollout, first come
> first served, a rollout past `progressDeadlineSeconds` no longer blocks).
> New accepted risk **16**: the residual MetalLB layer-2 re-announcement gap
> with `Local`, the bounded delay a stalled rollout imposes on its peers,
> and the in-process ordering lost on a leader change. **Components 1 and
> 2**, **Boundary 4**, **Scenario 1** and the **Security Controls Summary**
> record the changes. No new RBAC grant (the `pods/status` patch of
> ADR-0017 covers the new write), CRD field, actor, asset, trust boundary,
> network path or dependency; the operator subscribes to the BIND9 Pod and
> Deployment watches it already had. Accepted risk **14** is re-read
> unchanged (setting the gate `False` was already in scope). All other
> sections re-walked unchanged.
>
> **Revision note (v1.17):** Full pass for ADR-0017 (BIND9 pods are Ready
> only once their zones are loaded), found while preparing to roll
> v0.8.0-rc.4: a replacement BIND9 pod, which starts with empty `emptyDir`
> zone storage, went Ready as soon as `named` listened, entered its Service
> and answered `REFUSED` for every zone until the zone controller replayed
> them; a rollout that changes the rendered configuration replaces every
> primary at once. New threat **D6** (an empty BIND9 pod admitted to its
> Service), mapped to new mitigation **M-49**: every BIND9 pod carries the
> `bindy.firestoned.io/zones-loaded` readiness gate, and a Pod controller in
> the operator loads every live zone (and, on a primary, every record) onto
> the new pod before setting it `True`; writes reach pods whose containers
> are ready while the gate holds them out of the Service. **New RBAC grant:**
> `get`/`patch` on `pods/status` (cluster-wide role and the per-namespace
> Role); the operator stays read-only on `pods`. **Components**, **Boundaries
> 2 and 3**, **S1**, **E2**, **Attack Surface 1** and **Scenario 1** record
> the new write path: a holder of the operator's token can now mark a BIND9
> pod Ready before its zones load, or hold one out of its Service (new
> accepted risk **14**). New accepted risk **15**: the gate fails safe when
> the operator is down or older than the pod template (rollouts stall), and
> a zone that fails on every pod of an instance does not hold the pod back
> (`ZonesPartiallyLoaded`). **Accepted risk 13** revised: a replaced pod is
> now re-populated by the gate, not by the `Endpoints` replay. The operator
> also watches bindy's BIND9 pods (label-selected): no new namespace reach,
> `list`/`watch` on `pods` was already granted. No new actor, asset, trust
> boundary, network path or dependency. All other sections re-walked
> unchanged.
>
> **Revision note (v1.16):** Full pass for ADR-0016 (event-driven
> reconciliation, no periodic resync), prompted by the v0.8.0-rc.3 load test
> (300 records: about 9 reconciles per record, 3,595 record GETs, 1,681 zone
> GETs, 1,190 instance GETs over the leader's life, most of them timed
> resyncs that changed nothing). **D2** gains **M-48**: every controller
> returns `await_change` on success and on a wait for another object, each
> wait is ended by a named watch (three gaps closed with mappers, no new
> watch), failures retry with per-object capped backoff, and a steady-state
> record reconcile makes no API read (status from the cache, zone from the
> store, outcome returned). **T1** and **Boundary 4** record the trade-off as
> new accepted risk **13**: a change made inside a running BIND9 pod (with the
> RNDC/TSIG key, or through bindcar with a token its TokenReview accepts) is
> no longer reverted within five minutes, only on the owning resource's next
> event, a pod restart, an operator restart or the documented
> `bindy.firestoned.io/reconcile-trigger` annotation. **T3** gains a partial
> drift control: a cluster-level ConfigMap edited or deleted is now
> re-rendered on its watch event (it was only caught by the timer).
> Accepted risk **11** is revised: a hand-edited `DNSZone` status stands until
> the zone's next event. **Scheduled wakes** (RNDC key rotation, DNSSEC KSK
> rollover) are capped at 30 days. No new component, actor, asset, trust
> boundary, RBAC grant, network path or dependency. All other sections
> re-walked unchanged.
>
> **Revision note (v1.15):** Full pass for ADR-0013 stage 3: `named.conf` and
> `named.conf.options` are written by hornet 0.3.0's writer from typed values
> (ACL entries as address-match elements, forwarders as addresses, the
> DNSSEC policy as a typed statement), and the templates are retired. New
> mitigation **M-47** (configuration written by construction) closes the
> remaining D4 gap (hornet 0.2.0 only brace-checked `logging` and
> `dnssec-policy`) and adds a second layer behind the input validators
> against configuration injected through a CRD value (T3, D4). Checking every rendered file
> with `named-checkconf` 9.18 and 9.20 found that `validation: true` rendered
> `dnssec-validation yes` without `trust-anchors`: on 9.18 a recursive
> operand validated nothing (Scenario 2, cache poisoning), on 9.20 it did not
> start (D4). It now renders `auto`. No new component, actor, asset, trust
> boundary or dependency (hornet was added in v1.12); all other sections
> re-walked, no further changes.
>
> **Revision note (v1.14):** Full pass for ADR-0015 (bounded Kubernetes API
> cost of record and zone writes), prompted by the v0.8.0-rc.2 load test
> (300 records: 25,582 API requests, half the records unserved after 600 s,
> 30 still served after their deletion). **D2** gains **M-46**: a write
> resolves each instance's RNDC key and endpoints once per reconcile,
> endpoints and instance roles come from the existing reflector stores, the
> record controller no longer rewrites its zone's `status.records` or wakes on
> its own condition writes, and a zone reconcile no longer GETs or PATCHes
> every record. **T1** gains the orphan fix under M-46: a deleted record stays
> tracked in its zone until its data is confirmed gone from every primary,
> and a zone replay no longer re-publishes a record that is being deleted.
> **I1** records the new in-memory RNDC key cache: a key the operator already
> reads is reused for at most 60 s (invalidated on rotation and on any failed
> write); no new reader, RBAC grant, Secret list or watch, and the key type's
> `Debug` redaction still applies. No new component, actor, asset, trust
> boundary or network path. All other sections re-walked unchanged.
>
> **Revision note (v1.13):** Full pass for ADR-0014 (a bounded client-side
> deadline on non-watch Kubernetes API requests, from the v0.8.0-rc.2 load
> test). New threat **D5** (one stalled API server connection freezes the
> reconciles that share it for about five minutes; nothing in kube-rs 4.2
> bounds a request's wait for its response), mapped to new mitigation
> **M-45**: every non-watch request carries a 30 s deadline
> (`BINDY_KUBE_REQUEST_TIMEOUT_SECS`) over its headers and body, fails as a
> retryable error and re-enters the existing backoff; watches are exempt.
> Attack Surface 1 and the Resilience row of the controls summary cite M-45.
> New accepted risk **12**: Scout and the bootstrap CLI build their own
> clients without the deadline, and a legitimately slow write is now cut and
> retried. No new component, actor, asset, trust boundary, RBAC grant or
> network path; `http-body` becomes a direct dependency (already in the tree
> through kube and hyper, E3 unchanged). All other sections re-walked
> unchanged.
>
> **Revision note (v1.12):** Full pass for ADR-0013 stages 1 and 2 (hornet
> validates the BIND9 configuration the operator renders). New threat **D4**
> (one malformed configuration takes every BIND9 pod of an instance or
> cluster down: bug-177), mapped to new mitigation **M-44**: every rendered
> `named.conf*` is parsed and validated in CI across the option matrix and
> the examples, and at runtime before the ConfigMap is written, so an invalid
> render fails the reconcile with `ConfigurationInvalid` while the pods keep
> the last published configuration. **E3** records `hornet-bind9` as a new
> runtime dependency: maintained in the same organisation, Apache-2.0,
> `unsafe` forbidden, `default-features = false`, and it parses only text the
> operator itself generated; its `miette` diagnostics stack adds transitive
> crates (cargo-deny clean, in the SBOM). No new component, actor, asset,
> trust boundary, RBAC grant or network path. Stage 3 (rendering through
> hornet's writer) gets its own pass when it ships. All other sections
> re-walked unchanged.
>
> **Revision note (v1.11):** Full pass for the completion of ADR-0009
> (roadmap 01 Phases C to G). The operator is one binary built from eleven
> workspace crates; every controller now runs from the shared watch layer and
> drains on one shutdown signal. No new component, actor, asset, trust
> boundary, RBAC grant or network path; the deploy manifests are unchanged,
> and the operator holds 19 watch connections instead of 58 (measured on
> kind). Changes walked through every section:
> **D2** gains **M-41**: the `DNSZone` controller no longer retriggers on its
> own status writes, and a `DNSZone` event no longer spawns unbounded
> per-instance work outside the controller (its reconciles now go through the
> controller queue: deduplicated, rate-limited by backoff, counted). **E2/T4**
> gain **M-42**: a test fails when the Scout RBAC built by `bindy bootstrap`
> and its static manifests or documented examples drift. **Availability**
> gains **M-43**: SIGTERM and lease loss drain in-flight reconciles, and an
> e2e phase proves drift made while no operator ran is repaired on start, so
> the separate startup drift pass (which listed cluster-wide even in
> namespace-restricted mode, M-22) is deleted. New residual risks 10 (a
> deposed leader finishes its in-flight reconciles after losing the lease)
> and 11 (a hand-edited `DNSZone` status is corrected at the next requeue, no
> longer at once). Corrected in passing: the Scout component still described
> the cluster-wide `secrets: get` that M-25 removed in 2026-07. Supply chain:
> the new crates are workspace path dependencies (`publish = false`) and
> appear in the SBOM; one external dependency (`async-trait`) was dropped,
> none added. All other sections re-walked unchanged.
>
> **Revision note (v1.10):** Pass for ADR-0012 (shared DNSSEC keys from a
> Secret). New asset **DNSSEC Private Keys**; Boundary 4 gains the
> `dnssec-keys-init` init container, which is the only container that mounts
> the key Secret (read-only, read-only root filesystem, restricted context),
> copying it into a memory-backed `emptyDir` that `named` signs from. New
> threat **I5** (DNSSEC private key disclosure), mapped to the H2 Secret-name
> allow-list, the init-only mount and the tmpfs key directory. **M-14**
> amended: shared keys require `unlimited` lifetimes, enforced by a CRD CEL
> rule plus a runtime check. New accepted risk 9: Secret-supplied keys never
> roll automatically. No new actor, RBAC grant or network path: the operator
> still never reads the Secret (the kubelet mounts it). All other sections
> re-walked unchanged.
>
> **Revision note (v1.9):** Pass for ADR-0011 (CBOM per release, roadmap 28
> Phase 0). New implemented mitigation **M-39** (curated CycloneDX 1.6
> cryptographic inventory, lockfile-stamped, PR-gated, shipped and
> provenance-bound per release) and planned mitigation **M-40** (hybrid
> post-quantum key exchange on control-plane TLS, roadmap 28 Phase 2). New
> accepted risk 8: quantum-capable adversary / harvest-now-decrypt-later,
> with the full quantum modeling pass deferred to roadmap 28 Phase 5. The
> CBOM adds no runtime component, actor or trust boundary: it is a release
> artifact on the ADR-0010 rails; all other sections re-walked unchanged.
>
> **Revision note (v1.8):** Targeted update for ADR-0009 Phase B step B2
> (shared watch layer); not a full pass, which is due when ADR-0009 is fully
> implemented (roadmap 01 Phase G). The operator now holds one watch and
> cache per (kind, namespace target) instead of one per controller, built on
> kube-runtime's `unstable-runtime-stream-control` feature. No new
> component, actor, asset or trust boundary, and the same API access (same
> RBAC). New mitigation **M-38** (watch supervision and staleness metrics)
> and residual risk 7 (a shared watch is a common point of failure; the
> unstable feature). Fixed in passing: in namespace-restricted mode (M-22)
> the zone controller's `Endpoints` watch was cluster-wide, which the
> namespaced RBAC does not permit, so it was refused; it is now per
> namespace, so M-22's "per-namespace Roles only" holds for it.
>
> **Revision note (v1.7):** Pass for ADR-0010 (release SBOMs and SLSA Build
> L3) and ADR-0009 Phase A. Supply-chain rows re-checked against the release
> workflow and the GitHub rulesets rather than the previous text, which had
> drifted. New mitigations **M-33** (SLSA Build L3 provenance for every
> release artifact, images included), **M-34** (NTIA-gated SBOMs attested to
> their artifact's digest) and **M-35** (anchored signer identity in every
> verification path); **M-09** corrected (binary SBOMs had never reached a
> release). **S3 corrected:** the rulesets require signed commits, PRs and
> status checks but **0 approving reviews**, and org admins can bypass them;
> "2+ reviewers required" was not true. Recorded as residual risk with a new
> planned mitigation **M-36**. T2 and **M-15** updated: release
> `install.yaml`/`scout.yaml` pin the operator image by digest (P2-8);
> operand images remain tag-referenced. ADR-0009 Phase A moves source into a
> Cargo workspace with no change to the binary, its RBAC or its runtime
> behaviour. No new components, actors, assets or trust boundaries; all
> other sections re-walked unchanged.
>
> **Revision note (v1.6):** Pass for ADR-0008 (Scout remote endpoint mode).
> New mitigation **M-32**: the endpoint + token-file transport is a
> smaller-surface alternative to the Phase 2 kubeconfig Secret, no
> kubeconfig blob, no `secrets: get` use in this mode, endpoint and CA
> auditable in the Deployment spec, credential rotatable as a file without
> restart. The credential remains a bindy-cluster-minted scoped SA token
> (ADR-0002 credential direction, M-25 scoping unchanged); configuration is
> fail-closed against ambiguous dual-mode setups. No new trust boundary;
> same scout → queen-API edge with a second transport. All sections
> re-walked; I4/E4/T4 analyses unchanged.
>
> **Revision note (v1.5):** Pass for the bindcar v0.8.2 upgrade and the
> ADR-0006 amendment (`nextKeyRollover` from the sidecar's zone status). No
> new surface: the field is read over the existing authenticated
> operator → bindcar channel (SA token / TLS per ADR-0004), and key timing
> metadata is public-by-design scheduling information, no key material.
> Operand image default moves to bindcar v0.8.2; v0.8.1 images are skipped
> (version self-reporting divergence). All sections re-walked; no threat or
> mitigation rows changed.
>
> **Revision note (v1.4):** Full pass for ADR-0007 (uniform options
> rendering). Threat I2 (zone enumeration) updated: the cluster-level options
> builder previously emitted no `allow-transfer` directive, leaving AXFR open
> on BIND 9.18 operands configured at cluster level, now deny-by-default at
> both levels. The `dnssec-validation` fix also removes a silent
> intent-inversion (explicit `validation: false` was overridden by named's
> `auto` default). No new components, trust boundaries, assets, or actors;
> all other sections re-walked, no further changes required.
>
> **Revision note (v1.3):** Full pass for ADR-0006 (DNSSEC DS record status
> reporting), which completes roadmap 07. DNSSEC signing was stale as
> "planned"/"Future" throughout this document although Phases 1–4 shipped
> earlier. M-14 is now marked implemented (opt-in), and threat T1, Scenario 2
> (cache poisoning), D1, and the control matrix are updated accordingly. New
> surface reviewed: DS records/key tags in `DNSZone` status are public data
> by design; DNSSEC key Secrets were already covered by the H2 allow-list
> fix; the operator's new DNSKEY query path (operator → `named` :5353,
> read-only, in-cluster) is modeled in CALM. No new trust boundaries.
>
> **Revision note (v1.2):** Full pass for ADR-0005 (client-side Kubernetes API
> rate limiting). Adds mitigation **M-31** (rate-limited client, paginated
> LISTs, retries with backoff, throttling metrics) and updates threat **D2**
> (reconciliation flood): its API-server- and memory-amplification paths are
> now closed; the per-namespace CR-count limit remains open. No new
> components, trust boundaries, assets, or actors; the change is middleware
> inside the existing operator → API-server flow. All other sections
> re-walked; no further changes required.

> **Revision note (v1.1):** The v1.0 model predates the **Scout** controller
> (added 2026-03-20) and did not cover it. This revision adds Scout as a
> first-class component, trust boundary, and set of STRIDE threats, and
> updates the model to reflect several mitigations merged since v1.0: the B-5
> Secret-RBAC split, the opt-in namespace-scoped operator mode, the BIND9
> operand's move to an unprivileged DNS port (no `NET_BIND_SERVICE`), the
> expansion of `ValidatingAdmissionPolicy` coverage from 0 to 8 policies (16 manifests
> including bindings), Scout
> namespace whitelisting (`--namespace-selector`, M-30), and automated
> Dependabot auto-merge.
>
> **This draft identified a CRITICAL finding (I4/E4/Scenario 6): Scout's
> `ClusterRole` carried an unscoped `secrets: get` grant across every namespace
> in the cluster.** Per this project's disclosure practice (fix before
> publishing exploit-level detail), that finding was remediated (M-25) the same
> day it was drafted, before this revision was published. The document below
> retains the full historical write-up of the finding (marked ✅ FIXED) for
> audit-trail completeness; there is no live unpatched CRITICAL item in this
> revision as published.

---

## Table of Contents

- [Overview](#overview)
- [System Description](#system-description)
- [Assets](#assets)
- [Trust Boundaries](#trust-boundaries)
- [STRIDE Threat Analysis](#stride-threat-analysis)
- [Attack Surface](#attack-surface)
- [Threat Scenarios](#threat-scenarios)
- [Mitigations](#mitigations)
- [Residual Risks](#residual-risks)
- [Security Architecture](#security-architecture)

---

## Overview

This document provides a comprehensive threat model for the Bindy DNS Operator, a Kubernetes operator that manages BIND9 DNS servers. The threat model uses the STRIDE methodology (Spoofing, Tampering, Repudiation, Information Disclosure, Denial of Service, Elevation of Privilege) to identify and analyze security threats.

### Objectives

1. **Identify threats** to the DNS infrastructure managed by Bindy
2. **Assess risk** for each identified threat
3. **Document mitigations** (existing and required)
4. **Provide security guidance** for deployers and operators
5. **Support compliance** with SOX 404, PCI-DSS 6.4.1, Basel III

### Scope

**In Scope:**
- Bindy operator container and runtime
- Custom Resource Definitions (CRDs) and Kubernetes API interactions
- BIND9 pods managed by Bindy
- DNS zone data and configuration
- RNDC (Remote Name Daemon Control) communication
- Container images and supply chain
- CI/CD pipeline security

**Out of Scope:**
- Kubernetes cluster security (managed by platform team)
- Network infrastructure security (managed by network team)
- Physical security of data centers
- DNS client security (recursive resolvers outside our control)

---

## System Description

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     Kubernetes Cluster                       │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │              bindy-system Namespace                   │    │
│  │                                                      │    │
│  │  ┌──────────────────────────────────────────────┐  │    │
│  │  │        Bindy Operator (Deployment)         │  │    │
│  │  │  ┌────────────────────────────────────────┐  │  │    │
│  │  │  │  Operator Pod (Non-Root, ReadOnly)   │  │    │    │
│  │  │  │  - Watches CRDs                        │  │  │    │
│  │  │  │  - Reconciles DNS zones                │  │  │    │
│  │  │  │  - Manages BIND9 pods                  │  │  │    │
│  │  │  │  - Uses RNDC for zone updates         │  │  │    │
│  │  │  └────────────────────────────────────────┘  │  │    │
│  │  └──────────────────────────────────────────────┘  │    │
│  │                                                      │    │
│  │  ┌──────────────────────────────────────────────┐  │    │
│  │  │       BIND9 Primary (StatefulSet)           │  │    │
│  │  │  ┌────────────────────────────────────────┐  │  │    │
│  │  │  │  BIND Pod (Non-Root, ReadOnly)         │  │  │    │
│  │  │  │  - Authoritative DNS (Port 53)         │  │  │    │
│  │  │  │  - RNDC Control (Port 9530)             │  │  │    │
│  │  │  │  - Zone files (ConfigMaps)             │  │  │    │
│  │  │  │  - RNDC key (Secret, read-only)        │  │  │    │
│  │  │  └────────────────────────────────────────┘  │  │    │
│  │  └──────────────────────────────────────────────┘  │    │
│  │                                                      │    │
│  │  ┌──────────────────────────────────────────────┐  │    │
│  │  │      BIND9 Secondaries (StatefulSet)        │  │    │
│  │  │  - Receive zone transfers from primary       │  │    │
│  │  │  - Provide redundancy                        │  │    │
│  │  │  - Geographic distribution                   │  │    │
│  │  └──────────────────────────────────────────────┘  │    │
│  │                                                      │    │
│  └────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │         Other Namespaces (Multi-Tenancy)           │    │
│  │  - team-web (DNSZone CRs)                          │    │
│  │  - team-api (DNSZone CRs)                          │    │
│  │  - platform-dns (Bind9Cluster CRs)                 │    │
│  └────────────────────────────────────────────────────┘    │
│                                                              │
└─────────────────────────────────────────────────────────────┘
          │                           ▲
          │ DNS Queries (UDP/TCP 53)  │
          ▼                           │
    ┌─────────────────────────────────────┐
    │       External DNS Clients          │
    │  - Recursive resolvers              │
    │  - Corporate clients                │
    │  - Internet users                   │
    └─────────────────────────────────────┘
```

### Components

1. **Bindy Operator**
   - Kubernetes operator written in Rust: one binary built from a Cargo
     workspace (ADR-0009), one crate per controller over a shared controller SDK
   - Watches custom resources (Bind9Cluster, Bind9Instance, DNSZone, DNS records)
     through one shared watch and cache per kind and namespace target
   - Writes DNS records as TSIG-signed RFC 2136 UPDATEs, one per RRset, to each
     primary pod's `named` on container port 5353 (UDP, or TCP when the UPDATE
     exceeds the EDNS payload; ADR-0021, M-62)
   - Leader-elected; controllers start only after the shared caches finish their
     first list (M-56); SIGTERM or loss of the lease drains in-flight reconciles,
     and a SIGTERM while waiting for the lease or the caches exits cleanly
   - Reconciles desired state with actual state
   - Manages BIND9 deployments, ConfigMaps, Secrets, Services. Every Secret
     operation goes through one audited `SecretApi` and emits a
     `bindy::audit` event (ADR-0024, M-67); its Secret watch and
     per-reconcile Secret checks read metadata only (M-68)
   - Uses RNDC to update zones on running BIND9 instances
   - Sets the `bindy.firestoned.io/zones-loaded` condition on its BIND9 pods
     (`pods/status` patch) once their zones are loaded (ADR-0017), and back to
     `False` when a pod starts terminating (ADR-0017 decision 6); it reads
     bindy's BIND9 pods through a label-selected watch
   - Applies a change to a BIND9 pod template only while no instance sharing
     a zone or a cluster with it is mid-rollout (ADR-0018), ordered in
     process (the leader's memory, rebuilt from the cluster on restart)

2. **BIND9 Pods**
   - Authoritative DNS servers running BIND9
   - Primary server handles zone updates
   - Secondary servers replicate zones via AXFR/IXFR
   - Exposed via LoadBalancer or NodePort services
   - Each pod carries the zones-loaded readiness gate: it is Ready, and in its
     Service, only after the operator has loaded its zones, and leaves its
     Service at the start of its termination (ADR-0017)

3. **Custom Resources (CRDs)**
   - `Bind9Cluster`: Cluster-scoped, defines BIND9 cluster topology
   - `Bind9Instance`: Namespaced, defines individual BIND9 server
   - `DNSZone`: Namespaced, defines DNS zone (e.g., example.com)
   - DNS Records: `ARecord`, `CNAMERecord`, `MXRecord`, etc.

4. **Supporting Resources**
   - ConfigMaps: Store BIND9 configuration and zone files
   - Secrets: Store RNDC keys (symmetric HMAC keys). Every BIND9 pod mounts
     `{instance}-rndc-key`, owned by its instance: a key the operator
     generates, or the mirror of the `bindy-` Secret a `rndcKey.secretRef`
     names (ADR-0023); the referenced Secret is never mounted. The mirror
     records the source's `resourceVersion`, so the operator reads the
     source's key only when it changed (ADR-0024, M-68)
   - Services: Expose DNS (port 53, forwarding to the operand's unprivileged
     container port 5353) and RNDC (port 9530)
   - ServiceAccounts: RBAC for operator access

5. **Bindy Scout** *(added 2026-03-20; see [Trust Boundary 6](#boundary-6-scout-controller))*
   - Separate Deployment/`ServiceAccount` from the main operator (same binary, `bindy scout`), its own `ClusterRole` (`bindy-scout`)
   - Watches `Ingress`, `Service` (LoadBalancer), and Gateway API `HTTPRoute`/`TLSRoute`/`TCPRoute`
     resources **cluster-wide, across all namespaces**, for an opt-in annotation
   - On opt-in, auto-creates/deletes `ARecord` CRs (and prunes the records of removed
     hosts, of a Service that leaves `LoadBalancer` and of an old `--cluster-name`,
     M-59; each record is owned by one source kind and named from a hash of its
     source, M-63) and adds/removes its own finalizer. When configured it
     publishes only into `--allowed-zones` and only addresses inside
     `--allowed-ip-ranges` (M-69), and it leaves a record of another cluster
     alone when the bindy cluster refuses its delete (M-72),
     which requires cluster-wide `patch`/`update` on the watched resource types, not
     just `get`/`list`/`watch`
   - **Phase 2 (multi-cluster) mode**: reads a kubeconfig from one Kubernetes `Secret`
     to target a *different* cluster's Bindy install, through a namespaced,
     `resourceNames`-restricted Role (`bindy-scout-secrets-reader`), applied only in
     that mode. Until 2026-07-19 this was a cluster-wide `secrets: get`; see I4 and
     M-25. The endpoint mode (M-32) needs no Secret access at all. That read
     is a `bindy::audit` event (`purpose=scout-kubeconfig`, ADR-0024, M-67), as
     are `bindy bootstrap`'s ServiceAccount token writes, reads and revokes.
   - **On the bindy cluster** each edge cluster's Scout writes as its own
     ServiceAccount from `bindy bootstrap mc`, whose Role grants CRUD on
     `arecords` and read on `dnszones` in one namespace. Registered in the
     `bindy-scout-sources` ConfigMap, that account can only write records for
     its own cluster names and zones (M-70, ADR-0025).

---

## Assets

### High-Value Assets

| Asset | Description | Confidentiality | Integrity | Availability | Owner |
|-------|-------------|-----------------|-----------|--------------|-------|
| **DNS Zone Data** | Authoritative DNS records for all managed domains | Medium | **Critical** | **Critical** | Teams/Platform |
| **RNDC Keys** | Symmetric HMAC keys for BIND9 control, in the operator-owned `{instance}-rndc-key` the pods mount (generated, or mirrored from a `secretRef` and annotated `bindy.firestoned.io/rndc-source-secret`; owned by the instance and deleted with it) | **Critical** | **Critical** | High | Security Team |
| **RNDC key source Secret** | User-managed `bindy-` Secret a `rndcKey.secretRef` names (ADR-0023): the key under its `secretKey` entry or as `rndc.key`. Read by the operator only, never mounted; the mirror is a second copy of the same key (accepted risk 21) | **Critical** | **Critical** | Medium | Key owner |
| **DNSSEC Private Keys** | KSK/ZSK private keys signing a zone (user Secret with `keysFrom.secretRef`, or generated in the pod); possession forges validly signed answers until the DS is withdrawn | **Critical** | **Critical** | High | Zone owner |
| **Operator Binary** | Signed container image with operator logic | Medium | **Critical** | High | Development Team |
| **BIND9 Configuration** | named.conf, zone configs | Low | **Critical** | High | Platform Team |
| **Kubernetes API Access** | ServiceAccount token for operator | **Critical** | **Critical** | **Critical** | Platform Team |
| **Scout ServiceAccount Token** | Grants cluster-wide `patch`/`update` on Ingress/Service/Gateway-API routes on the edge cluster; Secret read only on the one Phase 2 kubeconfig Secret (M-25, since 2026-07-19) | High | **Critical** | High | Platform Team |
| **Scout remote token (bindy cluster)** | The `bootstrap mc` ServiceAccount token an edge cluster's Scout writes with: CRUD on `arecords`, read on `dnszones`, in one namespace. Bounded to its cluster names and zones once registered (M-70) | High | **Critical** | High | Platform Team |
| **Scout source registry** | `bindy-scout-sources` ConfigMap per records namespace (ADR-0025): which cluster names and zones each Scout ServiceAccount may publish. Editing it widens or narrows what an edge cluster may publish (accepted risk 23) | Low | **Critical** | Medium | Platform Team |
| **CRD Schemas** | Define API contract for DNS management | Low | **Critical** | Medium | Development Team |
| **Audit Logs** | Record of all DNS changes and access | High | **Critical** | High | Security Team |
| **SBOM** | Software Bill of Materials for compliance | Low | **Critical** | Medium | Compliance Team |

### Asset Protection Goals

- **DNS Zone Data**: Prevent unauthorized modification (tampering), ensure availability
- **RNDC Keys**: Prevent disclosure (compromise allows full BIND9 control)
- **Operator Binary**: Prevent supply chain attacks, ensure code integrity
- **Kubernetes API Access**: Prevent privilege escalation, enforce least privilege
- **Audit Logs**: Ensure non-repudiation, prevent tampering, retain for compliance

---

## Trust Boundaries

### Boundary 1: Kubernetes Cluster Perimeter

**Trust Level:** High
**Description:** Kubernetes API server, etcd, and cluster networking

**Assumptions:**
- Kubernetes RBAC is properly configured
- etcd is encrypted at rest
- Network policies are enforced
- Node security is managed by platform team

**Threats if Compromised:**
- Attacker gains full control of all resources in cluster
- DNS data can be exfiltrated or modified
- Operator can be manipulated or replaced

---

### Boundary 2: bindy-system Namespace

**Trust Level:** High
**Description:** Namespace containing Bindy operator and BIND9 pods

**Assumptions:**
- RBAC limits access to authorized ServiceAccounts only
- Secrets are encrypted at rest in etcd
- Pod Security Standards enforced (Restricted)

**Threats if Compromised:**
- Attacker can read RNDC keys
- Attacker can modify DNS zones
- Attacker can disrupt DNS service
- With the operator's identity, attacker can open or hold a BIND9 pod's
  zones-loaded readiness gate (accepted risk 14)

---

### Boundary 3: Operator Container

**Trust Level:** Medium-High
**Description:** Bindy operator runtime environment

**Assumptions:**
- Container runs as non-root user
- Filesystem is read-only except /tmp
- No privileged capabilities
- Resource limits enforced

**Threats if Compromised:**
- Attacker can abuse Kubernetes API access
- Attacker can read secrets operator has access to. Before that, the
  process itself holds less: its Secret watch is a metadata watch and its
  existence, rotation and mirror checks read metadata only (M-68), so a
  memory dump of a healthy operator holds the RNDC keys it loaded (cached
  for at most 60 s, M-46), not every Secret of its namespaces; every key it
  reads afterwards is recorded in the Secret audit trail (M-67)
- Attacker can disrupt reconciliation loops
- Attacker can patch the status of bindy's BIND9 pods: admit a pod before
  its zones load, or keep new pods out of their Service (ADR-0017, accepted
  risk 14). It cannot change a pod's spec, labels or lifecycle
- Attacker can bypass rollout staggering (ADR-0018) by patching Deployments
  directly; the queue orders only the operator's own writes

---

### Boundary 4: BIND9 Container

**Trust Level:** Medium
**Description:** BIND9 DNS server runtime

**Assumptions:**
- Container runs as non-root and, as of the operand's move to the
  **unprivileged container port 5353**, with **no added Linux capabilities**
  (`NET_BIND_SERVICE` has been dropped; the container `securityContext`
  `capabilities.add` is empty). The `named` process cannot bind any port `<
  1024` even if further compromised. The client-facing `Service` still exposes
  the standard DNS port 53 and forwards to the container's 5353.
- Exposed to internet (Service port 53 → container port 5353)
- Configuration is managed by operator (read-only)
- Zone transfers run pod to pod: a secondary's `primaries` name the
  primary pods admitted by the zones-loaded gate, and a primary's
  `allow-transfer` names the current secondary pods; the operator rewrites
  both when pods are replaced and records them in
  `DNSZone.status.transferPeers` (ADR-0019). A primary sends NOTIFY to the
  secondary instance's Service (port 53).
- The pod does not decide when it leaves its Service: the operator closes
  its zones-loaded gate when the pod is deleted (ADR-0017 decision 6), so a
  terminating `named` that has stopped answering is no longer routed to,
  whatever its readiness probe says.
- With DNSSEC signing, `named` holds the zone's private keys in its key
  directory by design. With `keysFrom.secretRef` (ADR-0012) the key Secret
  is mounted **only** into the `dnssec-keys-init` init container (read-only
  mount, read-only root filesystem, same restricted context as `named`),
  which copies the keys into a memory-backed `emptyDir`; `named` never
  mounts the Secret (`build_dnssec_keys_init_container`,
  `crates/bindy-bind9/src/bind9_resources/`).
- The pod mounts exactly one RNDC Secret, `{instance}-rndc-key`, in every
  RNDC mode. With `rndcKey.secretRef` it is the operator's mirror of two
  entries of the referenced `bindy-` Secret (ADR-0023, M-65, M-66); before,
  the pod mounted every entry of whatever Secret the reference named.

**Threats if Compromised:**
- Attacker can serve malicious DNS responses
- Attacker can exfiltrate zone data, and the zone's DNSSEC private keys
  when the pod signs (see I5)
- Zone data the attacker changes inside the running pod stays changed until
  the owning resource's next event, the pod's replacement or an operator
  restart: there is no periodic resync to revert it (ADR-0016, accepted risk
  13). A container restart inside the same pod keeps the `emptyDir` zone
  data, and the change with it
- A compromised operand cannot open its own readiness gate: the condition is
  on `pods/status`, which the operand's `bind9` ServiceAccount has no access
  to (ADR-0017)
- Attacker can pivot to other cluster resources (if network policies weak):
  a reference `NetworkPolicy` now exists (`deploy/pod-hardening.yaml`,
  ingress/egress scoped to 5353 for peer transfers and 53 for CoreDNS) but is
  **not applied by any install target**; it is opt-in and must be applied
  manually. See M-17 in [Mitigations](#mitigations).

---

### Boundary 5: External Network (Internet)

**Trust Level:** Untrusted
**Description:** Public internet where DNS clients reside

**Assumptions:**
- All traffic is potentially hostile
- DDoS attacks are likely
- DNS protocol vulnerabilities will be exploited

**Threats:**
- DNS amplification attacks (abuse open resolvers)
- Cache poisoning attempts
- Zone enumeration (AXFR abuse)
- DoS via query floods

---

### Boundary 6: Scout Controller

**Trust Level:** Medium (cluster-wide blast radius, narrower purpose than the main operator)
**Description:** Separate binary/`ServiceAccount`/`ClusterRole` (`bindy-scout`) that
watches source objects (`Ingress`, `Service`, `HTTPRoute`, `TLSRoute`, `TCPRoute`)
**across every namespace** and creates `ARecord` CRs in response to an opt-in
annotation. In Phase 2 (multi-cluster) mode it also reads a kubeconfig `Secret` to
act against a remote cluster.

**Assumptions:**
- Runs as its own Deployment/ServiceAccount, distinct from the main operator
- Pod-hardening posture (non-root, read-only rootfs, seccomp) matches the main operator
- The opt-in annotation (`bindy.firestoned.io/scout-enabled: "true"`) is the only gate
  before Scout mutates a resource: any tenant who can set that annotation on their
  own `Ingress`/`Service`/route object can cause Scout to write `ARecord`s
- **(Fixed 2026-07-19, M-25)** Scout's Secret access is namespaced and
  `resourceNames`-restricted to the single Phase 2 kubeconfig Secret, no longer
  cluster-wide. Same-cluster-only deployments (the default) get no Secret access at all.

**Threats if Compromised:**
- **Cross-tenant object tampering.** Scout's cluster-wide `patch`/`update` on
  `Ingress`/`Service`/route types (required for its own finalizer bookkeeping) means
  a compromised Scout could, in principle, modify any tenant's Ingress/Service/route
  object in any namespace, not only add/remove its own finalizer. This remains the
  primary residual risk for this component; see
  [T4](#t4-cross-tenant-tampering-via-scouts-cluster-wide-write-rbac).
- ~~Cluster-wide Secret exfiltration~~: **fixed 2026-07-19 (M-25)**. Scout's
  `ClusterRole` no longer grants any Secret access; a namespaced,
  `resourceNames`-restricted Role scoped to the single Phase 2 kubeconfig Secret is
  used instead (`deploy/scout/secrets-reader-rbac.yaml`, applied only when Phase 2
  mode is configured). See [I4](#i4-scout-cluster-wide-secret-read) and
  [Scenario 6](#scenario-6-compromised-scout-pod) for the historical analysis and
  current (resolved) status.
- A compromised Scout is **not** a path to DNS zone data or RNDC keys directly (it
  only creates `ARecord`s, gated by the same zone-authorization check as the main
  operator). Its distinctive risk is the cross-tenant write surface above.
- **Cross-cluster record injection** (multi-cluster mode). The bindy cluster
  trusts each edge cluster's token, and the zone grant names bare namespaces,
  which repeat across clusters. Without ADR-0025 controls a tenant on one
  edge cluster, or that cluster's compromised Scout, could publish into,
  rewrite or delete another cluster's records; see
  [S4](#s4-a-scout-claims-another-clusters-name) and
  [T5](#t5-cross-cluster-dns-record-injection-through-scout). The bindy-side
  half of this boundary is the `bindy-scout-source-binding` admission policy
  (M-70) and the DNSZone zone-label rule (M-71), which hold whatever the edge
  cluster does.

---

## STRIDE Threat Analysis

### S - Spoofing (Identity)

#### S1: Spoofed Kubernetes API Requests

**Threat:** Attacker impersonates the Bindy operator ServiceAccount to make unauthorized API calls.

**Impact:** HIGH
**Likelihood:** LOW (requires compromised cluster or stolen token)

**Attack Scenario:**
1. Attacker compromises a pod in the cluster
2. Steals ServiceAccount token from `/var/run/secrets/kubernetes.io/serviceaccount/token`
3. Uses token to impersonate operator and modify DNS zones, or to set the
   zones-loaded condition on a BIND9 pod's status (ADR-0017, accepted risk 14)

**Mitigations:**
- ✅ RBAC least privilege (operator cannot delete resources)
- ✅ Pod Security Standards (non-root, read-only filesystem)
- ✅ Short-lived ServiceAccount tokens (TokenRequest API)
- ❌ **MISSING**: Network policies to restrict egress from operator pod
- ❌ **MISSING**: Audit logging for all ServiceAccount API calls

**Residual Risk:** MEDIUM (need network policies and audit logs)

---

#### S2: Spoofed RNDC Commands

**Threat:** Attacker gains access to RNDC key and sends malicious commands to BIND9.

**Impact:** CRITICAL
**Likelihood:** LOW (RNDC keys stored in Kubernetes Secrets with RBAC)

**Attack Scenario:**
1. Attacker compromises operator pod or namespace
2. Reads RNDC key from Kubernetes Secret
3. Connects to BIND9 RNDC port (9530) and issues commands (e.g., `reload`, `freeze`, `thaw`)

**Mitigations:**
- ✅ Secrets encrypted at rest (Kubernetes)
- ✅ RBAC limits secret read access to operator only
- ✅ RNDC port (9530) not exposed externally
- ❌ **MISSING**: Secret access audit trail (H-3)
- ⚠️ **PARTIAL**: RNDC key rotation: documented manual runbook only, not automated

**Residual Risk:** MEDIUM (need secret audit trail)

---

#### S3: Spoofed Git Commits (Supply Chain)

**Threat:** Attacker forges commits without proper signature, injecting malicious code.

**Impact:** CRITICAL
**Likelihood:** VERY LOW (branch protection enforces signed commits)

**Attack Scenario:**
1. Attacker compromises GitHub account or uses stolen SSH key
2. Pushes unsigned commit to feature branch
3. Attempts to merge to main without proper review

**Mitigations:**
- ✅ All commits MUST be signed (GPG/SSH): `required_signatures` ruleset on `main`
- ✅ CI/CD verifies commit signatures ("Verify Signed Commits" is a required check)
- ✅ Changes reach `main` only through PRs; force pushes and deletion blocked
- ✅ Linear history (no merge commits)
- ❌ **No required approving reviews**: the rulesets require 0 approvals and there
  is no `CODEOWNERS` file, so a PR author can merge their own change once checks
  pass (corrected 2026-10-03; earlier revisions claimed 2+ reviewers). See M-36.
- ⚠️ Organization admins can bypass the `main` rulesets

**Residual Risk:** LOW-MEDIUM (a stolen account with a valid signing key can
land a change with no second person in the loop; signatures make it
attributable, not prevented)

---

#### S4: A Scout Claims Another Cluster's Name

**Threat:** A Scout stamps `bindy.firestoned.io/source-cluster` with another
edge cluster's name, so its records pass as that cluster's, its stale-cluster
cleanup treats that cluster's records as its own leftovers, and audit trails
by label point at the wrong cluster.

**Impact:** HIGH (enables T5)
**Likelihood:** LOW (needs a compromised Scout, its token, or write access to
its Deployment)

**Mitigations:**
- ✅ The cluster name of a **registered** account is bound at admission: a
  record it creates or changes must carry one of the names in its
  `<sa>.clusters` registry key, and a record it changes or deletes must
  already carry one (M-70). The label is no longer self-declared for that
  account.
- ✅ One ServiceAccount per edge cluster is the default practice
  (`bindy bootstrap mc --service-account`), so a name maps to one identity.
- ⚠️ An account not yet registered is still self-declared (accepted risk 23).

**Residual Risk:** LOW once every account is registered; HIGH for an
unregistered account in a namespace shared with other clusters.

---

### T - Tampering (Data Integrity)

#### T1: Tampering with DNS Zone Data

**Threat:** Attacker modifies DNS records to redirect traffic or cause outages.

**Impact:** CRITICAL
**Likelihood:** LOW (requires Kubernetes API access)

**Attack Scenario:**
1. Attacker gains write access to DNSZone CRs (via compromised RBAC or stolen credentials)
2. Modifies A/CNAME records to point to attacker-controlled servers
3. Traffic is redirected, enabling phishing, data theft, or service disruption

**Mitigations:**
- ✅ RBAC enforces least privilege (users can only modify zones in their namespace)
- ✅ GitOps workflow (changes via pull requests, not direct kubectl)
- ✅ Audit logging in Kubernetes (all CR modifications logged)
- ❌ **MISSING**: Webhook validation for DNS records (prevent obviously malicious changes)
- ✅ **Deleted records are not left served** (M-46, ADR-0015, 2026-10-05): a
  deleted record stays in its zone's `status.records` until its data is
  confirmed gone from every primary endpoint, so a failed finalizer cleanup is
  retried instead of forgotten; and a zone replay skips a record that is gone
  or being deleted, so it cannot re-publish data the finalizer just removed
- ✅ **A record write is atomic** (M-62, ADR-0021, 2026-10-13): each RRset is
  replaced in one TSIG-signed UPDATE, so a resolver never sees the name
  missing or half written during a reconcile, and unchanged records are not
  rewritten
- ✅ **Failures no longer count as success** (M-56, M-57, M-62, 2026-10-11): a record
  finalizer keeps its finalizer when the zone or a primary-role lookup fails,
  a primary whose role cannot be read fails the write instead of being
  skipped, every record kind's values are validated before the live RRset is
  touched, and an RRset replacement answered with anything but `NOERROR`
  fails the write (M-57, M-62); Scout prunes the records of
  removed hosts and validates TCPRoute record names (M-59)
- ✅ **A name shared by two records stays served** (M-63, M-64, ADR-0022,
  2026-10-13): Scout keeps a legacy or previous-cluster record until the
  record replacing it is bound to its zone and `Ready`, and when a record is
  deleted or renamed while another live record still declares its name, the
  operator re-pushes that record instead of deleting the RRset
- ⚠️ **Out-of-band changes inside BIND9 are not reverted on a timer**
  (ADR-0016, 2026-10-06, accepted risk 13): an attacker holding an
  instance's RNDC/TSIG key, or a token bindcar's TokenReview accepts, can
  change zone data directly in a running pod. The operator used to re-push
  every record every 5 minutes, which reverted such a change; it now
  reverts it only on the owning resource's next event, a pod restart, an
  operator restart, or the `bindy.firestoned.io/reconcile-trigger`
  annotation. Controls on the path itself are unchanged: per-instance RNDC
  keys in Secrets readable only by the operator (B-5), RNDC not exposed
  outside the cluster, bindcar TokenReview authentication (Mode B) and TLS
  (ADR-0004)
- ✅ **DNSSEC signing** (M-14, opt-in, roadmap 07 complete 2026-09-27): zones signed via
  BIND9 `dnssec-policy`; DS records auto-published in `DNSZone.status.dnssec`
  (ADR-0006) so the chain of trust can actually be completed in the parent zone.
  In-transit tampering is detectable by validating resolvers once DS is published.

**Residual Risk:** MEDIUM → LOW-MEDIUM for signed zones (signing is opt-in and
requires DS publication in the parent zone; unsigned zones keep the prior risk)

---

#### T2: Tampering with Container Images

**Threat:** Attacker replaces legitimate Bindy/BIND9 container image with malicious version.

**Impact:** CRITICAL
**Likelihood:** VERY LOW (signed images, supply chain controls)

**Attack Scenario:**
1. Attacker compromises CI/CD pipeline or registry credentials
2. Pushes malicious image with same tag (e.g., `:latest`)
3. Operator pulls compromised image on next rollout

**Mitigations:**
- ✅ Release images Cosign-signed, with SLSA Build L3 provenance and an SBOM
  attestation bound to the image digest (M-33, M-34; ADR-0010)
- ✅ Verification anchors on the release workflow identity, so a fork's or a
  lookalike repository's signature does not pass (M-35)
- ✅ GitHub Actions signed commits verification
- ✅ Multi-stage builds minimize attack surface
- ⚠️ **PARTIAL**: Release `install.yaml` and `scout.yaml` pin the operator image
  by digest (P2-8); operand images (BIND9, bindcar) are still tag-referenced (M-15)
- ❌ **MISSING**: Admission-time signature verification (VAP 15 restricts image
  sources but cannot check signatures; the Kyverno example in Signed Releases does)

**Residual Risk:** LOW (a replaced tag fails provenance and signature
verification, but nothing enforces that verification at admission by default)

---

#### T3: Tampering with ConfigMaps/Secrets

**Threat:** Attacker modifies BIND9 configuration or RNDC keys via Kubernetes API.

**Impact:** HIGH
**Likelihood:** LOW (RBAC protects ConfigMaps/Secrets)

**Attack Scenario:**
1. Attacker gains elevated privileges in `bindy-system` namespace
2. Modifies BIND9 ConfigMap to disable security features or add backdoor zones
3. BIND9 pod restarts with malicious configuration

**Mitigations:**
- ✅ Operator has NO delete permissions on Secrets/ConfigMaps (C-2)
- ✅ RBAC limits write access to operator only
- ✅ **B-5 hardening (2026-06-30):** the operator's cluster-wide `ClusterRole` grants
  only `get`/`list`/`watch` on Secrets; the mutating verbs
  (`create`/`update`/`patch`/`delete`) were moved to a namespaced Role
  (`bindy-secrets-writer`) bound **only in the operator's own namespace**. A
  compromised operator can no longer create/modify/delete Secrets in other
  namespaces such as `kube-system`.
- ✅ **Configuration written by construction** (M-47, ADR-0013 stage 3,
  2026-10-05): a CRD value cannot close a block and add directives to the
  generated configuration. Values are validated (CRD patterns, `bind9_acl`,
  M-24), parsed into typed hornet fields, and written by hornet's writer,
  which quotes and escapes each for its position; the rendered result is
  parsed again before the ConfigMap is written (M-44)
- ✅ **A mirrored RNDC key cannot inject configuration** (M-65, ADR-0023,
  2026-10-14): the operator re-renders `rndc.key`, which `named.conf`
  includes, from a user Secret's entries. The key name is always the
  constant `bindy-operator` (the source's name is never written), and the
  key must be standard base64, so a quote, `;`, brace or newline that could
  close the `key` statement is refused before any Secret is written
- ❌ **MISSING**: Immutable ConfigMaps: `build_configmap` / `build_cluster_configmap`
  (`crates/bindy-bind9/src/bind9_resources/`) do not set `immutable: true`, so a generated ConfigMap can
  be edited in place by anyone holding namespace write access (audit finding P2-2)
- ❌ **MISSING**: ConfigMap/Secret integrity checks (hash validation)
- ⚠️ **PARTIAL**: Automated drift detection. A generated ConfigMap edited or
  deleted is compared with what the operator renders and rewritten on its
  watch event: an instance's own ConfigMap through its owner reference, and,
  since ADR-0016 (M-48), a cluster-level ConfigMap, which has no owner and
  was previously caught only by the 5-minute resync. Not covered: BIND9's
  running state inside the pod (accepted risk 13)

**Residual Risk:** MEDIUM (need integrity checks; note the B-5 split reduces but does
not eliminate risk; the operator can still write Secrets within its own namespace.
Scout's Secret access, formerly a separate larger gap, was fixed 2026-07-19; see I4.)

---

#### T4: Cross-Tenant Tampering via Scout's Cluster-Wide Write RBAC

**Threat:** A compromised Scout pod/token modifies an `Ingress`, `Service`, or
Gateway API route object belonging to a different team/namespace.

**Impact:** HIGH
**Likelihood:** LOW (requires compromising the Scout pod or its ServiceAccount token)

**Attack Scenario:**
1. Attacker exploits a vulnerability in Scout (memory corruption, dependency CVE) or
   steals its ServiceAccount token from a compromised node
2. Scout's `ClusterRole` grants `patch`/`update` on `ingresses`, `services`,
   `httproutes`, `tlsroutes`, `tcproutes` **cluster-wide** (required so
   `kube-rs`'s `finalizer::finalizer()` helper can add/remove Scout's finalizer on
   the *main resource*, not just a subresource; see the comments in
   `deploy/scout/clusterrole.yaml`)
3. Attacker uses this to modify a tenant's Ingress/Service/route object in a
   namespace Scout has no legitimate business reason to touch that day

**Mitigations:**
- ✅ Scope is limited to `patch`/`update`: no `create`/`delete` on these types
- ✅ Same pod-hardening posture as the main operator (non-root, read-only rootfs)
- ✅ **Namespace whitelisting (`--namespace-selector` / `BINDY_SCOUT_NAMESPACE_SELECTOR`,
  M-30):** when configured, a namespace must match the selector *and* the individual
  object must carry its own opt-in annotation before Scout acts. This reduces the
  set of namespaces Scout's *application logic* will touch during normal operation,
  ⚠️ but does **not** shrink the underlying `ClusterRole` grant. A directly compromised
  ServiceAccount token still technically holds cluster-wide `patch`/`update` on these
  types regardless of the selector (the selector is enforced by Scout's own
  reconciler code, not by RBAC). **Opt-in and unset by default**; see M-30.
- ❌ **MISSING**: No admission policy constrains *what* Scout can patch on
  these types (e.g. restrict to only the finalizer/annotation fields), M-28

**Residual Risk:** MEDIUM (bounded by patch/update-only scope; namespace whitelisting
reduces likelihood of an *opt-in-triggered* incident when configured, but does not
change what a *token-holding* attacker could reach; no field-level admission control
exists to constrain that further)

---

#### T5: Cross-Cluster DNS Record Injection Through Scout

**Threat:** A tenant on edge cluster `foo` annotates a source with
`bindy.firestoned.io/zone: bar.example.com`, or `foo`'s Scout is compromised,
and records land in a zone that belongs to edge cluster `bar`: a new name, an
existing name taken over (the records controller replaces the RRset with the
last write, ADR-0021), an address pointed outside `foo`'s networks, or
`bar`'s records rewritten or deleted.

**Impact:** HIGH (authoritative DNS for another team's or cluster's names)
**Likelihood:** MEDIUM without ADR-0025 controls (any tenant who can annotate
a source, in a namespace whose name another cluster's zone grants); LOW with
them

**Attack Scenario:**
1. `bar.example.com`'s DNSZone grants namespace `team-a` (for cluster `bar`)
   in `bindy.firestoned.io/allow-zone-namespaces`
2. A tenant creates namespace `team-a` on cluster `foo` and an Ingress for
   `www.bar.example.com` annotated with that zone
3. `foo`'s Scout finds the zone, finds the grant (namespaces are compared by
   name only) and writes the record; or a stolen `foo` token writes any
   record directly
4. The record is served by `bar.example.com`

**Mitigations:**
- ✅ **Scout allowlists (M-69):** `--allowed-zones` refuses any zone outside
  the cluster's own (and its subzones) before the zone lookup;
  `--allowed-ip-ranges` refuses any address outside the cluster's ranges.
  Stops steps 2 and 3 for tenants. Enforced in Scout, so not against a stolen
  token.
- ✅ **Registered accounts bound at admission (M-70):** the
  `bindy-scout-source-binding` policy refuses a registered account's
  create, update or delete of an ARecord outside its cluster names and zones,
  on the old and new object. Stops step 3 even with a stolen token. Verified
  on kind 1.36 (fixtures under `deploy/admission-policies/tests/`).
- ✅ **Zone label is authoritative (M-71):** a DNSZone takes a Scout record
  only when the record's zone label equals its `spec.zoneName`, so a selector
  that matches `managed-by: scout` alone cannot hand another cluster's
  records to the zone. Makes M-70's zone check the record's real zone.
- ✅ **Refused deletes are final (M-72):** Scout leaves another cluster's
  record when the API server refuses its delete, instead of retrying or
  holding a source's finalizer.
- ✅ Existing: the namespace grant (H1), host-in-zone check, and per-source
  record ownership (M-63).
- ⚠️ Two clusters registered for the same zone can still take over each
  other's names (roadmap 27 item 2), and unregistered accounts are not bound
  (accepted risk 23).
- ❌ Addresses are not checked in admission (accepted risk 23).

**Residual Risk:** LOW with every edge cluster's Scout on allowlists and its
account registered with the policy installed; MEDIUM for zones registered to
more than one cluster.

---

### R - Repudiation (Non-Repudiation)

#### R1: Unauthorized DNS Changes Without Attribution

**Threat:** Attacker modifies DNS zones and there's no audit trail proving who made the change.

**Impact:** HIGH (compliance violation, incident response hindered)
**Likelihood:** LOW (Kubernetes audit logs capture API calls)

**Attack Scenario:**
1. Attacker gains access to cluster with weak RBAC
2. Modifies DNSZone CRs
3. No log exists linking the change to a specific user or ServiceAccount

**Mitigations:**
- ✅ Kubernetes audit logs enabled (captures all API requests)
- ✅ All commits signed (non-repudiation for code changes)
- ✅ GitOps workflow (changes traceable to Git commits and PR reviews)
- ✅ **Secret operations attributable** (M-67, ADR-0024, 2026-10-14): every
  Secret write and key read bindy performs is one structured `bindy::audit`
  event naming the operation, outcome, purpose, reason and owning object,
  the *why* the API server log cannot record
- ❌ **MISSING**: Centralized log aggregation with tamper-proof storage (H-2)
- ❌ **MISSING**: Log retention policy (90 days active, 1 year archive per PCI-DSS)
- ❌ **MISSING**: Audit trail queries documented for compliance reviews

**Residual Risk:** MEDIUM (need H-2 - Audit Log Retention Policy)

---

#### R2: Secret Access Without Audit Trail

**Threat:** Attacker reads RNDC keys from Secrets, no record of who accessed them.

**Impact:** HIGH
**Likelihood:** LOW (secret access is logged by Kubernetes, and bindy's own access by its audit trail, M-67)

**Attack Scenario:**
1. Attacker compromises ServiceAccount with secret read access
2. Reads RNDC key from Kubernetes Secret
3. Uses key to control BIND9, but no clear audit trail of secret access

**Mitigations:**
- ✅ Kubernetes audit logs capture Secret read operations
- ✅ **Dedicated audit trail for bindy's Secret access** (M-67, ADR-0024,
  2026-10-14, H-3 done): one `bindy::audit` event per Secret create,
  replace, patch, delete or read of key material (operator, Scout,
  bootstrap), with operation, outcome, purpose, reason, actor and owner,
  never key material or an API error message; on by default even under
  `RUST_LOG=warn`. Join it to the API server audit log by namespace, name
  and time to tell bindy's reads (each with a recorded reason) from anyone
  else's
- ✅ **Alerting without parsing logs**: every event increments
  `bindy_firestoned_io_secret_operations_total{operation, purpose,
  outcome}`, so failed key loads or mirror writes can page
- ✅ **Fewer reads to audit** (M-68): metadata-only checks and a metadata
  Secret watch, so every recorded `read` is a real access to key material
- ❌ **MISSING**: Secret access dashboard for compliance reviews

**Residual Risk:** LOW (bindy's own access is recorded with its reason;
durability of the events depends on the log pipeline, accepted risk 22)

---

### I - Information Disclosure

#### I1: Exposure of RNDC Keys

**Threat:** RNDC keys leaked via logs, environment variables, or insecure storage.

**Impact:** CRITICAL
**Likelihood:** VERY LOW (secrets stored in Kubernetes Secrets, not in code)

**Attack Scenario:**
1. Developer hardcodes RNDC key in code or logs it for debugging
2. Key is committed to Git or appears in log aggregation system
3. Attacker finds key and uses it to control BIND9

**Mitigations:**
- ✅ Secrets stored in Kubernetes Secrets (encrypted at rest)
- ✅ Pre-commit hooks to detect secrets in code
- ✅ GitHub secret scanning enabled
- ✅ CI/CD fails if secrets detected
- ✅ Log sanitization: RNDC keys and bindcar bearer tokens are redacted in their
  `Debug` impls (`crates/bindy-bind9/src/bind9/types.rs`, `crates/bindy-bind9/src/bind9/mod.rs`), so a key cannot reach a log
  line through structured logging
- ✅ **Bounded in-memory key reuse** (M-46, ADR-0015, 2026-10-05): the
  operator keeps a loaded RNDC key in process memory for at most 60 s
  (`RNDC_KEY_CACHE_TTL`) instead of re-reading its Secret on every record
  write. The key was already in the same process for the duration of each
  write; the cache changes how often the Secret is read, not who can read it
  (no new RBAC, no Secret list or watch). An entry is dropped when the
  operator rotates the key and after any write with it fails, and the cached
  type keeps its redacting `Debug` impl
- ✅ **Only the mounted key is read** (M-58, 2026-10-11): the operator reads the
  RNDC Secret named by the instance Deployment's `rndc-key` volume. Since
  ADR-0023 that is always the operator-owned `{instance}-rndc-key`; a
  `rndcKey.secretRef` is mirrored into it (M-65), and a reference naming
  `{instance}-rndc-key` itself is refused, so a user Secret of that name is
  never overwritten
- ✅ **Mirrored keys follow their source and never leak through errors**
  (M-65, ADR-0023, 2026-10-14): a change to the referenced Secret rewrites
  the mirror, drops the cached key and rolls the pods, so the operator and
  the pods never hold different keys; no error message built from a source
  Secret carries the key, and the key type keeps its redacting `Debug`
- ✅ **Least-data Secret access** (M-68, ADR-0024, 2026-10-14): the instance
  controller's Secret watch is a metadata watch (`Api<PartialObjectMeta<Secret>>`),
  and existence, rotation and mirror-staleness checks read metadata only (the
  mirror records its source's `resourceVersion`), so the operator no longer
  holds the data of every Secret in its namespaces and reads a source's key
  only when it changed. Audit events carry no key material and summarise an
  API failure by status code and reason only (`kube_error_summary`), and the
  `secretRef` e2e asserts the key never appears in the operator log
- ⚠️ **PARTIAL**: RNDC key rotation is a **documented manual procedure**
  (`docs/src/security/incident-response.md`), not an automated policy. There is no
  scheduled/automatic rotation in the operator.

**Residual Risk:** LOW (good controls, but rotation would improve)

---

#### I2: Zone Data Enumeration

**Threat:** Attacker uses AXFR (zone transfer) to download entire zone contents.

**Impact:** MEDIUM (zone data is semi-public, but bulk enumeration aids reconnaissance)
**Likelihood:** MEDIUM (AXFR often left open by mistake)

**Attack Scenario:**
1. Attacker sends AXFR request to BIND9 server
2. If AXFR is not restricted, server returns all records in zone
3. Attacker uses zone data for targeted attacks (subdomain enumeration, email harvesting)

**Mitigations:**
- ✅ AXFR restricted to secondary servers only (BIND9 `allow-transfer` directive)
- ✅ **Deny-by-default at BOTH options levels** (ADR-0007, 2026-09-27): with no
  `allowTransfer` ACL configured anywhere, the generated options render
  `allow-transfer { none; };` in the instance-level AND cluster-level
  ConfigMaps. Previously the cluster-level builder emitted no directive, and
  BIND 9.18's own default allows AXFR to ANY host (upstream deny-by-default
  only landed in BIND 9.20, GL #3567); cluster-configured operands were
  enumerable
- ✅ BIND9 configuration managed by operator (prevents manual misconfig)
- ✅ **`allow-transfer` names exactly the zone's current secondary pods**
  (M-52, ADR-0019, 2026-10-07): before, the ACL kept the IP of every
  secondary pod the zone was created for, and a later pod that inherited a
  stale IP could AXFR the zone
- ❌ **MISSING**: TSIG authentication for zone transfers (H-4)
- ❌ **MISSING**: Rate limiting on AXFR requests

**Residual Risk:** MEDIUM → LOW-MEDIUM (open-by-default window closed; TSIG
for AXFR remains the outstanding hardening)

---

#### I3: Container Image Vulnerability Disclosure

**Threat:** Container images contain vulnerabilities that could be exploited if disclosed.

**Impact:** MEDIUM
**Likelihood:** MEDIUM (vulnerabilities exist in all software)

**Attack Scenario:**
1. Vulnerability is disclosed in a dependency (e.g., CVE in glibc)
2. Attacker scans for services using vulnerable version
3. Exploits vulnerability to gain RCE or escalate privileges

**Mitigations:**
- ✅ Automated vulnerability scanning (cargo-audit + Trivy) - C-3
- ✅ CI blocks on CRITICAL/HIGH vulnerabilities
- ✅ Daily scheduled scans detect new CVEs
- ✅ Remediation SLAs defined (CRITICAL: 24h, HIGH: 7d)
- ✅ Chainguard zero-CVE base images used

**Residual Risk:** LOW (strong vulnerability management)

---

#### I4: Scout Cluster-Wide Secret Read

**Status: ✅ FIXED 2026-07-19 (M-25).** Kept in full below as the historical record of
the finding and its fix; see the "Fix" subsection for current state.

**Threat (historical):** A compromised Scout pod, or anyone able to exec into it or
steal its ServiceAccount token, could read **every Secret in the cluster**, not
just the one kubeconfig Secret it legitimately needs for multi-cluster mode.

**Impact (historical):** CRITICAL
**Likelihood:** LOW (requires compromising the Scout pod/token specifically), but
**this had the largest blast radius of any threat in this document**, worse than
compromising the main operator, whose Secret access is namespace-scoped for
mutation (B-5) and, when `BINDY_WATCH_NAMESPACES` is set, for read as well (M-22).

**Original finding:** Scout's `ClusterRole` (`deploy/scout/clusterrole.yaml`) granted
`apiGroups: [""], resources: ["secrets"], verbs: ["get"]` with **no `resourceNames`
and no `Role`/namespace scoping**: a `ClusterRole` applies cluster-wide by
construction, so this let a compromised Scout token read RNDC keys, other teams'
database credentials, TLS private keys, CI/CD tokens: any Secret in any namespace
in the cluster. The code's own comment had documented the intended fix ("Scope this
to a Role in the specific Secret's namespace for production deployments") since
before this was reported, but it had not been implemented.

**Fix (2026-07-19):**
- ✅ The cluster-wide `secrets` `PolicyRule` was **removed entirely** from the
  `bindy-scout` `ClusterRole` (`build_scout_cluster_role` / `clusterrole.yaml`).
- ✅ Replaced with a **namespaced**, **`resourceNames`-restricted** Role
  (`bindy-scout-secrets-reader`) + RoleBinding, granting `get` on exactly the one
  configured Secret, not every Secret in the namespace, and not any Secret in any
  other namespace.
- ✅ This Role/RoleBinding is applied **only when Phase 2 (multi-cluster) mode is
  configured** (`--remote-secret` at bootstrap time, or manually via the new opt-in
  `deploy/scout/secrets-reader-rbac.yaml`, mirroring the existing
  `remote-cluster-rbac.yaml` pattern). Same-cluster-only deployments (the default)
  now get **zero** Secret access.
- ✅ Tests assert the ClusterRole has no `secrets` rule at all, and that the new
  Role's rule is `get`-only and `resourceNames`-restricted.
- ❌ **Still missing**: audit alerting on Scout's Secret reads (a defense-in-depth
  addition, not required now that the grant itself is minimal).

**Residual Risk:** **LOW** (down from HIGH). The blast radius of a Scout
compromise for Secret confidentiality is now bounded to the single Phase 2
kubeconfig Secret, in deployments that use Phase 2 mode at all.

---

#### I5: DNSSEC Private Key Disclosure

**Threat:** The zone's KSK/ZSK private keys leak, letting an attacker forge
validly signed answers for the zone (Spoofing as a consequence).

**Impact:** CRITICAL (for a zone whose DS is published at its parent)
**Likelihood:** LOW

**Attack Scenario:**
1. A tenant names another tenant's Secret, or an arbitrary Secret, as
   `keysFrom.secretRef`, to get it mounted into a pod they control the config of
2. Or an attacker with node access reads key files from the node's disk
3. Or an attacker who compromises `named` reads its key directory

**Mitigations:**
- ✅ H2 allow-list: the `secretRef` name must start with `bindy-`
  (`validate_dnssec_key_secret_name`, `crates/bindy-bind9/src/safe_volume.rs`), and
  a pod volume can only reference a Secret in its own namespace
- ✅ The Secret is mounted only into the `dnssec-keys-init` init container,
  never into `named` or the bindcar sidecar (ADR-0012,
  `crates/bindy-bind9/src/bind9_resources/`)
- ✅ The key directory is a `medium: Memory` `emptyDir`: key copies stay off
  the node's disk; copied files are `0600`, the Secret files `0440` to the
  bind group
- ✅ The operator never reads key material: it has no RBAC on the key Secret,
  and DS extraction reads only public DNSKEYs (M-14)
- ❌ `named` itself holds the keys; a `named` compromise discloses them. This
  is inherent to online signing (offline signing is out of scope)

**Residual Risk:** **LOW** with the controls above; a `named` compromise
remains the path, as it is for zone data.

---

### D - Denial of Service

#### D1: DNS Query Flood (DDoS)

**Threat:** Attacker floods BIND9 servers with DNS queries, exhausting resources.

**Impact:** CRITICAL (DNS unavailability impacts all services)
**Likelihood:** HIGH (DNS is a common DDoS target)

**Attack Scenario:**
1. Attacker uses botnet to send millions of DNS queries to BIND9 servers
2. BIND9 CPU/memory exhausted, becomes unresponsive
3. Legitimate DNS queries fail, causing outages

**Mitigations:**
- ✅ Rate limiting in BIND9 (`rate-limit` directive)
- ✅ Resource limits on BIND9 pods (CPU/memory requests/limits)
- ✅ Horizontal scaling (multiple BIND9 secondaries)
- ❌ **MISSING**: DDoS protection at network edge (e.g., CloudFlare, AWS Shield)
- ❌ **MISSING**: Query pattern analysis and anomaly detection
- ❌ **MISSING**: Automated pod scaling based on query load (HPA)

**Residual Risk:** MEDIUM (need edge DDoS protection)

---

#### D2: Operator Resource Exhaustion

**Threat:** Attacker creates thousands of DNSZone CRs, overwhelming operator.

**Impact:** HIGH (operator fails, DNS updates stop)
**Likelihood:** LOW (requires cluster access)

**Attack Scenario:**
1. Attacker gains write access to Kubernetes API
2. Creates 10,000+ DNSZone CRs
3. Operator reconciliation queue overwhelms CPU/memory
4. Operator crashes or becomes unresponsive

**Mitigations:**
- ✅ Resource limits on operator pod
- ✅ Exponential backoff for failed reconciliations (per-object, capped, with decay)
- ✅ **Client-side Kubernetes API rate limiting** (M-31, ADR-0005, 2026-09-27):
  the operator's client is capped at 20 QPS / 30 burst (tunable via
  `BINDY_KUBE_QPS`/`BINDY_KUBE_BURST`), so a CR flood cannot turn the operator
  into an API-server amplifier; excess requests queue client-side
- ✅ **Paginated LIST operations** (M-31): 100 items per page keeps operator
  memory O(1) in the number of CRs, bounding the "10,000 CRs exhaust memory"
  path of this scenario; a pagination anomaly (repeated token, empty page with
  a token, page cap) fails the LIST instead of returning a partial list (M-56)
- ✅ HTTP 429/retry visibility: `bindy_firestoned_io_kube_api_rate_limit_hits_total`
  and `..._kube_api_retries_total` alert before degradation cascades
- ✅ **No self-triggering, no out-of-band work** (M-41, ADR-0009 §4/§5,
  2026-10-05): the `DNSZone` controller ignores its own status writes, so a
  CR flood no longer multiplies into a reconcile storm; and a `DNSZone` event
  enqueues the instances it selected instead of spawning an unbounded task that
  fetched and patched each one outside the controller, so that work is
  deduplicated per object, backed off on failure and counted; and only when
  the zone's instance selection changes, so the timestamps every record
  reconcile writes into its zone cannot fan out into records x instances
  reconciles (measured on kind before the filter: 30 instance reconciles in
  330 s for 10 records and 3 instances)
- ✅ **Write cost independent of records x instances** (M-46, ADR-0015,
  2026-10-05): each reconcile resolves an instance's RNDC key and endpoints
  once (`InstanceResolver`); endpoints and instance roles come from the
  reflector stores; the record controller no longer read-modify-writes its
  zone's `status.records` (which raced, and woke every unreconciled record on
  each write) and ignores its own condition writes; a zone reconcile tags only
  untagged records, lists instead of GETting each record, and no longer GETs
  every record to log readiness. Measured on rc.2 before the change: 300
  records and 3 primaries drove 6,935 record reconciles and 25,582 API
  requests at the 20 QPS client limit
- ✅ **No periodic resync** (M-48, ADR-0016, 2026-10-06): every controller
  returns `await_change` on success and on a wait for another object, so at
  rest the operator makes no reconciles and no API calls beyond its watches
  (rc.3: 300 Ready records cost about one full reconcile, and one push to
  every primary, per second, forever). Each wait is ended by a named watch;
  the three that had none now have a pure mapper on a stream the operator
  already watched, filtered so ordinary status writes do not fan out
  (`changed_only` on the zone name for `DuplicateZone`; only not-Ready
  records for a zone's status change; only cluster-level ConfigMaps). A
  failure retries with the per-object capped backoff (2 s to 60 s, never
  sooner than the 30 s rejected-write cooldown for a record), and the only
  scheduled wakes (RNDC key rotation, KSK rollover) are capped at 30 days.
  A steady-state record reconcile makes no API read (it made 3), and a zone
  reconcile with 3 primaries reads instance roles, endpoints and keys from
  the stores and the 60 s key cache (it made about 25 GETs)
- ❌ **MISSING**: Global reconciliation-frequency limiter (M-3 layer 1:
  API traffic is now bounded, but reconcile CPU work per CR is not)
- ❌ **MISSING**: Admission webhook to limit number of CRs per namespace
- ❌ **MISSING**: Horizontal scaling of operator (leader election)

**Residual Risk:** MEDIUM → LOW-MEDIUM (API-server and memory amplification
closed by M-31; the per-object steady-state cost is zero since M-48; unbounded
CR count per namespace remains; revisit when a CR-quota admission policy
lands)

---

#### D3: AXFR Amplification Attack

**Threat:** Attacker abuses AXFR to amplify traffic in DDoS attack.

**Impact:** MEDIUM
**Likelihood:** LOW (AXFR restricted to secondaries)

**Attack Scenario:**
1. Attacker spoofs source IP of DDoS target
2. Sends AXFR request to BIND9
3. BIND9 sends large zone file to spoofed IP (amplification)

**Mitigations:**
- ✅ AXFR restricted to known secondary IPs (`allow-transfer`), kept to
  exactly the current secondary pods (M-52)
- ✅ BIND9 does not respond to spoofed source IPs (anti-spoofing)
- ❌ **MISSING**: Response rate limiting (RRL) for AXFR

**Residual Risk:** LOW (AXFR restrictions effective)

---

#### D4: Malformed Configuration Takes Every BIND9 Pod Down

**Threat:** A configuration `named` cannot parse reaches the ConfigMap that
every BIND9 pod of an instance or cluster mounts, through an operator bug or a
CRD value that rendering mishandles. Each pod that (re)starts on it exits at
startup, and the zones it serves go dark.

**Impact:** HIGH (every pod sharing the ConfigMap; a cluster ConfigMap is
shared by all of its instances)
**Likelihood:** LOW (it happened once: bug-177, a template substitution that
left a stray `}`)

**Mitigations:**
- ✅ CRD schema patterns and the `bind9_acl` validator constrain user values
  before they are rendered; the ACL admission policy (M-24) rejects bad ACL
  syntax at the API server
- ✅ **Rendered-configuration gate** (M-44, ADR-0013, 2026-10-05): CI parses
  every rendered `named.conf*` across the option matrix and the examples; at
  runtime the operator parses and validates it before writing the ConfigMap
  and, on failure, does not write it, reports `Ready=False` /
  `ConfigurationInvalid` and retries with backoff. The pods keep the last
  published configuration
- ✅ **Configuration written by construction** (M-47, ADR-0013 stage 3,
  2026-10-05): hornet 0.3.0's writer produces `named.conf` and
  `named.conf.options` from typed values, quoting and escaping each for its
  position; nothing reaches the file through a raw carrier, `logging` and
  `dnssec-policy` are fully modelled, and every rendered file of the option
  matrix and the examples passes `named-checkconf` on BIND 9.18 and 9.20. A
  key lifetime BIND rejects (`1y`) is refused at render

**Residual Risk:** LOW (an invalid render fails closed; a configuration
hornet accepts but `named` rejects is still possible, and would surface as
pods failing their rollout. The `named-checkconf` run is a manual check at
this release, not yet a CI gate)

---

#### D5: A Stalled API Server Connection Freezes Reconciles

**Threat:** A connection between the operator and the API server stops
delivering responses without being closed: a node network fault, a load
balancer or proxy in the path silently dropping the flow, or an attacker with
a foothold on that path black-holing traffic. kube-rs 4.2 sets no read
timeout by default and its connection timers are hundreds of seconds long,
so every non-watch request sent down that connection hangs and the
reconciles awaiting them stall with it. The API server sees nothing slow.

**Impact:** MEDIUM (DNS changes stop propagating for the duration of the
stall; DNS serving is unaffected)
**Likelihood:** MEDIUM (observed in the v0.8.0-rc.2 load test: five requests
took about 290 s each on one stalled connection, freezing five reconciles for
about five minutes)

**Mitigations:**
- ✅ **Per-request deadline on non-watch API requests** (M-45, ADR-0014,
  2026-10-05): a tower layer in the operator's client stack bounds each
  non-watch request to 30 s by default (`BINDY_KUBE_REQUEST_TIMEOUT_SECS`)
  across its response headers and body; the timeout surfaces as
  `kube::Error::Service`, which the retry helpers treat as transient, so the
  request is retried with backoff on a fresh connection and failing
  reconciles requeue under the per-object backoff
- ✅ Watches are exempt and are bounded instead by the server-side
  `timeoutSeconds` and kube-runtime's watcher idle timeout; a stalled watch
  is covered by the shared watch layer's restart and staleness metrics (M-38)
- ✅ Timed-out requests count as `status="error"` in
  `bindy_firestoned_io_kube_api_requests_total`, so a persistent stall is
  visible in Prometheus

**Residual Risk:** LOW (a stall costs at most one deadline per request, then
the backoff takes over; Scout's clients do not carry the deadline yet, see
accepted risk 12)

---

#### D6: A Rollout Takes a Zone's Nameservers Out of Service

**Threat (as first recorded, v1.17):** A replacement BIND9 pod (rollout, eviction, deletion,
rescheduling) starts with empty `emptyDir` zone storage. Its readiness used to
check only that `named` listened and bindcar answered, so it went Ready, its
Service routed to it, the old pod was terminated, and every zone answered
`REFUSED` until the zone controller noticed the `Endpoints` change and
replayed the zones. A change that rolls every primary at once (an operator
upgrade that changes the rendered configuration) empties every nameserver of
a zone at the same moment. An attacker who can trigger rollouts (edit an
instance's spec, evict pods) could turn that into repeated outages.

**Threat (broadened, v1.18):** Even with the gate, the handover itself
dropped traffic. The old pod stayed `Ready`, so its endpoint stayed
`serving`, from the moment `named` exited at the end of its preStop drain
until its readiness probe failed (up to 15 s). Behind a MetalLB layer-2
`LoadBalancer` with `externalTrafficPolicy: Local`, MetalLB kept announcing
the IP from the old node and kube-proxy there fell back to the dead,
serving-terminating local endpoint. And a configuration change rolled every
instance of a cluster at the same moment, so every nameserver of a zone took
that gap together: on 2026-10-07 (v0.8.0-rc.5) every query to both primaries
timed out for 9 to 12 s.

**Impact:** HIGH (every zone of the instance, every instance of a cluster on
a cluster-wide rollout)
**Likelihood:** HIGH before ADR-0017 (every rollout); MEDIUM between ADR-0017
and its amendment (every cluster-wide rollout behind `externalTrafficPolicy:
Local`); LOW after M-50 and M-51

**Mitigations:**
- ✅ **Zones-loaded readiness gate** (M-49, ADR-0017, 2026-10-06): every
  BIND9 pod template lists `bindy.firestoned.io/zones-loaded` in
  `readinessGates`; the operator loads every live zone that selects the pod's
  instance onto the pod (zone, NS and glue records, and on a primary every
  record tagged with the zone) through the same write paths, restricted to
  that pod, then sets the condition `True`. Until then Kubernetes keeps the
  pod out of its Service, and with the default rolling update of a
  one-replica Deployment (`maxUnavailable` 0) the old pod keeps serving
- ✅ Writes reach gated pods: record and zone writes target the Service's
  ready addresses plus the not-ready addresses whose pod is
  `ContainersReady=True`, so a record written during a rollout is not lost on
  the new pod
- ✅ No deadlock: a pod whose instance no live zone selects is admitted at
  once; a zone that fails on the pod blocks it only while another Ready pod of
  the instance serves that zone; the gate is a one-way latch, so a new zone
  never pulls a serving pod back out
- ✅ The gate is event-driven (Pod and `DNSZone` watches) with the per-object
  backoff on failure (M-48); no timer
- ✅ **Gate closed at the start of termination** (M-50, ADR-0017 decision
  6, 2026-10-07, corrected in v1.19): when a gated pod gets a
  `deletionTimestamp`, the operator sets its gate `False` (`PodTerminating`)
  in the same Pod event; a failed patch retries with backoff. The pod's
  `Ready`, and so its endpoint's `serving`, follow only on the kubelet's next
  status sync (about 18 s on rc.6), so this does not move traffic before
  `named` exits. The handover is kept short by the new pod being Ready
  before the old one is deleted (`maxUnavailable` 0 and the gate) and by
  M-51
- ✅ **Staggered rollouts** (M-51, ADR-0018, 2026-10-07): a pod-template
  change is applied only while no instance that serves a zone in common or
  belongs to the same cluster is mid-rollout (read from the Deployment and
  Pod stores); first come, first served, with claims against concurrent
  reconciles; a rollout past `progressDeadlineSeconds` stops blocking;
  woken by Deployment and Pod events, no timer. Creation, replica changes,
  RNDC key rotation and any other replacement of an instance's RNDC key (a
  mirrored `secretRef` key changing, ADR-0023) are not delayed: an instance
  whose pods hold a stale key cannot answer writes (accepted risk 21). The drift check compares owned
  fields semantically, and a template patch that bumps no generation is
  remembered as a known no-op, so a comparison miss cannot loop (ADR-0018
  decision 8, v1.19)

**Residual Risk:** LOW (fail-safe: an operator that is down or lacks the
`pods/status` grant stalls rollouts rather than serving empty zones, accepted
risk 15; a user-set `publishNotReadyAddresses: true` on the instance's
Service bypasses the gate; the operator's token can open the gate early,
accepted risk 14; with `externalTrafficPolicy: Local` each handover still
waits for MetalLB's re-announcement, and a stalled rollout delays its peers
up to `progressDeadlineSeconds`, accepted risk 16)

---

### E - Elevation of Privilege

#### E1: Container Escape to Node

**Threat:** Attacker escapes from Bindy or BIND9 container to underlying Kubernetes node.

**Impact:** CRITICAL (full node compromise, lateral movement)
**Likelihood:** VERY LOW (Pod Security Standards enforced)

**Attack Scenario:**
1. Attacker exploits container runtime vulnerability (e.g., runc CVE)
2. Escapes container to host filesystem
3. Gains root access on node, compromises kubelet and other pods

**Mitigations:**
- ✅ Non-root containers (uid 1000+)
- ✅ Read-only root filesystem
- ✅ No privileged capabilities: the BIND9 operand previously required
  `NET_BIND_SERVICE` to bind privileged port 53; it now binds the **unprivileged
  container port 5353** (Service still exposes 53 to clients) and adds **zero**
  Linux capabilities back after `drop: ["ALL"]`. This closes the one capability
  the operand used to carry.
- ✅ Pod Security Standards (Restricted)
- ✅ seccomp profile (restrict syscalls)
- ✅ AppArmor/SELinux profiles
- ❌ **MISSING**: Regular node patching (managed by platform team)

**Residual Risk:** VERY LOW (defense in depth)

---

#### E2: RBAC Privilege Escalation

**Threat:** Attacker escalates from limited RBAC role to cluster-admin.

**Impact:** CRITICAL
**Likelihood:** VERY LOW (RBAC reviewed, least privilege enforced)

**Attack Scenario:**
1. Attacker compromises ServiceAccount with limited permissions
2. Exploits RBAC misconfiguration (e.g., wildcard permissions)
3. Gains cluster-admin and full control of cluster

**Mitigations:**
- ✅ RBAC least privilege (operator has NO delete permissions) - C-2
- ✅ **B-5 hardening:** operator's cluster-wide Secret access reduced to
  read-only; mutating verbs confined to a namespaced Role in the operator's own
  namespace (see T3)
- ✅ **Namespace-scoped operator mode (opt-in):** set `BINDY_WATCH_NAMESPACES` to a
  comma-separated namespace list and the operator builds one watch and one controller
  per namespace (`Api::namespaced`), needing only a Role + RoleBinding in each. With
  the cluster-wide ClusterRoleBinding removed, the operator SA can no longer read
  Secrets or create workloads outside the watched set, closing C2 and H3 by
  construction rather than by compensating control. VAP 11/12 remains as
  defence-in-depth, and is still the only mitigation in the default cluster-wide
  deployment. **Default is unchanged:** unset means cluster-wide, exactly as before.
  Install guide: `deploy/operator/rbac/namespaced/README.md`
  (unset = watch everything), so this mitigation is not yet load-bearing unless
  a deployer explicitly configures it.
- ✅ Automated RBAC verification script (`deploy/rbac/verify-rbac.sh`)
- ✅ **`pods/status` grant bounded** (ADR-0017, 2026-10-06): the zones-loaded
  gate needs `get`/`patch` on `pods/status` only; `pods` stays
  `get`/`list`/`watch`. `bootstrap_tests.rs` fails if either operator role
  grants more than `get`/`patch` on `pods/status` or any write on `pods`, and
  `verify-rbac.sh` checks both. A status patch cannot change what a pod runs,
  its ServiceAccount or its labels, so it is not an escalation path
- ✅ **Scout RBAC drift test** (M-42, 2026-10-05): `cargo test -p bindy-bootstrap
  rbac_drift` fails when `deploy/scout/*.yaml` or the examples in
  `docs/src/guide/scout.md` differ from the RBAC `bindy bootstrap scout` builds,
  so a grant cannot be widened in one representation and not reviewed in the others
- ✅ **RNDC `secretRef` allow-list** (M-66, ADR-0023, 2026-10-14): the
  operator copies two entries of the Secret a `rndcKey.secretRef` (or the
  deprecated `rndcSecretRef`) names into the Secret the operand pods mount.
  Without a name rule, anyone who can create a `Bind9Instance`,
  `Bind9Cluster` or `ClusterBind9Provider` could use the operator's
  cluster-wide Secret read as a confused deputy and get another workload's
  Secret into a pod; before ADR-0023 the pod mounted every entry of such a
  Secret. The name must start with `bindy-`, as for DNSSEC keys (H2), checked
  by the `bindy-pod-shape-validation` admission policy and by the reconciler
  (`validate_rndc_key_secret_name`). No RBAC grant changed
- ✅ No wildcard permissions in operator RBAC
- ✅ Regular RBAC audits (quarterly)
- ⚠️ **Scout is a separate, less-reviewed RBAC surface**; see T4/I4/E4 and
  [Trust Boundary 6](#boundary-6-scout-controller). The verification script and
  quarterly audits should be confirmed to cover `deploy/scout/clusterrole.yaml`,
  not just the main operator's RBAC.
- ❌ **MISSING**: RBAC policy-as-code validation (OPA/Gatekeeper)

**Residual Risk:** VERY LOW for the main operator (strong RBAC controls); see E4
for Scout, which is **not** covered by this residual-risk rating.

---

#### E3: Exploiting Vulnerable Dependencies

**Threat:** Attacker exploits vulnerability in Rust dependency to gain code execution.

**Impact:** HIGH
**Likelihood:** LOW (automated vulnerability scanning, rapid patching)

**Attack Scenario:**
1. CVE disclosed in dependency (e.g., `tokio`, `hyper`, `kube`)
2. Attacker crafts malicious Kubernetes API response to trigger vulnerability
3. Operator crashes or attacker gains RCE in operator pod

**Mitigations:**
- ✅ Automated vulnerability scanning (cargo-audit) - C-3
- ✅ CI blocks on CRITICAL/HIGH vulnerabilities
- ✅ Remediation SLAs enforced (CRITICAL: 24h)
- ✅ Daily scheduled scans
- ✅ Dependency updates via Dependabot
- ✅ **New runtime dependency `hornet-bind9`** (ADR-0013, 2026-10-05): same
  organisation, Apache-2.0, `unsafe` forbidden, built with
  `default-features = false`; it parses only configuration text the operator
  generated, never network input. Its `miette` diagnostics stack adds
  transitive crates (cargo-deny clean, listed in the SBOM)
- ⚠️ **Dependabot auto-merge is now automated** (`dependabot-auto-merge.yaml`):
  patch/minor updates are merged automatically once the full e2e gate (integration
  + regression suites) and all required status checks pass, with no human review
  step. Major version bumps are still held open for manual review. This trades a
  manual-review control for a broader-but-automated test gate. The risk this
  accepts is that the e2e suite may not exercise every code path a malicious or
  broken dependency update could affect. Partially offset by: signed-commit
  verification remains a required branch-protection check (not skipped), and the
  gate that verifies the *PR opener* is genuinely `dependabot[bot]`
  (`pull_request.user.login`) rather than the more easily-spoofed `github.actor`.

**Residual Risk:** LOW-MEDIUM (excellent vulnerability *scanning*, but the new
auto-merge automation removes a human checkpoint from the merge path for
patch/minor dependency updates, worth an explicit accept/revisit decision by
Security Team, not just an implicit one)

---

#### E4: Scout ServiceAccount Compromise → Cluster-Wide Pivot

**Status: ✅ FIXED 2026-07-19 (M-25).** This threat's root cause was the same
unscoped Secret read documented under I4; kept in full below as the historical
record.

**Threat (historical):** An attacker who compromises the Scout pod or steals its
ServiceAccount token uses the cluster-wide Secret read (I4) to pivot into other
workloads' trust domains, e.g., reading another team's database credentials or a
CI/CD token stored as a Secret, then using *those* credentials to escalate further.

**Impact (historical):** CRITICAL
**Likelihood:** LOW (requires an initial compromise of Scout specifically)

**Attack Scenario (historical):**
1. Attacker achieves code execution in the Scout pod (dependency CVE, container
   escape, or a stolen ServiceAccount token from a compromised node)
2. Uses Scout's cluster-wide `secrets:get` (I4) to enumerate Secrets across
   namespaces the attacker has no other access to
3. Finds and uses a higher-privilege credential (e.g., another operator's
   ServiceAccount token stored as a Secret, a cloud-provider credential, a CI/CD
   deploy key) to escalate beyond what Scout's own RBAC would allow

**Mitigations:**
- ✅ Scout has no `create`/`delete` on Secrets: read-only
- ✅ Pod Security Standards applied to the Scout Deployment (same as main operator)
- ✅ **I4 fixed (M-25)**: Scout's Secret RBAC is now namespaced and
  `resourceNames`-restricted to the single Phase 2 kubeconfig Secret. Step 2 above
  ("enumerate Secrets across namespaces") is no longer possible; the RBAC to do so
  doesn't exist.
- ❌ **MISSING**: Network policy restricting Scout's egress (it does not need to
  reach most in-cluster services directly, only the Kubernetes API and, in Phase
  2 mode, a remote cluster's API), M-27

**Residual Risk:** **LOW** (down from HIGH). The root-cause Secret read is closed;
egress restriction (M-27) remains a defense-in-depth item, not a live path to this
scenario.

---

## Attack Surface

### 1. Kubernetes API

**Exposure:** Internal (within cluster)
**Authentication:** ServiceAccount token (JWT)
**Authorization:** RBAC (least privilege)

**Attack Vectors:**
- Token theft from compromised pod
- RBAC misconfiguration allowing excessive permissions
- API server vulnerability (CVE in Kubernetes)

**Mitigations:**
- Short-lived tokens (TokenRequest API)
- RBAC verification script
- Regular Kubernetes upgrades
- One shared watch per kind and namespace target (ADR-0009): 19 operator watch
  connections in cluster-wide mode instead of 58, client-side rate limited (M-31)
- Every non-watch request bounded by a client-side deadline (M-45, ADR-0014),
  so a stalled connection cannot hold reconciles for minutes
- No periodic resync (M-48, ADR-0016): request volume follows change, not
  object count
- New write path (ADR-0017): `patch` on `pods/status` for the zones-loaded
  gate, one condition per BIND9 pod, written only when it changes; one more
  shared watch (bindy's BIND9 pods, label-selected on the API server)
- Secret requests (ADR-0024): every Secret operation goes through the
  audited `SecretApi` (M-67); the Secret watch and the per-reconcile checks
  are metadata-only requests (M-68)

**Risk:** MEDIUM

---

### 2. DNS Port 53 (UDP/TCP)

**Exposure:** External (internet-facing): the Kubernetes `Service` exposes the
standard port 53 and forwards to the BIND9 container's **unprivileged port 5353**
(`named` no longer binds a privileged port and carries no `NET_BIND_SERVICE`
capability). This is an internal implementation detail; the client-facing exposure
and risk profile below are unchanged.
**Authentication:** None (public DNS)
**Authorization:** None

**Attack Vectors:**
- DNS amplification attacks
- Query floods (DDoS)
- Cache poisoning attempts (if recursion enabled)
- NXDOMAIN attacks

**Mitigations:**
- Rate limiting (BIND9 `rate-limit`)
- Recursion disabled (authoritative-only)
- DNSSEC signing (opt-in, M-14)
- DDoS protection at edge
- Operand runs with zero added Linux capabilities (see E1)

**Risk:** HIGH (public-facing, no authentication)

---

### 3. RNDC Port 9530

**Exposure:** Internal (within cluster, not exposed externally)
**Authentication:** HMAC key (symmetric)
**Authorization:** Key-based (all-or-nothing)

**Attack Vectors:**
- RNDC key theft from Kubernetes Secret
- Brute-force HMAC key (unlikely with strong key)
- MITM attack (if network not encrypted)

**Mitigations:**
- Secrets encrypted at rest
- RBAC limits secret read access; the operator reads only the RNDC Secret the instance's pods mount (M-58), which is always the operator-owned `{instance}-rndc-key`; a `secretRef` source must be a `bindy-` Secret and is mirrored, never mounted (M-65, M-66)
- RNDC port not exposed externally
- NetworkPolicy (planned - L-1)
- A change made with a stolen key is no longer reverted on a timer (ADR-0016,
  accepted risk 13); it is visible in BIND9's own logs and query answers, and
  annotating the owning resource restores it

**Risk:** MEDIUM

---

### 4. Container Images (Supply Chain)

**Exposure:** Public (GitHub Container Registry)
**Authentication:** Pull is unauthenticated (public repo)
**Authorization:** Push requires GitHub token with packages:write

**Attack Vectors:**
- Compromised CI/CD pipeline pushing malicious image
- Dependency confusion (malicious crate with same name)
- Compromised base image (upstream supply chain attack)

**Mitigations:**
- Signed commits (all code changes)
- Release images Cosign-signed with SLSA Build L3 provenance (M-33)
- Signed SBOM attestation per image, bound to its digest (M-34)
- Base images pinned by multi-arch digest; release manifests pin the operator image by digest
- Vulnerability scanning (Trivy)
- Chainguard zero-CVE base images
- Dependabot for dependency updates

**Risk:** LOW (strong supply chain security)

---

### 5. Custom Resource Definitions (CRDs)

**Exposure:** Internal (Kubernetes API)
**Authentication:** Kubernetes user/ServiceAccount
**Authorization:** RBAC (namespace-scoped for DNSZone)

**Attack Vectors:**
- Malicious CRs with crafted input (e.g., XXL zone names)
- Schema validation bypass
- CR injection via compromised user

**Mitigations:**
- Schema validation in CRD (OpenAPI v3)
- Input sanitization in operator
- Namespace isolation (RBAC)
- `ValidatingAdmissionPolicy` suite (M-24); among its rules, a Secret a CR
  names for the operand pod (DNSSEC `keysFrom`, RNDC `secretRef`) must start
  with `bindy-` (H2, M-66)
- Admission webhooks (planned)

**Risk:** MEDIUM

---

### 6. Git Repository (Code)

**Exposure:** Public (GitHub)
**Authentication:** Push requires GitHub 2FA + signed commits
**Authorization:** Branch protection on `main`

**Attack Vectors:**
- Compromised GitHub account
- Unsigned commit merged to main
- Malicious PR approved by reviewers

**Mitigations:**
- All commits signed (GPG/SSH) - C-1
- Branch protection (2+ reviewers required)
- CI/CD verifies signatures
- Linear history (no merge commits)

**Risk:** VERY LOW (strong controls)

---

### 7. Scout Controller (Cluster-Wide RBAC)

**Exposure:** Internal (Kubernetes API), but with **cluster-wide** scope: every
namespace, not just `bindy-system`
**Authentication:** ServiceAccount token (JWT), same mechanism as the main operator
**Authorization:** `ClusterRole` `bindy-scout`: `get`/`list`/`watch`/`patch`/`update`
on `Ingress`/`Service`/`HTTPRoute`/`TLSRoute`/`TCPRoute` cluster-wide. **No Secret
access at all** on the ClusterRole; see below.

**Attack Vectors:**
- Token theft from a compromised Scout pod (as with the main operator's Attack
  Surface #1, but the resulting read/write scope is cluster-wide by design here)
- Theft of the remote token Scout writes to the bindy cluster with: bounded to
  its cluster names and zones once registered (M-70, T5)
- A tenant annotation naming another cluster's zone or an outside address
  (T5): refused by M-69 when the allowlists are set
- Any tenant setting the `bindy.firestoned.io/scout-enabled` annotation on their own
  resource can cause Scout to write an `ARecord`; this is expected/intended
  behavior, not a vulnerability, but means Scout's write path is reachable by any
  namespace user, not just admins
- ~~The unscoped `secrets: get`~~: **fixed 2026-07-19 (M-25)**. Secret access is now
  a namespaced, `resourceNames`-restricted Role
  (`deploy/scout/secrets-reader-rbac.yaml`), applied only in deployments using Phase
  2 (multi-cluster) mode. See [I4](#i4-scout-cluster-wide-secret-read).

**Mitigations:**
- Same pod-hardening posture as the main operator (non-root, read-only rootfs, seccomp)
- Zone-authorization check gates `ARecord` creation (same-namespace or explicit
  allow-list, preventing cross-tenant DNS hijack via Scout)
- ✅ Zone and address allowlists per edge cluster (M-69), registered accounts
  bound to their cluster names and zones at admission on the bindy cluster
  (M-70), zone label authoritative for Scout records (M-71); see T5
- ✅ The `bindy.firestoned.io/record-name` override is validated (P2-7) for every
  source kind; the TCPRoute path skipped it until 2026-10-11 (M-59)
- ✅ Secret access scoped to a namespaced, `resourceNames`-restricted Role (M-25, fixed)
- ✅ Namespace whitelisting available (`--namespace-selector`, M-30, opt-in) to bound
  which namespaces Scout's patch/update reach extends to
- ❌ **MISSING**: Field-level admission constraining *what* Scout can patch on
  Ingress/Service/route objects (M-28)

**Risk:** MEDIUM (down from HIGH). The cluster-wide `patch`/`update` on
Ingress/Service/route types (T4) is the remaining open item for this component;
see T4, [Trust Boundary 6](#boundary-6-scout-controller))

---

## Threat Scenarios

### Scenario 1: Compromised Operator Pod

**Severity:** HIGH

**Attack Path:**
1. Attacker exploits vulnerability in operator code (e.g., memory corruption, logic bug)
2. Gains code execution in operator pod
3. Reads ServiceAccount token from `/var/run/secrets/`
4. Uses token to modify DNSZone CRs or read RNDC keys from Secrets

**Impact:**
- Attacker can modify DNS records (redirect traffic)
- Attacker can disrupt DNS service (delete zones, BIND9 pods)
- Attacker can admit a new BIND9 pod before its zones are loaded, or hold new
  pods out of their Service (`pods/status` patch, ADR-0017); serving pods are
  not affected until they are replaced. Setting a serving pod's gate `False`
  takes it out of its Service too; the operator itself does this only for a
  terminating pod (ADR-0017 decision 6), and the same token can already edit
  or delete the instance's Deployment
- Attacker can roll every instance of a cluster at once by patching their
  Deployments directly: the staggering of ADR-0018 orders the operator's own
  changes, it is not an authorization control
- Attacker can pivot to other namespaces (if RBAC is weak)

**Mitigations:**
- Operator runs as non-root, read-only filesystem
- RBAC least privilege (no delete permissions)
- Resource limits prevent resource exhaustion
- Vulnerability scanning (cargo-audit, Trivy)
- The process holds only the keys it loaded (M-68), and every Secret
  operation it makes through bindy's code is an audit event (M-67); a token
  used directly with `kubectl` shows in the API server audit log with no
  matching bindy event, which is the signal to look for
- Network policies (planned - L-1)

**Residual Risk:** MEDIUM (need network policies)

---

### Scenario 2: DNS Cache Poisoning

**Severity:** MEDIUM

**Attack Path:**
1. Attacker sends forged DNS responses to recursive resolver
2. Resolver caches malicious record (e.g., A record for bank.com pointing to attacker IP)
3. Clients query resolver, receive poisoned response
4. Traffic redirected to attacker (phishing, MITM)

**Impact:**
- Users redirected to malicious sites
- Credentials stolen
- Man-in-the-middle attacks

**Mitigations:**
- DNSSEC signing (opt-in, M-14, ADR-0006) - cryptographically signs DNS responses;
  DS records surfaced in `DNSZone.status.dnssec` for parent-zone publication
- BIND9 is authoritative-only by default (`recursion` is off unless the CRD
  enables it); when recursion is enabled, `validation: true` renders
  `dnssec-validation auto` (built-in root trust anchor). Before ADR-0013
  stage 3 it rendered `yes` without `trust-anchors`, which validated nothing
  on BIND 9.18
- Recursive resolvers outside our control (client responsibility)

**Residual Risk:** LOW for signed zones with DS published; MEDIUM otherwise
(signing is opt-in per cluster/zone)

---

### Scenario 3: Supply Chain Attack via Malicious Dependency

**Severity:** CRITICAL

**Attack Path:**
1. Attacker compromises popular Rust crate (e.g., via compromised maintainer account)
2. Malicious code injected into crate update
3. Bindy operator depends on compromised crate
4. Malicious code runs in operator, exfiltrates secrets or modifies DNS zones

**Impact:**
- Complete compromise of DNS infrastructure
- Data exfiltration (secrets, zone data)
- Backdoor access to cluster

**Mitigations:**
- Dependency scanning (cargo-audit) - C-3; `cargo-deny` source and ban policy
- Per-binary SBOM listing the full Rust dependency tree, signed and bound to the tarball (M-34)
- Signed commits (code changes traceable)
- Dependency version pinning in `Cargo.lock`, enforced in release builds with `--locked`
- Manual review for major dependency updates (not enforced by the rulesets; see S3)

**Residual Risk:** LOW (strong supply chain controls)

---

### Scenario 4: Insider Threat (Malicious Admin)

**Severity:** HIGH

**Attack Path:**
1. Malicious cluster admin with `cluster-admin` RBAC role
2. Directly modifies DNSZone CRs to redirect traffic
3. Deletes audit logs to cover tracks
4. Exfiltrates RNDC keys from Secrets

**Impact:**
- DNS records modified without attribution
- Service disruption
- Data theft

**Mitigations:**
- GitOps workflow (changes via PRs, not direct kubectl)
- All changes require 2+ reviewers
- Immutable audit logs (planned - H-2)
- Secret access audit trail for bindy's own access (M-67, H-3 done): an
  admin's direct Secret reads appear in the API server audit log with no
  matching `bindy::audit` event; disabling bindy's events needs a visible
  `bindy::audit=off` in the Deployment spec (accepted risk 22)
- Separation of duties (no single admin has all access)

**Residual Risk:** MEDIUM (need H-2)

---

### Scenario 5: DDoS Attack on DNS Infrastructure

**Severity:** CRITICAL

**Attack Path:**
1. Attacker launches volumetric DDoS attack (millions of queries/sec)
2. BIND9 pods overwhelmed, become unresponsive
3. DNS queries fail, causing outages for all dependent services

**Impact:**
- Complete DNS outage
- All services depending on DNS become unavailable
- Revenue loss, SLA violations

**Mitigations:**
- Rate limiting in BIND9
- Horizontal scaling (multiple secondaries)
- Resource limits (prevent total resource exhaustion)
- DDoS protection at edge (planned - CloudFlare, AWS Shield)
- Autoscaling (planned - HPA based on query load)

**Residual Risk:** MEDIUM (need edge DDoS protection)

---

### Scenario 6: Compromised Scout Pod

**Severity:** CRITICAL (historical) → **MEDIUM (current, post-M-25)**

**Status: steps 2–3 below (the Secret-exfiltration path) were closed 2026-07-19
(M-25).** Step 4 (cross-tenant patch/update tampering) remains a live, open,
MEDIUM-severity path, kept as the current scope of this scenario.

**Attack Path (historical, steps 2–3 no longer possible):**
1. Attacker exploits a vulnerability in Scout (dependency CVE, container escape) or
   steals its ServiceAccount token from a compromised node
2. ~~Scout's `ClusterRole` grants unscoped `secrets: get` across every namespace in
   the cluster~~: **this RBAC rule no longer exists.** Scout's Secret access is now
   a namespaced, `resourceNames`-restricted Role scoped to the single Phase 2
   kubeconfig Secret (or no Secret access at all, in same-cluster-only deployments).
3. ~~Attacker uses a harvested credential to pivot~~: no longer reachable; there is
   nothing to harvest beyond the one Secret Scout legitimately needs, if even that.
4. **(Still live)** Scout's cluster-wide `patch`/`update` on `Ingress`/`Service`/route
   objects could be used to tamper with another tenant's networking configuration;
   see [T4](#t4-cross-tenant-tampering-via-scouts-cluster-wide-write-rbac).

**Impact (current scope, step 4 only):**
- Cross-tenant tampering with Ingress/Service/route objects in namespaces Scout has
  no legitimate reason to touch that day
- No confidentiality impact: Scout can no longer read Secrets beyond its own narrow need

**Mitigations:**
- Same pod-hardening posture as the main operator (non-root, read-only rootfs, seccomp)
- Zone-authorization check limits what DNS records Scout can actually create
- ✅ **Records bound to the cluster and its zones (M-69 to M-71):** a
  compromised Scout with a registered remote token cannot publish into,
  rewrite or delete another cluster's records on the bindy cluster (T5)
- ✅ **Secret RBAC scoped (M-25, fixed 2026-07-19):** closes the confidentiality half
  of this scenario entirely (formerly steps 2–3 above).
- ✅ **Namespace whitelisting (M-30, opt-in):** when `--namespace-selector` is
  configured, Scout's own reconcile logic only acts on objects in labeled
  namespaces, bounding step 4 during *normal operation*. Enforced by Scout's
  reconciler code, not RBAC, so a directly-held stolen token is unaffected by this
  control alone.
- ❌ **MISSING**: Field-level admission constraining what Scout may patch (M-28)
- ❌ **MISSING**: Egress NetworkPolicy limiting Scout to the Kubernetes API only (M-27)

**Residual Risk:** **MEDIUM** (down from CRITICAL). The confidentiality path is
closed; the remaining risk is bounded to cross-tenant `Ingress`/`Service`/route
tampering (T4), not cluster-wide Secret exposure.

---

## Mitigations

### Existing Mitigations (Implemented)

| ID | Mitigation | Threats Mitigated | Compliance |
|----|------------|-------------------|------------|
| M-01 | Signed commits required | S3 (spoofed commits) | ✅ C-1 |
| M-02 | RBAC least privilege | E2 (privilege escalation) | ✅ C-2 |
| M-03 | Vulnerability scanning | I3 (CVE disclosure), E3 (dependency exploit) | ✅ C-3 |
| M-04 | Non-root containers | E1 (container escape) | ✅ Pod Security |
| M-05 | Read-only filesystem | T2 (tampering), E1 (escape) | ✅ Pod Security |
| M-06 | Secrets encrypted at rest | I1 (RNDC key disclosure) | ✅ Kubernetes |
| M-07 | AXFR restricted to secondaries | I2 (zone enumeration) | ✅ BIND9 config |
| M-08 | Rate limiting (BIND9) | D1 (DNS query flood) | ✅ BIND9 config |
| M-09 | **SBOM generation** (corrected 2026-10-03): CycloneDX SBOM for every release binary (5 platforms, cargo-cyclonedx, spec 1.5) and image (Syft, by digest). Before ADR-0010 the binary SBOMs never reached a release | T2 (supply chain), Scenario 3 | ✅ `build.yaml` `sbom` job, `docker-release` |
| M-33 | **SLSA v1.0 Build L3 provenance** (2026-10-03, ADR-0010): `slsa-github-generator` generic generator over every release tarball, install manifest and SBOM, and its container generator for each release image (pushed to GHCR). Provenance is generated and signed outside the build jobs | T2, Scenario 3 (forged or substituted artifacts) | ✅ `build.yaml` `slsa-provenance`, `slsa-image-provenance`; `make verify-provenance`, `make verify-image-provenance` |
| M-34 | **SBOM quality gate and attestation** (2026-10-03, ADR-0010): `scripts/sbom.sh check` fails the build unless an SBOM meets the NTIA minimum elements; each SBOM is a Sigstore-signed `actions/attest-sbom` attestation bound to its tarball or image digest | T2 (SBOM swapped or edited), Scenario 3 | ✅ `make sbom-check`, `make verify-sbom-attestation` |
| M-35 | **Anchored signer identity** (2026-10-03): verification targets and docs accept only `build.yaml@refs/tags/*` (and `release.yaml` for releases before v0.6.0, `rebuild-release-images.yaml@refs/heads/main` for rebuilt images), not a `https://github.com/firestoned/bindy` prefix that a lookalike repository would also match | T2 (spoofed signer) | ✅ `Makefile` `SIGNER_IDENTITY_REGEXP` |
| M-39 | **Cryptographic inventory (CBOM) per release** (2026-10-04, ADR-0011): a curated CycloneDX 1.6 CBOM declares every algorithm bindy ships, configures or depends on, with its quantum exposure. `scripts/cbom.sh` stamps crypto-library versions from `Cargo.lock` (a dropped or renamed crypto dependency fails the build) and gates the document on every PR; it ships as a release asset covered by the SLSA provenance subjects | T2 (silent crypto dependency drift); crypto-agility evidence for the quantum transition (roadmap 28) | ✅ `make cbom-stage`, `build.yaml` `cbom` job required by `ci-gate` |
| M-10 | Chainguard zero-CVE images | I3 (CVE disclosure) | ✅ Container security |
| M-21 | **B-5 Secret RBAC split** (2026-06-30): operator's cluster-wide `ClusterRole` is read-only on Secrets; mutating verbs moved to a namespaced Role bound only in the operator's own namespace | T3 (Secret tampering), E2 (privilege escalation) | ✅ RBAC |
| M-22 | **Namespace-scoped operator mode** (opt-in via `BINDY_WATCH_NAMESPACES`): every watch is built per-namespace and the operator needs only Role/RoleBinding in each watched namespace | E2, R2, I1 | ✅ **Implemented** (opt-in; default remains cluster-wide). Eliminates cluster-wide Secret read (H3) and cluster-wide workload write (C2), verified with `kubectl auth can-i`. A slim ClusterRole remains for `clusterbind9providers`, the only cluster-scoped bindy kind, so this does **not** eliminate cluster-wide access *entirely*. See `deploy/operator/rbac/namespaced/README.md` |
| M-23 | **Unprivileged DNS port + capability drop**: BIND9 operand binds container port 5353 (Service still exposes 53) and adds zero Linux capabilities (`NET_BIND_SERVICE` removed) | E1 (container escape) | ✅ Pod Security |
| M-24 | **ValidatingAdmissionPolicy suite** (8 policies + 8 bindings = 16 manifests, as of 2026-07-01): ACL syntax, zone-name validation, RNDC strictness, operand pod shape, DNSSEC policy, operator-workload ServiceAccount identity, DNS record value validation, image provenance, and `volumeMount.mountPath` allow-listing (`safe_volume.rs`, closes audit finding F-001) | T1 (DNS tampering), T3 (ConfigMap/Secret tampering), E1 (container escape via malicious volume mounts), T2 (image provenance) | ✅ Kubernetes VAP, which supersedes M-13 below |
| M-30 | **Scout namespace whitelisting** (`--namespace-selector` / `BINDY_SCOUT_NAMESPACE_SELECTOR`, new in v1.1): a source object's namespace must match a configured Kubernetes label selector *in addition to* the object's own opt-in annotation before Scout acts on it. Label-selector matching delegates to the API server (no client-side selector parser). Reduces Scout's day-to-day operating footprint, bounding T4 (cross-tenant patch/update) during normal operation. **Does not reduce the `ClusterRole`'s RBAC ceiling** for Ingress/Service/route types; a directly compromised token is unaffected for those. **Opt-in, unset by default**, matching pre-v1.1 behavior for backward compatibility; Scout logs a startup warning when unset. See the Scout guide's "Namespace Whitelisting" section for the rollout/migration note. | T4 (partial) | ⚠️ Opt-in, recommended for all production deployments |
| M-32 | **Scout endpoint + token-file remote transport** (2026-09-28, ADR-0008): alternative to the kubeconfig-Secret mode; bare bindy-minted token file (rotatable, no restart), endpoint/CA in the Deployment spec, fail-closed mode selection. Removes the kubeconfig blob and the `secrets: get` dependency in this mode; supports Linkerd-meshed proxy mirrors for cross-cluster mTLS | I4/E4 (smaller credential surface), T4 (unchanged ceiling) | ✅ `crates/bindy-scout/src/scout.rs` (`resolve_remote_transport`) |
| M-31 | **Client-side Kubernetes API rate limiting** (2026-09-27, ADR-0005): tower `RateLimitLayer` in the operator's client stack (20 QPS / 30 burst default, env-tunable, invalid overrides fall back safely), paginated LISTs (O(1) memory), exponential-backoff retries on transient 429/5xx, and Prometheus visibility of server-side throttling (`kube_api_*` metrics) | D2 (reconciliation flood; API/memory amplification), platform availability (Basel III operational resilience) | ✅ `crates/bindy-controller-sdk/src/{rate_limit,pagination,retry}.rs` |
| M-38 | **Shared watch supervision** (2026-10-04, ADR-0009 §3): every cached kind is watched once per namespace target by the SDK `WatchSet`; a stream that ends is restarted with backoff, and `bindy_firestoned_io_watch_{events,errors,restarts}_total` / `_watch_last_event_timestamp_seconds` expose each watch's health and staleness. Before, a dead reflector task ended silently. Fan-out applies backpressure instead of dropping events, so a reconcile is never silently skipped | D2 (stale cache acting on old state), availability | ✅ `crates/bindy-controller-sdk/src/watch.rs` |
| M-41 | **No self-triggering, pure watch mappers** (2026-10-05, ADR-0009 §4/§5): the `DNSZone` primary stream passes only generation, finalizer, label and annotation changes (`sdk::watch::primary_predicate`), so the controller's own status writes do not retrigger it and the 2-second timestamp rate limiter is gone; every watch mapper returns object references and does no I/O, so the work the `DNSZone` → `Bind9Instance` mapper used to spawn now runs as a controller reconcile (deduplicated, backed off, counted), and only when the zone's instance selection changes (`sdk::watch::changed_only`), so record timestamps written into zone status cannot fan out into instance reconciles | D2 (reconcile storm; unbounded out-of-band work) | ✅ `crates/bindy-controller-zone/src/watch.rs`, `crates/bindy-controller-instance/src/watch.rs` |
| M-42 | **Scout RBAC drift test** (2026-10-05, ADR-0009 §6): the Scout ClusterRole, Roles and bindings that `bindy bootstrap scout` builds are compared with `deploy/scout/*.yaml` and the `docs/src/guide/scout.md` examples; any difference in rules, role references or subjects fails the build | E2, T4 (RBAC widened in one representation, unreviewed in the others) | ✅ `crates/bindy-bootstrap/src/bootstrap_tests.rs` (`rbac_drift`) |
| M-43 | **Draining shutdown and startup recovery** (2026-10-05, ADR-0009 §5): SIGTERM, SIGINT and loss of the leader lease fire one trigger; every controller stops taking work and finishes its in-flight reconciles (`graceful_shutdown_on`), and a controller that stops on its own fails the process (`supervise`). Drift made while no operator runs is repaired from the watchers' initial lists when one starts, proven by the restart e2e suite, so the separate startup pass (cluster-wide LISTs even under M-22) is deleted | Availability, partial writes on shutdown, M-22 scope leak | ✅ `crates/bindy-controller-sdk/src/shutdown.rs`, `crates/bindy/src/main.rs`, `tests/e2e/restart_test.sh` |
| M-44 | **Rendered-configuration gate** (2026-10-05, ADR-0013 stages 1 and 2): every `named.conf*` the operator renders is parsed and validated with hornet, in CI across the option matrix and every example, and at runtime before the ConfigMap is written; an invalid render is not published, the resource reports `Ready=False` / `ConfigurationInvalid`, and the pods keep the last published configuration | D4 (malformed config takes every BIND9 pod down) | ✅ `crates/bindy-bind9/src/config_check.rs`, `crates/bindy-bind9/src/rendered_config_tests.rs` |
| M-45 | **Per-request deadline on non-watch Kubernetes API requests** (2026-10-05, ADR-0014): a tower layer in the operator's client stack, inside the M-31 rate limiter, bounds each non-watch request to 30 s by default (`BINDY_KUBE_REQUEST_TIMEOUT_SECS`, invalid overrides fall back safely) across its response headers and body; the timeout is a retryable `kube::Error::Service`, so the existing backoff takes over. Requests with `watch=true` are exempt | D5 (stalled connection freezes reconciles) | ✅ `crates/bindy-controller-sdk/src/request_timeout.rs`, `crates/bindy-controller-sdk/src/rate_limit.rs` |
| M-46 | **Bounded API cost of DNS writes** (2026-10-05, ADR-0015): per-reconcile `InstanceResolver` (each instance's RNDC key and endpoints read once), endpoints and instance roles from the existing reflector stores, a 60 s in-memory RNDC key cache invalidated on rotation and on any failed write, no record-side rewrite of `DNSZone.status.records`, a zoneRef-only status trigger for record reconciles, tag-once and LIST-based existence checks in zone reconciles; deleted records stay tracked until their DNS data is confirmed gone, and replays skip terminating records | D2 (API amplification: records x instances), T1 (deleted records left served), I1 (key reuse bounded) | ✅ `crates/bindy-bind9/src/instances.rs`, `crates/bindy-bind9/src/record_push.rs`, `crates/bindy-controller-records/src/record_operator.rs`, `crates/bindy-controller-zone/src/dnszone/{cleanup,discovery}.rs` |
| M-47 | **Configuration written by construction** (2026-10-05, ADR-0013 stage 3): `named.conf` and `named.conf.options` are built as a hornet syntax tree from typed values (ACL entries parsed into address-match elements, forwarders into addresses, the DNSSEC policy into a typed statement) and written by hornet's writer, which quotes and escapes each value for its position; the text templates are retired, a test asserts every rendered file is hornet's canonical output with no raw carrier, and the option matrix and examples pass `named-checkconf` 9.18 and 9.20 | D4 (malformed config), T3 (configuration injected through a CRD value; second layer behind the CRD patterns, `bind9_acl` and M-24) | ✅ `crates/bindy-bind9/src/bind9_resources/`, `crates/bindy-bind9/src/bind9_acl.rs`, `crates/bindy-bind9/src/rendered_config_tests.rs` |
| M-48 | **Event-driven reconciliation, no periodic resync** (2026-10-06, ADR-0016): every controller (records, `DNSZone`, `Bind9Instance`, `Bind9Cluster`, `ClusterBind9Provider`) returns `await_change` on success and on a wait for another object; each wait is ended by a named watch, with pure mappers added for a `DuplicateZone` loser, records waiting on their zone, and cluster-level ConfigMaps; BIND9/bindcar failures retry with the per-object capped backoff (rejected record writes no sooner than the 30 s cooldown); scheduled wakes only for RNDC rotation and KSK rollover, capped at 30 days; record status decided from the watch cache with a patch that never carries `zone`/`zoneRef`, zones read from the store, and the `DNSZone` controller's instance roles, keys and endpoints from the stores and the ADR-0015 resolver | D2 (steady-state reconcile and API cost proportional to object count), T3 (cluster ConfigMap drift now event-driven) | ✅ `crates/bindy-controller-sdk/src/{reconcile,error,retry}.rs`, `crates/bindy-controller-records/src/{record_operator,record_wrappers}.rs`, `crates/bindy-controller-records/src/records/{mod,status_helpers}.rs`, `crates/bindy-controller-zone/src/watch.rs`, `crates/bindy-controller-zone/src/dnszone.rs`, `crates/bindy-controller-instance/src/watch.rs` |
| M-49 | **Zones-loaded readiness gate** (2026-10-06, ADR-0017): every BIND9 pod template carries `readinessGates: [{conditionType: bindy.firestoned.io/zones-loaded}]`; a Pod controller in the operator (label-selected BIND9 Pod watch plus a filtered `DNSZone` mapper, no timer) loads every live zone selecting the pod's instance, and on a primary every record tagged with it, onto the one pod through the zone controller's write paths, then sets the condition with a strategic merge patch of `pods/status`; BIND9 writes reach container-ready pods the gate still holds out of the Service; a failed zone blocks only while another Ready pod of the instance serves it; one-way latch per pod; RBAC adds `get`/`patch` on `pods/status` only, pinned by tests | D6 (empty pod admitted to its Service: availability), T1 (records written during a rollout reach the new pod) | ✅ `crates/bindy-bind9/src/bind9_resources/`, `crates/bindy-bind9/src/instances.rs`, `crates/bindy-controller-zone/src/zones_gate.rs`, `crates/bindy-controller-instance/src/bind9instance/resources.rs`, `deploy/operator/rbac/{role,namespaced/role}.yaml`, `crates/bindy-bootstrap/src/bootstrap_tests.rs` |
| M-50 | **Gate closed at the start of termination** (2026-10-07, ADR-0017 decision 6, corrected in v1.19): the zones-loaded gate controller sets a terminating pod's `bindy.firestoned.io/zones-loaded` condition `False` (`PodTerminating`) on the Pod deletion event, with no wait; the condition is `False` at once, but the pod's `Ready` (and its EndpointSlice `serving`) follow only on the kubelet's next status sync, about 18 s later on rc.6, so traffic does not move before `named` exits; the handover is kept short by the new pod being Ready first and by M-51 (rc.6: 2 of 136 probe queries lost over a staggered rollout); a pod without the gate or already `False` is left alone; a failed patch retries with the per-object backoff, a pod already gone is done; preStop drain kept at 10 s | D6 (availability during rollouts: dead endpoint still `serving`) | ✅ `crates/bindy-controller-zone/src/zones_gate.rs` (`gate_step`, `termination_condition`), `crates/bindy-api/src/constants.rs`, `crates/bindy-controller-zone/src/zones_gate_tests.rs` |
| M-51 | **Staggered BIND9 rollouts** (2026-10-07, ADR-0018): a Deployment change under `spec.template` is applied only while no instance in the instance's conflict set (shares a `DNSZone` in `status.bind9Instances`, or the same `Bind9Cluster` / `ClusterBind9Provider`) is mid-rollout, read from the Deployment and Pod stores; one in-process queue (first come, first served, claims close the race between concurrent reconciles); `ProgressDeadlineExceeded` and post-rollout degradation do not block; woken by Deployment and Pod mappers and by the queue, no timer; queued instances report `Rollout=False/RolloutQueued` and keep their observed generations; creation, replica changes and RNDC rotation are not delayed; the drift check compares every owned pod-template field semantically (API-server defaulting absorbed) and a template patch that bumps no generation is remembered as a known no-op, dropping its claim and queue place once, so a comparison miss cannot loop (ADR-0018 decision 8, v1.19) | D6 (every nameserver of a zone rolled at once) | ✅ `crates/bindy-controller-instance/src/rollout.rs` (`begin_template_change`, `finish_template_patch`), `crates/bindy-controller-instance/src/bind9instance/resources.rs` (`deployment_change`, `template_difference`, `create_or_update_deployment`), `crates/bindy-controller-instance/src/bind9instance/template_drift.rs`, `crates/bindy-controller-instance/src/watch.rs`, `crates/bindy-controller-instance/src/rollout_tests.rs`, `crates/bindy-controller-instance/src/bind9instance/resources_tests.rs` (`live_api_server_shape`) |
| M-52 | **Zone transfer peers follow the pods** (2026-10-07, ADR-0019): a zone's transfer peers are computed on every reconcile from the BIND9 Pod store (transfer sources: the admitted, non-terminating primary pods; `allow-transfer`: the non-terminating secondary pods; `also-notify`: the secondary Services' ClusterIPs), compared with `DNSZone.status.transferPeers`, and pushed when they change: every primary's `allow-transfer` / `also-notify` rewritten in place (bindcar PATCH, both lists always sent, empty included), every secondary zone whose transfer sources moved replaced (delete, create with exactly the current primaries, retransfer); recorded only once every server took them; the zones-loaded gate rewrites the primaries' ACLs before it loads a new secondary and holds it while a sibling serves and its transfer is pending; a secondary with the zone configured but not loaded makes the zone `Degraded` (`SecondaryNotLoaded`); bindcar's masked 500 for an unloaded zone is classified with a SOA probe of `named` and is not retried for two minutes | I2 (stale `allow-transfer` IP inherited by another pod), D3, availability (a secondary silently empty while the zone reported Ready) | ✅ `crates/bindy-bind9/src/peers.rs`, `crates/bindy-bind9/src/bind9/zone_ops.rs` (`zone_presence`, `update_primary_transfer_peers`, `replace_secondary_zone`), `crates/bindy-controller-zone/src/dnszone/transfer_peers.rs`, `crates/bindy-controller-zone/src/dnszone/bind9_config.rs`, `crates/bindy-controller-zone/src/zones_gate.rs`; tests `peers_tests.rs`, `zone_ops_tests.rs`, `transfer_peers_tests.rs`; `tests/e2e/chaos_test.sh` |
| M-53 | **Leader lease on its own client, renewed early** (2026-10-07): the leader election uses a Kubernetes client of its own (5 QPS, not queued behind the controllers' rate limit) with a request deadline of the renew deadline minus one retry period (8 s by default), and renews one retry period after each renewal (`renewal_grace` = lease duration minus retry period) instead of 2 s before expiry; a stalled renewal fails and is retried inside the lease, and a lease taken by another replica is noticed within one request | D2 (operator availability), split brain after a slow renewal | ✅ `crates/bindy-controller-sdk/src/leader.rs` (`renewal_grace`, `lease_request_timeout`), `crates/bindy/src/main.rs`, `crates/bindy-controller-sdk/src/leader_tests.rs` |
| M-54 | **Owned container env replaced whole** (2026-10-07): the bindcar container's `env` in the Deployment patch carries `$patch: replace`, like the volumes, init containers and readiness gates, so a variable removed from `bindcarConfig` leaves the pods (a strategic merge kept removed entries forever and the drift check sent a no-op patch on every operator start) | T3 (a setting removed from the spec, possibly for security reasons, stays in effect) | ✅ `crates/bindy-controller-instance/src/bind9instance/resources.rs` (`build_deployment_patch`), `resources_tests.rs` |
| M-55 | **Every pod that holds a zone is reached** (2026-10-08, ADR-0015 decision 7): `for_each_instance_endpoint_with_policy` fails, whatever the policy, when a pod that holds the instance's zones (live, admitted by the zones-loaded gate, instance not deleted: `coverage_pod_ips`) was not reached or its operation failed; record writes and deletions (finalizer, rename, unselected), the zone stale-record pass and zone deletion (primaries and secondaries) go through it; `SkipUnavailable` skips only an instance whose data is gone; the record finalizer allows deletion when the zone is gone or being deleted; discovery deletes a deleted record's data itself and keeps tracking it until confirmed; a pending deletion makes the zone `Degraded` (`RecordDeletionPending`) | T1 (a removed record stays served from a restarted pod), truthful status | ✅ `crates/bindy-bind9/src/instances.rs` (`coverage_pod_ips`, `for_each_instance_endpoint_with_policy`), `crates/bindy-bind9/src/record_push.rs`, `crates/bindy-controller-records/src/records/mod.rs`, `crates/bindy-controller-zone/src/dnszone.rs` (`delete_dnszone`), `dnszone/discovery.rs`, `dnszone/status_helpers.rs`; tests `instances_tests.rs` (9), `transfer_peers_tests.rs` (3); chaos step 15 |
| M-56 | **Failures stop the operation instead of proceeding on partial data** (2026-10-11): a record finalizer keeps the finalizer and retries when the zone or a role lookup fails (it lets go only when the record has no `zoneRef`, or the zone is gone, being deleted, selects no instances or has no primaries); the instance finalizer treats only 404 as deleted and deletes only the RNDC Secret the operator generated (never a `secretRef` Secret, `generated_rndc_secret`); a primary or secondary whose role cannot be read fails the filter instead of dropping out of the target set; a failed parent `Bind9Cluster` / `ClusterBind9Provider` read fails the reconcile instead of rolling pods to an instance-only spec; a paginated LIST that hits a repeated token, an empty page with a token or the page cap is an error, not a partial list; a failed role lookup never counts as zero expected instances (which could report `Ready=True`); no controller starts before every watch cache (provider, cluster, instance, Deployment, Endpoints, Pod, DNSZone and the nine record kinds) has listed once, so the zones-loaded gate never replays an empty record list onto a new pod; an owner without a UID is an error, not an empty `ownerReference` | T1 (a removed record stays served; a primary silently not updated), truthful status, D-class correctness under API failure | ✅ `crates/bindy-controller-records/src/records/mod.rs` (`zone_lookup_step`, `primaries_step`), `crates/bindy-controller-instance/src/bind9instance/{resources,cluster_helpers}.rs` (`deleted_or_absent`, `parent_or_none`), `crates/bindy-bind9/src/primary.rs` (`role_from_get_result`, `try_filter_instances_by_role_cached`), `crates/bindy-controller-zone/src/dnszone/{secondary,status_helpers}.rs`, `crates/bindy-controller-sdk/src/{pagination,watch,context}.rs` (`next_page_step`, `wait_until_ready`), `crates/bindy/src/main.rs` (`acquire_unless_shutdown`), `crates/bindy-controller-instance/src/bind9instance/mod.rs` (`fetch_cluster_info` fallible), `crates/bindy-bind9/src/bind9_resources/` (`controller_owner_reference`) |
| M-57 | **A record write validates before it replaces** (2026-10-11; superseded in mechanism by M-62, 2026-10-13): every kind's values are parsed and validated before any query or update, so an invalid entry fails the write with the live RRset intact; since ADR-0021 the old RRset's delete and the new values travel in one UPDATE, and any response other than `NOERROR` fails the write | T1 (a partly rebuilt RRset; new data appended to stale data while the write reports success) | ✅ `crates/bindy-bind9/src/bind9/records/` (per-kind `*_rdata` conversions, `check_replace_response`) |
| M-58 | **The operator uses the RNDC key the pods hold** (2026-10-11): the key loader reads the Secret name from the instance Deployment's `rndc-key` volume (`rndcKey.secretRef`, an inline `rndcKey.secret` name, or the default), and the writer, the drift check and the Deployment share one name helper; the instance Service follows the merged `bindcarConfig` (instance, cluster, provider). Anyone who can patch the Deployment could already mount that Secret into the pod, so reading the name from it adds no read path. Since ADR-0023 the name is `{instance}-rndc-key` in every RNDC mode (M-65) | I1 (no fallback Secret read outside the configured one), availability of dynamic updates | ✅ `crates/bindy-bind9/src/bind9_resources/` (`rndc_secret_name`, `rndc_secret_name_from_deployment`, `instance_configmap_name`), `crates/bindy-bind9/src/instances.rs` (`effective_rndc_secret_name`); uses the operator's existing `deployments: get` |
| M-59 | **Scout keeps published records in step with the source** (2026-10-11): the TCPRoute record-name override goes through the same P2-7 validation as every other source kind; an `Ingress`/`HTTPRoute`/`TLSRoute` host that was removed or no longer matches the zone has its record pruned; a `Service` that stops being a `LoadBalancer` releases its records and finalizer; `Service` records left by an old `--cluster-name` are cleaned up; `bindy bootstrap scout --namespace X` points the scout at `X`, where its writer Role is | T1 (tenant-controlled record names; stale records served after the source changed), least privilege (the scout writes where its RBAC is) | ✅ `crates/bindy-scout/src/scout.rs` (`tcproute_record_name`), `crates/bindy-scout/src/pipeline.rs` (`prune_unapplied_arecords`, `delete_stale_cluster_arecords`, the `Service` source's `publishable`; since ADR-0022 one pipeline for every kind, M-63), `crates/bindy-bootstrap/src/bootstrap.rs` (`SCOUT_TARGET_NAMESPACE_ENV`) |
| M-60 | **bindcar transport fails closed** (2026-10-11): the TLS CA bundle is read only from the instance's namespace (a manager without one refuses TLS instead of reading `default`), and a bindcar HTTP client that cannot be built with its timeouts fails every call instead of falling back to a client without timeouts | Spoofing of the bindcar endpoint (a CA bundle planted in `default` trusted for TLS, ADR-0004), availability (an untimed bindcar request hangs a reconcile; D5 is the API-server analogue) | ✅ `crates/bindy-bind9/src/bind9/mod.rs` (`new_for_instance`, `read_ca_bundle`, `build_http_client_with_timeouts`), `crates/bindy-bind9/src/context.rs` |
| M-61 | **The multi-cluster kubeconfig Secret is written to a private file** (2026-10-12): `bindy bootstrap mc` writes the `bindy.firestoned.io/remote-kubeconfig` Secret, which carries the remote ServiceAccount token, to a file created with mode `0600` (`--output`, default `<service-account>-kubeconfig.yaml`); an existing file is restricted to `0600` before it is truncated; the Secret is never printed (`--output -` is refused). Status messages stay on stderr | I1 (a token left in terminal scrollback, a CI log or a shared session recording), CodeQL `rust/cleartext-logging` | ✅ `crates/bindy-bootstrap/src/bootstrap.rs` (`mc_output_path`, `write_private_file`), `crates/bindy/src/cli.rs` (`--output`); tests `bootstrap_tests.rs` (`mc_output_tests`, 5) |
| M-62 | **An RRset is replaced in one atomic DNS UPDATE** (2026-10-13, ADR-0021): each record write is one TSIG-signed UPDATE (delete the RRset, then add every value), which BIND9 applies as a unit, so a replaced name never answers NXDOMAIN or a partial RRset; every kind's values are validated before any network call (an out-of-range MX priority is now rejected, not coerced); the current RRset is compared with the desired one by value (names normalised for case and trailing dot, order-insensitive, TTL compared), so an unchanged record sends nothing; an UPDATE larger than the EDNS payload is sent over TCP instead of an oversized UDP datagram; the compare before the write reads the RRset with an unsigned query, only the UPDATE is TSIG-signed (a spoofed answer can at most make the operator skip a write, not inject data) | T1 (DNS tampering: no window of missing or partial data during a write; no churn from text comparison) | ✅ `crates/bindy-bind9/src/bind9/records/mod.rs` (`replace_rrset_message`, `send_rrset_update`, `needs_update`, `update_transport`, `write_rrset`) and the per-kind conversions in `records/*.rs` |
| M-63 | **Scout sources share one pipeline; records are owned by exactly one source** (2026-10-13, ADR-0022): Ingress, Service, HTTPRoute, TLSRoute and TCPRoute go through one `reconcile_source`, so a check one kind skipped (B1, B11) cannot recur; every `ARecord` carries `bindy.firestoned.io/source-kind` and every selector Scout uses includes it; names are `scout-{cluster}-{namespace}-{name}-{hash}-{index}` with the hash over the NUL-joined (cluster, kind, namespace, name); a record from before ADR-0022 (no source-kind label, matched by `!bindy.firestoned.io/source-kind`) or from a previous `--cluster-name` in the same zone is deleted only once every record replacing it reports `status.zoneRef` and `Ready=True`, rechecked every 10 s until then, so the upgrade rename never takes a name off the air; status addresses that are not IPv4 are skipped; an invalid TTL annotation publishes nothing for that source; malformed startup configuration fails Scout at startup; a Gateway or Service lookup error other than 404 fails the reconcile; the opt-in check runs before the namespace LIST | T1 (a tenant's same-named source overwriting or pruning another's record; hyphen-ambiguous names across namespaces), closes accepted risk 20 | ✅ `crates/bindy-scout/src/pipeline.rs` (`ScoutSource`, `reconcile_source`, `handover_step`, `arecord_served`), `crates/bindy-scout/src/scout.rs` (`LABEL_SOURCE_KIND`, `source_identity_hash`, `arecord_cr_name`, `retain_ipv4`, `parse_ttl_annotation`, `ScoutConfig::from_lookup`, `legacy_arecord_label_selector`); tests `pipeline_tests.rs`, `tests/e2e/scout_test.sh` |
| M-64 | **A name shared by two records survives one's deletion** (2026-10-13, ADR-0022 decision 5): the record finalizer and the rename path do not delete an RRset that another live record of the same kind in the same zone still declares (bound to that zone by `status.zoneRef`, same name ignoring case, a different object, not being deleted); both re-push the survivor instead (compared by value first, so nothing is sent when BIND9 already serves it); a failed re-push keeps the finalizer, or on a rename keeps `status.publishedName`, so it is retried. One rule (`claims_rrset`) serves the finalizer and the zone's stale-record cleanup (bug-211) | T1 (a shared name taken off the air with nothing to re-push it, since ADR-0016 has no periodic resync) | ✅ `crates/bindy-controller-sdk/src/rrset_claim.rs`, `crates/bindy-controller-records/src/records/mod.rs` (`rrset_claimant`, `repush_claimant`, `delete_record`, `reconcile_record`), `crates/bindy-controller-zone/src/dnszone/discovery.rs` (`claimed_by_live_record`); tests `rrset_claim_tests.rs` (12), `records/mod_tests.rs::rrset_claimant_tests` (5) |
| M-65 | **A `secretRef` RNDC key is mirrored, never mounted** (2026-10-14, ADR-0023): the operator builds the key from the referenced Secret (its `secretKey` entry, the algorithm from its `algorithm` entry else `secretRef.algorithm`, else its `rndc.key` file), names it `bindy-operator` (the only name the cluster-shared `controls` block accepts; `keyNameKey` is ignored), validates it (algorithm allow-list, base64 key, no key in any error message) and writes it to the instance-owned `{instance}-rndc-key`, annotated `bindy.firestoned.io/rndc-source-secret`, which every pod mounts. A source change rewrites the mirror, drops the cached key and rolls the pods (the reconcile fast path counts a mirror behind its source as drift, `secret_ref_mirror_stale`); any replacement of an existing `{instance}-rndc-key` (mode switch, generated key recreated) does the same. A mirror is never rotated and is replaced by a fresh key when the instance returns to a generated one. Fail fast without destroying: a missing or unusable source fails the reconcile and keeps the mirror, a GET of the RNDC Secret that fails with anything but 404 fails instead of regenerating the key, and a `secretRef` naming `{instance}-rndc-key` is refused | T3 (configuration injected into `rndc.key`), I1 (key in errors; a stale key held by the pods), availability of dynamic updates after a key change | ✅ `crates/bindy-bind9/src/bind9/rndc.rs` (`rndc_key_from_source_secret`, `create_rndc_mirror_secret`), `crates/bindy-controller-instance/src/bind9instance/resources.rs` (`mirror_step`, `mirror_secret_ref_key`, `validate_rndc_secret_ref`), `crates/bindy-controller-instance/src/watch.rs` (`instances_for_secret`); tests `rndc_tests.rs`, `resources_tests.rs`, `watch_tests.rs::secret_wake_tests`, `tests/e2e/rndc_secretref_test.sh` |
| M-66 | **RNDC `secretRef` names a `bindy-` Secret** (2026-10-14, ADR-0023 decision 6): every `rndcKey.secretRef.name` and deprecated `rndcSecretRef.name`, at every level a CRD carries one, must start with `bindy-`, the H2 allow-list prefix (bug-021). Closes the confused deputy through which a CR author could have the operator place entries of any Secret of the namespace into a Secret the operand pods mount. Enforced at admission and again by the reconciler where the policy is not installed; no RBAC grant changed | E2 (operator Secret read as a confused deputy), I1 | ✅ `deploy/admission-policies/07-bindy-pod-shape-policy.yaml` (rule 5; fixtures `accept-bind9instance-rndc-bindy-secret.yaml`, `reject-bind9instance-rndc-foreign-secret.yaml`, `reject-bind9cluster-rndc-foreign-secret.yaml`), `crates/bindy-bind9/src/safe_volume.rs` (`validate_rndc_key_secret_name`), `validate_rndc_secret_ref`; tests `safe_volume_tests.rs`, `resources_tests.rs`, `tests/e2e/rndc_secretref_test.sh` |
| M-67 | **Secret audit trail** (2026-10-14, ADR-0024 as amended): every Secret create, replace, patch, delete or read of key material bindy performs (instance controller, cluster controller, the `bindy-bind9` key loader and TLS client, Scout, bootstrap) emits one event on the tracing target `bindy::audit` with `kind=audit`, operation, outcome, namespace, name, purpose, reason, actor and owning object, from closed enums; never key material, a token, or an API error message (a failure is its status code and reason only). Every Secret access goes through `SecretApi`, whose data methods record and whose metadata method does not; a test fails the build when other non-test code names `Api<Secret>` (a text scan: an `Api` whose type is only inferred escapes it, so review still checks new calls). `bindy::audit=info` is added to any `RUST_LOG` that does not name it, and each event increments `bindy_firestoned_io_secret_operations_total{operation,purpose,outcome}` | R1, R2 (Secret access attributable with its reason, joined to the API server audit log), Scenario 4 | ✅ `crates/bindy-controller-sdk/src/audit.rs` (`SecretApi`, `SecretAudit`, `audited`, `kube_error_summary`, `audit_log_filter`), `crates/bindy-controller-sdk/src/metrics.rs` (`SECRET_OPERATIONS_TOTAL`), `crates/bindy/src/main.rs` (`initialize_logging`); tests `audit_tests.rs` (incl. `no_code_outside_the_audit_module_builds_a_secret_api`), `tests/e2e/rndc_secretref_test.sh` |
| M-68 | **Least-data Secret access** (2026-10-14, ADR-0024 decision 3 as amended): the instance controller's Secret watch is a metadata watch (`Api<PartialObjectMeta<Secret>>`), so the operator no longer receives the data of every Secret in its namespaces; the drift check, rotation status and the cluster controller's existence check read metadata only; the ADR-0023 mirror records its source's `resourceVersion` (`bindy.firestoned.io/rndc-source-version`), so staleness is a metadata comparison and a source's key is read only when it changed | I1 (key material in operator memory, heap dumps and logs), R2 (every recorded `read` is a real key access) | ✅ `crates/bindy-controller-instance/src/watch.rs` (Secret metadata watch, `instances_for_secret`), `crates/bindy-controller-instance/src/bind9instance/resources.rs` (`secret_ref_mirror_stale`, `mirror_secret_ref_key`), `crates/bindy-controller-instance/src/bind9instance/mod.rs` (drift check), `crates/bindy-controller-cluster/src/bind9cluster/instances.rs`; tests `resources_tests.rs`, `watch_tests.rs::secret_wake_tests` |
| M-69 | **Scout zone and address allowlists** (2026-10-19, ADR-0025 decisions 1 and 2): `--allowed-zones` / `BINDY_SCOUT_ALLOWED_ZONES` (a zone and its subzones; `*` and non-DNS entries fail startup; `--default-zone` must be allowed) refuses a source's zone before the DNSZone lookup and publishes nothing, keeping earlier records; `--allowed-ip-ranges` / `BINDY_SCOUT_ALLOWED_IP_RANGES` (IPv4 CIDRs; default IPs must be inside) refuses a source with any annotated, discovered or default address outside and retries; unset keeps the old behavior with a startup warning; `bindy bootstrap scout` templates both | T5 (tenant names another cluster's zone or an outside address) | ✅ `crates/bindy-scout/src/guardrails.rs` (`parse_allowed_zones`, `zone_is_allowed`, `parse_allowed_ip_ranges`, `first_ip_outside`), `crates/bindy-scout/src/pipeline.rs` (`publish`), `crates/bindy-scout/src/scout.rs` (`ScoutConfig::from_lookup`), `crates/bindy-bootstrap/src/bootstrap.rs` (`build_scout_deployment`); tests `guardrails_tests.rs` (19), `pipeline_tests.rs`, `scout_tests.rs`, `bootstrap_tests.rs` |
| M-70 | **Registered Scout accounts bound to their clusters and zones at admission** (2026-10-19, ADR-0025 decisions 3 and 4): `bindy bootstrap mc --cluster-name --allowed-zones` writes `<sa>.clusters` and `<sa>.zones` into the `bindy-scout-sources` ConfigMap of the records namespace under a per-account field manager (`--revoke` removes them); the `bindy-scout-source-binding` ValidatingAdmissionPolicy (paramRef without namespace, `parameterNotFoundAction: Allow`) denies a registered ServiceAccount's ARecord CREATE, UPDATE or DELETE unless the new object carries `managed-by: scout`, a registered cluster and an allowed zone, and the old object a registered cluster and an allowed zone; unregistered principals are not matched. Verified on kind 1.36: type checking clean, refused writes answer 422 with the policy message, server-side apply denied the same way | S4, T5 (stolen or compromised Scout token) | ✅ `deploy/admission-policies/21-bindy-scout-source-binding-policy.yaml`, `22-bindy-scout-source-binding-binding.yaml`, fixtures `tests/accept-arecord-scout-registered.yaml`, `tests/reject-arecord-scout-{foreign-zone,foreign-cluster,suffix-zone,unmanaged}.yaml`, `tests/setup/scout-source-binding.yaml`; `crates/bindy-bootstrap/src/bootstrap.rs` (`build_scout_source_registry`, `scout_source_field_manager`, `unregister_scout_source`) |
| M-71 | **A DNSZone takes a Scout record only for its own zone** (2026-10-19, ADR-0025 decision 5): a record labelled `bindy.firestoned.io/managed-by: scout` that a `recordsFrom` selector matches is selected only when its `bindy.firestoned.io/zone` label equals the zone's `spec.zoneName` (case and trailing dot ignored); otherwise skipped with a warning | T5 (a selector that omits the zone label takes other clusters' records) | ✅ `crates/bindy-controller-zone/src/dnszone/discovery.rs` (`scout_record_belongs_to_zone`, `discover_records_generic`); tests `discovery_tests.rs` (5) |
| M-72 | **Scout treats a refused delete of another cluster's record as final** (2026-10-19, ADR-0025 decision 6): on handover and on source removal, a 403 or 422 for a record left under another cluster name is logged and skipped, so a source's finalizer is not held and nothing is retried; the source's own records still fail the reconcile on any error | Availability under M-70 (a refused cleanup holding a tenant's Ingress in `Terminating`) | ✅ `crates/bindy-scout/src/pipeline.rs` (`delete_other_cluster_arecord`), `crates/bindy-controller-sdk/src/kube_errors.rs` (`is_refused`); tests `pipeline_tests.rs` (refused-delete cases), `kube_errors_tests.rs` |
| M-25 | **Scout Secret RBAC scoped** (fixed 2026-07-19, same day as this finding's discovery): removed the cluster-wide `secrets: get` `PolicyRule` from the `bindy-scout` `ClusterRole` entirely. Replaced with a namespaced, `resourceNames`-restricted Role (`bindy-scout-secrets-reader`) scoped to exactly the one Phase 2 kubeconfig Secret, applied only when `--remote-secret` is configured. Same-cluster-only deployments (the default) now get zero Secret access. See I4/E4/Scenario 6 for the full before/after. | I4, E4, T4 (Secret-read component), Scenario 6 | ✅ RBAC: **was the highest-priority open item in v1.1; closed same-day** |

---

### Planned Mitigations (Roadmap)

| ID | Mitigation | Threats Mitigated | Priority | Roadmap Item |
|----|------------|-------------------|----------|--------------|
| M-11 | Audit log retention policy | R1 (non-repudiation) | HIGH | H-2 |
| ~~M-12~~ | ~~Secret access audit trail~~ **DONE: see M-67 and M-68** (ADR-0024) | R2 (secret access), I1 (disclosure) | n/a | Completed 2026-10-14 |
| ~~M-13~~ | ~~Admission webhooks~~ **DONE: see M-24** | T1 (DNS tampering) | n/a | Completed |
| M-14 | **DNSSEC signing** (roadmap 07 complete 2026-09-27, ADR-0006): opt-in `dnssec-policy` zone signing (Secret-backed / auto-generated keys; key Secret names validated against the allow-list prefix, see H2; ADR-0012: Secret keys copied by an init container into a tmpfs key directory, shared by every primary, `unlimited` lifetimes enforced by CRD CEL and at render), DS records derived from the zone's KSK DNSKEYs (SHA-256, RFC 8624) and published in `DNSZone.status.dnssec`. DS/keyTag are public data by design; no key material reaches status or logs. Adds one read-only in-cluster query path, operator → `named` :5353 (DNSKEY only, modeled in CALM) | T1 (tampering), Scenario 2 (cache poisoning) | ✅ Opt-in: effective once DS is published in the parent zone |
| M-15 | Image digest pinning: **partial**. Release `install.yaml`/`scout.yaml` pin the operator image by digest (P2-8); operand images (BIND9, bindcar) remain tag-referenced | T2 (image tampering) | MEDIUM | M-1 |
| M-16 | Rate limiting (operator) | D2 (operator exhaustion) | MEDIUM | M-3 |
| M-17 | Network policies: a reference manifest now exists (`deploy/pod-hardening.yaml`, ingress/egress scoped to container port 5353) but is **not applied by any install target**; remains opt-in/manual | S1 (API spoofing), E1 (lateral movement), T4/E4 (Scout egress) | LOW | L-1 |
| M-18 | DDoS edge protection | D1 (DNS query flood) | HIGH | External |
| M-19 | RNDC key rotation | I1 (key disclosure) | MEDIUM | Future |
| M-20 | TSIG for AXFR | I2 (zone enumeration) | MEDIUM | Future |
| ~~M-25~~ | ~~Scope Scout's `secrets: get` to a namespaced Role~~ **DONE: see Existing Mitigations table above.** Fixed same-day as discovery (2026-07-19), before this revision was published. | I4, E4, T4, Scenario 6 | n/a | **Completed (v1.1)** |
| ~~M-26~~ | ~~Namespace-scoping option for Scout~~ **DONE: see M-30** (`--namespace-selector`) | T4 | n/a | Completed (v1.1) |
| M-27 | Egress NetworkPolicy for Scout (API server + remote-cluster API only) | E4 | MEDIUM | New (v1.1) |
| M-28 | Field-level admission policy constraining what Scout may `patch` on Ingress/Service/route objects (e.g. only finalizer/annotation fields) | T4 | MEDIUM | New (v1.1) |
| M-36 | Require at least one approving review (or a `CODEOWNERS`-backed review) on `main` and remove the always-on admin bypass, so no single account can land a change | S3, Scenario 3 | HIGH | New (v1.7) |
| M-37 | Automated reproducibility check (build a release twice, compare digests) | T2, Scenario 3 | LOW | ADR-0010 follow-up |
| M-40 | Hybrid post-quantum key exchange (`X25519MLKEM768`) on control-plane TLS: the HNDL-exposed channels carrying TSIG/RNDC secrets. Needs a crypto-provider ADR (ring has no ML-KEM) coordinated with bindcar | I1/I3 (harvest-now-decrypt-later capture of key material in transit) | MEDIUM | Roadmap 28 Phase 2 |
| M-29 | Revisit Dependabot auto-merge: consider requiring a human approval step for patch/minor merges, or expand e2e coverage to compensate | E3 (dependency exploit via unreviewed auto-merge) | MEDIUM | New (v1.1) |

---

## Residual Risks

### Critical Residual Risks

None identified. **Scout's cluster-wide Secret read (I4/E4/Scenario 6)**, which
had CRITICAL impact and essentially no compensating control, was identified and
fixed the same day (2026-07-19), before this revision was published. See the
Existing Mitigations table (M-25) and I4/E4/Scenario 6 for the full record. No
other CRITICAL-impact threat in this document currently lacks a strong mitigation.

---

### High Residual Risks

1. **DDoS Attacks (D1)** - Risk reduced by rate limiting and horizontal scaling, but edge DDoS protection is needed for volumetric attacks (100+ Gbps).

2. **Insider Threats (Scenario 4)** - Risk reduced by GitOps and RBAC, and bindy's own Secret access is now recorded with its reason (M-67, H-3 done), but immutable audit logs (H-2) are needed for full non-repudiation.

---

### Medium Residual Risks

1. **DNS Tampering (T1)** - Substantially reduced by RBAC and, as of 2026-07-01, an 8-policy `ValidatingAdmissionPolicy` suite (M-24) covering ACLs, zone names, RNDC strictness, pod shape, and record values. DNSSEC signing (M-14) shipped 2026-09-27 as the in-transit tampering defense, opt-in, so the residual gap is deployment coverage (unsigned zones) and DS publication in parent zones, not a missing capability. Since 2026-10-11 a record write or deletion no longer reports success on partial data (M-55, M-56, M-57). Since 2026-10-13 a record write replaces its RRset in one atomic UPDATE, so a reconcile opens no window of missing or partial data (M-62).

2. **Operator Resource Exhaustion (D2)** - Risk reduced by resource limits, client-side API rate limiting (M-31), the removal of self-triggered and out-of-band reconcile work (M-41), and of the periodic resync (M-48), so steady-state cost no longer grows with object count; a per-namespace CR quota (admission) is still needed.

3. **Zone Enumeration (I2)** - Risk reduced by AXFR restrictions, but TSIG authentication would eliminate AXFR abuse.

4. **Compromised Operator Pod (Scenario 1)** - Risk reduced by Pod Security Standards, but network policies (L-1) would prevent lateral movement. A reference NetworkPolicy manifest now exists (`deploy/pod-hardening.yaml`) but is not applied by any install target.

5. **Cross-Tenant Tampering via Scout (T4)** - Bounded by patch/update-only RBAC scope and, when configured, namespace whitelisting (M-30, opt-in, not on by default). No field-level admission control (M-28) yet constrains what Scout can patch. (Scout's Secret-read risk, formerly part of this component's overall exposure, was resolved separately; see M-25.)

6. **Single-person change path (S3)** - The `main` rulesets require signed commits, PRs and passing checks but no approving review, and organization admins can bypass them. A compromised maintainer account with its signing key can land a change unreviewed; release provenance would faithfully attest it. Planned: M-36.

7. **Shared watch layer (ADR-0009 §3)** - One watch per kind now feeds every controller, so a stalled watch leaves every controller of that kind acting on a stale cache until it recovers, and one slow controller slows its kind's watch for the others (backpressure). It is built on kube-runtime's `unstable-runtime-stream-control` feature, pinned to the 4.2 line. Mitigated by restart with backoff and per-kind staleness metrics (M-38); alert on `watch_last_event_timestamp_seconds`. Revisit when kube stabilises the stream APIs, or before any kube minor upgrade.

8. **Quantum-capable adversary (HNDL)** - Control-plane TLS key exchange is classical (X25519/ECDHE), so traffic recorded today, including TSIG/RNDC secrets in transit, is decryptable once a cryptographically relevant quantum computer exists; DNSSEC and release signatures additionally become forgeable at that point. Accepted for now: the inventory is published per release (M-39, ADR-0011), hybrid key exchange is planned (M-40, roadmap 28 Phase 2), and the signature surfaces are blocked on upstream standardization (IETF/BIND9, Sigstore). *Revisit when:* roadmap 28's six-month watch cadence fires (first 2027-04) or any upstream ships PQC support. A full quantum-adversary modeling pass across every trust boundary is roadmap 28 Phase 5.

9. **Secret-supplied DNSSEC keys never roll (ADR-0012)** - With `keysFrom.secretRef`, KSK and ZSK lifetimes are pinned to `unlimited` so every primary keeps the same key set; automatic rollover would make each pod generate a different successor. A key therefore stays in use until the operator rotates it by hand (new key into the Secret, pods restarted), which lengthens the exposure window of a key that leaked unnoticed. Editing the Secret also does not roll the pods by itself. Accepted: the alternative was a DNSKEY RRset that differs per pod, which breaks validation outright. *Revisit when:* coordinated rollover lands (one signer with transfers to the other primaries, or operator-generated successors written into the Secret), or a Secret content hash on the pod template.

10. **A deposed leader drains (ADR-0009 §5, bounded by M-53)** - On loss of the lease the old leader stops starting reconciles but finishes the ones in flight, so for at most one reconcile's duration it can write while the new leader starts. A leader also learns that it lost the lease only when a renewal returns: the chaos suite measured a renewal stuck on the shared client's 30 s deadline, during which the lease expired and another replica led beside it. With M-53 a renewal is bounded by the lease client's 8 s deadline and retried inside the lease, so the overlap after an expiry is at most one lease request plus the drain. Accepted: every write it can make is idempotent (record pushes query BIND9 first and write only a differing RRset; zone creation checks existence; Kubernetes objects are written as desired state, create-or-update), the drain is capped by the pod's termination grace period, and cancelling mid-reconcile, the old behaviour, could leave half-applied changes; a leader that cannot reach the API to renew cannot write Kubernetes objects either. *Revisit when* a reconcile gains a non-idempotent write, or reconcile durations approach the lease duration (15 s).

11. **Hand-edited `DNSZone` status waits for the next event (ADR-0009 §4, revised for ADR-0016)** - The zone controller does not react to status-only changes, so a status edited by hand (which needs `dnszones/status` write access, granted only to the operator) stands until the zone's next reconcile. With no periodic resync (ADR-0016) that is the zone's next event: any spec, label or annotation change, a selected record or instance change, an Endpoints change, a retry of a degraded zone, or an operator restart; there is no longer a 5-minute bound. Accepted: status is informational and rewritten by the controller; DNS data on BIND9 is unaffected; annotating the zone corrects it at once. *Revisit when* any decision reads zone status as input from outside the operator.

12. **API request deadline is operator-only, and cuts slow writes (ADR-0014)** - Scout (`bindy scout`, local and remote clients) and the `bindy bootstrap` CLI build their own Kubernetes clients without the M-45 deadline, so a stalled connection can still hold a Scout reconcile for minutes. And a legitimate operator request slower than the deadline (for example behind slow admission webhooks) is now cut and retried rather than completing. Accepted: Scout's write volume is small and its reconciles are independent, the bootstrap CLI is interactive, every operator write is a patch or server-side apply (a retry after a write that did land is idempotent), and the deadline is tunable per deployment. *Revisit when* Scout is load-tested at scale, or timed-out requests appear on a healthy API server.

13. **Out-of-band changes inside BIND9 are not reverted on a timer (ADR-0016)** - A change made directly in a running BIND9 pod (`nsupdate` or `rndc` with the instance's RNDC/TSIG key, a bindcar call with a token its TokenReview accepts, or a compromised operand container, Boundary 4) raises no Kubernetes event. The operator used to re-push every record every 5 minutes and so reverted such a change within that window; it now reverts it only on the owning resource's next event (spec, label, annotation or finalizer change), when the pod is replaced (the zones-loaded gate loads the zone and its records onto the new pod, ADR-0017; a container restart inside the same pod keeps the `emptyDir` data, and the change), when the operator restarts, or when an operator sets the `bindy.firestoned.io/reconcile-trigger` annotation. Accepted: the actor able to make the change holds the RNDC key or a valid bindcar token and could repeat it after any timed revert, so the timer bounded exposure without preventing it, at the cost of one reconcile per object every 5 minutes forever (ADR-0016 Context); the controls stay on the path itself (RNDC keys per instance, readable only by the operator, B-5; RNDC not exposed outside the cluster; bindcar TokenReview and TLS, ADR-0004; DNSSEC, M-14, makes a forged answer detectable by validating resolvers). *Revisit when* a drift detector that compares BIND9's served data with the declared records without re-pushing every record exists (for example a periodic read-only AXFR diff), or a deployment needs a bounded revert window as a compliance control.

14. **The operator can open or hold the zones-loaded gate (ADR-0017)** - The operator's ServiceAccount gains `get`/`patch` on `pods/status` in every operand namespace (cluster-wide in the default mode). A holder of that token can set `bindy.firestoned.io/zones-loaded=True` on a new BIND9 pod before its zones load, re-opening the empty-pod window of D6 for that pod, or set it `False`/leave it unset so new pods never enter their Service (serving pods are unaffected until replaced; with `maxUnavailable` 0 a rollout then stalls rather than dropping service). The grant cannot change a pod's spec, image, ServiceAccount or labels, or delete it, and Kubernetes RBAC cannot narrow `patch` to one condition type, so the same token could also rewrite other conditions on any pod in those namespaces (status only, no effect on what runs; the kubelet re-asserts the conditions it owns). Accepted: the same token can already rewrite zones and records (`patch` on `DNSZone` and record CRs, and write access to every BIND9 pod through bindcar; T1, Scenario 1), so this adds no stronger capability; every patch is in the API server audit log. *Revisit when* Kubernetes offers field-level authorization for status subresources, or a `ValidatingAdmissionPolicy` on `pods/status` restricting the operator's patches to the gate condition on bindy's pods is adopted (VAPs can match subresources).

15. **The gate fails safe, and admits partially when a zone cannot load anywhere (ADR-0017)** - If the operator is down, lacks the `pods/status` grant, or is older than the pod template, a new BIND9 pod never becomes Ready: with one replica and `maxUnavailable` 0 the old pod keeps serving and the rollout stalls (visible as a Deployment past `progressDeadlineSeconds` and a pod without the condition); a pod whose predecessor is already gone (eviction, node loss) stays out of service until the operator returns. Separately, a zone that fails to load on the new pod and is served by no other Ready pod of the instance does not hold the pod back: the pod is admitted with `ZonesPartiallyLoaded` and the zone is retried by the `DNSZone` controller, so one invalid zone cannot keep every other zone of a shared instance out of service. Accepted: failing closed on the operator is the point of the gate, and holding a pod for a zone no pod can serve protects nothing. *Revisit when* an instance runs more than one replica per Deployment (the sibling check then has more to compare), or the operator is expected to be unavailable for long periods.

16. **Rollout handover gaps that remain (ADR-0017 decision 6, ADR-0018)** - (a) With a MetalLB layer-2 `LoadBalancer` on `externalTrafficPolicy: Local`, a handover that takes the last Ready pod off the announcing node drops traffic to that IP until MetalLB re-announces from another node and clients take the gratuitous ARP ("a few seconds", longer for clients that mishandle gratuitous ARP); the zone's other nameservers answer meanwhile because rollouts are staggered (rc.6: one query timeout per primary handover, 2 of 136 over a full rollout). (b) A rollout that never completes (a new pod held by `ZonesLoadFailed`, an image that cannot be pulled) blocks the instances in its conflict set until its Deployment reports `ProgressDeadlineExceeded` (600 s by default) for every change; an actor who can edit one instance or break one zone can thereby delay, not prevent, the rollouts of instances sharing its cluster or zones. (c) The ordering is held in the leader's memory: after a leader change, rollouts in flight are seen in the store and the new leader's claims order the rest, but waiting instances lose their queue positions (they re-queue in the order they reconcile), and a deposed leader's last in-flight reconcile can start one rollout while the new leader starts another (accepted risk 10). Accepted: (a) is a property of layer-2 failover with `Local`, removed by `externalTrafficPolicy: Cluster` and made rarer by more than one replica (documented in the HA guide); (b) is bounded and visible (`Rollout=True/RolloutPeerStalled` on the instance that proceeded, the stalled Deployment's own condition); (c) costs at most one overlap and needs no new state. *Revisit when* MetalLB or Kubernetes offers a drain-aware announcement handover, bindy sets a shorter `progressDeadlineSeconds`, or instances run with more than one replica by default.

17. **What bindcar 0.9.0 leaves of the transfer-peer refresh (ADR-0019)** - (a) bindcar 0.9.0 has no PATCH field for a secondary's `primaries`, so a secondary zone whose transfer sources moved is deleted and created again: for the length of one AXFR (milliseconds for an in-cluster zone of hundreds of records) that secondary answers `SERVFAIL` for that zone, about twice per primary rollout; the primaries keep answering. (b) NOTIFY goes to the secondary instance's Service ClusterIP, the only target bindcar 0.9.0 can rewrite in place: it reaches one replica of a multi-replica secondary per message (the others follow on the SOA `refresh` timer and the zone reconcile's retransfer), none behind a headless Service, and is refused by the secondary where kube-proxy masquerades pod-to-Service traffic (`masqueradeAll`). (c) Zones created before ADR-0019 keep a port-qualified `also-notify` that bindcar 0.9.0 cannot rewrite; their `allow-transfer` is rewritten, so transfers and the I2 control hold, and their NOTIFY follows when the pod is next replaced. (d) "Configured but not loaded" is inferred from bindcar's masked 500 plus `named` answering `SERVFAIL` to a SOA query; a zone that fails to load for another reason is reported the same way (`SecondaryNotLoaded`), which is still truthful (it serves nothing). Accepted: availability is reduced only on one nameserver at a time, every state is visible in status, and each item is removed by a bindcar change listed in ADR-0019. *Revisit when* bindcar accepts `primaries` and port-qualified `also-notify` in its zone PATCH and reports an unloaded zone as a typed state.

18. **A pod that holds a zone but never becomes ready again blocks deletion (ADR-0015 decision 7)** - Record and zone deletions now wait, retried with backoff, until every pod that holds the zone was reached, so a deleted record can no longer stay served from a pod whose container was restarting. A pod stuck `Running` but never container-ready (a crash-looping container) keeps the record or zone `Terminating` and the zone `Degraded` (`RecordDeletionPending`) until the pod recovers or is deleted. Accepted: serving data the spec removed is the worse failure (T1), the state is visible on the zone and in the logs, and deleting the pod (its data goes with its `emptyDir`), its instance or the zone unblocks it at once. *Revisit when* an operator needs a forced-deletion annotation for unrecoverable pods.

19. **Fail-fast trades silent wrong state for visible stalls (M-56)** - A Kubernetes API that keeps failing now holds record and instance finalizers, record writes and zone status instead of letting them proceed on partial data, and an operator whose watch caches cannot finish their first list (for example, RBAC missing for one watched kind) waits at startup without starting any controller. An instance whose cleanup DELETE is forbidden stays `Terminating` until the RBAC is fixed or the finalizer is removed by hand. Accepted: each stall is logged and retried with backoff, visible as `Terminating` objects and failing reconciles (a stuck startup shows only in the log, `Waiting for the watch caches to finish their initial list...`, and in the watch error and staleness metrics, M-38), while the behaviour it replaces (a removed record left served, a primary skipped, `Ready=True` on a zone that was not configured) was invisible. *Revisit when* the operator exposes a readiness probe that reports the cache sync, so a stuck startup fails the rollout instead of waiting.

20. ~~**Scout records of different source kinds with the same name collide**~~ **Closed 2026-10-13 (ADR-0022, M-63).** Every Scout `ARecord` carries a `bindy.firestoned.io/source-kind` label that every Scout selector includes, and its name carries a hash of the (cluster, kind, namespace, name) source, so kinds no longer overwrite or prune each other's records and namespaces whose hyphen-joined names coincide no longer share an `ARecord`. Existing records are renamed once on upgrade: the new record is applied, and the old one is deleted only after the new one is bound to its zone and `Ready` (M-63), so the DNS name is served throughout; the record finalizer guard (M-64) keeps the name if the old record's deletion races the new record's push. A legacy record carries no kind, so when two kinds shared a name before the upgrade, whichever reconciles first replaces the shared legacy record and the other kind publishes its own record on its next reconcile (every source reconciles at Scout startup).

21. **A mirrored RNDC key is a second copy, and its change skips the rollout queue (ADR-0023)** - With `rndcKey.secretRef`, the key exists in the user's `bindy-` source Secret and in the operator-owned `{instance}-rndc-key` mirror, in the same namespace. Anyone who can read Secrets there can read either, so the mirror adds no reader, but a source deleted to revoke a key leaves the mirror (and the pods) holding it until the reference is changed or the instance deleted: the operator keeps the mirror on purpose, so DNS stays up while a reference is fixed. A key change rolls the instance's pods at once, as an automatic rotation does, without waiting behind other instances that share a zone (ADR-0018), because pods holding a stale key cannot answer writes. *Revisit when* key revocation must take effect without an operator action (then a deleted source should empty the mirror and fail the instance closed), or when a key change must be staggered across a zone's instances.

22. **Secret audit events are only as durable as the log pipeline, and can be turned off (ADR-0024)** - bindy writes its `bindy::audit` events to stdout with every other log line. They survive only if the cluster's log pipeline collects them and routes them (`fields.kind == "audit"`) to retention-controlled, tamper-evident storage; bindy does not ship or sign them itself, and a node lost before collection loses its buffered lines. An operator can also set `RUST_LOG=...,bindy::audit=off`: that is visible in the Deployment spec and in its GitOps history, but nothing forbids it today. The API server audit log still records the Secret requests themselves in either case. *Revisit when* the operator must guarantee delivery of its own audit events (then a dedicated sink, or Kubernetes Events, rather than stdout), or when disabling them must be prevented (then an admission policy rejecting `bindy::audit=off` in the operator Deployment's `RUST_LOG`).

23. **Scout source binding has four gaps (ADR-0025)** - (a) Two edge clusters registered for the same zone may publish the same name, and the records controller keeps whichever record wrote last (ADR-0021), so either can take over the other's name; the registry makes that sharing an administrator's choice, and roadmap 27 item 2 tracks surfacing the conflict. (b) A Scout ServiceAccount minted before ADR-0025, or by hand, is not constrained until it has a `<sa>.clusters` key, and a namespace without a `bindy-scout-sources` ConfigMap is not constrained at all (`parameterNotFoundAction: Allow`), so existing installs keep their previous exposure until each account is registered; `bindy bootstrap mc` warns when it creates an unregistered account. (c) The registry is an ordinary ConfigMap: anyone who can write ConfigMaps in the records namespace can widen an account's clusters or zones. Scout's own Role cannot, and that namespace is normally platform-owned. (d) Addresses are checked only inside Scout (M-69), not in admission, so a stolen remote token can still point a name in its own zones at any address. Accepted: (a) and (c) are configuration the platform team controls; (b) keeps upgrades non-breaking and is closed by re-running `bootstrap mc`; (d) is bounded to the account's own zones. *Revisit when* roadmap 27 item 2 lands (a), every supported install has registered its accounts and the binding can fail closed (b), a ConfigMap-integrity style policy is wanted for the registry (c), or the supported Kubernetes floor guarantees CEL's IP and CIDR library (d).

---

## Security Architecture

### Defense in Depth Layers

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 7: Monitoring & Response                             │
│  - Audit logs (Kubernetes API)                              │
│  - Vulnerability scanning (daily)                           │
│  - Incident response playbooks                              │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 6: Application Security                              │
│  - Input validation (CRD schemas)                           │
│  - Least privilege RBAC                                     │
│  - Signed commits (non-repudiation)                         │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 5: Container Security                                │
│  - Non-root user (uid 1000+)                                │
│  - Read-only filesystem                                     │
│  - No privileged capabilities                               │
│  - Vulnerability scanning (Trivy)                           │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 4: Pod Security                                      │
│  - Pod Security Standards (Restricted)                      │
│  - seccomp profile (restrict syscalls)                      │
│  - AppArmor/SELinux profiles                                │
│  - Resource limits (CPU/memory)                             │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 3: Namespace Isolation                               │
│  - RBAC (namespace-scoped roles)                            │
│  - Network policies (planned)                               │
│  - Resource quotas                                          │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 2: Cluster Security                                  │
│  - etcd encryption at rest                                  │
│  - API server authentication/authorization                  │
│  - Secrets management                                       │
└─────────────────────────────────────────────────────────────┘
             │
┌─────────────────────────────────────────────────────────────┐
│  Layer 1: Infrastructure Security                           │
│  - Node OS hardening (managed by platform team)             │
│  - Network segmentation                                     │
│  - Physical security                                        │
└─────────────────────────────────────────────────────────────┘
```

---

## Security Controls Summary

| Control Category | Implemented | Planned | Residual Risk |
|------------------|-------------|---------|---------------|
| **Access Control** | RBAC least privilege (main operator; `pods/status` `get`/`patch` only for the zones-loaded gate, test-pinned, ADR-0017), signed commits, B-5 Secret RBAC split, namespace-scoped operator mode (opt-in), 11 `ValidatingAdmissionPolicy` policies, Scout namespace whitelisting (opt-in, M-30), **Scout Secret RBAC scoped (M-25, fixed 2026-07-19)**, Scout records bound to their cluster and zones (M-69 to M-72, ADR-0025) | Field-level admission for Scout patches (M-28), Scout egress NetworkPolicy (M-27) | MEDIUM: driven by Scout's remaining cluster-wide `patch`/`update` on Ingress/Service/route (T4); the formerly-HIGH Secret-read risk (I4/E4) is resolved |
| **Data Protection** | Secrets encrypted, AXFR restricted, DNSSEC zone signing (opt-in, M-14/ADR-0006), RNDC key read from the mounted Secret only (M-58), which is always the operator-owned `{instance}-rndc-key`, a `secretRef` key mirrored into it from a `bindy-` Secret (M-65, M-66), bindcar TLS CA bundle read only from the instance's namespace (M-60) | TSIG for AXFR; DNSSEC-by-default | MEDIUM |
| **Supply Chain** | Signed commits/images, SLSA Build L3 provenance for all release artifacts (M-33), NTIA-gated SBOM attestations (M-34), anchored signer identity (M-35), gated per-release crypto inventory (M-39), `--locked` release builds, vuln scanning | Required approving reviews (M-36); operand image digest pinning (M-15); reproducibility check (M-37); hybrid PQ key exchange (M-40); revisit Dependabot auto-merge human-review gap (M-29) | LOW-MEDIUM (no required review on `main`, see S3; automated auto-merge removed a manual checkpoint, see E3; classical key exchange is HNDL-exposed, see accepted risk 8) |
| **Monitoring** | Kubernetes audit logs, vuln scanning, Secret audit trail with `secret_operations_total` (M-67), least-data Secret access (M-68) | Audit retention policy (M-11) | MEDIUM (event durability depends on the log pipeline, accepted risk 22) |
| **Resilience** | Rate limiting, fail-fast on API errors and partial lists with a startup cache gate (M-56), validated, atomic RRset replacement in one UPDATE (M-57, M-62), per-request API deadline (M-45), event-driven reconciliation with no periodic resync (M-48), zones-loaded readiness gate so a rollout never admits an empty BIND9 pod (M-49), handover at the start of termination (M-50), staggered rollouts across instances sharing a zone or cluster (M-51), resource limits | Edge DDoS protection, HPA | MEDIUM |
| **Container Security** | Non-root, read-only FS, Pod Security Standards, unprivileged DNS port + zero added capabilities (M-23) | Network policies (reference manifest exists, not auto-applied, M-17) | LOW |

---

## References

- [OWASP Threat Modeling](https://owasp.org/www-community/Threat_Modeling)
- [Microsoft STRIDE Methodology](https://learn.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- [Kubernetes Threat Model](https://github.com/kubernetes/community/blob/master/sig-security/security-audit-2019/findings/Kubernetes%20Threat%20Model.pdf)
- [NIST SP 800-154 - Guide to Data-Centric System Threat Modeling](https://csrc.nist.gov/publications/detail/sp/800-154/draft)

---

**Last Updated:** 2026-10-14
**Next Review:** 2027-01-14 (Quarterly)
**Approved By:** Security Team *(pending re-approval for v1.1: this revision has not yet been formally reviewed/signed off; see the revision note at the top of this document)*
