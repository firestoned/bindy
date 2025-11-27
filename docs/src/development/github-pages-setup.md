# GitHub Pages Setup Guide

This guide explains how to enable GitHub Pages for the Bindy documentation.

## Prerequisites

- Repository must be pushed to GitHub
- You must have admin access to the repository
- The `.github/workflows/docs.yaml` workflow file must be present

## Setup Steps

### 1. Enable GitHub Pages

1. Go to your repository on GitHub: https://github.com/firestoned/bindy
2. Click **Settings** (in the repository menu)
3. Scroll down to the **Pages** section in the left sidebar
4. Click on **Pages**

### 2. Configure Source

Under "Build and deployment":

1. **Source**: Select "GitHub Actions"
2. This will use the workflow in `.github/workflows/docs.yaml`

That's it! GitHub will automatically use the workflow.

### 3. Trigger the First Build

The documentation will be built and deployed automatically when you push to the `main` branch.

To trigger the first build:

1. Push any change to `main`:
   ```bash
   git push origin main
   ```

2. Or manually trigger the workflow:
   - Go to **Actions** tab
   - Click on "Documentation" workflow
   - Click "Run workflow"
   - Select `main` branch
   - Click "Run workflow"

### 4. Monitor the Build

1. Go to the **Actions** tab in your repository
2. Click on the "Documentation" workflow run
3. Watch the build progress
4. Once complete, the "deploy" job will show the URL

### 5. Access Your Documentation

Once deployed, your documentation will be available at:

**https://firestoned.github.io/bindy/**

## Verification

### Check Deployment Status

1. Go to **Settings** → **Pages**
2. You should see: "Your site is live at https://firestoned.github.io/bindy/"
3. Click "Visit site" to view the documentation

### Verify Documentation Structure

Your deployed site should have:

- Main documentation (mdBook): https://firestoned.github.io/bindy/
- API reference (rustdoc): https://firestoned.github.io/bindy/rustdoc/

## Troubleshooting

### Build Fails

**Check workflow logs:**

1. Go to **Actions** tab
2. Click on the failed workflow run
3. Expand the failed step to see the error
4. Common issues:
   - Rust compilation errors
   - mdBook build errors
   - Missing files

**Fix and retry:**

1. Fix the issue locally
2. Test with `make docs`
3. Push the fix to `main`
4. GitHub Actions will automatically retry

### Pages Not Showing

**Verify GitHub Pages is enabled:**

1. Go to **Settings** → **Pages**
2. Ensure source is set to "GitHub Actions"
3. Check that at least one successful deployment has completed

**Check permissions:**

The workflow needs these permissions (already configured in `docs.yaml`):

```yaml
permissions:
  contents: read
  pages: write
  id-token: write
```

### 404 Errors on Subpages

**Check base URL configuration:**

The `book.toml` has:

```toml
site-url = "/bindy/"
```

This must match your repository name. If your repository is named differently, update this value.

### Custom Domain (Optional)

To use a custom domain:

1. Go to **Settings** → **Pages**
2. Under "Custom domain", enter your domain
3. Update the `CNAME` field in `book.toml`:
   ```toml
   cname = "docs.yourdomain.com"
   ```
4. Configure DNS:
   - Add a CNAME record pointing to `firestoned.github.io`
   - Or A records pointing to GitHub Pages IPs

## Updating Documentation

Documentation is automatically deployed on every push to `main`:

```bash
# Make changes to documentation
vim docs/src/introduction.md

# Commit and push
git add docs/src/introduction.md
git commit -m "Update introduction"
git push origin main

# GitHub Actions will automatically build and deploy
```

## Local Preview

Before pushing, preview your changes locally:

```bash
# Build and serve documentation
make docs-serve

# Or watch for changes
make docs-watch

# Open http://localhost:3000 in your browser
```

## Workflow Details

The GitHub Actions workflow (`.github/workflows/docs.yaml`):

1. **Build** job:
   - Checks out the repository
   - Sets up Rust toolchain
   - Installs mdBook
   - Builds rustdoc API documentation
   - Builds mdBook user documentation
   - Combines both into a single site
   - Uploads artifact to GitHub Pages

2. **Deploy** job (only on `main`):
   - Deploys the artifact to GitHub Pages
   - Updates the live site

## Branch Protection (Recommended)

To ensure documentation quality:

1. Go to **Settings** → **Branches**
2. Add a branch protection rule for `main`:
   - Require pull request reviews
   - Require status checks (include "Documentation / Build Documentation")
   - This ensures the documentation builds before merging

## Additional Configuration

### Custom Theme

The documentation uses a custom theme defined in:

- `docs/theme/custom.css` - Custom styling

To customize:

1. Edit the CSS file
2. Test locally with `make docs-watch`
3. Push to `main`

### Search Configuration

Search is configured in `book.toml`:

```toml
[output.html.search]
enable = true
limit-results = 30
```

Adjust as needed for your use case.

## Support

For issues with GitHub Pages deployment:

- **GitHub Pages Status**: https://www.githubstatus.com/
- **GitHub Actions Documentation**: https://docs.github.com/en/actions
- **GitHub Pages Documentation**: https://docs.github.com/en/pages

For issues with the documentation content:

- Create an issue: https://github.com/firestoned/bindy/issues
- Start a discussion: https://github.com/firestoned/bindy/discussions
