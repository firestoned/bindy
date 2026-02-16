# Error Handling and Retry Logic

Bindy implements robust error handling for DNS record reconciliation, ensuring the operator never crashes when encountering failures. Instead, it updates status conditions, creates Kubernetes Events, and automatically retries with exponential backoff.

## Overview

When reconciling DNS records, several failure scenarios can occur:
- **DNSZone not found**: No matching DNSZone resource exists
- **RNDC key loading fails**: Cannot load the RNDC authentication Secret
- **BIND9 connection fails**: Unable to connect to the BIND9 server
- **Record operation fails**: BIND9 rejects the record operation
- **HTTP API errors**: Bindcar sidecar temporary failures (503, 502, etc.)
- **Kubernetes API errors**: Rate limiting (429), server errors (5xx), network failures

Bindy handles all these scenarios gracefully with:
- ✅ Status condition updates following Kubernetes conventions
- ✅ Kubernetes Events for visibility
- ✅ **Automatic retry with exponential backoff for HTTP and Kubernetes API calls**
- ✅ Configurable retry intervals for reconciliation loops
- ✅ Idempotent operations safe for multiple retries
- ✅ Smart error classification (retryable vs permanent errors)

## Automatic Retry Infrastructure

Bindy includes automatic retry logic with exponential backoff for transient failures in HTTP and Kubernetes API calls. This ensures resilience to temporary issues without manual intervention.

### HTTP API Retry (Bindcar Operations)

All BIND9 zone management operations (via bindcar HTTP API sidecar) automatically retry on transient failures:

**Retryable HTTP Status Codes:**
- **429** - Too Many Requests (rate limiting)
- **500** - Internal Server Error
- **502** - Bad Gateway (proxy/load balancer issues)
- **503** - Service Unavailable (temporary unavailability)
- **504** - Gateway Timeout
- **Network errors** - Connection failures, DNS resolution errors

**Non-Retryable HTTP Status Codes:**
- **4xx** (except 429) - Client errors (400 Bad Request, 404 Not Found, 401 Unauthorized, etc.)
- These indicate permanent errors that won't be fixed by retrying

**HTTP Retry Configuration:**
- **Initial retry**: 50ms
- **Max interval**: 10 seconds between retries
- **Max duration**: 2 minutes total retry time
- **Backoff multiplier**: 2.0 (exponential growth)
- **Randomization**: ±10% (prevents thundering herd)

**Retry Schedule Example:**
```
Attempt 1: Immediate (0ms)
Attempt 2: 50ms after failure
Attempt 3: ~100ms after failure
Attempt 4: ~200ms after failure
Attempt 5: ~400ms after failure
Attempt 6: ~800ms after failure
Attempt 7: ~1.6s after failure
Attempt 8: ~3.2s after failure
Attempt 9: ~6.4s after failure
Attempt 10+: 10s intervals until 2 minutes elapsed
```

**Affected Operations:**
All bindcar HTTP API operations automatically benefit from retry:
- Zone creation (`create_zone()`)
- Zone updates (`update_zone()`)
- Zone deletion (`delete_zone()`)
- Zone reload (`reload_zone()`)
- Zone listing (`list_zones()`)
- Zone status checks (`get_zone_status()`)
- Primary/secondary zone operations

**Example Log Output:**
```
WARN  Retryable HTTP API error, will retry
      method=POST
      url=http://bind9-primary-api:8080/api/v1/zones
      attempt=3
      retry_after=200ms
      error=HTTP request 'POST http://...' failed with status 503: Service temporarily unavailable

INFO  HTTP API call succeeded after retries
      method=POST
      url=http://bind9-primary-api:8080/api/v1/zones
      attempt=4
      elapsed=850ms
```

### Kubernetes API Retry

Kubernetes API calls will automatically retry on transient failures (implementation planned for reconcilers):

**Retryable Kubernetes Errors:**
- **HTTP 429** - Too Many Requests (rate limiting)
- **HTTP 5xx** - Server errors (API server temporary issues)
- **Network errors** - Connection failures, timeouts

**Non-Retryable Kubernetes Errors:**
- **HTTP 4xx** (except 429) - Client errors (not found, unauthorized, forbidden, etc.)

**Kubernetes Retry Configuration:**
- **Initial retry**: 100ms
- **Max interval**: 30 seconds between retries
- **Max duration**: 5 minutes total retry time
- **Backoff multiplier**: 2.0 (exponential growth)
- **Randomization**: ±10% (prevents thundering herd)

**Why Different from HTTP API:**
- Kubernetes API targets remote cluster (variable network latency)
- API server issues may take longer to resolve
- Longer max interval (30s vs 10s) and total time (5min vs 2min)
- Slightly slower initial retry (100ms vs 50ms)

### Benefits of Automatic Retry

1. **Resilience to Transient Failures**: Temporary network blips, pod restarts, and service unavailability don't cause permanent failures
2. **No Manual Intervention**: Operators don't need to manually requeue or restart failed operations
3. **Smart Error Classification**: Permanent errors (404, 400, etc.) fail immediately without wasting time on retries
4. **Exponential Backoff**: Prevents overwhelming services during recovery while still providing fast retries for quick issues
5. **Thundering Herd Prevention**: Randomization (±10%) prevents all operators retrying simultaneously
6. **Observability**: Structured logging provides visibility into retry attempts, duration, and outcomes

