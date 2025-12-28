# Bindy Load Testing Framework - Claude Code Roadmap

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
