# Documentation Standards

## Before Marking Any Task Complete

ALWAYS ask: "Does documentation need to be updated?"

Applies to: code changes, CRD changes, API changes, configuration changes, architecture changes.

---

## Documentation Update Workflow

1. **Analyze the change**: user-facing impact? architectural implications? new APIs/config?
2. **Update in this order:**
   - `.claude/CHANGELOG.md` (see `update-changelog` skill — `**Author:**` is MANDATORY)
   - `docs/src/` — affected user guides, quickstart, config references, troubleshooting
   - `examples/*.yaml` — update to match new schema/behavior
   - Architecture diagrams (Mermaid in `docs/src/architecture/`) if structure changed
   - API docs if CRDs changed: run `regen-api-docs` skill (LAST)
   - `README.md` if getting-started or features changed
3. **Verify:** read docs as a new user, validate all YAML examples, run `build-docs` skill

---

## What to Update by Change Type

**Controller/reconciler changes** (`src/reconcilers/`):
- Update reconciliation flow diagrams
- Document new behaviors in user guides
- Update troubleshooting guides

**CRD changes** (`src/crd.rs`):
- Run `regen-crds` skill → update `examples/` → run `regen-api-docs` skill (LAST)
- Update ALL examples that use the changed CRD
- Update quickstart guides and configuration reference

**Core logic changes** (`src/bind9.rs`, etc.):
- Update architecture docs
- Add examples for new public functions
- Update troubleshooting guides

**New features:**
- Add to `docs/src/features/`, update `README.md`, add examples, add troubleshooting

**Bug fixes:**
- Update troubleshooting guides with the fix

---

## Documentation Examples Must Reference CRDs

ALWAYS verify field names against `deploy/crds/*.crd.yaml` or `src/crd.rs` before writing examples. NEVER guess field names.

```yaml
# ❌ WRONG - guessed field name
spec:
  config:
    recursion: true

# ✅ CORRECT - verified against CRD
spec:
  global:
    recursion: true
```

If CRDs change: search all docs for examples using the changed CRD:
```bash
grep -r "kind: <CRDName>" docs/src/
```

---

## Building Documentation

**ALWAYS use `make docs`, never `mdbook build` directly.**

> Run the `build-docs` skill.

---

## Changelog Requirements

Every entry in `.claude/CHANGELOG.md` MUST have `**Author:**` — no exceptions.

Format:
```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

**Author:** <Name of requester or approver>

### Changed
- `path/to/file.rs`: Description of the change

### Why
Brief explanation.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only
```

---

## Code Comments

All public functions and types MUST have rustdoc comments:

```rust
/// Reconciles the BindZone custom resource.
///
/// # Arguments
/// * `zone` - The BindZone CR to reconcile
/// * `ctx` - Controller context with client and state
///
/// # Errors
/// Returns `ReconcileError` if DNS zone update fails or API is unreachable.
pub async fn reconcile(zone: Arc<BindZone>, ctx: Arc<Context>) -> Result<Action, ReconcileError> {
```

---

## Validation Checklist

- [ ] `.claude/CHANGELOG.md` updated with `**Author:**`
- [ ] All affected `docs/src/` pages updated
- [ ] All YAML examples validate: `kubectl apply --dry-run=client -f examples/`
- [ ] API docs regenerated if CRDs changed
- [ ] Architecture diagrams updated if structure changed
- [ ] `make docs` succeeds
