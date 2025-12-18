# Audit Log Retention Policy - Bindy DNS Controller

**Version:** 1.0
**Last Updated:** 2025-12-17
**Owner:** Security Team
**Compliance:** SOX 404 (7 years), PCI-DSS 10.5.1 (1 year), Basel III

---

## Table of Contents

- [Overview](#overview)
- [Retention Requirements](#retention-requirements)
- [Log Types and Sources](#log-types-and-sources)
- [Log Collection](#log-collection)
- [Log Storage](#log-storage)
- [Log Retention Lifecycle](#log-retention-lifecycle)
- [Log Integrity](#log-integrity)
- [Access Controls](#access-controls)
- [Audit Trail Queries](#audit-trail-queries)
- [Compliance Evidence](#compliance-evidence)
- [Implementation Guide](#implementation-guide)

---

## Overview

This document defines the audit log retention policy for the Bindy DNS Controller to ensure compliance with SOX 404 (7-year retention), PCI-DSS 10.5.1 (1-year retention), and Basel III operational risk management requirements.

### Objectives

1. **Retention Compliance**: Meet regulatory retention requirements (SOX: 7 years, PCI-DSS: 1 year)
2. **Immutability**: Ensure logs cannot be modified or deleted (tamper-proof storage)
3. **Integrity**: Verify log integrity through checksums and cryptographic signing
4. **Accessibility**: Provide query capabilities for compliance audits and incident response
5. **Security**: Protect audit logs with encryption and access controls

---

## Retention Requirements

### Regulatory Requirements

| Regulation | Retention Period | Storage Type | Accessibility |
|------------|------------------|--------------|---------------|
| **SOX 404** | **7 years** | Immutable (WORM) | Online for 1 year, archive for 6 years |
| **PCI-DSS 10.5.1** | **1 year** | Immutable | Online for 3 months, readily available for 1 year |
| **Basel III** | **7 years** | Immutable | Online for 1 year, archive for 6 years |
| **Internal Policy** | **7 years** | Immutable | Online for 1 year, archive for 6 years |

### Retention Periods by Log Type

| Log Type | Active Storage | Archive Storage | Total Retention | Rationale |
|----------|----------------|-----------------|-----------------|-----------|
| **Kubernetes API Audit Logs** | 90 days | 7 years | 7 years | SOX 404 (IT controls change tracking) |
| **Controller Application Logs** | 90 days | 1 year | 1 year | PCI-DSS (DNS changes, RNDC operations) |
| **Secret Access Logs** | 90 days | 7 years | 7 years | SOX 404 (access to sensitive data) |
| **DNS Query Logs** | 30 days | 1 year | 1 year | PCI-DSS (network activity monitoring) |
| **Security Scan Results** | 1 year | 7 years | 7 years | SOX 404 (vulnerability management evidence) |
| **Incident Response Logs** | Indefinite | Indefinite | Indefinite | Legal hold, lessons learned |

---

## Log Types and Sources

### 1. Kubernetes API Audit Logs

**Source:** Kubernetes API server
**Content:** All API requests (who, what, when, result)
**Format:** JSON (structured)

**What is Logged:**
- User/ServiceAccount identity
- API verb (get, create, update, patch, delete)
- Resource type and name (e.g., `dnszones/example-com`)
- Namespace
- Timestamp (RFC3339)
- Response status (success/failure)
- Client IP address
- User agent

**Example:**
```json
{
  "kind": "Event",
  "apiVersion": "audit.k8s.io/v1",
  "level": "Metadata",
  "auditID": "a0b1c2d3-e4f5-6789-0abc-def123456789",
  "stage": "ResponseComplete",
  "requestURI": "/apis/bindy.firestoned.io/v1alpha1/namespaces/team-web/dnszones/example-com",
  "verb": "update",
  "user": {
    "username": "system:serviceaccount:dns-system:bindy",
    "uid": "12345678-90ab-cdef-1234-567890abcdef",
    "groups": ["system:serviceaccounts", "system:authenticated"]
  },
  "sourceIPs": ["10.244.0.5"],
  "userAgent": "kube-rs/0.88.0",
  "objectRef": {
    "resource": "dnszones",
    "namespace": "team-web",
    "name": "example-com",
    "apiGroup": "bindy.firestoned.io",
    "apiVersion": "v1alpha1"
  },
  "responseStatus": {
    "metadata": {},
    "code": 200
  },
  "requestReceivedTimestamp": "2025-12-17T10:23:45.123456Z",
  "stageTimestamp": "2025-12-17T10:23:45.234567Z"
}
```

**Retention:** 7 years (SOX 404)

---

### 2. Controller Application Logs

**Source:** Bindy controller pod (`kubectl logs`)
**Content:** Reconciliation events, RNDC commands, errors
**Format:** JSON (structured with tracing spans)

**What is Logged:**
- Reconciliation start/end (DNSZone, Bind9Instance)
- RNDC commands sent (reload, freeze, thaw)
- ConfigMap create/update operations
- Errors and warnings
- Performance metrics (reconciliation duration)

**Example:**
```json
{
  "timestamp": "2025-12-17T10:23:45.123Z",
  "level": "INFO",
  "target": "bindy::reconcilers::dnszone",
  "fields": {
    "message": "Reconciling DNSZone",
    "zone": "example.com",
    "namespace": "team-web",
    "action": "update"
  },
  "span": {
    "name": "reconcile_dnszone",
    "zone": "example.com"
  }
}
```

**Retention:** 1 year (PCI-DSS)

---

### 3. Secret Access Logs

**Source:** Kubernetes audit logs (filtered)
**Content:** All reads of Secrets in `dns-system` namespace
**Format:** JSON (structured)

**What is Logged:**
- ServiceAccount that read the secret
- Secret name (e.g., `rndc-key`)
- Timestamp
- Result (success/denied)

**Example:**
```json
{
  "kind": "Event",
  "verb": "get",
  "user": {
    "username": "system:serviceaccount:dns-system:bindy"
  },
  "objectRef": {
    "resource": "secrets",
    "namespace": "dns-system",
    "name": "rndc-key"
  },
  "responseStatus": {
    "code": 200
  },
  "requestReceivedTimestamp": "2025-12-17T10:23:45.123456Z"
}
```

**Retention:** 7 years (SOX 404 - access to sensitive data)

---

### 4. DNS Query Logs

**Source:** BIND9 pods (query logging enabled)
**Content:** DNS queries received and responses sent
**Format:** BIND9 query log format

**What is Logged:**
- Client IP address
- Query type (A, AAAA, CNAME, etc.)
- Query name (e.g., `www.example.com`)
- Response code (NOERROR, NXDOMAIN, etc.)
- Timestamp

**Example:**
```
17-Dec-2025 10:23:45.123 queries: info: client @0x7f8b4c000000 10.244.1.15#54321 (www.example.com): query: www.example.com IN A + (10.244.0.10)
```

**Retention:** 1 year (PCI-DSS - network activity monitoring)

---

### 5. Security Scan Results

**Source:** GitHub Actions artifacts (cargo-audit, Trivy)
**Content:** Vulnerability scan results
**Format:** JSON

**What is Logged:**
- Scan timestamp
- Vulnerabilities found (CVE, severity, package)
- Scan type (dependency, container image)
- Remediation status

**Example:**
```json
{
  "timestamp": "2025-12-17T10:23:45Z",
  "scan_type": "cargo-audit",
  "vulnerabilities": {
    "count": 0,
    "found": []
  }
}
```

**Retention:** 7 years (SOX 404 - vulnerability management evidence)

---

### 6. Incident Response Logs

**Source:** GitHub issues, post-incident review documents
**Content:** Incident timeline, actions taken, root cause
**Format:** Markdown, JSON

**Retention:** Indefinite (legal hold, lessons learned)

---

## Log Collection

### Kubernetes Audit Logs

**Configuration:** Kubernetes API server audit policy

```yaml
# /etc/kubernetes/audit-policy.yaml
apiVersion: audit.k8s.io/v1
kind: Policy
metadata:
  name: bindy-audit-policy
rules:
  # Log all Secret access (H-3 requirement)
  - level: Metadata
    verbs: ["get", "list", "watch"]
    resources:
      - group: ""
        resources: ["secrets"]
    namespaces: ["dns-system"]

  # Log all DNSZone CRD operations
  - level: Metadata
    verbs: ["create", "update", "patch", "delete"]
    resources:
      - group: "bindy.firestoned.io"
        resources: ["dnszones", "bind9instances", "bind9clusters"]

  # Log all DNS record CRD operations
  - level: Metadata
    verbs: ["create", "update", "patch", "delete"]
    resources:
      - group: "bindy.firestoned.io"
        resources: ["arecords", "cnamerecords", "mxrecords", "txtrecords", "srvrecords"]

  # Don't log read-only operations on low-sensitivity resources
  - level: None
    verbs: ["get", "list", "watch"]
    resources:
      - group: ""
        resources: ["configmaps", "pods", "services"]

  # Catch-all: log at Request level for all other operations
  - level: Request
```

**API Server Flags:**
```bash
kube-apiserver \
  --audit-log-path=/var/log/kubernetes/audit.log \
  --audit-log-maxage=90 \
  --audit-log-maxbackup=10 \
  --audit-log-maxsize=100 \
  --audit-policy-file=/etc/kubernetes/audit-policy.yaml
```

**Log Forwarding:**
- **Method 1 (Recommended)**: Fluent Bit DaemonSet → S3/CloudWatch/Elasticsearch
- **Method 2**: Kubernetes audit webhook → SIEM (Splunk, Datadog)

---

### Controller Application Logs

**Collection:** `kubectl logs` forwarded to log aggregation system

**Fluent Bit Configuration:**
```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: fluent-bit-config
  namespace: logging
data:
  fluent-bit.conf: |
    [SERVICE]
        Flush        5
        Daemon       Off
        Log_Level    info

    [INPUT]
        Name              tail
        Path              /var/log/containers/bindy-*.log
        Parser            docker
        Tag               bindy.controller
        Refresh_Interval  5

    [FILTER]
        Name                kubernetes
        Match               bindy.*
        Kube_URL            https://kubernetes.default.svc:443
        Kube_CA_File        /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
        Kube_Token_File     /var/run/secrets/kubernetes.io/serviceaccount/token

    [OUTPUT]
        Name   s3
        Match  bindy.*
        bucket bindy-audit-logs
        region us-east-1
        store_dir /tmp/fluent-bit/s3
        total_file_size 100M
        upload_timeout 10m
        s3_key_format /controller-logs/%Y/%m/%d/$UUID.gz
```

---

### DNS Query Logs

**BIND9 Configuration:**
```
# named.conf
logging {
    channel query_log {
        file "/var/log/named/query.log" versions 10 size 100m;
        severity info;
        print-time yes;
        print-category yes;
        print-severity yes;
    };
    category queries { query_log; };
};
```

**Collection:** Fluent Bit sidecar in BIND9 pods → S3

---

## Log Storage

### Storage Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Active Storage (90 days)                  │
│  - Elasticsearch / CloudWatch Logs                          │
│  - Fast queries, dashboards, alerts                         │
│  - Encrypted at rest (AES-256)                              │
└─────────────────────────────────────────────────────────────┘
                          │
                          │ Automatic archival
                          ▼
┌─────────────────────────────────────────────────────────────┐
│               Archive Storage (7 years)                      │
│  - AWS S3 Glacier / Google Cloud Archival Storage           │
│  - WORM (Write-Once-Read-Many) bucket                       │
│  - Object Lock enabled (Governance/Compliance mode)         │
│  - Versioning enabled                                       │
│  - Encrypted at rest (AES-256 or KMS)                       │
│  - Lifecycle policy: Transition to Glacier after 90 days    │
└─────────────────────────────────────────────────────────────┘
```

### AWS S3 Configuration (Example)

**Bucket Policy:**
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "DenyUnencryptedObjectUploads",
      "Effect": "Deny",
      "Principal": "*",
      "Action": "s3:PutObject",
      "Resource": "arn:aws:s3:::bindy-audit-logs/*",
      "Condition": {
        "StringNotEquals": {
          "s3:x-amz-server-side-encryption": "AES256"
        }
      }
    },
    {
      "Sid": "DenyInsecureTransport",
      "Effect": "Deny",
      "Principal": "*",
      "Action": "s3:*",
      "Resource": [
        "arn:aws:s3:::bindy-audit-logs",
        "arn:aws:s3:::bindy-audit-logs/*"
      ],
      "Condition": {
        "Bool": {
          "aws:SecureTransport": "false"
        }
      }
    }
  ]
}
```

**Lifecycle Policy:**
```json
{
  "Rules": [
    {
      "Id": "TransitionToGlacier",
      "Status": "Enabled",
      "Filter": {
        "Prefix": ""
      },
      "Transitions": [
        {
          "Days": 90,
          "StorageClass": "GLACIER"
        }
      ],
      "Expiration": {
        "Days": 2555
      }
    }
  ]
}
```

**Object Lock Configuration (WORM):**
```bash
# Enable versioning (required for Object Lock)
aws s3api put-bucket-versioning \
  --bucket bindy-audit-logs \
  --versioning-configuration Status=Enabled

# Enable Object Lock (WORM)
aws s3api put-object-lock-configuration \
  --bucket bindy-audit-logs \
  --object-lock-configuration '{
    "ObjectLockEnabled": "Enabled",
    "Rule": {
      "DefaultRetention": {
        "Mode": "GOVERNANCE",
        "Days": 2555
      }
    }
  }'
```

---

## Log Retention Lifecycle

### Phase 1: Active Storage (0-90 days)

**Storage:** Elasticsearch / CloudWatch Logs
**Access:** Real-time queries, dashboards, alerts
**Performance:** Sub-second query response
**Cost:** High (optimized for performance)

**Operations:**
- Log ingestion via Fluent Bit
- Real-time indexing and search
- Alert triggers (anomaly detection)
- Compliance queries (audit reviews)

---

### Phase 2: Archive Storage (91 days - 7 years)

**Storage:** AWS S3 Glacier / Google Cloud Archival Storage
**Access:** Retrieval takes 1-5 minutes (Glacier Instant Retrieval) or 3-5 hours (Glacier Flexible Retrieval)
**Performance:** Optimized for cost, not speed
**Cost:** Low ($0.004/GB/month for Glacier)

**Operations:**
- Automatic transition via S3 lifecycle policy
- Object Lock prevents deletion (WORM)
- Retrieval for compliance audits or incident forensics
- Periodic integrity verification (see below)

---

### Phase 3: Deletion (After 7 years)

**Process:**
1. Automated lifecycle policy expires objects
2. Legal hold check (ensure no active litigation)
3. Compliance team approval required
4. Final integrity verification before deletion
5. Deletion logged and audited

**Exception:** Incident response logs are retained indefinitely (legal hold)

---

## Log Integrity

### Checksum Verification

**Method:** SHA-256 checksums for all log files

**Process:**
1. Log file created (e.g., `audit-2025-12-17.log.gz`)
2. Calculate SHA-256 checksum
3. Store checksum in metadata file (`audit-2025-12-17.log.gz.sha256`)
4. Upload both to S3
5. S3 ETag provides additional integrity check

**Verification:**
```bash
# Download log file and checksum
aws s3 cp s3://bindy-audit-logs/audit-2025-12-17.log.gz .
aws s3 cp s3://bindy-audit-logs/audit-2025-12-17.log.gz.sha256 .

# Verify checksum
sha256sum -c audit-2025-12-17.log.gz.sha256

# Expected output: audit-2025-12-17.log.gz: OK
```

---

### Cryptographic Signing (Optional, High-Security)

**Method:** GPG signing of log files

**Process:**
1. Log file created
2. Sign with GPG private key
3. Upload log + signature to S3

**Verification:**
```bash
# Download log and signature
aws s3 cp s3://bindy-audit-logs/audit-2025-12-17.log.gz .
aws s3 cp s3://bindy-audit-logs/audit-2025-12-17.log.gz.sig .

# Verify signature
gpg --verify audit-2025-12-17.log.gz.sig audit-2025-12-17.log.gz

# Expected output: Good signature from "Bindy Security Team <security@firestoned.io>"
```

---

### Tamper Detection

**Indicators of Tampering:**
- Checksum mismatch
- GPG signature invalid
- S3 Object Lock violation attempt
- Missing log files (gaps in sequence)
- Timestamp inconsistencies

**Response to Tampering:**
1. Trigger security incident (P2: Compromised System)
2. Preserve evidence (take snapshots of S3 bucket)
3. Investigate root cause (who, how, when)
4. Restore from backup if available
5. Notify compliance team and auditors

---

## Access Controls

### Who Can Access Logs?

| Role | Active Logs (90 days) | Archive Logs (7 years) | Deletion Permission |
|------|----------------------|------------------------|---------------------|
| **Security Team** | ✅ Read | ✅ Read (with approval) | ❌ No |
| **Compliance Team** | ✅ Read | ✅ Read | ❌ No |
| **Auditors (External)** | ✅ Read (time-limited) | ✅ Read (time-limited) | ❌ No |
| **Developers** | ❌ No | ❌ No | ❌ No |
| **Platform Admins** | ✅ Read | ❌ No | ❌ No |
| **CISO** | ✅ Read | ✅ Read | ✅ Yes (with approval) |

### AWS IAM Policy (Example)

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "AllowReadAuditLogs",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:ListBucket"
      ],
      "Resource": [
        "arn:aws:s3:::bindy-audit-logs",
        "arn:aws:s3:::bindy-audit-logs/*"
      ],
      "Condition": {
        "IpAddress": {
          "aws:SourceIp": ["203.0.113.0/24"]
        }
      }
    },
    {
      "Sid": "DenyDelete",
      "Effect": "Deny",
      "Action": [
        "s3:DeleteObject",
        "s3:DeleteObjectVersion"
      ],
      "Resource": "arn:aws:s3:::bindy-audit-logs/*"
    }
  ]
}
```

### Access Logging

**All log access is logged:**
- S3 server access logging enabled
- CloudTrail logs all S3 API calls
- Access logs retained for 7 years (meta-logging)

---

## Audit Trail Queries

### Common Compliance Queries

#### 1. Who modified DNSZone X in the last 30 days?

**Elasticsearch Query:**
```json
{
  "query": {
    "bool": {
      "must": [
        { "term": { "objectRef.resource": "dnszones" } },
        { "term": { "objectRef.name": "example-com" } },
        { "terms": { "verb": ["create", "update", "patch", "delete"] } },
        { "range": { "requestReceivedTimestamp": { "gte": "now-30d" } } }
      ]
    }
  },
  "_source": ["user.username", "verb", "requestReceivedTimestamp", "responseStatus.code"]
}
```

**Expected Output:**
```json
{
  "hits": [
    {
      "_source": {
        "user": { "username": "system:serviceaccount:dns-system:bindy" },
        "verb": "update",
        "requestReceivedTimestamp": "2025-12-15T14:32:10Z",
        "responseStatus": { "code": 200 }
      }
    }
  ]
}
```

---

#### 2. When was RNDC key secret last accessed?

**Elasticsearch Query:**
```json
{
  "query": {
    "bool": {
      "must": [
        { "term": { "objectRef.resource": "secrets" } },
        { "term": { "objectRef.name": "rndc-key" } },
        { "term": { "verb": "get" } }
      ]
    }
  },
  "sort": [
    { "requestReceivedTimestamp": { "order": "desc" } }
  ],
  "size": 10
}
```

---

#### 3. Show all failed authentication attempts in last 7 days

**Elasticsearch Query:**
```json
{
  "query": {
    "bool": {
      "must": [
        { "range": { "responseStatus.code": { "gte": 401, "lte": 403 } } },
        { "range": { "requestReceivedTimestamp": { "gte": "now-7d" } } }
      ]
    }
  },
  "_source": ["user.username", "sourceIPs", "requestReceivedTimestamp", "responseStatus.code"]
}
```

---

#### 4. List all DNS record changes by user alice@example.com

**Elasticsearch Query:**
```json
{
  "query": {
    "bool": {
      "must": [
        { "term": { "user.username": "alice@example.com" } },
        { "terms": { "objectRef.resource": ["arecords", "cnamerecords", "mxrecords", "txtrecords"] } },
        { "terms": { "verb": ["create", "update", "patch", "delete"] } }
      ]
    }
  },
  "sort": [
    { "requestReceivedTimestamp": { "order": "desc" } }
  ]
}
```

---

## Compliance Evidence

### SOX 404 Audit Evidence

**Auditor Requirement:** Demonstrate 7-year retention of IT change logs

**Evidence to Provide:**
1. **Audit Log Retention Policy** (this document)
2. **S3 Bucket Configuration:**
   - Object Lock enabled (WORM)
   - Lifecycle policy (7-year retention)
   - Encryption enabled (AES-256)
3. **Sample Queries:**
   - Show all changes to CRDs in last 7 years
   - Show access control changes (RBAC modifications)
4. **Integrity Verification:**
   - Demonstrate checksum verification process
   - Show no tampering detected

**Audit Query Example:**
```bash
# Retrieve all DNSZone changes from 2019-2025 (7 years)
curl -X POST "elasticsearch:9200/kubernetes-audit-*/_search" -H 'Content-Type: application/json' -d'
{
  "query": {
    "bool": {
      "must": [
        { "term": { "objectRef.resource": "dnszones" } },
        { "range": { "requestReceivedTimestamp": { "gte": "2019-01-01", "lte": "2025-12-31" } } }
      ]
    }
  },
  "size": 10000
}'
```

---

### PCI-DSS 10.5.1 Audit Evidence

**Auditor Requirement:** Demonstrate 1-year retention of audit logs with 3 months readily available

**Evidence to Provide:**
1. **Active Storage:** Elasticsearch with 90 days of logs (online, sub-second queries)
2. **Archive Storage:** S3 with 1 year of logs (retrieval within 5 minutes via Glacier Instant Retrieval)
3. **Sample Queries:** Show ability to query logs from 11 months ago within 5 minutes
4. **Access Controls:** Demonstrate logs are read-only (WORM)

---

### Basel III Operational Risk Audit Evidence

**Auditor Requirement:** Demonstrate ability to reconstruct incident timeline from logs

**Evidence to Provide:**
1. **Incident Response Logs:** Complete timeline of security incidents
2. **Audit Queries:** Show all actions taken during incident (who, what, when)
3. **Integrity Verification:** Prove logs were not tampered with
4. **Retention:** Show logs are retained for 7 years (operational risk data)

---

## Implementation Guide

### Step 1: Enable Kubernetes Audit Logging

**For Managed Kubernetes (EKS, GKE, AKS):**

```bash
# AWS EKS - Enable control plane logging
aws eks update-cluster-config \
  --name bindy-cluster \
  --logging '{"clusterLogging":[{"types":["audit"],"enabled":true}]}'

# Google GKE - Enable audit logging
gcloud container clusters update bindy-cluster \
  --enable-cloud-logging \
  --logging=SYSTEM,WORKLOAD,API

# Azure AKS - Enable diagnostic settings
az monitor diagnostic-settings create \
  --name bindy-audit \
  --resource /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.ContainerService/managedClusters/bindy-cluster \
  --logs '[{"category":"kube-audit","enabled":true}]' \
  --workspace /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.OperationalInsights/workspaces/bindy-logs
```

**For Self-Managed Kubernetes:**

Edit `/etc/kubernetes/manifests/kube-apiserver.yaml`:

```yaml
spec:
  containers:
  - command:
    - kube-apiserver
    - --audit-log-path=/var/log/kubernetes/audit.log
    - --audit-log-maxage=90
    - --audit-log-maxbackup=10
    - --audit-log-maxsize=100
    - --audit-policy-file=/etc/kubernetes/audit-policy.yaml
    volumeMounts:
    - mountPath: /var/log/kubernetes
      name: audit-logs
    - mountPath: /etc/kubernetes/audit-policy.yaml
      name: audit-policy
      readOnly: true
  volumes:
  - hostPath:
      path: /var/log/kubernetes
      type: DirectoryOrCreate
    name: audit-logs
  - hostPath:
      path: /etc/kubernetes/audit-policy.yaml
      type: File
    name: audit-policy
```

---

### Step 2: Deploy Fluent Bit for Log Forwarding

```bash
# Add Fluent Bit Helm repo
helm repo add fluent https://fluent.github.io/helm-charts

# Install Fluent Bit with S3 output
helm install fluent-bit fluent/fluent-bit \
  --namespace logging \
  --create-namespace \
  --set config.outputs="[OUTPUT]\n    Name   s3\n    Match  *\n    bucket bindy-audit-logs\n    region us-east-1"
```

---

### Step 3: Create S3 Bucket with WORM

```bash
# Create bucket
aws s3api create-bucket \
  --bucket bindy-audit-logs \
  --region us-east-1

# Enable versioning
aws s3api put-bucket-versioning \
  --bucket bindy-audit-logs \
  --versioning-configuration Status=Enabled

# Enable Object Lock (WORM)
aws s3api put-object-lock-configuration \
  --bucket bindy-audit-logs \
  --object-lock-configuration '{
    "ObjectLockEnabled": "Enabled",
    "Rule": {
      "DefaultRetention": {
        "Mode": "GOVERNANCE",
        "Days": 2555
      }
    }
  }'

# Enable encryption
aws s3api put-bucket-encryption \
  --bucket bindy-audit-logs \
  --server-side-encryption-configuration '{
    "Rules": [{
      "ApplyServerSideEncryptionByDefault": {
        "SSEAlgorithm": "AES256"
      }
    }]
  }'

# Add lifecycle policy
aws s3api put-bucket-lifecycle-configuration \
  --bucket bindy-audit-logs \
  --lifecycle-configuration file://lifecycle.json
```

---

### Step 4: Deploy Elasticsearch for Active Logs

```bash
# Deploy Elasticsearch using ECK (Elastic Cloud on Kubernetes)
kubectl create -f https://download.elastic.co/downloads/eck/2.10.0/crds.yaml
kubectl apply -f https://download.elastic.co/downloads/eck/2.10.0/operator.yaml

# Create Elasticsearch cluster
kubectl apply -f - <<EOF
apiVersion: elasticsearch.k8s.elastic.co/v1
kind: Elasticsearch
metadata:
  name: bindy-logs
  namespace: logging
spec:
  version: 8.11.0
  nodeSets:
  - name: default
    count: 3
    config:
      node.store.allow_mmap: false
    volumeClaimTemplates:
    - metadata:
        name: elasticsearch-data
      spec:
        accessModes:
        - ReadWriteOnce
        resources:
          requests:
            storage: 100Gi
        storageClassName: fast-ssd
EOF

# Create Kibana for log visualization
kubectl apply -f - <<EOF
apiVersion: kibana.k8s.elastic.co/v1
kind: Kibana
metadata:
  name: bindy-logs
  namespace: logging
spec:
  version: 8.11.0
  count: 1
  elasticsearchRef:
    name: bindy-logs
EOF
```

---

### Step 5: Configure Log Integrity Verification

```bash
# Create CronJob to verify log integrity daily
kubectl apply -f - <<EOF
apiVersion: batch/v1
kind: CronJob
metadata:
  name: log-integrity-check
  namespace: logging
spec:
  schedule: "0 2 * * *"  # Daily at 2 AM
  jobTemplate:
    spec:
      template:
        spec:
          serviceAccountName: log-integrity-checker
          containers:
          - name: integrity-check
            image: amazon/aws-cli:latest
            command:
            - /bin/bash
            - -c
            - |
              #!/bin/bash
              set -e

              # List all log files in S3
              aws s3 ls s3://bindy-audit-logs/ --recursive | awk '{print \$4}' | grep '\.log\.gz$' > /tmp/logfiles.txt

              # Verify checksums for each file
              while read logfile; do
                echo "Verifying \$logfile"
                aws s3 cp s3://bindy-audit-logs/\$logfile /tmp/\$logfile
                aws s3 cp s3://bindy-audit-logs/\$logfile.sha256 /tmp/\$logfile.sha256

                # Verify checksum
                if sha256sum -c /tmp/\$logfile.sha256; then
                  echo "✅ \$logfile: OK"
                else
                  echo "❌ \$logfile: CHECKSUM MISMATCH - POTENTIAL TAMPERING"
                  exit 1
                fi
              done < /tmp/logfiles.txt

              echo "All log files verified successfully"
          restartPolicy: Never
EOF
```

---

## References

- [SOX 404 IT General Controls](https://www.aicpa.org/interestareas/frc/assuranceadvisoryservices/sorhome.html)
- [PCI-DSS v4.0 Requirement 10.5.1](https://www.pcisecuritystandards.org/)
- [NIST SP 800-92 - Guide to Computer Security Log Management](https://csrc.nist.gov/publications/detail/sp/800-92/final)
- [AWS S3 Object Lock](https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html)
- [Kubernetes Audit Logging](https://kubernetes.io/docs/tasks/debug/debug-cluster/audit/)

---

**Last Updated:** 2025-12-17
**Next Review:** 2025-03-17 (Quarterly)
**Approved By:** Security Team, Compliance Team
