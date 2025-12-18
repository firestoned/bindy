# PCI-DSS Compliance

**Payment Card Industry Data Security Standard**

---

## Overview

The Payment Card Industry Data Security Standard (PCI-DSS) is a set of security standards designed to ensure that all companies that accept, process, store, or transmit credit card information maintain a secure environment.

While Bindy itself does not process payment card data, it operates in a **payment card processing environment** and must comply with PCI-DSS requirements as part of the overall security infrastructure.

**Why Bindy is In-Scope for PCI-DSS:**

1. **Supports Cardholder Data Environment (CDE)**: Bindy provides DNS resolution for payment processing systems
2. **Service Availability**: DNS outages prevent access to payment systems (PCI-DSS 12.10 - incident response)
3. **Secure Development**: Code handling DNS data must follow secure development practices (PCI-DSS 6.x)
4. **Access Controls**: Secret management follows least privilege (PCI-DSS 7.x)
5. **Audit Logging**: All system access logged (PCI-DSS 10.x)

---

## PCI-DSS Requirements Applicable to Bindy

PCI-DSS has 12 requirements organized into 6 control objectives. Bindy complies with the following:

| PCI-DSS Requirement | Description | Bindy Status |
|---------------------|-------------|--------------|
| **6.2** | Ensure all system components are protected from known vulnerabilities | ✅ Complete |
| **6.4.1** | Secure coding practices | ✅ Complete |
| **6.4.6** | Code review before production release | ✅ Complete |
| **7.1.2** | Restrict access based on need-to-know | ✅ Complete |
| **10.2.1** | Implement audit trails | ✅ Complete |
| **10.5.1** | Protect audit trail from unauthorized modification | ✅ Complete |
| **12.1** | Establish security policies | ✅ Complete |
| **12.10** | Implement incident response plan | ✅ Complete |

---

## Requirement 6: Secure Systems and Applications

### 6.2 - Ensure All System Components Are Protected from Known Vulnerabilities

**Requirement:** Apply security patches and updates within defined timeframes based on risk.

**Bindy Implementation:**

| Control | Implementation | Evidence |
|---------|----------------|----------|
| **Daily Vulnerability Scanning** | `cargo audit` runs daily at 00:00 UTC | GitHub Actions workflow logs |
| **CI/CD Scanning** | `cargo audit --deny warnings` fails PR on CRITICAL/HIGH CVEs | GitHub Actions PR checks |
| **Container Image Scanning** | Trivy scans all container images (CRITICAL, HIGH, MEDIUM, LOW) | GitHub Security tab, SARIF reports |
| **Remediation SLAs** | CRITICAL (24h), HIGH (7d), MEDIUM (30d), LOW (90d) | [Vulnerability Management Policy](../../security/VULNERABILITY_MANAGEMENT.md) |
| **Automated Alerts** | GitHub Security Advisories create issues automatically | GitHub Security tab |

**Remediation Tracking:**

```bash
# Check for open vulnerabilities
cargo audit

# View vulnerability history
gh api repos/firestoned/bindy/security-advisories

# Show remediation SLA compliance
# (All CRITICAL vulnerabilities patched within 24 hours)
cat docs/security/VULNERABILITY_MANAGEMENT.md
```

**Evidence for QSA (Qualified Security Assessor):**
- **Vulnerability Scan Results**: GitHub Security tab → Code scanning alerts
- **Remediation Evidence**: GitHub issues tagged `security`, `vulnerability`
- **Patch History**: `CHANGELOG.md` entries for security updates
- **SLA Compliance**: Monthly vulnerability remediation reports

**Compliance Status:** ✅ **PASS** - Daily scanning, automated remediation tracking, SLAs met

---

### 6.4.1 - Secure Coding Practices

**Requirement:** Develop software applications based on industry standards and best practices.

**Bindy Implementation:**

| Control | Implementation | Evidence |
|---------|----------------|----------|
| **Input Validation** | All DNS zone names validated against RFC 1035 | `src/bind9.rs:validate_zone_name()` |
| **Error Handling** | No panics in production (use `Result<T, E>`) | `cargo clippy -- -D warnings` |
| **Secure Dependencies** | All dependencies from crates.io (verified sources) | `Cargo.toml`, `Cargo.lock` |
| **No Hardcoded Secrets** | Pre-commit hooks detect secrets | GitHub Advanced Security |
| **Memory Safety** | Rust's borrow checker prevents buffer overflows | Rust language guarantees |
| **Logging Best Practices** | No sensitive data in logs (PII, secrets) | Code review checks |

