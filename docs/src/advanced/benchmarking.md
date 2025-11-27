# Benchmarking

Measure and analyze DNS performance using industry-standard tools.

## Tools

### dnsperf

Industry-standard DNS benchmarking:

```bash
# Install dnsperf
apt-get install dnsperf

# Create query file
cat > queries.txt <<'QUERIES'
example.com A
www.example.com A
mail.example.com MX
QUERIES

# Run benchmark
dnsperf -s $SERVICE_IP -d queries.txt -l 60 -Q 1000
```

### resperf

Response rate testing:

```bash
# Test maximum QPS
resperf -s $SERVICE_IP -d queries.txt -m 10000
```

### dig

Simple latency testing:

```bash
# Measure query time
dig @$SERVICE_IP example.com | grep "Query time"

# Multiple queries for average
for i in {1..100}; do
  dig @$SERVICE_IP example.com +stats | grep "Query time"
done | awk '{sum+=$4; count++} END {print "Average:", sum/count, "ms"}'
```

## Benchmark Scenarios

### Scenario 1: Baseline Performance

Single client, sequential queries:

```bash
dnsperf -s $SERVICE_IP -d queries.txt -l 60 -Q 100
```

**Expected:** < 10ms latency, > 90% success

### Scenario 2: Load Test

Multiple clients, high QPS:

```bash
dnsperf -s $SERVICE_IP -d queries.txt -l 300 -Q 5000 -c 50
```

**Expected:** < 50ms latency under load

### Scenario 3: Stress Test

Maximum capacity test:

```bash
resperf -s $SERVICE_IP -d queries.txt -m 50000
```

**Expected:** Find maximum QPS before degradation

## Metrics to Collect

### Response Time

- Minimum latency
- Average latency  
- 95th percentile
- 99th percentile
- Maximum latency

### Throughput

- Queries per second
- Successful responses
- Failed queries
- Timeout rate

### Resource Usage

```bash
# During benchmark
kubectl top pods -n dns-system

# CPU and memory trends
kubectl top pods -n dns-system --use-protocol-buffers
```

## Sample Benchmark Report

```
Benchmark: Load Test
Date: 2024-11-26
Duration: 300 seconds
Target QPS: 5000

Results:
- Queries sent: 1,500,000
- Queries completed: 1,498,500
- Success rate: 99.9%
- Average latency: 12.3ms
- 95th percentile: 24.1ms
- 99th percentile: 45.2ms
- Max latency: 89.5ms

Resource Usage:
- Average CPU: 1.2 cores
- Average Memory: 512MB
- Peak CPU: 1.8 cores
- Peak Memory: 768MB
```

## Continuous Benchmarking

### Automated Testing

```yaml
apiVersion: batch/v1
kind: CronJob
metadata:
  name: dns-benchmark
spec:
  schedule: "0 2 * * *"  # Daily at 2 AM
  jobTemplate:
    spec:
      template:
        spec:
          containers:
          - name: dnsperf
            image: dnsperf:latest
            command:
            - /bin/sh
            - -c
            - dnsperf -s primary-dns -d /queries.txt -l 60 >> /results/benchmark.log
```

### Trend Analysis

Track performance over time:
- Daily benchmarks
- Compare before/after changes
- Identify degradation early
- Capacity planning

## Best Practices

1. **Consistent tests** - Same queries, duration
2. **Isolated environment** - Minimize external factors
3. **Multiple runs** - Average results
4. **Document changes** - Link to config changes
5. **Realistic load** - Match production patterns

## Next Steps

- [Performance](./performance.md) - Performance overview
- [Tuning](./tuning.md) - Optimization parameters
