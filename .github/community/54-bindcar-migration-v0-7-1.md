# bindcar `v0.6.0` → `v0.7.1` — bindy Integration & Upgrade Guide

> **Status:** 📄 Reference — **superseded** by 56. Read
> [`56-bindcar-migration-v0-7-4.md`](56-bindcar-migration-v0-7-4.md) instead.
>
> *Migrated 2026-09-12 from the external roadmap set into `.github/community/`.*

> Supersedes [`53-bindcar-migration-v0-7-0.md`](53-bindcar-migration-v0-7-0.md). Built from the actual
> `git diff v0.6.0..v0.7.1` and verified against published artifacts
> (ghcr images + crates.io) on 2026-07-05.
>
> **bindy status:** the entire v0.6.0→v0.7.0 migration (Sections 1–10 below) was
> completed in bindy on 2026-07-01/02 (Mode B chosen; see bindy
> `.claude/CHANGELOG.md` and `docs/src/operations/migration-guide.md`). The
> v0.7.0→v0.7.1 delta (Section 11) is small; the residual blocker (Section 12)
> is unchanged.

---

## 0. Version facts (verified)

| Artifact | v0.7.0 | v0.7.1 |
|---|---|---|
| Git tag | `14505e5` (2026-07-01) | `29c71ef` (2026-07-05) |
| `src/` diff between the two tags | — | **byte-identical (zero files changed)** |
| `Cargo.toml` (deps/features) between tags | — | unchanged (`default = []`, `k8s-token-review` optional) |
| crates.io | `0.7.0` | `0.7.1` published — **bindy's `bindcar = "0.7"` already resolves to it** (Cargo.lock, tests green) |
| ghcr images | `v0.7.0`, `v0.7.0-distroless` (built from old `release.yml`) | `v0.7.1`, `v0.7.1-distroless` (built from new `build.yaml`, 2026-07-05) |
| TokenReview feature in published binary | ❌ not compiled | ❌ **still not compiled** (see §12) |
| `nsupdate` binary in image | ❌ missing | ✅ **bundled** (see §11a) |

The only v0.7.1 commit is PR #70 (“Move to using hickory for nsupdate and add a
full e2e regression suite”). Despite the title, **the runtime still shells out
to `nsupdate -k` with a 0600 TSIG temp key file** — there is no hickory
dependency in bindcar at v0.7.1. The shipped change is packaging + CI + test
infrastructure only.

---

## 1–10. v0.6.0 → v0.7.0 (unchanged; DONE in bindy)

The full analysis lives in the superseded guide. Summary of what it required and
what bindy did (Mode B — TokenReview — chosen):

