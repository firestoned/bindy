---
name: build-docs
description: Build the Bindy documentation site. Use whenever docs need building or verifying — after editing anything under docs/src/, docs/mkdocs.yml, or rustdoc comments, and before marking any documentation task complete. Always use this instead of running mkdocs build directly.
---

# build-docs

Build the full documentation site — MkDocs user guide plus rustdoc API
reference — exactly the way CI does.

## The one command

```bash
make docs
```

**Never run `mkdocs build` directly.** `make docs` wraps it with the required
extras (Poetry-managed environment, rustdoc copy into `docs/site/rustdoc/`,
index redirect). A bare `mkdocs build` produces an incomplete site and can
succeed where the real build would fail.

## Procedure

1. Run `make docs` from the repo root.
2. **Success criterion:** the run ends with
   `✓ Documentation built successfully in docs/site/`. A non-zero exit is a
   failure — fix it before the task is complete.
3. **Check the WARNING lines** for the files you touched. Pre-existing
   warnings (broken relative links out of `docs_dir`, e.g. to `SECURITY.md`
   or `Cargo.toml`) are known noise — but any NEW warning mentioning a file
   you changed must be fixed, typically a bad relative link or a page missing
   from the `docs/mkdocs.yml` nav.
4. Spot-check output only when the change was structural (nav, new page):
   the built site lands in `docs/site/` (git-ignored — never commit it).

## Related

- New pages must be added to the `nav:` section of `docs/mkdocs.yml` or they
  build but stay unreachable.
- Full documentation workflow: `.claude/rules/documentation.md`.
