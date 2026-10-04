# Cryptographic Bill of Materials (CBOM)

`bindy-cbom.template.cdx.json` is the **curated source of truth** for the
cryptography bindy ships, configures or depends on: a CycloneDX 1.6 document
whose components are `cryptographic-asset` entries (algorithms, the TLS
protocol, the TSIG/RNDC secret material) plus the crypto libraries that
`provide` them. ADR-0011 records the decision; roadmap 28 Phase 0 is the
context.

No scanner maintains this file: there is no crypto-asset discovery tooling
for Rust. It is maintained **by review**, and that duty is the price of the
artifact.

## When you must update the template

In the same PR as the change itself:

- a new algorithm becomes configurable (CRD surface: `DnssecConfig.algorithm`
  guidance, `RndcAlgorithm` variants) or a default changes;
- the TLS stack changes: crypto provider (ring today; see the Cargo.toml
  comment and roadmap 28 Phase 2), new key-exchange groups, protocol
  versions;
- a crypto dependency is added, removed or renamed in `Cargo.toml`
  (ring, rustls, rustls-webpki, hickory-proto today);
- an algorithm's status changes (deprecation, removal, a PQC algorithm
  landing).

Keep `docs/src/security/pqc-readiness.md` in step: it is the human-readable
half of the same inventory.

## What gets stamped automatically

`make cbom-stage` writes `sbom/bindy-cbom.cdx.json` (gitignored, shipped as
a release asset) from this template. `scripts/cbom.sh generate` injects the
serial number, the timestamp, the release version, and the version of every
`pkg:cargo/*` library **from `Cargo.lock`**: a template library missing from
the lockfile fails the build, which is the drift alarm for a dropped or
renamed crypto dependency. `scripts/cbom.sh check` is the quality gate; it
runs in the `cbom` job on every PR (required by `ci-gate`) and daily in the
scheduled SBOM workflow.

## Conventions

- Every cryptographic asset carries a `firestoned:bindy:surface` property:
  `operator-binary`, `operand-config` (rendered into `named.conf`; BIND9's
  OpenSSL executes it), `control-plane-transport`, or `operand`.
- Every algorithm asset carries `nistQuantumSecurityLevel` (0 means
  quantum-vulnerable or no NIST category) and a
  `firestoned:bindy:pqc-exposure` property saying what that means for
  migration.
- Versions in the template are `0.0.0` placeholders by design; never
  hand-pin them.
