# Bindy Load Testing Framework - Implementation Prompt

## Context

Bindy is a Rust-based Kubernetes operator built with kube-rs that manages BIND9 DNS infrastructure declaratively. The operator watches custom resources (DNSZone, DNSRecord) and reconciles them to BIND9 instances via RNDC commands executed through a sidecar container called bindcar (a REST API wrapper around RNDC operations).

The architecture consists of:
- **bindy**: The Kubernetes operator (Rust, kube-rs, tokio async runtime)
- **bindcar**: REST API sidecar for RNDC operations (Rust, axum)
- **BIND9**: The managed DNS server instances
- **zonewarden**: Service discovery component (may be tested in integration)

## Objective

Design and implement a comprehensive load testing framework that validates bindy's performance, reliability, and failure handling under realistic and extreme conditions. The framework should be usable for:
1. Pre-release validation
2. Capacity planning
3. Regression detection
4. Chaos engineering experiments

---

## Test Dimensions

### 1. Operator Reconciliation Performance

#### 1.1 Burst Creation Load
Test the operator's ability to handle sudden influxes of custom resources.

**Scenarios:**
- Create 100/500/1000/5000 DNSRecord CRs simultaneously
- Create 10/50/100 DNSZone CRs with varying record counts (10, 100, 1000 records each)
- Mixed creation: zones and records interleaved

**Metrics to capture:**
- Time from CR creation to `Ready` condition
- Reconciliation queue depth over time
- p50/p95/p99 reconciliation duration
- Memory allocation patterns (watch for unbounded growth)
- CPU utilization during burst

**Implementation approach:**
```rust
use kube::{Api, Client};
use tokio::time::{Instant, Duration};
use futures::stream::{self, StreamExt};

pub struct BurstLoadTest {
    client: Client,
    namespace: String,
    concurrency: usize,
}

impl BurstLoadTest {
    pub async fn run_record_burst(&self, count: usize) -> BurstResult {
        let api: Api<DNSRecord> = Api::namespaced(self.client.clone(), &self.namespace);
        let start = Instant::now();
        
        let records: Vec<DNSRecord> = (0..count)
            .map(|i| self.generate_record(i))
            .collect();
        
        // Parallel creation with bounded concurrency
        let creation_results = stream::iter(records)
            .map(|record| {
                let api = api.clone();
                async move {
                    let name = record.metadata.name.clone().unwrap();
                    let create_time = Instant::now();
                    let result = api.create(&PostParams::default(), &record).await;
                    (name, create_time, result)
                }
            })
            .buffer_unordered(self.concurrency)
            .collect::<Vec<_>>()
            .await;
        
        // Wait for all to reach Ready state
        let ready_times = self.wait_for_ready_conditions(&creation_results).await;
        
        BurstResult {
            total_duration: start.elapsed(),
            creation_results,
            ready_times,
            // ... metrics
        }
    }
}
```

#### 1.2 Sustained Throughput
Test steady-state performance under continuous load.

**Scenarios:**
- Constant rate: 10/50/100 CR operations per second for 10/30/60 minutes
- Mixed operations: 60% create, 30% update, 10% delete
- Record churn: high turnover simulating dynamic service discovery

**Metrics:**
- Sustained reconciliation rate (ops/sec)
- Queue depth stability (should not grow unbounded)
- Memory stability over time (leak detection)
- Error rate and retry patterns

```rust
pub struct SustainedLoadConfig {
    pub target_rate: f64,           // operations per second
    pub duration: Duration,
    pub operation_mix: OperationMix,
    pub ramp_up: Duration,          // gradual ramp to target rate
}

pub struct OperationMix {
    pub create_weight: u32,  // e.g., 60
    pub update_weight: u32,  // e.g., 30
    pub delete_weight: u32,  // e.g., 10
}
```

#### 1.3 Reconciliation Backpressure
Test behavior when reconciliation cannot keep up with incoming changes.

**Scenarios:**
- Artificially slow RNDC operations (via bindcar delay injection)
- Reduce operator resource limits while maintaining load
- Simulate API server throttling (429 responses)

**Expected behaviors to validate:**
- Graceful degradation (no crashes)
- Queue depth limits respected
- Exponential backoff on failures
- No duplicate reconciliations causing conflicts

---

### 2. BIND9 Backend Performance

#### 2.1 Zone Configuration Stress
Test BIND9's ability to handle the configurations bindy generates.

**Scenarios:**
- Single zone with 1K/10K/100K records
- Many zones: 100/500/1000 zones with 100 records each
- Complex records: MX, SRV, TXT with large payloads
- DNSSEC-signed zones (if supported)

