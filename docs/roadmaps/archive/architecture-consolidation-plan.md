# Architecture Documentation Consolidation Plan

**Status:** 📝 Planning
**Created:** 2026-01-25
**Author:** Claude Code (based on comprehensive analysis)

---

## Problem Statement

Bindy currently has **15 architecture-related files** totaling **6,374 lines** of documentation spread across multiple directories:

- `docs/src/guide/` (1 file - 522 lines)
- `docs/src/concepts/` (7 files - 3,387 lines)
- `docs/src/development/` (3 files - 702 lines)
- `docs/src/architecture/` (3 files - 1,422 lines)
- `docs/src/security/` (1 file - 741 lines)

**Issues:**
- No clear reading order or hierarchy
- Content overlap and duplication
- Confusing file names ("controller" vs "operator" for DNSZone)
- 1 deprecated file still present
- 1 empty stub file
- Users must read 10+ files to understand the system

---

## Current File Analysis

### By Line Count (Descending):

| Lines | File | Purpose | Action |
|-------|------|---------|--------|
| 741 | `security/architecture.md` | Security architecture | **KEEP** - Move to security section |
| 734 | `architecture/label-selector-reconciliation.md` | Label selector logic | **KEEP** - Technical reference |
| 720 | `concepts/architecture-http-api.md` | HTTP API protocol | **KEEP** - Protocol reference |
| 680 | `concepts/architecture.md` | High-level components | **CONSOLIDATE** → Core architecture |
| 651 | `concepts/architecture-rndc.md` | RNDC protocol | **KEEP** - Protocol reference |
| 618 | `architecture/reconciler-hierarchy.md` | Reconciler structure | **KEEP** - Developer reference |
| 595 | `concepts/architecture-diagrams.md` | Diagram collection | **MERGE** → Into relevant sections |
| 522 | `guide/architecture.md` | User-focused overview | **CONSOLIDATE** → User guide |
| 381 | `development/cluster-architecture.md` | Cluster patterns | **MERGE** → User guide |
| 381 | `concepts/architecture-technical.md` | Technical deep dive | **CONSOLIDATE** → Developer guide |
| 329 | `concepts/dnszone-controller-architecture.md` | DNSZone reconciler | **MERGE** → Reconciler hierarchy |
| 269 | `development/architecture.md` | **DEPRECATED** legacy | **DELETE** (marked deprecated) |
| 70 | `architecture/deployment.md` | Deployment patterns | **KEEP** - Ops reference |
| 52 | `development/architecture-deep-dive.md` | Deep dive stub | **MERGE** → Developer guide |
| 1 | `concepts/dnszone-operator-architecture.md` | **EMPTY STUB** | **DELETE** (empty) |

---

## Proposed Structure (4 Core Documents)

### 1. **User Guide: Architecture Overview** (`guide/architecture.md`)
**Audience:** Users, operators, platform engineers
**Purpose:** High-level understanding for deployment and usage
**Content:**
- Cluster models (namespace-scoped vs cluster-scoped)
- Multi-tenancy patterns
- Resource hierarchy (Cluster → Zone → Records)
- Basic reconciliation flow
- Deployment patterns

**Sources:**
- Keep: `guide/architecture.md` (522 lines) - **BASE**
- Merge: `development/cluster-architecture.md` (381 lines) - cluster patterns
- Merge: `architecture/deployment.md` (70 lines) - deployment info

**Estimated Size:** ~900 lines

---

### 2. **Developer Reference: Architecture Deep Dive** (`concepts/architecture.md`)
**Audience:** Contributors, developers, maintainers
**Purpose:** Technical understanding for development
**Content:**
- Component architecture (operator, reconcilers, controllers)
- Event-driven programming model
- Kubernetes watch/informer patterns
- State management
- Error handling and retry logic
- Testing architecture

**Sources:**
- Keep: `concepts/architecture.md` (680 lines) - **BASE**
- Merge: `concepts/architecture-technical.md` (381 lines) - technical details
- Merge: `development/architecture-deep-dive.md` (52 lines) - deep dive content
- Merge: `concepts/architecture-diagrams.md` (595 lines) - diagrams into sections

**Estimated Size:** ~1,200 lines

---

### 3. **Protocol Reference: Communication Architecture** (`concepts/architecture-protocols.md`)
**Audience:** Advanced users, security teams, architects
**Purpose:** Understanding how Bindy communicates
**Content:**
- RNDC protocol (HMAC-TSIG, command/response)
- HTTP API design (REST endpoints, authentication)
- DNS zone transfer protocols (AXFR/IXFR)
- Network security (mTLS, encryption)

**Sources:**
- Merge: `concepts/architecture-rndc.md` (651 lines)
- Merge: `concepts/architecture-http-api.md` (720 lines)

**Estimated Size:** ~1,400 lines (new file consolidating protocols)

---

### 4. **Reconciliation Reference** (`architecture/`)
**Audience:** Developers, advanced operators
**Purpose:** Understanding reconciliation logic
**Content:**
- Keep as separate technical references in `architecture/` directory

