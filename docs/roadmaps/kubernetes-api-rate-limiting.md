# Kubernetes API Rate Limiting Improvements

**Date:** 2026-01-01
**Status:** Proposed
**Impact:** High - Prevents API server overload in large deployments
**Author:** Erick Bourgeois

## Overview

This roadmap outlines improvements to ensure the Bindy operator respects Kubernetes API server rate limits and operates efficiently at scale. While the current implementation uses event-driven patterns (watches) and avoids polling, there are opportunities to add explicit rate limiting, pagination, and retry logic to prevent API server overload in large deployments.

## Current State Analysis

### ✅ What's Working Well

1. **Event-Driven Architecture**
   - Controllers use watch API instead of polling (follows CLAUDE.md best practices)
   - Efficient reconciliation triggered only on resource changes
   - No wasteful periodic listing

2. **Adaptive Requeue Intervals**
   - Ready resources: 5 minutes (reduced API pressure)
   - Degraded/not ready: 30 seconds (faster recovery)
   - Errors: 60 seconds (configurable)

3. **kube-rs Built-in Features**
   - Connection pooling and reuse
   - Automatic handling of 429 (Too Many Requests) errors
   - Watch API efficiency

### ⚠️ Areas for Improvement

1. **No Explicit Rate Limiter Configuration**
   - Client created with `Client::try_default()` uses default limits
   - No QPS (queries per second) or burst configuration
   - Risk: Multiple controller replicas could overwhelm API server

2. **List Operations Without Pagination**
   - `ListParams::default()` used throughout codebase
   - Large result sets load all items into memory
   - Locations:
     - `src/reconcilers/dnszone.rs` - zone discovery for `zonesFrom` selectors
     - `src/reconcilers/bind9instance.rs:799` - pod listing for status
     - `src/reconcilers/bind9cluster.rs:186` - instance listing
   - Risk: High memory usage and API server load with hundreds of resources

3. **No Exponential Backoff on Direct API Calls**
   - Direct calls to `api.get()`, `api.list()`, etc. have no retry logic
   - Transient errors cause immediate failures
   - Risk: Unnecessary error conditions during temporary API server pressure

4. **No Metrics for API Rate Limiting**
   - No visibility into rate limit hits or API client performance
   - Can't diagnose API server pressure issues
   - No alerting on rate limit violations

## Goals

1. **Prevent API Server Overload**
   - Configure explicit rate limits matching Kubernetes best practices
   - Implement pagination for large list operations
   - Add exponential backoff for transient errors

2. **Scale to Large Deployments**
   - Support hundreds of DNSZones per namespace
   - Support dozens of Bind9Instances per cluster
   - Support multiple controller replicas (leader election)

3. **Improve Observability**
   - Add Prometheus metrics for API client performance
   - Track rate limit hits, retries, and pagination
   - Enable proactive monitoring and alerting

4. **Maintain Event-Driven Patterns**
   - Do NOT introduce polling loops
   - Preserve watch-based reconciliation
   - Keep adaptive requeue intervals

## Implementation Phases

### Phase 1: Explicit Client Rate Limiting (High Priority)

**Goal:** Configure Kubernetes client with explicit QPS and burst limits.

**Tasks:**

1. **Add Constants for Rate Limits**
   - Create `src/constants.rs` entries:
     ```rust
     /// Kubernetes API client queries per second (sustained rate)
     pub const KUBE_CLIENT_QPS: f32 = 20.0;

     /// Kubernetes API client burst size (max concurrent requests)
     pub const KUBE_CLIENT_BURST: u32 = 30;
     ```
   - Document reasoning (matches kubectl defaults, tested at scale)

