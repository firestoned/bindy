# MkDocs Migration Roadmap

**Status:** Proposed
**Date:** 2026-01-22
**Author:** Erick Bourgeois
**Impact:** Documentation infrastructure and developer workflow

## Executive Summary

This roadmap outlines the migration from mdBook to MkDocs Material for the Bindy documentation. MkDocs Material provides superior features, better search, improved navigation, and a more modern documentation experience while maintaining compatibility with existing Markdown content.

## Why MkDocs Material?

### Advantages Over mdBook

1. **Superior Features**:
   - Advanced search with instant previews
   - Built-in versioning support
   - Social cards for better sharing
   - Improved navigation with tabs and sections
   - Better mobile experience
   - Code annotations and content tabs
   - Admonitions (notes, warnings, tips) with better styling

2. **Better Ecosystem**:
   - Larger community (Python ecosystem)
   - More plugins and extensions
   - Better maintained (active development)
   - Extensive theme customization

3. **Enhanced Developer Experience**:
   - Live reload with better performance
   - Better syntax highlighting
   - Easier customization with Python
   - Better integration with CI/CD

4. **Compliance & Auditing**:
   - Better support for versioned documentation
   - Improved search for audit requirements
   - Better print/PDF generation
   - Enhanced metadata support

### Trade-offs

- **Learning Curve**: Team needs to learn MkDocs configuration (minimal, Python-based)
- **Build Dependencies**: Requires Python instead of Rust tooling
- **Migration Effort**: Initial time investment to migrate configuration and test

## Migration Strategy

### Phase 1: Preparation & Setup (Week 1)

#### 1.1 Environment Setup

**Tasks**:
- [ ] Install MkDocs Material and dependencies
- [ ] Create initial `mkdocs.yml` configuration
- [ ] Set up Python virtual environment for documentation
- [ ] Document installation instructions for contributors

**Deliverables**:
```bash
# Install MkDocs Material
pip install mkdocs-material

# Install additional plugins
pip install mkdocs-mermaid2-plugin
pip install mkdocs-git-revision-date-localized-plugin
pip install mkdocs-minify-plugin
pip install mkdocs-redirects
pip install mkdocs-macros-plugin
```

**Files to Create**:
- `requirements-docs.txt` - Python dependencies for documentation
- `mkdocs.yml` - Initial MkDocs configuration
- `docs/.python-version` - Pin Python version for consistency

#### 1.2 Configuration Mapping

**Tasks**:
- [ ] Map mdBook `book.toml` settings to `mkdocs.yml`
- [ ] Configure Material theme with similar appearance to current docs
- [ ] Set up Mermaid diagram support
- [ ] Configure search settings
- [ ] Map custom CSS and JavaScript

**Key Configuration Areas**:

```yaml
# mkdocs.yml initial structure
site_name: Bindy - BIND9 DNS Operator for Kubernetes
site_url: https://firestoned.github.io/bindy/
repo_url: https://github.com/firestoned/bindy
repo_name: firestoned/bindy
edit_uri: edit/main/docs/

theme:
  name: material
  palette:
    # Light mode
    - scheme: default
      primary: indigo
      accent: indigo
      toggle:
        icon: material/brightness-7
        name: Switch to dark mode
    # Dark mode
    - scheme: slate
      primary: indigo
      accent: indigo
      toggle:
        icon: material/brightness-4
        name: Switch to light mode

  features:
    - navigation.instant
    - navigation.tracking
    - navigation.tabs
    - navigation.sections
    - navigation.expand
    - navigation.top
    - search.suggest
    - search.highlight
    - content.code.copy
    - content.code.annotate

plugins:
  - search:
      lang: en
  - mermaid2
  - git-revision-date-localized:
      enable_creation_date: true
  - minify:
      minify_html: true
  - redirects:
      redirect_maps: {}

markdown_extensions:
  - pymdownx.highlight:
      anchor_linenums: true
  - pymdownx.superfences:
      custom_fences:
        - name: mermaid
          class: mermaid
          format: !!python/name:mermaid2.fence_mermaid
  - pymdownx.tabbed:
      alternate_style: true
  - admonition
  - pymdownx.details
  - pymdownx.emoji:
      emoji_index: !!python/name:material.extensions.emoji.twemoji
  - attr_list
  - def_list
  - tables
  - footnotes
  - toc:
      permalink: true
```

