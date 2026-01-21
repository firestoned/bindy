# Logging

Configure and analyze logs from the Bindy operator and BIND9 instances.

## Operator Logging

### Log Levels

Set log level via RUST_LOG environment variable:

```yaml
env:
  - name: RUST_LOG
    value: "info"  # error, warn, info, debug, trace
```

### Log Format

Set log output format via RUST_LOG_FORMAT environment variable:

```yaml
env:
  - name: RUST_LOG_FORMAT
    value: "json"  # text or json (default: text)
```

**Text format (default):**
- Human-readable compact format
- Ideal for development and local debugging
- Includes timestamps, file locations, and line numbers

**JSON format:**
- Structured JSON output
- Recommended for production Kubernetes deployments
- Easy integration with log aggregation tools (Loki, ELK, Splunk)
- Enables programmatic log parsing and analysis

### Viewing Operator Logs

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

BIND9 instances are configured by default to log to stderr, making logs available through standard Kubernetes logging commands.

### Default Logging Configuration

Bindy automatically configures BIND9 with the following logging channels:

- **stderr_log**: All logs directed to stderr for container-native logging
- **Severity**: Info level by default (configurable)
- **Categories**: Default, queries, security, zone transfers (xfer-in/xfer-out)
- **Format**: Includes timestamps, categories, and severity levels

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

### JSON Format

Enable JSON logging with `RUST_LOG_FORMAT=json`:

```yaml
env:
  - name: RUST_LOG_FORMAT
    value: "json"
```

**Example JSON output:**

```json
{
  "timestamp": "2025-11-30T10:00:00.123456Z",
  "level": "INFO",
  "message": "Reconciling DNSZone: dns-system/example-com",
  "file": "dnszone.rs",
  "line": 142,
  "threadName": "bindy-operator"
}
```

### Text Format

Default human-readable format (RUST_LOG_FORMAT=text or unset):

```
2025-11-30T10:00:00.123456Z dnszone.rs:142 INFO bindy-operator Reconciling DNSZone: dns-system/example-com
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
2. **Use JSON format in production** - Enable structured logging for better integration with log aggregation tools
3. **Use text format for development** - More readable for local debugging and development
4. **Centralize logs** - Use log aggregation for easier analysis
5. **Set up log rotation** - Prevent disk space issues
6. **Create alerts** - Alert on ERROR level logs
7. **Regular review** - Periodically review logs for issues

### Example Production Configuration

```yaml
env:
  - name: RUST_LOG
    value: "info"
  - name: RUST_LOG_FORMAT
    value: "json"
```

### Example Development Configuration

```yaml
env:
  - name: RUST_LOG
    value: "debug"
  - name: RUST_LOG_FORMAT
    value: "text"
```
