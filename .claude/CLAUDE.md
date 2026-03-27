@.claude/SKILL.md

# Project Instructions for Claude Code

> Platform Engineering - Kubernetes Operators & Infrastructure
> Environment: k0rdent / Multi-cluster / Strong banking compliance standards apply

**Service Mesh Standard**: Always use Linkerd in docs, examples, and comments. Never use Istio, Consul Connect, or other implementations unless specifically required.

**CRITICAL Coding Patterns** (full details in `rules/`):
- **TDD**: Write tests FIRST — `rules/testing.md` + `tdd-workflow` skill
- **After ANY Rust change**: run `cargo-quality` skill (NON-NEGOTIABLE)
- **Early returns / magic numbers / style**: `rules/rust-style.md`
- **Event-Driven controllers**: use watch API, never polling

---

## 🚨 CRITICAL: Keep Bootstrap RBAC in Sync

Any time you modify ClusterRole/Role/Binding definitions in `src/bootstrap.rs`, update ALL of:

| File | What to update |
|------|---------------|
| `deploy/scout/clusterrole.yaml` | Mirror rules from `build_scout_cluster_role` |
| `deploy/scout/clusterrolebinding.yaml` | Mirror binding changes |
| `deploy/scout/role.yaml` | Mirror rules from `build_scout_role` |
| `deploy/scout/rolebinding.yaml` | Mirror role binding changes |
| `deploy/scout.yaml` | Update inline RBAC sections |
| `docs/src/guide/scout.md` | Update ClusterRole/Role YAML examples |

**Verification:** `deploy/scout/clusterrole.yaml` matches `bootstrap.rs`; tests in `bootstrap_tests.rs` cover updated rules; `deploy/scout.yaml` ClusterRole matches.

**REMEMBER:** `bootstrap.rs` and the static YAML files are THREE representations of the same RBAC policy — keep them in sync.

---

## 🚨 CRITICAL: CRD Schema Sync

Before investigating any Kubernetes issue, verify CRDs match Rust code definitions.

> **How:** Run the `verify-crd-sync` skill.

CRD YAMLs in `deploy/crds/` are AUTO-GENERATED from `src/crd.rs`. Schema mismatches cause silent failures — patches succeed (HTTP 200) but fields don't persist.

**When to check:** reconciliation loops, "field not appearing in kubectl output", after `src/crd.rs` edits, when status patches don't persist.

---

## 🚨 CRITICAL: Always Review Official Documentation

When unsure of a decision, ALWAYS read official docs before implementing. Never take shortcuts based on assumptions. Research first, implement second.

---

## 🚨 CRITICAL: Docker Base Images Must Be Multi-Arch

Use multi-arch manifest list digests, NOT platform-specific digests. Platform-specific digests cause QEMU errors on ARM64.

> **How:** Run the `get-multiarch-digest` skill.

Update ALL Dockerfiles when changing base images: `docker/Dockerfile`, `Dockerfile.chainguard`, `Dockerfile.chef`, `Dockerfile.fast` (`.local` usually has no digest).

---

## 🔍 MANDATORY: Use ripgrep

ALWAYS use `rg` for code search. NEVER use `grep`, `find`, or `lsof`.

- Rust files: `rg -trs "pattern" . -g '!target/'`

---

## 🚫 Docker and Kubernetes Operations Restrictions

**NEVER build or push Docker images.** The user manages all image operations.

**Allowed kubectl:** `get`, `describe`, `logs`, `annotate` (read-only + annotations only)

**FORBIDDEN:** `docker build/push/tag`, `kind load`, `kubectl rollout restart`, `kubectl delete pods`, `kubectl apply`, `kubectl patch` (unless explicitly requested)

After code changes: run `cargo fmt`, `cargo clippy`, `cargo test`, then inform the user changes are ready to build and deploy.

---

## 🚨 Plans and Roadmaps → `docs/roadmaps/`

ALL planning documents MUST go in `docs/roadmaps/`. Filenames: **lowercase**, **hyphens only** (no underscores, no uppercase).

```
✅ docs/roadmaps/integration-test-plan.md
❌ ROADMAP.md  ❌ docs/roadmaps/ZONES_FROM_LABEL_SELECTOR.md  ❌ docs/roadmaps/Phase_3.md
```

---

## 🔧 GitHub Workflows & CI/CD

See `rules/github-workflows.md` for full standards. Key rules:

- **NEVER** replace `firestoned/github-actions` composite actions with direct action calls — update the `firestoned/github-actions` repo instead
- All workflows MUST delegate logic to Makefile targets (no inline bash scripts)
- New workflows MUST support `workflow_call` for reusability

---

## 🔒 Compliance & Security Context

Regulated banking environment. All changes must be auditable and traceable.

**Never commit:** secrets, tokens, credentials, internal hostnames/IPs, customer data.

---

## 📝 Documentation Requirements

See `rules/documentation.md` for full workflow.

