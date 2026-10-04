# 0011: Cryptographic Bill of Materials (CBOM) per release

- **Status:** Proposed
- **Date:** 2026-10-04
- **Deciders:** Erick Bourgeois
- **Related:** Roadmap 28 Phase 0 (`.github/community/28-pqc-readiness.md`);
  extends [ADR-0010](0010-release-sbom-and-slsa-build-l3.md), whose SBOM
  pipeline this rides

## Context

Roadmap 28 (post-quantum cryptography readiness) starts from an inventory:
which cryptographic algorithms bindy ships, configures or depends on, and
which of them are quantum-vulnerable. That inventory exists today as a
hand-maintained table in the roadmap document. A table in a Markdown file is
not auditable evidence: it has no schema, no binding to a release, and no
way for a downstream consumer (a regulator, a scanner, Dependency-Track) to
ingest it.

CycloneDX 1.6 (ECMA-424) defines cryptographic assets as first-class
components (`type: cryptographic-asset` with `cryptoProperties`: algorithms,
protocols, certificates, related material) and crypto-aware dependency
semantics (`provides`). This is the CBOM format, and regulated-finance
guidance (DORA ICT risk, BSI TR-03183, NIST IR 8547 migration planning)
increasingly expects one.

Constraints found while evaluating tooling (2026-10-04):

1. **No crypto-asset scanner exists for Rust.** IBM's CBOMkit detects
   crypto usage in Java and Python; the system-level scanners target whole
   Linux hosts. Nothing walks a Rust dependency graph and emits
   cryptographic assets.
2. **The existing SBOM pipeline is CycloneDX 1.5** (cargo-cyclonedx 0.5.9,
   ADR-0010) and correct as it is. CBOM needs 1.6; upgrading the SBOM legs
   to 1.6 is independent work and not required, because a CBOM is a
   separate document, not a section of the SBOM.
3. bindy's crypto surfaces are few, stable and already enumerated
   (roadmap 28): DNSSEC signing algorithms rendered into `dnssec-policy`,
   TSIG/RNDC HMAC, rustls/ring TLS on the control plane, and the TSIG/RNDC
   secret material. They change when a dependency or a CRD surface changes,
   which is reviewable, not continuous.

## Decision

Ship a **curated CBOM** as a release artifact: `bindy-cbom.cdx.json`,
CycloneDX 1.6 JSON, one per release (platform-independent).

### 1. Source of truth is a checked-in template

`cbom/bindy-cbom.template.cdx.json` holds the cryptographic assets:

- **Algorithm assets** for every algorithm bindy ships or configures,
  each with `primitive`, `parameterSetIdentifier`, OID where one exists,
  `classicalSecurityLevel` and `nistQuantumSecurityLevel` (0 for the
  quantum-vulnerable asymmetric algorithms), and `cryptoFunctions`.
- **Protocol assets** for TLS as used on the control plane.
- **Related-crypto-material assets** for the TSIG/RNDC shared secrets.
- A `firestoned:bindy:surface` property on each asset naming where it
  lives: `operator-binary` (compiled into bindy), `operand-config`
  (rendered into BIND9's configuration; BIND9's own OpenSSL executes it)
  or `control-plane-transport`.
- **`provides` dependencies** from the crypto libraries (ring, rustls,
  hickory-proto, rustls-webpki) to the assets they implement, and
  `dependsOn` from the root `bindy` component to those libraries.

Curation is a review duty, documented in `cbom/README.md`: a change to the
crypto surfaces (new algorithm in the CRD, a TLS provider change, a new
crypto dependency) updates the template in the same PR.

### 2. Generation stamps, it does not invent

`make cbom-generate` (via `scripts/cbom.sh generate`) produces
`sbom/bindy-cbom.cdx.json` from the template by injecting only build facts:
`serialNumber` (fresh UUID), `metadata.timestamp`, the release version, and
the **versions of the crypto libraries read from `Cargo.lock`**, so the
CBOM cannot claim a ring or rustls version the build did not use. If a
library named in the template is missing from the lockfile, generation
fails: that is the drift alarm for a crypto dependency being dropped or
renamed without a template update.

### 3. A quality gate, like the SBOMs have

`make cbom-check` (via `scripts/cbom.sh check`) fails unless the document
has: CycloneDX ≥ 1.6, serial number, timestamp, tool, supplier, a versioned
root component, at least one cryptographic asset, `cryptoProperties` with
`assetType` on every crypto asset, `primitive` and `nistQuantumSecurityLevel`
on every algorithm asset, a `firestoned:bindy:surface` property on every
crypto asset, and a dependency graph containing at least one `provides`
relationship. `make cbom-stage` chains generate and check, mirroring
`sbom-stage`.

### 4. Delivery rides the ADR-0010 rails

A `cbom` job in `build.yaml` runs `make cbom-stage` and uploads the result
as artifact `sbom-cbom`. The existing release collection
(`sbom-*/*.cdx.json`) and SLSA provenance subject globs (`sbom-*`) pick it
up with no changes: the CBOM ships as a release asset and is a subject of
the SLSA generic provenance, which binds it to the release. The `cbom` job
is required by the `ci-gate` job, so a PR that breaks the CBOM (including
one that drops a crypto dependency from `Cargo.lock` without a template
update) fails before merge; the scheduled SBOM workflow regenerates it
daily on `main` as a drift check. Workflow steps only call Makefile
targets, per the repo's workflow rules.

### 5. Documentation

`docs/src/security/pqc-readiness.md` renders the inventory for humans:
the same assets as the CBOM, the migration posture per surface, and the
roadmap 28 watch items. The CBOM is the machine-readable half; the page is
the readable half, and both change together under the curation duty.

## Consequences

**Good**

- The PQC inventory becomes schema-valid, release-bound, ingestible
  evidence instead of a Markdown table; Phase 0 of roadmap 28 is auditable
  output on its own.
- `Cargo.lock` injection means the CBOM's library versions are facts of
  the build, and a vanished crypto dependency fails the release.
- No new trust surface: delivery, provenance and PR gating reuse ADR-0010
  machinery unchanged.

**Bad**

- The asset list itself is curated, so an entirely new crypto surface that
  reviewers miss is absent from the CBOM until caught. The gate catches
  dropped dependencies, not unlisted new ones; the `cbom/README.md` review
  duty and roadmap 28's revisit cadence are the mitigations.
- One more release job and artifact.
- CycloneDX 1.6 consumers are still uneven; some SBOM tooling will ignore
  the crypto assets. Acceptable: the document validates against the
  schema, and consumers catch up.

**Rules out**

- Claiming the CBOM is exhaustive discovery. It is a declared inventory
  with build-fact injection, and the docs say so.
- Folding crypto assets into the binary SBOMs: that would couple the SBOM
  legs to a 1.6 migration they do not need.

**Follow-ups**

- Per-artifact `attest-sbom` binding for the CBOM (predicate
  `https://cyclonedx.org/bom`) once the one-CBOM-to-many-artifacts
  subject mapping is settled; provenance-subject coverage suffices now.
- Revisit scanner-based generation if Rust CBOM tooling appears
  (roadmap 28 watch cadence).
- CALM is unchanged: the runtime architecture does not change. The threat
  model gains a mitigation row for the crypto inventory in the same
  change.