2. **Modify Client Initialization**
   - Update `src/main.rs::initialize_services()`:
     ```rust
     async fn initialize_services() -> Result<(Client, Arc<Bind9Manager>)> {
         debug!("Initializing Kubernetes client with rate limiting");

         // Load kubeconfig and apply rate limits
         let mut config = kube::Config::infer().await?;
         config.qps = KUBE_CLIENT_QPS;
         config.burst = KUBE_CLIENT_BURST;

         let client = Client::try_from(config)?;
         info!(
             qps = KUBE_CLIENT_QPS,
             burst = KUBE_CLIENT_BURST,
             "Kubernetes client initialized with rate limiting"
         );

         // ... rest of function
     }
     ```

3. **Add Environment Variable Overrides**
   - Support `BINDY_KUBE_QPS` and `BINDY_KUBE_BURST` env vars
   - Allow tuning for different cluster sizes
   - Document in deployment YAML and helm chart

4. **Testing**
   - Unit tests: Verify config is applied correctly
   - Integration tests: Create 100+ DNSZones, verify no 429 errors
   - Load tests: Run multiple controller replicas, measure API pressure

5. **Documentation**
   - Update `docs/src/configuration.md` with rate limit settings
   - Add tuning guide for large deployments
   - Document symptoms of rate limiting (429 errors, slow reconciliation)

**Acceptance Criteria:**
- Client initialized with configurable QPS and burst limits
- Default values prevent 429 errors in deployments with 500+ resources
- Environment variables allow tuning without recompilation
- Integration tests pass with 1000+ resources

**Estimated Effort:** 1-2 days

---

### Phase 2: Pagination for List Operations (High Priority)

**Goal:** Implement pagination for all `list()` operations to reduce memory usage and API server load.

**Tasks:**

1. **Add Pagination Helper Function**
   - Create `src/reconcilers/pagination.rs`:
     ```rust
     /// List all resources with automatic pagination
     ///
     /// # Arguments
     ///
     /// * `api` - Kubernetes API client for the resource type
     /// * `list_params` - Base list parameters (labels, fields, etc.)
     ///
     /// # Returns
     ///
     /// Vector of all resources, fetched in pages
     pub async fn list_all_paginated<K>(
         api: &Api<K>,
         mut list_params: ListParams,
     ) -> Result<Vec<K>>
     where
         K: kube::Resource<DynamicType = ()> + Clone + DeserializeOwned + Debug,
     {
         const PAGE_SIZE: u32 = 100;
         list_params.limit = Some(PAGE_SIZE);

         let mut all_items = Vec::new();

         loop {
             let result = api.list(&list_params).await?;
             all_items.extend(result.items);

             if let Some(continue_token) = result.metadata.continue_ {
                 list_params.continue_token = Some(continue_token);
             } else {
                 break;
             }
         }

         Ok(all_items)
     }
     ```

2. **Update Zone Discovery (bind9instance.rs)**
   - Replace direct `list()` calls in `discover_zones()` (line ~1094):
     ```rust
     // OLD:
     let zones = zones_api.list(&ListParams::default()).await?;

     // NEW:
     let zones = list_all_paginated(&zones_api, ListParams::default()).await?;
     ```

3. **Update Instance Listing (bind9cluster.rs)**
   - Replace direct `list()` calls in `list_cluster_instances()` (line ~189):
     ```rust
     // OLD:
     let instances = instances_api.list(&list_params).await?;

     // NEW:
     let instances = list_all_paginated(&instances_api, list_params).await?;
     ```

4. **Update Pod Listing (bind9instance.rs)**
   - Replace direct `list()` calls in `update_status_from_deployment()` (line ~799):
     ```rust
     // OLD:
     let pods = pod_api.list(&list_params).await?;

     // NEW:
     let pods = list_all_paginated(&pod_api, list_params).await?;
     ```

5. **Add Pagination Constant**
   - Add to `src/constants.rs`:
     ```rust
     /// Page size for Kubernetes API list operations
     /// Balances memory usage vs. number of API calls
     pub const KUBE_LIST_PAGE_SIZE: u32 = 100;
     ```

6. **Testing**
   - Unit tests: Verify pagination logic with mock API
   - Integration tests: Create 250+ DNSZones, verify all discovered
   - Performance tests: Measure memory usage with/without pagination

