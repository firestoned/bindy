# Error Handling and Retry Logic

Bindy implements robust error handling for DNS record reconciliation, ensuring the operator never crashes when encountering failures. Instead, it updates status conditions, creates Kubernetes Events, and automatically retries with configurable intervals.

## Overview

When reconciling DNS records, several failure scenarios can occur:
- **DNSZone not found**: No matching DNSZone resource exists
- **RNDC key loading fails**: Cannot load the RNDC authentication Secret
- **BIND9 connection fails**: Unable to connect to the BIND9 server
- **Record operation fails**: BIND9 rejects the record operation

Bindy handles all these scenarios gracefully with:
- ✅ Status condition updates following Kubernetes conventions
- ✅ Kubernetes Events for visibility
- ✅ Automatic retry with exponential backoff
- ✅ Configurable retry intervals
- ✅ Idempotent operations safe for multiple retries

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
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zone: example.com  # No DNSZone with zoneName: example.com exists
  name: www
  ipv4Address: 192.0.2.1
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
   apiVersion: bindy.firestoned.io/v1alpha1
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
    message: "Cannot connect to BIND9 server at bind9-primary.dns-system.svc.cluster.local:953: connection refused. Will retry in 30s"
    lastTransitionTime: "2025-11-29T23:45:00Z"
```

**Event:**
```
Type     Reason           Message
Warning  RecordAddFailed  Cannot connect to BIND9 server at bind9-primary.dns-system.svc.cluster.local:953
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
     nc -zv bind9-primary.dns-system.svc.cluster.local 953
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

All BIND9 operations are idempotent, making them safe for controller retries:

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
- Controller can safely requeue failed reconciliations

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
apiVersion: bindy.firestoned.io/v1alpha1
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
  ipv4Address: 192.0.2.1
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
     nc -zv {cluster-name}.dns-system.svc.cluster.local 953
   ```
3. Check BIND9 logs for errors:
   ```bash
   kubectl logs -n dns-system -l app={cluster-name} | grep -i error
   ```
4. Verify RNDC is listening on port 953:
   ```bash
   kubectl exec -n dns-system {bind9-pod} -- ss -tlnp | grep 953
   ```

## See Also

- [Debugging Guide](debugging.md) - Detailed debugging procedures
- [Logging Configuration](logging.md) - Configure operator logging levels
- [Bind9Instance Reference](../reference/bind9instance-spec.md) - BIND9 instance configuration
- [DNSZone Reference](../reference/dnszone-spec.md) - DNS zone configuration
