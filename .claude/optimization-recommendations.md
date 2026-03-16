# CLAUDE.md Optimization Recommendations

## Summary
Current size: ~1,524 lines
Target size: ~900-1,000 lines (40% reduction)
Approach: Remove redundancy, shorten examples, extract to separate files

---

## 1. REMOVE: Redundant/Overly Detailed Code Examples (~300 lines)

### Lines 970-1094: Early Return / Guard Clause Pattern
**Current:** 125 lines with extensive code examples
**Recommendation:** Reduce to ~30 lines with principles only

**Before:**
```markdown
#### Early Return / Guard Clause Pattern

**CRITICAL: Prefer early returns over nested if-else statements.**

[... 125 lines of detailed examples ...]
```

**After:**
```markdown
#### Early Return / Guard Clause Pattern

**CRITICAL: Prefer early returns over nested if-else statements.**

**Key Principles:**
- Handle preconditions first - validate and return early if invalid
- Minimize else statements - use early returns instead
- Use `?` for error propagation to keep happy path unindented
- Fail-fast approach catches invalid states early

**Example:**
```rust
// ✅ GOOD - Early return
pub async fn reconcile(needs_work: bool) -> Result<()> {
    if !needs_work {
        return Ok(()); // Early return
    }
    // Happy path continues
    do_work().await?;
    Ok(())
}
```

**Benefits:** Reduced nesting, clearer flow, easier testing
```

**Lines Saved:** ~95 lines

---

### Lines 1096-1195: Magic Numbers Rule
**Current:** 100 lines with extensive examples
**Recommendation:** Reduce to ~35 lines

**Before:**
```markdown
#### Magic Numbers Rule

**CRITICAL: Eliminate all magic numbers from the codebase.**

[... 100 lines of detailed examples and special cases ...]
```

**After:**
```markdown
#### Magic Numbers Rule

**CRITICAL: All numeric literals (except 0 and 1) MUST be named constants.**

**Why:** Readability, maintainability, semantic meaning

**Rules:**
- `0` and `1` are allowed (ubiquitous and self-explanatory)
- All other numbers MUST be named constants
- Use descriptive names explaining the *purpose*

**Example:**
```rust
// ✅ GOOD
const DEFAULT_ZONE_TTL: u32 = 3600;
const RECONCILE_INTERVAL_SECS: u64 = 300;

// ❌ BAD
let ttl = 3600;  // What does 3600 mean?
```

**Verification:** `grep -Ern '\b[2-9][0-9]*\b' src/ --include="*.rs" --exclude="*_tests.rs"`
```

**Lines Saved:** ~65 lines

---

### Lines 519-646: GitHub Workflows Reusable Pattern Examples
**Current:** 128 lines of detailed YAML examples
**Recommendation:** Reduce to ~40 lines with principles only

**After:**
```markdown
### CRITICAL: Workflows Must Be Reusable and Composable

**Requirements:**
1. **Use Reusable Workflows** (`.github/workflows/*.yml` with `workflow_call`)
2. **Use Composite Actions** (`.github/actions/*/action.yml`)
3. **Integration Strategy:** New workflows MUST be callable from existing workflows

**Key Principles:**
- Define workflows with `workflow_call` and inputs/outputs
- Support both `workflow_call` and `workflow_dispatch` triggers
- Extract shared logic into composite actions
- Avoid duplicating workflow logic across files

**Checklist:**
- [ ] Can this be added as a job to an existing workflow?
- [ ] Can this be made into a reusable workflow?
- [ ] Does this duplicate logic from an existing workflow?

**See:** `.github/workflows/` for existing patterns
```

**Lines Saved:** ~88 lines

---

## 2. CONSOLIDATE: Redundant CRD Sections (~100 lines)

CRD guidance appears in **3 separate sections:**
- Lines 20-52: Always Verify CRD Schema Sync
- Lines 330-404: High Priority CRD Code Generation + Documentation Examples
- Lines 1209-1276: CRD Development - Rust as Source of Truth

**Recommendation:** Merge into ONE comprehensive section