#### 1.3 Content Inventory

**Tasks**:
- [ ] Audit all existing Markdown files
- [ ] Identify mdBook-specific syntax that needs conversion
- [ ] Document custom components (mermaid diagrams, code blocks)
- [ ] Identify files requiring restructuring

**Audit Checklist**:
- Total pages: ~100+ (from SUMMARY.md)
- Mermaid diagrams: Multiple architecture diagrams
- Code blocks: Extensive (Rust, YAML, Bash)
- Custom CSS: `theme/custom.css`
- Custom JS: `mermaid.min.js`, `mermaid-init.js`, `page-toc.js`

### Phase 2: Content Migration (Week 2-3)

#### 2.1 Navigation Structure

**Tasks**:
- [ ] Convert `SUMMARY.md` to `mkdocs.yml` nav structure
- [ ] Organize content into logical sections
- [ ] Set up navigation tabs for main sections
- [ ] Configure navigation hierarchy

**Navigation Mapping** (from SUMMARY.md):

```yaml
nav:
  - Home: index.md
  - Getting Started:
      - Introduction: introduction.md
      - Installation:
          - Overview: installation/index.md
          - Prerequisites: installation/prerequisites.md
          - Quick Start: installation/quickstart.md
          - Installing CRDs: installation/crds.md
          - Deploying Operator: installation/operator.md
      - Basic Concepts:
          - Overview: concepts/index.md
          - Architecture: concepts/architecture.md
          - Technical Architecture: concepts/architecture-technical.md
          - RNDC Architecture: concepts/architecture-rndc.md
          - Architecture Diagrams: concepts/architecture-diagrams.md
          - DNSZone Architecture: concepts/dnszone-operator-architecture.md
          - Custom Resources: concepts/crds.md
          - Bind9Cluster: concepts/bind9cluster.md
          - ClusterBind9Provider: concepts/clusterbind9provider.md
          - Bind9Instance: concepts/bind9instance.md
          - DNSZone: concepts/dnszone.md
          - DNS Records: concepts/records.md

  - User Guide:
      - Architecture: guide/architecture.md
      - Multi-Tenancy: guide/multi-tenancy.md
      - Choosing Cluster Type: guide/choosing-cluster-type.md
      - Creating Infrastructure:
          - Overview: guide/infrastructure.md
          - Primary Instances: guide/primary-instance.md
          - Secondary Instances: guide/secondary-instance.md
          - Multi-Region: guide/multi-region.md
      - Managing Zones:
          - Overview: guide/zones.md
          - Creating Zones: guide/creating-zones.md
          - Zone Selection: guide/zone-selection.md
          - Cluster References: guide/label-selectors.md
          - Zone Configuration: guide/zone-config.md
      - Managing Records:
          - Overview: guide/records-guide.md
          - A Records: guide/a-records.md
          - AAAA Records: guide/aaaa-records.md
          - CNAME Records: guide/cname-records.md
          - MX Records: guide/mx-records.md
          - TXT Records: guide/txt-records.md
          - NS Records: guide/ns-records.md
          - SRV Records: guide/srv-records.md
          - CAA Records: guide/caa-records.md

  - Operations:
      - Configuration:
          - Overview: operations/configuration.md
          - Environment Variables: operations/env-vars.md
          - RBAC: operations/rbac.md
          - Resource Limits: operations/resources.md
      - Monitoring:
          - Overview: operations/monitoring.md
          - Status Conditions: operations/status.md
          - Logging: operations/logging.md
          - Log Levels: operations/log-level-change.md
          - Metrics: operations/metrics.md
      - Troubleshooting:
          - Overview: operations/troubleshooting.md
          - Error Handling: operations/error-handling.md
          - Common Issues: operations/common-issues.md
          - DNSZone Migration: operations/dnszone-migration-troubleshooting.md
          - Debugging: operations/debugging.md
          - FAQ: operations/faq.md
      - Migration:
          - v0.2.x → v0.3.x: operations/migration-guide.md

  - Advanced Topics:
      - Replacing CoreDNS: advanced/coredns-replacement.md
      - High Availability:
          - Overview: advanced/ha.md
          - Zone Transfers: advanced/zone-transfers.md
          - Replication: advanced/replication.md
      - Security:
          - Overview: advanced/security.md
          - DNSSEC: advanced/dnssec.md
          - Access Control: advanced/access-control.md
      - Performance:
          - Overview: advanced/performance.md
          - Tuning: advanced/tuning.md
          - Benchmarking: advanced/benchmarking.md
      - Integration:
          - Overview: advanced/integration.md
          - External DNS: advanced/external-dns.md
          - Service Discovery: advanced/service-discovery.md

  - Developer Guide:
      - Development Setup:
          - Overview: development/setup.md
          - Building: development/building.md
          - Testing: development/testing.md
          - Testing Guide: development/testing-guide.md
          - Coverage: development/test-coverage.md
          - Workflow: development/workflow.md
          - GitHub Pages: development/github-pages-setup.md
      - Architecture:
          - Deep Dive: development/architecture-deep-dive.md
          - Operator Design: development/operator-design.md
          - Reconciliation: development/reconciliation.md
          - Reconciler Hierarchy: architecture/reconciler-hierarchy.md
          - BIND9 Integration: development/bind9-integration.md
      - Contributing:
          - Overview: development/contributing.md
          - Code Style: development/code-style.md
          - Testing Guidelines: development/testing-guidelines.md
          - PR Process: development/pr-process.md

  - Security & Compliance:
      - Overview: security-compliance-overview.md
      - Security:
          - Architecture: security/architecture.md
          - Threat Model: security/threat-model.md
          - Signed Releases: security/signed-releases.md
          - Incident Response: security/incident-response.md
          - Vulnerability Management: security/vulnerability-management.md
          - Build Reproducibility: security/build-reproducibility.md
          - Secret Access Audit: security/secret-access-audit.md
          - Audit Log Retention: security/audit-log-retention.md
      - Compliance:
          - Overview: compliance/overview.md
          - SOX 404: compliance/sox-404.md
          - PCI-DSS: compliance/pci-dss.md
          - Basel III: compliance/basel-iii.md
          - SLSA: compliance/slsa.md
          - NIST Framework: compliance/nist.md

  - Reference:
      - API Reference: reference/api.md
      - Specifications:
          - Bind9Cluster: reference/bind9cluster-spec.md
          - Bind9Instance: reference/bind9instance-spec.md
          - DNSZone: reference/dnszone-spec.md
          - DNS Records: reference/record-specs.md
          - Status Conditions: reference/status-conditions.md
      - Examples:
          - Overview: reference/examples.md
          - Simple Setup: reference/examples-simple.md
          - Production Setup: reference/examples-production.md
          - Multi-Region: reference/examples-multi-region.md
      - Rustdoc: rustdoc.md
      - Changelog: changelog.md
      - License: license.md
```

