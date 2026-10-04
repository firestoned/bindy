# 28: Post-Quantum Cryptography (PQC) Readiness

> **Goal.** Know every cryptographic algorithm bindy ships, configures or
> depends on; migrate the surfaces that can move today (symmetric hygiene,
> hybrid post-quantum TLS key exchange); and be positioned to adopt
> post-quantum DNSSEC in the first BIND9 release that supports it, ahead of
> the NIST IR 8547 timeline (quantum-vulnerable algorithms deprecated after
> 2030, disallowed after 2035).
>
> **Stop condition.** A cryptographic bill of materials (CBOM) is published
> with every release; control-plane TLS negotiates a hybrid post-quantum key
> exchange by default; no SHA-1-class algorithm is accepted by any CRD without
> a deprecation warning; the threat model names a quantum-capable adversary
> with explicit harvest-now-decrypt-later (HNDL) analysis; and every surface
> that is blocked upstream (DNSSEC algorithms, sigstore signatures) has a
> tracked watch item with a revisit date. PQC DNSSEC signing itself is
> explicitly *not* in scope: it does not exist upstream yet.

## Why now

This operator runs in a regulated banking environment (SOX 404, PCI-DSS,
Basel III cyber-risk). The external deadlines are no longer abstract:

- **NIST IR 8547** (transition to PQC standards): ECDSA, RSA and EdDSA at
  112-bit classical security are *deprecated after 2030* and *disallowed
  after 2035*. Every DNSSEC algorithm bindy can configure today is on that
  list.
- **FIPS 203/204/205** (ML-KEM, ML-DSA, SLH-DSA) were finalized in August
  2024; the tooling wave (TLS hybrid key exchange, CBOM formats) is shipping
  now.
- **HNDL is a today-problem for confidentiality surfaces.** TSIG secrets,
  RNDC keys and zone data recorded off the wire today can be decrypted once
  a cryptographically relevant quantum computer (CRQC) exists. Signature
  surfaces (DNSSEC, cosign, commit signing) only break *after* a CRQC
  exists, so their deadline is softer, but key rollover and re-attestation
  take years at DNS scale, which is why readiness starts now.
- Regulators in this environment (DORA ICT-risk, BCBS) increasingly ask for
  a crypto inventory and a migration plan as evidence, independent of the
  migration itself. Phase 0 is auditable output on its own.

## Crypto inventory (as of 2026-10-04, branch `refactor/workspace-crates`)

| # | Surface | Where | Algorithms today | Quantum exposure | Migration path |
|---|---------|-------|------------------|------------------|----------------|
| 1 | DNSSEC zone signing | `dnssec-policy` block rendered by `crates/bindy/src/bind9_resources.rs` (`DEFAULT_DNSSEC_ALGORITHM = "ECDSAP256SHA256"`); `DnssecConfig.algorithm` free-token field in `crates/bindy-api/src/crd.rs` | ECDSAP256SHA256 (13) default; ECDSAP384SHA384 (14), RSASHA256 (8) documented | Signature forgery after CRQC; no HNDL (signatures are not confidentiality) | Blocked upstream: IETF dnsop + BIND9. Phase 3 keeps bindy agile and watching |
| 2 | TSIG / RNDC keys | `RndcAlgorithm` enum in `crates/bindy-api/src/crd.rs` (HMAC-SHA1 through HMAC-SHA512, default HMAC-SHA256) | HMAC (symmetric) | Grover halves effective strength; HMAC-SHA256 with a 256-bit secret remains safe | Phase 1: deprecate SHA-1/SHA-224 variants, document minimum secret entropy |
| 3 | Control-plane TLS (operator → bindcar, operator → kube API, Scout remote mode) | rustls 0.23 with the **ring** provider, installed process-wide in `crates/bindy/src/main.rs`; same pin in bindcar's `Cargo.toml`; `BindcarTlsConfig` CRD surface (roadmap 25) | X25519 / P-256 ECDHE key exchange, classical certificates | **HNDL applies**: TSIG secrets and zone data transit these channels | Phase 2: hybrid `X25519MLKEM768` key exchange. Requires a crypto-provider decision (ring has no ML-KEM) on both bindy and bindcar ends |
| 4 | Zone transfer (AXFR/IXFR) authentication | BIND9, keys from surface 2 | TSIG HMAC | Same as surface 2 | Covered by Phase 1; transfer *confidentiality* rides on mesh/TLS (surface 3/6) |
| 5 | Supply chain: image/SBOM attestation, provenance, commit signing | ADR-0010 pipeline (cosign/sigstore, SLSA Build L3), GPG-signed commits (threat-model M-33/M-34) | ECDSA (sigstore), RSA/EdDSA (GPG) | Forgery after CRQC; long-lived attestations outlive the algorithms that signed them | Phase 4: blocked on sigstore/GitHub PQC support; plan re-attestation |
| 6 | In-mesh mTLS | Linkerd (environment, not bindy code) | Linkerd supports hybrid post-quantum key exchange in current releases | HNDL applies to meshed hops | Ops note in docs; verify the mesh profile enables it. Not a bindy code change |
| 7 | Secrets at rest | Kubernetes Secrets (TSIG/RNDC/TLS keys), etcd/kine encryption | Platform-dependent | Platform concern | Out of scope here; noted in the threat model as an environment assumption |

