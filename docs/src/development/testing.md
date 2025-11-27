# Running Tests

Run and write tests for Bindy.

## Unit Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

## Integration Tests

```bash
# Requires Kubernetes cluster
cargo test --test simple_integration -- --ignored

# Or use make
make test-integration
```

## Test Coverage

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage
cargo tarpaulin --out Html

# Open report
open tarpaulin-report.html
```

## Writing Tests

See [Testing Guidelines](./testing-guidelines.md) for details.