**OWASP Top 10 Mitigations:**

| OWASP Risk | Bindy Mitigation |
|------------|------------------|
| **A01: Broken Access Control** | ✅ RBAC least privilege (minimal delete permissions for lifecycle management) |
| **A02: Cryptographic Failures** | ✅ TLS for all API calls, secrets in Kubernetes Secrets |
| **A03: Injection** | ✅ Parameterized DNS zone updates (RNDC), input validation |
| **A04: Insecure Design** | ✅ Threat model (STRIDE), security architecture documented |
| **A05: Security Misconfiguration** | ✅ Minimal RBAC, non-root containers, read-only filesystem |
| **A06: Vulnerable Components** | ✅ Daily `cargo audit`, Trivy container scanning |
| **A07: Identification/Authentication** | ✅ Kubernetes ServiceAccount auth, signed commits |
| **A08: Software/Data Integrity** | ✅ Signed commits, SBOM, reproducible builds |
| **A09: Logging Failures** | ✅ Comprehensive logging (controller, audit, DNS queries) |
| **A10: Server-Side Request Forgery** | ✅ No external HTTP calls (only Kubernetes API, RNDC) |

**Evidence for QSA:**
- **Code Review Records**: GitHub PR approval history
- **Static Analysis**: `cargo clippy` results (all PRs)
- **Security Training**: `CONTRIBUTING.md` - secure coding guidelines
- **Threat Model**: `docs/security/THREAT_MODEL.md` - STRIDE analysis

**Compliance Status:** ✅ **PASS** - Rust memory safety, OWASP Top 10 mitigations, secure coding guidelines

---

### 6.4.6 - Code Review Before Production Release

**Requirement:** All code changes reviewed by individuals other than the original author before release.

**Bindy Implementation:**

| Control | Implementation | Evidence |
|---------|----------------|----------|
| **2+ Reviewers Required** | GitHub branch protection enforces 2 approvals | Branch protection rules |
| **No Self-Approval** | PR author cannot approve own PR | GitHub settings |
| **Signed Commits** | All commits GPG/SSH signed (non-repudiation) | Git commit log |
| **Automated Security Checks** | `cargo audit`, `cargo clippy`, `cargo test` must pass | GitHub Actions status checks |
| **Change Documentation** | All changes documented in `CHANGELOG.md` | CHANGELOG.md |

**Code Review Checklist:**

Every PR is reviewed for:
- ✅ Security vulnerabilities (injection, XSS, secrets in code)
- ✅ Input validation (DNS zone names, RNDC keys)
- ✅ Error handling (no panics, proper `Result` usage)
- ✅ Logging (no PII/secrets in logs)
- ✅ Tests (unit tests for new code, integration tests for features)

**Evidence for QSA:**

```bash
# Show PR approval history (last 6 months)
gh pr list --state merged --since "6 months ago" --json number,title,reviews

# Show commit signatures
git log --show-signature --since="6 months ago"

# Show CI/CD security check results
gh run list --workflow ci.yaml --limit 100
```

**Compliance Status:** ✅ **PASS** - 2+ reviewers, signed commits, automated security checks

---

## Requirement 7: Restrict Access to Cardholder Data

### 7.1.2 - Restrict Access Based on Need-to-Know

**Requirement:** Limit access to system components and cardholder data to only those individuals whose job requires such access.

**Bindy Implementation:**

| Control | Implementation | Evidence |
|---------|----------------|----------|
| **Least Privilege RBAC** | Controller minimal RBAC (create/delete secrets for RNDC lifecycle, delete managed resources for cleanup) | `deploy/rbac/clusterrole.yaml` |
| **Minimal Delete Permissions** | Controller delete limited to managed resources (finalizer cleanup, scaling) | RBAC verification script |
| **Secret Access Audit Trail** | All secret access logged (7-year retention) | [Secret Access Audit Trail](../../security/SECRET_ACCESS_AUDIT.md) |
| **Quarterly Access Reviews** | Security team reviews access every quarter | Access review reports |
| **Role-Based Access** | Different roles for dev, ops, security teams | GitHub team permissions |

**RBAC Policy Verification:**

