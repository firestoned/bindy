# 29: Validate and render BIND9 configuration with hornet

> **Goal.** A BIND9 configuration `named` cannot parse never reaches
> `named`, and every value from a CRD reaches `named.conf` quoted or escaped
> by construction, by adopting [hornet](https://github.com/firestoned/hornet)
> (`hornet-bind9`) for validation and rendering
> ([ADR-0013](../../docs/adr/0013-validate-and-render-bind9-config-with-hornet.md)).
>
> **Stop condition.** CI parses every rendered configuration (stage 1); the
> operator refuses to publish one that does not parse and reports
> `ConfigurationInvalid` (stage 2); `named.conf` and `named.conf.options` are
> written by hornet's writer and their templates are deleted (stage 3).

**Status:** 🔶 Stages 1 and 2 done 2026-10-05; stage 3 waits on hornet 0.3.0.
**Owner:** Erick Bourgeois

## Why

bug-177: a template substitution left a stray `}` in `named.conf.options`,
and `named` exited at startup on every pod that mounted the ConfigMap. The
templates are 34 plain string substitutions and nothing parsed the result
before `named` did.

## Stage 1: every rendered configuration parses, in tests

- [x] `hornet-bind9` in the workspace (`default-features = false`).
- [x] `crates/bindy-bind9/src/rendered_config_tests.rs`: the instance and
      cluster builders across the option matrix, and every `Bind9Instance` /
      `Bind9Cluster` in `examples/`, parse with no Error-severity finding.
      *Found `examples/bind9-cluster-with-rotation.yaml` (and seven docs
      pages) writing `role: Primary`, which the CRD rejects; fixed.*

## Stage 2: the operator refuses an invalid configuration

- [x] `bindy_bind9::config_check`: parse and validate every `named.conf*`
      file before a ConfigMap builder returns; Error-severity findings and
      parse errors fail the build, warnings are logged.
- [x] `Bind9Instance` and `Bind9Cluster` report `Ready=False`, reason
      `ConfigurationInvalid`, `Configuration not published: …`; the pods keep
      the last published configuration. *A separate `ConfigValid` condition
      (the ADR's first draft) would be erased by the status writers, which
      rebuild the condition list on every update, so the existing `Ready`
      condition carries it.*
- [x] Docs: `operations/common-issues.md`, `operations/status.md`.

## Stage 3: render through hornet's writer

Blocked on hornet 0.3.0, which must:

- [ ] accept `print-time iso8601`, `iso8601-utc` and `local` (0.2.0 falls
      back to a raw block for bindy's `logging`);
- [ ] model `dnssec-policy` as a typed statement (keys with role, lifetime
      and algorithm; the timing options bindy sets);
- [ ] model `allow-new-zones` and `key-directory` as typed options;
- [ ] keep `miette`'s `fancy` terminal rendering out of library builds
      (only the CLI needs it).

Then in bindy:

- [ ] Build `named.conf` / `named.conf.options` as a hornet tree and write it
      with hornet's writer; delete `templates/named.conf.tmpl` and
      `templates/named.conf.options.tmpl` (`rndc.conf.tmpl` stays).
- [ ] Release note: the rendered text changes, so every BIND9 Deployment
      rolls once after the upgrade (PodDisruptionBudgets honoured).
- [ ] Threat model pass (new injection control).
