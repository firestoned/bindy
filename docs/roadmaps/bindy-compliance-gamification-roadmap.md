# Bindy Compliance & Security Gamification Roadmap

## Overview

This roadmap defines the implementation plan for adding gamified security and compliance reporting to the bindy Kubernetes operator. The system enables platform teams to define organization-wide security and compliance policies that automatically evaluate all DNS zones and generate namespace-scoped reports with scoring, badges, and compliance status.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Cluster-Scoped (Platform Team)                  │
│  ┌─────────────────────────┐    ┌─────────────────────────┐        │
│  │   DNSSecurityPolicy     │    │   DNSCompliancePolicy   │        │
│  │   (DNSSEC, transfers,   │    │   (NIST, FIPS, PCI-DSS, │        │
│  │    CAA, hardening)      │    │    SOX, Basel III)      │        │
│  └───────────┬─────────────┘    └───────────┬─────────────┘        │
└──────────────┼──────────────────────────────┼──────────────────────┘
               │                              │
               │         Propagates to        │
               │         all namespaces       │
               ▼                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                  Namespace-Scoped (Application Teams)               │
│                                                                     │
│  ┌──────────────┐                                                   │
│  │   DNSZone    │◄─────── User creates/manages zones                │
│  │   (app.x.com)│                                                   │
│  └──────┬───────┘                                                   │
│         │                                                           │
│         │ Operator evaluates zone against policies                  │
│         │                                                           │
│         ▼                                                           │
│  ┌─────────────────────┐    ┌──────────────────────┐               │
│  │  DNSSecurityReport  │    │ DNSComplianceReport  │               │
│  │  (per zone)         │    │ (per zone)           │               │
│  │  - Score: 425/500   │    │ - NIST: 92%          │               │
│  │  - Tier: Gold       │    │ - PCI-DSS: 100%      │               │
│  │  - Badges: [...]    │    │ - SOX: 88%           │               │
│  └─────────────────────┘    └──────────────────────┘               │
└─────────────────────────────────────────────────────────────────────┘
```

## Phase 1: CRD Definitions

### 1.1 DNSSecurityPolicy (Cluster-Scoped)

Platform teams define security requirements that apply to all DNS zones.

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSSecurityPolicy
metadata:
  name: enterprise-dns-security
spec:
  # Selector for which zones this applies to (empty = all)
  zoneSelector:
    matchLabels: {}
    matchExpressions: []
  
  # Environments where this policy applies
  environments:
    - production
    - staging
  
  # DNSSEC Requirements
  dnssec:
    enabled: true
    required: true
    algorithms:
      allowed:
        - ECDSAP256SHA256
        - ECDSAP384SHA384
        - ED25519
      denied:
        - RSASHA1
        - DSA
    keyRotation:
      maxAgeDays: 365
      warningDays: 30
    points: 100
    severity: critical
  
  # Zone Transfer Security
  zoneTransfers:
    mode: restricted  # restricted | disabled | tsig-only
    allowedServers: []  # Empty = deny all except explicit
    requireTSIG: true
    tsigAlgorithms:
      allowed:
        - hmac-sha256
        - hmac-sha512
      denied:
        - hmac-md5
    points: 75
    severity: critical
  
  # Query Security
  queryControls:
    recursionAllowed: false
    rateLimiting:
      enabled: true
      minQPS: 10
      maxQPS: 1000
    points: 50
    severity: high
  
  # Record Hygiene
  recordHygiene:
    # TTL requirements
    ttl:
      minimum: 300
      maximum: 86400
      points: 25
    # Wildcard restrictions
    wildcards:
      allowed: false
      exceptions:
        - "*.acme-challenge"
      points: 30
      severity: medium
    # Stale record detection
    staleRecords:
      maxAgeDays: 90
      points: 20
  
  # CAA (Certificate Authority Authorization)
  caaRecords:
    required: true
    allowedIssuers:
      - "letsencrypt.org"
      - "digicert.com"
      - "sectigo.com"
    requireWildcardTag: true
    requireIodefTag: false
    points: 50
    severity: high
  
  # DMARC/DKIM/SPF for email-enabled domains
  emailSecurity:
    enabled: true
    spf:
      required: true
      maxLookups: 10
      points: 25
    dkim:
      required: true
      minKeySize: 2048
      points: 25
    dmarc:
      required: true
      minPolicy: quarantine  # none | quarantine | reject
      points: 25
    severity: medium
  
  # Tiers and scoring
  scoring:
    tiers:
      - name: platinum
        minScore: 450
        color: "#E5E4E2"
      - name: gold
        minScore: 350
        color: "#FFD700"
      - name: silver
        minScore: 250
        color: "#C0C0C0"
      - name: bronze
        minScore: 100
        color: "#CD7F32"
      - name: unrated
        minScore: 0
        color: "#808080"
    maxPossibleScore: 500
  
  # Badges
  badges:
    - name: dnssec-champion
      description: "All zones DNSSEC signed with approved algorithms"
      icon: "🛡️"
      criteria:
        check: dnssec
        duration: 30d
    
    - name: zero-wildcards
      description: "No wildcard records in production zones"
      icon: "🎯"
      criteria:
        check: wildcards
        duration: 7d
    
    - name: transfer-fortress
      description: "Zone transfers fully secured with TSIG"
      icon: "🏰"
      criteria:
        check: zoneTransfers
        duration: 7d
    
    - name: email-guardian
      description: "Full SPF/DKIM/DMARC implementation"
      icon: "📧"
      criteria:
        check: emailSecurity
        duration: 7d
    
    - name: perfect-hygiene
      description: "No stale records, all TTLs in range"
      icon: "✨"
      criteria:
        check: recordHygiene
        duration: 14d

status:
  observedGeneration: 1
  conditions:
    - type: Ready
      status: "True"
      lastTransitionTime: "2024-12-30T10:00:00Z"
  appliedToZones: 47
  lastEvaluated: "2024-12-30T10:00:00Z"
```

