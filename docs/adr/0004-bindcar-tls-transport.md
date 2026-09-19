# ADR-0004: TLS transport to the bindcar sidecar

## Status

Accepted — implemented in part (2026-09-19)

Built: the CRD surface (`bindcarConfig.tls`), the sidecar wiring (TLS Secret
mounted, `BIND_TLS_CERT` / `BIND_TLS_KEY` / `BIND_TLS_RELOAD_INTERVAL` set),
scheme selection in `build_api_url_with_scheme`, the reserved-env guard, and
the CA-pinned verifier and client builder in `src/bind9/tls_client.rs`.

Also built (2026-09-19): the `Bind9Manager` plumbing. The manager carries the
instance's resolved TLS config and a Kubernetes client, lazily builds and caches
a CA-pinned HTTPS client on first use, and qualifies each bare `<pod-ip>:<port>`
endpoint with the right scheme. `Stores::resolve_bindcar_tls` merges
`bindcarConfig` across instance, cluster and provider and hands the manager the
`tls` block.

`resolve_client` **fails** rather than falling back to the plaintext client when
TLS is on but the CA bundle cannot be read — no Kubernetes client, no
`caBundle`, a missing object or key, or a bundle that does not parse. A fallback
would send the token in the clear on a deployment whose operator believes TLS is
on.

Remaining: VAP 15/16 (admission-level rejection of reserved env names) as
defence in depth behind the reconciler guard, and an end-to-end test against a
live cluster with cert-manager.

## Context

Bindy authenticates to each bindcar sidecar with a Kubernetes ServiceAccount
token, sent as a bearer credential on every zone operation. bindcar 0.8.0 can
now terminate TLS (`BIND_TLS_CERT` / `BIND_TLS_KEY`, optional mutual TLS via
`BIND_TLS_CLIENT_CA`, with certificate hot-reload), but bindy has no way to ask
for it: there is no field on `Bind9Instance`, `Bind9Cluster` or
`ClusterBind9Provider` that expresses a scheme or a trust anchor.

Until that exists, the token crosses the pod network in cleartext on every
operation — audit finding **P2-4**. `deploy/pod-hardening.yaml` restricts
ingress to the operator pod, which narrows *who can connect*; it does nothing
for anyone able to observe the link.

`build_api_url` (`src/bind9/zone_ops.rs:105`) already passes an explicit
`https://` through untouched, so the client is nominally ready. That framing is
misleading, and the rest of this ADR is why.

### The complication: bindy addresses pods, not Services

Bindy does not talk to a Service. `get_endpoint`
(`src/reconcilers/dnszone/helpers.rs:513`) reads the Service's `Endpoints`
object, collects the **ready pod IPs**, and every call site then builds
`format!("{}:{}", endpoint.ip, endpoint.port)` — eight of them across
`src/reconcilers/dnszone.rs` and `dnszone/secondary.rs`.

This is deliberate and load-bearing. `Bind9Instance.spec.replicas` defaults to 1
but its own documentation says "For production, use 2+ replicas", and bindy fans
each zone operation out to **every** endpoint (`stream::iter(endpoints.iter())`
at `dnszone.rs:1146` and `:1517`). A zone must land on all replicas of an
instance, not on whichever one a Service happened to pick. Routing through the
Service ClusterIP would load-balance a single request to a single pod and
silently leave the others without the zone.

So the transport is `https://<pod-ip>:<port>`, and **a certificate cannot
practically carry a SAN for an ephemeral pod IP**. Pods are rescheduled and
renumbered; cert-manager cannot know the address in advance. Standard TLS
hostname verification therefore cannot succeed as things stand.

This is the decision this ADR exists to make. Everything else — field names,
mounting a Secret, setting env vars — is mechanical once it is settled.

## Options considered

### A. Connect via the Service DNS name

Issue the certificate for `<instance>-api.<ns>.svc.cluster.local` and dial that
instead of pod IPs. Full standard verification, no custom code.

**Rejected.** It breaks the multi-replica semantics described above. A zone
applied through a ClusterIP reaches one replica; the others diverge silently,
which is a correctness regression traded for a transport improvement. It would
also mask the "no ready endpoints" error `get_endpoint` raises today behind a
connection refusal.

### B. Issue a certificate per pod, with the pod IP as a SAN

Correct in principle, and what SPIFFE/SPIRE does properly.

**Rejected for now.** cert-manager cannot issue for an address that does not
exist yet, so this needs an init container or a sidecar agent obtaining a
certificate at pod start, plus a CA that will sign for IP SANs on demand. That
is a much larger piece of infrastructure than the problem justifies today, and
it is the natural path if a mesh is ever adopted (see E).

### C. Pin trust to a CA; do not verify the hostname

Verify that the peer presents a certificate chaining to a CA bundle the operator
is configured with, and skip the SAN check against the pod IP.

This keeps the properties that actually matter here:

- the connection is encrypted, so the bearer token is no longer readable on the
  wire — which is the entirety of P2-4;
