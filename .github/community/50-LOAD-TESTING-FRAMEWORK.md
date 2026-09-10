# Bindy Load Testing Framework - Claude Code Roadmap

> **Status:** ⛔ Not started — the target layout (`crates/loadtest/`) does not exist; the repo has no `crates/` directory at all, so this lands **after** the workspace conversion in [10](10-CONTROLLER-CRATE-SPLIT.md).
>
> *Migrated 2026-09-10 from the external roadmap set. Status verified against `fix-idempotency` @ `648ff7a`.*

---

## Project Overview

Build a comprehensive load testing framework for bindy, a Rust-based Kubernetes operator that manages BIND9 DNS infrastructure. The framework will validate performance, reliability, and failure handling under realistic and extreme conditions.

**Repository:** github.com/firestoned/bindy
**Target location:** `crates/loadtest/`

---

## Phase 1: Foundation

### Milestone 1.1: Crate Scaffolding

Create the basic crate structure within the bindy workspace.

**Tasks:**

- [ ] Create `crates/loadtest/Cargo.toml` with dependencies:
  - `kube` (client-rs features)
  - `k8s-openapi` (matching bindy's k8s version)
  - `tokio` (full features)
  - `futures`
  - `reqwest` (for bindcar API calls)
  - `clap` (derive feature for CLI)
  - `serde` / `serde_json`
  - `tracing` / `tracing-subscriber`
  - `prometheus-client`
  - `chrono`
  - `hdrhistogram` (for latency percentiles)
  - `tokio-metrics` (optional, for runtime stats)

- [ ] Create directory structure:
  ```
  crates/loadtest/
  ├── src/
  │   ├── lib.rs
  │   ├── main.rs
  │   ├── config.rs
  │   ├── fixtures.rs
  │   ├── metrics.rs
  │   ├── reporting.rs
  │   ├── cluster.rs
  │   └── scenarios/
  │       └── mod.rs
  └── Cargo.toml
  ```

- [ ] Add `loadtest` to workspace members in root `Cargo.toml`

- [ ] Implement basic CLI skeleton in `main.rs` using clap with subcommands:
  - `burst`
  - `sustained`
  - `chaos`
  - `e2e`
  - `report`
  - `baseline`

**Acceptance:** `cargo build -p loadtest` succeeds, `cargo run -p loadtest -- --help` shows CLI

---

### Milestone 1.2: Configuration System

Implement typed configuration for test scenarios.

**Tasks:**

- [ ] Define `LoadTestConfig` struct in `config.rs`:
  ```rust
  pub struct LoadTestConfig {
      pub namespace: String,
      pub kubeconfig: Option<PathBuf>,
      pub output_dir: PathBuf,
      pub prometheus_url: Option<String>,
  }
  ```

- [ ] Define scenario-specific configs:
  ```rust
  pub struct BurstConfig {
      pub record_count: usize,
      pub zone_count: usize,
      pub concurrency: usize,
      pub timeout: Duration,
  }

  pub struct SustainedConfig {
      pub target_rate: f64,
      pub duration: Duration,
      pub operation_mix: OperationMix,
      pub ramp_up: Duration,
  }

  pub struct OperationMix {
      pub create_weight: u32,
      pub update_weight: u32,
      pub delete_weight: u32,
  }
  ```

- [ ] Implement `Default` for all configs with sensible values

- [ ] Add config loading from TOML file (optional override)

**Acceptance:** Configs parse from CLI args and optional TOML file

---

### Milestone 1.3: Kubernetes Client Setup

Establish connection to the test cluster.

**Tasks:**

- [ ] Implement `cluster.rs` with `TestCluster` struct:
  ```rust
  pub struct TestCluster {
      client: Client,
      namespace: String,
  }

  impl TestCluster {
      pub async fn connect(config: &LoadTestConfig) -> Result<Self>;
      pub async fn setup_namespace(&self) -> Result<()>;
      pub async fn cleanup_namespace(&self) -> Result<()>;
      pub fn client(&self) -> &Client;
  }
  ```

- [ ] Add namespace creation with proper labels for test isolation

- [ ] Implement cleanup that deletes all test resources (zones, records)

- [ ] Add validation that bindy CRDs are installed

- [ ] Add check that bindy operator is running

**Acceptance:** Can connect to cluster, create test namespace, verify bindy is operational

---

## Phase 2: Fixture Generation

### Milestone 2.1: CR Generators

Build generators for DNSZone and DNSRecord custom resources.

**Tasks:**

- [ ] Import or reference bindy's CR types (DNSZone, DNSRecord specs)

- [ ] Implement `FixtureGenerator` in `fixtures.rs`:
  ```rust
  pub struct FixtureGenerator {
      zone_prefix: String,
      record_prefix: String,
      base_domain: String,
  }

  impl FixtureGenerator {
      pub fn generate_zone(&self, index: usize) -> DNSZone;
      pub fn generate_record(&self, zone: &str, index: usize) -> DNSRecord;
      pub fn generate_zones_with_records(
          &self,
          zone_count: usize,
          records_per_zone: usize,
      ) -> (Vec<DNSZone>, Vec<DNSRecord>);
  }
  ```

- [ ] Support multiple record types: A, AAAA, CNAME, TXT, MX, SRV

- [ ] Generate realistic data:
  - Valid IP addresses (10.x.y.z range)
  - Proper TTL values (300-86400)
  - Valid hostnames

- [ ] Add `generate_query_file()` for dnsperf compatibility:
  ```rust
  pub fn generate_query_file(&self, records: &[DNSRecord]) -> String;
  ```

**Acceptance:** Can generate 10,000 valid DNSRecord CRs, output dnsperf query file

---

### Milestone 2.2: CR Operations

Implement CRUD operations with timing instrumentation.

**Tasks:**

- [ ] Create `CROperations` struct:
  ```rust
  pub struct CROperations {
      client: Client,
      namespace: String,
  }

  impl CROperations {
      pub async fn create_zone(&self, zone: &DNSZone) -> Result<OperationResult>;
      pub async fn create_record(&self, record: &DNSRecord) -> Result<OperationResult>;
      pub async fn update_record(&self, record: &DNSRecord) -> Result<OperationResult>;
      pub async fn delete_record(&self, name: &str) -> Result<OperationResult>;
      pub async fn wait_for_ready(&self, name: &str, timeout: Duration) -> Result<Duration>;
  }

  pub struct OperationResult {
      pub name: String,
      pub operation: OperationType,
      pub duration: Duration,
      pub success: bool,
      pub error: Option<String>,
  }
  ```

- [ ] Implement `wait_for_ready` that watches for `Ready` condition on CR status

- [ ] Add retry logic with exponential backoff for transient failures

- [ ] Instrument all operations with tracing spans

**Acceptance:** Can create/update/delete CRs with timing data, detect Ready state

---

## Phase 3: Metrics & Reporting

### Milestone 3.1: Metrics Collection

Implement metrics gathering during test runs.

**Tasks:**

- [ ] Create `metrics.rs` with `MetricsCollector`:
  ```rust
  pub struct MetricsCollector {
      reconcile_latency: Histogram,
      operation_latency: Histogram,
      e2e_latency: Histogram,
      operations_total: Counter,
      errors_total: Counter,
      queue_depth: Gauge,
  }

  impl MetricsCollector {
      pub fn record_operation(&self, op: &OperationResult);
      pub fn record_reconcile(&self, duration: Duration);
      pub fn snapshot(&self) -> MetricsSnapshot;
  }
  ```

- [ ] Use `hdrhistogram` for accurate percentile calculations

- [ ] Implement Prometheus metrics scraping from operator (if exposed):
  ```rust
  pub async fn scrape_operator_metrics(&self, url: &str) -> Result<OperatorMetrics>;
  ```

- [ ] Add periodic metrics sampling (configurable interval)

- [ ] Track resource utilization via Kubernetes metrics API:
  ```rust
  pub async fn get_pod_metrics(&self, pod: &str) -> Result<ResourceMetrics>;
  ```

**Acceptance:** Collect latency histograms, error rates, resource utilization during tests

---

### Milestone 3.2: Report Generation

Generate human and machine-readable reports.

**Tasks:**

- [ ] Define report structures in `reporting.rs`:
  ```rust
  pub struct LoadTestReport {
      pub metadata: TestMetadata,
      pub summary: TestSummary,
      pub latency_stats: LatencyStats,
      pub throughput_stats: ThroughputStats,
      pub error_analysis: ErrorAnalysis,
      pub resource_utilization: ResourceStats,
      pub timeline: Vec<TimelineEvent>,
  }

  pub struct LatencyStats {
      pub p50: Duration,
      pub p95: Duration,
      pub p99: Duration,
      pub max: Duration,
      pub mean: Duration,
  }
  ```

- [ ] Implement JSON output for CI integration

- [ ] Implement Markdown output for human review

- [ ] Add baseline comparison:
  ```rust
  pub fn compare_to_baseline(
      current: &LoadTestReport,
      baseline: &LoadTestReport,
      threshold: f64,
  ) -> ComparisonResult;
  ```

- [ ] Generate pass/fail verdict based on `SuccessCriteria`:
  ```rust
  pub struct SuccessCriteria {
      pub reconcile_p99_max: Duration,
      pub max_error_rate: f64,
      pub max_memory_mb: usize,
  }
  ```

**Acceptance:** Generate JSON + Markdown reports, compare against baseline with threshold

---

## Phase 4: Test Scenarios

### Milestone 4.1: Burst Load Test

Implement burst creation scenario.

**Tasks:**

- [ ] Create `scenarios/burst.rs`:
  ```rust
  pub struct BurstScenario {
      config: BurstConfig,
      cluster: TestCluster,
      generator: FixtureGenerator,
      metrics: MetricsCollector,
  }

  impl BurstScenario {
      pub async fn run(&self) -> Result<ScenarioResult>;
  }
  ```

- [ ] Implement parallel CR creation with bounded concurrency:
  ```rust
  async fn create_records_parallel(
      &self,
      records: Vec<DNSRecord>,
      concurrency: usize,
  ) -> Vec<OperationResult>;
  ```

- [ ] Wait for all CRs to reach Ready state

- [ ] Measure:
  - Total time from first create to last Ready
  - Individual operation latencies
  - Queue depth during burst (if metrics available)

- [ ] Wire up CLI: `bindy-loadtest burst --records 1000 --concurrency 50`

**Acceptance:** Burst test runs, creates N records in parallel, reports timing stats

---

### Milestone 4.2: Sustained Load Test

Implement sustained throughput scenario.

**Tasks:**

- [ ] Create `scenarios/sustained.rs`:
  ```rust
  pub struct SustainedScenario {
      config: SustainedConfig,
      // ...
  }
  ```

- [ ] Implement rate-limited operation dispatch:
  ```rust
  async fn dispatch_at_rate(
      &self,
      rate: f64,
      duration: Duration,
      mix: &OperationMix,
  ) -> Vec<OperationResult>;
  ```

- [ ] Use token bucket or leaky bucket for rate limiting

- [ ] Implement operation mix selection (weighted random)

- [ ] Track sustained metrics over time (periodic snapshots)

- [ ] Detect queue backup (operations falling behind target rate)

- [ ] Wire up CLI: `bindy-loadtest sustained --rate 50 --duration 30m --mix 60,30,10`

**Acceptance:** Sustained test maintains target rate for duration, reports stability metrics

---

### Milestone 4.3: Chaos Scenarios

Implement failure injection tests.

**Tasks:**

- [ ] Create `scenarios/chaos.rs`:
  ```rust
  pub enum ChaosScenario {
      PodKill { selector: String, interval: Duration },
      NetworkDelay { target: String, latency: Duration },
      ResourcePressure { memory_limit: String },
  }
  ```

- [ ] Implement pod kill scenario:
  ```rust
  async fn kill_pod_periodically(
      &self,
      selector: &str,
      interval: Duration,
      duration: Duration,
  );
  ```

- [ ] Integrate with Chaos Mesh (if available):
  ```rust
  async fn apply_network_chaos(&self, spec: NetworkChaosSpec) -> Result<()>;
  ```

- [ ] Implement manual chaos (no Chaos Mesh dependency):
  - Pod deletion via API
  - Resource limit patching

- [ ] Run background load during chaos

- [ ] Measure recovery time after failures

- [ ] Wire up CLI: `bindy-loadtest chaos --scenario pod-kill --duration 10m`

**Acceptance:** Chaos tests inject failures, measure recovery, verify no data loss

---

### Milestone 4.4: End-to-End Simulation

Implement production traffic simulation.

**Tasks:**

- [ ] Create `scenarios/e2e.rs`:
  ```rust
  pub struct E2EScenario {
      pub zone_count: usize,
      pub records_per_zone: Range<usize>,
      pub churn_rate: f64,
      pub duration: Duration,
  }
  ```

- [ ] Setup phase: create baseline zones and records

- [ ] Run continuous churn (create/update/delete mix)

- [ ] Optionally run dnsperf in background (subprocess or container)

- [ ] Collect comprehensive metrics throughout

- [ ] Generate timeline of events and metrics

- [ ] Wire up CLI: `bindy-loadtest e2e --zones 50 --records 100-500 --duration 1h`

**Acceptance:** Full simulation runs for configured duration, generates comprehensive report

---

## Phase 5: DNS Validation

### Milestone 5.1: DNS Query Testing

Verify DNS actually resolves correctly.

**Tasks:**

- [ ] Add DNS resolution verification:
  ```rust
  pub struct DnsValidator {
      resolver: TokioAsyncResolver,
      bind_service: String,
  }

  impl DnsValidator {
      pub async fn verify_record(&self, record: &DNSRecord) -> Result<ValidationResult>;
      pub async fn verify_all(&self, records: &[DNSRecord]) -> ValidationReport;
  }
  ```

- [ ] Measure DNS query latency

- [ ] Detect propagation delay (CR Ready → DNS resolvable)

- [ ] Add to scenarios: verify records resolve after creation

- [ ] Optionally integrate dnsperf for query load:
  ```rust
  pub async fn run_dnsperf(
      &self,
      query_file: &Path,
      qps: u32,
      duration: Duration,
  ) -> DnsPerfResult;
  ```

**Acceptance:** Tests verify DNS resolution works, measure query latency

---

### Milestone 5.2: Bindcar Integration

Test RNDC operations via bindcar.

**Tasks:**

- [ ] Add bindcar client:
  ```rust
  pub struct BindcarClient {
      base_url: String,
      client: reqwest::Client,
  }

  impl BindcarClient {
      pub async fn reload_zone(&self, zone: &str) -> Result<Duration>;
      pub async fn zone_status(&self, zone: &str) -> Result<ZoneStatus>;
      pub async fn health(&self) -> Result<bool>;
  }
  ```

- [ ] Add RNDC operation stress test:
  ```rust
  pub async fn rndc_stress(
      &self,
      zone: &str,
      operations: usize,
      concurrency: usize,
  ) -> RndcStressResult;
  ```

- [ ] Measure RNDC latency percentiles

**Acceptance:** Can stress test bindcar, measure RNDC operation performance

---

## Phase 6: CI Integration

### Milestone 6.1: GitHub Actions Workflow

Create CI pipeline for automated load testing.

**Tasks:**

- [ ] Create `.github/workflows/load-tests.yaml`:
  - Trigger: nightly schedule, manual dispatch
  - Matrix: quick/standard/extended profiles
  - Steps: setup Kind, deploy bindy, run tests, upload results

- [ ] Create test profiles in `deploy/loadtest/profiles/`:
  ```toml
  # quick.toml
  [burst]
  record_count = 100
  concurrency = 20

  [sustained]
  rate = 10
  duration = "2m"
  ```

- [ ] Add baseline management:
  - Store baseline in repo or artifact
  - Compare against baseline
  - Fail on regression > threshold

- [ ] Create Kind cluster config: `deploy/loadtest/kind-config.yaml`

**Acceptance:** CI runs nightly, fails on performance regression

---

### Milestone 6.2: Local Development Support

Make it easy to run locally.

**Tasks:**

- [ ] Create `Makefile` or `just` targets:
  ```makefile
  loadtest-setup:    ## Create Kind cluster with bindy
  loadtest-quick:    ## Run quick load test
  loadtest-full:     ## Run full load test suite
  loadtest-cleanup:  ## Tear down test cluster
  ```

- [ ] Add Docker Compose for local BIND9 testing (without k8s)

- [ ] Create `LOADTEST_README.md` with usage instructions

- [ ] Add example output / report for reference

**Acceptance:** Developer can run `make loadtest-quick` from fresh checkout

---

## Phase 7: Polish & Documentation

### Milestone 7.1: Error Handling & Resilience

Harden the framework.

**Tasks:**

- [ ] Add comprehensive error types:
  ```rust
  #[derive(thiserror::Error, Debug)]
  pub enum LoadTestError {
      #[error("Kubernetes error: {0}")]
      Kube(#[from] kube::Error),
      #[error("Timeout waiting for {resource} to become ready")]
      Timeout { resource: String },
      // ...
  }
  ```

- [ ] Handle cluster connection failures gracefully

- [ ] Add test timeout enforcement (don't run forever)

- [ ] Implement graceful shutdown (Ctrl+C cleanup)

- [ ] Add progress output for long-running tests

**Acceptance:** Framework handles errors gracefully, cleans up on interrupt

---

### Milestone 7.2: Documentation

Document usage and architecture.

**Tasks:**

- [ ] Write `crates/loadtest/README.md`:
  - Quick start
  - Configuration reference
  - Scenario descriptions
  - Interpreting results

- [ ] Add rustdoc comments to public API

- [ ] Create example reports in `docs/examples/`

- [ ] Document success criteria and how to tune them

- [ ] Add architecture diagram (Mermaid in docs)

**Acceptance:** New contributor can understand and run load tests from docs

---

## Success Criteria Defaults

Reference thresholds for pass/fail (tune based on actual performance):

| Metric | Quick | Standard | Extended |
|--------|-------|----------|----------|
| Reconcile p99 | < 10s | < 5s | < 3s |
| E2E propagation p99 | < 30s | < 15s | < 10s |
| Error rate | < 1% | < 0.5% | < 0.1% |
| Memory growth | < 100MB/hr | < 50MB/hr | < 20MB/hr |
| Sustained rate | 10 ops/s | 50 ops/s | 100 ops/s |

---

## Dependency Graph

```
Phase 1 (Foundation)
    │
    ├── 1.1 Scaffolding
    ├── 1.2 Configuration
    └── 1.3 Cluster Client
            │
Phase 2 (Fixtures) ──────────────────┐
    │                                │
    ├── 2.1 CR Generators            │
    └── 2.2 CR Operations            │
            │                        │
Phase 3 (Metrics) ───────────────────┤
    │                                │
    ├── 3.1 Collection               │
    └── 3.2 Reporting                │
            │                        │
            ▼                        │
Phase 4 (Scenarios) ◄────────────────┘
    │
    ├── 4.1 Burst
    ├── 4.2 Sustained
    ├── 4.3 Chaos
    └── 4.4 E2E
            │
Phase 5 (DNS) ───────────────────────┐
    │                                │
    ├── 5.1 Query Testing            │
    └── 5.2 Bindcar                  │
            │                        │
Phase 6 (CI) ◄───────────────────────┘
    │
    ├── 6.1 GitHub Actions
    └── 6.2 Local Dev
            │
Phase 7 (Polish)
    │
    ├── 7.1 Error Handling
    └── 7.2 Documentation
```

---

## Notes for Claude Code

- Always run `cargo check -p loadtest` after changes
- Run `cargo clippy -p loadtest` before marking tasks complete
- Test against a real Kind cluster when possible
- Reference existing bindy code patterns for consistency
- Keep dependencies minimal; prefer what's already in the workspace

---

## Appendix A — original implementation prompt

*Merged on migration from a second external document (`load-testing-prompt.md`) that covered the same work. Kept verbatim below, headings demoted one level.*

### Context

Bindy is a Rust-based Kubernetes operator built with kube-rs that manages BIND9 DNS infrastructure declaratively. The operator watches custom resources (DNSZone, DNSRecord) and reconciles them to BIND9 instances via RNDC commands executed through a sidecar container called bindcar (a REST API wrapper around RNDC operations).

The architecture consists of:
- **bindy**: The Kubernetes operator (Rust, kube-rs, tokio async runtime)
- **bindcar**: REST API sidecar for RNDC operations (Rust, axum)
- **BIND9**: The managed DNS server instances
- **zonewarden**: Service discovery component (may be tested in integration)

### Objective

Design and implement a comprehensive load testing framework that validates bindy's performance, reliability, and failure handling under realistic and extreme conditions. The framework should be usable for:
1. Pre-release validation
2. Capacity planning
3. Regression detection
4. Chaos engineering experiments

---

### Test Dimensions

#### 1. Operator Reconciliation Performance

##### 1.1 Burst Creation Load
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

##### 1.2 Sustained Throughput
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

##### 1.3 Reconciliation Backpressure
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

#### 2. BIND9 Backend Performance

##### 2.1 Zone Configuration Stress
Test BIND9's ability to handle the configurations bindy generates.

**Scenarios:**
- Single zone with 1K/10K/100K records
- Many zones: 100/500/1000 zones with 100 records each
- Complex records: MX, SRV, TXT with large payloads
- DNSSEC-signed zones (if supported)

**Tools:**
```bash
## Use dnsperf for query load
dnsperf -s ${BIND_IP} -d queries.txt -c 100 -Q 10000 -l 60

## Use queryperf for resolution testing  
queryperf -s ${BIND_IP} -d queries.txt

## Named-checkzone for configuration validation
named-checkzone example.com /var/named/example.com.zone
```

**Metrics:**
- Query latency (p50/p95/p99)
- Queries per second sustained
- Zone transfer time (for secondary scenarios)
- BIND9 memory/CPU utilization

##### 2.2 RNDC Operation Performance
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

#### 3. Failure Injection & Chaos Engineering

##### 3.1 Component Failure Scenarios

| Scenario | Implementation | Expected Behavior |
|----------|---------------|-------------------|
| BIND9 pod crash mid-reconciliation | `kubectl delete pod` | Operator retries, no data loss |
| bindcar sidecar unavailable | Kill sidecar, network policy | Backoff, eventual recovery |
| API server partitioned | Network policy on operator | Queue locally, resume on reconnect |
| Operator OOM kill | Set low memory limit + load | Restart, re-sync from API server |
| BIND9 zone file corruption | Inject bad zone data | Validation failure, rollback |
| Split-brain: multiple operators | Scale replicas > 1 without leader election | Validate leader election works |

##### 3.2 Network Chaos

```yaml
## Example Chaos Mesh network delay
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

##### 3.3 Resource Exhaustion

**Scenarios:**
- Disk full on BIND9 (zone file writes fail)
- Memory pressure on operator pod
- CPU throttling during peak load
- File descriptor exhaustion (too many watches)
- Etcd storage limits (too many CRs)

---

#### 4. Integration & End-to-End Scenarios

##### 4.1 Full Stack Load Test

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

##### 4.2 Zonewarden Integration

If testing with zonewarden (service discovery):

**Scenarios:**
- Rapid service endpoint changes (pod scaling)
- Headless service with many endpoints
- Cross-cluster service discovery load
- Annotation-driven record generation at scale

---

#### 5. Metrics & Observability Requirements

##### 5.1 Operator Metrics (Prometheus)

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

##### 5.2 Load Test Metrics

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

##### 5.3 Reporting

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

#### 6. Test Infrastructure

##### 6.1 Test Cluster Setup

```yaml
## Kind cluster config for local testing
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

##### 6.2 Fixture Generation

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

##### 6.3 CI Integration

```yaml
## GitHub Actions workflow
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

#### 7. Implementation Structure

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

#### 8. CLI Interface

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

#### 9. Success Criteria

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

#### 10. Future Enhancements

- **Distributed load generation**: Run from multiple nodes for higher throughput
- **Traffic replay**: Record production traffic patterns for replay
- **Comparative benchmarking**: Test against CoreDNS, external-dns
- **Multi-cluster scenarios**: Test cross-cluster DNS synchronization
- **Long-haul testing**: 24-72 hour stability runs
- **Fuzz testing**: Invalid CR inputs, malformed zone data

---

### Deliverables

1. **`bindy-loadtest` crate** - Rust library and CLI tool
2. **Test scenarios** - Implemented as described above
3. **CI integration** - GitHub Actions workflows
4. **Documentation** - Usage guide, interpretation of results
5. **Baseline data** - Performance baselines for regression detection
6. **Grafana dashboards** - Visualization of load test metrics

---

### References

- kube-rs documentation: https://kube.rs
- dnsperf: https://github.com/DNS-OARC/dnsperf
- Chaos Mesh: https://chaos-mesh.org
- Prometheus client for Rust: https://github.com/prometheus/client_rust
