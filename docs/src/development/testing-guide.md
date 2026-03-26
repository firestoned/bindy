# Bindy DNS Operator - Testing Guide

Complete guide for testing the Bindy DNS Operator, including unit tests and integration tests with Kind (Kubernetes in Docker).

## Quick Start

```bash
# Unit tests (fast, no Kubernetes required)
make test

# Integration tests (automated with Kind cluster)
make kind-integration-test

# View results
# Unit: 62 tests passing
# Integration: All 8 DNS record types + infrastructure tests
```

## Table of Contents

- [Test Overview](#test-overview)
- [Unit Tests](#unit-tests)
- [Integration Tests](#integration-tests)
- [Makefile Targets](#makefile-targets)
- [Troubleshooting](#troubleshooting)
- [CI/CD Integration](#cicd-integration)

## Test Overview

### Test Results

**Unit Tests: 62 PASSING ✅**
```
test result: ok. 62 passed; 0 failed; 0 ignored
```

**Integration Tests: Automated with Kind**
- Kubernetes connectivity ✅
- CRD verification ✅
- All 8 DNS record types ✅
- Resource lifecycle ✅

### Test Structure

```
bindy/
├── src/
│   ├── crd_tests.rs              # CRD structure tests (28 tests)
│   └── reconcilers/
│       └── tests.rs              # Bind9Manager tests (34 tests)
├── tests/
│   ├── simple_integration.rs     # Rust integration tests
│   ├── integration_test.sh       # Full integration test suite
│   └── common/mod.rs            # Shared test utilities
└── deploy/
    ├── kind-deploy.sh           # Deploy to Kind cluster
    ├── kind-test.sh             # Basic functional tests
    └── kind-cleanup.sh          # Cleanup Kind cluster
```

## Unit Tests

Unit tests run locally without Kubernetes (< 1 second).

### Running Unit Tests

```bash
# All unit tests
make test
# or
cargo test

# Specific module
cargo test crd_tests::
cargo test bind9::tests::

# With output
cargo test -- --nocapture
```

### Unit Test Coverage (62 tests)

#### CRD Tests (28 tests)
- Label selectors and matching
- SOA record structure
- DNSZone specs (primary/secondary)
- All DNS record types (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)
- Bind9Instance configurations
- DNSSEC settings

#### Bind9Manager Tests (34 tests)
- Zone file creation
- Email formatting for DNS
- All DNS record types (with/without TTL)
- Secondary zone configuration
- Zone lifecycle (create, exists, delete)
- Edge cases and workflows

## Integration Tests

Integration tests run against Kind (Kubernetes in Docker) clusters.

### Prerequisites

```bash
# Docker
docker --version  # 20.10+

# Kind
kind --version    # 0.20.0+
brew install kind  # macOS

# kubectl
kubectl version --client  # 1.24+
```

### Running Integration Tests

#### Full Integration Suite (Recommended)

```bash
make kind-integration-test
```

This automatically:
1. Creates Kind cluster (if needed)
2. Builds and deploys operator
3. Runs all integration tests
4. Cleans up test resources

#### Step-by-Step

```bash
# 1. Deploy to Kind
make kind-deploy

# 2. Run functional tests
make kind-test

# 3. Run comprehensive integration tests
make kind-integration-test

# 4. View logs
make kind-logs

# 5. Cleanup
make kind-cleanup
```

### Integration Test Coverage

**Rust Integration Tests**
- `test_kubernetes_connectivity` - Cluster access
- `test_crds_installed` - CRD verification
- `test_create_and_cleanup_namespace` - Namespace lifecycle

**Full Integration Suite** (`integration_test.sh`)
- Bind9Instance creation
- DNSZone creation
- A Record (IPv4)
- AAAA Record (IPv6)
- CNAME Record
- MX Record
- TXT Record
- NS Record
- SRV Record
- CAA Record

### Expected Output

```
🧪 Running Bindy Integration Tests

✅ Using existing cluster 'bindy-test'

1️⃣  Running Rust integration tests...
test test_kubernetes_connectivity ... ok
test test_crds_installed ... ok
test test_create_and_cleanup_namespace ... ok

2️⃣  Running functional tests with kubectl...
Testing Bind9Instance creation...
Testing DNSZone creation...
Testing all DNS record types...

3️⃣  Verifying resources...
  ✓ Bind9Instance created
  ✓ DNSZone created
  ✓ arecord created
  ✓ aaaarecord created
  ✓ cnamerecord created
  ✓ mxrecord created
  ✓ txtrecord created
  ✓ nsrecord created
  ✓ srvrecord created
  ✓ caarecord created

✅ All integration tests passed!
```

## Makefile Targets

### Test Targets

```bash
make test                   # Run unit tests
make test-lib              # Library tests only
make test-integration      # Rust integration tests
make test-all             # Unit + Rust integration tests
make test-cov             # Coverage report (HTML)
make test-cov-view        # Generate and open coverage
```

### Kind Targets

```bash
make kind-create          # Create Kind cluster (Queen Bee / operator cluster)
make kind-deploy          # Deploy operator
make kind-test            # Basic functional tests
make kind-integration-test # Full integration suite
make kind-logs            # View operator logs
make kind-cleanup         # Delete cluster
```

#### Multi-Cluster Scout Testing

A second Kind cluster (`bindy-scout`) can be created to test Scout in multi-cluster mode — Scout running on the child cluster, writing `ARecord` CRs back to the Queen Bee cluster.

```bash
# Create and install Scout on the child cluster
make kind-create-scout IMAGE_TAG=latest-dev

# Tear it down when done
make kind-scout-cleanup
```

`kind-create-scout` performs all setup steps automatically:

1. Creates a single-node Kind cluster named `bindy-scout`
2. Loads the image into the cluster
3. Creates the `bindy-system` namespace
4. Installs all Bindy CRDs
5. Applies Scout RBAC (ServiceAccount, ClusterRole, ClusterRoleBinding, Role, RoleBinding)
6. Deploys the Scout controller
7. Waits for the deployment to become available

**Connecting Scout to the Queen Bee cluster (multi-cluster mode)**

After both clusters are running, use `bindy bootstrap mc` to generate credentials on the Queen Bee cluster and apply them to the Scout cluster:

```bash
# Switch to the Queen Bee cluster
kubectl config use-context kind-bindy-test

# Generate credentials and pipe directly to the Scout cluster
bindy bootstrap mc | kubectl --context kind-bindy-scout apply -f -

# Then patch the Scout Deployment to enable remote mode
kubectl --context kind-bindy-scout set env deployment/bindy-scout \
  BINDY_SCOUT_REMOTE_SECRET=bindy-scout-remote-kubeconfig \
  -n bindy-system
```

See the [Scout Multi-Cluster Guide](../guide/scout.md#multi-cluster-mode-phase-2) for the full architecture and configuration reference.

### Other Targets

```bash
make lint                 # Run clippy and fmt check
make format               # Format code
make build                # Build release binary
make docker-build         # Build Docker image
```

## Troubleshooting

### Unit Tests

**Tests fail to compile**
```bash
cargo clean
cargo test
```

**Specific test fails**
```bash
cargo test test_name -- --nocapture
```

### Integration Tests

**"Cluster not found"**
```bash
# Auto-created by integration test, or:
./deploy/kind-deploy.sh
```

**"Operator not ready"**
```bash
# Check status
kubectl get pods -n bindy-system

# View logs
kubectl logs -n bindy-system -l app=bindy

# Redeploy
./deploy/kind-deploy.sh
```

**"CRDs not installed"**
```bash
# Check CRDs
kubectl get crds | grep bindy.firestoned.io

# Install
kubectl apply -k deploy/crds
```

**Resource creation fails**
```bash
# Operator logs
kubectl logs -n bindy-system -l app=bindy --tail=50

# Resource status
kubectl describe bind9instance <name> -n bindy-system

# Events
kubectl get events -n bindy-system --sort-by='.lastTimestamp'
```

### Manual Cleanup

```bash
# Delete test resources
kubectl delete bind9instances,dnszones,arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords --all -n bindy-system

# Delete cluster
kind delete cluster --name bindy-test

# Clean build
cargo clean
```

## CI/CD Integration

### GitHub Actions

Current PR workflow (`.github/workflows/pr.yaml`):
- Lint (formatting, clippy)
- Test (unit tests)
- Build (stable, beta)
- Docker (build and push to ghcr.io)
- Security audit
- Coverage

### Add Integration Tests

```yaml
integration-tests:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable

    - name: Install Kind
      run: |
        curl -Lo ./kind https://kind.sigs.k8s.io/dl/latest/kind-linux-amd64
        chmod +x ./kind
        sudo mv ./kind /usr/local/bin/kind

    - name: Run Integration Tests
      run: |
        chmod +x tests/integration_test.sh
        ./tests/integration_test.sh
```

## Test Development

### Writing Unit Tests

Add to `src/crd_tests.rs` or `src/reconcilers/tests.rs`:

```rust
#[test]
fn test_my_feature() {
    // Arrange
    let (_temp_dir, manager) = create_test_manager();

    // Act
    let result = manager.my_operation();

    // Assert
    assert!(result.is_ok());
}
```

### Writing Integration Tests

Add to `tests/simple_integration.rs`:

```rust
#[tokio::test]
#[ignore]  // Always mark as ignored
async fn test_my_scenario() {
    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,  // Skip if no cluster
    };

    // Test code here
}
```

### Using Test Helpers

From `tests/common/mod.rs`:

```rust
use common::*;

let client = setup_dns_test_environment("my-test-ns").await?;
create_bind9_instance(&client, "ns", "dns", None).await?;
wait_for_ready(Duration::from_secs(10)).await;
cleanup_test_namespace(&client, "ns").await?;
```

## Performance Testing

### Coverage

```bash
make test-cov-view
# Opens coverage/tarpaulin-report.html
```

### Load Testing

```bash
# Create many resources
for i in {1..100}; do
  kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: test-${i}
  namespace: bindy-system
spec:
  zone: example.com
  name: host-${i}
  ipv4Addresses:
    - "192.0.2.${i}"
EOF
done

# Monitor
kubectl top pod -n bindy-system
```

## Best Practices

### Unit Tests
- Test one thing at a time
- Fast (< 1s each)
- No external dependencies
- Descriptive names

### Integration Tests
- Always use `#[ignore]`
- Check cluster connectivity first
- Unique namespaces
- Always cleanup
- Good error messages

### General
- Run `cargo fmt` before committing
- Run `cargo clippy` to catch issues
- Keep tests updated
- Document complex scenarios

## Additional Resources

- [Rust Testing](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Kube-rs Examples](https://github.com/kube-rs/kube/tree/main/examples)
- [Kind Docs](https://kind.sigs.k8s.io/)
- [BIND9 Docs](https://bind9.readthedocs.io/)
- [TEST_SUMMARY.md](TEST_SUMMARY.md) - Quick reference

## Support

- GitHub Issues: https://github.com/firestoned/bindy/issues
- Operator logs: `make kind-logs`
- Test with output: `cargo test -- --nocapture`