#### 2.2 Content Conversion

**Tasks**:
- [ ] Convert mdBook-specific syntax to MkDocs syntax
- [ ] Update admonitions to Material syntax
- [ ] Verify Mermaid diagrams render correctly
- [ ] Update internal links
- [ ] Convert code block annotations

**Conversion Examples**:

**Admonitions** (mdBook → MkDocs Material):
```markdown
# mdBook style (if used)
> **Note**: This is a note

# MkDocs Material style
!!! note
    This is a note

!!! warning "Custom Title"
    This is a warning with custom title

!!! tip "Pro Tip"
    This is a helpful tip
```

**Code Blocks** (enhanced in Material):
```markdown
# Basic code block (same)
```rust
fn main() {}
```

# With line numbers and highlighting
```rust linenums="1" hl_lines="2 3"
fn main() {
    println!("Hello");  // (1)!
}
```

1. This is a code annotation
```

**Tabs** (new in Material):
```markdown
=== "Rust"
    ```rust
    fn main() {}
    ```

=== "YAML"
    ```yaml
    apiVersion: v1
    kind: Pod
    ```
```

#### 2.3 Asset Migration

**Tasks**:
- [ ] Move custom CSS to `docs/stylesheets/extra.css`
- [ ] Move custom JavaScript to `docs/javascripts/`
- [ ] Update Mermaid initialization for MkDocs
- [ ] Optimize images and diagrams
- [ ] Set up static file handling

**File Structure**:
```
docs/
├── stylesheets/
│   └── extra.css           # Custom CSS (from theme/custom.css)
├── javascripts/
│   └── extra.js            # Custom JS if needed
├── assets/
│   └── images/             # Images and diagrams
└── overrides/
    └── main.html           # Template overrides if needed
```

### Phase 3: Integration & Automation (Week 3)