```bash
# Verify controller has minimal permissions
./deploy/rbac/verify-rbac.sh

# Expected output:
# ✅ Controller can READ secrets (get, list, watch)
# ✅ Controller can CREATE/DELETE secrets (RNDC key lifecycle only)
# ✅ Controller CANNOT UPDATE/PATCH secrets (immutable pattern)
# ✅ Controller can DELETE managed resources (Bind9Instance, Bind9Cluster, finalizer cleanup)
# ✅ Controller CANNOT DELETE user resources (DNSZone, Records, Bind9GlobalCluster)
```

**Secret Access Monitoring:**

```bash
# Query: Non-controller secret access (should return 0 results)
curl -X POST "https://elasticsearch:9200/bindy-audit-*/_search" \
  -H 'Content-Type: application/json' \
  -d '{
    "query": {
      "bool": {
        "must": [
          { "term": { "objectRef.resource": "secrets" } },
          { "term": { "objectRef.namespace": "dns-system" } }
        ],
        "must_not": [
          { "term": { "user.username.keyword": "system:serviceaccount:dns-system:bindy-controller" } }
        ]
      }
    }
  }'

# Expected: 0 hits (only authorized controller accesses secrets)
```

**Evidence for QSA:**
- **RBAC Policy**: `deploy/rbac/clusterrole.yaml`
- **RBAC Verification**: CI/CD artifact `rbac-verification.txt`
- **Secret Access Logs**: Elasticsearch query results (quarterly)
- **Access Reviews**: `docs/compliance/access-reviews/YYYY-QN.md`

**Compliance Status:** ✅ **PASS** - Least privilege RBAC, quarterly access reviews, audit trail

---

## Requirement 10: Log and Monitor All Access

### 10.2.1 - Implement Audit Trails

**Requirement:** Implement automated audit trails for all system components to reconstruct the following events:
- All individual user accesses to cardholder data
- Actions taken by individuals with root/admin privileges
- Access to all audit trails
- Invalid logical access attempts
- Use of identification/authentication mechanisms
- Initialization, stopping, or pausing of audit logs
- Creation and deletion of system-level objects

**Bindy Implementation:**

| Control | Implementation | Evidence |
|---------|----------------|----------|
| **Kubernetes Audit Logs** | All API requests logged (CRD ops, secret access) | Kubernetes audit policy |
| **Secret Access Logging** | All secret get/list/watch logged | `docs/security/SECRET_ACCESS_AUDIT.md` |
| **Controller Logs** | All reconciliation loops, DNS updates | Fluent Bit, S3 storage |
| **Access Attempts** | Failed secret access (403 Forbidden) logged | Kubernetes audit logs |
| **Authentication Events** | ServiceAccount token usage logged | Kubernetes audit logs |

**Audit Log Fields (PCI-DSS 10.2.1 Compliance):**

| PCI-DSS Requirement | Bindy Audit Log Field | Example Value |
|---------------------|----------------------|---------------|
| **User identification** | `user.username` | `system:serviceaccount:dns-system:bindy-controller` |
| **Type of event** | `verb` | `get`, `list`, `watch`, `create`, `update`, `delete` |
| **Date and time** | `requestReceivedTimestamp` | `2025-12-18T12:34:56.789Z` (ISO 8601 UTC) |
| **Success/failure indication** | `responseStatus.code` | `200` (success), `403` (forbidden) |
| **Origination of event** | `sourceIPs` | `10.244.1.15` (pod IP) |
| **Identity of affected data** | `objectRef.name` | `rndc-key-primary` (secret name) |

**Sample Audit Log Entry:**

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
    "username": "system:serviceaccount:dns-system:bindy-controller",
    "uid": "abc123",
    "groups": ["system:serviceaccounts", "system:serviceaccounts:dns-system"]
  },
  "sourceIPs": ["10.244.1.15"],
  "objectRef": {
    "resource": "secrets",
    "namespace": "dns-system",
    "name": "rndc-key-primary",
    "apiVersion": "v1"
  },
  "responseStatus": {
    "code": 200
  },
  "requestReceivedTimestamp": "2025-12-18T12:34:56.789Z"
}
```

**Evidence for QSA:**

```bash
# Show audit logs for last 30 days (sample)
curl -X POST "https://elasticsearch:9200/bindy-audit-*/_search" \
  -H 'Content-Type: application/json' \
  -d '{
    "query": {
      "range": {
        "requestReceivedTimestamp": {
          "gte": "now-30d"
        }
      }
    },
    "size": 100
  }' | jq .

