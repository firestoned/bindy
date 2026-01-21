# Building from Source

Build the Bindy operator from source code.

## Build Debug Version

For development with debug symbols:

```bash
cargo build
```

Binary location: `target/debug/bindy`

## Build Release Version

Optimized for production:

```bash
cargo build --release
```

Binary location: `target/release/bindy`

## Run Locally

```bash
# Set log level
export RUST_LOG=info

# Run operator (requires kubeconfig)
cargo run --release
```

## Build Docker Image

```bash
# Build image
docker build -t bindy:dev .

# Or use make
make docker-build TAG=dev
```

## Build for Different Platforms

### Cross-Compilation

```bash
# Install cross
cargo install cross

# Build for Linux (from macOS/Windows)
cross build --release --target x86_64-unknown-linux-gnu
```

### Multi-Architecture Images

```bash
# Build for multiple architectures
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t bindy:multi \
  --push .
```

## Build Documentation

### Rustdoc (API docs)

```bash
cargo doc --no-deps --open
```

### mdBook (User guide)

**Prerequisites:**

The documentation uses Mermaid diagrams which require the `mdbook-mermaid` preprocessor:

```bash
# Install mdbook-mermaid
cargo install mdbook-mermaid

# Ensure ~/.cargo/bin is in your PATH
export PATH="$HOME/.cargo/bin:$PATH"

# Initialize Mermaid support (first time only)
mdbook-mermaid install .
```

**Build and serve:**

```bash
# Build book
mdbook build

# Serve locally
mdbook serve --open
```

### Combined Documentation

```bash
make docs
```

## Optimization

### Profile-Guided Optimization

```bash
# Generate profile data
cargo build --release
./target/release/bindy  # Run workload

# Build with PGO
cargo build --release
```

### Size Optimization

```toml
# In Cargo.toml
[profile.release]
opt-level = 'z'     # Optimize for size
lto = true          # Link-time optimization
codegen-units = 1   # Better optimization
strip = true        # Strip symbols
```

## Troubleshooting

### Build Errors

**OpenSSL not found:**
```bash
# Ubuntu/Debian
apt-get install libssl-dev pkg-config

# macOS
brew install openssl
```

**Linker errors:**
```bash
# Install build essentials
apt-get install build-essential
```

## Next Steps

- [Running Tests](./testing.md) - Test your build
- [Development Workflow](./workflow.md) - Daily development
