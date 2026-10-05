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
├── Cargo.toml        # Cargo workspace (ADR-0009): every dependency pinned once
├── crates/
│   ├── bindy-api/                 # CRD types, constants, labels; crdgen/crddoc bins
│   ├── bindy-controller-sdk/      # Shared framework: Context, WatchSet, shutdown, leader
│   │                              # election, finalizers, errors, retry, status, metrics
│   ├── bindy-bind9/               # BIND9 domain logic: bindcar client, RNDC, instance
│   │                              # resources, record writes (no controllers)
│   ├── bindy-controller-cluster/  # Bind9Cluster + ClusterBind9Provider controllers
│   ├── bindy-controller-instance/ # Bind9Instance controller
│   ├── bindy-controller-zone/     # DNSZone controller
│   ├── bindy-controller-records/  # The nine record controllers (one generic controller)
│   ├── bindy-scout/               # `bindy scout`
│   ├── bindy-bootstrap/           # `bindy bootstrap`
│   └── bindy/                     # The binary: CLI, logging, client, metrics server,
│       ├── src/                   # leader-election handoff, running each controller
│       │   ├── main.rs
│       │   └── cli.rs
│       └── tests/                 # Rust integration tests against a live API server
├── deploy/           # Kubernetes manifests
│   ├── crds/         # CRD definitions
│   ├── rbac/         # RBAC resources
│   └── operator/     # Operator deployment
├── tests/            # Shell integration and e2e suites
├── examples/         # Example configurations
└── docs/             # Documentation
```

Dependencies only point downward: `bindy` depends on the controller crates,
Scout and bootstrap; a controller crate depends on `bindy-controller-sdk`,
`bindy-bind9` and `bindy-api`, never on another controller crate; `bindy-bind9`
depends on the SDK and `bindy-api`; `bindy-api` depends on nothing in the
workspace. Each controller crate's only public item is
`pub async fn controller(ctx: Arc<Context>) -> anyhow::Result<()>`.

## Dependencies

Key dependencies:
- `kube` - Kubernetes client
- `tokio` - Async runtime
- `serde` - Serialization
- `tracing` - Logging

See [Cargo.toml](../../../Cargo.toml) for full list.

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

- [Developer Guide Overview](./index.md) - ADD methodology (`ADR → CALM → TDD`)
- [Building from Source](./building.md) - Build the operator
- [Testing Guide](./testing-guide.md) - Test your changes
- [Development Workflow](./workflow.md) - Daily development workflow
