# Mermaid.js Configuration Comprehensive Analysis

**Date:** 2026-01-24
**Status:** Analysis Complete - Recommendations Provided
**Impact:** Documentation rendering quality and maintenance

## Executive Summary

Current setup uses an **outdated approach** with the `mkdocs-mermaid2-plugin` (v1.2.3) and Mermaid.js v10.9.0. Material for MkDocs now provides **native Mermaid support** that is simpler, better integrated, and more maintainable.

### Key Findings

- ✅ **Plugin Version:** mkdocs-mermaid2-plugin 1.2.3 (latest available - October 2025)
- ⚠️ **Mermaid.js Version:** 10.9.0 (current) vs 11.12.2 (latest stable - December 2024)
- ❌ **Configuration:** Using deprecated `fence_div_format` instead of native `fence_code_format`
- ❌ **Approach:** Using plugin when Material for MkDocs has native support

## Current Configuration

### Dependencies (docs/pyproject.toml)
```toml
mkdocs-mermaid2-plugin = "^1.1.1"  # Installed: 1.2.3
```

### MkDocs Configuration (docs/mkdocs.yml)
```yaml
plugins:
  - mermaid2  # DEPRECATED APPROACH

markdown_extensions:
  - pymdownx.superfences:
      custom_fences:
        - name: mermaid
          class: mermaid
          format: !!python/name:pymdownx.superfences.fence_div_format  # WRONG FORMAT

extra_javascript:
  - https://unpkg.com/mermaid@10.9.0/dist/mermaid.min.js  # OUTDATED VERSION
  - javascripts/mermaid-init.js  # CUSTOM INITIALIZATION
```

### Issues with Current Approach

1. **Using Plugin When Native Support Exists:**
   - Material for MkDocs has native Mermaid support (no plugin needed)
   - Plugin adds complexity and potential conflicts
   - Plugin warning: "Using extra_javascript is now DEPRECATED; use mermaid:javascript instead!"

2. **Wrong Superfences Format:**
   - Current: `fence_div_format` (wraps in `<div class="mermaid">`)
   - Recommended: `fence_code_format` (wraps in `<code class="mermaid">`)
   - Native support expects `fence_code_format`

3. **Outdated Mermaid.js Version:**
   - Current: v10.9.0 (November 2024)
   - Latest: v11.12.2 (December 2024)
   - Missing 14+ months of bug fixes and features

4. **Complex Custom Initialization:**
   - 326 lines of custom JavaScript (mermaid-init.js)
   - Handles navigation events, zoom/pan, scroll restoration
   - Required due to conflicts between plugin and Material's instant loading

## Recommended Configuration (Native Support)

### Option 1: Material for MkDocs Native Support (RECOMMENDED)

**Benefits:**
- ✅ No plugin dependency
- ✅ Automatic theme integration (fonts, colors)
- ✅ Works seamlessly with instant loading
- ✅ Simpler configuration
- ✅ Better long-term support

**Configuration:**

```yaml
# docs/mkdocs.yml
plugins:
  - search
  # REMOVE: - mermaid2

markdown_extensions:
  - pymdownx.superfences:
      custom_fences:
        - name: mermaid
          class: mermaid
          format: !!python/name:pymdownx.superfences.fence_code_format  # CHANGED

# REMOVE extra_javascript for mermaid (Material handles it)
extra_javascript:
  # - https://unpkg.com/mermaid@10.9.0/dist/mermaid.min.js  # REMOVE
  # - javascripts/mermaid-init.js  # REMOVE (or simplify for zoom/pan only)
```

**Custom Initialization (if needed for zoom/pan):**

```javascript
// docs/src/javascripts/mermaid-config.js (simplified)
window.mermaidConfig = {
  startOnLoad: true,
  theme: 'default',
  securityLevel: 'loose'
};
```

### Option 2: Keep Plugin but Update (NOT RECOMMENDED)

If you must keep the plugin (e.g., for specific features):

```yaml
plugins:
  - mermaid2:
      javascript: https://unpkg.com/mermaid@11.12.2/dist/mermaid.min.js  # UPDATED
      # OR specify version:
      version: 11.12.2

markdown_extensions:
  - pymdownx.superfences:
      custom_fences:
        - name: mermaid
          class: mermaid
          format: !!python/name:pymdownx.superfences.fence_div_format  # Keep for plugin

# DO NOT use extra_javascript - plugin handles it
```

## Migration Path

### Phase 1: Test Native Support (Low Risk)

1. Create a test branch
2. Update configuration to native support
3. Simplify or remove custom mermaid-init.js
4. Test all pages with diagrams
5. Verify instant loading, zoom/pan, and navigation work

### Phase 2: Implement Zoom/Pan (If Needed)

