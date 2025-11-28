# CRD API Reference

This directory contains the auto-generated API reference documentation for Bindy's Custom Resource Definitions (CRDs).

## Generating Documentation

The API reference is automatically generated from the Rust type definitions in `src/crd.rs` using the `crddoc` binary:

```bash
# Generate the API reference
cargo run --bin crddoc > docs/src/reference/api.md

# Or use the make target (includes all docs)
make docs
```

## Files

- `api.md` - Auto-generated CRD API reference (DO NOT EDIT MANUALLY)

The documentation is automatically updated as part of the `docs` make target.