# Show failed access attempts (last 30 days)
curl -X POST "https://elasticsearch:9200/bindy-audit-*/_search" \
  -H 'Content-Type: application/json' \
  -d '{
    "query": {
      "bool": {
        "must": [
          { "term": { "responseStatus.code": 403 } },
          { "range": { "requestReceivedTimestamp": { "gte": "now-30d" } } }
        ]
      }
    }
  }' | jq .
```

**Compliance Status:** ✅ **PASS** - All PCI-DSS 10.2.1 fields captured, audit logs retained 7 years

---

### 10.5.1 - Protect Audit Trail from Unauthorized Modification

**Requirement:** Limit viewing of audit trails to those with a job-related need.

**Bindy Implementation:**

| Control | Implementation | Evidence |
|---------|----------------|----------|
| **Immutable Storage** | S3 Object Lock (WORM) prevents log deletion/modification | S3 bucket configuration |
| **Access Controls** | IAM policies restrict S3 access to security team only | AWS IAM policy |
| **Access Logging (Meta-Logging)** | S3 server access logs track who reads audit logs | S3 access logs |
| **Integrity Verification** | SHA-256 checksums verify logs not tampered | Daily CronJob output |
| **Encryption at Rest** | S3 SSE-S3 encryption for all audit logs | S3 bucket configuration |
| **Encryption in Transit** | TLS 1.3 for all S3 API calls | AWS default |

**S3 WORM (Object Lock) Configuration:**

```bash
# Show Object Lock enabled
aws s3api get-object-lock-configuration --bucket bindy-audit-logs

# Expected output:
# {
#   "ObjectLockConfiguration": {
#     "ObjectLockEnabled": "Enabled",
#     "Rule": {
#       "DefaultRetention": {
#         "Mode": "GOVERNANCE",
#         "Days": 2555
#       }
#     }
#   }
# }
```

**IAM Policy (Audit Log Access):**

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "DenyDelete",
      "Effect": "Deny",
      "Principal": "*",
      "Action": [
        "s3:DeleteObject",
        "s3:DeleteObjectVersion"
      ],
      "Resource": "arn:aws:s3:::bindy-audit-logs/*"
    },
    {
      "Sid": "SecurityTeamReadOnly",
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::123456789012:role/SecurityTeam"
      },
      "Action": [
        "s3:GetObject",
        "s3:ListBucket"
      ],
      "Resource": [
        "arn:aws:s3:::bindy-audit-logs",
        "arn:aws:s3:::bindy-audit-logs/*"
      ]
    }
  ]
}
```

**Evidence for QSA:**
- **S3 Bucket Policy**: AWS IAM policy (deny delete, security team read-only)
- **Object Lock Configuration**: `aws s3api get-object-lock-configuration`
- **Integrity Verification**: CronJob logs (daily SHA-256 checksum verification)
- **Access Logs**: S3 server access logs (who accessed audit logs)

**Compliance Status:** ✅ **PASS** - Immutable WORM storage, access controls, integrity verification

---

## Requirement 12: Maintain a Security Policy

### 12.1 - Establish, Publish, Maintain, and Disseminate a Security Policy

**Requirement:** Establish, publish, maintain, and disseminate a security policy that addresses all PCI-DSS requirements.

**Bindy Implementation:**

| Policy Document | Location | Last Updated |
|----------------|----------|--------------|
| **Security Policy** | [SECURITY.md](../../../SECURITY.md) | 2025-12-18 |
| **Threat Model** | [docs/security/THREAT_MODEL.md](../../security/THREAT_MODEL.md) | 2025-12-17 |
| **Security Architecture** | [docs/security/ARCHITECTURE.md](../../security/ARCHITECTURE.md) | 2025-12-17 |
| **Incident Response** | [docs/security/INCIDENT_RESPONSE.md](../../security/INCIDENT_RESPONSE.md) | 2025-12-17 |
| **Vulnerability Management** | [docs/security/VULNERABILITY_MANAGEMENT.md](../../security/VULNERABILITY_MANAGEMENT.md) | 2025-12-15 |
| **Audit Log Retention** | [docs/security/AUDIT_LOG_RETENTION.md](../../security/AUDIT_LOG_RETENTION.md) | 2025-12-18 |