### 1.2 DNSCompliancePolicy (Cluster-Scoped)

Platform teams define regulatory compliance requirements.

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSCompliancePolicy
metadata:
  name: financial-services-compliance
spec:
  # Selector for which zones this applies to
  zoneSelector:
    matchLabels:
      environment: production
  
  # Compliance frameworks to evaluate
  frameworks:
    # NIST 800-53 (Required for US Federal and many financial institutions)
    - name: nist-800-53
      version: "rev5"
      enabled: true
      controls:
        # SC-20: Secure Name/Address Resolution Service (Authoritative Source)
        - id: SC-20
          name: "Secure Name Resolution (Authoritative)"
          description: "Provide DNSSEC origin authentication and integrity verification"
          checks:
            - dnssec.enabled == true
            - dnssec.algorithm in approved_algorithms
          points: 100
          severity: critical
        
        # SC-21: Secure Name/Address Resolution Service (Recursive or Caching Resolver)
        - id: SC-21
          name: "Secure Name Resolution (Recursive)"
          description: "Request and perform DNSSEC validation"
          checks:
            - dnssec.validation == true
          points: 75
          severity: high
        
        # SC-22: Architecture and Provisioning for Name/Address Resolution Service
        - id: SC-22
          name: "DNS Architecture"
          description: "Fault-tolerant DNS implementation"
          checks:
            - zone.nameservers.count >= 2
            - zone.nameservers.distributed == true
          points: 50
          severity: high
        
        # AU-2: Audit Events
        - id: AU-2
          name: "DNS Audit Events"
          description: "DNS query logging enabled for audit trail"
          checks:
            - logging.queryLog.enabled == true
            - logging.retention >= 90d
          points: 50
          severity: medium
        
        # CM-7: Least Functionality
        - id: CM-7
          name: "Least Functionality"
          description: "Disable unnecessary DNS features"
          checks:
            - recursion.enabled == false
            - zoneTransfer.mode != "any"
          points: 50
          severity: medium
    
    # FIPS 140-2/140-3 (Cryptographic requirements)
    - name: fips-140
      version: "3"
      enabled: true
      controls:
        - id: FIPS-CRYPTO
          name: "FIPS Approved Cryptography"
          description: "Use only FIPS-approved cryptographic algorithms"
          checks:
            - dnssec.algorithm in [ECDSAP256SHA256, ECDSAP384SHA384]
            - tsig.algorithm in [hmac-sha256, hmac-sha384, hmac-sha512]
          points: 100
          severity: critical
        
        - id: FIPS-KEYLEN
          name: "Minimum Key Lengths"
          description: "Cryptographic keys meet minimum length requirements"
          checks:
            - dnssec.keySize >= 2048 (RSA) OR dnssec.keySize >= 256 (ECDSA)
            - dkim.keySize >= 2048
          points: 75
          severity: critical
    
    # PCI-DSS v4.0 (Payment Card Industry)
    - name: pci-dss
      version: "4.0"
      enabled: true
      controls:
        # Requirement 1: Network Security Controls
        - id: PCI-1.3
          name: "Network Segmentation"
          description: "Restrict inbound/outbound DNS traffic"
          checks:
            - zoneTransfer.restricted == true
            - recursion.enabled == false
          points: 75
          severity: high
        
        # Requirement 2: Secure Configurations
        - id: PCI-2.2
          name: "Secure DNS Configuration"
          description: "DNS servers configured per hardening standards"
          checks:
            - config.versionHidden == true
            - config.chaosDisabled == true
          points: 50
          severity: medium
        
        # Requirement 3: Protect Stored Data
        - id: PCI-3.5
          name: "Cryptographic Key Protection"
          description: "DNSSEC keys properly protected"
          checks:
            - dnssec.keyStorage == "secure"
            - dnssec.keyRotation.enabled == true
          points: 75
          severity: high
        
        # Requirement 10: Logging and Monitoring
        - id: PCI-10.2
          name: "Audit Logging"
          description: "Log all DNS administrative actions"
          checks:
            - logging.adminLog.enabled == true
            - logging.retention >= 365d
          points: 50
          severity: high
        
        # Requirement 11: Security Testing
        - id: PCI-11.3
          name: "Vulnerability Management"
          description: "Regular DNS security assessments"
          checks:
            - scanning.enabled == true
            - scanning.frequency <= 90d
          points: 50
          severity: medium
    
    # SOX (Sarbanes-Oxley - Change control and audit trail)
    - name: sox
      version: "2024"
      enabled: true
      controls:
        - id: SOX-CHANGE
          name: "Change Management"
          description: "All DNS changes tracked and auditable"
          checks:
            - gitops.enabled == true
            - changes.approved == true
            - changes.auditable == true
          points: 100
          severity: critical
        
        - id: SOX-AUDIT
          name: "Audit Trail"
          description: "Complete audit trail for DNS modifications"
          checks:
            - logging.changeLog.enabled == true
            - logging.retention >= 7y
          points: 75
          severity: critical
        
        - id: SOX-ACCESS
          name: "Access Controls"
          description: "RBAC enforced for DNS management"
          checks:
            - rbac.enabled == true
            - rbac.reviewed == true
          points: 75
          severity: high
    
    # Basel III/IV (Banking specific - operational resilience)
    - name: basel
      version: "iv"
      enabled: true
      controls:
        - id: BASEL-OPER
          name: "Operational Resilience"
          description: "DNS infrastructure meets resilience requirements"
          checks:
            - ha.enabled == true
            - ha.rpo <= 5m
            - ha.rto <= 15m
          points: 100
          severity: critical
        
        - id: BASEL-BCP
          name: "Business Continuity"
          description: "DNS disaster recovery capabilities"
          checks:
            - dr.enabled == true
            - dr.tested == true
            - dr.lastTestAge <= 180d
          points: 75
          severity: high
    
    # CIS DNS Benchmark
    - name: cis-dns
      version: "1.0"
      enabled: true
      controls:
        - id: CIS-1.1
          name: "BIND Version Hidden"
          description: "Hide BIND version in responses"
          checks:
            - config.versionHidden == true
          points: 25
          severity: low
        
        - id: CIS-1.2
          name: "Chaos Class Disabled"
          description: "Disable CHAOS class queries"
          checks:
            - config.chaosDisabled == true
          points: 25
          severity: low
        
        - id: CIS-2.1
          name: "Zone Transfer Restrictions"
          description: "Restrict zone transfers to authorized servers"
          checks:
            - zoneTransfer.mode == "restricted"
            - zoneTransfer.acl.defined == true
          points: 50
          severity: high
        
        - id: CIS-3.1
          name: "DNSSEC Signing"
          description: "All authoritative zones DNSSEC signed"
          checks:
            - dnssec.enabled == true
            - dnssec.signed == true
          points: 75
          severity: high
  
  # Custom organizational controls
  customControls:
    - id: ORG-DNS-001
      name: "Internal DNS Naming Convention"
      description: "DNS zones follow organizational naming standards"
      framework: internal
      checks:
        - zone.name matches "^[a-z0-9-]+\\.(internal|corp|bank)\\.example\\.com$"
      points: 25
      severity: low
    
    - id: ORG-DNS-002
      name: "Production Zone Approval"
      description: "Production zones require CAB approval annotation"
      framework: internal
      checks:
        - annotations["change.example.com/cab-approved"] exists
        - annotations["change.example.com/cab-ticket"] exists
      points: 50
      severity: medium
  
  # Reporting configuration
  reporting:
    # How to calculate overall compliance
    aggregation:
      method: weighted  # weighted | minimum | average
      weights:
        nist-800-53: 1.0
        fips-140: 1.0
        pci-dss: 1.0
        sox: 0.8
        basel: 0.8
        cis-dns: 0.5
    
    # Thresholds
    thresholds:
      compliant: 90
      partiallyCompliant: 70
      nonCompliant: 0
    
    # Grace periods for new zones
    gracePeriod:
      newZones: 7d
      controlChanges: 24h

