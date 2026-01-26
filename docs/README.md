# Bindy Documentation

This directory contains the comprehensive documentation for Bindy, the BIND9 DNS Operator for Kubernetes.

## Documentation Structure

- **`book/`** - mdBook source files for user and developer documentation
- **`target/`** - Build output (gitignored)

## Building Documentation

### Prerequisites

1. **mdBook** - Install with cargo:
   ```bash
   cargo install mdbook
   ```

2. **Rust toolchain** - For generating API documentation

### Build All Documentation

Build both mdBook and rustdoc:

```bash
make docs
```

This creates a combined documentation site in `docs/site/` with:
- User and developer guides (MkDocs Material)
- API reference (rustdoc) at `/rustdoc/`

### Build and Serve Locally

Build and serve documentation at http://localhost:3000:

```bash
make docs-serve
```

Or use mdBook's built-in server with live reload:

```bash
make docs-watch
```

This serves at http://localhost:3000 and automatically rebuilds on changes.

### Build Individual Components

Build only mdBook documentation:

```bash
make docs-mdbook
```

Build only rustdoc API documentation:

```bash
make docs-rustdoc
```

## Documentation Organization

### User Documentation

Located in `book/src/`:

- **Getting Started**
  - Installation guides
  - Quick start tutorial
  - Basic concepts

- **User Guide**
  - Creating DNS infrastructure
  - Managing zones and records
  - Production best practices

- **Operations**
  - Configuration
  - Monitoring
  - Troubleshooting

### Developer Documentation

- **Development Setup**
  - Building from source
  - Running tests
  - Development workflow

- **Architecture**
  - Operator design
  - Reconciliation logic
  - BIND9 integration

- **Contributing**
  - Code style
  - Testing guidelines
  - Pull request process

### API Reference

Generated from Rust source code documentation comments using rustdoc.

## Writing Documentation

### mdBook Pages

Create new pages in `book/src/` and add them to `book/src/SUMMARY.md`.

Use Markdown with these extensions:
- GitHub-flavored Markdown
- Code blocks with syntax highlighting
- Tables
- Task lists

Example:

```markdown
# Page Title

Introduction paragraph.

## Section

Content with `inline code` and:

\`\`\`yaml
# YAML code block
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
\`\`\`

See [other page](./other-page.md) for more.
```

### API Documentation

Add documentation comments to Rust code:

```rust
/// Brief description of the function.
///
/// More detailed explanation with examples:
///
/// # Examples
///
/// \`\`\`
/// use bindy::crd::DNSZone;
/// let zone = DNSZone::new();
/// \`\`\`
///
/// # Errors
///
/// Returns an error if...
pub fn example() -> Result<(), Error> {
    // ...
}
```

## GitHub Pages Deployment

Documentation is automatically built and deployed to GitHub Pages on every push to the `main` branch via the `.github/workflows/docs.yaml` workflow.

The live documentation is available at: https://firestoned.github.io/bindy/

### Manual Deployment

To deploy manually:

1. Build documentation: `make docs`
2. Push to the `gh-pages` branch (automated by CI/CD)

## Troubleshooting

### mdBook not found

Install mdBook:

```bash
cargo install mdbook
```

### Python not found (for docs-serve)

The `docs-serve` target uses Python's built-in HTTP server. Install Python 3 or use:

```bash
make docs-watch
```

This uses mdBook's built-in server instead.

### Documentation not updating

Clean and rebuild:

```bash
make docs-clean
make docs
```

## Contributing to Documentation

1. Make changes to files in `book/src/`
2. Test locally: `make docs-watch`
3. Verify the changes look correct
4. Commit and create a pull request

Documentation follows the same contribution guidelines as code:
- Clear, concise writing
- Proper grammar and spelling
- Tested code examples
- Linked cross-references

## Resources

- [mdBook Documentation](https://rust-lang.github.io/mdBook/)
- [Rustdoc Book](https://doc.rust-lang.org/rustdoc/)
- [Markdown Guide](https://www.markdownguide.org/)
