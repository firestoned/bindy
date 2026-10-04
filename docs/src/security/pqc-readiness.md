# Post-Quantum Cryptography Readiness

Bindy maintains a cryptographic inventory and a staged migration plan toward
post-quantum cryptography (PQC). The plan is
[roadmap 28](https://github.com/firestoned/bindy/blob/main/.github/community/28-pqc-readiness.md);
the machine-readable inventory is the **CBOM** (Cryptographic Bill of
Materials, [ADR-0011](https://github.com/firestoned/bindy/blob/main/docs/adr/0011-cryptographic-bill-of-materials.md)): a
CycloneDX 1.6 document shipped with every release as
`bindy-cbom.cdx.json`, generated from the curated template in `cbom/` with
library versions stamped from `Cargo.lock` at build time.

## Why this exists

- **NIST IR 8547** deprecates the quantum-vulnerable asymmetric algorithms
  (ECDSA, RSA, EdDSA) after 2030 and disallows them after 2035. Every
  DNSSEC algorithm bindy can configure today is on that list.
- **Harvest-now-decrypt-later (HNDL)** applies today to confidentiality:
  traffic recorded now can be decrypted once a cryptographically relevant
  quantum computer exists. For bindy that means the TLS channels carrying
  TSIG/RNDC secrets, not the DNSSEC signatures (signatures fail later, by
  forgery, not retroactively).
- Regulated environments increasingly expect a crypto inventory and a
  migration plan as auditable evidence, independent of the migration
  itself. The CBOM is that evidence.

## The inventory, by surface

| Surface | Algorithms | Quantum exposure | Posture |
|---------|------------|------------------|---------|
| DNSSEC signing (`dnssec-policy` rendered into `named.conf`; executed by BIND9) | ECDSAP256SHA256 (default), ECDSAP384SHA384, RSASHA256 | Signature forgery after a CRQC exists; no HNDL | Blocked upstream (IETF dnsop, BIND9). Bindy stays algorithm-agile: the CRD passes the algorithm mnemonic through, and the rotation machinery from DNSSEC zone signing handles a future algorithm rollover. Watch item, revisited every 6 months |
| TSIG / RNDC authentication | HMAC-SHA256 (default); SHA-1 through SHA-512 configurable | Symmetric: PQ-safe with a 256-bit secret | Keep. HMAC-SHA1/SHA-224 deprecation is roadmap 28 Phase 1 |
| Control-plane TLS (operator to bindcar, operator to Kubernetes API) | TLS 1.3/1.2, X25519 or ECDHE P-256 key exchange (rustls, ring provider) | **HNDL-exposed**: TSIG secrets and zone data transit these channels | Hybrid `X25519MLKEM768` key exchange is roadmap 28 Phase 2; it requires a crypto-provider decision (ring has no ML-KEM) coordinated with bindcar |
| In-mesh mTLS (Linkerd) | Set by the mesh | HNDL applies to meshed hops | Environment concern: current Linkerd releases support hybrid post-quantum key exchange; verify it in your mesh profile |
| Release signing (Cosign/Sigstore attestations, SLSA provenance, GPG commits) | ECDSA, RSA/EdDSA | Forgery after a CRQC exists | Blocked on Sigstore and GitHub PQC support; re-attestation strategy is roadmap 28 Phase 4 |

The CBOM carries the same facts per algorithm (`nistQuantumSecurityLevel`,
classical level, OID, where it lives via the `firestoned:bindy:surface`
property), so scanners and inventory tooling can consume them.

## Verifying a release's CBOM

```sh
# The CBOM ships as a release asset and is a subject of the release's
# SLSA provenance (ADR-0010 machinery):
make verify-provenance ARTIFACT=bindy-cbom.cdx.json

# Validate and inspect it locally:
make cbom-check CBOM_FILE=bindy-cbom.cdx.json
jq '[.components[] | select(.type == "cryptographic-asset")
     | {name, level: .cryptoProperties.algorithmProperties.nistQuantumSecurityLevel}]' \
   bindy-cbom.cdx.json
```

## What bindy deliberately does not claim

- The CBOM is a **declared, curated inventory** with build-fact injection,
  not exhaustive discovery: no crypto-asset scanner exists for Rust. The
  curation duty is documented in `cbom/README.md`, and the quality gate
  fails a build whose lockfile no longer matches the template's crypto
  dependencies.
- No PQC DNSSEC support is claimed or shipped: nothing is standardized
  upstream yet. Bindy's commitment is to not be the bottleneck when BIND9
  ships it.

## Timeline anchors

| Date | Event |
|------|-------|
| 2024-08 | FIPS 203 (ML-KEM), 204 (ML-DSA), 205 (SLH-DSA) finalized |
| 2030 | NIST IR 8547: 112-bit classical asymmetric algorithms deprecated |
| 2035 | NIST IR 8547: quantum-vulnerable asymmetric algorithms disallowed |
| Every 6 months (from 2027-04) | Roadmap 28 watch-item review: IETF dnsop PQC drafts, BIND9 release notes, Sigstore PQC |
