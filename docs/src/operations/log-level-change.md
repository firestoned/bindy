# Changing Log Levels at Runtime

This guide explains how to change the operator's log level without modifying code or redeploying the application.

---

## Overview

The Bindy operator's log level is configured via a ConfigMap (`bindy-config`), which allows runtime changes without code modifications. This is especially useful for:

- **Troubleshooting**: Temporarily enable `debug` logging to investigate issues
- **Performance**: Reduce log verbosity in production (`info` or `warn`)
- **Compliance**: Meet PCI-DSS 3.4 requirements (no sensitive data in production logs)

---

## Default Log Levels

| Environment | Log Level | Log Format | Rationale |
|-------------|-----------|------------|-----------|
| **Production** | `info` | `json` | PCI-DSS compliant, structured logging for SIEM |
| **Staging** | `info` | `json` | Production-like logging |
| **Development** | `debug` | `text` | Human-readable, detailed logging |

---

## Changing Log Level

### Method 1: Update ConfigMap (Recommended)

```bash
# Change log level to debug
kubectl patch configmap bindy-config -n dns-system \
  --patch '{"data": {"log-level": "debug"}}'

# Restart operator pods to apply changes
kubectl rollout restart deployment/bindy -n dns-system

# Verify new log level
kubectl logs -n dns-system -l app=bindy --tail=20
```

**Available Log Levels:**
- `error` - Only errors (critical issues)
- `warn` - Warnings and errors
- `info` - Normal operations (default for production)
- `debug` - Detailed reconciliation steps (troubleshooting)
- `trace` - Extremely verbose (rarely needed)

---

### Method 2: Direct Deployment Patch (Temporary)

For temporary debugging without ConfigMap changes:

```bash
# Enable debug logging (overrides ConfigMap)
kubectl set env deployment/bindy RUST_LOG=debug -n dns-system

# Revert to ConfigMap value
kubectl set env deployment/bindy RUST_LOG- -n dns-system
```

**Warning:** This method bypasses the ConfigMap and is lost on next deployment. Use for quick debugging only.

---

## Changing Log Format

```bash
# Change to JSON format (production)
kubectl patch configmap bindy-config -n dns-system \
  --patch '{"data": {"log-format": "json"}}'

# Change to text format (development)
kubectl patch configmap bindy-config -n dns-system \
  --patch '{"data": {"log-format": "text"}}'

# Restart to apply
kubectl rollout restart deployment/bindy -n dns-system
```

**Log Formats:**
- `json` - Structured JSON logs (recommended for production, SIEM integration)
- `text` - Human-readable logs (recommended for development)

---

## Verifying Log Level Changes

```bash
# Check current ConfigMap values
kubectl get configmap bindy-config -n dns-system -o yaml

# Check environment variables in running pod
kubectl exec -n dns-system deployment/bindy -- printenv | grep RUST_LOG

# View recent logs to confirm verbosity
kubectl logs -n dns-system -l app=bindy --tail=100
```

---

## Production Log Level Best Practices

### ✅ DO:
- **Use `info` level in production** - Balances visibility with performance
- **Use `json` format in production** - Enables structured logging and SIEM integration
- **Temporarily enable `debug` for troubleshooting** - Use ConfigMap, document in incident log
- **Revert to `info` after troubleshooting** - Debug logs impact performance

### ❌ DON'T:
- **Leave `debug` enabled in production** - Performance impact, log volume explosion
- **Use `trace` level** - Extremely verbose, only for deep troubleshooting
- **Hardcode log levels in deployment** - Use ConfigMap for runtime changes

---

## Audit Debug Logs for Sensitive Data

Before enabling `debug` logging in production, verify no sensitive data is logged:

```bash
# Audit debug logs for secrets, passwords, keys
kubectl logs -n dns-system -l app=bindy --tail=1000 | \
  grep -iE '(password|secret|key|token|credential)'

# If sensitive data found, fix in code before enabling debug
```

**PCI-DSS 3.4 Requirement:** Mask or remove PAN (Primary Account Number) from all logs.

**Bindy Compliance:** Operator does not handle payment card data directly, but RNDC keys and DNS zone data are considered sensitive.

---

## Troubleshooting Scenarios

### Scenario 1: Operator Not Reconciling Zones

```bash
# Enable debug logging
kubectl patch configmap bindy-config -n dns-system \
  --patch '{"data": {"log-level": "debug"}}'

# Restart operator
kubectl rollout restart deployment/bindy -n dns-system

# Watch logs for reconciliation details
kubectl logs -n dns-system -l app=bindy --follow

# Look for errors in reconciliation loop
kubectl logs -n dns-system -l app=bindy | grep -i error
```

---

### Scenario 2: High Log Volume (Performance Issue)

```bash
# Reduce log level to warn
kubectl patch configmap bindy-config -n dns-system \
  --patch '{"data": {"log-level": "warn"}}'

# Restart operator
kubectl rollout restart deployment/bindy -n dns-system

# Verify reduced log volume
kubectl logs -n dns-system -l app=bindy --tail=100
```

---

### Scenario 3: SIEM Integration (Structured Logging)

```bash
# Ensure JSON format for SIEM
kubectl patch configmap bindy-config -n dns-system \
  --patch '{"data": {"log-format": "json"}}'

# Restart operator
kubectl rollout restart deployment/bindy -n dns-system

# Verify JSON output
kubectl logs -n dns-system -l app=bindy --tail=10 | jq .
```

---

## Log Level Change Procedures (Compliance)

For compliance audits (SOX 404, PCI-DSS), document log level changes:

### Change Request Template

```markdown
# Log Level Change Request

**Date:** 2025-12-18
**Requester:** [Your Name]
**Approver:** [Security Team Lead]
**Environment:** Production

**Current State:**
- Log Level: info
- Log Format: json

**Requested Change:**
- Log Level: debug
- Log Format: json
- Duration: 2 hours (for troubleshooting)

**Justification:**
Investigating slow DNS zone reconciliation (Incident INC-12345)

**Rollback Plan:**
Revert to info level after 2 hours or when issue is resolved

**Approved by:** [Security Team Lead Signature]
```

---

## See Also

- [Logging](./logging.md) - Log configuration and analysis
- [Debugging](./debugging.md) - Troubleshooting guide
- [Environment Variables](./env-vars.md) - All available environment variables