**Files:**
- `architecture/reconciler-hierarchy.md` (618 lines) - **KEEP AS-IS**
- `architecture/label-selector-reconciliation.md` (734 lines) - **KEEP AS-IS**
- Merge: `concepts/dnszone-controller-architecture.md` (329 lines) → Into reconciler-hierarchy.md

**Rationale:** These are technical references that developers will look up, not introductory material

---

### 5. **Security Architecture** (Already Separate)
- `security/architecture.md` (741 lines) - **KEEP AS-IS** (already well-organized)

---

## Files to Delete

1. **`development/architecture.md`** (269 lines)
   - Marked as deprecated
   - References legacy two-level operator architecture
   - Historical reference only

2. **`concepts/dnszone-operator-architecture.md`** (1 line)
   - Empty stub file
   - No content

---

## Consolidation Steps

### Phase 1.1: Delete Obsolete Files
```bash
rm docs/src/development/architecture.md
rm docs/src/concepts/dnszone-operator-architecture.md
```

### Phase 1.2: Consolidate User Guide
1. Read `guide/architecture.md` (base)
2. Extract relevant content from `development/cluster-architecture.md`
3. Extract relevant content from `architecture/deployment.md`
4. Merge into `guide/architecture.md`
5. Ensure clear reading flow for users

### Phase 1.3: Consolidate Developer Guide
1. Read `concepts/architecture.md` (base)
2. Extract relevant content from `concepts/architecture-technical.md`
3. Extract relevant content from `development/architecture-deep-dive.md`
4. Integrate diagrams from `concepts/architecture-diagrams.md`
5. Merge into `concepts/architecture.md`

### Phase 1.4: Create Protocol Reference
1. Merge `concepts/architecture-rndc.md` + `concepts/architecture-http-api.md`
2. Create new file: `concepts/architecture-protocols.md`
3. Organize by protocol type (RNDC, HTTP, DNS zone transfer)

### Phase 1.5: Consolidate Reconciliation Reference
1. Merge `concepts/dnszone-controller-architecture.md` → `architecture/reconciler-hierarchy.md`
2. Keep `architecture/label-selector-reconciliation.md` separate (focused topic)

### Phase 1.6: Update Navigation
Update `docs/mkdocs.yml` to reflect new structure:
```yaml
- Developer Guide:
    - Architecture:
        - Overview: guide/architecture.md  # User-focused
        - Technical Deep Dive: concepts/architecture.md  # Developer-focused
        - Protocols: concepts/architecture-protocols.md  # NEW
        - Reconciler Hierarchy: architecture/reconciler-hierarchy.md
        - Label Selector Reconciliation: architecture/label-selector-reconciliation.md
        - Security Architecture: security/architecture.md
```

---

## After Consolidation

### Files Remaining: 6 (down from 15)

| File | Lines | Purpose |
|------|-------|---------|
| `guide/architecture.md` | ~900 | User guide |
| `concepts/architecture.md` | ~1,200 | Developer guide |
| `concepts/architecture-protocols.md` | ~1,400 | Protocol reference (NEW) |
| `architecture/reconciler-hierarchy.md` | ~950 | Reconciliation logic |
| `architecture/label-selector-reconciliation.md` | 734 | Label selector logic |
| `security/architecture.md` | 741 | Security architecture |

**Total:** ~5,925 lines (down from 6,374 - saved ~450 lines of duplication)

**Files Deleted:** 9
**Files Created:** 1 (protocols)
**Files Updated:** 3 (guide, concepts, reconciler-hierarchy)

---

## Benefits

1. **Clear Reading Path**: Users start with guide, developers go to concepts, advanced topics in architecture/
2. **Reduced Duplication**: ~450 lines of duplicate content removed
3. **Better Organization**: Protocol documentation consolidated in one place
4. **Easier Maintenance**: Fewer files to keep in sync
5. **Improved Navigation**: Clear hierarchy in mkdocs.yml

---

## Risks & Mitigation

**Risk 1:** Breaking existing links
**Mitigation:** Use mkdocs redirect plugin or search for internal links before consolidation

**Risk 2:** Loss of detailed content during merge
**Mitigation:** Careful review of each source file, preserve all unique diagrams/examples

**Risk 3:** Files becoming too large
**Mitigation:** Structure with clear TOC, use internal anchors, keep related content together

---

## Next Steps

1. Get approval on consolidation plan
2. Execute Phase 1.1 (delete obsolete files)
3. Execute Phase 1.2 (consolidate user guide)
4. Execute Phase 1.3 (consolidate developer guide)
5. Execute Phase 1.4 (create protocol reference)
6. Execute Phase 1.5 (consolidate reconciliation reference)
7. Execute Phase 1.6 (update navigation)
8. Verify documentation builds
9. Update CHANGELOG.md

---

## Approval

- [ ] Plan reviewed
- [ ] Consolidation approach approved
- [ ] Ready to execute

**Estimated Time:** 4-6 hours for full consolidation
