# Logging

Configure and analyze logs from the Bindy controller and BIND9 instances.

## Controller Logging

### Log Levels

Set log level via RUST_LOG environment variable:

```yaml
env:
  - name: RUST_LOG
    value: "info"  # error, warn, info, debug, trace
```

### Viewing Controller Logs

```bash
# View recent logs
kubectl logs -n dns-system deployment/bindy --tail=100

# Follow logs in real-time
kubectl logs -n dns-system deployment/bindy -f

# Filter by log level
kubectl logs -n dns-system deployment/bindy | grep ERROR

# Search for specific resource
kubectl logs -n dns-system deployment/bindy | grep "example-com"
```

## BIND9 Instance Logging

### Viewing BIND9 Logs

```bash
# Logs from all BIND9 pods
kubectl logs -n dns-system -l app=bind9

# Logs from specific instance
kubectl logs -n dns-system -l instance=primary-dns

# Follow logs
kubectl logs -n dns-system -l instance=primary-dns -f --tail=50
```

### Common Log Messages

**Successful Zone Load:**
```
zone example.com/IN: loaded serial 2024010101
```

**Zone Transfer:**
```
transfer of 'example.com/IN' from 10.0.1.10#53: Transfer completed
```

**Query Logging (if enabled):**
```
client @0x7f... 192.0.2.1#53210: query: www.example.com IN A
```

## Log Aggregation

### Using Fluentd/Fluent Bit

Collect logs to centralized logging:

```yaml
# Example Fluent Bit DaemonSet configuration
# Automatically collects pod logs
```

### Using Loki

Store and query logs with Grafana Loki:

```bash
# Query logs for DNS zone
{namespace="dns-system", app="bind9"} |= "example.com"

# Query for errors
{namespace="dns-system"} |= "ERROR"
```

## Structured Logging

Controller logs use structured format:

```json
{
  "timestamp": "2024-11-26T10:00:00Z",
  "level": "INFO",
  "target": "bindy::reconcilers::dnszone",
  "message": "Reconciling DNSZone: dns-system/example-com"
}
```

## Log Retention

Configure log retention based on your needs:

- **Development**: 7 days
- **Production**: 30-90 days  
- **Compliance**: As required by regulations

## Troubleshooting with Logs

### Find Failed Reconciliations

```bash
kubectl logs -n dns-system deployment/bindy | grep "ERROR\|Failed"
```

### Track Zone Transfer Issues

```bash
kubectl logs -n dns-system -l dns-role=secondary | grep "transfer"
```

### Monitor Resource Creation

```bash
kubectl logs -n dns-system deployment/bindy | grep "Creating\|Updating"
```

## Best Practices

1. **Use appropriate log levels** - info for production, debug for troubleshooting
2. **Centralize logs** - Use log aggregation for easier analysis
3. **Set up log rotation** - Prevent disk space issues
4. **Create alerts** - Alert on ERROR level logs
5. **Regular review** - Periodically review logs for issues
