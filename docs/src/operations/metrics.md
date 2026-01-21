# Metrics

Monitor performance and health metrics for Bindy DNS infrastructure.

## Operator Metrics

Bindy exposes Prometheus-compatible metrics on port 8080 at `/metrics`. These metrics provide comprehensive observability into the operator's behavior and resource management.

### Accessing Metrics

The metrics endpoint is exposed on all operator pods:

```bash
# Port forward to the operator
kubectl port-forward -n dns-system deployment/bindy-operator 8080:8080

# View metrics
curl http://localhost:8080/metrics
```

### Available Metrics

All metrics use the namespace prefix `bindy_firestoned_io_`.

#### Reconciliation Metrics

**`bindy_firestoned_io_reconciliations_total`** (Counter)
Total number of reconciliation attempts by resource type and outcome.

Labels:
- `resource_type`: Kind of resource (`Bind9Cluster`, `Bind9Instance`, `DNSZone`, `ARecord`, `AAAARecord`, `TXTRecord`, `CNAMERecord`, `MXRecord`, `NSRecord`, `SRVRecord`, `CAARecord`)
- `status`: Outcome (`success`, `error`, `requeue`)

```promql
# Reconciliation success rate
rate(bindy_firestoned_io_reconciliations_total{status="success"}[5m])

# Error rate by resource type
rate(bindy_firestoned_io_reconciliations_total{status="error"}[5m])
```

**`bindy_firestoned_io_reconciliation_duration_seconds`** (Histogram)
Duration of reconciliation operations in seconds.

Labels:
- `resource_type`: Kind of resource

Buckets: 0.001, 0.01, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0

```promql
# Average reconciliation duration
rate(bindy_firestoned_io_reconciliation_duration_seconds_sum[5m])
/ rate(bindy_firestoned_io_reconciliation_duration_seconds_count[5m])

# 95th percentile latency
histogram_quantile(0.95, bindy_firestoned_io_reconciliation_duration_seconds_bucket)
```

**`bindy_firestoned_io_requeues_total`** (Counter)
Total number of requeue operations.

Labels:
- `resource_type`: Kind of resource
- `reason`: Reason for requeue (`error`, `rate_limit`, `dependency_wait`)

```promql
# Requeue rate by reason
rate(bindy_firestoned_io_requeues_total[5m])
```

#### Resource Lifecycle Metrics

**`bindy_firestoned_io_resources_created_total`** (Counter)
Total number of resources created.

Labels:
- `resource_type`: Kind of resource

**`bindy_firestoned_io_resources_updated_total`** (Counter)
Total number of resources updated.

Labels:
- `resource_type`: Kind of resource

**`bindy_firestoned_io_resources_deleted_total`** (Counter)
Total number of resources deleted.

Labels:
- `resource_type`: Kind of resource

**`bindy_firestoned_io_resources_active`** (Gauge)
Currently active resources being tracked.

Labels:
- `resource_type`: Kind of resource

```promql
# Resource creation rate
rate(bindy_firestoned_io_resources_created_total[5m])

# Active resources by type
bindy_firestoned_io_resources_active
```

#### Error Metrics

**`bindy_firestoned_io_errors_total`** (Counter)
Total number of errors by resource type and category.

Labels:
- `resource_type`: Kind of resource
- `error_type`: Category (`api_error`, `validation_error`, `network_error`, `timeout`, `reconcile_error`)

```promql
# Error rate by type
rate(bindy_firestoned_io_errors_total[5m])

# Errors by resource type
sum(rate(bindy_firestoned_io_errors_total[5m])) by (resource_type)
```

#### Leader Election Metrics

**`bindy_firestoned_io_leader_elections_total`** (Counter)
Total number of leader election events.

Labels:
- `status`: Event type (`acquired`, `lost`, `renewed`)

**`bindy_firestoned_io_leader_status`** (Gauge)
Current leader election status (1 = leader, 0 = follower).

Labels:
- `pod_name`: Name of the pod

```promql
# Current leader
bindy_firestoned_io_leader_status == 1

# Leader election rate
rate(bindy_firestoned_io_leader_elections_total[5m])
```

#### Performance Metrics

**`bindy_firestoned_io_generation_observation_lag_seconds`** (Histogram)
Lag between resource spec generation change and operator observation.

Labels:
- `resource_type`: Kind of resource

Buckets: 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0

```promql
# Average observation lag
rate(bindy_firestoned_io_generation_observation_lag_seconds_sum[5m])
/ rate(bindy_firestoned_io_generation_observation_lag_seconds_count[5m])
```

### Prometheus Configuration

The operator deployment includes Prometheus scrape annotations:

```yaml
annotations:
  prometheus.io/scrape: "true"
  prometheus.io/port: "8080"
  prometheus.io/path: "/metrics"
```

Prometheus will automatically discover and scrape these metrics if configured with Kubernetes service discovery.

### Example Queries