status:
  observedGeneration: 1
  conditions:
    - type: Ready
      status: "True"
  frameworksEnabled: 6
  controlsTotal: 24
  appliedToZones: 47
```

### 1.3 DNSSecurityReport (Namespace-Scoped)

Generated per DNSZone, created in the zone's namespace.

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSSecurityReport
metadata:
  name: api-payments-example-com  # Derived from zone name
  namespace: payments
  labels:
    bindy.firestoned.io/zone: api.payments.example.com
    bindy.firestoned.io/tier: gold
    bindy.firestoned.io/environment: production
  ownerReferences:
    - apiVersion: bindy.firestoned.io/v1alpha1
      kind: DNSZone
      name: api.payments.example.com
      uid: abc-123
      operator: true
spec:
  # Reference to the zone being evaluated
  zoneRef:
    name: api.payments.example.com
    namespace: payments
  
  # Reference to the policy being applied
  policyRef:
    name: enterprise-dns-security
  
  # Evaluation interval
  evaluationInterval: 5m

status:
  # Overall scoring
  score:
    current: 425
    maximum: 500
    percentage: 85
  
  tier:
    name: gold
    color: "#FFD700"
    promotionThreshold: 450  # Points needed for next tier
    demotionThreshold: 350   # Points to maintain current tier
  
  # Trend analysis
  trend:
    direction: improving  # improving | stable | declining
    changePercent: 5
    periodDays: 7
  
  # Individual check results
  checks:
    - name: dnssec-enabled
      category: dnssec
      passed: true
      points: 100
      maxPoints: 100
      message: "DNSSEC enabled with ECDSAP256SHA256"
      lastChecked: "2024-12-30T10:00:00Z"
    
    - name: dnssec-algorithm
      category: dnssec
      passed: true
      points: 0  # Bonus points already counted above
      maxPoints: 0
      message: "Using approved algorithm ECDSAP256SHA256"
    
    - name: zone-transfer-restricted
      category: zoneTransfers
      passed: true
      points: 75
      maxPoints: 75
      message: "Zone transfers restricted with TSIG"
    
    - name: recursion-disabled
      category: queryControls
      passed: true
      points: 50
      maxPoints: 50
      message: "Recursion disabled on authoritative server"
    
    - name: minimum-ttl
      category: recordHygiene
      passed: true
      points: 25
      maxPoints: 25
      message: "All TTLs >= 300 seconds"
    
    - name: no-wildcards
      category: recordHygiene
      passed: false
      points: 0
      maxPoints: 30
      severity: medium
      message: "Wildcard record detected: *.api.payments.example.com"
      remediation: "Remove wildcard record or add to exceptions list"
    
    - name: caa-records
      category: caaRecords
      passed: true
      points: 50
      maxPoints: 50
      message: "CAA records present for letsencrypt.org"
    
    - name: spf-record
      category: emailSecurity
      passed: true
      points: 25
      maxPoints: 25
      message: "SPF record present with 8 lookups"
    
    - name: dkim-record
      category: emailSecurity
      passed: true
      points: 25
      maxPoints: 25
      message: "DKIM record present with 2048-bit key"
    
    - name: dmarc-record
      category: emailSecurity
      passed: true
      points: 25
      maxPoints: 25
      message: "DMARC policy set to quarantine"
    
    - name: stale-records
      category: recordHygiene
      passed: true
      points: 20
      maxPoints: 20
      message: "No stale records detected"
  
  # Summary by category
  categories:
    - name: dnssec
      score: 100
      maxScore: 100
      percentage: 100
    - name: zoneTransfers
      score: 75
      maxScore: 75
      percentage: 100
    - name: queryControls
      score: 50
      maxScore: 50
      percentage: 100
    - name: recordHygiene
      score: 45
      maxScore: 75
      percentage: 60
    - name: caaRecords
      score: 50
      maxScore: 50
      percentage: 100
    - name: emailSecurity
      score: 75
      maxScore: 75
      percentage: 100
  
  # Badges earned
  badges:
    earned:
      - name: dnssec-champion
        icon: "🛡️"
        earnedAt: "2024-11-15T00:00:00Z"
        description: "All zones DNSSEC signed with approved algorithms"
      - name: transfer-fortress
        icon: "🏰"
        earnedAt: "2024-12-01T00:00:00Z"
        description: "Zone transfers fully secured with TSIG"
      - name: email-guardian
        icon: "📧"
        earnedAt: "2024-12-20T00:00:00Z"
        description: "Full SPF/DKIM/DMARC implementation"
    
    inProgress:
      - name: zero-wildcards
        icon: "🎯"
        progress: 0
        requirement: "Remove wildcard records"
      - name: perfect-hygiene
        icon: "✨"
        progress: 75
        requirement: "Maintain clean records for 14 days"
    
    available:
      - name: platinum-streak
        icon: "💎"
        requirement: "Maintain platinum tier for 90 days"
  
  # Violations requiring attention
  violations:
    - check: no-wildcards
      severity: medium
      message: "Wildcard record detected: *.api.payments.example.com"
      remediation: "Remove wildcard record or request exception via platform team"
      detectedAt: "2024-12-25T08:30:00Z"
  
  # Historical scores for trending
  history:
    - date: "2024-12-30"
      score: 425
      tier: gold
    - date: "2024-12-23"
      score: 405
      tier: gold
    - date: "2024-12-16"
      score: 380
      tier: gold
    - date: "2024-12-09"
      score: 350
      tier: silver
  
  # Metadata
  lastEvaluated: "2024-12-30T10:00:00Z"
  nextEvaluation: "2024-12-30T10:05:00Z"
  policyGeneration: 3
  conditions:
    - type: Evaluated
      status: "True"
      lastTransitionTime: "2024-12-30T10:00:00Z"
      message: "Successfully evaluated against enterprise-dns-security policy"
```