**After:**
```markdown
### 🚨 CRITICAL: CRD Development - Rust as Source of Truth

**MANDATORY:** Rust types in `src/crd.rs` are the source of truth. CRD YAMLs are **AUTO-GENERATED**.

**Before ANY Kubernetes issue investigation:**
> **How:** Run the `verify-crd-sync` skill.

**Workflow for CRD Changes:**
> **How:** Run the `regen-crds` skill, then `regen-api-docs` skill (LAST).

**After CRD changes, MUST update:**
- `/deploy/crds/*.crd.yaml` (auto-generated via `cargo run --bin crdgen`)
- `/examples/*.yaml` (validate with `kubectl apply --dry-run=client`)
- `/docs/src/` (any documentation referencing CRD fields)
- API docs (via `cargo run --bin crddoc > docs/src/reference/api.md`)

**REMEMBER:**
- ALWAYS read CRD schema BEFORE writing documentation examples
- NEVER guess field names - verify against `/deploy/crds/*.crd.yaml` or `src/crd.rs`
- Use `kubectl replace --force` not `kubectl apply` (Bind9Instance CRD exceeds 256KB)

**See:** `add-new-crd` skill for adding new CRDs
```

**Lines Saved:** ~70 lines (by eliminating duplicate content)

---

## 3. REMOVE: FluxCD Section (If Not Used) (~20 lines)

### Lines 1351-1371: FluxCD / GitOps Integration
**Question:** Is this actively used in the project?

**If NO:** Remove entirely (saves 20 lines)
**If YES:** Keep but simplify to 5-10 lines

---

## 4. CONSOLIDATE: Documentation Requirements (~150 lines)

Documentation guidance appears in **multiple sections:**
- Lines 666-784: Documentation Update Workflow (119 lines)
- Lines 786-813: Update Changelog (28 lines)
- Lines 815-828: Code Comments (14 lines)

**Recommendation:** Keep structure but remove redundant examples

**Shorten Lines 688-755:**
From 68 lines of detailed "What Documentation to Update" examples → 25 lines

**After:**
```markdown
#### What Documentation to Update

**For Controller/Reconciler Changes:** Update reconciliation flow diagrams, user guides, troubleshooting
**For CRD Changes:** Regenerate CRDs/API docs, update ALL examples, validate with kubectl
**For Core Logic Changes:** Update architecture docs, API docs, troubleshooting
**For New Features:** Add feature docs, examples, architecture diagrams, troubleshooting
**For Bug Fixes:** Update troubleshooting guides, document workarounds

**Always verify:**
```bash
make docs && kubectl apply --dry-run=client -f examples/
```
```

**Lines Saved:** ~43 lines

---

## 5. SIMPLIFY: Docker/Kubernetes Restrictions (~40 lines)

### Lines 150-193: CRITICAL: Docker and Kubernetes Operations Restrictions
**Current:** 44 lines with detailed examples
**Recommendation:** Reduce to ~20 lines

**After:**
```markdown
## 🚫 CRITICAL: Docker and Kubernetes Operations Restrictions

**NEVER build or push Docker images yourself. The user handles all Docker image operations.**

**Allowed kubectl Operations:**
- ✅ Read-only: `kubectl get/describe/logs`
- ✅ `kubectl annotate`

**FORBIDDEN Operations:**
- ❌ `docker build/push/tag`
- ❌ `kind load`
- ❌ `kubectl rollout restart/delete pods/apply/patch` (unless explicitly requested)

**What to do instead:**
1. Make code changes and run `cargo fmt`, `cargo clippy`, `cargo test`
2. Inform the user that changes are ready for deployment
3. Let the user handle building, pushing, and deploying
```

**Lines Saved:** ~24 lines

---

## 6. MOVE TO SEPARATE FILE: GitHub Workflows (~200 lines)

**Lines 408-646: GitHub Workflows & CI/CD**

**Recommendation:** Extract to `.claude/rules/github-workflows.md`

**Benefits:**
- Reduces main CLAUDE.md by ~200 lines
- Separates concerns (CI/CD vs coding standards)
- Can be referenced with `@.claude/rules/github-workflows.md`

---

## 7. REMOVE: "Things to Avoid" Section (~6 lines)

**Lines 1480-1486: Things to Avoid**

**Recommendation:** Remove - already covered by Rust Style Guidelines

---

## 8. SIMPLIFY: File Organization (~50 lines)

**Lines 1428-1476: File Organization**

**Current:** 49 lines with full directory tree
**Recommendation:** Reduce to ~20 lines

**After:**
```markdown
## 📁 File Organization

**Source Files:**
- `src/*.rs` - Main source code
- `src/*_tests.rs` - Unit tests (MUST exist for every source file)
- `src/reconcilers/` - Controller reconciliation logic
- `src/bin/` - Binary utilities (crdgen, crddoc)

**Documentation:**
- `docs/roadmaps/` - ALL roadmaps and planning docs (MANDATORY location)
- `docs/adr/` - Architecture Decision Records
- `docs/src/` - User-facing documentation

**Test Pattern:** Every `foo.rs` → `foo_tests.rs` (same directory)
**Roadmap Naming:** lowercase, hyphens only (e.g., `integration-test-plan.md`)
```

**Lines Saved:** ~29 lines

---

## Summary of Savings

| Section | Current Lines | After | Saved |
|---------|---------------|-------|-------|
| Early Return examples | 125 | 30 | 95 |
| Magic Numbers examples | 100 | 35 | 65 |
| GitHub Workflows examples | 128 | 40 | 88 |
| CRD consolidation | 235 | 165 | 70 |
| FluxCD section | 20 | 0 | 20 |
| Documentation examples | 119 | 76 | 43 |
| Docker/K8s restrictions | 44 | 20 | 24 |
| GitHub Workflows (move) | 238 | 0 | 238 |
| Things to Avoid | 6 | 0 | 6 |
| File Organization | 49 | 20 | 29 |
| **TOTAL** | **1,064** | **386** | **~678 lines** |

**Expected Final Size:** ~850-900 lines (from 1,524 lines = 44% reduction)

---

## Implementation Plan

1. **Phase 1: Quick Wins (Save ~200 lines)**
   - Remove "Things to Avoid" section
   - Simplify Early Return examples
   - Simplify Magic Numbers examples

2. **Phase 2: Consolidation (Save ~150 lines)**
   - Merge 3 CRD sections into one
   - Simplify Documentation sections

3. **Phase 3: Extraction (Save ~300 lines)**
   - Move GitHub Workflows to `.claude/rules/github-workflows.md`
   - Simplify File Organization
   - Remove FluxCD (if not used)

---

## Alternative: Modular Structure

Instead of one large CLAUDE.md, split into:

```
.claude/
├── CLAUDE.md                    # Core principles (300 lines)
├── SKILL.md                     # Procedural workflows (existing)
├── rules/
│   ├── rust-style.md           # Rust coding standards
│   ├── testing.md              # Testing requirements
│   ├── documentation.md        # Documentation standards
│   ├── kubernetes.md           # K8s operator patterns
│   └── github-workflows.md     # CI/CD workflows
```

Reference in CLAUDE.md:
```markdown
@.claude/rules/rust-style.md
@.claude/rules/testing.md
@.claude/rules/documentation.md
@.claude/rules/kubernetes.md
@.claude/rules/github-workflows.md
```

This approach gives you **maximum flexibility** while keeping each file focused and manageable.