Two properties bindy already has, worth preserving deliberately:

- **Algorithm agility in the CRD.** `DnssecConfig.algorithm` is a validated
  free token (`^[A-Za-z0-9]{1,32}$`), not an enum. The day BIND9 accepts a
  PQC mnemonic in `dnssec-policy`, bindy passes it through with no CRD
  change. Do not "improve" this into a closed enum.
- **Rotation machinery exists.** Roadmap 07 shipped `dnssec-policy`-driven
  key lifetimes, DS extraction and `status.dnssec.nextKeyRollover` (ADR-0006
  as amended). An algorithm rollover is operationally the same machinery.

## Phases

Ordered by what is actionable now. Phases 0 to 2 are real work; phases 3 and
4 are agility plus watch items; phase 5 closes the loop. Per ADD, each phase
that is architecturally significant gets its own ADR before implementation;
this roadmap is the what/why, not the how.

### Phase 0: Cryptographic inventory as a release artifact (CBOM)

Builds on roadmap 15's SBOM pipeline. CycloneDX 1.6 defines cryptographic
asset properties (CBOM); emit one per release so the inventory above stops
being a hand-maintained table.

- [ ] Evaluate CycloneDX 1.6 CBOM tooling for Rust binaries and container
      images; pick the generation point in the existing `sbom.yml` flow
- [ ] Emit a CBOM per release binary and image, attested like the SBOMs
      (extends ADR-0010; amend it or write a follow-up ADR)
- [ ] New docs page `docs/src/security/pqc-readiness.md`: the inventory
      table above, the migration posture, and the watch items with dates
- [ ] CHANGELOG + `ROADMAPS.md` row update

### Phase 1: Symmetric hygiene (TSIG/RNDC)

HMAC is PQ-safe at adequate hash and secret sizes; the work is retiring the
legacy tail, not replacing the primitive.

- [ ] Deprecate `HmacSha1` and `HmacSha224` in `RndcAlgorithm`: rustdoc
      deprecation notes, a Warning event or status condition when a resource
      uses them, and a migration note in `docs/src/`
- [ ] Document the minimum secret entropy (256 bits for HMAC-SHA256) where
      key generation and `secret_ref` are documented; verify
      operator-generated secrets already meet it
- [ ] Decide (ADR) whether removal of the deprecated variants is a breaking
      CRD change worth scheduling, and if so for which release
- [ ] Mirror any guidance into the bindcar docs if its TSIG surface repeats it

### Phase 2: Hybrid post-quantum key exchange on control-plane TLS

The one HNDL-exposed surface bindy fully controls. `X25519MLKEM768` is the
deployed-at-scale hybrid group; rustls supports it, but **not** through the
ring provider that both bindy (`main.rs` process-default) and bindcar pin
today. The provider choice was deliberate (the Cargo.toml comment excludes
aws-lc-rs to keep C/assembly out of the graph), so this is an ADR-shaped
trade-off, not a feature flag:

- [ ] ADR: crypto-provider strategy for hybrid KEX. Options include
      aws-lc-rs as default provider, the `rustls-post-quantum` provider
      crate layered over ring, or staying on ring until it grows ML-KEM.
      Record the supply-chain trade-off explicitly; this reverses a
      documented decision, so the Cargo.toml comment and the ADR must agree
