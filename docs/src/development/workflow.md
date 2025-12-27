# Development Workflow

Daily development workflow for Bindy contributors.

## Development Cycle

1. **Create feature branch**
```bash
git checkout -b feature/my-feature
```

2. **Make changes**
- Edit code in `src/`
- If modifying CRDs, edit Rust types in `src/crd.rs`
- Add tests
- Update documentation

3. **Regenerate CRDs (if modified)**
```bash
# If you modified src/crd.rs, regenerate YAML files
cargo run --bin crdgen
# or
make crds
```

4. **Test locally**
```bash
cargo test
cargo clippy -- -D warnings
cargo fmt
```

5. **Validate CRDs**
```bash
# Ensure generated CRDs are valid
kubectl apply --dry-run=client -f deploy/crds/
```

6. **Commit changes**
```bash
git add .
git commit -m "Add feature: description"
```

7. **Push and create PR**
```bash
git push origin feature/my-feature
# Create PR on GitHub
```

## CRD Development

**IMPORTANT:** `src/crd.rs` is the source of truth. CRD YAML files in `deploy/crds/` are auto-generated.

### Modifying Existing CRDs

1. **Edit the Rust type** in `src/crd.rs`:
```rust
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "bindy.firestoned.io",
    version = "v1beta1",
    kind = "Bind9Cluster",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct Bind9ClusterSpec {
    pub version: Option<String>,
    // Add new fields here
    pub new_field: Option<String>,
}
```

2. **Regenerate YAML files**:
```bash
cargo run --bin crdgen
# or
make crds
```

3. **Verify the generated YAML**:
```bash
# Check the generated file
cat deploy/crds/bind9clusters.crd.yaml

# Validate it
kubectl apply --dry-run=client -f deploy/crds/bind9clusters.crd.yaml
```

4. **Update documentation** to describe the new field

### Adding New CRDs

1. **Define the CustomResource** in `src/crd.rs`
2. **Add to crdgen** in `src/bin/crdgen.rs`:
```rust
generate_crd::<MyNewResource>("mynewresources.crd.yaml", output_dir)?;
```
3. **Regenerate YAMLs**: `make crds`
4. **Export the type** in `src/lib.rs` if needed

### Generated YAML Format

All generated CRD files include:
- Copyright header
- SPDX license identifier
- Auto-generated warning

**Never edit YAML files directly** - they will be overwritten!

## Local Testing

```bash
# Start kind cluster
kind create cluster --name bindy-dev

# Deploy CRDs (regenerate first if modified)
make crds
kubectl apply -k deploy/crds/

# Run controller locally
RUST_LOG=debug cargo run
```

## Hot Reload

```bash
# Auto-rebuild on changes
cargo watch -x 'run --release'
```