### 1.4 DNSComplianceReport (Namespace-Scoped)

Generated per DNSZone for regulatory compliance status.

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSComplianceReport
metadata:
  name: api-payments-example-com
  namespace: payments
  labels:
    bindy.firestoned.io/zone: api.payments.example.com
    bindy.firestoned.io/compliance-status: compliant
    bindy.firestoned.io/environment: production
  ownerReferences:
    - apiVersion: bindy.firestoned.io/v1alpha1
      kind: DNSZone
      name: api.payments.example.com
      uid: abc-123
      operator: true
spec:
  zoneRef:
    name: api.payments.example.com
    namespace: payments
  
  policyRef:
    name: financial-services-compliance
  
  evaluationInterval: 15m

status:
  # Overall compliance status
  overallStatus: compliant  # compliant | partially_compliant | non_compliant
  overallPercentage: 94
  
  # Per-framework compliance
  frameworks:
    - name: nist-800-53
      version: "rev5"
      status: compliant
      percentage: 96
      controlsPassed: 24
      controlsTotal: 25
      controls:
        - id: SC-20
          name: "Secure Name Resolution (Authoritative)"
          status: passed
          points: 100
          maxPoints: 100
          evidence:
            - "DNSSEC enabled with ECDSAP256SHA256"
            - "Zone signed since 2024-10-15"
        - id: SC-21
          name: "Secure Name Resolution (Recursive)"
          status: passed
          points: 75
          maxPoints: 75
        - id: SC-22
          name: "DNS Architecture"
          status: passed
          points: 50
          maxPoints: 50
          evidence:
            - "3 nameservers configured"
            - "Distributed across 2 availability zones"
        - id: AU-2
          name: "DNS Audit Events"
          status: passed
          points: 50
          maxPoints: 50
        - id: CM-7
          name: "Least Functionality"
          status: passed
          points: 50
          maxPoints: 50
    
    - name: fips-140
      version: "3"
      status: compliant
      percentage: 100
      controlsPassed: 2
      controlsTotal: 2
      controls:
        - id: FIPS-CRYPTO
          name: "FIPS Approved Cryptography"
          status: passed
          points: 100
          maxPoints: 100
          evidence:
            - "DNSSEC: ECDSAP256SHA256 (FIPS approved)"
            - "TSIG: hmac-sha256 (FIPS approved)"
        - id: FIPS-KEYLEN
          name: "Minimum Key Lengths"
          status: passed
          points: 75
          maxPoints: 75
    
    - name: pci-dss
      version: "4.0"
      status: compliant
      percentage: 100
      controlsPassed: 5
      controlsTotal: 5
      controls:
        - id: PCI-1.3
          status: passed
        - id: PCI-2.2
          status: passed
        - id: PCI-3.5
          status: passed
        - id: PCI-10.2
          status: passed
        - id: PCI-11.3
          status: passed
    
    - name: sox
      version: "2024"
      status: compliant
      percentage: 100
      controlsPassed: 3
      controlsTotal: 3
      controls:
        - id: SOX-CHANGE
          name: "Change Management"
          status: passed
          points: 100
          maxPoints: 100
          evidence:
            - "GitOps enabled via FluxCD"
            - "All changes tracked in git history"
            - "CAB approval annotation present"
        - id: SOX-AUDIT
          status: passed
        - id: SOX-ACCESS
          status: passed
    
    - name: basel
      version: "iv"
      status: partially_compliant
      percentage: 75
      controlsPassed: 1
      controlsTotal: 2
      controls:
        - id: BASEL-OPER
          name: "Operational Resilience"
          status: passed
          points: 100
          maxPoints: 100
        - id: BASEL-BCP
          name: "Business Continuity"
          status: failed
          points: 0
          maxPoints: 75
          findings:
            - severity: high
              message: "DR test not performed in last 180 days"
              remediation: "Schedule DR test with platform team"
              controlRef: "BASEL-BCP"
    
    - name: cis-dns
      version: "1.0"
      status: compliant
      percentage: 100
      controlsPassed: 4
      controlsTotal: 4
  
  # Custom control results
  customControls:
    - id: ORG-DNS-001
      name: "Internal DNS Naming Convention"
      status: passed
    - id: ORG-DNS-002
      name: "Production Zone Approval"
      status: passed
      evidence:
        - "CAB ticket: CHG0012345"
        - "Approved: 2024-12-01"
  
  # All findings/violations
  findings:
    - id: finding-001
      framework: basel
      controlId: BASEL-BCP
      severity: high
      status: open
      title: "DR Test Overdue"
      description: "Business continuity DR test not performed within required 180-day window"
      remediation: "Schedule DR test with platform team. Contact dns-platform@example.com"
      detectedAt: "2024-12-15T00:00:00Z"
      dueDate: "2025-01-15T00:00:00Z"
  
  # Audit trail for compliance evidence
  auditTrail:
    - timestamp: "2024-12-30T10:00:00Z"
      event: evaluation_completed
      details: "Evaluated against financial-services-compliance policy v3"
    - timestamp: "2024-12-29T14:30:00Z"
      event: control_passed
      details: "SOX-CHANGE: GitOps verification passed"
    - timestamp: "2024-12-15T00:00:00Z"
      event: finding_created
      details: "BASEL-BCP: DR test overdue finding created"
  
  # For regulatory reporting
  attestation:
    lastFullAssessment: "2024-12-01T00:00:00Z"
    nextAssessmentDue: "2025-03-01T00:00:00Z"
    assessor: "platform-team"
    attestationId: "ATT-2024-12-001"
  
  # Historical compliance for trending
  history:
    - date: "2024-12-30"
      overallPercentage: 94
      status: compliant
    - date: "2024-12-23"
      overallPercentage: 94
      status: compliant
    - date: "2024-12-16"
      overallPercentage: 88
      status: partially_compliant
  
  lastEvaluated: "2024-12-30T10:00:00Z"
  nextEvaluation: "2024-12-30T10:15:00Z"
  conditions:
    - type: Evaluated
      status: "True"
      lastTransitionTime: "2024-12-30T10:00:00Z"