#### 3.1 Makefile Integration

**Tasks**:
- [ ] Update `make docs` target for MkDocs
- [ ] Create `make docs-serve` for local development
- [ ] Integrate rustdoc generation
- [ ] Integrate CRD API doc generation

**Updated Makefile Targets**:

```makefile
.PHONY: docs docs-serve docs-clean docs-install

docs-install: ## Install documentation dependencies
	@echo "Installing MkDocs and dependencies..."
	@pip install -r requirements-docs.txt

docs: export PATH := $(HOME)/.cargo/bin:$(PATH)
docs: docs-install ## Build all documentation (MkDocs + rustdoc + CRD API reference)
	@echo "Building all documentation..."
	@echo "Generating CRD API reference documentation..."
	@cargo run --bin crddoc > docs/reference/api.md
	@echo "Building rustdoc API documentation..."
	@cargo doc --no-deps --all-features
	@echo "Building MkDocs documentation..."
	@mkdocs build
	@echo "Copying rustdoc into documentation..."
	@mkdir -p site/rustdoc
	@cp -r target/doc/* site/rustdoc/
	@echo "✅ Documentation built successfully in site/"

docs-serve: export PATH := $(HOME)/.cargo/bin:$(PATH)
docs-serve: docs-install ## Serve documentation locally with live reload
	@echo "Serving documentation at http://127.0.0.1:8000"
	@mkdocs serve

docs-clean: ## Clean documentation build artifacts
	@echo "Cleaning documentation..."
	@rm -rf site/
	@echo "✅ Documentation cleaned"
```

#### 3.2 GitHub Actions Integration

**Tasks**:
- [ ] Update GitHub Pages workflow
- [ ] Set up Python environment in CI
- [ ] Configure MkDocs build in CI
- [ ] Set up deployment to GitHub Pages

**Updated GitHub Workflow** (`.github/workflows/docs.yml`):

```yaml
name: Documentation

on:
  push:
    branches: [main]
    paths:
      - 'docs/**'
      - 'src/**/*.rs'
      - 'mkdocs.yml'
      - 'requirements-docs.txt'
  pull_request:
    paths:
      - 'docs/**'
      - 'mkdocs.yml'

jobs:
  build-and-deploy:
    runs-on: ubuntu-latest
    permissions:
      contents: write

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0  # For git-revision-date-localized plugin

      - name: Set up Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.11'
          cache: 'pip'

      - name: Install Rust toolchain
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable

      - name: Cache cargo build
        uses: actions/cache@v3
        with:
          path: target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Install documentation dependencies
        run: pip install -r requirements-docs.txt

      - name: Generate CRD API docs
        run: cargo run --bin crddoc > docs/reference/api.md

      - name: Build rustdoc
        run: cargo doc --no-deps --all-features

      - name: Build MkDocs
        run: mkdocs build

      - name: Copy rustdoc
        run: |
          mkdir -p site/rustdoc
          cp -r target/doc/* site/rustdoc/

      - name: Deploy to GitHub Pages
        if: github.ref == 'refs/heads/main'
        uses: peaceiris/actions-gh-pages@v3
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: ./site
          cname: bindy.firestoned.io  # If using custom domain
```

#### 3.3 Local Development Setup

**Tasks**:
- [ ] Document local setup in CONTRIBUTING.md
- [ ] Create virtual environment setup script
- [ ] Add pre-commit hooks for documentation
- [ ] Update development workflow documentation

**Setup Script** (`scripts/setup-docs-env.sh`):

```bash
#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

set -e

echo "Setting up documentation development environment..."

# Create virtual environment
python3 -m venv .venv-docs

# Activate virtual environment
source .venv-docs/bin/activate

# Install dependencies
pip install --upgrade pip
pip install -r requirements-docs.txt

echo "✅ Documentation environment ready!"
echo "To activate: source .venv-docs/bin/activate"
echo "To serve docs: mkdocs serve"
```

### Phase 4: Testing & Validation (Week 4)

#### 4.1 Build Verification

**Tasks**:
- [ ] Verify all pages build without errors
- [ ] Check all internal links resolve correctly
- [ ] Validate Mermaid diagrams render properly
- [ ] Test search functionality
- [ ] Verify navigation hierarchy
- [ ] Test on multiple browsers (Chrome, Firefox, Safari)

