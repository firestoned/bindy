# 0029: Scout exposes Prometheus metrics

- **Status:** Proposed
- **Date:** 2026-10-26
- **Deciders:** Erick Bourgeois
- **Related:** Implements roadmap 27 item 1 (roadmap 12 open question 4); gives [ADR-0028](0028-scout-routes-follow-their-gateway-addresses.md) accepted risk 25 (c) the measurement its *Revisit when* waits for; reuses the operator's registry and conventions (`crates/bindy-controller-sdk/src/metrics.rs`)

## Context

Scout publishes every edge cluster's names, and it reports nothing but
logs. The operator serves Prometheus metrics on `:8080/metrics` (pod
annotations `prometheus.io/scrape`, port `metrics`); the `bindy scout`
process starts no metrics server, and its Deployment declares no port. The
counters Scout already increments through the shared watch layer
(`watch_events_total`, `watch_errors_total`, `watch_restarts_total`) are
registered and never served.

What an operator of Scout cannot see today without reading logs:

1. Whether Scout is publishing at all, and how long reconciles take.
2. Why a source publishes nothing. Each reason (no zone, a zone outside
   `--allowed-zones`, a zone not found or not granted, an invalid TTL or
   record name, no address, an address outside `--allowed-ip-ranges`) is a
   warning line per reconcile.
3. Whether writes to the bindy cluster succeed: a lost race (409), a refusal
   by admission policy 21 (ADR-0025), or an unreachable remote all look the
   same from outside.
4. Gateway effects of ADR-0028: discovered addresses dropped by the
   allowlist, and how often Gateway and Service events wake routes. Accepted
   risk 25 (c) is to be revisited "when route wake-ups show up in Scout's
   reconcile metrics", which do not exist.

Metric labels are public to anyone who can scrape the pod. Source names,
namespaces, hosts, zones and addresses are tenant data, and per-object labels
would grow without bound on a busy cluster.

## Decision

### 1. Scout serves the operator's metrics endpoint

`bindy scout` starts the same metrics server as the operator, on the same
address, port and path (`0.0.0.0:8080/metrics`, the `METRICS_SERVER_*`
constants), from the same registry. The Scout Deployment (`bindy bootstrap
scout` and `deploy/scout/deployment.yaml`, kept in sync by the drift test)
declares container port `metrics` 8080 and the operator's
`prometheus.io/scrape|port|path` pod annotations. No Service or
ServiceMonitor is shipped, as for the operator.

### 2. Scout metrics

Prefix `bindy_firestoned_io_scout_`. `kind` is the source kind (`Ingress`,
`Service`, `HTTPRoute`, `GRPCRoute`, `TLSRoute`, `TCPRoute`).

| Metric | Type | Labels | Counts |
|---|---|---|---|
| `reconciliations_total` | counter | `kind`, `outcome` | Every reconcile: `published`, `released` (opt-out, deletion, no longer publishable), `ignored` (not opted in, namespace excluded or not selected), `not_published`, `error` |
| `reconcile_duration_seconds` | histogram | `kind` | Reconcile wall time |
| `not_published_total` | counter | `kind`, `reason` | Each `not_published` outcome by reason: `zone_missing`, `zone_invalid`, `zone_not_allowed`, `zone_not_found`, `zone_forbidden`, `ttl_invalid`, `record_name_invalid`, `no_address`, `address_not_allowed` |
| `record_writes_total` | counter | `operation`, `result` | Each write to the bindy cluster: `operation` `create`, `replace`, `delete`, `unchanged` (no write needed); `result` `success`, `conflict` (409, retried), `refused` (403/422), `error` |
| `remote_last_success_timestamp_seconds` | gauge | none | Unix time of the last successful call to the bindy cluster, so an unreachable remote alerts on staleness |
| `gateway_addresses_dropped_total` | counter | `from` | Discovered addresses dropped by `--allowed-ip-ranges` (ADR-0028 Decision 5): `from` is `Gateway` or `Service` |
| `route_wakeups_total` | counter | `kind`, `trigger` | Routes woken by a `Gateway` or `Service` event (ADR-0028 Decision 4) |

The label values are closed sets, declared once as enums in Scout, so a
metric cannot gain a value by a typo or from tenant input.

### 3. No tenant data in labels

No Scout metric carries a namespace, object name, host, DNS name, zone,
cluster name or address. A metric tells an operator that something is
wrong and of what kind; the warning log line it accompanies says which
source.

### 4. The shared counters are served as they are

The watch-layer counters Scout already records are served unchanged (with
their `kind` and `namespace` labels, where `namespace` is the watch target:
`<all>` for Scout's cluster-wide source watches, the records namespace for
its DNSZone watch on the bindy cluster, never a tenant's namespace). The operator-only families are
registered on first use, so Scout's endpoint lists only what Scout touches.

## Consequences

- Scout can be alerted on: reconcile errors, write refusals, a stale remote,
  sources that cannot publish and why. Roadmap 27 item 1 is done; accepted
  risk 25 (c) becomes measurable.
- One more unauthenticated listener per edge cluster, as for the operator:
  its content is counts by closed label sets. A NetworkPolicy restricting
  scrapes stays the operator's planned control (M-17, M-27); the threat
  model records the port.
- The Scout Deployment changes (a port and annotations): `bindy bootstrap
  scout` updates it in place; with the `Recreate` strategy (ADR-0002
  addendum) that is one Scout restart.
- Ruled out: per-source gauges such as "sources published" by name (tenant
  data and unbounded cardinality); metrics for held-back conflicts, which
  the operator decides (ADR-0027) and which belong in the operator's
  metrics if wanted.
