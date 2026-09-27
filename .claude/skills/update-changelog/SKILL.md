---
name: update-changelog
description: Prepend an audit entry to .claude/CHANGELOG.md. MANDATORY after ANY code change in this regulated-banking repo — every entry MUST carry an **Author:** line, no exceptions.
---

# update-changelog

Every change is auditable: prepend an entry to `.claude/CHANGELOG.md`
(newest first) in this exact format.

## Format

```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

**Author:** <Name of requester or approver>

### Changed
- `path/to/file.rs`: Description of the change

### Why
Brief explanation of the business or technical reason.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only
```

## Rules

- `**Author:**` is MANDATORY — the requester or approver (usually Erick
  Bourgeois), never Claude.
- Check the applicable `### Impact` boxes; a breaking change also needs a
  `docs/src/operations/migration-guide.md` entry (see `docs-sync-check`).
- New dependencies: record why the crate was chosen.

## Verification

Entry has the `**Author:**` line, a timestamp, and at least one `### Changed`
item.
