# 0028: Scout routes follow the addresses of their Gateway

- **Status:** Accepted
- **Date:** 2026-10-24
- **Proposed:** 2026-10-24
- **Deciders:** Erick Bourgeois
- **Related:** Fixes issue #569; Decision 1 is adopted from PR #570 (Daniel Guns); amends [ADR-0025](0025-scout-records-bound-to-cluster-and-zones.md) Decision 2 for discovered addresses; builds on [ADR-0026](0026-scout-one-record-per-dns-name.md) (a source's addresses are one contribution to a per-name record) and [ADR-0022](0022-scout-sources-share-one-pipeline.md) (one pipeline for every source kind)

## Context

A Gateway API route (`HTTPRoute`, `GRPCRoute`, `TLSRoute`, `TCPRoute`) has no
load-balancer status of its own. When it has no `bindy.firestoned.io/ip`
annotation, Scout follows its `parentRefs` to the serving Gateway and
publishes the Gateway's `status.addresses`, or, when those are empty, the
addresses of the `LoadBalancer` Service that `--gateway-service` maps the
Gateway's class to. A target is either `namespace/name` (one Service) or
`namespace/<label-selector>`.

Issue #569: a label-selected target stopped at the **first** matching Service
with an address, so with the common one-Service-per-VIP layout each route
published one VIP of several. PR #570 changed that to the union of every
matching Service. Review of that change, and of how routes resolve addresses
at all, found more:

1. **Routes never re-resolve.** A route reconciles on its own changes only.
   A gateway Service that gets its address later, is added, deleted or
   readdressed, or a Gateway whose `status.addresses` changes, is not seen
   until the route happens to reconcile again. With the union, a route
   reconciled while one of three Services was still pending keeps two of
   three addresses indefinitely. The PR left this out believing it needed new
   RBAC; Scout's ClusterRole already grants `get`, `list` and `watch` on
   `gateways` and runs a cluster-wide Service watch.
2. **One class, several Gateways.** The target is per class. Implementations
   that deploy one `LoadBalancer` Service per Gateway (Envoy Gateway, Cilium,
   Istio) label them alike, so a class-wide union publishes every Gateway's
   VIPs for every route, and part of the clients reach a Gateway that has no
   listener for the host. Gateway API GEP-1762 (Standard) requires
   implementations to label every resource they generate for a Gateway with
   `gateway.networking.k8s.io/gateway-name: <gateway>`.
3. **One Service can block a class.** With `--allowed-ip-ranges` set, a
   single matched Service whose address is outside the ranges makes every
   route of the class publish nothing (ADR-0025 Decision 2 refuses the whole
   source), and anyone who can create a labelled `LoadBalancer` Service in the
   gateway namespace can do that.
4. **Terminating Services are unioned.** A Service held by its load-balancer
   finalizer still lists its address, which is about to be released.
5. **A LIST per reconcile.** Each route reconcile lists the gateway
   namespace's Services from the API server, once per parentRef, although
   Scout already holds every Service in its watch cache.
6. **Order churn.** A load-balancer controller that reorders
   `status.loadBalancer.ingress`, or a route that reorders its parentRefs,
   changes the source's contribution and rewrites the record although the
   address set is the same.

## Decision

A route publishes the addresses its Gateway serves, scoped to that Gateway,
read from Scout's caches, and re-resolved whenever the Gateway or one of its
Services changes.

### 1. A label-selected target publishes every matching Service

A `namespace/<label-selector>` target resolves to the union of the external
IPv4 addresses of every matching `LoadBalancer` Service that is not being
deleted and has an address (adopted from PR #570). `namespace/name` targets,
Gateways with `status.addresses`, and the precedence annotation → discovered
→ `--default-ips` are unchanged.

### 2. Scoped to the Gateway being resolved

When any matched Service carries `gateway.networking.k8s.io/gateway-name`
(GEP-1762), only matched Services whose value equals the name of the Gateway
being resolved count; Services generated for other Gateways of the class are
left out. When none carries the label (one Service shared by every Gateway
of the class, as with Traefik), every match counts. The label holds a name,
not a namespace: two Gateways of one name in different namespaces whose
Services one selector matches are not told apart; the docs say to keep such
selectors to one namespace's Gateways.

### 3. Read from the watch cache

Gateway Services are read from Scout's Service watch cache (namespace and
label selector, or name), not listed from the API server on each reconcile.
Gateways are read from a new Gateway watch cache (see Decision 4) instead of
a GET per parentRef.

### 4. Routes re-resolve when their Gateway or its Services change

Scout watches Gateways (the existing ClusterRole already allows it) and wakes
routes from two more events, with no timer:

- a **Gateway** change (its `status.addresses`, its class) wakes the routes
  whose `parentRefs` name it;
- a **Service** change (created, readdressed, being deleted, deleted) that a
  configured `--gateway-service` target matches wakes the routes whose parent
  Gateway's class maps to that target.

Both mappings read only the watch caches. A Service that gets its address
after a route first reconciled is therefore published when it does, and a
VIP removed from a Service leaves DNS when it is removed.

### 5. Discovered addresses outside the allowed ranges are dropped

Amends ADR-0025 Decision 2 for discovered addresses only. An address read
from a Gateway's `status.addresses` or a gateway Service that is outside
`--allowed-ip-ranges` is dropped with a warning naming the Service or
Gateway; the source publishes the rest, or nothing when nothing is left.
Addresses an operator wrote (the `bindy.firestoned.io/ip` annotation,
`--default-ips`) still refuse the whole source, as before: those are a
configuration error, while a discovered address belongs to an object other
people may create.

### 6. A contribution is normalized

Before Scout writes a source's contribution (ADR-0026 Decision 2), its
addresses are sorted numerically and de-duplicated, for every source kind. A
reordered status or parentRef list then writes nothing.

## Consequences

- Issue #569 is fixed, and a route's record follows its Gateway's VIPs as
  they come and go, instead of whatever was true when the route last changed.
- Behavior change on upgrade: a selector that matches several `LoadBalancer`
  Services now publishes all of their addresses, not the first one's. The
  Traefik chart labels every Service it creates `app.kubernetes.io/name:
  traefik`, including extra internal ones (`service.additionalServices`), so
  the documented example selector would publish an internal VIP. The example
  becomes VIP-specific, and the migration guide gives a pre-check.
- Scout runs one more watch (Gateways, cluster-wide), already permitted. Route
  reconciles make no Service LIST and no Gateway GET.
- A Service event in a namespace that a target names wakes every route of
  that target's classes: a burst of Service churn in a busy gateway namespace
  reconciles those routes once each per event, from the caches.
- With `--allowed-ip-ranges`, a stray labelled Service no longer takes a
  gateway class off the air; its address is dropped and logged. The ranges
  still decide what can be published.
- Gateways of the same name in different namespaces behind one selector are
  not separated (Decision 2); recorded in the threat model.