**Evidence for QSA:**
- **Published Policies**: All policies in GitHub repository (public access)
- **Version Control**: Git history shows policy updates and reviews
- **Annual Review**: Policies reviewed quarterly (Next Review: 2026-03-18)

**Compliance Status:** ✅ **PASS** - Security policies documented, published, and maintained

---

### 12.10 - Implement an Incident Response Plan

**Requirement:** Implement an incident response plan. Be prepared to respond immediately to a system breach.

**Bindy Implementation:**

| Incident Type | Playbook | Response Time | SLA |
|---------------|----------|---------------|-----|
| **Critical Vulnerability (CVSS 9.0-10.0)** | [P1](../../security/INCIDENT_RESPONSE.md#p1-critical-vulnerability-detected) | < 15 minutes | Patch within 24 hours |
| **Compromised Controller Pod** | [P2](../../security/INCIDENT_RESPONSE.md#p2-compromised-controller-pod) | < 15 minutes | Isolate within 1 hour |
| **DNS Service Outage** | [P3](../../security/INCIDENT_RESPONSE.md#p3-dns-service-outage) | < 15 minutes | Restore within 4 hours |
| **RNDC Key Compromise** | [P4](../../security/INCIDENT_RESPONSE.md#p4-rndc-key-compromise) | < 15 minutes | Rotate keys within 1 hour |
| **Unauthorized DNS Changes** | [P5](../../security/INCIDENT_RESPONSE.md#p5-unauthorized-dns-changes) | < 1 hour | Revert within 4 hours |
| **DDoS Attack** | [P6](../../security/INCIDENT_RESPONSE.md#p6-ddos-attack) | < 15 minutes | Mitigate within 1 hour |
| **Supply Chain Compromise** | [P7](../../security/INCIDENT_RESPONSE.md#p7-supply-chain-compromise) | < 15 minutes | Rebuild within 24 hours |

**Incident Response Process (NIST Lifecycle):**

1. **Preparation**: Playbooks documented, tools configured, team trained
2. **Detection & Analysis**: Prometheus alerts, audit log analysis
3. **Containment**: Isolate affected systems, prevent escalation
4. **Eradication**: Remove threat, patch vulnerability
5. **Recovery**: Restore service, verify integrity
6. **Post-Incident Activity**: Document lessons learned, improve defenses

**Evidence for QSA:**
- **Incident Response Playbooks**: `docs/security/INCIDENT_RESPONSE.md`
- **Tabletop Exercise Results**: Semi-annual drill reports
- **Incident Logs**: S3 `bindy-audit-logs/incidents/` (if any incidents occurred)

**Compliance Status:** ✅ **PASS** - 7 incident playbooks documented, tabletop exercises conducted

---

## PCI-DSS Audit Evidence Package

For your annual PCI-DSS assessment, provide the QSA with:

1. **Requirement 6 (Secure Systems)**:
   - Vulnerability scan results (GitHub Security tab)
   - Remediation tracking (GitHub issues, CHANGELOG.md)
   - Code review records (PR approval history)
   - Static analysis results (cargo clippy, cargo audit)

2. **Requirement 7 (Access Controls)**:
   - RBAC policy (`deploy/rbac/clusterrole.yaml`)
   - RBAC verification output (CI/CD artifact)
   - Quarterly access review reports
   - Secret access audit logs (Elasticsearch query results)

3. **Requirement 10 (Logging)**:
   - Sample audit logs (redacted, last 30 days)
   - S3 bucket configuration (WORM, encryption, access controls)
   - Log integrity verification results (CronJob output)
   - Audit log access logs (meta-logging, S3 server access logs)

4. **Requirement 12 (Policies)**:
   - Security policies (`SECURITY.md`, `docs/security/*.md`)
   - Incident response playbooks
   - Tabletop exercise results

---

## See Also

- [Vulnerability Management Policy](../security/vulnerability-management.md) - Remediation SLAs
- [Secret Access Audit Trail](../security/secret-access-audit.md) - PCI-DSS 7.1.2, 10.2.1
- [Audit Log Retention Policy](../security/audit-log-retention.md) - PCI-DSS 10.5.1
- [Incident Response Playbooks](../security/incident-response.md) - PCI-DSS 12.10
- [Security Architecture](../security/architecture.md) - RBAC, secrets management
- [Build Reproducibility](../security/build-reproducibility.md) - Supply chain integrity