**Tools:**
```bash
# Use dnsperf for query load
dnsperf -s ${BIND_IP} -d queries.txt -c 100 -Q 10000 -l 60

# Use queryperf for resolution testing  
queryperf -s ${BIND_IP} -d queries.txt

# Named-checkzone for configuration validation
named-checkzone example.com /var/named/example.com.zone
```

**Metrics:**
- Query latency (p50/p95/p99)
- Queries per second sustained
- Zone transfer time (for secondary scenarios)
- BIND9 memory/CPU utilization

#### 2.2 RNDC Operation Performance
Test bindcar and RNDC command throughput.

**Scenarios:**
- Rapid zone reloads: `rndc reload zone` at high frequency
- Concurrent RNDC commands from multiple reconciliations
- Large zone file writes followed by reload

```rust
pub struct RndcLoadTest {
    bindcar_url: String,
    client: reqwest::Client,
}

impl RndcLoadTest {
    pub async fn reload_stress(&self, zone: &str, count: usize, concurrency: usize) {
        let results = stream::iter(0..count)
            .map(|_| self.trigger_reload(zone))
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;
        
        // Analyze timing distribution
    }
    
    async fn trigger_reload(&self, zone: &str) -> RndcResult {
        let start = Instant::now();
        let response = self.client
            .post(format!("{}/zones/{}/reload", self.bindcar_url, zone))
            .send()
            .await;
        RndcResult {
            duration: start.elapsed(),
            success: response.map(|r| r.status().is_success()).unwrap_or(false),
        }
    }
}
```

---

### 3. Failure Injection & Chaos Engineering

#### 3.1 Component Failure Scenarios

| Scenario | Implementation | Expected Behavior |
|----------|---------------|-------------------|
| BIND9 pod crash mid-reconciliation | `kubectl delete pod` | Operator retries, no data loss |
| bindcar sidecar unavailable | Kill sidecar, network policy | Backoff, eventual recovery |
| API server partitioned | Network policy on operator | Queue locally, resume on reconnect |
| Operator OOM kill | Set low memory limit + load | Restart, re-sync from API server |
| BIND9 zone file corruption | Inject bad zone data | Validation failure, rollback |
| Split-brain: multiple operators | Scale replicas > 1 without leader election | Validate leader election works |

#### 3.2 Network Chaos

```yaml
# Example Chaos Mesh network delay
apiVersion: chaos-mesh.org/v1alpha1
kind: NetworkChaos
metadata:
  name: bindcar-latency
spec:
  action: delay
  mode: all
  selector:
    labelSelectors:
      app: bindcar
  delay:
    latency: "500ms"
    jitter: "100ms"
  duration: "5m"
```

**Scenarios:**
- Latency injection: 100ms/500ms/2s between operator and bindcar
- Packet loss: 1%/5%/20% loss rates
- Partition: Complete network isolation of components
- Bandwidth throttling: Slow zone file transfers

#### 3.3 Resource Exhaustion

**Scenarios:**
- Disk full on BIND9 (zone file writes fail)
- Memory pressure on operator pod
- CPU throttling during peak load
- File descriptor exhaustion (too many watches)
- Etcd storage limits (too many CRs)

---

### 4. Integration & End-to-End Scenarios

#### 4.1 Full Stack Load Test

Simulate realistic production traffic patterns:

```rust
pub struct ProductionSimulation {
    // Cluster configuration
    pub zone_count: usize,              // e.g., 50 zones
    pub records_per_zone: Range<usize>, // e.g., 100..1000
    
    // Traffic patterns
    pub churn_rate: f64,                // records changed per minute
    pub query_rate: f64,                // DNS queries per second
    pub burst_probability: f64,         // chance of traffic spike
    pub burst_multiplier: f64,          // spike magnitude
    
    // Duration
    pub duration: Duration,
    pub measurement_interval: Duration,
}

impl ProductionSimulation {
    pub async fn run(&self) -> SimulationReport {
        // 1. Setup: Create initial zones and records
        // 2. Background: Continuous DNS query load via dnsperf
        // 3. Foreground: CR churn with occasional bursts
        // 4. Measurement: Collect metrics at intervals
        // 5. Chaos: Inject failures per schedule
        // 6. Report: Aggregate and analyze
    }
}
```

#### 4.2 Zonewarden Integration

If testing with zonewarden (service discovery):

