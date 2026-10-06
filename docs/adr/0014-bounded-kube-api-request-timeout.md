# 0014: Bounded client-side timeout for non-watch Kubernetes API requests

- **Status:** Accepted
- **Date:** 2026-10-05
- **Deciders:** Erick Bourgeois
- **Related:** Follow-up to [ADR-0005](0005-client-side-kube-api-rate-limiting.md) (the same tower middleware stack); prompted by the v0.8.0-rc.2 load test (finding 2)

## Context

During the v0.8.0-rc.2 load test on a real cluster, five ordinary (non-watch)
Kubernetes API calls from the operator each took about 290 seconds. They were
all on one stalled connection; the API server's own logs and metrics showed
nothing slow. One dead connection froze five reconciles for about five
minutes, and the existing retry and backoff (`retry.rs`, the per-object
reconcile backoff) never got a chance to act, because the call never failed.

What kube-rs 4.2 (the version in `Cargo.lock`) actually bounds:

- `kube::Config` has `connect_timeout` (default 30 s), `write_timeout`
  (default 295 s) and `read_timeout` (default **`None`**)
  (`kube-client-4.2.0/src/config/mod.rs`: `Config::new`,
  `incluster_with_uri` and `new_from_loader`; constants `DEFAULT_CONNECT_TIMEOUT` and
  `DEFAULT_WRITE_TIMEOUT`).
- These are **connection-level** timers: `make_generic_builder` in
  `kube-client-4.2.0/src/client/builder.rs` wraps the connector in
  `hyper_timeout::TimeoutConnector` and sets all three on it. A read timeout
  there is an idle timer on every pooled socket, shared by every request the
  pool multiplexes onto it, watches included.
- The `read_timeout` rustdoc says it defaults to `None` "to avoid breaking
  long-lived connections such as exec, attach and port-forward sessions.
  Watch streams are protected by a watcher-level idle timeout instead."
  That idle timeout is `next_with_idle_timeout` in
  `kube-runtime-4.2.0/src/watcher.rs`: the server-side `timeoutSeconds`
  (default 290, `kube-core-4.2.0/src/params.rs`) plus a 5 s margin.

So nothing bounds how long a non-watch request waits for its response, apart
from socket-level timers that are hundreds of seconds long. That matches the
observed stalls.

Options considered:

1. **Set `Config::read_timeout`.** One line, but it is an idle timer on the
   shared socket, not a per-request deadline. Any value short enough to help
   would also cut a quiet watch stream (a watch with no events between
   bookmarks), forcing needless re-lists, and it does not bound a request
   that trickles bytes slowly. Rejected.
2. **A second client for watches.** The shared watch layer (ADR-0009) would
   need its own client, plumbed through `Context`, with two connection pools
   and two rate limiters (or one shared limiter across two stacks). More
   moving parts for the same result. Rejected.
3. **A per-request deadline layer in the existing tower stack that exempts
   watch requests.** Selected.

## Decision

1. **A `RequestTimeoutLayer` in the operator's client stack**
   (`bindy-controller-sdk/src/request_timeout.rs`), added through
   `ClientBuilder::with_layer` next to the ADR-0005 layers. For every request
   that is not a watch, it starts one deadline when the request is
   dispatched and enforces it across the whole exchange: waiting for the
   response headers and reading the response body. When the deadline passes
   the request fails with `RequestTimeoutError` and the stalled exchange is
   dropped.
2. **Watches are exempt.** A request whose query carries `watch=true` (or
   `watch=1`) passes through untouched. kube-rs sends exactly `watch=true`
   for every watch (`WatchParams::populate_qp`), and those streams are
   already bounded by the server-side `timeoutSeconds` and the watcher's own
   idle timeout. The operator does not use exec, attach, port-forward or
   followed log streams, which are the other long-running request kinds.
3. **Placement.** The deadline layer is the innermost of the bindy layers:
   inside the ADR-0005 rate limiter, so time spent queued for a rate-limit
   slot does not count against the deadline, and inside the metrics layer,
   so a timed-out request is recorded as an error with its real duration.
4. **Default and override.** `KUBE_CLIENT_REQUEST_TIMEOUT_SECS = 30` in
   `bindy-api/src/constants.rs`, overridable per deployment with
   `BINDY_KUBE_REQUEST_TIMEOUT_SECS`, parsed exactly like `BINDY_KUBE_QPS`:
   an unset, unparsable or zero value keeps the default with a warning.
   30 s is the longest an admission webhook may take (the API server caps a
   webhook's `timeoutSeconds` at 30) and the kube-rs connect timeout;
   ordinary bindy requests (GET, PATCH, server-side apply, 100-item list
   pages) complete in well under a second.
5. **Retryable.** kube-rs maps an error from a middleware layer to
   `kube::Error::Service`, both for the response future
   (`Client::send`) and for the body (`client/body.rs`). `retry.rs`'s
   `is_retryable_error` already treats `Service` errors as transient, so a
   timeout feeds straight into the existing exponential backoff; a reconcile
   that fails on it is requeued by the per-object reconcile backoff. No
   classification change was needed; a test pins this behaviour.

CALM: the operator to API server relationship description in
`calm/bindy-control-plane.architecture.json` lists the client-side controls
of ADR-0005; it now also names the request deadline. No node, interface,
protocol or relationship is added or removed.

## Consequences

- A stalled connection costs a request at most 30 s (by default) instead of
  about 290 s, then the request is retried on a fresh connection under the
  existing backoff. The rc.2 scenario (five reconciles frozen for about five
  minutes) becomes five failed attempts of 30 s each, retried.
- A legitimately slow request longer than the deadline now fails and is
  retried instead of completing. Operators with very slow admission chains
  can raise `BINDY_KUBE_REQUEST_TIMEOUT_SECS`. bindy's writes are patches and
  server-side applies, so a retry after a timeout whose write did land is
  idempotent.
- Timed-out requests appear in `bindy_firestoned_io_kube_api_requests_total`
  with `status="error"`. A body-phase timeout surfaces as the request's error
  but is not counted again by the metrics layer, which records at the
  response headers.
- `http-body` becomes a direct dependency of `bindy-controller-sdk` (already
  in the tree through kube and hyper) for the deadline-enforcing body wrapper.
- Scope is the operator's client (`bindy run`). Scout builds its own clients
  (`bindy-scout/src/scout.rs`) and the bootstrap CLI uses
  `Client::try_default`; neither carries this layer yet. Revisit when Scout
  is load-tested at scale.
- If kube-rs later ships a per-request timeout that exempts watches, this
  layer can be replaced by it.