- Ask "Does documentation need to be updated?" before marking ANY task complete
- Update `.claude/CHANGELOG.md` with `**Author:**` on EVERY code change (MANDATORY — no exceptions)
- Build docs with `make docs` — use `build-docs` skill (never `mdbook build` directly)
- For ADRs: create `/docs/adr/NNNN-title.md` with Status / Context / Decision / Consequences

---

## 🦀 Rust Workflow

Full style guide: `rules/rust-style.md`. Full testing standards: `rules/testing.md`.

**After ANY `.rs` change:** run `cargo-quality` skill (`cargo fmt` + `cargo clippy` + `cargo test`). Task is NOT complete until all three pass.

### TDD (mandatory)

Write failing tests FIRST, then implement minimum code to pass. See `tdd-workflow` skill.

Test file pattern: `src/foo.rs` → `#[cfg(test)] mod foo_tests;` at bottom → `src/foo_tests.rs`

### Dependency Management

Before adding deps: verify actively maintained (commits in last 6 months), prefer well-known crates, document reason in CHANGELOG.

---

## ☸️ Kubernetes Operator Patterns

### CRD Development — Rust as Source of Truth

`src/crd.rs` is the source of truth. YAML files in `deploy/crds/` are auto-generated — never edit them directly.

> CRD changes: `regen-crds` skill → update `examples/` → `regen-api-docs` skill (LAST).

Use `kubectl replace --force` (not `apply`) — `Bind9Instance` CRD exceeds 256KB annotation limit.

Adding a new CRD: follow `add-new-crd` skill.

### CRD Documentation Examples

ALWAYS read `deploy/crds/*.crd.yaml` or `src/crd.rs` before writing any YAML examples. Never guess field names.

### Controllers: Event-Driven (Watch, Not Poll)

Use `Controller::new()` from kube-runtime. Never poll in a loop.

```rust
// ✅ CORRECT
Controller::new(api, Config::default())
    .run(reconcile, error_policy, context)
    .for_each(|_| futures::future::ready(()))
    .await;
```

**Best practices:** set `ownerReferences`, use finalizers, exponential backoff, log reconciliation start/end.

### Status Conditions

```rust
Condition {
    type_: "Ready".to_string(),
    status: "True".to_string(),
    reason: "ReconcileSucceeded".to_string(),
    message: "Zone synchronized successfully".to_string(),
    last_transition_time: Some(Time(Utc::now())),
    observed_generation: Some(zone.metadata.generation.unwrap_or(0)),
}
```

---

## 🔄 FluxCD / GitOps

Structure: `clusters/base/` + `clusters/overlays/{dev,staging,prod}/`

HelmRelease changes: bump chart version, add suspend annotation for breaking changes, document rollback in CHANGELOG.

---

## 🧪 Testing

See `rules/testing.md` for full standards.

- Every public function MUST have unit tests
- Tests in separate `_tests.rs` files (never embedded in source)
- Integration tests in `/tests/` directory
- Run: `cargo-quality` skill. Specific module: `cargo test --lib <module>`. Verbose: `cargo test -- --nocapture`

---

## 📁 File Organization

```
src/
├── main.rs / main_tests.rs
├── bind9.rs / bind9_tests.rs
├── bind9_resources.rs / bind9_resources_tests.rs
├── crd.rs / crd_tests.rs
├── reconcilers/
│   ├── bind9cluster.rs / bind9cluster_tests.rs
│   ├── bind9instance.rs / bind9instance_tests.rs
│   ├── dnszone.rs / dnszone_tests.rs
│   └── records.rs / records_tests.rs
└── bin/ (crdgen.rs, crddoc.rs)

docs/
├── roadmaps/   ← ALL planning docs here (lowercase-hyphen filenames)
├── adr/        ← Architecture Decision Records
└── src/        ← mdBook source
```

---

## 🚫 Things to Avoid

- `unwrap()` in production — use `?` or explicit error handling
- Hardcoded namespaces — make them configurable
- `sleep()` for synchronization — use k8s watch/informers
- Ignoring errors in finalizers — blocks resource deletion
- State outside Kubernetes — operators must be stateless

---

## 💡 Helpful Commands

```bash
RUST_LOG=debug cargo run                       # Run locally
kubectl apply --dry-run=server -f deploy/      # Validate manifests
```

Skills: `regen-crds`, `regen-api-docs`, `validate-examples`, `cargo-quality`, `build-docs`, `get-multiarch-digest`, `verify-crd-sync`, `tdd-workflow`, `pre-commit-checklist`, `update-changelog`, `update-docs`, `add-new-crd`.

---

## 📋 PR/Commit Checklist

**Run `pre-commit-checklist` skill before EVERY commit. A task is NOT complete until it passes.**

Documentation is NOT optional — it is a critical requirement equal in importance to the code.

---

## 🔗 Project References

- [kube-rs documentation](https://kube.rs/)
- [Kubernetes API conventions](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md)
- [Operator pattern](https://kubernetes.io/docs/concepts/extend-kubernetes/operator/)
- Internal: k0rdent platform docs (check Confluence)
