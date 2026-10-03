# 0010: Release SBOMs and SLSA Build L3 provenance

- **Status:** Proposed
- **Date:** 2026-10-03
- **Deciders:** Erick Bourgeois
- **Related:** Roadmap 15 Phase 6 (`.github/community/15-security-scanning.md`);
  follows [ADR-0009](0009-workspace-crate-split-and-shared-watch-layer.md),
  whose virtual workspace exposed the SBOM path bug

## Context

bindy ships five binary tarballs, two container images (Chainguard and
Distroless, each multi-arch) and three install manifests per release. Its
docs claim "SLSA Level 3 ✅ Complete" with SBOMs on every release. Checked
against `.github/workflows/build.yaml` on 2026-10-03, that is not what ships:

1. **No binary SBOM reaches a release.** The Linux build legs run
   cargo-cyclonedx, which writes `<crate>.cdx.json` into the package
   directory. The upload step looks in `target/<triple>/release/*.cdx.json`,
   which never matches; the step passes because the binary matches its
   other path. The macOS and Windows legs generate no SBOM at all.
2. **The SBOMs that are generated are CycloneDX 1.3.** The pinned
   `firestoned/github-actions/rust/generate-sbom` action (v1.3.7) cannot pass
   `--spec-version`. Its `package` input passes `--package`, which
   cargo-cyclonedx 0.5.x does not accept.
3. **No SBOM is signed or bound to the artifact it describes.** The image
   SBOMs from Syft are generated from a mutable tag, uploaded as plain
   files, and not attested.
4. **Container images have no SLSA Build L3 provenance.** BuildKit's
   `provenance: true` writes provenance from inside the same job that runs
   the build steps, so a compromised step could forge it: that is Build L2
   at best. Only the binary tarballs go through
   `slsa-github-generator` (L3).
5. **The provenance covers the tarballs only.** The install manifests,
   which pin the image digests users actually deploy, are outside it.
6. **Release builds do not pass `--locked`**, so a release build could
   resolve dependencies differently from the reviewed `Cargo.lock`.
7. **The docs claim things that do not exist:** a daily bit-for-bit
   reproducibility workflow (`verify-reproducibility.yaml` is not in the
   tree), `bindy-*.sbom.json` release assets, and a mix of "Level 2" and
   "Level 3" for the same thing. They use the retired SLSA v0.1
   requirement table.

## Decision

Target **SLSA v1.0 Build L3** for every release artifact, and ship a
verifiable, NTIA-minimum-elements SBOM for every binary and image.

### 1. SBOMs

- **Binaries:** CycloneDX **1.5** JSON from cargo-cyclonedx **0.5.9**, one
  per shipped binary (all five platforms), describing the `bindy` package
  for that target triple. Each release carries `bindy-<os>-<arch>.cdx.json`.
- **Images:** a CycloneDX JSON SBOM from Syft for each variant, generated
  from the pushed image **by digest**, shipped as
  `bindy-image-<variant>.cdx.json`. BuildKit keeps attaching its own SBOM
  attestation to the image index (`sbom: true`).
- **Quality gate:** `make sbom-check SBOM=<file>` fails the build unless
  the SBOM has: CycloneDX ≥ 1.5, a serial number, a timestamp, the
  generating tool, an author, a root component with name, version and
  purl, a name, version and purl on every component, and a dependency
  graph. It reports, but does not fail on, components without supplier or
  author data: crates.io does not require it, and the purl already names
  the distribution source.
- **Binding:** each SBOM is attested to the exact artifact it describes
  with GitHub artifact attestations (`actions/attest-sbom`, Sigstore keyless,
  predicate type `https://cyclonedx.org/bom`): binary SBOMs to their
  tarball, image SBOMs to the image digest (also pushed to GHCR).

### 2. Provenance

- **Binaries and manifests:** the existing
  `slsa-framework/slsa-github-generator` generic generator (v2.1.0), with
  subjects extended from the five tarballs to the tarballs, the three
  install manifests and every release SBOM.
- **Images:** the same project's container generator
  (`generator_container_slsa3.yml`, v2.1.0), once per variant, against the
  digest `docker-release` pushed. Provenance is pushed to GHCR next to the
  image.
- Both generators run as reusable workflows, isolated from the build jobs
  and signing with an identity no build step can reach. That isolation is
  what makes them Build L3. They are referenced by tag, not SHA, because
  `slsa-verifier` checks the builder's tag.

### 3. Build inputs

Release builds pass `--locked`, in the Linux legs (through the
`build-binary` action's `extra-args`) and in the macOS/Windows legs.

### 4. Verification is a Makefile target, not prose

`make verify-provenance`, `make verify-image-provenance` and
`make verify-sbom-attestation` run `slsa-verifier` and
`gh attestation verify` with the source URI, tag and predicate types
already filled in, so the documented procedure is the one CI and auditors
run.

### 5. Shared action changes go upstream

Per the repo rule, the `generate-sbom` fixes (a `spec-version` input,
`extra-args`, and `package` resolved to `--manifest-path`) land in
`firestoned/github-actions` as v1.3.8, and bindy pins that release.

### 6. Docs say what is true

`docs/src/compliance/slsa.md` is rewritten against SLSA v1.0 (Build track,
L3), and the claims in the other security and compliance pages are
corrected to match. Reproducible builds are documented as a procedure,
not as an automated check, until a workflow exists.

## Consequences

**Good**

- Every release artifact has L3 provenance that anyone can check with
  `slsa-verifier`, including the images and the manifests that pin them.
- Every shipped binary and image has an SBOM that is complete enough for
  NTIA minimum elements, signed, and bound to its artifact's digest.
- A release fails, rather than ships, when an SBOM is missing or
  incomplete: the quality gate runs in the build legs.
- The docs stop claiming controls that do not exist.

**Bad**

- More release jobs: two container-provenance runs and the SBOM
  attestations. Release wall-clock grows by a few minutes.
- `slsa-github-generator` must stay tag-pinned, which OpenSSF Scorecard's
  pinned-dependencies check flags; this is a documented exception.
- GitHub artifact attestations tie SBOM verification to the GitHub
  attestations API (`gh attestation verify`). The image attestations are
  also in GHCR and verifiable with `cosign verify-attestation`.
- CycloneDX 1.5, not 1.6: cargo-cyclonedx 0.5.9 tops out at 1.5. 1.5 is
  enough for NTIA minimum elements and BSI TR-03183-2.
- The bindy changes depend on `firestoned/github-actions` v1.3.8 being
  tagged first.

**Rules out**

- Claiming SLSA Source-track levels. Signed commits and branch protection
  are documented as controls, not as a level.
- Claiming hermetic or reproducible builds until a check enforces them.

**Follow-ups**

- An automated reproducibility check (build twice, compare digests).
- VEX documents that reference the SBOM serial numbers (roadmap 17).
- CALM is unchanged: the runtime architecture does not change. The
  threat model's supply-chain mitigations (M-09) are updated in the same
  change.
