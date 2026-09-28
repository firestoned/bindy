# 0007 — Uniform options rendering: honor explicit dnssec-validation, deny transfers by default at cluster level

- **Status:** Accepted
- **Date:** 2026-09-27
- **Deciders:** Erick Bourgeois
- **Related:** Closes the two behavior quirks deliberately preserved by
  roadmap 03 (`.github/community/03-early-return-refactoring.md`); completes
  the #466 deny-by-default work

## Context

The roadmap-03 early-return refactor of `src/bind9_resources.rs` was
behavior-preserving by design, and it documented (and pinned with tests) two
pre-existing asymmetries in how `named.conf.options` is rendered, deferring
the fix/keep decision to a deliberate change. Both are now measured against
what BIND 9.18 — the operand image — actually does when a directive is
absent:

**Quirk 1 — `dnssec-validation` asymmetry.** With no instance `config` block
at all, a cluster-global `dnssec.validation: false` emitted *no* directive,
while an instance config block merely silent on dnssec rendered an explicit
`dnssec-validation no;` from that same global value. Per the BIND 9.18 ARM,
an absent `dnssec-validation` statement defaults to **`auto`** — validation
ON with the built-in root trust anchor. So for the no-config-block case the
operator silently **inverted** the user's explicit `validation: false` into
validating behavior, purely as a side effect of whether an unrelated config
block existed.

**Quirk 2 — no transfer deny-by-default at cluster level.** The
instance-level builder renders `allow-transfer { none; };` when no ACL is
configured at any level (#466). The cluster-level builder
(`build_cluster_options_conf`) did not: no global ACL meant no directive.
In BIND 9.18 an absent `allow-transfer` **allows zone transfers to any
host** — the upstream deny-by-default only landed in BIND 9.20 (GL #3567).
A cluster-level ConfigMap without an explicit ACL therefore left AXFR open —
zone enumeration exposure (threat model I2) in a codebase that already
decided deny-by-default at the instance level.

## Decision

Fix both. Rendered options are **uniform across the two builders**, and
explicit configuration is **always honored**:

1. `resolve_dnssec_validation` renders from the cluster-global `dnssec`
   block whenever it is set, whether or not the instance has a `config`
   block. Instance-level `dnssec` still overrides global; when neither level
   configures `dnssec`, no directive is emitted (named's `auto` applies).
2. `build_cluster_options_conf` uses the same `render_allow_transfer`
   fallback chain as the instance builder: an explicit global ACL renders it,
   an explicitly empty list renders `none`, and no ACL at all renders
   `allow-transfer { none; };`.

Zone transfers to legitimate secondaries are unaffected: per-zone
`allow-transfer` ACLs (set by bindcar with the secondaries' IPs) override the
options-level default, exactly as they already do under the instance-level
deny-by-default.

The roadmap-03 pinning tests that locked in the old behavior are inverted to
pin the new behavior, per the TDD-first rule.

## Consequences

- **Breaking (behavioral), needs migration notes:** a deployment that relied
  on named's implicit defaults changes behavior. (a) Global
  `dnssec.validation: false` with no instance config block now actually
  disables validation — which is what the manifest said. A deployment that
  *wanted* validation must say `validation: true` (or omit `dnssec`
  entirely). (b) A cluster-level options file with no `allow_transfer`
  anywhere now denies AXFR; deployments doing ad-hoc transfers against
  cluster-configured operands must configure the ACL explicitly.
- The AXFR exposure window on BIND 9.18 operands is closed operator-side
  instead of waiting for a BIND 9.20 image bump, and stays closed if the
  image ever moves backward.
- The `resolve_dnssec_validation` special case (and its rustdoc paragraph
  explaining the asymmetry) disappears — less precedence logic to hold in
  mind.
- No CALM change: both fixes are rendering logic inside the existing
  operator → ConfigMap flow; no node, relationship, or interface changes.