**Scenarios:**
- Rapid service endpoint changes (pod scaling)
- Headless service with many endpoints
- Cross-cluster service discovery load
- Annotation-driven record generation at scale

---

### 5. Metrics & Observability Requirements

#### 5.1 Operator Metrics (Prometheus)

The load test framework should validate these metrics exist and are accurate:

```rust
// Metrics the operator should expose
pub struct OperatorMetrics {
    // Reconciliation
    reconcile_duration_seconds: Histogram,      // by resource type, result
    reconcile_total: Counter,                   // by resource type, result
    reconcile_queue_depth: Gauge,               // current queue size
    
    // RNDC operations  
    rndc_operation_duration_seconds: Histogram, // by operation type
    rndc_operation_total: Counter,              // by operation type, result
    
    // Resources
    managed_zones_total: Gauge,
    managed_records_total: Gauge,
    
    // Errors
    errors_total: Counter,                      // by error type
    retries_total: Counter,
}
```

#### 5.2 Load Test Metrics

```rust
pub struct LoadTestMetrics {
    // Timing
    pub operation_latency: HistogramVec,        // by operation type
    pub e2e_propagation_time: Histogram,        // CR create to DNS resolvable
    
    // Throughput
    pub operations_per_second: Gauge,
    pub successful_operations: Counter,
    pub failed_operations: Counter,
    
    // System
    pub operator_memory_bytes: Gauge,
    pub operator_cpu_usage: Gauge,
    pub bind_query_latency: Histogram,
}
```

#### 5.3 Reporting

Generate reports in multiple formats:

```rust
pub enum ReportFormat {
    Json,           // Machine-readable for CI
    Markdown,       // Human-readable summary
    Html,           // Visual report with charts
    Prometheus,     // Push to Prometheus Pushgateway
}

pub struct LoadTestReport {
    pub metadata: TestMetadata,
    pub summary: TestSummary,
    pub reconciliation: ReconciliationStats,
    pub dns_performance: DnsPerformanceStats,
    pub failures: Vec<FailureEvent>,
    pub resource_utilization: ResourceTimeSeries,
    pub recommendations: Vec<String>,
}
```

---

### 6. Test Infrastructure

#### 6.1 Test Cluster Setup

```yaml
# Kind cluster config for local testing
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: control-plane
  - role: worker
  - role: worker
  - role: worker
containerdConfigPatches:
  - |-
    [plugins."io.containerd.grpc.v1.cri".registry.mirrors."localhost:5000"]
      endpoint = ["http://kind-registry:5000"]
```

```rust
pub struct TestCluster {
    pub kind: ClusterKind,          // Kind, k3d, or real cluster
    pub node_count: usize,
    pub resource_quotas: ResourceQuotas,
    pub network_policies: bool,
    pub monitoring_stack: bool,     // Deploy Prometheus/Grafana
    pub chaos_mesh: bool,           // Deploy Chaos Mesh
}
```

#### 6.2 Fixture Generation

```rust
pub struct FixtureGenerator {
    pub zone_name_pattern: String,      // "zone-{}.example.com"
    pub record_name_pattern: String,    // "host-{}"
    pub record_types: Vec<RecordType>,  // A, AAAA, CNAME, SRV, TXT
    pub ttl_range: Range<u32>,
    pub realistic_data: bool,           // Use realistic IPs, hostnames
}

impl FixtureGenerator {
    pub fn generate_zone(&self, index: usize, record_count: usize) -> DNSZone { ... }
    pub fn generate_record(&self, zone: &str, index: usize) -> DNSRecord { ... }
    pub fn generate_query_file(&self, zones: &[DNSZone]) -> String { ... }
}
```

#### 6.3 CI Integration

```yaml
# GitHub Actions workflow
name: Load Tests

on:
  schedule:
    - cron: '0 2 * * *'  # Nightly
  workflow_dispatch:
    inputs:
      profile:
        description: 'Test profile'
        required: true
        default: 'standard'
        type: choice
        options:
          - quick      # 5 min, basic validation
          - standard   # 30 min, full suite
          - extended   # 2 hr, stress testing
          - chaos      # 1 hr, failure injection

jobs:
  load-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup Kind cluster
        uses: helm/kind-action@v1
        
      - name: Deploy bindy
        run: |
          kubectl apply -f deploy/
          kubectl wait --for=condition=available deployment/bindy
          
      - name: Run load tests
        run: |
          cargo run --release --bin loadtest -- \
            --profile ${{ inputs.profile }} \
            --output results/
            
      - name: Upload results
        uses: actions/upload-artifact@v4
        with:
          name: load-test-results
          path: results/
          
      - name: Fail on regression
        run: |
          cargo run --bin analyze -- \
            --baseline baseline.json \
            --current results/report.json \
            --threshold 10%
```

