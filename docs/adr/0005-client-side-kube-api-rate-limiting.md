# 0005 — Client-side Kubernetes API rate limiting via tower middleware

- **Status:** Accepted
- **Date:** 2026-09-27
- **Deciders:** Erick Bourgeois
- **Related:** Completes roadmap 05 (`.github/community/05-kubernetes-api-rate-limiting.md`)

## Context

The operator talks to the Kubernetes API server through a single shared
`kube::Client`. Watches keep the steady-state request rate low, but
reconciliation storms — a rollout touching every `Bind9Instance`, a namespace
resync after a reflector restart, hundreds of `DNSZone`s requeuing at once —
produce request bursts bounded only by the API server's own server-side
throttling (HTTP 429). In a regulated multi-tenant platform, the operator must
bound its own load rather than rely on being throttled.

Two halves of the mitigation already landed under roadmap 05:
`reconcilers/pagination.rs` (paged LIST, `KUBE_LIST_PAGE_SIZE = 100`) and
`reconcilers/retry.rs` (exponential backoff for transient 429/5xx errors).
The missing piece is the sustained-rate cap itself. The constants
(`KUBE_CLIENT_QPS = 20.0`, `KUBE_CLIENT_BURST = 30`) and the
`BINDY_KUBE_QPS` / `BINDY_KUBE_BURST` environment overrides exist and are
even documented — but the values were parsed, logged, and discarded:
`Client::try_from(config)` builds the default stack with no limiter.

Unlike client-go, kube-rs deliberately has no `qps`/`burst` fields on
`Config`; its extension point is the tower middleware stack, exposed since
kube 0.90 as `kube::client::ClientBuilder::with_layer`.

## Decision

Bound the client at the transport layer with tower middleware, inserted via
`ClientBuilder`, in a new `src/rate_limit.rs` module:

1. **Rate limiting:** `tower::limit::RateLimitLayer::new(burst, burst / qps)`
   — a windowed approximation of client-go's token bucket. Allowing `burst`
   requests per `burst / qps` seconds yields the configured sustained QPS
   while letting short bursts up to `burst` through unthrottled. Requests
   beyond the window queue on the client (backpressure), they are not
   rejected.
2. **Configuration:** defaults from `constants.rs` (20 QPS / 30 burst,
   deliberately conservative for a controller sharing the API server with
   tenant workloads), overridable per deployment via `BINDY_KUBE_QPS` and
   `BINDY_KUBE_BURST`. Invalid or non-positive overrides are rejected with a
   warning and fall back to the defaults — a misconfigured limiter must never
   disable the operator or the limit.
3. **Observability:** a companion metrics middleware in the same stack counts
   every API request (`bindy_firestoned_io_kube_api_requests_total`), times it,
   and counts server-side throttles (HTTP 429,
   `..._kube_api_rate_limit_hits_total`), labelled by HTTP verb and the
   resource plural parsed from the request path. `retry.rs` and
   `pagination.rs` record retries and page counts into the same registry.
4. **Scope:** the limiter wraps the one client built by
   `initialize_services()`, so every reconciler, reflector, watch, and CLI
   subcommand sharing that client is bounded. Watches pass through the
   limiter once per (re)connect, which is negligible.

The event-driven architecture is unchanged: no polling was added, adaptive
requeue intervals stay as they are.

## Consequences

- The operator's API load is bounded client-side; 429s become a signal of
  misconfiguration (limit set above the server's tolerance) instead of the
  normal throttling mechanism, and are now visible in Prometheus.
- Under a sustained burst larger than the budget, requests queue: worst-case
  reconciliation latency grows by the queueing delay instead of failing.
  The limit applies per operator replica; replicas each get their own budget
  (leader election keeps only one active reconciler, so this does not
  multiply effective load).
- `tower` becomes a direct dependency (it was already in the tree via kube).
- The rate-limit window is an approximation: a full token bucket would smooth
  refill continuously. Accepted — tower's limiter is maintained upstream,
  and the difference is immaterial at 20 QPS. Revisit if profiles show
  bursty window-edge behavior.
- Docs previously advertised defaults of 50 QPS / 100 burst; the constants
  (20 / 30) are the source of truth and the docs are corrected to match.
- Load-scale validation (1000+ resources) is deferred to roadmap 18
  (load-testing framework), which owns that harness.
