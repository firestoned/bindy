# Secret Access Audit Trail

**Status:** ✅ Implemented
**Compliance:** SOX 404 (Access Controls), PCI-DSS 7.1.2 (Least Privilege), Basel III (Cyber Risk)
**Last Updated:** 2025-12-18
**Owner:** Security Team

---

## Table of Contents

1. [Overview](#overview)
2. [Secret Access Monitoring](#secret-access-monitoring)
3. [Audit Policy Configuration](#audit-policy-configuration)
4. [Audit Queries](#audit-queries)
5. [Alerting Rules](#alerting-rules)
6. [Compliance Requirements](#compliance-requirements)
7. [Incident Response](#incident-response)

---

## Overview

This document describes Bindy's secret access audit trail implementation, which provides:

- **Comprehensive Logging**: All secret access (get, list, watch) is logged via Kubernetes audit logs
- **Immutable Storage**: Audit logs stored in S3 with WORM (Object Lock) for tamper-proof retention
- **Real-Time Alerting**: Prometheus/Alertmanager alerts on anomalous secret access patterns
- **Compliance Queries**: Pre-built queries for SOX 404, PCI-DSS, and Basel III audit reviews
- **Retention**: 7-year retention (SOX 404 requirement) with 90-day active storage (Elasticsearch)

### Secrets Covered

Bindy audit logging covers all Kubernetes Secrets in the `dns-system` namespace:

| Secret Name | Purpose | Access Pattern |
|-------------|---------|----------------|
| `rndc-key-*` | RNDC authentication keys for BIND9 control | Operator reads on reconciliation (every 5 minutes) |
| `tls-cert-*` | TLS certificates for DNS-over-TLS/HTTPS | BIND9 pods read on startup |
| Custom secrets | User-defined secrets for DNS credentials | Varies by use case |

### Compliance Mapping

| Framework | Requirement | How We Comply |
|-----------|-------------|---------------|
| **SOX 404** | IT General Controls - Access Control | Audit logs show who accessed secrets and when (7-year retention) |
| **PCI-DSS 7.1.2** | Restrict access to privileged user IDs | RBAC limits secret access to operator (read-only) + audit trail |
| **PCI-DSS 10.2.1** | Audit log all access to cardholder data | Secret access logged with user, timestamp, action, outcome |
| **Basel III** | Cyber Risk - Access Monitoring | Real-time alerting on anomalous secret access, quarterly reviews |

---

## Secret Access Monitoring

### What is Logged

Every secret access operation generates an audit log entry with:

```json
{
  "kind": "Event",
  "apiVersion": "audit.k8s.io/v1",
  "level": "Metadata",
  "auditID": "a4b5c6d7-e8f9-0a1b-2c3d-4e5f6a7b8c9d",
  "stage": "ResponseComplete",
  "requestURI": "/api/v1/namespaces/dns-system/secrets/rndc-key-primary",
  "verb": "get",
  "user": {
    "username": "system:serviceaccount:dns-system:bindy-operator",
    "uid": "abc123",
    "groups": ["system:serviceaccounts", "system:serviceaccounts:dns-system"]
  },
  "sourceIPs": ["10.244.1.15"],
  "userAgent": "bindy/v0.1.0 (linux/amd64) kubernetes/abc123",
  "objectRef": {
    "resource": "secrets",
    "namespace": "dns-system",
    "name": "rndc-key-primary",
    "apiVersion": "v1"
  },
  "responseStatus": {
    "code": 200
  },
  "requestReceivedTimestamp": "2025-12-18T12:34:56.789Z",
  "stageTimestamp": "2025-12-18T12:34:56.790Z"
}
```

### Key Fields for Auditing

| Field | Description | Audit Use Case |
|-------|-------------|----------------|
| `user.username` | ServiceAccount or user who accessed the secret | **Who** accessed the secret |
| `sourceIPs` | Pod IP or client IP that made the request | **Where** the request came from |
| `objectRef.name` | Secret name (e.g., `rndc-key-primary`) | **What** secret was accessed |
| `verb` | Action performed (`get`, `list`, `watch`) | **How** the secret was accessed |
| `responseStatus.code` | HTTP status code (200 = success, 403 = denied) | **Outcome** of the access attempt |
| `requestReceivedTimestamp` | When the request was made | **When** the access occurred |
| `userAgent` | Client application (e.g., `bindy/v0.1.0`) | **Which** application accessed the secret |

---

## Audit Policy Configuration

### Kubernetes Audit Policy

The audit policy is configured in `/etc/kubernetes/audit-policy.yaml` on the Kubernetes control plane.

**Relevant Section for Secret Access (H-3 Requirement):**

```yaml
apiVersion: audit.k8s.io/v1
kind: Policy
metadata:
  name: bindy-secret-access-audit
rules:
  # ============================================================================
  # H-3: Secret Access Audit Trail
  # ============================================================================

  # Log ALL secret access in dns-system namespace (read operations)
  - level: Metadata
    verbs: ["get", "list", "watch"]
    resources:
      - group: ""
        resources: ["secrets"]
    namespaces: ["dns-system"]
    omitStages:
      - "RequestReceived"  # Only log after response is sent

  # Log ALL secret modifications (should be DENIED by RBAC, but log anyway)
  - level: RequestResponse
    verbs: ["create", "update", "patch", "delete"]
    resources:
      - group: ""
        resources: ["secrets"]
    namespaces: ["dns-system"]
    omitStages:
      - "RequestReceived"

  # Log secret access failures (403 Forbidden)
  # This catches unauthorized access attempts
  - level: Metadata
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
    resources:
      - group: ""
        resources: ["secrets"]
    namespaces: ["dns-system"]
    omitStages:
      - "RequestReceived"
```

### Audit Log Rotation

Audit logs are rotated and forwarded using Fluent Bit:

```yaml
# /etc/fluent-bit/fluent-bit.conf
[INPUT]
    Name              tail
    Path              /var/log/kubernetes/audit.log
    Parser            json
    Tag               kube.audit
    Refresh_Interval  5
    Mem_Buf_Limit     50MB
    Skip_Long_Lines   On

[FILTER]
    Name    grep
    Match   kube.audit
    Regex   objectRef.resource secrets

[OUTPUT]
    Name                s3
    Match               kube.audit
    bucket              bindy-audit-logs
    region              us-east-1
    store_dir           /var/log/fluent-bit/s3
    total_file_size     100M
    upload_timeout      10m
    use_put_object      On
    s3_key_format       /audit/secrets/%Y/%m/%d/$UUID.json.gz
    compression         gzip
```

**Key Points:**
- Audit logs filtered to only include secret access (`objectRef.resource secrets`)
- Uploaded to S3 in `/audit/secrets/` prefix for easy querying
- Compressed with gzip (10:1 compression ratio)
- WORM protection via S3 Object Lock (see [audit-log-retention.md](audit-log-retention.md))

---

## Audit Queries

### Pre-Built Queries for Compliance Reviews

These queries are designed for use in Elasticsearch (Kibana) or direct S3 queries (Athena).

#### Q1: All Secret Access by ServiceAccount (Last 90 Days)

**Use Case:** SOX 404 quarterly access review

**Elasticsearch Query:**

```json
{
  "query": {
    "bool": {
      "must": [
        { "term": { "objectRef.resource": "secrets" } },
        { "term": { "objectRef.namespace": "dns-system" } },
        { "range": { "requestReceivedTimestamp": { "gte": "now-90d" } } }
      ]
    }
  },
  "aggs": {
    "by_service_account": {
      "terms": {
        "field": "user.username.keyword",
        "size": 50
      },
      "aggs": {
        "by_secret": {
          "terms": {
            "field": "objectRef.name.keyword",
            "size": 20
          },
          "aggs": {
            "access_count": {
              "value_count": {
                "field": "auditID"
              }
            }
          }
        }
      }
    }
  },
  "size": 0
}
```

**Expected Output:**

```json
{
  "aggregations": {
    "by_service_account": {
      "buckets": [
        {
          "key": "system:serviceaccount:dns-system:bindy-operator",
          "doc_count": 25920,
          "by_secret": {
            "buckets": [
              {
                "key": "rndc-key-primary",
                "doc_count": 12960,
                "access_count": { "value": 12960 }
              },
              {
                "key": "rndc-key-secondary-1",
                "doc_count": 6480,
                "access_count": { "value": 6480 }
              }
            ]
          }
        }
      ]
    }
  }
}
```

**Interpretation:**
- Operator accessed `rndc-key-primary` 12,960 times in 90 days
- Expected: ~144 times/day (reconciliation every 10 minutes = 6 times/hour × 24 hours)
- 12,960 / 90 days = 144 accesses/day ✅ **NORMAL**

---

#### Q2: Secret Access by Non-Operator ServiceAccounts

**Use Case:** Detect unauthorized secret access (should be ZERO)

**Elasticsearch Query:**

```json
{
  "query": {
    "bool": {
      "must": [
        { "term": { "objectRef.resource": "secrets" } },
        { "term": { "objectRef.namespace": "dns-system" } }
      ],
      "must_not": [
        { "term": { "user.username.keyword": "system:serviceaccount:dns-system:bindy-operator" } }
      ]
    }
  },
  "sort": [
    { "requestReceivedTimestamp": { "order": "desc" } }
  ],
  "size": 100
}
```

**Expected Output:** `0 hits` (only operator should access secrets)

**If non-zero:** 🚨 **ALERT** - Unauthorized secret access detected, trigger incident response (see [incident-response.md](incident-response.md#p4-rndc-key-compromise))

---

#### Q3: Failed Secret Access Attempts (403 Forbidden)

**Use Case:** Detect brute-force attacks or misconfigurations

**Elasticsearch Query:**

```json
{
  "query": {
    "bool": {
      "must": [
        { "term": { "objectRef.resource": "secrets" } },
        { "term": { "objectRef.namespace": "dns-system" } },
        { "term": { "responseStatus.code": 403 } }
      ]
    }
  },
  "aggs": {
    "by_user": {
      "terms": {
        "field": "user.username.keyword",
        "size": 50
      },
      "aggs": {
        "by_secret": {
          "terms": {
            "field": "objectRef.name.keyword",
            "size": 20
          }
        }
      }
    }
  },
  "sort": [
    { "requestReceivedTimestamp": { "order": "desc" } }
  ],
  "size": 100
}
```

**Expected Output:** Low volume (< 10/day) for misconfigured pods or during upgrades

**If high volume (> 100/day):** 🚨 **ALERT** - Potential brute-force attack, investigate source IPs

---

#### Q4: Secret Access Outside Business Hours

**Use Case:** Detect after-hours access (potential insider threat)

**Elasticsearch Query:**

```json
{
  "query": {
    "bool": {
      "must": [
        { "term": { "objectRef.resource": "secrets" } },
        { "term": { "objectRef.namespace": "dns-system" } }
      ],
      "should": [
        {
          "range": {
            "requestReceivedTimestamp": {
              "gte": "now/d",
              "lte": "now/d+8h",
              "time_zone": "America/New_York"
            }
          }
        },
        {
          "range": {
            "requestReceivedTimestamp": {
              "gte": "now/d+18h",
              "lte": "now/d+24h",
              "time_zone": "America/New_York"
            }
          }
        }
      ],
      "minimum_should_match": 1
    }
  },
  "aggs": {
    "by_hour": {
      "date_histogram": {
        "field": "requestReceivedTimestamp",
        "calendar_interval": "hour",
        "time_zone": "America/New_York"
      }
    }
  },
  "size": 100
}
```

**Expected Output:** Consistent volume (automated reconciliation runs 24/7)

**Anomalies:**
- Sudden spike in after-hours access → 🚨 Investigate source IPs and ServiceAccounts
- Human users accessing secrets after hours → 🚨 Verify with change management records

---

#### Q5: Specific Secret Access History (e.g., `rndc-key-primary`)

**Use Case:** Compliance audit - "Show me all access to RNDC key in Q4 2025"

**Elasticsearch Query:**

```json
{
  "query": {
    "bool": {
      "must": [
        { "term": { "objectRef.resource": "secrets" } },
        { "term": { "objectRef.name.keyword": "rndc-key-primary" } },
        { "term": { "objectRef.namespace": "dns-system" } },
        {
          "range": {
            "requestReceivedTimestamp": {
              "gte": "2025-10-01T00:00:00Z",
              "lte": "2025-12-31T23:59:59Z"
            }
          }
        }
      ]
    }
  },
  "aggs": {
    "access_by_day": {
      "date_histogram": {
        "field": "requestReceivedTimestamp",
        "calendar_interval": "day"
      },
      "aggs": {
        "by_service_account": {
          "terms": {
            "field": "user.username.keyword",
            "size": 10
          }
        }
      }
    }
  },
  "sort": [
    { "requestReceivedTimestamp": { "order": "asc" } }
  ],
  "size": 10000
}
```

**Expected Output:** Daily access pattern showing operator accessing key ~144 times/day

**Export for Auditors:**

```bash
# Export to CSV for external auditors
curl -X POST "https://elasticsearch:9200/bindy-audit-*/_search?scroll=5m" \
  -H 'Content-Type: application/json' \
  -d @query-q5.json | \
  jq -r '.hits.hits[]._source | [
    .requestReceivedTimestamp,
    .user.username,
    .objectRef.name,
    .verb,
    .responseStatus.code,
    .sourceIPs[0]
  ] | @csv' > secret-access-q4-2025.csv
```

---

## Alerting Rules

### Prometheus Alerting for Secret Access Anomalies

**Prerequisites:**
- Prometheus configured to scrape audit logs from Elasticsearch
- Alertmanager configured for email/Slack/PagerDuty notifications

**Alert: Unauthorized Secret Access**

```yaml
# /etc/prometheus/rules/bindy-secret-access.yaml
groups:
  - name: bindy_secret_access
    interval: 1m
    rules:
      # CRITICAL: Non-operator ServiceAccount accessed secrets
      - alert: UnauthorizedSecretAccess
        expr: |
          sum(rate(kubernetes_audit_event_total{
            objectRef_resource="secrets",
            objectRef_namespace="dns-system",
            user_username!~"system:serviceaccount:dns-system:bindy-operator"
          }[5m])) > 0
        for: 1m
        labels:
          severity: critical
          compliance: "SOX-404,PCI-DSS-7.1.2"
        annotations:
          summary: "Unauthorized secret access detected in dns-system namespace"
          description: |
            ServiceAccount {{ $labels.user_username }} accessed secret {{ $labels.objectRef_name }}.
            This violates least privilege RBAC policy (only bindy-operator should access secrets).

            Investigate immediately:
            1. Check source IP: {{ $labels.sourceIP }}
            2. Review audit logs for full context
            3. Verify RBAC policy is applied correctly
            4. Follow incident response: docs/security/incident-response.md#p4
          runbook_url: "https://github.com/firestoned/bindy/blob/main/docs/security/incident-response.md#p4-rndc-key-compromise"

      # HIGH: Excessive secret access (potential compromised operator)
      - alert: ExcessiveSecretAccess
        expr: |
          sum(rate(kubernetes_audit_event_total{
            objectRef_resource="secrets",
            objectRef_namespace="dns-system",
            user_username="system:serviceaccount:dns-system:bindy-operator"
          }[5m])) > 10
        for: 10m
        labels:
          severity: warning
          compliance: "SOX-404"
        annotations:
          summary: "Operator accessing secrets at abnormally high rate"
          description: |
            Bindy operator is accessing secrets at {{ $value }}/sec (expected: ~0.5/sec).
            This may indicate:
            - Reconciliation loop bug (rapid retries)
            - Compromised operator pod
            - Performance issue causing excessive reconciliations

            Actions:
            1. Check operator logs for errors
            2. Verify reconciliation requeue times are correct
            3. Check for BIND9 pod restart loops
          runbook_url: "https://github.com/firestoned/bindy/blob/main/docs/troubleshooting.md"

      # MEDIUM: Failed secret access attempts (brute force detection)
      - alert: FailedSecretAccessAttempts
        expr: |
          sum(rate(kubernetes_audit_event_total{
            objectRef_resource="secrets",
            objectRef_namespace="dns-system",
            responseStatus_code="403"
          }[5m])) > 1
        for: 5m
        labels:
          severity: warning
          compliance: "PCI-DSS-10.2.1"
        annotations:
          summary: "Multiple failed secret access attempts detected"
          description: |
            {{ $value }} failed secret access attempts per second.
            This may indicate:
            - Misconfigured pod trying to access secrets without RBAC
            - Attacker probing for secrets
            - RBAC policy change breaking legitimate access

            Actions:
            1. Review audit logs to identify source ServiceAccount/IP
            2. Verify RBAC policy is correct
            3. Check for recent RBAC changes
          runbook_url: "https://github.com/firestoned/bindy/blob/main/docs/security/secret-access-audit.md#q3-failed-secret-access-attempts-403-forbidden"
```

### Alertmanager Routing

```yaml
# /etc/alertmanager/config.yaml
route:
  group_by: ['alertname', 'severity']
  group_wait: 10s
  group_interval: 10s
  repeat_interval: 1h
  receiver: 'security-team'
  routes:
    # CRITICAL alerts go to PagerDuty + Slack
    - match:
        severity: critical
      receiver: 'pagerduty-security'
      continue: true
    - match:
        severity: critical
      receiver: 'slack-security'

receivers:
  - name: 'security-team'
    email_configs:
      - to: 'security@firestoned.io'
        from: 'alertmanager@firestoned.io'
        smarthost: 'smtp.sendgrid.net:587'

  - name: 'pagerduty-security'
    pagerduty_configs:
      - service_key: '<PagerDuty Integration Key>'
        description: '{{ .GroupLabels.alertname }}: {{ .Annotations.summary }}'

  - name: 'slack-security'
    slack_configs:
      - api_url: '<Slack Webhook URL>'
        channel: '#security-alerts'
        title: '🚨 {{ .GroupLabels.alertname }}'
        text: |
          *Severity:* {{ .Labels.severity }}
          *Compliance:* {{ .Labels.compliance }}

          {{ .Annotations.description }}

          *Runbook:* {{ .Annotations.runbook_url }}
```

---

## Compliance Requirements

### SOX 404 - IT General Controls

**Control Objective:** Ensure only authorized users access sensitive secrets

**How We Comply:**

| SOX 404 Requirement | Bindy Implementation | Evidence |
|---------------------|----------------------|----------|
| Access logs for all privileged accounts | ✅ Kubernetes audit logs capture all secret access | Query Q1 (quarterly review) |
| Logs retained for 7 years | ✅ S3 Glacier with WORM (Object Lock) | [audit-log-retention.md](audit-log-retention.md) |
| Quarterly access reviews | ✅ Run Query Q1, review access patterns | Scheduled Kibana report |
| Separation of duties (no single person can access + modify) | ✅ Operator has read-only access (cannot create/update/delete) | RBAC policy verification |

**Quarterly Review Process:**

1. **Week 1 of each quarter (Jan, Apr, Jul, Oct):**
   - Security team runs Query Q1 (All Secret Access by ServiceAccount)
   - Export results to CSV for offline review
   - Verify only `bindy-operator` accessed secrets

2. **Anomaly Investigation:**
   - If non-operator access detected → Run Query Q2, follow incident response
   - If excessive access detected → Run Query Q3, check for reconciliation loop bugs

3. **Document Review:**
   - Create quarterly access review report (template below)
   - File report in `docs/compliance/access-reviews/YYYY-QN.md`
   - Retain for 7 years (SOX requirement)

**Quarterly Review Report Template:**

```markdown
# Secret Access Review - Q4 2025

**Reviewer:** [Name]
**Date:** 2025-12-31
**Period:** 2025-10-01 to 2025-12-31 (90 days)

## Summary
- **Total secret access events:** 25,920
- **ServiceAccounts with access:** 1 (bindy-operator)
- **Secrets accessed:** 2 (rndc-key-primary, rndc-key-secondary-1)
- **Unauthorized access:** 0 ✅
- **Failed access attempts:** 12 (misconfigured test pod)

## Findings
- ✅ **PASS** - Only authorized ServiceAccount (bindy-operator) accessed secrets
- ✅ **PASS** - Access frequency matches expected reconciliation rate (~144/day)
- ⚠️ **MINOR** - 12 failed attempts from test pod (fixed on 2025-11-15)

## Actions
- None required - all access authorized and expected

## Approval
- **Reviewed by:** [Security Manager]
- **Approved by:** [CISO]
- **Date:** 2025-12-31
```

---

### PCI-DSS 7.1.2 - Restrict Access to Privileged User IDs

**Requirement:** Limit access to system components and cardholder data to only those individuals whose job requires such access.

**How We Comply:**

| PCI-DSS Requirement | Bindy Implementation | Evidence |
|---------------------|----------------------|----------|
| Least privilege access | ✅ Only `bindy-operator` ServiceAccount can read secrets | RBAC policy (`deploy/rbac/`) |
| No modify/delete permissions | ✅ Operator CANNOT create/update/patch/delete secrets | RBAC policy verification script |
| Audit trail for all access | ✅ Kubernetes audit logs capture all secret access | Query Q1, Q5 |
| Regular access reviews | ✅ Quarterly reviews using pre-built queries | Quarterly review reports |

**Annual PCI-DSS Audit Evidence:**

Provide auditors with:
1. **RBAC Policy:** `deploy/rbac/clusterrole.yaml` (shows read-only secret access)
2. **RBAC Verification:** `deploy/rbac/verify-rbac.sh` output (proves no modify permissions)
3. **Audit Logs:** Query Q5 results for last 365 days (shows all access)
4. **Quarterly Reviews:** 4 quarterly review reports (proves regular monitoring)

---

### PCI-DSS 10.2.1 - Audit Logs for Access to Cardholder Data

**Requirement:** Implement automated audit trails for all system components to reconstruct events.

**How We Comply:**

| PCI-DSS 10.2.1 Requirement | Bindy Implementation | Evidence |
|----------------------------|----------------------|----------|
| User identification | ✅ Audit logs include `user.username` (ServiceAccount) | Query results show ServiceAccount |
| Type of event | ✅ Audit logs include `verb` (get, list, watch) | Query results show action |
| Date and time | ✅ Audit logs include `requestReceivedTimestamp` (ISO 8601 UTC) | Query results show timestamp |
| Success/failure indication | ✅ Audit logs include `responseStatus.code` (200, 403, etc.) | Query Q3 shows failed attempts |
| Origination of event | ✅ Audit logs include `sourceIPs` (pod IP) | Query results show source IP |
| Identity of affected data | ✅ Audit logs include `objectRef.name` (secret name) | Query results show secret name |

---

### Basel III - Cyber Risk Management

**Principle:** Banks must have robust cyber risk management frameworks including access monitoring and incident response.

**How We Comply:**

| Basel III Requirement | Bindy Implementation | Evidence |
|-----------------------|----------------------|----------|
| Access monitoring | ✅ Real-time Prometheus alerts on unauthorized access | Alerting rules |
| Incident response | ✅ Playbooks for secret compromise (P4) | [incident-response.md](incident-response.md#p4-rndc-key-compromise) |
| Audit trail | ✅ Immutable audit logs (S3 WORM) | [audit-log-retention.md](audit-log-retention.md) |
| Quarterly risk reviews | ✅ Quarterly secret access reviews | Quarterly review reports |

---

## Incident Response

### When to Trigger Incident Response

Trigger **[P4: RNDC Key Compromise](incident-response.md#p4-rndc-key-compromise)** if:

1. **Unauthorized Secret Access** (Query Q2 returns results):
   - Non-operator ServiceAccount accessed secrets
   - Human user accessed secrets via `kubectl get secret`
   - Unknown source IP accessed secrets

2. **Excessive Failed Access Attempts** (Query Q3 returns > 100/day):
   - Potential brute-force attack
   - Attacker probing for secrets

3. **Secret Access Outside Normal Patterns**:
   - Sudden spike in access frequency (Query Q1 shows > 1000/day instead of ~144/day)
   - After-hours access by human users (Query Q4)

### Incident Response Steps (Quick Reference)

See full playbook: **[incident-response.md - P4: RNDC Key Compromise](incident-response.md#p4-rndc-key-compromise)**

1. **Immediate (< 15 minutes):**
   - Rotate compromised secret (`kubectl create secret generic rndc-key-primary --from-literal=key=<new-key> --dry-run=client -o yaml | kubectl replace -f -`)
   - Restart all BIND9 pods to pick up new key
   - Disable compromised ServiceAccount (if applicable)

2. **Containment (< 1 hour):**
   - Review audit logs to identify scope of compromise (Query Q5)
   - Check for unauthorized DNS zone modifications
   - Verify RBAC policy is correct

3. **Eradication (< 4 hours):**
   - Patch vulnerability that allowed unauthorized access
   - Deploy updated RBAC policy if needed
   - Verify no backdoors remain

4. **Recovery (< 8 hours):**
   - Re-enable legitimate ServiceAccounts
   - Verify DNS queries resolve correctly
   - Run Query Q2 to confirm no unauthorized access

5. **Post-Incident (< 1 week):**
   - Document lessons learned
   - Update RBAC policy if needed
   - Add new alerting rules to prevent recurrence

---

## Appendix: Manual Audit Log Inspection

### Extract Audit Logs from S3

```bash
# Download last 7 days of secret access logs
aws s3 sync s3://bindy-audit-logs/audit/secrets/$(date -d '7 days ago' +%Y/%m/%d)/ \
  ./audit-logs/ \
  --exclude "*" \
  --include "*.json.gz"

# Decompress
gunzip ./audit-logs/*.json.gz

# Search for specific secret access
jq 'select(.objectRef.name == "rndc-key-primary")' ./audit-logs/*.json | \
  jq -r '[.requestReceivedTimestamp, .user.username, .verb, .responseStatus.code] | @csv'
```

### Verify Audit Log Integrity (SHA-256 Checksums)

```bash
# Download checksums
aws s3 cp s3://bindy-audit-logs/checksums/2025/12/17/checksums.sha256 ./

# Verify checksums
sha256sum -c checksums.sha256
```

**Expected Output:**
```
audit/secrets/2025/12/17/abc123.json.gz: OK
audit/secrets/2025/12/17/def456.json.gz: OK
```

**If checksum fails:** 🚨 **CRITICAL** - Audit log tampering detected, escalate to security team immediately

---

## References

- **[audit-log-retention.md](audit-log-retention.md)** - Audit log retention policy (7 years, S3 WORM)
- **[incident-response.md](incident-response.md)** - P4: RNDC Key Compromise playbook
- **[architecture.md](architecture.md)** - RBAC architecture and secrets management
- **[threat-model.md](threat-model.md)** - STRIDE threat S2 (Tampered RNDC Keys)
- **PCI-DSS v4.0** - Requirement 7.1.2 (Least Privilege), 10.2.1 (Audit Logs)
- **SOX 404** - IT General Controls (Access Control, Audit Logs)
- **Basel III** - Cyber Risk Management Principles

---

**Last Updated:** 2025-12-18
**Next Review:** 2026-03-18 (Quarterly)
