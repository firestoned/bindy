# Development Workflow

Daily development workflow for Bindy contributors.

## Development Cycle

1. **Create feature branch**
```bash
git checkout -b feature/my-feature
```

2. **Make changes**
- Edit code
- Add tests
- Update documentation

3. **Test locally**
```bash
cargo test
cargo clippy
cargo fmt
```

4. **Commit changes**
```bash
git add .
git commit -m "Add feature: description"
```

5. **Push and create PR**
```bash
git push origin feature/my-feature
# Create PR on GitHub
```

## Local Testing

```bash
# Start kind cluster
kind create cluster --name bindy-dev

# Deploy CRDs
kubectl apply -f deploy/crds/

# Run controller locally
RUST_LOG=debug cargo run
```

## Hot Reload

```bash
# Auto-rebuild on changes
cargo watch -x 'run --release'
```