7. **Documentation**
   - Document pagination behavior in architecture docs
   - Add performance tuning section about page size

**Acceptance Criteria:**
- All list operations use pagination helper
- Page size configurable via constant
- Memory usage remains constant regardless of resource count
- Integration tests pass with 1000+ resources per namespace

**Estimated Effort:** 2-3 days

---

### Phase 3: Exponential Backoff for Retries (Medium Priority)

**Goal:** Add retry logic with exponential backoff for transient API errors.

**Tasks:**

1. **Add Dependency**
   - Add to `Cargo.toml`:
     ```toml
     [dependencies]
     backoff = { version = "0.4", features = ["tokio"] }
     ```

2. **Create Retry Helper Function**
   - Create `src/reconcilers/retry.rs`:
     ```rust
     use backoff::{ExponentialBackoff, Error as BackoffError};
     use std::time::Duration;

     /// Retry configuration for Kubernetes API calls
     pub fn default_backoff() -> ExponentialBackoff {
         ExponentialBackoff {
             initial_interval: Duration::from_millis(100),
             max_interval: Duration::from_secs(30),
             max_elapsed_time: Some(Duration::from_secs(300)), // 5 minutes total
             multiplier: 2.0,
             randomization_factor: 0.1,
             ..Default::default()
         }
     }

     /// Retry a Kubernetes API call with exponential backoff
     ///
     /// Retries on transient errors (429, 5xx), fails immediately on client errors (4xx)
     pub async fn retry_api_call<T, F, Fut>(f: F) -> Result<T>
     where
         F: FnMut() -> Fut,
         Fut: Future<Output = Result<T, kube::Error>>,
     {
         backoff::future::retry(default_backoff(), || async {
             f().await.map_err(|e| {
                 if is_retryable_error(&e) {
                     warn!("Retryable Kubernetes API error: {}", e);
                     BackoffError::transient(anyhow::Error::from(e))
                 } else {
                     error!("Non-retryable Kubernetes API error: {}", e);
                     BackoffError::permanent(anyhow::Error::from(e))
                 }
             })
         }).await
     }

     /// Determine if a Kubernetes error is retryable
     fn is_retryable_error(err: &kube::Error) -> bool {
         match err {
             kube::Error::Api(api_err) => {
                 // Retry on rate limiting and server errors
                 api_err.code == 429 || (api_err.code >= 500 && api_err.code < 600)
             }
             kube::Error::Service(_) => true,  // Network errors
             _ => false,  // Client errors, not retryable
         }
     }
     ```

3. **Apply Retry Logic to Critical Paths**
   - Identify critical API calls in reconcilers:
     - Cluster/provider fetching (bind9instance.rs:305-350)
     - Zone status updates (dnszone.rs)
     - Instance status updates (bind9instance.rs)
   - Wrap with retry helper:
     ```rust
     // Example: Fetching cluster with retry
     let cluster = retry_api_call(|| cluster_api.get(&cluster_ref)).await?;
     ```

4. **Add Retry Metrics**
   - Track retry attempts in Prometheus:
     ```rust
     pub fn record_api_retry(resource_kind: &str, attempt: u32) {
         // Increment retry counter
     }
     ```

5. **Testing**
   - Unit tests: Mock API with intermittent 429 errors, verify retry
   - Integration tests: Simulate API server pressure, verify recovery
   - Chaos tests: Inject random errors, ensure controllers stay healthy

6. **Documentation**
   - Document retry behavior in architecture docs
   - Add troubleshooting guide for transient errors

**Acceptance Criteria:**
- Critical API calls automatically retry on transient errors
- Exponential backoff prevents API server overload
- Metrics track retry attempts and success rates
- Chaos tests pass with 10% random error injection

**Estimated Effort:** 3-4 days

---

### Phase 4: API Client Metrics (Medium Priority)

**Goal:** Add Prometheus metrics for API client performance and rate limiting.