If zoom/pan functionality is critical:

```javascript
// docs/src/javascripts/mermaid-zoom.js (new, simplified)
document.addEventListener('DOMContentLoaded', function() {
  // Wait for Material to render diagrams
  setTimeout(() => {
    const svgs = document.querySelectorAll('.mermaid svg');
    svgs.forEach(svg => {
      // Add zoom/pan functionality (extract from current mermaid-init.js)
      addZoomPan(svg);
    });
  }, 500);
});
```

### Phase 3: Update Dependencies

```bash
# Remove plugin
cd docs
poetry remove mkdocs-mermaid2-plugin

# Verify Material version supports native Mermaid
poetry show mkdocs-material
# Should be 9.5.0+ (current: 9.5.x)
```

## Comparison: Plugin vs Native

| Feature | Plugin (Current) | Native (Recommended) |
|---------|------------------|----------------------|
| Configuration Complexity | High (plugin + custom JS) | Low (markdown extension only) |
| Mermaid Version Control | Manual (extra_javascript) | Automatic (Material manages) |
| Theme Integration | Manual | Automatic (fonts, colors) |
| Instant Loading Support | Requires custom JS workarounds | Built-in, seamless |
| Maintenance Burden | High (custom 326-line JS) | Low (Material handles it) |
| Zoom/Pan Support | Custom implementation | Requires custom JS (same effort) |
| Future Compatibility | Plugin may become unmaintained | Material actively maintained |

## Version Information

### Current Versions
- **mkdocs-mermaid2-plugin:** 1.2.3 (latest available)
- **Mermaid.js:** 10.9.0 (in use) vs 11.12.2 (latest)
- **Material for MkDocs:** 9.5.x (supports native Mermaid)

### Latest Versions (as of 2026-01-24)
- **mkdocs-mermaid2-plugin:** 1.2.3 (October 17, 2025)
- **Mermaid.js:** 11.12.2 (December 2, 2024)
- **Material for MkDocs:** 9.5.x series (actively maintained)

## Root Cause of Current Issues

The diagram disappearance bug occurred because:

1. **Plugin uses `fence_div_format`** → Creates `<div class="mermaid">graph code</div>`
2. **Material's instant loading replaces DOM** → Destroys rendered SVGs
3. **Custom JS tries to re-render** → Conflicts with plugin's initialization
4. **Result:** Diagrams show raw graph code instead of SVG

With native support:
- Material handles diagram lifecycle correctly
- Instant loading aware of diagram rendering
- No conflicts between plugin and theme

## Recommendations

### Immediate (Next Sprint)

1. ✅ **Keep current setup working** - Your custom mermaid-init.js fixes are good
2. ⚠️ **Upgrade Mermaid.js** to 11.12.2 (low risk, bug fixes only)
   ```yaml
   extra_javascript:
     - https://unpkg.com/mermaid@11.12.2/dist/mermaid.min.js
   ```

### Short-term (Next Month)

3. 🔄 **Test native support** in a branch
4. 🔄 **Evaluate zoom/pan importance** - Is it heavily used?
5. 🔄 **Plan migration** if native support works well

### Long-term (Next Quarter)

6. 🎯 **Migrate to native support** for better maintainability
7. 🎯 **Simplify custom JS** to only zoom/pan if needed
8. 🎯 **Remove plugin dependency** to reduce complexity

## References

- [Material for MkDocs - Diagrams](https://squidfunk.github.io/mkdocs-material/reference/diagrams/)
- [mkdocs-mermaid2-plugin GitHub](https://github.com/fralau/mkdocs-mermaid2-plugin)
- [mkdocs-mermaid2-plugin PyPI](https://pypi.org/project/mkdocs-mermaid2-plugin/)
- [Mermaid.js Releases](https://github.com/mermaid-js/mermaid/releases)
- [MkDocs-Mermaid2 Documentation](https://mkdocs-mermaid2.readthedocs.io/)

## Conclusion

**Current setup is functional but complex.** The custom JavaScript fixes the diagram disappearance issue, but the root cause is the conflict between the plugin and Material's instant loading.

**Recommended approach:** Migrate to Material's native Mermaid support for better long-term maintainability, simpler configuration, and automatic theme integration. If zoom/pan is critical, implement it as a lightweight addon on top of native support.

**Risk Assessment:**
- Low risk: Upgrading Mermaid.js to 11.12.2
- Medium risk: Migrating to native support (requires testing)
- High reward: Simplified configuration, better integration, less maintenance

## Next Steps

1. Review this analysis with team
2. Decide on migration timeline
3. Test native support in development environment
4. Evaluate zoom/pan usage analytics (if available)
5. Create migration plan based on findings
