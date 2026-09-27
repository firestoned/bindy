# Architecture Driven Development (ADD)

> **ADD is the governing methodology for bindy.** Architecture is designed,
> recorded, and visualized **before** code is written, and its security posture
> is re-verified **after**. ADRs, CALM diagrams, and the threat model are
> first-class deliverables — equal in importance to the code and the tests.

ADD layers *on top of* the existing TDD discipline (`rules/testing.md`); it does
not replace it. The order is fixed:

```
ADR  →  CALM  →  TDD  →  implement  →  docs  →  threat model
```

## The ADD cycle

For any **architecturally significant** change, complete each step before
starting the next:

### 1. ADR — decide and record (FIRST)

Write or update an Architecture Decision Record in
`docs/adr/NNNN-title.md` (lowercase-hyphen, zero-padded sequential number,
never renumbered).

**Metadata is a bullet list under the title, never a `## Status` section** —
one field per bullet, so status and date stay greppable rather than buried in
a prose paragraph:

```markdown
# NNNN — Title

- **Status:** Accepted
- **Date:** 2026-09-27
- **Proposed:** 2026-09-26          (when it sat Proposed first)
- **Deciders:** Erick Bourgeois
- **Amended:** 2026-09-10 (Decision #3, …)
- **Supersedes:** ADR-NNNN …
- **Related:** Extends [ADR-NNNN](…) …
```

`Status` and `Date` are required; the rest appear only when they apply. Then
the standard sections:

- **Context** — the forces, constraints, and the problem being solved
- **Decision** — what we will do, stated plainly
- **Consequences** — trade-offs, follow-ups, what this rules out

Status runs Proposed → Accepted (→ Superseded by NNNN). *Accepted* records
that the decision is made, not that it shipped — an Accepted ADR may carry an
explicit `Not implemented.` note.

Keep ADRs in the repo. One decision per ADR. If a change reverses an earlier
ADR, mark the old one *Superseded* and link forward. Existing ADRs 0001–0004
predate this rule and use `## Status` sections — migrate each to metadata
bullets the next time it is touched; do not renumber.

### 2. CALM — model and visualize

Update the FINOS CALM architecture models (`calm/*.architecture.json` —
control plane and/or multi-cluster) to reflect the decision: nodes,
relationships, interfaces, protocols. Then:

```sh
make calm-validate    # every model conforms to CALM 1.2 (hard gate, CI-enforced)
make calm-docs        # regenerate the Mermaid pages in docs/src/architecture/
make calm-docs-check  # verify the committed pages match the models (CI drift gate)
```

The architecture must be modeled and the diagrams must render cleanly **before**
implementation begins. A change that isn't reflected in CALM isn't designed yet.
See `calm/README.md` for the full editing workflow.

### 3. TDD — red / green / refactor

Only now write code, tests first, per `rules/testing.md` and the `tdd-workflow`
skill: failing test → minimum implementation → refactor. After any `.rs` change,
run the `cargo-quality` skill.

### 4. Docs — including **both** roadmap artefacts

Update `.claude/CHANGELOG.md` (with `**Author:**`) and any affected
`docs/src/` pages / examples, per `rules/documentation.md`. CRD changes
regenerate `deploy/operator/crds/` and the API reference (`regen-crds` skill,
then `regen-api-docs` skill LAST).

**If the work advanced a roadmap item, update both places, in this commit:**

1. the detail doc, `.github/community/NN-*.md` — tick the checkbox or update
   the phase-table row, and say what actually landed;
2. **`ROADMAPS.md`** at the repo root — the status board row.

They have different readers. The detail doc is the task list you work from;
`ROADMAPS.md` is the one-screen answer to "what state is this project in" and
is what gets read when deciding what to do *next*. A board that lags the tree
sends the next session to redo finished work, or to plan around a blocker that
no longer exists.

The trigger is **completion, not change**: if a checkbox is true now, tick it
now — even when the work that made it true was an earlier session's. And while
you are in the detail doc, **audit the rest of it against the tree**. "Done",
"superseded" and "still open" are three different answers and only the tree
knows which applies.

### 5. Threat model — full pass (LAST)

Once the ADR is implemented, make a **full pass** over
`docs/src/security/threat-model.md`. Walk every section — components, assets,
actors, trust boundaries (including diagrams), STRIDE tables, hardening
requirements, accepted risks — not just the one table that obviously changed.
Map every new or changed threat to a control that actually exists in `deploy/`
or `src/`, or record it as an accepted risk with a *Revisit when*.

Then bump the document's header stamp — the `**Last Updated:**` date, the
version, **and** a `Last full pass YYYY-MM-DD, against ADR-0001 … ADR-NNNN`
line. That stamp is the deliverable: an unchanged stamp means the pass did not
happen. "No change" is a valid conclusion, but it is still a pass — bump the
stamp and say so in the CHANGELOG.

**An ADR is not implemented until this pass is done.**

## When does ADD apply?

**Full ADR + CALM + post-implementation threat-model pass** (architecturally significant):

- New CRDs, controllers/reconcilers, or binaries
- Changes to a contract (the CRD schema surface, the bindcar API, the
  Scout → Queen Bee fan-in, RNDC/TSIG usage)
- New deploy / GitOps topology (FluxCD structure, kustomize layout,
  admission policies)
- Cross-cutting concerns: security boundaries, RBAC posture, TLS transport,
  failure domains, multi-cluster topology
- Any decision where "why A over B" is worth recording

**TDD only** (no ADR/CALM needed):

- Typos, comment/doc tweaks, formatting
- Isolated bug fixes with no architectural impact
- Mechanical refactors that preserve behavior and structure

> When unsure whether a change is "architectural," **write the ADR.** A short,
> slightly-redundant ADR costs little; an undocumented architectural decision
> costs the next person a re-derivation.

## Checklist (paste into the work)

- [ ] ADR written/updated in `docs/adr/NNNN-*.md` — metadata bullets
      (`- **Status:**` / `- **Date:**`), then Context/Decision/Consequences
- [ ] CALM model updated; `make calm-validate` passes; `make calm-docs` renders;
      `make calm-docs-check` clean
- [ ] Tests written **first**, then implementation (TDD)
- [ ] `cargo-quality` passes (fmt + clippy + test)
- [ ] CHANGELOG + docs updated
- [ ] Roadmap detail doc **and** `ROADMAPS.md` both updated for anything that
      completed (and the rest of the detail doc audited against the tree)
- [ ] Full threat-model pass done; header stamp bumped
