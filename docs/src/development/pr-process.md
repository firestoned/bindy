# Pull Request Process

Process for submitting and reviewing pull requests.

## Before Submitting

1. **Create issue** (for non-trivial changes)
2. **Create branch** from main
3. **Make changes** with tests
4. **Run checks** locally:
```bash
cargo test
cargo clippy
cargo fmt
```

## PR Requirements

- [ ] Tests pass
- [ ] Code formatted
- [ ] Documentation updated
- [ ] Commit messages clear
- [ ] PR description complete

## PR Template

```markdown
## Description
Brief description of changes

## Related Issue
Fixes #123

## Changes
- Added feature X
- Fixed bug Y

## Testing
How changes were tested

## Checklist
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] Changelog updated (if needed)
```

## Review Process

1. **Automated checks** must pass
2. **Maintainer review** required
3. **Address feedback**
4. **Merge** when approved

## After Merge

Changes included in next release.
