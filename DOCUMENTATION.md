# Bindy Documentation Guide

This document provides information about the Bindy documentation system.

## Documentation Overview

Bindy uses a dual documentation system:

1. **User & Developer Guide** (mdBook) - Comprehensive guides, tutorials, and examples
2. **API Reference** (rustdoc) - Generated from Rust source code documentation

Both are combined and deployed to GitHub Pages at: **https://firestoned.github.io/bindy/**

## Quick Start

### View Documentation Locally

Build and serve the documentation:

```bash
make docs-serve
```

Then open http://localhost:3000 in your browser.

### Build Documentation

Build all documentation:

```bash
make docs
```

Output will be in `docs/target/site/`.

## Documentation Structure

```
bindy/
├── book.toml                      # mdBook configuration
├── docs/
│   ├── README.md                  # Documentation guide
│   ├── book/
│   │   ├── src/                   # mdBook source files
│   │   │   ├── SUMMARY.md         # Table of contents
│   │   │   ├── introduction.md    # Introduction page
│   │   │   ├── installation/      # Installation guides
│   │   │   ├── concepts/          # Core concepts
│   │   │   ├── guide/             # User guides
│   │   │   ├── operations/        # Operations guides
│   │   │   ├── advanced/          # Advanced topics
│   │   │   ├── development/       # Developer guides
│   │   │   └── reference/         # Reference documentation
│   │   └── theme/
│   │       └── custom.css         # Custom styling
│   └── target/                    # Build output (gitignored)
│       ├── book/                  # mdBook output
│       └── site/                  # Combined documentation site
├── src/                           # Rust source code
│   └── lib.rs                     # Main documentation comments
└── target/
    └── doc/                       # rustdoc output
        └── bindy/                 # API documentation
```

## Available Make Targets

### Build Targets

- `make docs` - Build all documentation (mdBook + rustdoc) for local use
- `make docs-github-pages` - Build documentation for GitHub Pages deployment (includes redirects)
- `make docs-mdbook` - Build only mdBook documentation
- `make docs-rustdoc` - Build only rustdoc API documentation

### Development Targets

- `make docs-serve` - Build and serve documentation at http://localhost:3000
- `make docs-watch` - Watch for changes and rebuild mdBook automatically with live reload
- `make docs-clean` - Clean all documentation build artifacts

### Quick Reference

```bash
# Install mdbook (one-time setup)
cargo install mdbook

# Build everything
make docs

# Build for GitHub Pages deployment
make docs-github-pages

# Develop documentation with live reload
make docs-watch

# Build and serve for preview
make docs-serve

# Clean build artifacts
make docs-clean
```

## Writing Documentation

### mdBook Pages

1. Create a new Markdown file in `docs/book/src/`
2. Add it to `docs/book/src/SUMMARY.md`
3. Write content using GitHub-flavored Markdown
4. Test locally with `make docs-watch`

Example page structure:

```markdown
# Page Title

Introduction paragraph.

## Section Heading

Content with inline `code` and:

\`\`\`yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example
\`\`\`

See [related page](./other-page.md) for more information.
```

### API Documentation (rustdoc)

Add documentation comments to Rust code:

```rust
/// Brief description of the function.
///
/// More detailed explanation with examples.
///
/// # Examples
///
/// \`\`\`
/// use bindy::crd::DNSZone;
/// let zone = DNSZone::new("example.com");
/// \`\`\`
///
/// # Errors
///
/// Returns an error if the zone name is invalid.
pub fn create_zone(name: &str) -> Result<DNSZone, Error> {
    // ...
}
```

## GitHub Pages Deployment

Documentation is automatically deployed via GitHub Actions:

- **Workflow**: `.github/workflows/docs.yaml`
- **Build Script**: Uses `make docs-github-pages` target
- **Trigger**: Push to `main` branch or pull requests
- **Deploy**: GitHub Pages at https://firestoned.github.io/bindy/

### Deployment Process

1. Push changes to `main` branch
2. GitHub Actions runs:
   - Sets up Rust toolchain and mdBook
   - Executes `make docs-github-pages` which:
     - Builds rustdoc API documentation
     - Builds mdBook user documentation
     - Combines both into a single site
     - Creates rustdoc redirect page
   - Deploys to GitHub Pages (on `main` branch only)
