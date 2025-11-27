# Development Setup

Set up your development environment for contributing to Bindy.

## Prerequisites

### Required Tools

- **Rust** - 1.70 or later
- **Kubernetes** - 1.27 or later (for testing)
- **kubectl** - Matching your Kubernetes version
- **Docker** - For building images
- **kind** - For local Kubernetes testing (optional)

### Install Rust

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version
cargo --version
```

### Install Development Tools

```bash
# Install cargo tools
cargo install cargo-watch  # Auto-rebuild on changes
cargo install cargo-tarpaulin  # Code coverage

# Install mdbook for documentation
cargo install mdbook
```

## Clone Repository

```bash
git clone https://github.com/firestoned/bindy.git
cd bindy
```

## Project Structure

```
bindy/
├── src/              # Rust source code
│   ├── main.rs       # Entry point
│   ├── crd.rs        # CRD definitions
│   ├── reconcilers/  # Reconciliation logic
│   └── bind9.rs      # BIND9 integration
├── deploy/           # Kubernetes manifests
│   ├── crds/         # CRD definitions
│   ├── rbac/         # RBAC resources
│   └── controller/   # Controller deployment
├── tests/            # Integration tests
├── examples/         # Example configurations
├── docs/             # Documentation
└── Cargo.toml        # Rust dependencies
```

## Dependencies

Key dependencies:
- `kube` - Kubernetes client
- `tokio` - Async runtime
- `serde` - Serialization
- `tracing` - Logging

See [Cargo.toml](../../Cargo.toml) for full list.

## IDE Setup

### VS Code

Recommended extensions:
- rust-analyzer
- crates
- Even Better TOML
- Kubernetes

### IntelliJ IDEA / CLion

- Install Rust plugin
- Install Kubernetes plugin

## Verify Setup

```bash
# Build the project
cargo build

# Run tests
cargo test

# Run clippy (linter)
cargo clippy

# Format code
cargo fmt
```

If all commands succeed, your development environment is ready!

## Next Steps

- [Building from Source](./building.md) - Build the controller
- [Running Tests](./testing.md) - Test your changes
- [Development Workflow](./workflow.md) - Daily development workflow
