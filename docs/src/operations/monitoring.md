# Monitoring

Monitor the health and performance of your Bindy DNS infrastructure.

## Status Conditions

All Bindy resources report their status using standardized conditions:

```bash
# Check Bind9Instance status
kubectl get bind9instance primary-dns -n dns-system -o jsonpath='{.status.conditions}'

# Check DNSZone status
kubectl get dnszone example-com -n dns-system -o jsonpath='{.status.conditions}'
```

See [Status Conditions](./status.md) for detailed condition types.

## Logging

View controller and BIND9 logs:

```bash
# Controller logs
kubectl logs -n dns-system deployment/bindy

# BIND9 instance logs
kubectl logs -n dns-system -l instance=primary-dns

# Follow logs
kubectl logs -n dns-system deployment/bindy -f
```

See [Logging](./logging.md) for log configuration.

## Metrics

Monitor resource usage and performance:

```bash
# Pod resource usage
kubectl top pods -n dns-system

# Node resource usage
kubectl top nodes
```

See [Metrics](./metrics.md) for detailed metrics.

## Health Checks

BIND9 pods include liveness and readiness probes:

```yaml
livenessProbe:
  exec:
    command: ["dig", "@localhost", "version.bind", "txt", "chaos"]
  initialDelaySeconds: 30
  periodSeconds: 10

readinessProbe:
  exec:
    command: ["dig", "@localhost", "version.bind", "txt", "chaos"]
  initialDelaySeconds: 5
  periodSeconds: 5
```

Check probe status:

```bash
kubectl describe pod -n dns-system <bind9-pod-name>
```

## Monitoring Tools

### Prometheus

Scrape metrics from BIND9 using bind_exporter:

```yaml
# Add exporter sidecar to Bind9Instance
# (Future enhancement)
```

### Grafana

Create dashboards for:
- Query rate and latency
- Zone transfer status
- Resource usage
- Error rates

## Alerts

Set up alerts for:
1. Pod crashes or restarts
2. Failed zone transfers
3. High query latency
4. Resource exhaustion
5. DNSSEC validation failures

## Next Steps

- [Status Conditions](./status.md) - Understanding resource status
- [Logging](./logging.md) - Log configuration and analysis
- [Metrics](./metrics.md) - Detailed metrics collection
- [Troubleshooting](./troubleshooting.md) - Debugging issues
