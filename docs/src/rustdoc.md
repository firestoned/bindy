# API Documentation (rustdoc)

The complete API documentation is generated from Rust source code and is available separately.

## Viewing API Documentation

### Online
The API Reference is available in the `rustdoc/bindy/index.html` section of the documentation site once built.

### Locally

Build and view the API documentation:

```bash
# Build API docs
cargo doc --no-deps --all-features

# Open in browser
cargo doc --no-deps --all-features --open
```

Or build the complete documentation (user guide + API):

```bash
make docs-serve
# Navigate to http://localhost:3000/rustdoc/bindy/index.html
```

## What's in the API Documentation

The rustdoc API documentation includes:

- **Module Documentation** - All public modules and their organization
- **Struct Definitions** - Complete CRD type definitions (Bind9Instance, DNSZone, etc.)
- **Function Signatures** - All public functions with parameter types and return values
- **Examples** - Code examples showing how to use the API
- **Type Documentation** - Detailed information about all public types
- **Trait Implementations** - All trait implementations for types

## Key Modules

- `bindy::crd` - Custom Resource Definitions
- `bindy::reconcilers` - Operator reconciliation logic
- `bindy::bind9` - BIND9 zone file management
- `bindy::bind9_resources` - Kubernetes resource builders

## Direct Links

When the documentation is built, you can access:

- **Main API Index**: `rustdoc/bindy/index.html`
- **CRD Module**: `rustdoc/bindy/crd/index.html`
- **Reconcilers**: `rustdoc/bindy/reconcilers/index.html`
