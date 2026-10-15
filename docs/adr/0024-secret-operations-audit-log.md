# 0024: Every Secret operation emits one structured audit event

- **Status:** Accepted
- **Date:** 2026-10-14
- **Proposed:** 2026-10-14
- **Deciders:** Erick Bourgeois
- **Amended:** 2026-10-14 (Decision 2: the wrapper is a `SecretApi` type and a test forbids any other `Api<Secret>`; Decision 3: the Secret watch becomes a metadata watch)
- **Related:** Implements roadmap 16 (approved 2026-03-09, never built); covers the Secret traffic [ADR-0023](0023-secretref-rndc-key-mirrored.md) added; RBAC posture as in the threat model's B-5 split

## Context

bindy runs in a regulated banking environment. Its operator and its
companion binaries read and write Kubernetes Secrets that hold key material:

| Where | Secret | Operations |
|---|---|---|
| instance controller | `{instance}-rndc-key` (generated, or the ADR-0023 mirror) | create, replace (mirror change, rotation), patch (rotation annotations), delete (recreate after an algorithm change, a mirror switching back to generated, instance deletion) |
| instance controller | the `rndcKey.secretRef` source (ADR-0023) | read |
| instance controller, drift check | `{instance}-rndc-key`, the source | read, on every reconcile, to compare |
| cluster controller | the instance's RNDC Secret | read, to check it exists |
| `bindy-bind9` key loader | the RNDC Secret a Deployment mounts | read, on a key-cache miss, before a TSIG-signed write or bindcar call |
| `bindy-bind9` TLS client | a bindcar CA bundle Secret | read |
| `bindy scout` | the remote kubeconfig Secret (ADR-0002) | read |
| `bindy bootstrap` | a ServiceAccount token Secret | read |

None of these produce an audit trail of their own. The operator logs
free-text `info!` lines for some writes and nothing for most reads; the
lines do not share fields, cannot be filtered from ordinary logs, and a
failed operation often logs nothing at the call site.

The Kubernetes API server audit log records every Secret request the
operator's ServiceAccount makes, and it stays the authoritative record of
*that a request happened*. It cannot say *why*: which instance a key belongs
to, whether a write was a rotation, a mirror following its source or a
recreate after drift, or which controller decision caused it. Investigators
need both, and a compliance review asks for the second.

Two properties of the current code also matter here:

- The drift check and the cluster controller read whole Secrets (key
  material included) only to learn whether they exist, what their
  annotations say, or (ADR-0023) whether a mirror still matches its source,
  which reads both Secrets' keys on every reconcile. Auditing those reads as key access would bury the real
  ones under one event per reconcile, and the reads are broader than they
  need to be.
- Logging is configured from `RUST_LOG` alone. Setting `RUST_LOG=warn`, a
  common way to quiet an operator, would silently drop anything logged at
  `info`.

## Decision

1. **One event per Secret operation, with fixed fields.** Every create,
   replace, patch, delete or read of a Secret's data emits exactly one
   `tracing` event on the target `bindy::audit`, after the API call returns,
   carrying:
   - `kind = "audit"`, `resource = "Secret"`;
   - `operation`: `create`, `replace`, `patch`, `delete` or `read`;
   - `outcome`: `success` or `failure`, with `error` (the API error's
     reason and code, never a request or response body) on failure;
   - `namespace`, `name`;
   - `purpose`: what the Secret is to bindy, from a closed set
     (`rndc-key`, `rndc-key-mirror`, `rndc-key-source`, `bindcar-ca`,
     `scout-kubeconfig`, `serviceaccount-token`);
   - `reason`: why the operation happened, from a closed set per purpose
     (`initial`, `rotation`, `mirror-sync`, `recreate-algorithm`,
     `recreate-malformed`, `mode-switch`, `instance-deleted`,
     `key-load`, `client-build`);
   - `actor`: the component (`bind9instance`, `bind9cluster`, `scout`,
     `bootstrap`, `bindy-bind9`);
   - `owner`: the object the operation serves (for example
     `Bind9Instance dns/primary-0`), when there is one.

   No field ever carries Secret data, a key, a token, or an annotation value
   other than bindy's own rotation timestamps and counts.