**Validation Checklist**:
```bash
# Build documentation
mkdocs build --strict  # Fail on warnings

# Check for broken links (install linkchecker)
linkchecker site/

# Validate search index
# (Manual test: search for key terms)

# Check mobile responsiveness
# (Manual test: browser dev tools)
```

#### 4.2 Content Review

**Tasks**:
- [ ] Review all migrated pages for formatting issues
- [ ] Verify code blocks render with correct syntax highlighting
- [ ] Check admonitions display correctly
- [ ] Verify images and diagrams load properly
- [ ] Review responsive design on mobile

**Review Areas**:
- Code syntax highlighting (Rust, YAML, Bash, JSON)
- Mermaid diagram rendering
- Table formatting
- Admonition styling
- Navigation flow
- Search relevance

#### 4.3 Performance Testing

**Tasks**:
- [ ] Measure build time (mdBook vs MkDocs)
- [ ] Test search performance with full content
- [ ] Verify page load times
- [ ] Check minification effectiveness

**Performance Metrics**:
```bash
# Build time
time mkdocs build

# Site size
du -sh site/

# Lighthouse score (use Chrome DevTools)
# Target: 90+ on all metrics
```

### Phase 5: Deployment & Rollout (Week 4-5)

#### 5.1 Parallel Deployment

**Strategy**: Run both documentation systems in parallel for one release cycle.

**Tasks**:
- [ ] Deploy MkDocs to subdomain (e.g., `docs-beta.firestoned.io`)
- [ ] Add banner to mdBook docs linking to MkDocs beta
- [ ] Collect feedback from users
- [ ] Fix issues discovered in beta

**Beta Banner** (add to mdBook):
```html
<!-- In docs/theme/head.html -->
<div style="background: #2196F3; color: white; padding: 12px; text-align: center;">
  🚀 Try our new documentation at <a href="https://docs-beta.firestoned.io" style="color: white; text-decoration: underline;">docs-beta.firestoned.io</a>
</div>
```

#### 5.2 Final Cutover

