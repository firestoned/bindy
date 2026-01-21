# Integration Tests for Bindy DNS Operator

This directory contains integration tests that run against a Kind (Kubernetes in Docker) cluster.

## Quick Start

### Option 1: Using Make (Recommended)

```bash
# Run full integration test suite
make kind-integration-test

# Or step by step:
make kind-deploy              # Deploy to Kind cluster
make kind-test                # Run basic functional tests
make kind-integration-test    # Run comprehensive integration tests
make kind-cleanup             # Clean up when done
```

### Option 2: Direct Script Execution

```bash
# Run all integration tests
./tests/integration_test.sh

# Or manually:
./deploy/kind-deploy.sh       # Setup Kind cluster and deploy operator
./deploy/kind-test.sh         # Basic functional tests
cargo test --test simple_integration -- --ignored  # Rust integration tests
```

## Test Structure

### Integration Test Script (`integration_test.sh`)

Comprehensive test suite that:
1. Creates/uses Kind cluster
2. Deploys Bindy operator
3. Runs Rust integration tests
4. Creates all resource types (Bind9Instance, DNSZone, all 8 DNS record types)
5. Verifies resources were created successfully
6. Cleans up test resources

### Rust Integration Tests

#### Simple Integration Tests (`simple_integration.rs`)

- **test_kubernetes_connectivity** - Verifies cluster access
- **test_crds_installed** - Checks for Bindy CRDs
- **test_create_and_cleanup_namespace** - Tests namespace management

#### Multi-Tenancy Integration Tests (`multi_tenancy_integration.rs`)

Comprehensive tests for the dual-cluster model:

- **test_bind9globalcluster_creation** - Cluster-scoped global cluster creation
- **test_bind9cluster_namespace_scoped** - Namespace-scoped cluster isolation
- **test_dnszone_with_global_cluster_ref** - DNSZone referencing global clusters
- **test_dnszone_with_cluster_ref** - DNSZone referencing namespace-scoped clusters
- **test_namespace_isolation** - Verify resources are isolated between namespaces
- **test_global_cluster_cross_namespace_access** - Global clusters accessible from all namespaces
- **test_bind9instance_references_global_cluster** - Instances can reference global clusters
- **test_list_global_clusters_across_all_namespaces** - List cluster-scoped resources
- **test_hybrid_deployment** - Production (global) + Development (namespaced) pattern

Run with:
```bash
# All multi-tenancy tests
./tests/run_multi_tenancy_tests.sh

# Specific test
./tests/run_multi_tenancy_tests.sh test_namespace_isolation

# Or directly with cargo
cargo test --test multi_tenancy_integration -- --ignored --nocapture --test-threads=1
```

## Prerequisites

- **Docker** - For Kind cluster
- **Kind** - Kubernetes in Docker
  ```bash
  brew install kind  # macOS
  ```
- **kubectl** - Kubernetes CLI
- **Rust** - For running cargo tests

## Running Tests

### Full Integration Test Suite

```bash
./tests/integration_test.sh
```

This will:
- ✅ Setup Kind cluster (if not exists)
- ✅ Deploy operator
- ✅ Run Rust integration tests
- ✅ Test Bind9Instance creation
- ✅ Test DNSZone creation
- ✅ Test all 8 DNS record types (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)
- ✅ Verify resource creation
- ✅ Clean up test resources

### Individual Test Components

```bash
# Just Rust integration tests
cargo test --test simple_integration -- --ignored

# Just functional tests (requires cluster)
./deploy/kind-test.sh
```

## Test Coverage

### Resources Tested

✅ **Bind9Instance**
- Primary instance creation
- Label selectors
- Replica configuration

✅ **DNSZone**
- Primary zone creation
- SOA record configuration
- Instance selector matching

✅ **DNS Records** (All 8 Types)
- A Record (IPv4)
- AAAA Record (IPv6)
- CNAME Record
- MX Record
- TXT Record
- NS Record
- SRV Record
- CAA Record

### Test Scenarios

1. **Cluster Setup** - Automated Kind cluster creation
2. **Operator Deployment** - Automated operator deployment
3. **Resource Creation** - All CRD types
4. **Resource Verification** - kubectl get/describe checks
5. **Cleanup** - Automatic resource cleanup

## Output Example

```
🧪 Running Bindy Integration Tests

✅ Using existing cluster 'bindy-test'

1️⃣  Running Rust integration tests...
test test_unit_tests_work ... ok
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

## Troubleshooting

### Tests Skip/Fail

**"Cluster 'bindy-test' not found"**
- The script will automatically create it
- Or manually run: `./deploy/kind-deploy.sh`

**"Operator not found"**
- Redeploy: `./deploy/kind-deploy.sh`
- Check logs: `make kind-logs`

**Resource Creation Fails**
```bash
# Check CRDs are installed
kubectl get crds | grep dns.firestoned.io

# Check operator status
kubectl get pods -n dns-system

# View operator logs
kubectl logs -n dns-system -l app=bindy
```

### Manual Cleanup

```bash
# Delete test resources
kubectl delete bind9instances,dnszones,arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords --all -n dns-system

# Delete cluster
kind delete cluster --name bindy-test
```

## CI/CD Integration

### GitHub Actions Example

```yaml
- name: Run Integration Tests
  run: |
    chmod +x tests/integration_test.sh
    ./tests/integration_test.sh
```

The script handles all setup automatically.

## Directory Structure

```
tests/
├── README.md                        # This file
├── integration_test.sh              # Main integration test script
├── run_multi_tenancy_tests.sh       # Multi-tenancy test runner
├── simple_integration.rs            # Basic Rust integration tests
├── multi_tenancy_integration.rs     # Multi-tenancy integration tests
└── common/
    └── mod.rs                       # Shared test utilities
```

## See Also

- [../TESTING_GUIDE.md](../TESTING_GUIDE.md) - Complete testing guide
- [../deploy/TESTING.md](../deploy/TESTING.md) - Kind deployment testing guide
- [../deploy/kind-deploy.sh](../deploy/kind-deploy.sh) - Deployment script
- [../deploy/kind-test.sh](../deploy/kind-test.sh) - Basic test script