---

### 7. Implementation Structure

```
bindy/
├── crates/
│   ├── bindy/              # Main operator
│   ├── bindcar/            # RNDC sidecar
│   └── loadtest/           # Load testing framework
│       ├── src/
│       │   ├── lib.rs
│       │   ├── main.rs           # CLI entry point
│       │   ├── config.rs         # Test configuration
│       │   ├── fixtures.rs       # CR generation
│       │   ├── scenarios/
│       │   │   ├── mod.rs
│       │   │   ├── burst.rs      # Burst load tests
│       │   │   ├── sustained.rs  # Sustained load tests
│       │   │   ├── chaos.rs      # Failure injection
│       │   │   └── e2e.rs        # Full stack tests
│       │   ├── metrics.rs        # Metrics collection
│       │   ├── reporting.rs      # Report generation
│       │   └── cluster.rs        # Test cluster management
│       ├── tests/
│       │   └── integration.rs
│       └── Cargo.toml
├── deploy/
│   └── loadtest/
│       ├── kind-config.yaml
│       ├── prometheus-values.yaml
│       └── chaos-mesh-values.yaml
└── .github/
    └── workflows/
        └── load-tests.yaml
```

---

### 8. CLI Interface

```
bindy-loadtest 0.1.0
Load testing framework for bindy DNS operator

USAGE:
    bindy-loadtest [OPTIONS] <COMMAND>

COMMANDS:
    burst       Run burst load test
    sustained   Run sustained load test
    chaos       Run chaos engineering scenarios
    e2e         Run end-to-end simulation
    report      Generate report from results
    baseline    Create performance baseline

OPTIONS:
    -n, --namespace <NS>      Kubernetes namespace [default: bindy-loadtest]
    -k, --kubeconfig <PATH>   Path to kubeconfig
    -o, --output <DIR>        Output directory for results
    -v, --verbose             Increase verbosity
    --prometheus <URL>        Prometheus endpoint for metrics
    --json                    Output results as JSON

EXAMPLES:
    # Quick burst test
    bindy-loadtest burst --records 1000 --concurrency 50

    # Sustained load for 30 minutes
    bindy-loadtest sustained --rate 50 --duration 30m --mix 60,30,10

    # Chaos testing with network delays
    bindy-loadtest chaos --scenario network-delay --duration 10m

    # Full simulation
    bindy-loadtest e2e --profile production --duration 1h
```

---

### 9. Success Criteria

Define pass/fail thresholds:

```rust
pub struct SuccessCriteria {
    // Latency
    pub reconcile_p99_max: Duration,        // e.g., 5s
    pub e2e_propagation_p99_max: Duration,  // e.g., 10s
    pub dns_query_p99_max: Duration,        // e.g., 50ms
    
    // Throughput
    pub min_sustained_ops_per_sec: f64,     // e.g., 50
    
    // Reliability
    pub max_error_rate: f64,                // e.g., 0.1%
    pub max_retry_rate: f64,                // e.g., 5%
    
    // Resources
    pub max_memory_mb: usize,               // e.g., 512
    pub max_cpu_cores: f64,                 // e.g., 1.0
    
    // Stability
    pub max_queue_depth: usize,             // e.g., 1000
    pub memory_growth_rate_max: f64,        // bytes/hour, leak detection
}
```

---

### 10. Future Enhancements

- **Distributed load generation**: Run from multiple nodes for higher throughput
- **Traffic replay**: Record production traffic patterns for replay
- **Comparative benchmarking**: Test against CoreDNS, external-dns
- **Multi-cluster scenarios**: Test cross-cluster DNS synchronization
- **Long-haul testing**: 24-72 hour stability runs
- **Fuzz testing**: Invalid CR inputs, malformed zone data

---

## Deliverables

1. **`bindy-loadtest` crate** - Rust library and CLI tool
2. **Test scenarios** - Implemented as described above
3. **CI integration** - GitHub Actions workflows
4. **Documentation** - Usage guide, interpretation of results
5. **Baseline data** - Performance baselines for regression detection
6. **Grafana dashboards** - Visualization of load test metrics

---

## References

- kube-rs documentation: https://kube.rs
- dnsperf: https://github.com/DNS-OARC/dnsperf
- Chaos Mesh: https://chaos-mesh.org
- Prometheus client for Rust: https://github.com/prometheus/client_rust