## Configuration

### Retry Interval

Control how long to wait before retrying failed DNS record operations:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: bindy-operator
  namespace: bindy-system
spec:
  template:
    spec:
      containers:
      - name: bindy
        image: ghcr.io/firestoned/bindy:latest
        env:
        - name: BINDY_RECORD_RETRY_SECONDS
          value: "60"  # Default: 30 seconds
```

**Recommendations:**
- **Development**: 10-15 seconds for faster iteration
- **Production**: 30-60 seconds to avoid overwhelming the API server
- **High-load environments**: 60-120 seconds to reduce reconciliation pressure

## Error Scenarios

### 1. DNSZone Not Found

**Scenario:** DNS record references a zone that doesn't exist

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zone: example.com  # No DNSZone with zoneName: example.com exists
  name: www
  ipv4Addresses:
    - "192.0.2.1"
```

**Status:**
```yaml
status:
  conditions:
  - type: Ready
    status: "False"
    reason: ZoneNotFound
    message: "No DNSZone found for zone example.com in namespace dns-system"
    lastTransitionTime: "2025-11-29T23:45:00Z"
  observedGeneration: 1
```

**Event:**
```
Type     Reason         Message
Warning  ZoneNotFound   No DNSZone found for zone example.com in namespace dns-system
```

**Resolution:**
1. Create the DNSZone resource:
   ```yaml
   apiVersion: bindy.firestoned.io/v1beta1
   kind: DNSZone
   metadata:
     name: example-com
     namespace: dns-system
   spec:
     zoneName: example.com
     clusterRef: bind9-primary
   ```
2. Or fix the zone reference in the record if it's a typo

### 2. RNDC Key Load Failed

**Scenario:** Cannot load the RNDC authentication Secret

**Status:**
```yaml
status:
  conditions:
  - type: Ready
    status: "False"
    reason: RndcKeyLoadFailed
    message: "Failed to load RNDC key for cluster bind9-primary: Secret bind9-primary-rndc-key not found"
    lastTransitionTime: "2025-11-29T23:45:00Z"
```

**Event:**
```
Type     Reason              Message
Warning  RndcKeyLoadFailed   Failed to load RNDC key for cluster bind9-primary
```

**Resolution:**
1. Check if the Secret exists:
   ```bash
   kubectl get secret -n dns-system bind9-primary-rndc-key
   ```
2. Verify the Bind9Instance is running and has created its Secret:
   ```bash
   kubectl get bind9instance -n dns-system bind9-primary -o yaml
   ```
3. If missing, the Bind9Instance reconciler should create it automatically

### 3. BIND9 Connection Failed

**Scenario:** Cannot connect to the BIND9 server (network issue, pod not ready, etc.)

**Status:**
```yaml
status:
  conditions:
  - type: Ready
    status: "False"
    reason: RecordAddFailed
    message: "Cannot connect to BIND9 server at bind9-primary.dns-system.svc.cluster.local:9530: connection refused. Will retry in 30s"
    lastTransitionTime: "2025-11-29T23:45:00Z"
```

**Event:**
```
Type     Reason           Message
Warning  RecordAddFailed  Cannot connect to BIND9 server at bind9-primary.dns-system.svc.cluster.local:9530
```

**Resolution:**
1. Check BIND9 pod status:
   ```bash
   kubectl get pods -n dns-system -l app=bind9-primary
   ```
2. Check BIND9 logs:
   ```bash
   kubectl logs -n dns-system -l app=bind9-primary --tail=50
   ```
3. Verify network connectivity:
   ```bash
   kubectl run -it --rm debug --image=nicolaka/netshoot --restart=Never -- \
     nc -zv bind9-primary.dns-system.svc.cluster.local 9530
   ```
4. The operator will automatically retry after the configured interval

### 4. Record Created Successfully

**Scenario:** DNS record successfully created in BIND9

**Status:**
```yaml
status:
  conditions:
  - type: Ready
    status: "True"
    reason: RecordCreated
    message: "A record www.example.com created successfully"
    lastTransitionTime: "2025-11-29T23:45:00Z"
  observedGeneration: 1
```

**Event:**
```
Type    Reason         Message
Normal  RecordCreated  A record www.example.com created successfully
```

## Monitoring

### View Record Status

```bash
# List all DNS records with status
kubectl get arecords,aaaarecords,cnamerecords,mxrecords,txtrecords -A

# Check specific record status
kubectl get arecord www-example -n dns-system -o jsonpath='{.status.conditions[0]}' | jq .

# Find failing records
kubectl get arecords -A -o json | \
  jq -r '.items[] | select(.status.conditions[0].status == "False") |
  "\(.metadata.namespace)/\(.metadata.name): \(.status.conditions[0].reason) - \(.status.conditions[0].message)"'
```

### View Events