- [ ] Coordinate the same decision in bindcar (its rustls/tokio-rustls pins
      mirror bindy's); the server end must offer the group for the client
      end to matter
- [ ] Enable and prefer `X25519MLKEM768` for the bindy → bindcar client and
      verify the kube-client path tolerates it (kube API servers that do not
      support it must cleanly fall back to classical: hybrid is negotiated,
      not forced)
- [ ] Observability: log or export the negotiated group so "are we actually
      PQ on the wire" is answerable from metrics, then verify in the TLS
      e2e suite (`make tls-transport-test` / `e2e-tls`)
- [ ] Threat-model pass for this phase folds into Phase 5

### Phase 3: DNSSEC algorithm agility (watch item, not a build)

No PQC DNSSEC algorithm is standardized: ML-DSA and SLH-DSA signatures are
too large for comfortable UDP responses, and the IETF dnsop work (including
stateful hash-based and MTL-mode proposals) is still in flight. BIND9 signs
with what its crypto library offers; nothing to integrate yet. Bindy's job
is to not be the bottleneck when it lands:

- [ ] Keep `DnssecConfig.algorithm` a pass-through token (guard with a test
      that asserts an unknown-to-bindy mnemonic reaches the rendered
      `dnssec-policy` block unmodified)
- [ ] Write the algorithm-rollover runbook now, using the existing
      roadmap 07 rotation machinery with a classical pair
      (e.g. ECDSAP256SHA256 → ECDSAP384SHA384) as the rehearsal; a PQC
      rollover is the same procedure with a different mnemonic
- [ ] Verify large-response behavior while rehearsing: EDNS0 sizes, TCP
      fallback, and transfer sizes are where PQC DNSSEC will hurt first
- [ ] Watch items with revisit dates (first revisit 2027-04, then every
      6 months): IETF dnsop PQC drafts, BIND9 release notes for PQC
      `dnssec-policy` support, parent-zone DS digest support
- [ ] When BIND9 ships support: write the adoption ADR; it is a config
      surface change, not (expected) a CRD change

### Phase 4: Supply-chain signature migration (watch item)

Blocked upstream on sigstore and GitHub shipping PQC signing. The bindy-side
preparation is a plan, not code:

- [ ] Watch items (same revisit cadence as Phase 3): sigstore PQC roadmap,
      GitHub support for PQC commit-signing keys
- [ ] Document the re-attestation strategy: existing SLSA provenance and
      SBOM attestations (M-33/M-34) are signed classically; decide whether
      historical releases get re-attested or time-boxed as
      accepted risk when PQC signing lands
- [ ] Fold the decision into ADR-0010's lineage when actionable

### Phase 5: Threat model and docs closure

- [ ] Full threat-model pass: add a quantum-capable adversary to the actors,
      HNDL analysis per trust boundary, map each inventory row above to a
      mitigation or an accepted risk with a *Revisit when*; bump the header
      stamp per ADD
- [ ] `docs/src/security/pqc-readiness.md` kept current as phases land
      (created in Phase 0)
- [ ] Linkerd ops note (surface 6): document verifying hybrid KEX in the
      mesh profile for meshed bindy ↔ bindcar hops

## Non-goals

- Implementing or back-porting PQC DNSSEC signing ahead of BIND9: bindy
  configures BIND9, it does not sign zones itself
- PQC for DNS *query* privacy (DoT/DoH): bindy does not terminate those
- Replacing HMAC-based TSIG with something asymmetric: symmetric TSIG is
  already the PQ-safe part of the stack
- Kubernetes-platform concerns (etcd encryption, kubelet TLS): environment
  assumptions, recorded in the threat model, not bindy work

## Dependencies and ordering

- Phase 0 depends on roadmap 15's SBOM pipeline (shipped through phase 6)
- Phase 2 interacts with roadmap 25/26 TLS surfaces and requires a matching
  bindcar release; sequence it behind the live-cluster TLS verification that
  roadmap 25 still has open
- Phase 3's rehearsal rollover wants a live cluster with a delegated test
  zone; pairs naturally with the roadmap 26 live-zone DNSSEC follow-ups
- Phases 3 and 4 produce no code until upstream moves; they must not be
  closed as "done", only re-triaged at each revisit date

## References

- NIST IR 8547: Transition to Post-Quantum Cryptography Standards
- FIPS 203 (ML-KEM), FIPS 204 (ML-DSA), FIPS 205 (SLH-DSA)
- CNSA 2.0 timeline (NSA, for the strictest-deadline comparison)
- CycloneDX 1.6 CBOM specification
- IETF dnsop working group: PQC DNSSEC drafts and research
- rustls post-quantum documentation (`X25519MLKEM768`, provider model)
- Internal: ADR-0006 (DNSSEC), ADR-0010 (SBOM/provenance), roadmaps 07, 15,
  25, 26; threat model v1.7