```

---

## Phase 2: Implementation Milestones

### Milestone 1: Core CRD Structure (Week 1-2)

**Tasks:**
1. [ ] Define CRD schemas in Rust using kube-rs
2. [ ] Implement CRD validation webhooks
3. [ ] Create CRD installation manifests
4. [ ] Write unit tests for CRD serialization/deserialization
5. [ ] Document CRD fields and usage

**Files to create:**
```
src/
├── crds/
│   ├── mod.rs
│   ├── dns_security_policy.rs
│   ├── dns_compliance_policy.rs
│   ├── dns_security_report.rs
│   └── dns_compliance_report.rs
├── schemas/
│   ├── security_checks.rs
│   └── compliance_frameworks.rs
```

**Acceptance Criteria:**
- CRDs can be installed via `kubectl apply`
- Custom resources can be created and validated
- Status subresources work correctly

---

### Milestone 2: Policy Evaluation Engine (Week 3-4)

**Tasks:**
1. [ ] Implement check evaluation framework
2. [ ] Create evaluators for each security check category
3. [ ] Create evaluators for each compliance framework
4. [ ] Implement scoring calculation
5. [ ] Implement tier determination logic
6. [ ] Add badge evaluation logic

**Files to create:**
```
src/
├── evaluators/
│   ├── mod.rs
│   ├── security/
│   │   ├── mod.rs
│   │   ├── dnssec.rs
│   │   ├── zone_transfer.rs
│   │   ├── query_controls.rs
│   │   ├── record_hygiene.rs
│   │   ├── caa.rs
│   │   └── email_security.rs
│   └── compliance/
│       ├── mod.rs
│       ├── nist.rs
│       ├── fips.rs
│       ├── pci_dss.rs
│       ├── sox.rs
│       ├── basel.rs
│       └── cis.rs
├── scoring/
│   ├── mod.rs
│   ├── calculator.rs
│   ├── tiers.rs
│   └── badges.rs
```

**Acceptance Criteria:**
- All security checks evaluate correctly
- All compliance controls evaluate correctly
- Scores calculate accurately
- Tiers assigned correctly based on thresholds

---

### Milestone 3: Operators (Week 5-6)

**Tasks:**
1. [ ] Implement DNSSecurityPolicy operator (watches policies, triggers evaluations)
2. [ ] Implement DNSCompliancePolicy operator
3. [ ] Implement DNSSecurityReport operator (creates/updates reports per zone)
4. [ ] Implement DNSComplianceReport operator
5. [ ] Add reconciliation logic for policy changes
6. [ ] Implement garbage collection for orphaned reports

**Files to create:**
```
src/
├── operators/
│   ├── mod.rs
│   ├── security_policy.rs
│   ├── compliance_policy.rs
│   ├── security_report.rs
│   └── compliance_report.rs
```

**Acceptance Criteria:**
- Reports automatically created when zones are created
- Reports update when zones or policies change
- Orphaned reports cleaned up when zones deleted
- Policy changes propagate to all affected zones

---

### Milestone 4: Integration with Existing bindy (Week 7-8)

**Tasks:**
1. [ ] Integrate with existing DNSZone reconciliation
2. [ ] Add zone data extraction for evaluation
3. [ ] Implement BIND9 configuration checks
4. [ ] Add bindcar API integration for runtime checks
5. [ ] Implement DNSSEC key inspection
6. [ ] Add zone transfer configuration checks

**Integration Points:**
- DNSZone status → Security/Compliance evaluation input
- BIND9 named.conf → Configuration compliance checks
- bindcar RNDC API → Runtime state verification
- DNSSEC keys → Algorithm and rotation checks

**Acceptance Criteria:**
- Evaluations use real zone data
- BIND9 configuration verified
- Runtime state checked via bindcar
- All checks produce accurate results

---

### Milestone 5: Metrics & Observability (Week 9)

**Tasks:**
1. [ ] Export Prometheus metrics for scores and compliance
2. [ ] Create Grafana dashboard templates
3. [ ] Add Kubernetes events for significant changes
4. [ ] Implement alerting annotations

**Metrics to export:**
```
# Security metrics
bindy_security_score{zone, namespace, tier}
bindy_security_check_passed{zone, namespace, check}
bindy_security_violations_total{zone, namespace, severity}
bindy_badge_earned{zone, namespace, badge}

