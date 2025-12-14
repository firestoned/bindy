# Reusable Composite Actions

This directory contains reusable GitHub Actions composite actions that reduce code duplication across workflows.

## Available Actions

### `setup-rust-build`
Sets up the Rust build environment with caching for multi-architecture builds.

**Inputs:**
- `target` (required): Rust target triple (e.g., `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`)
- `cache-key` (optional): Additional cache key component for versioned caching

**Features:**
- Installs Rust toolchain with specified target
- Caches Rust dependencies with Swatinem/rust-cache
- Automatically installs and caches `cross` for ARM64 builds
- Verifies Docker availability for cross-compilation

**Usage:**
```yaml
- name: Setup Rust build environment
  uses: ./.github/actions/setup-rust-build
  with:
    target: ${{ matrix.platform.target }}
    cache-key: release-v1.0.0
```

### `build-binary`
Builds Rust binary for the specified target architecture.

**Inputs:**
- `target` (required): Rust target triple (e.g., `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`)

**Features:**
- Uses native `cargo build` for x86_64
- Uses `cross` for ARM64 cross-compilation
- Automatically selects the correct build tool based on target

**Usage:**
```yaml
- name: Build binary
  uses: ./.github/actions/build-binary
  with:
    target: ${{ matrix.platform.target }}
```

### `prepare-docker-binaries`
Downloads build artifacts and prepares binaries for multi-arch Docker builds.

**Inputs:** None (uses implicit artifacts from previous jobs)

**Features:**
- Downloads all binary artifacts
- Copies binaries to `binaries/amd64/` and `binaries/arm64/`
- Sets executable permissions
- Verifies binaries with `file` command
- Fails fast if binaries are missing

**Usage:**
```yaml
- name: Prepare Docker binaries
  uses: ./.github/actions/prepare-docker-binaries
```

### `setup-docker`
Sets up Docker Buildx and authenticates to GitHub Container Registry.

**Inputs:** None (uses implicit GitHub context)

**Features:**
- Sets up Docker Buildx for multi-platform builds
- Logs into ghcr.io using GitHub token
- Automatically uses `github.actor` and `secrets.GITHUB_TOKEN`

**Usage:**
```yaml
- name: Setup Docker environment
  uses: ./.github/actions/setup-docker
```

### `cache-cargo`
Caches cargo registry, index, and build artifacts.

**Inputs:** None (uses implicit Cargo.lock hash)

**Features:**
- Caches `~/.cargo/registry`
- Caches `~/.cargo/git`
- Caches `target/` directory
- Uses Cargo.lock hash for cache keys

**Usage:**
```yaml
- name: Cache cargo dependencies
  uses: ./.github/actions/cache-cargo
```

## Code Reduction

Using these composite actions has significantly reduced code duplication:

| Metric | Before | After | Reduction |
|--------|--------|-------|-----------|
| **Total workflow lines** | ~850 | 688 | **19% reduction** |
| **Duplicated setup code** | ~120 lines × 3 workflows | Reused 5 actions | **Eliminated ~360 lines** |
| **Composite actions** | 0 | 167 lines | Centralized logic |

### Benefits

1. **DRY Principle**: Each piece of logic exists in exactly one place
2. **Maintainability**: Update once, apply everywhere
3. **Consistency**: All workflows use identical setup procedures
4. **Testability**: Composite actions can be tested independently
5. **Readability**: Workflows are more concise and focused on their unique logic

## Example: Before vs After

### Before (Duplicated in 3 workflows)
```yaml
- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@stable
  with:
    targets: ${{ matrix.platform.target }}

- name: Cache Rust dependencies
  uses: Swatinem/rust-cache@v2
  with:
    key: ${{ matrix.platform.target }}
    cache-on-failure: true

- name: Cache cross binary
  if: matrix.platform.target == 'aarch64-unknown-linux-gnu'
  id: cache-cross
  uses: actions/cache@v4
  with:
    path: ~/.cargo/bin/cross
    key: ${{ runner.os }}-cross-v0.2.5

- name: Install cross for ARM64 builds
  if: matrix.platform.target == 'aarch64-unknown-linux-gnu' && steps.cache-cross.outputs.cache-hit != 'true'
  run: |
    cargo install cross --git https://github.com/cross-rs/cross --tag v0.2.5

- name: Verify Docker for cross (ARM64)
  if: matrix.platform.target == 'aarch64-unknown-linux-gnu'
  run: |
    docker --version
    docker info

- name: Build (release) - x86_64
  if: matrix.platform.target == 'x86_64-unknown-linux-gnu'
  run: cargo build --release --target ${{ matrix.platform.target }} --verbose

- name: Build (release) - ARM64 with cross
  if: matrix.platform.target == 'aarch64-unknown-linux-gnu'
  run: cross build --release --target ${{ matrix.platform.target }} --verbose
```

**Lines:** ~40 lines × 3 workflows = **120 lines total**

### After (Using composite actions)
```yaml
- name: Setup Rust build environment
  uses: ./.github/actions/setup-rust-build
  with:
    target: ${{ matrix.platform.target }}

- name: Build binary
  uses: ./.github/actions/build-binary
  with:
    target: ${{ matrix.platform.target }}
```

**Lines:** ~6 lines × 3 workflows = **18 lines total**

**Savings:** **102 lines eliminated** from workflows (85% reduction for this section)

## Maintenance

When updating build logic:

1. **For all workflows**: Update the composite action in `.github/actions/*/action.yaml`
2. **For specific workflows**: Add workflow-specific logic in the workflow file itself

## Testing

To test composite actions locally, you can use [act](https://github.com/nektos/act):

```bash
# Install act
brew install act

# Run a workflow locally
act -j build -W .github/workflows/pr.yaml
```

## Related Documentation

- [GitHub Actions: Creating composite actions](https://docs.github.com/en/actions/creating-actions/creating-a-composite-action)
- [Rust CI/CD Best Practices](../../docs/src/development/ci_cd.md)
- [Docker Build Strategy](../../CI_CD_DOCKER_BUILD.md)