| § | Requirement | bindy status |
|---|---|---|
| 1 | Auth startup guard — real auth on non-loopback bind | ✅ Mode B: operator sends its SA token as Bearer |
| 2 | TokenReview: audience enforced + fail-closed allowlists | ✅ projected `audience: bindcar` token (`/var/run/secrets/bindcar/token`), `BIND_ALLOWED_SERVICE_ACCOUNTS` = **operator** SA, `BIND_TOKEN_AUDIENCES=bindcar`, `bindcar-tokenreview` ClusterRole for the operand `bind9` SA |
| 3 | Strict request validation (zone/record charsets, trailing dots, plain-IP lists) | ✅ `primaries` sent as bare IPs; VAP 13/14 `bindy-record-value-validation` added |
| 4 | RNDC keys SHA-2 only | ✅ both parsers reject `hmac-md5`/`hmac-sha1`; RNDC-strict VAP promoted to default |
| 5 | Purpose-built tokenreview RBAC | ✅ `deploy/operator/rbac/tokenreview-clusterrole{,binding}.yaml` |
| 6 | PSA `restricted` + writable `/tmp` + `TMPDIR` | ✅ `named` = drop-ALL + `NET_BIND_SERVICE`, sidecar = RO rootfs + seccomp + memory `emptyDir` at `/tmp`; **DNS moved 5353→53** (forced by §3 plain-IP `primaries` + no transfer-port knob) |
| 7 | NetworkPolicy egress w/ real API-server CIDR | ✅ `deploy/pod-hardening.yaml` (placeholder `10.0.0.1/32` must be replaced per-cluster) |
| 8 | Swagger/OpenAPI off unless `BIND_ENABLE_DOCS=true` | ✅ documented (dev-only via `bindcarConfig.envVars`) |
| 9 | Crate bump `bindcar = "0.7"` | ✅ (types-only; kube 4.0 gated behind the optional feature) |
| 10 | Chainguard base re-pin | n/a for bindy (bindcar's Dockerfile) |

Regression coverage: `make regression-test` (bindy) pins the §1–§6 contract —
22 admission fixtures + ~20 operand pod-shape assertions + liveness smoke.

---

## 11. 🟠 v0.7.0 → v0.7.1 delta (what's actually new)

### 11a. Images finally contain `nsupdate` — bump the default image tag

Both `docker/Dockerfile.chainguard` and `docker/Dockerfile` (distroless) gained
a builder stage that stages `/usr/bin/nsupdate` + its non-glibc shared libs into
the runtime image (Wolfi `bind-tools` / Debian 13 `bind9-dnsutils`, digest-pinned
bases).

**Consequence:** in the published **v0.7.0** images, bindcar's own
record-management endpoints (`/records`, which shell out to `nsupdate`) were
silently broken — the binary wasn't in the image. bindy is unaffected in normal
operation (it does RFC 2136 directly against `named:53` with hickory, bypassing
bindcar for records), but anything driving bindcar's record API directly was.

**Action (bindy):** bump `DEFAULT_BINDCAR_IMAGE` → `ghcr.io/firestoned/bindcar:v0.7.1`
(`src/constants.rs`). The `/tmp` + `TMPDIR` pod plumbing added for §6 is exactly
what this nsupdate path needs — keep it.

### 11b. `DISABLE_AUTH` no longer baked into the image

The `ENV DISABLE_AUTH=false` line was removed from the Dockerfiles (BuildKit's
`SecretsUsedInArgOrEnv` lint trips on `*AUTH*` names). The binary defaults it to
`false`, so behavior is identical. **No bindy action** (bindy never sets it).

### 11c. CI consolidation + bindcar's own regression suite

`main.yaml` / `pr.yml` / `release.yml` merged into one `build.yaml`
(push/PR/release triggers). New Makefile targets: `regression`
(fmt + clippy **both feature sets** + unit tests + deploy-manifest validation)
and `regression-full` (adds `integration-test/kind-e2e.sh`). Notably their
`clippy-all`/`unit-tests` now exercise `--features k8s-token-review` — the
feature is *tested* in CI, just not *shipped* (§12). Their kind-e2e exercises
**Mode A only** (shared-secret `BIND_API_TOKEN`).

**No bindy action** — informational.

### 11d. `Cargo.lock` refresh

Dependency bumps inside the bindcar binary (`bitflags` 2.13, `defmt` 1.x, etc.).
No API surface change for crate consumers; bindy's own lockfile already resolved
`bindcar 0.7.1` and the full test suite passes against it.

---

## 12. 🔴 Residual blocker (unchanged from v0.7.0): published images cannot do Mode B

The new `build.yaml` still invokes `firestoned/github-actions/rust/build-binary`
with only `target:` — **no `--features k8s-token-review`** — so the ghcr
`v0.7.1` images physically lack the TokenReview code path. bindy is configured
for Mode B, so with the stock image the sidecar hits the auth startup guard on
`0.0.0.0:8080` (no `BIND_API_TOKEN`, no TokenReview capability) and **refuses to
start**.

Fix in the **bindcar** repo (one line, verified the action supports it):

```yaml
# .github/workflows/build.yaml — Build binary step
- name: Build binary
  uses: firestoned/github-actions/rust/build-binary@…
  with:
    target: ${{ matrix.platform.target }}
    extra-args: "--features k8s-token-review"   # ← add
```

(Feature-enabled builds remain fully Mode A-compatible, so enabling it for all
release artifacts is safe. Alternative: a `-tokenreview` image variant via the
existing docker matrix — then bindy's default tag should name that variant.)

Until a feature-enabled image ships, bindy's Phase C regression (operand
liveness) will skip, and real deployments need a locally built
`bindcar --features k8s-token-review` image.

---

## Pre-upgrade checklist for bindy (v0.7.0 → v0.7.1)

- [ ] `src/constants.rs`: `DEFAULT_BINDCAR_IMAGE` → `…:v0.7.1`; refresh the
      stale `v0.6.0` example in `src/crd.rs` (`BindcarConfig.image` rustdoc).
- [ ] `docs/src/operations/migration-guide.md`: reference `v0.7.1`.
- [ ] Crate dep: none — `bindcar = "0.7"` already resolves to 0.7.1.
- [ ] Run `cargo-quality` + `make regression-test` (image-prefix assertion
      `bindcar:v0.7` already covers v0.7.1).
- [ ] **(bindcar repo)** add `extra-args: "--features k8s-token-review"` to
      `build.yaml` and cut v0.7.2 / re-release — the Mode B deployment blocker.

## Commit range covered

`v0.6.0 (7080edc)` → `v0.7.1 (29c71ef)`, including PRs #46, #47, #64, #66, #67,
#68, #69, #70 and the `security-sweep` remediation (A1–A20).