# Compliance metrics
bindy_compliance_percentage{zone, namespace, framework}
bindy_compliance_control_passed{zone, namespace, framework, control}
bindy_compliance_findings_total{zone, namespace, framework, severity}
bindy_compliance_status{zone, namespace}  # 0=non_compliant, 1=partial, 2=compliant
```

**Acceptance Criteria:**
- Metrics exported to /metrics endpoint
- Grafana dashboard shows all zones and scores
- Alerts fire on compliance violations
- Events visible in `kubectl describe`

---

### Milestone 6: CLI & Reporting (Week 10)

**Tasks:**
1. [ ] Add `bindyctl compliance` subcommand
2. [ ] Implement report generation (PDF, JSON, CSV)
3. [ ] Add leaderboard display
4. [ ] Create audit export functionality

**CLI Commands:**
```bash
# View security status
bindyctl security status -n payments
bindyctl security report api.payments.example.com -o json

# View compliance status
bindyctl compliance status -n payments
bindyctl compliance report api.payments.example.com --framework pci-dss
bindyctl compliance audit-export --from 2024-01-01 --to 2024-12-31

# Leaderboards
bindyctl leaderboard security --top 10
bindyctl leaderboard compliance --framework sox

# Badge management
bindyctl badges list -n payments
bindyctl badges history api.payments.example.com
```

**Acceptance Criteria:**
- CLI provides all compliance information
- Reports exportable in multiple formats
- Leaderboards display correctly
- Audit exports suitable for auditors

---

### Milestone 7: Documentation & Examples (Week 11)

**Tasks:**
1. [ ] Write user documentation
2. [ ] Create example policies for different environments
3. [ ] Document compliance framework mappings
4. [ ] Create runbooks for common scenarios
5. [ ] Add architecture diagrams

**Documentation Structure:**
```
docs/
├── compliance/
│   ├── overview.md
│   ├── security-policy-reference.md
│   ├── compliance-policy-reference.md
│   ├── frameworks/
│   │   ├── nist-800-53.md
│   │   ├── fips-140.md
│   │   ├── pci-dss.md
│   │   ├── sox.md
│   │   └── basel.md
│   ├── gamification.md
│   └── reporting.md
├── examples/
│   ├── financial-services/
│   ├── healthcare/
│   └── government/
└── runbooks/
    ├── achieving-platinum.md
    ├── remediation-guide.md
    └── audit-preparation.md