3. Documentation available at https://firestoned.github.io/bindy/

All build logic is centralized in the Makefile for consistency between local and CI builds.

### Manual Deployment

To deploy manually (not recommended):

```bash
# Build documentation
make docs

# The site is in docs/target/site/
# Deploy this directory to GitHub Pages
```

## Documentation Sections

### User Documentation

**Getting Started**
- [Installation](docs/book/src/installation/installation.md) - Install Bindy
- [Quick Start](docs/book/src/installation/quickstart.md) - First DNS zone
- [Prerequisites](docs/book/src/installation/prerequisites.md) - Requirements

**Concepts**
- [Architecture](docs/book/src/concepts/architecture.md) - System design
- [CRDs](docs/book/src/concepts/crds.md) - Custom resources
- [Bind9Instance](docs/book/src/concepts/bind9instance.md) - DNS instances
- [DNSZone](docs/book/src/concepts/dnszone.md) - DNS zones
- [Records](docs/book/src/concepts/records.md) - DNS record types

**User Guides**
- [Creating Infrastructure](docs/book/src/guide/infrastructure.md)
- [Managing Zones](docs/book/src/guide/zones.md)
- [Managing Records](docs/book/src/guide/records-guide.md)

**Operations**
- [Configuration](docs/book/src/operations/configuration.md)
- [Monitoring](docs/book/src/operations/monitoring.md)
- [Troubleshooting](docs/book/src/operations/troubleshooting.md)

### Developer Documentation

**Development**
- [Setup](docs/book/src/development/setup.md) - Development environment
- [Building](docs/book/src/development/building.md) - Build from source
- [Testing](docs/book/src/development/testing.md) - Run tests
- [Contributing](docs/book/src/development/contributing.md) - Contribution guide

**Architecture**
- [Deep Dive](docs/book/src/development/architecture-deep-dive.md)
- [Controller Design](docs/book/src/development/controller-design.md)
- [Reconciliation](docs/book/src/development/reconciliation.md)

### Reference

**API Reference**
- [rustdoc](target/doc/bindy/index.html) - Complete API documentation
- [Bind9Instance Spec](docs/book/src/reference/bind9instance-spec.md)
- [DNSZone Spec](docs/book/src/reference/dnszone-spec.md)
- [Examples](docs/book/src/reference/examples.md)

## Documentation Standards

### Writing Style

- **Clear and concise** - Get to the point quickly
- **Active voice** - "Create a zone" not "A zone is created"
- **Present tense** - "Bindy creates zones" not "Bindy will create zones"
- **Second person** - "You can create" not "One can create"

### Code Examples

- **Complete** - Include all necessary imports and context
- **Tested** - All examples should work
- **Realistic** - Use realistic names and values
- **Annotated** - Add comments to explain non-obvious parts

### Markdown Guidelines

- Use ATX-style headings (`#` not `===`)
- Use fenced code blocks with language tags
- Use relative links for cross-references
- Include alt text for images
- Use tables for structured data
- Use lists for sequential or grouped items

## Troubleshooting

### mdBook not found

Install mdBook:

```bash
cargo install mdbook
```

Or use the Makefile which will install it automatically:

```bash
make docs
```

### Build fails

1. Clean and rebuild:
   ```bash
   make docs-clean
   make docs
   ```

2. Check for syntax errors in Markdown files
3. Verify SUMMARY.md links are correct
4. Check book.toml configuration

### Documentation not updating

- Clear browser cache
- Rebuild documentation: `make docs-clean && make docs`
- Verify file was saved
- Check that SUMMARY.md includes the page

## Resources

- **mdBook Documentation**: https://rust-lang.github.io/mdBook/
- **rustdoc Book**: https://doc.rust-lang.org/rustdoc/
- **Markdown Guide**: https://www.markdownguide.org/
- **GitHub Pages**: https://pages.github.com/

## Support

For documentation issues:

- **Questions**: [GitHub Discussions](https://github.com/firestoned/bindy/discussions)
- **Bugs**: [GitHub Issues](https://github.com/firestoned/bindy/issues)
- **Improvements**: [Pull Requests](https://github.com/firestoned/bindy/pulls)