**Tasks:**

1. **Define Metrics**
   - Add to `src/metrics.rs`:
     ```rust
     // API client request rate
     lazy_static! {
         pub static ref KUBE_API_REQUESTS_TOTAL: IntCounterVec = register_int_counter_vec!(
             "bindy_kube_api_requests_total",
             "Total Kubernetes API requests by resource and operation",
             &["resource", "operation", "status"]
         ).unwrap();

         pub static ref KUBE_API_REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
             "bindy_kube_api_request_duration_seconds",
             "Kubernetes API request duration in seconds",
             &["resource", "operation"]
         ).unwrap();

         pub static ref KUBE_API_RATE_LIMIT_HITS_TOTAL: IntCounterVec = register_int_counter_vec!(
             "bindy_kube_api_rate_limit_hits_total",
             "Total number of 429 rate limit responses",
             &["resource", "operation"]
         ).unwrap();

         pub static ref KUBE_API_RETRIES_TOTAL: IntCounterVec = register_int_counter_vec!(
             "bindy_kube_api_retries_total",
             "Total number of API call retries",
             &["resource", "reason"]
         ).unwrap();

         pub static ref KUBE_API_PAGINATION_PAGES_TOTAL: HistogramVec = register_histogram_vec!(
             "bindy_kube_api_pagination_pages_total",
             "Number of pages fetched during list operations",
             &["resource"]
         ).unwrap();
     }
     ```

2. **Instrument API Calls**
   - Wrap API calls with metric recording:
     ```rust
     pub async fn instrumented_get<K>(
         api: &Api<K>,
         name: &str,
         resource_kind: &str,
     ) -> Result<K>
     where
         K: kube::Resource<DynamicType = ()> + Clone + DeserializeOwned,
     {
         let start = std::time::Instant::now();
         let result = api.get(name).await;
         let duration = start.elapsed();

         let status = if result.is_ok() { "success" } else { "error" };
         KUBE_API_REQUESTS_TOTAL
             .with_label_values(&[resource_kind, "get", status])
             .inc();

         KUBE_API_REQUEST_DURATION_SECONDS
             .with_label_values(&[resource_kind, "get"])
             .observe(duration.as_secs_f64());

         if let Err(ref e) = result {
             if let kube::Error::Api(api_err) = e {
                 if api_err.code == 429 {
                     KUBE_API_RATE_LIMIT_HITS_TOTAL
                         .with_label_values(&[resource_kind, "get"])
                         .inc();
                 }
             }
         }

         result.map_err(Into::into)
     }
     ```

3. **Update Pagination Helper**
   - Track page count in pagination:
     ```rust
     pub async fn list_all_paginated<K>(
         api: &Api<K>,
         mut list_params: ListParams,
         resource_kind: &str,
     ) -> Result<Vec<K>>
     {
         let mut page_count = 0;

         loop {
             let result = instrumented_list(api, &list_params, resource_kind).await?;
             page_count += 1;
             all_items.extend(result.items);

             if result.metadata.continue_.is_none() {
                 break;
             }
         }

         KUBE_API_PAGINATION_PAGES_TOTAL
             .with_label_values(&[resource_kind])
             .observe(page_count as f64);

         Ok(all_items)
     }
     ```

4. **Create Grafana Dashboard**
   - Design dashboard showing:
     - API request rate by resource type
     - Request latency percentiles (p50, p95, p99)
     - Rate limit hit rate
     - Retry count and success rate
     - Pagination page count distribution

5. **Add Alerting Rules**
   - Create Prometheus alert rules:
     ```yaml
     - alert: BindyHighAPIRateLimitHits
       expr: rate(bindy_kube_api_rate_limit_hits_total[5m]) > 0.1
       for: 5m
       annotations:
         summary: "Bindy hitting Kubernetes API rate limits"
         description: "{{ $value }} rate limit hits per second"

     - alert: BindyHighAPIRetryRate
       expr: rate(bindy_kube_api_retries_total[5m]) > 1.0
       for: 10m
       annotations:
         summary: "Bindy experiencing high API retry rate"
         description: "{{ $value }} retries per second"
     ```

