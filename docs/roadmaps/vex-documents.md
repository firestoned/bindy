# VEX Documents Roadmap

**Status:** Planned
**Date:** 2026-03-18
**Impact:** Supply chain security / release artifact enhancement

---

## Overview

Add Vulnerability Exploitability eXchange (VEX) documents to the release pipeline. VEX documents communicate whether a product is affected by known CVEs — allowing consumers to distinguish between vulnerabilities that are present in a dependency but not exploitable in the specific build versus ones that require remediation.

## Why

- **Reduce false positives**: Trivy/Grype scanners flag many CVEs in Rust crates that are not reachable in `bindy`'s code paths. VEX documents let consumers suppress known-not-affected findings without losing signal on real risks.
- **Compliance**: Regulated environments (banking/capital markets) increasingly require VEX alongside SBOM as part of SSDF/EO-14028 artifact attestation.
- **Cosign integration**: VEX documents can be attached to OCI images via `cosign attest`, keeping them verifiable and tied to specific image digests.
- **OpenVEX standard**: The [OpenVEX spec](https://github.com/openvex/spec) is the emerging standard, with tooling already available (`vexctl`, `openvex-go`).

## Planned Artifacts

Each release should produce one VEX document per image variant (Chainguard, Distroless) and one for the binary tarballs:

```
bindy-<version>.vex.json          # Binary tarballs VEX
bindy-chainguard-<version>.vex.json
bindy-distroless-<version>.vex.json
```

## Implementation Plan

### Phase 1: Static VEX Authoring

1. Add `vex/` directory with manually authored OpenVEX statements for known not-affected CVEs identified during Trivy scans.
2. Validate VEX documents with `vexctl` in CI (`make vex-validate`).
3. Include VEX files as release assets alongside `checksums.sha256`.

**Tooling**: [`vexctl`](https://github.com/openvex/vexctl), OpenVEX JSON format.

### Phase 2: CI-Generated VEX

1. Add `make vex-generate` Makefile target that runs Trivy with `--vex` flag to auto-generate VEX from a curated list of statements.
2. Integrate into `release.yaml` after the Trivy scan job — generate VEX from scan results + static overrides, then upload as release artifact.
3. Sign VEX documents with Cosign (same keyless workflow as binary tarballs).

### Phase 3: OCI Attestation

1. Attach VEX documents to container images via `cosign attest --type vex`.
2. Consumers can verify with `cosign verify-attestation --type vex`.
3. Update documentation with verification instructions.

## Workflow Integration (Phase 2 target)

```yaml
# In release.yaml, after trivy job
vex-generate:
  name: Generate VEX Documents
  runs-on: ubuntu-latest
  needs: [trivy, extract-version]
  steps:
    - uses: actions/checkout@v6
    - name: Generate VEX documents
      run: make vex-generate VERSION=${{ needs.extract-version.outputs.version }}
    - name: Sign VEX documents
      run: make vex-sign
    - uses: actions/upload-artifact@v7
      with:
        name: vex-documents
        path: vex/*.vex.json
```

## Makefile Targets (to be added)

```makefile
vex-validate:   ## Validate VEX documents with vexctl
vex-generate:   ## Generate VEX from Trivy scan + static statements
vex-sign:       ## Sign VEX documents with Cosign
```

## References

- [OpenVEX Specification](https://github.com/openvex/spec)
- [vexctl CLI](https://github.com/openvex/vexctl)
- [Cosign attestation](https://docs.sigstore.dev/cosign/attestation/)
- [CISA VEX Use Cases](https://www.cisa.gov/resources-tools/resources/minimum-requirements-vulnerability-exploitability-exchange-vex)
- [Trivy VEX support](https://aquasecurity.github.io/trivy/latest/docs/supply-chain/vex/)
