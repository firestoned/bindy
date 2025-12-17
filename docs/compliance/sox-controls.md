# SOX Compliance - IT General Controls (ITGC)

**Document Version:** 1.0
**Last Updated:** 2025-12-17
**Scope:** Bindy DNS Operator for Kubernetes
**Regulatory Framework:** Sarbanes-Oxley Act (SOX) Section 404

---

## Executive Summary

This document maps the Bindy DNS Operator's technical controls to Sarbanes-Oxley (SOX) IT General Controls (ITGC) requirements. SOX compliance is mandatory for publicly traded companies and their critical IT systems that support financial reporting.

Bindy is designed for use in regulated banking environments where DNS infrastructure supports financial applications and must maintain:
- **Change Management Controls** - All changes tracked, reviewed, and auditable
- **Access Controls** - Principle of least privilege enforced
- **Data Integrity** - Immutable audit trails
- **Disaster Recovery** - Documented backup and recovery procedures

---

## SOX ITGC Control Domains

### 1. Change Management (CM)

**Requirement:** All changes to production systems must be authorized, tested, and documented.

#### CM-1: Source Code Version Control
- **Control:** All code changes tracked in Git with immutable commit history
- **Evidence:** [GitHub repository](https://github.com/firestoned/bindy)
- **Implementation:**
  - All commits signed with GPG keys
  - Pull request approval required for main branch
  - Automated changelog generation via `CHANGELOG.md`
  - Git tags for all releases

#### CM-2: Automated Testing
- **Control:** All changes validated via automated tests before deployment
- **Evidence:** `.github/workflows/test.yml`
- **Implementation:**
  - Unit tests: `cargo test`
  - Integration tests: `make kind-integration-test`
  - Clippy linting: `cargo clippy -- -D warnings`
  - Security audit: `cargo audit`

#### CM-3: Code Review Process
- **Control:** All production changes peer-reviewed
- **Evidence:** GitHub branch protection rules
- **Implementation:**
  - Minimum 1 reviewer required for PR approval
  - Automated checks must pass before merge
  - Review checklist in PR template

#### CM-4: Release Management
- **Control:** Production releases are tagged, signed, and traceable
- **Evidence:** Git tags, container image signatures
- **Implementation:**
  - Semantic versioning (vX.Y.Z)
  - Container images signed with cosign/sigstore
  - Release notes generated from changelog
  - Immutable container image tags

#### CM-5: Rollback Procedures
- **Control:** Documented procedures for reverting changes
- **Evidence:** `docs/operations/rollback.md`
- **Implementation:**
  - Git revert procedures
  - Kubernetes rollout undo procedures
  - Version pinning in Helm/Kustomize

### 2. Access Controls (AC)

**Requirement:** Access to systems and data restricted based on business need and role.

#### AC-1: Role-Based Access Control (RBAC)
- **Control:** Kubernetes RBAC enforces least privilege
- **Evidence:** `deploy/rbac/`
- **Implementation:**
  - Service account with minimal permissions
  - ClusterRole scoped to DNS resources only
  - No cluster-admin privileges required
  - Namespace isolation supported

#### AC-2: Multi-Tenancy Support
- **Control:** Tenant isolation via Kubernetes namespaces
- **Evidence:** `src/reconcilers/dnszone.rs`, namespace filtering
- **Implementation:**
  - Resources scoped to namespaces
  - Cross-namespace access denied by default
  - Owner references enforce lifecycle boundaries

#### AC-3: Secret Management
- **Control:** Sensitive data encrypted at rest and in transit
- **Evidence:** Kubernetes Secret resources, TLS configuration
- **Implementation:**
  - BIND9 TSIG keys stored in Kubernetes Secrets
  - Secrets encrypted at rest (etcd encryption)
  - TLS for all control plane communication
  - No secrets in container images or logs

#### AC-4: Audit Logging
- **Control:** All privileged operations logged
- **Evidence:** `src/reconcilers/*.rs`, tracing instrumentation
- **Implementation:**
  - Structured logging via `tracing` crate
  - All reconciliation events logged with resource names
  - Kubernetes audit logs capture API calls
  - Log retention per organizational policy

### 3. Data Integrity (DI)

**Requirement:** Data accuracy and completeness maintained throughout processing.

#### DI-1: Declarative Configuration
- **Control:** Infrastructure as Code prevents configuration drift
- **Evidence:** CRD schemas in `deploy/crds/`
- **Implementation:**
  - All DNS configuration in CustomResources
  - Schema validation via OpenAPI v3
  - Immutable spec fields where appropriate
  - Status subresource for reconciliation state

#### DI-2: Input Validation
- **Control:** All input validated against schemas
- **Evidence:** `src/crd.rs`, JSON Schema validation
- **Implementation:**
  - CRD schema validation on admission
  - Rust type safety enforces correctness
  - DNS name validation (RFC 1035)
  - IP address validation (RFC 791, RFC 4291)

#### DI-3: Idempotent Operations
- **Control:** Operations can be safely retried without side effects
- **Evidence:** `src/reconcilers/*.rs`
- **Implementation:**
  - All reconcilers idempotent
  - State stored in Kubernetes API (source of truth)
  - No external state dependencies
  - Safe to replay reconciliation events

#### DI-4: Data Consistency
- **Control:** DNS zones synchronized across primary and secondary servers
- **Evidence:** `src/bind9_resources.rs`, ConfigMap/Secret generation
- **Implementation:**
  - Zone serial numbers auto-incremented
  - AXFR/IXFR for zone transfers
  - Health checks verify consistency
  - Status conditions reflect sync state

### 4. Computer Operations (CO)

**Requirement:** IT operations follow documented, repeatable procedures.

#### CO-1: Automated Deployment
- **Control:** Deployments repeatable and auditable
- **Evidence:** `.github/workflows/`, `Makefile`
- **Implementation:**
  - CI/CD pipelines fully automated
  - No manual production changes
  - Deployment artifacts versioned and signed
  - GitOps-ready (FluxCD/ArgoCD compatible)

#### CO-2: Monitoring and Alerting
- **Control:** System health monitored continuously
- **Evidence:** Prometheus metrics, health checks
- **Implementation:**
  - `/health` and `/ready` endpoints
  - Prometheus metrics exported
  - Kubernetes liveness/readiness probes
  - Status conditions in CRD status

#### CO-3: Backup and Recovery
- **Control:** Critical data backed up and recoverable
- **Evidence:** `docs/operations/disaster-recovery.md`
- **Implementation:**
  - DNS configuration in Kubernetes (etcd backed up)
  - Zone files in ConfigMaps (versioned)
  - CRD resources stored in Git (GitOps)
  - Documented recovery procedures

#### CO-4: Incident Response
- **Control:** Security incidents documented and resolved
- **Evidence:** `SECURITY.md`, GitHub Security Advisories
- **Implementation:**
  - Security vulnerability reporting process
  - CVE monitoring via `cargo audit`
  - Dependency update procedures
  - Incident post-mortem template

### 5. Segregation of Duties (SD)

**Requirement:** No single person controls all aspects of critical transactions.

#### SD-1: Code Review Separation
- **Control:** Code author cannot approve their own changes
- **Evidence:** GitHub branch protection settings
- **Implementation:**
  - PR author cannot merge own PR
  - Minimum 1 independent reviewer
  - CODEOWNERS file defines review requirements

#### SD-2: Deployment Approval
- **Control:** Production deployments require approval
- **Evidence:** `.github/workflows/`, environment protection rules
- **Implementation:**
  - Manual approval gates for production
  - Separate staging and production environments
  - Different service accounts for environments

---

## Compliance Evidence Collection

### Automated Evidence
1. **Git History** - All changes tracked with author, timestamp, commit message
2. **CI/CD Logs** - Build and test results for every change
3. **Container Registry** - Signed images with provenance
4. **Kubernetes Audit Logs** - All API operations logged
5. **Application Logs** - Reconciliation events and errors

### Audit Procedures
1. **Quarterly Access Reviews** - Review RBAC permissions
2. **Change Log Reviews** - Verify all changes documented
3. **Security Scan Reviews** - Review `cargo audit` results
4. **Incident Log Reviews** - Review security advisories

### Artifacts for Auditors
- `CHANGELOG.md` - Complete change history
- `deploy/rbac/` - Access control definitions
- `.github/workflows/` - Automated controls
- `docs/compliance/` - This documentation
- Git tags and release notes

---

## Control Testing

### Annual Control Tests
1. **CM-1 Test:** Verify all production code in version control
2. **CM-2 Test:** Deploy change without tests (should fail)
3. **AC-1 Test:** Attempt unauthorized operation (should deny)
4. **DI-1 Test:** Apply invalid CRD (should reject)
5. **CO-1 Test:** Verify deployment reproducibility

### Evidence Collection
- Screenshots of control failures
- CI/CD pipeline execution logs
- RBAC denial events from audit logs
- Schema validation errors

---

## Remediation Procedures

### Non-Compliance Scenarios
1. **Unauthorized Change Detected**
   - Revert change immediately
   - Investigate access logs
   - Document in incident log
   - Review and strengthen controls

2. **Failed Security Scan**
   - Create security advisory
   - Patch or mitigate vulnerability
   - Update dependencies
   - Document in changelog

3. **Missing Changelog Entry**
   - Add entry retroactively with explanation
   - Review PR template and process
   - Additional training if needed

---

## References

- [SOX Section 404](https://www.sec.gov/rules/final/33-8238.htm) - Management's Report on Internal Control
- [COSO Framework](https://www.coso.org/) - Internal Control Integrated Framework
- [COBIT 2019](https://www.isaca.org/resources/cobit) - IT Governance Framework
- [PCAOB Auditing Standard 2201](https://pcaobus.org/oversight/standards/auditing-standards/details/AS2201) - Audit of Internal Control

---

## Document Control

| Version | Date       | Author          | Changes                          |
|---------|------------|-----------------|----------------------------------|
| 1.0     | 2025-12-17 | Erick Bourgeois | Initial SOX controls documentation |

---

**Approval Signatures:**

_This section to be completed during formal compliance review._

- **CISO:** _________________________ Date: _____________
- **Compliance Officer:** _________________________ Date: _____________
- **External Auditor:** _________________________ Date: _____________
