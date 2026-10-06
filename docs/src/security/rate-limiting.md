# Rate Limiting Implementation Plan (M-3)

**Status:** 🔶 Partially implemented — layer 2 (Kubernetes API client QPS/burst, pagination, retry) shipped via [ADR-0005](https://github.com/firestoned/bindy/blob/main/docs/adr/0005-client-side-kube-api-rate-limiting.md); other layers planned
**Compliance:** Basel III Availability, Operational Resilience
**Effort:** 1-2 weeks
**Priority:** MEDIUM

---

## Overview

This document provides a comprehensive implementation plan for rate limiting in the Bindy operator to prevent runaway reconciliation loops, API server overload, and BIND9 server exhaustion.

**Problem:** Without rate limiting, the operator can:
- Overwhelm the Kubernetes API server with excessive requests
- Flood BIND9 servers with RNDC commands during cascading failures
- Exhaust pod CPU/memory during reconciliation storms
- Cause cluster-wide performance degradation

**Solution:** Implement multi-layer rate limiting at:
1. **Reconciliation Loop Level** - Limit reconciliation frequency per resource
2. **Kubernetes API Client Level** - Set QPS/burst limits for API calls
3. **RNDC Client Level** - Implement circuit breakers for BIND9 communication
4. **Pod Resource Level** - CPU/memory throttling via resource limits

---

## 1. Reconciliation Loop Rate Limiting

### Current Behavior

The operator currently uses `kube-rs` default reconciliation behavior:
- Reconcile immediately on resource changes (watch events)
- Requeue on errors with exponential backoff (1s, 2s, 4s, 8s, ...)
- No global rate limit across all resources

**Problem:**
- Before ADR-0016: 1,000 DNS zones × reconcile every 5 minutes = 3.3 reconciliations/second
  (the periodic resync is gone: at rest there are no reconciles at all)
- If all zones fail simultaneously → 1,000 immediate retries → API server overload

### Proposed Solution

```rust
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use std::num::NonZeroU32;

// Global rate limiter for all reconciliations
// Allow 10 reconciliations per second with bursts up to 50
lazy_static! {
    static ref RECONCILE_RATE_LIMITER: RateLimiter<
        governor::state::direct::NotKeyed,
        governor::state::InMemoryState,
        governor::clock::DefaultClock
    > = RateLimiter::direct(Quota::per_second(nonzero!(10u32)));
}

pub async fn reconcile(zone: Arc<DNSZone>, ctx: Arc<Context>) -> Result<Action, ReconcileError> {
    // Wait for rate limiter before proceeding
    RECONCILE_RATE_LIMITER.until_ready().await;

    // ... existing reconciliation logic ...
}
```

**Benefits:**
- Prevents reconciliation storms (max 10/sec globally)
- Bursts allowed for normal operations (up to 50)
- Protects Kubernetes API and BIND9 servers
- Automatic backpressure (reconciliations queue up)

**Configuration:**

Add to `ConfigMap` (`bindy-config`):
```yaml
data:
  # Reconciliation rate limiting
  reconcile-rate-limit-per-second: "10"
  reconcile-rate-limit-burst: "50"
```

---

## 2. Kubernetes API Client Rate Limiting

### Status: ✅ Implemented ([ADR-0005](https://github.com/firestoned/bindy/blob/main/docs/adr/0005-client-side-kube-api-rate-limiting.md))

Unlike client-go, `kube-rs` has no QPS/burst fields on `Config`; its extension
point is the tower middleware stack. `crates/bindy-controller-sdk/src/rate_limit.rs` builds the operator's
client through `kube::client::ClientBuilder` with two layers:

- `tower::limit::RateLimitLayer` — allows `burst` requests per `burst / qps`
  seconds (a windowed approximation of client-go's token bucket, sustained
  20 QPS with bursts of 30 by default). Requests over budget queue on the
  client; they are not rejected.
- A metrics middleware that counts every request, times it, and counts HTTP
  429 responses, so server-side throttling is visible in Prometheus
  (`bindy_firestoned_io_kube_api_*` metrics).

Paginated LIST operations (`reconcilers/pagination.rs`, 100 items/page) and
exponential-backoff retries for transient 429/5xx errors
(`reconcilers/retry.rs`) round out the client-side controls.

Every non-watch request also carries a client-side deadline (ADR-0014,
`BINDY_KUBE_REQUEST_TIMEOUT_SECS`, default 30 s) covering the response
headers and body. A request stuck on a stalled connection fails with a
retryable error after the deadline and is retried with backoff, instead of
holding a reconcile for minutes. Watch streams are exempt; they are bounded
by the server-side `timeoutSeconds` and the watcher's idle timeout. Time spent
queued behind the rate limiter does not count against the deadline.

**Configuration** (environment variables on the operator Deployment):

```yaml
env:
  - name: BINDY_KUBE_QPS
    value: "20.0"
  - name: BINDY_KUBE_BURST
    value: "30"
```

Invalid or non-positive values fall back to the compiled defaults with a
warning — a misconfigured limiter never disables the operator or the limit.

**Tuning Guidelines:**

| Cluster Size | DNS Zones | Recommended QPS | Recommended Burst |
|--------------|-----------|-----------------|-------------------|
| Small (< 100 zones) | < 100 | 10 | 20 |
| Medium (100-1000 zones) | 100-1000 | 50 | 100 |
| Large (1000-5000 zones) | 1000-5000 | 100 | 200 |
| Extra Large (> 5000 zones) | > 5000 | 200 | 400 |

**Monitoring:**

Add Prometheus metrics for API client throttling:
```rust
// Increment when rate limited
api_client_throttled_total.inc();

// Track wait time
api_client_throttle_wait_seconds.observe(wait_duration.as_secs_f64());
```

---

## 3. RNDC Circuit Breaker

### Current Behavior

The operator makes RNDC calls to BIND9 servers without circuit breakers:
- Retries failed RNDC calls indefinitely
- No timeout between retries
- Can overwhelm a failing BIND9 server

**Problem:**
- BIND9 server crashes → 1,000 zones retry RNDC → server never recovers
- Cascading failures across all BIND9 instances

### Proposed Solution

Implement circuit breaker pattern using `tokio-retry`:

```rust
use tokio_retry::strategy::{ExponentialBackoff, jitter};
use tokio_retry::Retry;
use std::time::Duration;

const MAX_RNDC_RETRIES: usize = 3;
const RNDC_INITIAL_BACKOFF_MS: u64 = 100;
const RNDC_MAX_BACKOFF_MS: u64 = 5000;

pub async fn execute_rndc_with_circuit_breaker(
    cmd: &str,
    server: &str
) -> Result<String, RndcError> {
    // Exponential backoff: 100ms, 200ms, 400ms, 800ms, 1600ms (max 5s)
    let retry_strategy = ExponentialBackoff::from_millis(RNDC_INITIAL_BACKOFF_MS)
        .max_delay(Duration::from_millis(RNDC_MAX_BACKOFF_MS))
        .map(jitter)
        .take(MAX_RNDC_RETRIES);

    // Retry with circuit breaker
    let result = Retry::spawn(retry_strategy, || async {
        execute_rndc(cmd, server).await
    }).await;

    match result {
        Ok(output) => Ok(output),
        Err(e) => {
            // Circuit breaker: mark server as unhealthy
            mark_server_unhealthy(server).await;

            Err(RndcError::CircuitOpen {
                server: server.to_string(),
                last_error: e.to_string(),
            })
        }
    }
}

// Track server health
static SERVER_HEALTH: Lazy<DashMap<String, ServerHealth>> = Lazy::new(DashMap::new);

#[derive(Clone)]
struct ServerHealth {
    failures: usize,
    last_failure: Instant,
    circuit_open: bool,
}

async fn mark_server_unhealthy(server: &str) {
    let mut health = SERVER_HEALTH.entry(server.to_string())
        .or_insert(ServerHealth {
            failures: 0,
            last_failure: Instant::now(),
            circuit_open: false,
        });

    health.failures += 1;
    health.last_failure = Instant::now();

    // Open circuit breaker if 5 failures in 1 minute
    if health.failures >= 5 {
        health.circuit_open = true;
        warn!("Circuit breaker OPEN for server {}", server);

        // Close circuit after 60 seconds
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if let Some(mut health) = SERVER_HEALTH.get_mut(server) {
                health.circuit_open = false;
                health.failures = 0;
                info!("Circuit breaker CLOSED for server {}", server);
            }
        });
    }
}
```

**Benefits:**
- Prevents RNDC retry storms
- Failing servers get 60-second cool-down period
- Automatic recovery when server comes back
- Protects BIND9 from operator-induced overload

---

## 4. Pod Resource Limits (CPU/Memory Throttling)

### Current Configuration

```yaml
resources:
  limits:
    cpu: 500m
    memory: 512Mi
  requests:
    cpu: 100m
    memory: 128Mi
```

**Analysis:**
- **CPU Limit (500m):** 0.5 CPU cores - reasonable for most workloads
- **Memory Limit (512Mi):** May be too low for large clusters (1000+ zones)
- **No runtime throttling:** OOMKilled if memory exceeded

### Proposed Configuration

```yaml
resources:
  limits:
    cpu: 1000m      # Increase to 1 CPU core (runaway reconciliation protection)
    memory: 1Gi      # Increase to 1GB (large cluster support)
  requests:
    cpu: 200m       # Increase to 0.2 CPU cores (better scheduling)
    memory: 256Mi    # Increase to 256MB (avoid OOMKill on startup)
```

**Tuning Guidelines:**

| Cluster Size | DNS Zones | CPU Request | CPU Limit | Memory Request | Memory Limit |
|--------------|-----------|-------------|-----------|----------------|--------------|
| Small (< 100 zones) | < 100 | 100m | 500m | 128Mi | 512Mi |
| Medium (100-1000 zones) | 100-1000 | 200m | 1000m | 256Mi | 1Gi |
| Large (1000-5000 zones) | 1000-5000 | 500m | 2000m | 512Mi | 2Gi |
| Extra Large (> 5000 zones) | > 5000 | 1000m | 4000m | 1Gi | 4Gi |

**Monitoring:**

Add Prometheus alerts for resource exhaustion:
```yaml
- alert: OperatorHighCPUUsage
  expr: rate(container_cpu_usage_seconds_total{pod=~"bindy-.*"}[5m]) > 0.8
  for: 10m
  labels:
    severity: warning
  annotations:
    summary: "Operator CPU usage > 80% for 10 minutes"

- alert: OperatorHighMemoryUsage
  expr: container_memory_working_set_bytes{pod=~"bindy-.*"} / container_spec_memory_limit_bytes{pod=~"bindy-.*"} > 0.8
  for: 10m
  labels:
    severity: warning
  annotations:
    summary: "Operator memory usage > 80% for 10 minutes"
```

---

## 5. Runaway Reconciliation Detection

### Proposed Monitoring

Add Prometheus metrics to detect reconciliation loops:

```rust
use prometheus::{Counter, Histogram, IntGauge};

lazy_static! {
    // Total reconciliations (by resource type and result)
    static ref RECONCILE_TOTAL: CounterVec = register_counter_vec!(
        "bindy_reconcile_total",
        "Total number of reconciliations",
        &["resource_type", "result"]  // result: success, error, requeue
    ).unwrap();

    // Reconciliation duration
    static ref RECONCILE_DURATION: HistogramVec = register_histogram_vec!(
        "bindy_reconcile_duration_seconds",
        "Reconciliation duration in seconds",
        &["resource_type"]
    ).unwrap();

    // Reconciliations in progress
    static ref RECONCILE_IN_PROGRESS: IntGaugeVec = register_int_gauge_vec!(
        "bindy_reconcile_in_progress",
        "Number of reconciliations currently in progress",
        &["resource_type"]
    ).unwrap();

    // Requeue rate (indicator of issues)
    static ref RECONCILE_REQUEUE_RATE: CounterVec = register_counter_vec!(
        "bindy_reconcile_requeue_rate",
        "Rate of reconciliation requeues (errors or pending work)",
        &["resource_type", "reason"]  // reason: error, pending, rate_limit
    ).unwrap();
}

pub async fn reconcile(zone: Arc<DNSZone>, ctx: Arc<Context>) -> Result<Action, ReconcileError> {
    let _in_progress = RECONCILE_IN_PROGRESS.with_label_values(&["dnszone"]).guard();
    let timer = RECONCILE_DURATION.with_label_values(&["dnszone"]).start_timer();

    let result = reconcile_inner(zone, ctx).await;

    match &result {
        Ok(Action::Requeue(duration)) => {
            RECONCILE_TOTAL.with_label_values(&["dnszone", "requeue"]).inc();
            RECONCILE_REQUEUE_RATE.with_label_values(&["dnszone", "pending"]).inc();
        },
        Ok(Action::None) => {
            RECONCILE_TOTAL.with_label_values(&["dnszone", "success"]).inc();
        },
        Err(e) => {
            RECONCILE_TOTAL.with_label_values(&["dnszone", "error"]).inc();
            RECONCILE_REQUEUE_RATE.with_label_values(&["dnszone", "error"]).inc();
        },
    }

    timer.observe_duration();
    result
}
```

**Prometheus Alerts for Runaway Reconciliation:**

```yaml
- alert: RunawayReconciliation
  expr: rate(bindy_reconcile_requeue_rate{reason="error"}[5m]) > 10
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "Runaway reconciliation loop detected"
    description: "{{ $value }} reconciliation errors per second (> 10/sec threshold)"
    runbook_url: "../operations/error-handling.md"
```

---

## Implementation Checklist

### Phase 1: Rate Limiting (Week 1)

- [ ] Add `governor` crate dependency to `Cargo.toml`
- [ ] Implement global reconciliation rate limiter (10/sec)
- [ ] Add ConfigMap keys for rate limit configuration
- [x] Update Kubernetes API client with QPS/burst limits (ADR-0005; env vars, not ConfigMap)
- [ ] Add Prometheus metrics for rate limiting
- [ ] Test with 1,000 DNS zones (load testing)
- [ ] Document rate limit tuning guidelines

### Phase 2: Circuit Breakers (Week 1)

- [ ] Add `tokio-retry` crate dependency to `Cargo.toml`
- [ ] Implement RNDC circuit breaker with exponential backoff
- [ ] Track server health (failures, last_failure, circuit_open)
- [ ] Add circuit breaker open/close logging
- [ ] Test circuit breaker with failing BIND9 server
- [ ] Add Prometheus metrics for circuit breaker state

### Phase 3: Resource Tuning (Week 2)

- [ ] Update deployment.yaml with increased resource limits
- [ ] Add resource tuning guidelines to documentation
- [ ] Create Prometheus alerts for resource exhaustion
- [ ] Test with various cluster sizes (100, 1000, 5000 zones)
- [ ] Document memory/CPU usage per DNS zone

### Phase 4: Monitoring & Alerting (Week 2)

- [ ] Add reconciliation metrics (total, duration, in_progress, requeue_rate)
- [ ] Create Grafana dashboard for reconciliation health
- [ ] Add runaway reconciliation alert
- [ ] Create runbook for runaway reconciliation incidents
- [ ] Document troubleshooting procedures

---

## Testing Plan

### 1. Load Testing (1,000 DNS Zones)

```bash
# Create 1,000 DNS zones
for i in {1..1000}; do
  kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: zone-$i
  namespace: default
spec:
  zoneName: "zone-$i.example.com"
  clusterRef:
    name: test-cluster
EOF
done

# Monitor reconciliation rate
kubectl top pod -n bindy-system
watch -n 1 'kubectl get events -n bindy-system --sort-by=.lastTimestamp | tail -20'

# Check Prometheus metrics
curl -s localhost:8080/metrics | grep bindy_reconcile
```

**Expected Results:**
- Reconciliation rate ≤ 10/sec (rate limiter working)
- CPU usage < 80% of limit
- Memory usage < 80% of limit
- No OOMKilled pods

---

### 2. Circuit Breaker Testing (Failing BIND9 Server)

```bash
# Kill BIND9 pod
kubectl delete pod -n bindy-system bind9-primary-0

# Watch circuit breaker logs
kubectl logs -n bindy-system -l app=bindy --follow | grep -i circuit

# Expected logs:
# WARN Circuit breaker OPEN for server bind9-primary-0.bind9-primary:9530
# INFO Circuit breaker CLOSED for server bind9-primary-0.bind9-primary:9530 (after 60s)
```

---

## See Also

- [Compliance Roadmap - M-3](../compliance/overview.md)
- [Operational Resilience (Basel III)](../compliance/basel-iii.md#principle-5-cyber-resilience-and-response)
- [Troubleshooting](../operations/troubleshooting.md)
- [Performance Tuning](../advanced/tuning.md)
