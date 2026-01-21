# Performance

Optimize Bindy DNS infrastructure for maximum performance and efficiency.

## Performance Metrics

Key metrics to monitor:
- **Query latency** - Time to respond to DNS queries
- **Throughput** - Queries per second (QPS)
- **Resource usage** - CPU and memory utilization
- **Cache hit ratio** - Percentage of cached responses
- **Reconciliation loops** - Unnecessary status updates

## Operator Performance

### Status Update Optimization

The Bindy operator implements status change detection in all reconcilers to prevent tight reconciliation loops. This optimization:

- **Reduces Kubernetes API calls** by skipping unnecessary status updates
- **Prevents reconciliation storms** that can occur when status updates trigger new reconciliations
- **Improves overall system performance** by reducing CPU and network overhead

All reconcilers check if the status has actually changed before updating the status subresource. Status updates only occur when:
- Condition type changes
- Status value changes
- Message changes
- Status doesn't exist yet

This optimization is implemented across all resource types:
- Bind9Cluster
- Bind9Instance
- DNSZone
- All DNS record types (A, AAAA, CNAME, MX, NS, SRV, TXT, CAA)

For more details, see the [Reconciliation Logic](../development/reconciliation.md#status-update-optimization) documentation.

## Optimization Strategies

### 1. Resource Allocation

Provide adequate CPU and memory:

```yaml
spec:
  resources:
    requests:
      cpu: "500m"
      memory: "512Mi"
    limits:
      cpu: "2000m"
      memory: "2Gi"
```

### 2. Horizontal Scaling

Add more replicas for higher capacity:

```yaml
spec:
  replicas: 5  # More replicas = more capacity
```

### 3. Geographic Distribution

Place DNS servers near clients:
- Reduced network latency
- Better user experience
- Regional load distribution

### 4. Caching Strategy

Configure BIND9 caching (when appropriate):
- Longer TTLs reduce upstream queries
- Negative caching for NXDOMAIN
- Prefetching for popular domains

## Performance Testing

### Baseline Testing

```bash
# Single query latency
time dig @$SERVICE_IP example.com

# Sustained load (100 QPS for 60 seconds)
dnsp erf -s $SERVICE_IP -d example.com -q 100 -t 60
```

### Load Testing

```bash
# Using dnsperf
dnsperf -s $SERVICE_IP -d queries.txt -l 60 -Q 1000

# Using custom script
for i in {1..1000}; do
  dig @$SERVICE_IP test$i.example.com &
done
wait
```

## Resource Optimization

### CPU Optimization

- Use efficient query algorithms
- Enable query parallelization
- Optimize zone file format

### Memory Optimization

- Right-size zone cache
- Limit journal size
- Regular zone file cleanup

### Network Optimization

- Use UDP for queries (TCP for transfers)
- Enable TCP Fast Open
- Optimize MTU size

## Monitoring Performance

```bash
# Real-time resource usage
kubectl top pods -n dns-system -l app=bind9

# Query statistics
kubectl exec -n dns-system deployment/primary-dns -- \
  rndc stats

# View statistics file
kubectl exec -n dns-system deployment/primary-dns -- \
  cat /var/cache/bind/named.stats
```

## Performance Targets

| Metric | Target | Good | Excellent |
|--------|--------|------|-----------|
| Query Latency | < 50ms | < 20ms | < 10ms |
| Throughput | > 1000 QPS | > 5000 QPS | > 10000 QPS |
| CPU Usage | < 70% | < 50% | < 30% |
| Memory Usage | < 80% | < 60% | < 40% |
| Cache Hit Ratio | > 60% | > 80% | > 90% |

## Next Steps

- [Tuning](./tuning.md) - Detailed tuning parameters
- [Benchmarking](./benchmarking.md) - Performance testing methodology
