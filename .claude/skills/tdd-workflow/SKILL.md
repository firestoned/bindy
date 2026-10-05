---
name: tdd-workflow
description: The mandatory RED → GREEN → REFACTOR Test-Driven Development cycle for this repo. Use when adding any feature or function, fixing a bug, or refactoring. Tests are written FIRST, in separate _tests.rs files, never embedded in source files.
---

# tdd-workflow

TDD is mandatory for all code changes (see `rules/testing.md`). Write failing
tests first, then the minimum implementation, then refactor.

## RED — write failing tests first (before any implementation)

Edit `src/<module>_tests.rs` — add test(s) that define the expected behavior.

```bash
cargo test <test_name>   # Must FAIL at this point
```

## GREEN — implement minimum code to pass

Edit `src/<module>.rs` — write the simplest code that makes the tests pass.

```bash
cargo test <test_name>   # Must PASS now
```

## REFACTOR — improve while keeping tests green

Extract constants, add rustdoc, improve error handling.

```bash
cargo test               # Must still PASS
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -A clippy::module_name_repetitions
```

## Test file pattern (required)

- Source: `crates/<crate>/src/foo.rs` → declare `#[cfg(test)] mod foo_tests;` at the bottom
- Tests: `crates/<crate>/src/foo_tests.rs` → wrap in `#[cfg(test)] mod tests { use super::super::*; ... }`
- Never embed `#[cfg(test)] mod tests` blocks inside the source file itself.

## Coverage requirements

- Success path, every failure path, edge cases (empty, boundary, null)
- Descriptive names (`test_reconcile_creates_zone_when_missing`)
- Arrange-Act-Assert structure; mock external dependencies; deterministic

## Exceptions

Only exploratory/prototype code (marked as such, removed before merging) and
behavior-preserving mechanical refactors covered by existing tests.

## Verification

All tests pass, clippy is clean, and tests cover success + error + edge paths.
Finish with the `cargo-quality` skill.