6. **Documentation**
   - Document all metrics in `docs/src/metrics.md`
   - Add Grafana dashboard JSON to repository
   - Document alerting rules and response procedures

**Acceptance Criteria:**
- All API calls tracked in Prometheus metrics
- Grafana dashboard visualizes API client performance
- Alert rules fire on rate limit violations
- Metrics exported via `/metrics` endpoint

**Estimated Effort:** 2-3 days

---

### Phase 5: Load Testing and Validation (High Priority)

**Goal:** Validate that rate limiting improvements handle large-scale deployments.

**Tasks:**

1. **Create Load Test Suite**
   - Create `tests/load/mod.rs`:
     ```rust
     /// Load test: 1000 DNSZones in single namespace
     #[tokio::test]
     #[ignore] // Run manually with --ignored
     async fn test_large_zone_discovery() {
         let client = Client::try_default().await.unwrap();

         // Create 1000 DNSZones with varying labels
         for i in 0..1000 {
             create_test_zone(&client, &format!("zone-{}", i), i % 10).await;
         }

         // Create Bind9Instance with zonesFrom selector
         let instance = create_instance_with_zones_from(&client).await;

         // Wait for reconciliation
         tokio::time::sleep(Duration::from_secs(60)).await;

         // Verify all matching zones discovered
         let instance = get_instance(&client, &instance.name_any()).await;
         assert!(instance.status.unwrap().selected_zones.len() > 0);

         // Check metrics: no rate limit hits
         let metrics = fetch_metrics().await;
         assert_eq!(metrics.rate_limit_hits, 0);
     }
     ```

2. **Create Scale Test Scenarios**
   - Test scenarios:
     - 1000 DNSZones, 1 Bind9Instance (zone discovery)
     - 100 Bind9Instances, 10 DNSZones each (multi-instance)
     - 5 Bind9Clusters with 20 instances each (cluster scale)
     - 10 controller replicas (multi-replica coordination)

3. **Measure Performance**
   - Metrics to collect:
     - Reconciliation time (p50, p95, p99)
     - API requests per second
     - Rate limit hits (should be 0)
     - Memory usage (controller pods)
     - CPU usage (controller pods)

4. **Compare Before/After**
   - Run load tests on:
     - `main` branch (baseline)
     - Feature branch (with rate limiting improvements)
   - Document performance improvements:
     - Reduced 429 errors
     - Lower memory usage (pagination)
     - Faster recovery from transient errors (retry)

5. **Chaos Testing**
   - Inject failures during load tests:
     - Random API 429 errors (10% rate)
     - Random API 5xx errors (5% rate)
     - Random network delays (100-500ms)
   - Verify controllers remain healthy and eventually consistent

6. **Documentation**
   - Create `docs/src/performance-testing.md`
   - Document load test procedures
   - Publish performance benchmarks
   - Add tuning guide for different cluster sizes

**Acceptance Criteria:**
- Load tests pass with 1000+ resources per namespace
- Zero 429 errors under normal load
- Controllers remain healthy under chaos conditions
- Performance benchmarks documented

**Estimated Effort:** 3-5 days

---

## Success Metrics

### Performance Metrics

1. **API Rate Limiting**
   - Zero 429 (rate limit) errors under normal load
   - < 0.01 rate limit hits per second under peak load
   - QPS stays below configured limit (20 req/sec default)

2. **Scalability**
   - Support 1000+ DNSZones per namespace
   - Support 100+ Bind9Instances per namespace
   - Support 10+ controller replicas (with leader election)

3. **Reliability**
   - 99.9% reconciliation success rate
   - < 5% retry rate on API calls
   - Recovery within 30 seconds of transient errors

4. **Resource Efficiency**
   - Memory usage O(1) with respect to resource count (pagination)
   - CPU usage proportional to reconciliation rate (not resource count)