```promql
# Reconciliation success rate (last 5 minutes)
sum(rate(bindy_firestoned_io_reconciliations_total{status="success"}[5m]))
/ sum(rate(bindy_firestoned_io_reconciliations_total[5m]))

# DNSZone reconciliation p95 latency
histogram_quantile(0.95,
  sum(rate(bindy_firestoned_io_reconciliation_duration_seconds_bucket{resource_type="DNSZone"}[5m])) by (le)
)

# Error rate by resource type (last hour)
topk(10,
  sum(rate(bindy_firestoned_io_errors_total[1h])) by (resource_type)
)

# Active resources per type
sum(bindy_firestoned_io_resources_active) by (resource_type)

# Requeue backlog
sum(rate(bindy_firestoned_io_requeues_total[5m])) by (resource_type, reason)
```

### Grafana Dashboard

Import the Bindy operator dashboard (coming soon) or create custom panels using the queries above.

Recommended panels:
1. **Reconciliation Rate** - Total reconciliations/sec by resource type
2. **Reconciliation Latency** - P50, P95, P99 latencies
3. **Error Rate** - Errors/sec by resource type and error category
4. **Active Resources** - Gauge showing current active resources
5. **Leader Status** - Current leader pod and election events
6. **Resource Lifecycle** - Created/Updated/Deleted rates

## Resource Metrics

### Pod Metrics

View CPU and memory usage:

```bash
# All DNS pods
kubectl top pods -n dns-system

# Specific instance
kubectl top pods -n dns-system -l instance=primary-dns

# Sort by CPU
kubectl top pods -n dns-system --sort-by=cpu

# Sort by memory
kubectl top pods -n dns-system --sort-by=memory
```

### Node Metrics

```bash
# Node resource usage
kubectl top nodes

# Detailed node info
kubectl describe node <node-name>
```

## DNS Query Metrics

### Using BIND9 Statistics

Enable BIND9 statistics channel (future enhancement):

```yaml
spec:
  config:
    statisticsChannels:
      - address: "127.0.0.1"
        port: 8053
```

### Query Counters

Monitor query rate and types:
- Total queries received
- Queries by record type (A, AAAA, MX, etc.)
- Successful vs failed queries
- NXDOMAIN responses

## Performance Metrics

### Query Latency

Measure DNS query response time:

```bash
# Test query latency
time dig @<dns-server-ip> example.com

# Multiple queries for average
for i in {1..10}; do time dig @<dns-server-ip> example.com +short; done
```

### Zone Transfer Metrics

Monitor zone transfer performance:
- Transfer duration
- Transfer size
- Transfer failures
- Lag between primary and secondary

## Kubernetes Metrics

### Resource Utilization

```yaml
# View resource requests vs limits
kubectl describe pod -n dns-system <pod-name> | grep -A5 "Limits:\|Requests:"
```

### Pod Health

```yaml
# Pod status and restarts
kubectl get pods -n dns-system -o wide

# Events
kubectl get events -n dns-system --sort-by='.lastTimestamp'
```

## Prometheus Integration

### BIND9 Exporter

Deploy bind_exporter as sidecar (future enhancement):

```yaml
containers:
- name: bind-exporter
  image: prometheuscommunity/bind-exporter:latest
  args:
    - "--bind.stats-url=http://localhost:8053"
  ports:
    - name: metrics
      containerPort: 9119
```

### Service Monitor

```yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: bindy-metrics
spec:
  selector:
    matchLabels:
      app: bind9
  endpoints:
  - port: metrics
    interval: 30s
```

## Key Metrics to Monitor

1. **Query Rate** - Queries per second
2. **Query Latency** - Response time
3. **Error Rate** - Failed queries percentage
4. **Cache Hit Ratio** - Cache effectiveness
5. **Zone Transfer Status** - Success/failure of transfers
6. **Resource Usage** - CPU and memory utilization
7. **Pod Health** - Running vs desired replicas

## Grafana Dashboards

Create dashboards for:

### DNS Overview
- Total query rate
- Average latency
- Error rate
- Top queried domains

### Instance Health
- Pod status
- CPU/memory usage
- Restart count
- Network I/O

### Zone Management
- Zones count
- Records per zone
- Zone transfer status
- Serial numbers

## Alerting Thresholds

Recommended alert thresholds:

| Metric | Warning | Critical |
|--------|---------|----------|
| CPU Usage | > 70% | > 90% |
| Memory Usage | > 70% | > 90% |
| Query Latency | > 100ms | > 500ms |
| Error Rate | > 1% | > 5% |
| Pod Restarts | > 3/hour | > 10/hour |

## Best Practices

1. **Baseline metrics** - Establish normal operating ranges
2. **Set appropriate alerts** - Avoid alert fatigue
3. **Monitor trends** - Look for gradual degradation
4. **Capacity planning** - Use metrics to plan scaling
5. **Regular review** - Review dashboards weekly
