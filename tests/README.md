# Integration Tests for Bindy DNS Operator

This directory contains integration tests that run against a Kind (Kubernetes in Docker) cluster.

## Quick Start

The e2e gate is **one Makefile target per suite**. Each suite owns its own kind
cluster, so any of them can be run on its own — and several can run at once.

```bash
make e2e-lifecycle       # zones/records come up and BIND9 actually serves them
make e2e-idempotency     # re-applying the identical spec changes nothing
make e2e-restart         # zones/records survive operator + operand restarts
make e2e-rust            # simple_integration.rs + scout_integration.rs, live API server
make e2e-multi-tenancy   # namespace isolation
make e2e-scout           # Scout's zone-scoped stale-cluster ARecord cleanup (#474)
make e2e-regression      # admission policies + operand pod shape + liveness
make e2e-zone-spread     # spec.placement topology spread (3-zone cluster)
make e2e-tls             # cert-manager-issued bindcar sidecar cert

make e2e-all             # all of the above, sequentially
make e2e-clean           # delete every kind cluster the suites create
```

Useful options:

| Variable | Effect |
|---|---|
| `E2E_IMAGE=<ref>` | Run against a prebuilt image instead of building one locally |
| `KEEP_CLUSTER=1` | Leave the kind cluster up after the suite, for debugging |

Building the operator image is the slowest part of a cold run, so build it once
and share it:

```bash
make e2e-image                                        # build + save dist/bindy-e2e-image.tar
make e2e-lifecycle E2E_IMAGE=ghcr.io/firestoned/bindy:ci-e2e
make e2e-restart   E2E_IMAGE=ghcr.io/firestoned/bindy:ci-e2e
```

That is exactly what CI does: `.github/workflows/e2e.yaml` builds the image in
one job, uploads the tarball, and runs the eight suite targets as eight parallel
jobs so a failure names the suite that broke.

To run everything against a **single** cluster instead (the older workflow):

```bash
make kind-integration-test     # tests/integration_test.sh
```

## Test Structure

```
tests/
├── lib/                     shared bash libraries (sourced, never run)
│   ├── common.sh            colours, logging, assertions, arg parsing, summaries
│   ├── cluster.sh           kind cluster + image + operator bring-up
│   └── dns_fixtures.sh      the zone/record fixture and its assertions
├── e2e/                     one file per suite, each its own program
│   ├── lifecycle_test.sh
│   ├── idempotency_test.sh
│   ├── restart_test.sh
│   ├── rust_api_test.sh
│   ├── multi_tenancy_test.sh
│   └── scout_test.sh
├── simple_integration.rs    CRD/client contract across every record kind
├── scout_integration.rs     Scout stale-cleanup selectors vs a real API server
├── integration_test.sh      orchestrator: every DNS suite against one cluster
├── regression_test.sh       admission policies + operand pod shape + liveness
├── zone_spread_test.sh      spec.placement topology spread
└── tls_transport_test.sh    cert-manager-issued sidecar certificate
```

Every suite takes the same two options:

```bash
tests/e2e/lifecycle_test.sh --image ghcr.io/firestoned/bindy:ci-e2e   # skip the local build
tests/e2e/lifecycle_test.sh --skip-deploy                             # reuse a running cluster
CLUSTER_NAME=my-cluster tests/e2e/lifecycle_test.sh                   # pick the kind cluster
```

### The DNS suites

`lifecycle`, `idempotency` and `restart` share one fixture (`tests/lib/dns_fixtures.sh`):
a Bind9Cluster with 2 primaries, a standalone Bind9Instance, a forward and a
reverse DNSZone, and one CR of every supported record type. They differ in what
they then do to it.

**`e2e-lifecycle`** — the baseline. Applies the fixture, checks every CR exists,
waits for all three primaries to be Ready, and then **verifies BIND9 actually
serves every zone and record** by running `dig` inside each operand container. A
CR existing in Kubernetes proves nothing about what BIND9 is serving, so this is
the assertion that matters. If this suite is red, the other two are meaningless.

**`e2e-idempotency`** — applies the fixture, then re-applies it unchanged twice.
The failure this catches is not a `kubectl` error (there never is one) but the
operator reacting to a no-change update with a duplicate Bind9Instance, a second
zone, or a reconcile storm that drops a record it was already serving. Asserts a
resource census plus a full DNS re-check after each round.

**`e2e-restart`** — the slowest suite, and why it is its own job. Restarts the
operator (it holds no state of its own, so it must rebuild its watches and reach
the same conclusion about zones it did not create in this process lifetime),
then deletes **every** BIND9 operand Pod. A recreated Pod starts with an empty
zone directory, so anything that answers afterwards was re-pushed by the
operator rather than restored from disk — the zone/record replay path (#486). A
full operand wipe costs about 100s of replay.

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

### Everything against one cluster

```bash
./tests/integration_test.sh              # or: make kind-integration-test
./tests/integration_test.sh --skip-restart   # omit the slow restart suite
```

This brings up one kind cluster and runs `rust_api`, `lifecycle`, `idempotency`
and `restart` against it in order, each with `--skip-deploy` so they share the
cluster. It reports which suites failed, not just that something did.

Prefer the individual `make e2e-*` targets when you want one answer fast, or
want them running in parallel.

### Gotchas when debugging by hand

- The operand serves DNS on **port 5353**, not 53, so it can run without
  `NET_BIND_SERVICE`. `dig @127.0.0.1 -p 5353 ...` inside the `bind9` container.
- `named.conf.options` sets `rate-limit { responses-per-second 15; }`. Firing a burst
  of `dig` calls in a shell loop gets answers silently dropped, which looks exactly
  like missing records. Query one at a time, or use `AXFR` to dump the whole zone.

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

✅ **DNS Records** (All 9 Types)
- A Record (IPv4)
- AAAA Record (IPv6)
- CNAME Record
- MX Record
- TXT Record
- NS Record
- SRV Record
- CAA Record
- PTR Record (reverse DNS)

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
  ✓ ptrrecord created

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
kubectl get pods -n bindy-system

# View operator logs
kubectl logs -n bindy-system -l app=bindy
```

### Manual Cleanup

```bash
# Delete test resources
kubectl delete bind9instances,dnszones,arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords,ptrrecords --all -n bindy-system

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