- the peer must hold a key signed by a CA the platform team controls, so an
  arbitrary pod cannot impersonate a sidecar.

What it gives up is binding the certificate to a specific address. Within a
cluster where the CA is private to the platform and only used to issue sidecar
certificates, an attacker who can obtain a certificate from that CA has already
defeated the control regardless of the SAN.

**This is a real trade-off and must be documented as one, not hidden.** It is
not "TLS with verification"; it is "encrypted, with peer authentication by CA
and no address binding".

### D. Mutual TLS, same verification model as C

As C, plus the operator presents a client certificate so the sidecar can
authenticate it, and the bearer token stops being the only credential.

**Recommended as the eventual target**, layered on C rather than instead of it.
bindcar's `BIND_TLS_CLIENT_CA` already implements the server half.

### E. Rely on a service mesh

Linkerd provides automatic mTLS between meshed pods, which solves transport
confidentiality without either side configuring anything.

**Not a substitute.** It covers mesh-internal traffic only, and bindy must work
on clusters without a mesh. Where a mesh *is* present, bindcar TLS is defence in
depth and the two compose fine. This is the standard recommendation and should
be documented as the preferred deployment, with C as the portable fallback.

## Decision

Adopt **C**, with the surface shaped so that **D** and **B** are additive later
and so the trade-off is impossible to adopt by accident.

1. TLS is **opt-in** per instance/cluster/provider. Default behaviour is
   unchanged plaintext.
2. When enabled, the operator mounts a Secret into the sidecar and sets
   `BIND_TLS_CERT` / `BIND_TLS_KEY`, and dials `https://`.
3. The operator verifies the sidecar's certificate against a **configured CA
   bundle**. There is no "trust the system roots" mode: a private CA is the
   only sensible source here, and falling back to public roots would be a
   silent weakening.
4. Hostname verification is **off by default** because the peer is dialled by
   IP, and this is stated in the field documentation rather than buried.
5. An optional `serverName` re-enables full verification for deployments that
   can issue certificates covering a stable name — the upgrade path to B.
6. **No `insecureSkipVerify` escape hatch.** Encryption without peer
   authentication would let anything on the pod network impersonate a sidecar
   and collect tokens, which is worse than today's honest plaintext because it
   looks secure. If the CA bundle is missing or unreadable, the operator must
   refuse to use TLS rather than fall back.

## Consequences

**Good**

- P2-4 is **remediated and verified end to end**: the operator reaches the
  sidecar over HTTPS, so the ServiceAccount token it presents is no longer
  written to the pod network in cleartext.

  The evidence is `tests/tls_transport_test.sh`, run against kind with
  cert-manager issuing the sidecar certificate. It asserts that a real
  certificate was issued, that the sidecar serves TLS and **refuses plaintext on
  the same port**, that the operator pushed a zone over a CA-verified connection
  and made no plaintext calls, and that a half-configured instance is refused
  rather than downgraded.

  Two earlier claims in this repository were wrong, and are recorded here so the
  correction is not lost:

  1. An earlier revision of this section stated flatly that P2-4 was remediated.
     It was not: the plumbing existed but nothing had watched a real handshake.
  2. Correcting that to "implemented, not yet proven" was still too generous.
     The per-instance TLS client had been wired into the **record** reconciler
     only. Zone operations — `addzone`, `freeze`, `thaw`, `notify`, the bulk of
     the API surface — used the process-wide `Bind9Manager` built at startup,
     which has no TLS configuration and no client for reading a CA bundle, and
     so dialled `http://` with the token attached. Every unit test passed
     throughout. Only the end-to-end run surfaced it.

  The lesson worth keeping: for a finding about what crosses the wire, a passing
  unit suite is not evidence. Only a real handshake is.
- The trust anchor is explicit and operator-controlled.
- Existing deployments are untouched — the fields are optional and default off.
- bindcar's certificate hot-reload means rotation costs no restart, which
  matters because restarting a sidecar restarts the BIND9 operand beside it.

**Bad, and accepted**

- A certificate issued by the configured CA is accepted from **any** address.
  The CA must therefore be dedicated to this purpose and not shared with a
  general-purpose issuer. This needs saying in the docs in those words.
- A custom `rustls` verifier is required, because reqwest has no "verify chain
  but not hostname" switch. Custom verifier code is security-sensitive and must
  be unit-tested for the cases that matter: wrong CA rejected, expired
  rejected, correct chain accepted.

**Follow-on**

- Mutual TLS (D) once the server side is in use.
- `serverName` plus per-pod certificates (B) if stable identities ever exist.
- The `bindcarConfig.envVars` override hole (bindy guide 57 §25) now also
  reaches `BIND_TLS_*`; a reserved-name guard is a prerequisite for trusting
  any of this, since a tenant that can set `BIND_TLS_CERT` can defeat it.

## Notes

`reqwest` is already built with `rustls-no-provider` and
`default-features = false`, so a custom `ServerCertVerifier` can be installed
without changing the TLS backend.