```

---

### Milestone 8: Testing & Hardening (Week 12)

**Tasks:**
1. [ ] Comprehensive unit tests for all evaluators
2. [ ] Integration tests with real BIND9
3. [ ] End-to-end tests for full workflow
4. [ ] Performance testing with 100+ zones
5. [ ] Security review of CRD permissions

**Test Coverage Targets:**
- Unit tests: 90%+ coverage on evaluators
- Integration tests: All check types
- E2E tests: Policy → Zone → Report flow

---

## Phase 3: Future Enhancements

### 3.1 Aggregation CRDs

```yaml
# Namespace-level summary
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSComplianceSummary
metadata:
  name: payments-summary
  namespace: payments
status:
  totalZones: 12
  averageSecurityScore: 412
  averageCompliancePercentage: 92
  tierDistribution:
    platinum: 3
    gold: 7
    silver: 2
  frameworkCompliance:
    nist-800-53: 95%
    pci-dss: 100%
    sox: 88%
```

### 3.2 Policy Exceptions

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSPolicyException
metadata:
  name: legacy-zone-exception
  namespace: legacy
spec:
  zoneRef:
    name: legacy.internal.example.com
  exceptions:
    - check: dnssec-enabled
      reason: "Legacy system incompatible with DNSSEC"
      approvedBy: "security-team"
      expiresAt: "2025-06-30T00:00:00Z"
      ticketRef: "SEC-12345"
```

