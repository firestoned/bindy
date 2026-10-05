# 0013: Validate and render BIND9 configuration with hornet

- **Status:** Accepted
- **Date:** 2026-10-05
- **Deciders:** Erick Bourgeois
- **Related:** Extends [ADR-0007](0007-uniform-options-rendering-deny-by-default.md) (options rendering); prompted by bug-177

## Context

The operator writes BIND9's configuration as text. `named.conf`,
`named.conf.options` and `rndc.conf` are templates in `templates/`, filled in
by 34 plain `{{...}}` string substitutions in
`crates/bindy-bind9/src/bind9_resources.rs`, and published in a ConfigMap
that every BIND9 pod of the instance or cluster mounts. Nothing parses the
result before `named` does.

That has failed in production. In bug-177 a substitution produced a stray `}`
in `named.conf.options`, and `named` exited at startup with `'}' expected`:
every pod mounting that ConfigMap crash-looped, and the zones they served went
dark, because the operator had already published the broken file. Unit tests
assert fragments of the rendered text, so a syntax error that only shows up in
the assembled file gets through.

[hornet](https://github.com/firestoned/hornet) (`hornet-bind9`, same
organisation, Apache-2.0, `unsafe` forbidden) parses, validates and writes
`named.conf`. Its writer quotes and escapes every modelled value for its
position. A spike on 2026-10-05 rendered the instance and cluster ConfigMaps
for every manifest in `examples/` and parsed each config file with hornet
0.2.0:

- every `named.conf` and `named.conf.options` parses;
- the only validation finding is a Warning, `dnssec-validation is enabled but
  recursion is disabled`, which is the normal shape of an authoritative-only
  server;
- two blocks fall back to hornet's raw `Unknown` statement, so their inside
  is only brace-checked: `logging`, because hornet 0.2.0 accepts only
  `print-time yes|no` and bindy emits `print-time iso8601` (valid since BIND
  9.16), and `dnssec-policy`, which hornet does not model.

The spike also showed `examples/bind9-cluster-with-rotation.yaml` cannot be
applied: it writes `role: Primary` / `Secondary`, which the CRD rejects.

`rndc.conf` is not `named.conf` grammar (it is the `rndc` client's file) and
holds no user input; it is out of scope.

## Decision

Adopt hornet in three stages.

### 1. Every rendered configuration parses, in tests

`hornet-bind9` becomes a dev-dependency of `bindy-bind9`. Tests render
`named.conf` and `named.conf.options` through the real builders across the
option matrix (listen addresses, forwarders, ACLs, recursion, rate limits,
DNSSEC validation and signing, cluster and instance level) and for every
`Bind9Instance` and `Bind9Cluster` in `examples/`, and assert that each file
parses and has no Error-severity validation finding. A template edit that
breaks the assembled file fails CI. Every example must deserialize, which
catches invalid examples.

### 2. The operator refuses to publish a configuration that does not parse

Before creating or updating a BIND9 ConfigMap (instance or cluster), the
operator parses every `named.conf*` file in it with hornet and runs hornet's
validator. A parse error or an Error-severity finding means the ConfigMap is
**not written**:

- the reconcile returns an error, so the controller retries with its capped
  backoff;
- the owning `Bind9Instance` or `Bind9Cluster` reports `Ready=False` with
  reason `ConfigurationInvalid` and `Configuration not published: …` naming
  the file and hornet's message. (A separate `ConfigValid` condition would not
  survive: both status writers rebuild the condition list on every update, so
  the existing `Ready` condition carries it, and the next successful
  reconcile clears it.)
- the BIND9 pods keep running the last configuration that was published.

Warnings are logged at debug level and do not block. The check runs on text
the operator itself produced, so it costs one parse per reconcile that
renders a ConfigMap, which is cheap next to the API calls around it.

`hornet-bind9` becomes a runtime dependency of `bindy-bind9`, with
`default-features = false` (no `clap`). It brings `winnow`, `miette` and a
second major version of `thiserror`; cargo-deny allows duplicate versions with
a warning, and the SBOM lists them.

### 3. The configuration is rendered through hornet's writer

`named.conf` and `named.conf.options` are built as a hornet syntax tree and
written with hornet's writer instead of filled-in templates, so every value
from a CRD is quoted or escaped for its position by construction rather than
by each substitution remembering to. `templates/named.conf.tmpl` and
`templates/named.conf.options.tmpl` are retired; `rndc.conf.tmpl` stays.
The stage 2 check still runs on the written text.

This stage needs hornet to model what bindy emits. It is gated on a hornet
release (0.3.0) that:

- accepts `print-time` `iso8601`, `iso8601-utc` and `local`;
- models `dnssec-policy` (name, `keys` with role, lifetime and algorithm, and
  the timing options bindy sets) as a typed statement, so no CRD value goes
  through a raw carrier;
- models `allow-new-zones` and `key-directory` as typed options, or bindy
  passes only constants through hornet's `extra` raw carrier (both are
  constants today).

Until that release, stages 1 and 2 ship on hornet 0.2.0.

## Consequences

**Good**

- A configuration `named` cannot parse no longer reaches `named`: CI catches
  template regressions (stage 1), and a bad render at runtime leaves the
  running pods untouched and says why on the resource (stage 2). bug-177
  would have been a failed reconcile and a status condition, not an outage.
- Rendering stops depending on each substitution escaping correctly
  (stage 3). Today the CRD's regex patterns constrain the DNSSEC policy name,
  algorithm and lifetimes, so no injection is known; the writer makes that
  hold by construction rather than by schema alone.
- Broken examples are caught by the same tests.

**Bad**

- One more runtime dependency, maintained in-house. It parses only text the
  operator generated, never input from the network.
- Stage 3 changes the rendered text (formatting, and the explanatory comments
  in the templates go away). The ConfigMap's content hash changes, so **every
  BIND9 Deployment rolls once** after upgrading to the release that ships
  stage 3, honouring each instance's PodDisruptionBudget. That release's notes
  must say so.
- Stage 2 can block a configuration BIND9 would accept, if hornet is stricter
  than `named` about something. That fails closed (old config keeps serving)
  and shows on the resource; the fix is in hornet, or a hornet bump.
- A stage that is still waiting on hornet (stage 3) leaves `logging` and
  `dnssec-policy` brace-checked only, in stages 1 and 2.

**Follow-ups**

- hornet 0.3.0 with the gaps above; then stage 3.
- CALM: no change. The check runs inside the operator; no node,
  relationship or interface changes.
- Threat model: stage 2 is a new integrity control on the operator-to-BIND9
  configuration path; stage 3 a new injection control. Full pass when each
  ships.