```bash
# Recent events in namespace
kubectl get events -n dns-system --sort-by='.lastTimestamp' | tail -20

# Watch events in real-time
kubectl get events -n dns-system --watch

# Filter for DNS record events
kubectl get events -n dns-system --field-selector involvedObject.kind=ARecord
```

### Prometheus Metrics

Bindy exposes reconciliation metrics (if enabled):

```promql
# Reconciliation errors by reason
bindy_reconcile_errors_total{resource="ARecord", reason="ZoneNotFound"}

# Reconciliation duration
histogram_quantile(0.95, bindy_reconcile_duration_seconds_bucket{resource="ARecord"})
```

## Status Reason Codes

| Reason | Status | Meaning | Action Required |
|--------|--------|---------|-----------------|
| `RecordCreated` | `Ready=True` | DNS record successfully created in BIND9 | None - record is operational |
| `ZoneNotFound` | `Ready=False` | No matching DNSZone resource exists | Create DNSZone or fix zone reference |
| `RndcKeyLoadFailed` | `Ready=False` | Cannot load RNDC key Secret | Verify Bind9Instance is running and Secret exists |
| `RecordAddFailed` | `Ready=False` | Failed to communicate with BIND9 or add record | Check BIND9 pod status and network connectivity |

## Idempotent Operations

All BIND9 operations are idempotent, making them safe for operator retries:

### add_zones / add_primary_zone / add_secondary_zone
- **add_zones**: Centralized dispatcher that routes to `add_primary_zone` or `add_secondary_zone` based on zone type
- **add_primary_zone**: Checks if zone exists before attempting to add primary zone
- **add_secondary_zone**: Checks if zone exists before attempting to add secondary zone
- All functions return success if zone already exists
- Safe to call multiple times (idempotent)

### reload_zone
- Returns clear error if zone doesn't exist
- Otherwise performs reload operation
- Safe to call multiple times

### Record Operations
- All record add/update operations are idempotent
- Retrying a failed operation won't create duplicates
- Operator can safely requeue failed reconciliations

## Best Practices

### 1. Monitor Status Conditions

Always check status conditions when debugging DNS record issues:

```bash
kubectl describe arecord www-example -n dns-system
```

Look for the `Status` section showing current conditions.

### 2. Use Events for Troubleshooting

Events provide a timeline of what happened:

```bash
kubectl get events -n dns-system --field-selector involvedObject.name=www-example
```

### 3. Adjust Retry Interval for Your Needs

- **Fast feedback during development**: `BINDY_RECORD_RETRY_SECONDS=10`
- **Production stability**: `BINDY_RECORD_RETRY_SECONDS=60`
- **High-load clusters**: `BINDY_RECORD_RETRY_SECONDS=120`

### 4. Create DNSZones Before Records

To avoid `ZoneNotFound` errors, always create DNSZone resources before creating DNS records:

```bash
# 1. Create DNSZone
kubectl apply -f dnszone.yaml

# 2. Wait for it to be ready
kubectl wait --for=condition=Ready dnszone/example-com -n dns-system --timeout=60s

# 3. Create DNS records
kubectl apply -f records/
```

### 5. Use Labels for Organization

Tag related resources for easier monitoring:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    app: web-frontend
    environment: production
spec:
  zone: example.com
  name: www
  ipv4Addresses:
    - "192.0.2.1"
```

Then filter:
```bash
kubectl get arecords -n dns-system -l environment=production
```

## Troubleshooting Guide

### Record Stuck in "ZoneNotFound"

1. Verify DNSZone exists:
   ```bash
   kubectl get dnszones -A
   ```
2. Check zone name matches:
   ```bash
   kubectl get dnszone example-com -n dns-system -o jsonpath='{.spec.zoneName}'
   ```
3. Ensure they're in the same namespace

### Record Stuck in "RndcKeyLoadFailed"

1. Check Secret exists:
   ```bash
   kubectl get secret -n dns-system {cluster-name}-rndc-key
   ```
2. Verify Bind9Instance is Ready:
   ```bash
   kubectl get bind9instance -n dns-system
   ```
3. Check Bind9Instance logs:
   ```bash
   kubectl logs -n bindy-system -l app=bindy-operator
   ```

### Record Stuck in "RecordAddFailed"

1. Check BIND9 pod is running:
   ```bash
   kubectl get pods -n dns-system -l app={cluster-name}
   ```
2. Test network connectivity:
   ```bash
   kubectl run -it --rm debug --image=nicolaka/netshoot -- \
     nc -zv {cluster-name}.dns-system.svc.cluster.local 9530
   ```
3. Check BIND9 logs for errors:
   ```bash
   kubectl logs -n dns-system -l app={cluster-name} | grep -i error
   ```
4. Verify RNDC is listening on port 9530:
   ```bash
   kubectl exec -n dns-system {bind9-pod} -- ss -tlnp | grep 9530
   ```

## See Also

- [Debugging Guide](debugging.md) - Detailed debugging procedures
- [Logging Configuration](logging.md) - Configure operator logging levels
- [Bind9Instance Reference](../reference/bind9instance-spec.md) - BIND9 instance configuration
- [DNSZone Reference](../reference/dnszone-spec.md) - DNS zone configuration
