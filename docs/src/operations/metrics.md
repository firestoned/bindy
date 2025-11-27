# Metrics

Monitor performance and health metrics for Bindy DNS infrastructure.

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