**Tasks**:
- [ ] Announce migration timeline (1 week notice)
- [ ] Update all links in README.md to MkDocs
- [ ] Update CONTRIBUTING.md with new documentation workflow
- [ ] Deploy MkDocs to main domain
- [ ] Set up redirects from mdBook URLs to MkDocs URLs
- [ ] Archive mdBook files (don't delete, keep for reference)

**Redirect Configuration** (in `mkdocs.yml`):

```yaml
plugins:
  - redirects:
      redirect_maps:
        # Map old mdBook URLs to new MkDocs URLs if structure changed
        'old-path.html': 'new-path.md'
```

#### 5.3 Post-Migration Cleanup

**Tasks**:
- [ ] Remove mdBook dependencies from CI/CD
- [ ] Update Makefile to remove mdBook targets
- [ ] Remove `docs/book.toml`
- [ ] Remove `docs/theme/` directory (after verifying CSS/JS migrated)
- [ ] Update `.gitignore` to exclude `site/` instead of `docs/target/`
- [ ] Archive mdBook setup in a branch for reference

### Phase 6: Enhancement & Optimization (Ongoing)

#### 6.1 Advanced Features

**Tasks** (Post-migration enhancements):
- [ ] Set up versioned documentation (1.0, 2.0, etc.)
- [ ] Add social cards for better link sharing
- [ ] Implement multi-language support (if needed)
- [ ] Add tags/categories to pages
- [ ] Set up analytics (optional)

**Versioning Setup**:
```yaml
# mkdocs.yml
plugins:
  - mike:  # Version selector
      version_selector: true
      css_dir: css
      javascript_dir: js

extra:
  version:
    provider: mike
```

**Social Cards** (auto-generated preview images):
```yaml
plugins:
  - social:
      cards_layout_options:
        background_color: "#1976D2"
        font_family: Roboto
```

#### 6.2 Documentation Quality

**Tasks**:
- [ ] Set up automated link checking in CI
- [ ] Add documentation linting (markdownlint)
- [ ] Implement spellchecking
- [ ] Set up automated screenshot testing for diagrams

**CI Enhancements**:
```yaml
# .github/workflows/docs-quality.yml
- name: Lint documentation
  run: |
    npm install -g markdownlint-cli
    markdownlint 'docs/**/*.md'

- name: Check spelling
  uses: streetsidesoftware/cspell-action@v2
  with:
    files: 'docs/**/*.md'

- name: Check links
  uses: lycheeverse/lychee-action@v1
  with:
    args: --verbose --no-progress 'site/**/*.html'
```

## Migration Checklist

### Pre-Migration
- [ ] Team buy-in on MkDocs Material
- [ ] Python environment set up
- [ ] MkDocs Material installed and tested
- [ ] Initial configuration created

### Content Migration
- [ ] All pages migrated from `docs/src/`
- [ ] Navigation structure defined in `mkdocs.yml`
- [ ] Admonitions converted to Material syntax
- [ ] Code blocks verified
- [ ] Mermaid diagrams tested
- [ ] Internal links updated
- [ ] Images and assets migrated

### Build & CI/CD
- [ ] Makefile targets updated
- [ ] GitHub Actions workflow updated
- [ ] Local development documented
- [ ] Build process tested

### Testing
- [ ] All pages build without errors
- [ ] Search functionality tested
- [ ] Navigation verified
- [ ] Links checked (internal and external)
- [ ] Mobile responsiveness tested
- [ ] Browser compatibility verified

### Deployment
- [ ] Beta deployment completed
- [ ] Feedback collected and addressed
- [ ] Production deployment completed
- [ ] Redirects configured
- [ ] Old documentation archived

### Post-Migration
- [ ] Team documentation updated
- [ ] Contributors notified
- [ ] Monitoring set up
- [ ] Enhancement backlog created

## Risk Mitigation

### Risk: Build Time Increase
**Mitigation**:
- Use caching in CI/CD
- Enable minification only for production builds
- Optimize image sizes

### Risk: Search Quality Degradation
**Mitigation**:
- Test search extensively before cutover
- Configure search plugin with optimized settings
- Consider enabling offline search for large docs

### Risk: Broken Links After Migration
**Mitigation**:
- Set up comprehensive redirects
- Run link checker in CI/CD
- Keep old documentation accessible for 1 release cycle

### Risk: Team Adoption
**Mitigation**:
- Document migration thoroughly
- Provide training/walkthrough
- Create quick reference guide
- Maintain backward compatibility during transition

## Success Metrics

### Quantitative
- Build time: < 2 minutes (current mdBook baseline)
- Search latency: < 100ms for typical queries
- Page load time: < 1 second (P95)
- Lighthouse score: 90+ on all metrics
- Zero broken links in production

### Qualitative
- Improved navigation feedback from users
- Easier contribution process
- Better search result relevance
- Enhanced mobile experience

## Timeline Summary

| Phase | Duration | Key Deliverables |
|-------|----------|------------------|
| 1. Preparation | Week 1 | Environment setup, initial config |
| 2. Content Migration | Week 2-3 | All pages migrated, nav structured |
| 3. Integration | Week 3 | CI/CD updated, Makefile updated |
| 4. Testing | Week 4 | Validation complete, issues fixed |
| 5. Deployment | Week 4-5 | Beta launch, production cutover |
| 6. Enhancement | Ongoing | Advanced features, optimizations |

**Total Estimated Time**: 4-5 weeks for full migration

## Resources Required

### Tools & Dependencies
- Python 3.11+
- MkDocs Material (latest)
- Mermaid plugin
- Git revision date plugin
- Minify plugin
- Redirects plugin

### Team Effort
- Developer time: ~40-60 hours
- Review time: ~10 hours
- Testing time: ~10 hours

### Documentation
- Migration guide for contributors
- Updated development setup docs
- New documentation workflow guide

## Next Steps

1. **Review and approve this roadmap** with the team
2. **Set up pilot environment** to test MkDocs Material
3. **Migrate a single section** (e.g., Getting Started) as proof of concept
4. **Gather feedback** and adjust roadmap
5. **Execute full migration** following this plan

## References

- [MkDocs Material Documentation](https://squidfunk.github.io/mkdocs-material/)
- [MkDocs Official Docs](https://www.mkdocs.org/)
- [Markdown Extensions Reference](https://facelessuser.github.io/pymdown-extensions/)
- [Material Theme Configuration](https://squidfunk.github.io/mkdocs-material/setup/)
- [Mermaid Diagrams in MkDocs](https://github.com/fralau/mkdocs-mermaid2-plugin)

---

**Questions or Concerns?**

Open an issue on GitHub or discuss in team channels before proceeding with migration.