2. **One module, used through one wrapper.** `bindy_controller_sdk::audit`
   defines the event types and a wrapper that performs the API call and
   emits the event for whichever outcome it returns. Every Secret operation
   goes through it, so a call site cannot record success and forget failure.
   The closed sets are Rust enums, so a typo cannot create a new value.

   **Amended 2026-10-14:** the wrapper is a `SecretApi` type around
   `Api<Secret>`, whose data methods (`get`, `get_opt`, `create`, `replace`,
   `patch`, `delete`) each take the operation's purpose, reason and owner,
   and whose `get_metadata` returns no data and records nothing. No other
   non-test code constructs an `Api<Secret>`: a test scans the workspace and
   fails the build if one does. The only exception it allows is the
   metadata watch below.
3. **Existence and annotation checks read metadata only.** The drift check
   and the cluster controller's existence check use `get_metadata`, which
   returns no data. They are then not reads of key material and emit no
   audit event. The ADR-0023 staleness check becomes a metadata comparison
   too: the mirror records the source's `resourceVersion` in a
   `bindy.firestoned.io/rndc-source-version` annotation, and the mirror is
   stale when the source's current `resourceVersion` differs (or the
   annotation is missing, as on a mirror written before this change). The
   source's data is then read only when it changed. Every remaining `read`
   event is a real access to key material.

   **Amended 2026-10-14:** the instance controller's Secret watch, which
   maps an owned `{instance}-rndc-key` or a `secretRef` source to the
   instances it serves, needs only names, namespaces and owner references.
   It becomes a metadata watch (`metadata_watcher`), so the operator no
   longer receives the data of every Secret in the namespaces it watches. A
   watch stream cannot be audited per event; it now carries no key
   material to audit.
4. **Audit events cannot be filtered away by accident.** When `RUST_LOG`
   does not mention `bindy::audit`, the operator, Scout and bootstrap add
   the directive `bindy::audit=info` to whatever filter it gives, so quieting
   ordinary logs leaves the audit trail intact. An operator who wants it off
   must say `bindy::audit=off` explicitly. The events use the existing
   `RUST_LOG_FORMAT` (`json` for a SIEM); `kind = "audit"` and the target
   let a log pipeline route them to a separate sink.
5. **A failure counter for alerting.** Each event also increments
   `bindy_firestoned_io_secret_operations_total{operation, purpose,
   outcome}`, so a run of failed key loads or mirror writes can alert
   without parsing logs.

## Consequences

- Each Secret operation bindy performs can be answered with what, on which
  Secret, for which object, why, and whether it worked, joined to the API
  server audit log by namespace, name and time.
- Fewer Secret reads reach the API server with data in the response: the
  per-reconcile checks become metadata reads, and the Secret watch stops
  streaming every Secret's data to the operator (least privilege in
  practice; the RBAC grants are unchanged, since `get`, `list` and `watch`
  cover both).
- Log volume grows by one line per Secret write and per key-cache miss, not
  per reconcile; key loads are already cached (ADR-0015).
- `RUST_LOG=warn` no longer hides the audit trail. Anyone relying on that to
  silence it must use `bindy::audit=off`, which is visible in the
  Deployment spec.
- The closed `purpose` and `reason` sets must be extended in code when a new
  Secret use appears; an unlisted use does not compile. The add-new-Secret
  checklist (docs) says so.
- Not covered: Secrets that only Kubernetes reads on bindy's behalf (the
  kubelet mounting `{instance}-rndc-key` or a DNSSEC key Secret into a pod).
  The API server audit log covers those.
- CALM gains the audit event stream as a data flow from the operator, Scout
  and bootstrap to the cluster's log pipeline. No new trust boundary: the
  events go to stdout like every other log line.