### Observability Metrics

1. **Metrics Coverage**
   - All API operations tracked in Prometheus
   - Rate limit hits, retries, and pagination visible
   - Dashboard shows real-time API client health

2. **Alerting**
   - Alerts fire on rate limit violations
   - Alerts fire on high retry rates
   - Runbooks document response procedures

## Risk Mitigation

### Risk: Breaking Changes in kube-rs

**Mitigation:**
- Pin kube-rs version until rate limiting changes tested
- Review kube-rs changelog before upgrading
- Add integration tests covering client configuration

### Risk: Performance Regression

**Mitigation:**
- Run load tests before and after changes
- Compare metrics (reconciliation time, API requests)
- Rollback plan: feature flag for new rate limiting code

### Risk: Pagination Breaks Zone Discovery

**Mitigation:**
- Comprehensive unit tests for pagination logic
- Integration tests with 1000+ zones
- Verify all zones discovered (not just first page)

### Risk: Retry Logic Causes Cascading Failures

**Mitigation:**
- Max retry duration (5 minutes total)
- Exponential backoff prevents thundering herd
- Chaos testing validates recovery behavior

## Dependencies

### External Dependencies

1. **kube-rs >= 0.87**
   - Rate limiting configuration API
   - Pagination support in list operations
   - Current version: Check `Cargo.toml`

2. **backoff crate**
   - Exponential backoff implementation
   - Tokio async support
   - Version: 0.4

### Internal Dependencies

1. **Metrics Infrastructure**
   - Prometheus metrics endpoint (`/metrics`)
   - Existing metrics definitions in `src/metrics.rs`

2. **Integration Test Framework**
   - Kind cluster setup
   - Test resource creation helpers
   - Current tests in `tests/integration/`

## Timeline

### Optimistic (12-14 days)

- Week 1:
  - Phase 1: Explicit Client Rate Limiting (2 days)
  - Phase 2: Pagination (3 days)
- Week 2:
  - Phase 3: Exponential Backoff (4 days)
  - Phase 4: API Client Metrics (3 days)
- Week 3:
  - Phase 5: Load Testing (5 days)

### Realistic (18-22 days)

- Week 1-2:
  - Phase 1: Explicit Client Rate Limiting (3 days)
  - Phase 2: Pagination (4 days)
- Week 2-3:
  - Phase 3: Exponential Backoff (5 days)
- Week 3-4:
  - Phase 4: API Client Metrics (4 days)
  - Phase 5: Load Testing (5 days)

### Pessimistic (25-30 days)

- Includes time for:
  - Unexpected kube-rs API changes
  - Performance regressions requiring redesign
  - Complex debugging of race conditions
  - Multiple iterations on load testing

## Future Considerations

### Beyond This Roadmap

1. **Client-Side Caching**
   - Cache frequently accessed resources (clusters, zones)
   - Reduce API calls for read-heavy workloads
   - Invalidate cache on watch events

2. **Batch Operations**
   - Batch status updates (multiple zones at once)
   - Reduce number of API calls
   - Improves efficiency for bulk operations

3. **Dynamic Rate Limit Tuning**
   - Detect API server pressure (429 errors)
   - Automatically reduce QPS
   - Gradually increase when pressure subsides

4. **Multi-Cluster Support**
   - Different rate limits per cluster
   - Aggregate metrics across clusters
   - Cross-cluster resource discovery

## References

- [Kubernetes API Conventions - Rate Limiting](https://kubernetes.io/docs/reference/using-api/api-concepts/#server-side-rate-limiting)
- [kube-rs Client Configuration](https://docs.rs/kube/latest/kube/struct.Config.html)
- [kubectl Default Rate Limits](https://kubernetes.io/docs/reference/command-line-tools-reference/kube-apiserver/)
- [Exponential Backoff Best Practices](https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/)

## Changelog

- **2026-01-01**: Initial roadmap created (Erick Bourgeois)
