# Incident Response Playbooks - Bindy DNS Operator

**Version:** 1.0
**Last Updated:** 2025-12-17
**Owner:** Security Team
**Compliance:** SOX 404, PCI-DSS 12.10.1, Basel III

---

## Table of Contents

- [Overview](#overview)
- [Incident Classification](#incident-classification)
- [Response Team](#response-team)
- [Communication Protocols](#communication-protocols)
- [Playbook Index](#playbook-index)
- [Playbooks](#playbooks)
  - [P1: Critical Vulnerability Detected](#p1-critical-vulnerability-detected)
  - [P2: Compromised Operator Pod](#p2-compromised-operator-pod)
  - [P3: DNS Service Outage](#p3-dns-service-outage)
  - [P4: RNDC Key Compromise](#p4-rndc-key-compromise)
  - [P5: Unauthorized DNS Changes](#p5-unauthorized-dns-changes)
  - [P6: DDoS Attack](#p6-ddos-attack)
  - [P7: Supply Chain Compromise](#p7-supply-chain-compromise)
- [Post-Incident Activities](#post-incident-activities)

---

## Overview

This document provides step-by-step incident response playbooks for security incidents involving the Bindy DNS Operator. Each playbook follows the NIST Incident Response Lifecycle: Preparation, Detection & Analysis, Containment, Eradication, Recovery, and Post-Incident Activity.

### Objectives

1. **Rapid Response**: Minimize time between detection and containment
2. **Clear Procedures**: Provide step-by-step guidance for responders
3. **Minimize Impact**: Reduce blast radius and prevent escalation
4. **Evidence Preservation**: Maintain audit trail for forensics and compliance
5. **Continuous Improvement**: Learn from incidents to strengthen defenses

---

## Incident Classification

### Severity Levels

| Severity | Definition | Response Time | Escalation |
|----------|------------|---------------|------------|
| 🔴 **CRITICAL** | Complete service outage, data breach, or active exploitation | **Immediate** (< 15 min) | CISO, CTO, VP Engineering |
| 🟠 **HIGH** | Degraded service, vulnerability with known exploit, unauthorized access | **< 1 hour** | Security Lead, Engineering Manager |
| 🟡 **MEDIUM** | Vulnerability without exploit, suspicious activity, minor service impact | **< 4 hours** | Security Team, On-Call Engineer |
| 🔵 **LOW** | Informational findings, potential issues, no immediate risk | **< 24 hours** | Security Team |

---

## Response Team

### Roles and Responsibilities

| Role | Responsibilities | Contact |
|------|------------------|---------|
| **Incident Commander** | Overall coordination, decision-making, stakeholder communication | On-call rotation |
| **Security Lead** | Threat analysis, forensics, remediation guidance | security@firestoned.io |
| **Platform Engineer** | Kubernetes cluster operations, pod management | platform@firestoned.io |
| **DNS Engineer** | BIND9 expertise, zone management | dns-team@firestoned.io |
| **Compliance Officer** | Regulatory reporting, evidence collection | compliance@firestoned.io |
| **Communications** | Internal/external communication, customer notifications | comms@firestoned.io |

### On-Call Rotation

- **Primary**: Security Lead (24/7 PagerDuty)
- **Secondary**: Platform Engineer (escalation)
- **Tertiary**: CTO (executive escalation)

---

## Communication Protocols

### Internal Communication

**War Room (Incident > MEDIUM):**
- **Slack Channel**: `#incident-[YYYY-MM-DD]-[number]`
- **Video Call**: Zoom war room (pinned in channel)
- **Status Updates**: Every 30 minutes during active incident

**Status Page:**
- Update `status.firestoned.io` for customer-impacting incidents
- Templates: Investigating → Identified → Monitoring → Resolved

### External Communication

**Regulatory Reporting (CRITICAL incidents only):**
- **PCI-DSS**: Notify acquiring bank within 24 hours if cardholder data compromised
- **SOX**: Document incident for quarterly IT controls audit
- **Basel III**: Report cyber risk event to risk management committee

**Customer Notification:**
- **Criteria**: Data breach, prolonged outage (> 4 hours), SLA violation
- **Channel**: Email to registered contacts, status page
- **Timeline**: Initial notification within 2 hours, updates every 4 hours

---

## Playbook Index

| ID | Playbook | Severity | Trigger |
|----|----------|----------|---------|
| **P1** | Critical Vulnerability Detected | 🔴 CRITICAL | GitHub issue, CVE alert, security scan |
| **P2** | Compromised Operator Pod | 🔴 CRITICAL | Anomalous behavior, unauthorized access |
| **P3** | DNS Service Outage | 🔴 CRITICAL | All BIND9 pods down, DNS queries failing |
| **P4** | RNDC Key Compromise | 🔴 CRITICAL | Key leaked, unauthorized RNDC access |
| **P5** | Unauthorized DNS Changes | 🟠 HIGH | Unexpected zone modifications |
| **P6** | DDoS Attack | 🟠 HIGH | Query flood, resource exhaustion |
| **P7** | Supply Chain Compromise | 🔴 CRITICAL | Malicious commit, compromised dependency |

---

## Playbooks

---

## P1: Critical Vulnerability Detected

**Severity:** 🔴 CRITICAL
**Response Time:** Immediate (< 15 minutes)
**SLA:** Patch deployed within 24 hours

### Trigger

- Daily security scan detects CRITICAL vulnerability (CVSS 9.0-10.0)
- GitHub Security Advisory published for Bindy dependency
- CVE announced with active exploitation in the wild
- Automated GitHub issue created: `[SECURITY] CRITICAL vulnerability detected`

### Detection

```bash
# Automated detection via GitHub Actions
# Workflow: .github/workflows/security-scan.yaml
# Frequency: Daily at 00:00 UTC

# Manual check:
cargo audit --deny warnings
trivy image ghcr.io/firestoned/bindy:latest --severity CRITICAL,HIGH
```

### Response Procedure

#### Phase 1: Detection & Analysis (T+0 to T+15 min)

**Step 1.1: Acknowledge Incident**
```bash
# Acknowledge PagerDuty alert
# Create Slack war room: #incident-[date]-vuln-[CVE-ID]
```

**Step 1.2: Assess Vulnerability**
```bash
# Review GitHub issue or security scan results
# Questions to answer:
# - What is the vulnerable component? (dependency, base image, etc.)
# - What is the CVSS score and attack vector?
# - Is there a known exploit (Exploit-DB, Metasploit)?
# - Is Bindy actually vulnerable (code path reachable)?
```

**Step 1.3: Check Production Exposure**
```bash
# Verify if vulnerable version is deployed
kubectl get deploy -n dns-system bindy -o jsonpath='{.spec.template.spec.containers[0].image}'

# Check image digest
kubectl get pods -n dns-system -l app.kubernetes.io/name=bindy -o jsonpath='{.items[0].spec.containers[0].image}'

# Compare with vulnerable version from security advisory
```

**Step 1.4: Determine Impact**
- **If Bindy is NOT vulnerable** (code path not reachable):
  - Update to patched version at next release (non-urgent)
  - Document exception in ../../../SECURITY.md
  - Close incident as FALSE POSITIVE

- **If Bindy IS vulnerable** (exploitable in production):
  - **PROCEED TO CONTAINMENT** (Phase 2)

---

#### Phase 2: Containment (T+15 min to T+1 hour)

**Step 2.1: Isolate Vulnerable Pods (if actively exploited)**
```bash
# Scale down operator to prevent further exploitation
kubectl scale deploy -n dns-system bindy --replicas=0

# NOTE: This stops DNS updates but does NOT affect DNS queries
# BIND9 continues serving existing zones
```

**Step 2.2: Review Audit Logs**
```bash
# Check for signs of exploitation
kubectl logs -n dns-system -l app.kubernetes.io/name=bindy --tail=1000 | grep -i "error\|panic\|exploit"

# Review Kubernetes audit logs (if available)
# Look for: Unusual API calls, secret reads, privilege escalation attempts
```

**Step 2.3: Assess Blast Radius**
- **Operator compromised?** Check for unauthorized DNS changes, secret reads
- **BIND9 affected?** Check if RNDC keys were stolen
- **Data exfiltration?** Review network logs for unusual egress traffic

---

#### Phase 3: Eradication (T+1 hour to T+24 hours)

**Step 3.1: Apply Patch**

**Option A: Update Dependency (Rust crate)**
```bash
# Update specific dependency
cargo update -p <vulnerable-package>

# Verify fix
cargo audit

# Run tests
cargo test

# Build new image
docker build -t ghcr.io/firestoned/bindy:hotfix-$(date +%s) .

# Push to registry
docker push ghcr.io/firestoned/bindy:hotfix-$(date +%s)
```

**Option B: Update Base Image**
```bash
# Update Dockerfile to latest Chainguard image
# docker/Dockerfile:
FROM cgr.dev/chainguard/static:latest-dev  # Use latest digest

# Rebuild and push
docker build -t ghcr.io/firestoned/bindy:hotfix-$(date +%s) .
docker push ghcr.io/firestoned/bindy:hotfix-$(date +%s)
```

**Option C: Apply Workaround (if no patch available)**
- Disable vulnerable feature flag
- Add input validation to prevent exploit
- Document workaround in ../../../SECURITY.md

**Step 3.2: Verify Fix**
```bash
# Scan patched image
trivy image ghcr.io/firestoned/bindy:hotfix-$(date +%s) --severity CRITICAL,HIGH

# Expected: No CRITICAL vulnerabilities found
```

**Step 3.3: Emergency Release**
```bash
# Tag release
git tag -s hotfix-v0.1.1 -m "Security hotfix: CVE-XXXX-XXXXX"
git push origin hotfix-v0.1.1

# Trigger release workflow
# Verify signed commits, SBOM generation, vulnerability scans pass
```

---

#### Phase 4: Recovery (T+24 hours to T+48 hours)

**Step 4.1: Deploy Patched Version**
```bash
# Update deployment manifest (GitOps)
# deploy/operator/deployment.yaml:
spec:
  template:
    spec:
      containers:
      - name: bindy
        image: ghcr.io/firestoned/bindy:hotfix-v0.1.1  # Patched version

# Apply via FluxCD (GitOps) or manually
kubectl apply -f deploy/operator/deployment.yaml

# Verify rollout
kubectl rollout status deploy/bindy -n dns-system

# Confirm pods running patched version
kubectl get pods -n dns-system -l app.kubernetes.io/name=bindy -o jsonpath='{.items[0].spec.containers[0].image}'
```

**Step 4.2: Verify Service Health**
```bash
# Check operator logs
kubectl logs -n dns-system -l app.kubernetes.io/name=bindy --tail=100

# Verify reconciliation working
kubectl get dnszones --all-namespaces
kubectl describe dnszone -n team-web example-com

# Test DNS resolution
dig @<bind9-ip> example.com
```

**Step 4.3: Run Security Scans**
```bash
# Full security scan
cargo audit
trivy image ghcr.io/firestoned/bindy:hotfix-v0.1.1

# Expected: All clear
```

---

#### Phase 5: Post-Incident (T+48 hours to T+1 week)

**Step 5.1: Document Incident**
- Update `CHANGELOG.md` with hotfix details
- Document root cause in incident report
- Update [SECURITY.md](../../../SECURITY.md) if needed (known issues, exceptions)

**Step 5.2: Notify Stakeholders**
- Update status page: "Resolved - Security patch deployed"
- Send email to compliance team (attach incident report)
- Notify customers if required (data breach, SLA violation)

**Step 5.3: Post-Incident Review (PIR)**
- **What went well?** (Detection, response time, communication)
- **What could improve?** (Patch process, testing, automation)
- **Action items:** (Update playbook, add monitoring, improve defenses)

**Step 5.4: Update Metrics**
- MTTR (Mean Time To Remediate): ____ hours
- SLA compliance: ✅ Met / ❌ Missed
- Update vulnerability dashboard

---

### Success Criteria

- ✅ Patch deployed within 24 hours
- ✅ No exploitation detected in production
- ✅ Service availability maintained (or minimal downtime)
- ✅ All security scans pass post-patch
- ✅ Incident documented and reported to compliance

---

## P2: Compromised Operator Pod

**Severity:** 🔴 CRITICAL
**Response Time:** Immediate (< 15 minutes)
**Impact:** Unauthorized DNS modifications, secret theft, lateral movement

### Trigger

- Anomalous operator behavior (unexpected API calls, network traffic)
- Unauthorized modifications to DNS zones
- Security alert from SIEM or IDS
- Pod logs show suspicious activity (reverse shell, file downloads)

### Detection

```bash
# Monitor operator logs for anomalies
kubectl logs -n dns-system -l app.kubernetes.io/name=bindy --tail=500 | grep -E "(shell|wget|curl|nc|bash)"

# Check for unexpected processes in pod
kubectl exec -n dns-system <operator-pod> -- ps aux

# Review Kubernetes audit logs
# Look for: Unusual secret reads, excessive API calls, privilege escalation attempts
```

### Response Procedure

#### Phase 1: Detection & Analysis (T+0 to T+15 min)

**Step 1.1: Confirm Compromise**
```bash
# Check operator logs
kubectl logs -n dns-system <operator-pod> --tail=1000 > /tmp/operator-logs.txt

# Indicators of compromise (IOCs):
# - Reverse shell activity (nc, bash -i, /dev/tcp/)
# - File downloads (wget, curl to suspicious domains)
# - Privilege escalation attempts (sudo, setuid)
# - Crypto mining (high CPU, connections to mining pools)
```

**Step 1.2: Assess Impact**
```bash
# Check for unauthorized DNS changes
kubectl get dnszones --all-namespaces -o yaml > /tmp/dnszones-snapshot.yaml

# Compare with known good state (GitOps repo)
diff /tmp/dnszones-snapshot.yaml /path/to/gitops/dnszones/

# Check for secret reads
# Review Kubernetes audit logs for GET /api/v1/namespaces/dns-system/secrets/*
```

---

#### Phase 2: Containment (T+15 min to T+1 hour)

**Step 2.1: Isolate Operator Pod**
```bash
# Apply network policy to block all egress (prevent data exfiltration)
kubectl apply -f - <<EOF
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: bindy-operator-quarantine
  namespace: dns-system
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: bindy
  policyTypes:
  - Egress
  egress: []  # Block all egress
EOF

# Delete compromised pod (force recreation)
kubectl delete pod -n dns-system <operator-pod> --force --grace-period=0
```

**Step 2.2: Rotate Credentials**
```bash
# Rotate RNDC key (if potentially stolen)
# Generate new key
tsig-keygen -a hmac-sha256 rndc-key > /tmp/new-rndc-key.conf

# Update secret
kubectl create secret generic rndc-key-new \
  --from-file=rndc.key=/tmp/new-rndc-key.conf \
  -n dns-system \
  --dry-run=client -o yaml | kubectl apply -f -

# Update BIND9 pods to use new key (restart required)
kubectl rollout restart statefulset/bind9-primary -n dns-system
kubectl rollout restart statefulset/bind9-secondary -n dns-system

# Delete old secret
kubectl delete secret rndc-key -n dns-system
```

**Step 2.3: Preserve Evidence**
```bash
# Save pod logs before deletion
kubectl logs -n dns-system <operator-pod> --all-containers > /tmp/forensics/operator-logs-$(date +%s).txt

# Capture pod manifest
kubectl get pod -n dns-system <operator-pod> -o yaml > /tmp/forensics/operator-pod-manifest.yaml

# Save Kubernetes events
kubectl get events -n dns-system --sort-by='.lastTimestamp' > /tmp/forensics/events.txt

# Export audit logs (if available)
# - ServiceAccount API calls
# - Secret access logs
# - DNS zone modifications
```

---

#### Phase 3: Eradication (T+1 hour to T+4 hours)

**Step 3.1: Root Cause Analysis**
```bash
# Analyze logs for initial compromise vector
# Common vectors:
# - Vulnerability in operator code (RCE, memory corruption)
# - Compromised dependency (malicious crate)
# - Supply chain attack (malicious image)
# - Misconfigured RBAC (excessive permissions)

# Check image provenance
kubectl get pod -n dns-system <operator-pod> -o jsonpath='{.spec.containers[0].image}'

# Verify image signature and SBOM
# If signature invalid or SBOM shows unexpected dependencies → supply chain attack
```

**Step 3.2: Patch Vulnerability**
- If operator code vulnerability: Apply patch (see P1)
- If supply chain attack: Investigate upstream, rollback to known good image
- If RBAC misconfiguration: Fix RBAC, re-run verification script

**Step 3.3: Scan for Backdoors**
```bash
# Scan all images for malware
trivy image ghcr.io/firestoned/bindy:latest --scanners vuln,secret,misconfig

# Check for unauthorized SSH keys, cron jobs, persistence mechanisms
kubectl exec -n dns-system <new-operator-pod> -- ls -la /root/.ssh/
kubectl exec -n dns-system <new-operator-pod> -- cat /etc/crontab
```

---

#### Phase 4: Recovery (T+4 hours to T+24 hours)

**Step 4.1: Deploy Clean Operator**
```bash
# Verify image integrity
# - Signed commits in Git history
# - Signed container image with provenance
# - Clean vulnerability scan

# Deploy patched operator
kubectl rollout restart deploy/bindy -n dns-system

# Remove quarantine network policy
kubectl delete networkpolicy bindy-operator-quarantine -n dns-system

# Verify health
kubectl get pods -n dns-system -l app.kubernetes.io/name=bindy
kubectl logs -n dns-system -l app.kubernetes.io/name=bindy --tail=100
```

**Step 4.2: Verify DNS Zones**
```bash
# Restore DNS zones from GitOps (if unauthorized changes detected)
# 1. Revert changes in Git
# 2. Force FluxCD reconciliation
flux reconcile kustomization bindy-system --with-source

# Verify all zones match expected state
kubectl get dnszones --all-namespaces -o yaml | diff - /path/to/gitops/dnszones/
```

**Step 4.3: Validate Service**
```bash
# Test DNS resolution
dig @<bind9-ip> example.com

# Verify operator reconciliation
kubectl get dnszones --all-namespaces
kubectl describe dnszone -n team-web example-com | grep "Ready.*True"
```

---

#### Phase 5: Post-Incident (T+24 hours to T+1 week)

**Step 5.1: Forensic Analysis**
- Engage forensics team if required
- Analyze preserved logs for IOCs
- Timeline of compromise (initial access → lateral movement → exfiltration)

**Step 5.2: Notify Stakeholders**
- **Compliance:** Report to SOX/PCI-DSS auditors (security incident)
- **Customers:** If DNS records were modified or data exfiltrated
- **Regulators:** If required by Basel III (cyber risk event reporting)

**Step 5.3: Improve Defenses**
- **Short-term:** Implement missing network policies (L-1)
- **Medium-term:** Add runtime security monitoring (Falco, Tetragon)
- **Long-term:** Implement admission operator for image verification

**Step 5.4: Update Documentation**
- Update incident playbook with lessons learned
- Document new IOCs for detection rules
- Update threat model (docs/security/threat-model.md)

---

### Success Criteria

- ✅ Compromised pod isolated within 15 minutes
- ✅ No lateral movement to other pods/namespaces
- ✅ Credentials rotated (RNDC keys)
- ✅ Root cause identified and patched
- ✅ DNS service fully restored with verified integrity
- ✅ Forensic evidence preserved for investigation

---

## P3: DNS Service Outage

**Severity:** 🔴 CRITICAL
**Response Time:** Immediate (< 15 minutes)
**Impact:** All DNS queries failing, service unavailable

### Trigger

- All BIND9 pods down (CrashLoopBackOff, OOMKilled)
- DNS queries timing out
- Monitoring alert: "DNS service unavailable"
- Customer reports: "Cannot resolve domain names"

### Response Procedure

#### Phase 1: Detection & Analysis (T+0 to T+10 min)

**Step 1.1: Confirm Outage**
```bash
# Test DNS resolution
dig @<bind9-loadbalancer-ip> example.com

# Check pod status
kubectl get pods -n dns-system -l app.kubernetes.io/name=bind9

# Check service endpoints
kubectl get svc -n dns-system bind9-dns -o wide
kubectl get endpoints -n dns-system bind9-dns
```

**Step 1.2: Identify Root Cause**
```bash
# Check pod logs
kubectl logs -n dns-system <bind9-pod> --tail=200

# Common root causes:
# - OOMKilled (memory exhaustion)
# - CrashLoopBackOff (configuration error, missing ConfigMap)
# - ImagePullBackOff (registry issue, image not found)
# - Pending (insufficient resources, node failure)

# Check events
kubectl describe pod -n dns-system <bind9-pod>
```

---

#### Phase 2: Containment & Quick Fix (T+10 min to T+30 min)

**Scenario A: OOMKilled (Memory Exhaustion)**
```bash
# Increase memory limit
kubectl patch statefulset bind9-primary -n dns-system -p '
spec:
  template:
    spec:
      containers:
      - name: bind9
        resources:
          limits:
            memory: "512Mi"  # Increase from 256Mi
'

# Restart pods
kubectl rollout restart statefulset/bind9-primary -n dns-system
```

**Scenario B: Configuration Error**
```bash
# Check ConfigMap
kubectl get cm -n dns-system bind9-config -o yaml

# Common issues:
# - Syntax error in named.conf
# - Missing zone file
# - Invalid RNDC key

# Fix configuration (update ConfigMap)
kubectl edit cm bind9-config -n dns-system

# Restart pods to apply new config
kubectl rollout restart statefulset/bind9-primary -n dns-system
```

**Scenario C: Image Pull Failure**
```bash
# Check image pull secret
kubectl get secret -n dns-system ghcr-pull-secret

# Verify image exists
docker pull ghcr.io/firestoned/bindy:latest

# If image missing, rollback to previous version
kubectl rollout undo statefulset/bind9-primary -n dns-system
```

---

#### Phase 3: Recovery (T+30 min to T+2 hours)

**Step 3.1: Verify Service Restoration**
```bash
# Check all pods healthy
kubectl get pods -n dns-system -l app.kubernetes.io/name=bind9

# Test DNS resolution (all zones)
dig @<bind9-ip> example.com
dig @<bind9-ip> test.example.com

# Check service endpoints
kubectl get endpoints -n dns-system bind9-dns
# Should show all healthy pod IPs
```

**Step 3.2: Validate Data Integrity**
```bash
# Verify all zones loaded
kubectl exec -n dns-system <bind9-pod> -- rndc status

# Check zone serial numbers (ensure no data loss)
dig @<bind9-ip> example.com SOA

# Compare with expected serial (from GitOps)
```

---

#### Phase 4: Post-Incident (T+2 hours to T+1 week)

**Step 4.1: Root Cause Analysis**
- **Why did BIND9 exhaust memory?** (Too many zones, memory leak, query flood)
- **Why did configuration break?** (Operator bug, bad CRD validation, manual change)
- **Why did image pull fail?** (Registry downtime, authentication issue)

**Step 4.2: Preventive Measures**
- **Add horizontal pod autoscaling** (HPA based on CPU/memory)
- **Add health checks** (liveness/readiness probes for BIND9)
- **Add configuration validation** (admission webhook for ConfigMaps)
- **Add chaos engineering tests** (kill pods, exhaust memory, test recovery)

**Step 4.3: Update SLO/SLA**
- Document actual downtime
- Calculate availability percentage
- Update SLA reports for customers

---

### Success Criteria

- ✅ DNS service restored within 30 minutes
- ✅ All zones serving correctly
- ✅ No data loss (zone serial numbers match)
- ✅ Root cause identified and documented
- ✅ Preventive measures implemented

---

## P4: RNDC Key Compromise

**Severity:** 🔴 CRITICAL
**Response Time:** Immediate (< 15 minutes)
**Impact:** Attacker can control BIND9 (reload zones, freeze service, etc.)

### Trigger

- RNDC key found in logs, Git commit, or public repository
- Unauthorized RNDC commands detected (audit logs)
- Security scan detects secret in code or environment variables

### Response Procedure

#### Phase 1: Detection & Analysis (T+0 to T+15 min)

**Step 1.1: Confirm Compromise**
```bash
# Search for leaked key in logs
grep -r "rndc-key" /var/log/ /tmp/

# Search Git history for accidentally committed keys
git log -S "rndc-key" --all

# Check GitHub secret scanning alerts
# GitHub → Security → Secret scanning alerts
```

**Step 1.2: Assess Impact**
```bash
# Check BIND9 logs for unauthorized RNDC commands
kubectl logs -n dns-system <bind9-pod> --tail=1000 | grep "rndc command"

# Check for malicious activity:
# - rndc freeze (stop zone updates)
# - rndc reload (load malicious zone)
# - rndc querylog on (enable debug logging for reconnaissance)
```

---

#### Phase 2: Containment (T+15 min to T+1 hour)

**Step 2.1: Rotate RNDC Key (Emergency)**
```bash
# Generate new RNDC key
tsig-keygen -a hmac-sha256 rndc-key-emergency > /tmp/rndc-key-new.conf

# Extract key from generated file
cat /tmp/rndc-key-new.conf

# Create new Kubernetes secret
kubectl create secret generic rndc-key-rotated \
  --from-literal=key="<new-key-here>" \
  -n dns-system

# Update operator deployment to use new secret
kubectl set env deploy/bindy -n dns-system RNDC_KEY_SECRET=rndc-key-rotated

# Update BIND9 StatefulSets
kubectl set volume statefulset/bind9-primary -n dns-system \
  --add --name=rndc-key \
  --type=secret \
  --secret-name=rndc-key-rotated \
  --mount-path=/etc/bind/rndc.key \
  --sub-path=rndc.key

# Restart all BIND9 pods
kubectl rollout restart statefulset/bind9-primary -n dns-system
kubectl rollout restart statefulset/bind9-secondary -n dns-system

# Delete compromised secret
kubectl delete secret rndc-key -n dns-system
```

**Step 2.2: Block Network Access (if attacker active)**
```bash
# Apply network policy to block RNDC port (9530) from external access
kubectl apply -f - <<EOF
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: bind9-rndc-deny-external
  namespace: dns-system
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: bind9
  policyTypes:
  - Ingress
  ingress:
  # Allow DNS queries (port 53)
  - from:
    - namespaceSelector: {}
    ports:
    - protocol: UDP
      port: 53
    - protocol: TCP
      port: 53
  # Allow RNDC only from operator
  - from:
    - podSelector:
        matchLabels:
          app.kubernetes.io/name: bindy
    ports:
    - protocol: TCP
      port: 9530
EOF
```

---

#### Phase 3: Eradication (T+1 hour to T+4 hours)

**Step 3.1: Remove Leaked Secrets**

**If secret in Git:**
```bash
# Remove from Git history (use BFG Repo-Cleaner)
git clone --mirror git@github.com:firestoned/bindy.git
bfg --replace-text passwords.txt bindy.git
cd bindy.git
git reflog expire --expire=now --all && git gc --prune=now --aggressive
git push --force

# Notify all team members to re-clone repository
```

**If secret in logs:**
```bash
# Rotate logs immediately
kubectl delete pod -n dns-system <operator-pod>  # Forces log rotation

# Purge old logs from log aggregation system
# (Depends on logging backend: Elasticsearch, CloudWatch, etc.)
```

**Step 3.2: Audit All Secret Access**
```bash
# Review Kubernetes audit logs
# Find all ServiceAccounts that read rndc-key secret in last 30 days
# Check if any unauthorized access occurred
```

---

#### Phase 4: Recovery (T+4 hours to T+24 hours)

**Step 4.1: Verify Key Rotation**
```bash
# Test RNDC with new key
kubectl exec -n dns-system <operator-pod> -- \
  rndc -s <bind9-ip> -k /etc/bindy/rndc/rndc.key status

# Expected: Command succeeds with new key

# Test DNS service
dig @<bind9-ip> example.com

# Expected: DNS queries work normally
```

**Step 4.2: Update Documentation**
```bash
# Update secret rotation procedure in ../../../SECURITY.md
# Document rotation frequency (e.g., quarterly, or after incident)
```

---

#### Phase 5: Post-Incident (T+24 hours to T+1 week)

**Step 5.1: Implement Secret Detection**
```bash
# Add pre-commit hook to detect secrets
# .git/hooks/pre-commit:
#!/bin/bash
git diff --cached --name-only | xargs grep -E "(rndc-key|BEGIN RSA PRIVATE KEY)" && {
  echo "ERROR: Secret detected in commit. Aborting."
  exit 1
}

# Enable GitHub secret scanning (if not already enabled)
# GitHub → Settings → Code security and analysis → Secret scanning: Enable
```

**Step 5.2: Automate Key Rotation**
```bash
# Implement automated quarterly key rotation
# Add CronJob to generate and rotate keys every 90 days
```

**Step 5.3: Improve Secret Management**
- Consider external secret manager (HashiCorp Vault, AWS Secrets Manager)
- Implement secret access audit trail (H-3)
- Add alerts on unexpected secret reads

---

### Success Criteria

- ✅ RNDC key rotated within 1 hour
- ✅ Leaked secret removed from all locations
- ✅ No unauthorized RNDC commands executed
- ✅ DNS service fully functional with new key
- ✅ Secret detection mechanisms implemented
- ✅ Audit trail reviewed and documented

---

## P5: Unauthorized DNS Changes

**Severity:** 🟠 HIGH
**Response Time:** < 1 hour
**Impact:** DNS records modified without approval, potential traffic redirection

### Trigger

- Unexpected changes to DNSZone custom resources
- DNS records pointing to unknown IP addresses
- GitOps detects drift (actual state ≠ desired state)
- User reports: "DNS not resolving correctly"

### Response Procedure

#### Phase 1: Detection & Analysis (T+0 to T+30 min)

**Step 1.1: Identify Unauthorized Changes**
```bash
# Get current DNSZone state
kubectl get dnszones --all-namespaces -o yaml > /tmp/current-dnszones.yaml

# Compare with GitOps source of truth
diff /tmp/current-dnszones.yaml /path/to/gitops/dnszones/

# Check Kubernetes audit logs for who made changes
# Look for: kubectl apply, kubectl edit, kubectl patch on DNSZone resources
```

**Step 1.2: Assess Impact**
```bash
# Which zones were modified?
# What records changed? (A, CNAME, MX, TXT)
# Where is traffic being redirected?

# Test DNS resolution
dig @<bind9-ip> suspicious-domain.com

# Check if malicious IP is reachable
nslookup suspicious-domain.com
curl -I http://<suspicious-ip>/
```

---

#### Phase 2: Containment (T+30 min to T+1 hour)

**Step 2.1: Revert Unauthorized Changes**
```bash
# Revert to known good state (GitOps)
kubectl apply -f /path/to/gitops/dnszones/team-web/example-com.yaml

# Force operator reconciliation
kubectl annotate dnszone -n team-web example-com \
  reconcile-at="$(date +%s)" --overwrite

# Verify zone restored
kubectl get dnszone -n team-web example-com -o yaml | grep "status"
```

**Step 2.2: Revoke Access (if compromised user)**
```bash
# Identify user who made unauthorized change (from audit logs)
# Example: user=alice, namespace=team-web

# Remove user's RBAC permissions
kubectl delete rolebinding dnszone-editor-alice -n team-web

# Force user to re-authenticate
# (Depends on authentication provider: OIDC, LDAP, etc.)
```

---

#### Phase 3: Eradication (T+1 hour to T+4 hours)

**Step 3.1: Root Cause Analysis**
- **Compromised user credentials?** Rotate passwords, check for MFA bypass
- **RBAC misconfiguration?** User had excessive permissions
- **Operator bug?** Operator reconciled incorrect state
- **Manual kubectl change?** Bypassed GitOps workflow

**Step 3.2: Fix Root Cause**
```bash
# Example: RBAC was too permissive
# Fix RoleBinding to limit scope
kubectl apply -f - <<EOF
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: dnszone-editor-alice
  namespace: team-web
subjects:
- kind: User
  name: alice
roleRef:
  kind: Role
  name: dnszone-editor  # Role only allows CRUD on DNSZones, not deletion
  apiGroup: rbac.authorization.k8s.io
EOF
```

---

#### Phase 4: Recovery (T+4 hours to T+24 hours)

**Step 4.1: Verify DNS Integrity**
```bash
# Test all zones
for zone in $(kubectl get dnszones --all-namespaces -o jsonpath='{.items[*].spec.zoneName}'); do
  echo "Testing $zone"
  dig @<bind9-ip> $zone SOA
done

# Expected: All zones resolve correctly with expected serial numbers
```

**Step 4.2: Restore User Access (if revoked)**
```bash
# After confirming user is not compromised, restore access
kubectl apply -f /path/to/gitops/rbac/team-web/alice-rolebinding.yaml
```

---

#### Phase 5: Post-Incident (T+24 hours to T+1 week)

**Step 5.1: Implement Admission Webhooks**
```bash
# Add ValidatingWebhook to prevent suspicious DNS changes
# Example: Block A records pointing to private IPs (RFC 1918)
# Example: Require approval for changes to critical zones (*.bank.com)
```

**Step 5.2: Add Drift Detection**
```bash
# Implement automated GitOps drift detection
# Alert if cluster state ≠ Git state for > 5 minutes
# Tool: FluxCD notification operator + Slack webhook
```

**Step 5.3: Enforce GitOps Workflow**
```bash
# Remove direct kubectl access for users
# Require all changes via Pull Requests in GitOps repo
# Implement branch protection: 2+ reviewers required
```

---

### Success Criteria

- ✅ Unauthorized changes reverted within 1 hour
- ✅ Root cause identified (user, RBAC, operator bug)
- ✅ Access revoked/fixed to prevent recurrence
- ✅ DNS integrity verified (all zones correct)
- ✅ Drift detection and admission webhooks implemented

---

## P6: DDoS Attack

**Severity:** 🟠 HIGH
**Response Time:** < 1 hour
**Impact:** DNS service degraded or unavailable due to query flood

### Trigger

- High query rate (> 10,000 QPS per pod)
- BIND9 pods high CPU/memory utilization
- Monitoring alert: "DNS response time elevated"
- Users report: "DNS slow or timing out"

### Response Procedure

#### Phase 1: Detection & Analysis (T+0 to T+15 min)

**Step 1.1: Confirm DDoS Attack**
```bash
# Check BIND9 query rate
kubectl exec -n dns-system <bind9-pod> -- rndc status | grep "queries resulted"

# Check pod resource utilization
kubectl top pods -n dns-system -l app.kubernetes.io/name=bind9

# Analyze query patterns
kubectl exec -n dns-system <bind9-pod> -- rndc dumpdb -zones
kubectl exec -n dns-system <bind9-pod> -- cat /var/cache/bind/named_dump.db | head -100
```

**Step 1.2: Identify Attack Type**
- **Volumetric attack:** Millions of queries from many IPs (botnet)
- **Amplification attack:** Abusing AXFR or ANY queries
- **NXDOMAIN attack:** Flood of queries for non-existent domains

---

#### Phase 2: Containment (T+15 min to T+1 hour)

**Step 2.1: Enable Rate Limiting (BIND9)**
```bash
# Update BIND9 configuration
kubectl edit cm -n dns-system bind9-config

# Add rate-limit directive:
# named.conf:
rate-limit {
    responses-per-second 10;
    nxdomains-per-second 5;
    errors-per-second 5;
    window 10;
};

# Restart BIND9 to apply config
kubectl rollout restart statefulset/bind9-primary -n dns-system
```

**Step 2.2: Scale Up BIND9 Pods**
```bash
# Horizontal scaling
kubectl scale statefulset bind9-secondary -n dns-system --replicas=5

# Vertical scaling (if needed)
kubectl patch statefulset bind9-primary -n dns-system -p '
spec:
  template:
    spec:
      containers:
      - name: bind9
        resources:
          requests:
            cpu: "1000m"
            memory: "1Gi"
          limits:
            cpu: "2000m"
            memory: "2Gi"
'
```

**Step 2.3: Block Malicious IPs (if identifiable)**
```bash
# If attack comes from small number of IPs, block at firewall/LoadBalancer
# Example: AWS Network ACL, GCP Cloud Armor

# Add NetworkPolicy to block specific CIDRs
kubectl apply -f - <<EOF
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: block-attacker-ips
  namespace: dns-system
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: bind9
  policyTypes:
  - Ingress
  ingress:
  - from:
    - ipBlock:
        cidr: 0.0.0.0/0
        except:
        - 192.0.2.0/24  # Attacker CIDR
        - 198.51.100.0/24  # Attacker CIDR
EOF
```

---

#### Phase 3: Eradication (T+1 hour to T+4 hours)

**Step 3.1: Engage DDoS Protection Service**
```bash
# If volumetric attack (> 10 Gbps), edge DDoS protection required
# Options:
# - CloudFlare DNS (proxy DNS through CloudFlare)
# - AWS Shield Advanced
# - Google Cloud Armor

# Migrate DNS to CloudFlare (example):
# 1. Add zone to CloudFlare
# 2. Update NS records at domain registrar
# 3. Configure CloudFlare → Origin (BIND9 backend)
```

**Step 3.2: Implement Response Rate Limiting (RRL)**
```bash
# BIND9 RRL configuration (more aggressive)
rate-limit {
    responses-per-second 5;
    nxdomains-per-second 2;
    referrals-per-second 5;
    nodata-per-second 5;
    errors-per-second 2;
    window 5;
    log-only no;  # Actually drop packets (not just log)
    slip 2;  # Send truncated response every 2nd rate-limited query
    max-table-size 20000;
};
```

---

#### Phase 4: Recovery (T+4 hours to T+24 hours)

**Step 4.1: Monitor Service Health**
```bash
# Check query rate stabilized
kubectl exec -n dns-system <bind9-pod> -- rndc status

# Check pod resource utilization
kubectl top pods -n dns-system

# Test DNS resolution
dig @<bind9-ip> example.com

# Expected: Normal response times (< 50ms)
```

**Step 4.2: Scale Down (if attack subsided)**
```bash
# Return to normal replica count
kubectl scale statefulset bind9-secondary -n dns-system --replicas=2
```

---

#### Phase 5: Post-Incident (T+24 hours to T+1 week)

**Step 5.1: Implement Permanent DDoS Protection**
- **Edge DDoS protection:** CloudFlare, AWS Shield, Google Cloud Armor
- **Anycast DNS:** Distribute load across multiple geographic locations
- **Autoscaling:** HPA based on query rate, CPU, memory

**Step 5.2: Improve Monitoring**
```bash
# Add Prometheus metrics for query rate
# Add alerts:
# - Query rate > 5000 QPS per pod
# - NXDOMAIN rate > 50%
# - Response time > 100ms (p95)
```

**Step 5.3: Document Attack Details**
- Attack duration: ____ hours
- Peak query rate: ____ QPS
- Attack type: Volumetric / Amplification / NXDOMAIN
- Attack sources: IP ranges, ASNs, geolocation
- Mitigation effectiveness: RRL / Scaling / Edge protection

---

### Success Criteria

- ✅ DNS service restored within 1 hour
- ✅ Query rate normalized (< 1000 QPS per pod)
- ✅ Response times < 50ms (p95)
- ✅ Permanent DDoS protection implemented (CloudFlare, etc.)
- ✅ Autoscaling and monitoring in place

---

## P7: Supply Chain Compromise

**Severity:** 🔴 CRITICAL
**Response Time:** Immediate (< 15 minutes)
**Impact:** Malicious code in operator, backdoor access, data exfiltration

### Trigger

- Malicious commit detected in Git history
- Dependency vulnerability with active exploit (supply chain attack)
- Image signature verification fails
- SBOM shows unexpected dependency or binary

### Response Procedure

#### Phase 1: Detection & Analysis (T+0 to T+30 min)

**Step 1.1: Identify Compromised Component**
```bash
# Check Git commit signatures
git log --show-signature | grep "BAD signature"

# Check image provenance
docker buildx imagetools inspect ghcr.io/firestoned/bindy:latest --format '{{ json .Provenance }}'

# Expected: Valid signature from GitHub Actions

# Check SBOM for unexpected dependencies
# Download SBOM from GitHub release artifacts
curl -L https://github.com/firestoned/bindy/releases/download/v1.0.0/sbom.json | jq '.components[].name'

# Expected: Only known dependencies from Cargo.toml
```

**Step 1.2: Assess Impact**
```bash
# Check if compromised version deployed to production
kubectl get deploy -n dns-system bindy -o jsonpath='{.spec.template.spec.containers[0].image}'

# If compromised image is running → **CRITICAL** (proceed to containment)
# If compromised image NOT deployed → **HIGH** (patch and prevent deployment)
```

---

#### Phase 2: Containment (T+30 min to T+2 hours)

**Step 2.1: Isolate Compromised Operator**
```bash
# Scale down compromised operator
kubectl scale deploy -n dns-system bindy --replicas=0

# Apply network policy to block egress (prevent exfiltration)
kubectl apply -f - <<EOF
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: bindy-quarantine
  namespace: dns-system
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: bindy
  policyTypes:
  - Egress
  egress: []
EOF
```

**Step 2.2: Preserve Evidence**
```bash
# Save pod logs
kubectl logs -n dns-system -l app.kubernetes.io/name=bindy --all-containers > /tmp/forensics/operator-logs.txt

# Save compromised image for analysis
docker pull ghcr.io/firestoned/bindy:compromised-tag
docker save ghcr.io/firestoned/bindy:compromised-tag > /tmp/forensics/compromised-image.tar

# Scan for malware
trivy image ghcr.io/firestoned/bindy:compromised-tag --scanners vuln,secret,misconfig
```

**Step 2.3: Rotate All Credentials**
```bash
# Rotate RNDC keys
# See P4: RNDC Key Compromise

# Rotate ServiceAccount tokens (if operator potentially stole them)
kubectl delete secret -n dns-system $(kubectl get secrets -n dns-system | grep bindy-token | awk '{print $1}')
kubectl rollout restart deploy/bindy -n dns-system  # Will generate new token
```

---

#### Phase 3: Eradication (T+2 hours to T+8 hours)

**Step 3.1: Root Cause Analysis**
```bash
# Identify how malicious code was introduced:
# - Compromised developer account?
# - Malicious dependency in Cargo.toml?
# - Compromised CI/CD pipeline?
# - Insider threat?

# Check Git history for unauthorized commits
git log --all --show-signature

# Check CI/CD logs for anomalies
# GitHub Actions → Workflow runs → Check for unusual activity

# Check dependency sources
cargo tree | grep -v "crates.io"
# Expected: All dependencies from crates.io (no git dependencies)
```

**Step 3.2: Clean Git History (if malicious commit)**
```bash
# Identify malicious commit
git log --all --oneline | grep "suspicious"

# Revert malicious commit
git revert <malicious-commit-sha>

# Force push (if malicious code not yet merged to main)
git push --force origin feature-branch

# If malicious code merged to main → Contact GitHub Security
# Request help with incident response and forensics
```

**Step 3.3: Rebuild from Clean Source**
```bash
# Checkout known good commit (before compromise)
git checkout <last-known-good-commit>

# Rebuild binaries
cargo build --release

# Rebuild container image
docker build -t ghcr.io/firestoned/bindy:clean-$(date +%s) .

# Scan for vulnerabilities
cargo audit
trivy image ghcr.io/firestoned/bindy:clean-$(date +%s)

# Expected: All clean

# Push to registry
docker push ghcr.io/firestoned/bindy:clean-$(date +%s)
```

---

#### Phase 4: Recovery (T+8 hours to T+24 hours)

**Step 4.1: Deploy Clean Operator**
```bash
# Update deployment manifest
kubectl set image deploy/bindy -n dns-system \
  bindy=ghcr.io/firestoned/bindy:clean-$(date +%s)

# Remove quarantine network policy
kubectl delete networkpolicy bindy-quarantine -n dns-system

# Verify health
kubectl get pods -n dns-system -l app.kubernetes.io/name=bindy
kubectl logs -n dns-system -l app.kubernetes.io/name=bindy --tail=100
```

**Step 4.2: Verify Service Integrity**
```bash
# Test DNS resolution
dig @<bind9-ip> example.com

# Verify all zones correct
kubectl get dnszones --all-namespaces -o yaml | diff - /path/to/gitops/dnszones/

# Expected: No drift
```

---

#### Phase 5: Post-Incident (T+24 hours to T+1 week)

**Step 5.1: Implement Supply Chain Security**
```bash
# Enable Dependabot security updates
# .github/dependabot.yml:
version: 2
updates:
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "daily"
    open-pull-requests-limit: 10

# Pin dependencies by hash (Cargo.lock already does this)
# Verify Cargo.lock is committed to Git

# Implement image signing verification
# Add admission operator (Kyverno, OPA Gatekeeper) to verify image signatures before deployment
```

**Step 5.2: Implement Code Review Enhancements**
```bash
# Require 2+ reviewers for all PRs (already implemented)
# Add CODEOWNERS for sensitive files:
# .github/CODEOWNERS:
/Cargo.toml @security-team
/Cargo.lock @security-team
/Dockerfile @security-team
/.github/workflows/ @security-team
```

**Step 5.3: Notify Stakeholders**
- **Users:** Email notification about supply chain incident
- **Regulators:** Report to SOX/PCI-DSS auditors (security incident)
- **GitHub Security:** Report compromised dependency or account

**Step 5.4: Update Documentation**
- Document supply chain incident in threat model
- Update supply chain security controls in ../../../SECURITY.md
- Add supply chain attack scenarios to threat model

---

### Success Criteria

- ✅ Compromised component identified within 30 minutes
- ✅ Malicious code removed from Git history
- ✅ Clean operator deployed within 24 hours
- ✅ All credentials rotated
- ✅ Supply chain security improvements implemented
- ✅ Stakeholders notified and incident documented

---

## Post-Incident Activities

### Post-Incident Review (PIR) Template

**Incident ID:** INC-YYYY-MM-DD-XXXX
**Severity:** 🔴 / 🟠 / 🟡 / 🔵
**Incident Commander:** [Name]
**Date:** [YYYY-MM-DD]
**Duration:** [Detection to resolution]

#### Summary

[1-2 paragraph summary of incident]

#### Timeline

| Time | Event | Action Taken |
|------|-------|--------------|
| T+0 | [Detection event] | [Action] |
| T+15min | [Analysis] | [Action] |
| T+1hr | [Containment] | [Action] |
| T+4hr | [Eradication] | [Action] |
| T+24hr | [Recovery] | [Action] |

#### Root Cause

[Detailed root cause analysis]

#### What Went Well ✅

- [Detection was fast]
- [Playbook was clear]
- [Team communication was effective]

#### What Could Improve ❌

- [Monitoring gaps]
- [Playbook outdated]
- [Slow escalation]

#### Action Items

| Action | Owner | Due Date | Status |
|--------|-------|----------|--------|
| [Implement network policies] | Platform Team | 2025-01-15 | 🔄 In Progress |
| [Add monitoring alerts] | SRE Team | 2025-01-10 | ✅ Complete |
| [Update playbook] | Security Team | 2025-01-05 | ✅ Complete |

#### Metrics

- **MTTD (Mean Time To Detect):** [X] minutes
- **MTTR (Mean Time To Remediate):** [X] hours
- **SLA Met:** ✅ Yes / ❌ No
- **Downtime:** [X] minutes
- **Customers Impacted:** [N]

---

## References

- [NIST Incident Response Guide (SP 800-61)](https://csrc.nist.gov/publications/detail/sp/800-61/rev-2/final)
- [SANS Incident Handler's Handbook](https://www.sans.org/white-papers/33901/)
- [PCI-DSS v4.0 Requirement 12.10](https://www.pcisecuritystandards.org/)
- [Kubernetes Security Incident Response](https://kubernetes.io/docs/tasks/debug/debug-cluster/)

---

**Last Updated:** 2025-12-17
**Next Review:** 2025-03-17 (Quarterly)
**Approved By:** Security Team