### 3.3 Webhook Notifications

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSComplianceNotification
metadata:
  name: slack-notifications
spec:
  events:
    - tierChange
    - badgeEarned
    - violationDetected
    - complianceDropBelow: 90
  destinations:
    - type: slack
      webhook: "${SLACK_WEBHOOK_URL}"
      channel: "#dns-compliance"
    - type: pagerduty
      routingKey: "${PD_ROUTING_KEY}"
      severity: critical
      events: [violationDetected]
```

### 3.4 Multi-Cluster Federation

Aggregate compliance across multiple clusters for enterprise dashboards.

---

## Security Check Reference

| Check | Category | Max Points | Severity | Description |
|-------|----------|------------|----------|-------------|
| dnssec-enabled | dnssec | 100 | critical | Zone is DNSSEC signed |
| dnssec-algorithm | dnssec | 0 | critical | Uses approved algorithms |
| dnssec-key-rotation | dnssec | 25 | high | Keys rotated within policy |
| zone-transfer-restricted | zoneTransfers | 75 | critical | Transfers limited to ACL |
| tsig-enabled | zoneTransfers | 25 | high | TSIG authentication enabled |
| recursion-disabled | queryControls | 50 | high | No recursion on authoritative |
| rate-limiting | queryControls | 25 | medium | Query rate limiting enabled |
| minimum-ttl | recordHygiene | 25 | low | All TTLs >= minimum |
| no-wildcards | recordHygiene | 30 | medium | No wildcard records |
| no-stale-records | recordHygiene | 20 | low | No records older than threshold |
| caa-present | caaRecords | 50 | high | CAA records configured |
| spf-valid | emailSecurity | 25 | medium | Valid SPF record |
| dkim-present | emailSecurity | 25 | medium | DKIM record present |
| dmarc-enforced | emailSecurity | 25 | medium | DMARC policy enforced |

---

## Compliance Framework Control Mapping

### NIST 800-53 Rev 5

| Control | Family | DNS Relevance | Check Implementation |
|---------|--------|---------------|---------------------|
| SC-20 | System Communications | DNSSEC signing | dnssec.enabled, dnssec.algorithm |
| SC-21 | System Communications | DNSSEC validation | dnssec.validation |
| SC-22 | System Communications | DNS redundancy | nameservers.count, distribution |
| AU-2 | Audit | Query logging | logging.enabled, retention |
| CM-7 | Configuration | Minimal services | recursion.disabled, transfers.restricted |

### PCI-DSS v4.0

| Requirement | DNS Relevance | Check Implementation |
|-------------|---------------|---------------------|
| 1.3 | Network segmentation | zoneTransfer.restricted, recursion.disabled |
| 2.2 | Secure configuration | versionHidden, chaosDisabled |
| 3.5 | Key protection | dnssec.keyStorage, keyRotation |
| 10.2 | Audit logging | adminLog.enabled, retention |
| 11.3 | Security testing | scanning.enabled, frequency |

---

## Getting Started

### Prerequisites

- bindy operator v0.x.x+ installed
- bindcar sidecar deployed with BIND9 instances
- Prometheus/Grafana for metrics (optional)

### Quick Start

1. Install compliance CRDs:
```bash
kubectl apply -f https://raw.githubusercontent.com/firestoned/bindy/main/config/crds/compliance/
```

2. Create a security policy:
```bash
kubectl apply -f examples/policies/enterprise-security-policy.yaml
```

3. Create a compliance policy:
```bash
kubectl apply -f examples/policies/financial-services-compliance.yaml
```

4. View reports:
```bash
kubectl get dnssecurityreports -A
kubectl get dnscompliancereports -A
```

---

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines on adding new:
- Security checks
- Compliance framework controls
- Badges
- Integrations
