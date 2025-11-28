# Changelog

All notable changes to this project will be documented in this file.

## [2025-11-28 23:10] - Add Custom Right-Side Page Table of Contents

### Added
- `docs/theme/page-toc.js`: Custom JavaScript for right-side in-page navigation
- `docs/theme/custom.css`: CSS styles for right-side page TOC with smooth scrolling and active highlighting
- `docs/theme/custom.css`: Hidden arrow navigation (previous/next chapter buttons) for cleaner page layout

### Changed
- `docs/book.toml`: Added `theme/page-toc.js` to `additional-js`

### Why
Created a custom right-side table of contents solution to provide in-page navigation without relying on third-party plugins. The implementation:
- Automatically generates TOC from H2, H3, and H4 headings
- Displays on the right side of pages (visible on screens >1280px wide)
- Highlights the current section as you scroll
- Provides smooth scrolling to sections
- Matches the rustdoc-inspired theme styling
- Removes distracting arrow navigation buttons (users can navigate via sidebar instead)

This solution is more reliable than mdbook-pagetoc (which is incompatible with mdBook 0.5) and provides better control over styling and behavior.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

**Result:** Documentation now has a persistent right-side navigation panel showing all headings on the current page, improving navigation for long pages.

## [2025-11-28 23:00] - Remove mdbook-toc Preprocessor

### Changed
- `docs/book.toml`: Removed `[preprocessor.toc]` configuration
- `.github/workflows/docs.yaml`: Removed `mdbook-toc` installation
- `.github/workflows/pr.yaml`: Removed `mdbook-toc` installation

### Why
The mdbook-toc preprocessor is no longer needed. It was used to insert table of contents markers (`<!-- toc -->`) in markdown files, but this functionality is not currently being used in the documentation.

mdBook 0.5 provides built-in navigation features that cover the documentation's needs:
- **Sidebar navigation**: Full book structure with all pages and sections
- **Sidebar heading navigation**: In-page navigation showing all headings within the current chapter

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

**Result:** Simplified documentation build with fewer dependencies, relying on mdBook's native navigation features.

## [2025-11-28 18:45] - Remove mdbook-pagetoc in Favor of Built-in mdBook 0.5 Feature

### Changed
- `docs/book.toml`: Removed `[preprocessor.pagetoc]` configuration
- `docs/book.toml`: Removed pagetoc CSS and JS from `additional-css` and `additional-js`
- `.github/workflows/docs.yaml`: Removed `mdbook-pagetoc` installation
- `.github/workflows/pr.yaml`: Removed `mdbook-pagetoc` installation
- Deleted generated `docs/theme/pagetoc.css` and `docs/theme/pagetoc.js` files

### Why
mdbook-pagetoc is incompatible with mdBook 0.5.x due to HTML structure changes. It caused JavaScript errors: `TypeError: can't access property "childNodes", main is null`.

mdBook 0.5.0 introduced **built-in sidebar heading navigation** which provides the same functionality natively without requiring a plugin. This feature is enabled by default and provides proper in-page navigation for all headings within a chapter.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

**Result:** Users now get in-page navigation through mdBook's native sidebar heading navigation feature, which is properly maintained and compatible with the current mdBook version.

## [2025-11-28 17:30] - Fix mdBook Edit Button Path

### Changed
- `docs/book.toml`: Fixed `edit-url-template` to point to `docs/src/{path}` instead of `{path}`

### Why
The edit button (pencil icon) in the mdBook HTML output was pointing to the wrong path in the GitHub repository. It was generating URLs like `https://github.com/firestoned/bindy/edit/main/introduction.md` instead of `https://github.com/firestoned/bindy/edit/main/docs/src/introduction.md`, resulting in 404 errors when users tried to edit documentation pages.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

## [2025-11-28 17:15] - Upgrade mdBook and Preprocessors to Latest Versions

### Changed
- `.github/workflows/docs.yaml`: Updated mdBook from v0.4.40 to v0.5.1
- `.github/workflows/docs.yaml`: Updated mdbook-mermaid from v0.14.0 to v0.17.0
- `.github/workflows/docs.yaml`: Updated mdbook-toc from v0.14.2 to v0.15.1
- `.github/workflows/pr.yaml`: Updated mdBook from v0.4.40 to v0.5.1
- `.github/workflows/pr.yaml`: Updated mdbook-mermaid from v0.14.0 to v0.17.0
- `.github/workflows/pr.yaml`: Updated mdbook-toc from v0.14.2 to v0.15.1
- `.github/workflows/pr.yaml`: Re-added GitHub Pages deployment for debugging mdbook-toc issues

### Why
Upgrading to the latest versions ensures we have the newest features, bug fixes, and security patches for our documentation toolchain. mdBook 0.5.1 includes performance improvements and new features. mdbook-mermaid 0.17.0 upgrades to Mermaid.js v11.2.0 with improved diagram rendering. mdbook-toc 0.15.1 aligns with the latest mdBook APIs.

The PR workflow temporarily includes Pages deployment to help debug mdbook-toc integration issues.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI/CD workflow)
- [ ] Documentation only

**Note:** Local development requires Rust 1.88.0+ for these versions. CI/CD uses latest stable Rust automatically. Local developers with older Rust versions can use mdBook 0.4.52, mdbook-mermaid 0.14.0, and mdbook-toc 0.14.2 (compatible with Rust 1.82+).

## [2025-11-28 14:30] - Optimize PR Workflow Test Job

### Changed
- `.github/workflows/pr.yaml`: Added artifact upload/download to reuse build artifacts from build job in test job

### Why
The test job was rebuilding the entire project despite running after the build job, wasting CI time and resources. By uploading build artifacts (target directory) from the build job and downloading them in the test job, we eliminate redundant compilation.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI/CD workflow)
- [ ] Documentation only

**Expected Benefit:** Significantly reduced PR CI time by avoiding duplicate builds in the test job.
