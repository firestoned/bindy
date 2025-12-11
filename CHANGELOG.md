# Changelog

All notable changes to this project will be documented in this file.

## [2025-12-11 01:36] - Fix Mermaid Diagram Zoom/Pan JavaScript Error

**Author:** Erick Bourgeois

### Changed
- `docs/mermaid-init.js`: Added null checks before calling `addEventListener()` on theme switcher buttons to prevent JavaScript errors when elements don't exist

### Why
The mermaid-init.js script was attempting to add event listeners to theme switcher elements without checking if they exist first. This caused a JavaScript error: "can't access property 'addEventListener', document.getElementById(...) is null". The error prevented zoom and pan functionality from working on Mermaid diagrams in the generated documentation.

### Impact
- [x] Documentation only
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only

**Benefits:**
1. **Zoom and pan now works** - Mermaid diagrams are now interactive without JavaScript errors
2. **Better error handling** - Script safely handles missing theme switcher elements
3. **Improved UX** - Users can zoom and pan large architecture diagrams

## [2025-12-11 01:30] - Complete DNS Record Type Guide Documentation

**Author:** Erick Bourgeois

### Added
- `docs/src/guide/aaaa-records.md`: Comprehensive guide for IPv6 AAAA records
  - Dual-stack configuration examples (IPv4 + IPv6)
  - IPv6 address format variations
  - Load balancing with multiple AAAA records
  - Common use cases (web servers, API endpoints, mail servers)
  - Best practices and troubleshooting
- `docs/src/guide/mx-records.md`: Complete MX (Mail Exchange) records guide
  - Priority-based failover configuration
  - Load balancing with equal priorities
  - Google Workspace and Microsoft 365 configurations
  - Self-hosted mail server setup
  - FQDN requirements and common mistakes
  - Mail delivery testing procedures
- `docs/src/guide/txt-records.md`: Detailed TXT records guide
  - SPF (Sender Policy Framework) configuration
  - DKIM (DomainKeys Identified Mail) setup
  - DMARC (Domain-based Message Authentication) policies
  - Domain verification for various services (Google, Microsoft, Atlassian, Stripe)
  - Multiple TXT values and string formatting
  - Online validation tools and testing
- `docs/src/guide/ns-records.md`: NS (Name Server) delegation guide
  - Subdomain delegation to external nameservers
  - Multi-cloud delegation examples (AWS Route 53)
  - Environment separation (production, staging)
  - Glue records for in-zone nameservers
  - FQDN requirements and best practices
- `docs/src/guide/srv-records.md`: SRV (Service Location) records guide
  - Service discovery for XMPP, SIP, LDAP, Minecraft
  - Priority and weight-based load balancing
  - Protocol-specific examples (TCP and UDP)
  - Failover configuration
  - Required supporting A/AAAA records
- `docs/src/guide/caa-records.md`: CAA (Certificate Authority Authorization) guide
  - Certificate issuance authorization
  - Let's Encrypt, DigiCert, AWS ACM configurations
  - Wildcard certificate authorization (`issuewild` tag)
  - Incident reporting (`iodef` tag)
  - Multi-CA authorization
  - Security benefits and compliance

### Why
All DNS record type guide documentation pages were marked as "under construction" with placeholder content. Users needed comprehensive, production-ready documentation for:
- IPv6 deployment with AAAA records
- Email infrastructure with MX and TXT records (SPF, DKIM, DMARC)
- Subdomain delegation with NS records
- Service discovery with SRV records
- Certificate security with CAA records

### Impact
- [x] Documentation only
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only

**Benefits:**
1. **Complete documentation coverage** - All 8 DNS record types now have comprehensive guides
2. **Production-ready examples** - Real-world configurations for common services (Google, Microsoft, AWS)
3. **Security guidance** - Email authentication (SPF/DKIM/DMARC) and certificate control (CAA)
4. **Better onboarding** - New users can find complete examples for any record type
5. **Troubleshooting support** - Each guide includes testing procedures and common issues

**Documentation Structure:**
- Creating records with YAML examples
- Common use cases and configurations
- Best practices and recommendations
- Status monitoring with granular conditions
- Troubleshooting with testing commands
- Cross-references to related guides

---

## [2025-12-10 23:00] - Update Documentation for Granular Status Conditions

**Author:** Erick Bourgeois

### Changed
- `docs/src/reference/status-conditions.md`:
  - Updated "Current Usage" section with granular status information for all resources
  - Added detailed Bind9Cluster status reasons (AllInstancesReady, SomeInstancesNotReady, NoInstancesReady)
  - Documented DNSZone reconciliation flow with Progressing, Ready, and Degraded conditions
  - Documented DNS Record reconciliation flow with all 8 record types
  - Replaced all examples with realistic granular status examples showing:
    - DNSZone progressing through primary and secondary reconciliation phases
    - DNS records showing Progressing → Ready or Degraded states
    - Bind9Cluster showing partial readiness
- `docs/src/development/reconciliation.md`:
  - Completely rewrote DNSZone Reconciliation section with detailed multi-phase flow
  - Added Status Conditions subsection documenting all condition types and reasons
  - Added Benefits subsection explaining advantages of granular status
  - Completely rewrote Record Reconciliation section with granular status flow
  - Added documentation for all 8 record types
- `docs/src/concepts/dnszone.md`:
  - Completely rewrote Status section with granular condition examples
  - Added "Status During Reconciliation" showing Progressing states
  - Added "Status After Successful Reconciliation" showing Ready state
  - Added "Status After Partial Failure" showing Degraded state
  - Added "Condition Types" section documenting all reasons
  - Added "Benefits of Granular Status" subsection

### Why
Documentation was out of sync with the new granular status implementation from 2025-12-10 changes. Users needed comprehensive documentation showing:
- How to interpret Progressing, Ready, and Degraded condition types
- What each status reason means (PrimaryReconciling, SecondaryFailed, RecordFailed, etc.)
- Real-world examples of status during each reconciliation phase
- Benefits of the new granular status approach

### Impact
- [x] Documentation only
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only

**Benefits:**
1. **Users can now understand status conditions** - Clear documentation of all condition types and reasons
2. **Better troubleshooting** - Examples show what each failure state looks like
3. **Reconciliation visibility** - Documentation explains the multi-phase reconciliation flow
4. **Consistency** - All documentation now reflects the actual reconciler implementation

---

## [2025-12-10 22:00] - Implement Granular Status Updates for DNS Records

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/records.rs`:
  - Modified `add_record_to_all_endpoints()` to return `usize` (count of configured endpoints)
  - Updated ALL 8 record reconcilers (A, TXT, AAAA, CNAME, MX, NS, SRV, CAA) to use granular status updates:
    - Set `Progressing/RecordReconciling` status before record configuration
    - Set `Degraded/RecordFailed` status on errors with specific error messages
    - Set `Ready/ReconcileSucceeded` status on success with endpoint count

### Why
DNS record reconciliation previously had a single status update at the end, making it impossible to track progress or identify which phase failed.

**New Architecture:**
- **Incremental status updates** - Users see `Progressing` status while records are being configured
- **Better failure visibility** - `Degraded/RecordFailed` status shows exactly what failed
- **Accurate endpoint counts** - Status message includes number of endpoints successfully configured
- **Kubernetes-native conditions** - Proper use of `Progressing`/`Ready`/`Degraded` types

**Status Flow:**
```
Progressing/RecordReconciling (before work)
  ↓
Ready/ReconcileSucceeded (on success with count)

OR

Progressing/RecordReconciling
  ↓
Degraded/RecordFailed (on failure with error details)
```

### Impact
- [x] Enhancement - Better observability for DNS record operations
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

**Benefits:**
1. **Real-time progress** - See when records are being configured
2. **Better debugging** - Know immediately if/why a record failed
3. **Accurate reporting** - Status shows exact number of endpoints configured
4. **Consistent with zones** - Same status pattern as DNSZone reconciliation

**Testing:**
- All 270 unit tests pass
- All clippy checks pass
- cargo fmt applied
- All 8 record types tested (A, TXT, AAAA, CNAME, MX, NS, SRV, CAA)

---

## [2025-12-10 21:00] - Implement Granular DNSZone Status Updates with Progressing/Degraded Conditions

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`:
  - **Complete redesign of status update architecture** for better observability and failure visibility
  - Modified `add_dnszone()` to return `usize` (count of configured primary endpoints)
  - Modified `add_dnszone_to_secondaries()` to return `usize` (count of configured secondary endpoints)
  - Added `update_condition()` helper for incremental status updates during reconciliation
  - Modified `update_status_with_secondaries()` to accept a `reason` parameter
  - Modified `reconcile_dnszone()` to:
    - Set `Progressing` status with reason `PrimaryReconciling` before primary configuration
    - Set `Progressing` status with reason `PrimaryReconciled` after successful primary configuration
    - Set `Progressing` status with reason `SecondaryReconciling` before secondary configuration
    - Set `Progressing` status with reason `SecondaryReconciled` after successful secondary configuration
    - Set `Ready` status with reason `ReconcileSucceeded` when all phases complete successfully
    - Set `Degraded` status with specific reason on failures:
      - `PrimaryFailed` - Primary reconciliation failed (fatal)
      - `SecondaryFailed` - Secondary reconciliation failed (non-fatal, primaries still work)
  - Secondary failures are now non-fatal and result in `Degraded` status instead of failing reconciliation

### Why
The previous implementation had several problems:

1. **Single status update at the end** - If reconciliation failed partway through, status didn't reflect partial progress
2. **Re-fetching was wasteful** - Extra API call to get information we already had
3. **Lack of granularity** - Couldn't tell if primaries succeeded but secondaries failed
4. **Poor observability** - Users couldn't see reconciliation progress
5. **Inaccurate secondary count** - Status showed "0 secondary server(s)" even when secondaries were configured

**New Architecture:**

- **Incremental status updates** - Status reflects actual progress as each phase completes
- **Better failure visibility** - Can see which phase failed (primary vs secondary reconciliation)
- **Kubernetes-native conditions** - Uses proper condition types (`Progressing`, `Ready`, `Degraded`)
- **No wasted API calls** - Status is updated with counts returned from add functions
- **Graceful degradation** - Secondary failures don't break the zone (primaries still work)

**Condition Flow:**

```
Progressing/PrimaryReconciling
  → Progressing/PrimaryReconciled (on success)
  → Progressing/SecondaryReconciling
  → Progressing/SecondaryReconciled (on success)
  → Ready/ReconcileSucceeded (all complete)

OR

Progressing/PrimaryReconciling
  → Degraded/PrimaryFailed (on primary failure - fatal)

OR

Progressing/SecondaryReconciling
  → Degraded/SecondaryFailed (on secondary failure - non-fatal)
```

### Impact
- [x] Enhancement - Better observability and failure handling
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

**Benefits:**
1. **Real-time progress visibility** - Users can see exactly where reconciliation is in the process
2. **Partial success handling** - Primaries can work even if secondaries fail
3. **Better debugging** - Clear indication of which component failed
4. **Accurate counts** - Status reflects actual number of configured endpoints
5. **Graceful degradation** - DNS service continues even with secondary failures

**Testing:**
- All unit tests pass (270 passed)
- All clippy checks pass
- cargo fmt applied
- Verified status conditions update correctly through each phase

---

## [2025-12-10 20:30] - Fix DNSZone Status to Reflect Actual Secondary Count

**Author:** Erick Bourgeois

**NOTE: This change was superseded by the granular status update implementation above (2025-12-10 21:00).**

---

## [2025-12-10 16:12] - Fix ServiceAccount OwnerReference Conflict

**Author:** Erick Bourgeois

### Changed
- `src/bind9_resources.rs`:
  - Modified `build_service_account()` to NOT set `ownerReferences` on the `bind9` ServiceAccount
  - ServiceAccount is now a shared resource across all Bind9Instance resources in the namespace

### Why
Multiple Bind9Instance resources (primary and secondary) in the same namespace were trying to create the same ServiceAccount named "bind9", each setting themselves as the controller owner (`Controller: true` in ownerReferences).

**Kubernetes Constraint:**
Only ONE ownerReference can have `Controller: true`. When multiple instances tried to claim controller ownership, Kubernetes rejected the updates with error:
```
ServiceAccount "bind9" is invalid: metadata.ownerReferences: Only one reference can have Controller set to true
```

**Root Cause:**
The `build_service_account()` function called `build_owner_references(instance)`, which always sets `controller: Some(true)`. Since all instances in a namespace share the same ServiceAccount name ("bind9"), this caused ownership conflicts.

**Solution:**
ServiceAccount is now created **without ownerReferences** (`owner_references: None`). This is the correct pattern for shared resources in Kubernetes:
- Each Bind9Instance has its own Deployment, ConfigMap, Secret (owned by that instance)
- ServiceAccount is shared across all instances in the namespace (no owner)
- ServiceAccount will be cleaned up manually or via namespace deletion

### Impact
- [x] Bug fix - No breaking changes
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

**Testing:**
- Tested with multiple Bind9Instance resources (primary-0, primary-1, secondary-0)
- ServiceAccount created successfully without ownerReferences
- All instances reconcile successfully without conflicts

---

## [2025-12-10 20:00] - Fix Reconciliation to Detect Missing Resources (Drift Detection)

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/bind9instance.rs`:
  - Added drift detection to reconciliation logic
  - Now checks if Deployment exists before skipping reconciliation
  - Reconciles resources even when spec unchanged if resources are missing
  - Prevents reconciliation from being permanently stuck when initial creation fails due to RBAC or other errors

### Why
The reconciler was incorrectly skipping resource creation when the spec hadn't changed (generation unchanged), even though actual Kubernetes resources (Deployment, Service, ConfigMap, ServiceAccount) might not exist in the cluster.

**Problem Scenario:**
1. User creates a `Bind9Instance` resource
2. Reconciler attempts to create resources but fails due to RBAC permissions issue (e.g., missing `serviceaccounts` permissions)
3. User fixes RBAC by applying updated ClusterRole
4. Reconciler runs again but sees `current_generation == observed_generation` and **skips resource creation**
5. Resources are never created, operator is permanently stuck

**Root Cause:**
The `should_reconcile()` function only checked generation, assuming that if `observed_generation` was set, reconciliation had succeeded. This assumption is invalid because:
- `observed_generation` might be set even if reconciliation failed partway through
- External factors (deletion, RBAC changes, API server issues) can cause resources to be missing
- Kubernetes operators must implement drift detection to maintain desired state

**Solution:**
Added resource existence check (drift detection):
1. Check if spec changed (generation-based)
2. Check if Deployment exists in cluster
3. Only skip reconciliation if BOTH:
   - Spec hasn't changed (generation match)
   - AND resources exist (no drift)
4. If resources are missing, reconcile regardless of generation

This follows Kubernetes operator best practices where controllers continuously reconcile toward desired state, not just on spec changes.

### Impact
- [x] Bug fix - No breaking changes
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

**Testing:**
- All existing tests pass (`cargo test`)
- Clippy passes with strict warnings
- Manually tested scenario:
  1. Created `Bind9Instance` with RBAC issue
  2. Fixed RBAC by applying correct ClusterRole
  3. Reconciler now detects missing Deployment and creates all resources

## [2025-12-10 19:30] - Make Zone Deletion Idempotent

**Author:** Erick Bourgeois

### Changed
- `src/bind9/zone_ops.rs`:
  - Modified `delete_zone()` to handle non-existent zones gracefully
  - Changed freeze error handling from silent ignore to debug logging
  - Made DELETE operation idempotent by treating "not found" and 404 errors as success
  - Added proper error context for actual failure cases
- `src/constants.rs`:
  - Fixed rustdoc formatting for `ServiceAccount` (backticks for proper rendering)
- `src/reconcilers/bind9instance.rs`:
  - Fixed rustdoc formatting for `ServiceAccount` (backticks for proper rendering)
  - Refactored `create_or_update_service_account()` from `match` to `if let` for better readability

### Why
Zone deletion was failing with ERROR logs when attempting to delete zones that don't exist on secondary servers. This is a common scenario during cleanup operations where:
1. A zone might not have been successfully transferred to a secondary yet
2. A secondary server is being cleaned up after a zone was already removed
3. Reconciliation is retrying a failed deletion operation

The freeze operation before deletion would fail with "not found" and log an ERROR even though the error was being ignored. This created noise in logs and made it difficult to distinguish real errors from expected conditions.

Making the deletion operation idempotent follows Kubernetes operator best practices where repeated delete operations should succeed if the resource is already gone.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only

## [2025-12-10 18:45] - Add ServiceAccount and Authentication for BIND9 Pods

**Author:** Erick Bourgeois

### Changed
- `src/constants.rs`:
  - Added `BIND9_SERVICE_ACCOUNT` constant for the `bind9` ServiceAccount name
- `src/bind9_resources.rs`:
  - Added `build_service_account()` function to create ServiceAccount for BIND9 pods
  - Updated `build_pod_spec()` to set `service_account_name` to `bind9`
  - Added `BIND_ALLOWED_SERVICE_ACCOUNTS` environment variable to bindcar container
  - Imported `ServiceAccount` from `k8s_openapi::api::core::v1`
- `src/reconcilers/bind9instance.rs`:
  - Added `create_or_update_service_account()` function to manage ServiceAccount lifecycle
  - Updated `create_or_update_resources()` to create ServiceAccount as the first step
  - Updated `delete_resources()` to delete ServiceAccount when owned by the instance
  - Imported `build_service_account` and `ServiceAccount`

### Why
To enable secure service-to-service authentication between the bindy controller and the bindcar API sidecar using Kubernetes service account tokens. This improves security by:
- Authenticating requests using Kubernetes-native mechanisms instead of external secrets
- Restricting access to the bindcar API to only authorized service accounts
- Following Kubernetes best practices for pod identity and authentication

The `BIND_ALLOWED_SERVICE_ACCOUNTS` environment variable configures bindcar to accept connections from pods using the `bind9` service account, enabling the bindy controller to manage DNS zones securely.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only

## [2025-12-10 16:30] - Centralize Zone Addition Logic

**Author:** Erick Bourgeois

### Changed
- `src/bind9/zone_ops.rs`:
  - Renamed `add_zone()` to `add_primary_zone()` for clarity
  - Removed `zone_type` parameter from `add_primary_zone()` - always uses `ZONE_TYPE_PRIMARY`
  - Added centralized `add_zones()` function that dispatches to `add_primary_zone()` or `add_secondary_zone()` based on zone type
- `src/bind9/mod.rs`:
  - Updated `Bind9Manager::add_zone()` to `add_zones()` with unified signature
  - Added `Bind9Manager::add_primary_zone()` for direct primary zone creation
  - Both methods now use the centralized `add_zones()` dispatcher
- `src/reconcilers/dnszone.rs`:
  - Updated primary zone reconciliation to use `add_zones()` with `ZONE_TYPE_PRIMARY`
  - Updated secondary zone reconciliation to use `add_zones()` with `ZONE_TYPE_SECONDARY`
  - Added `ZONE_TYPE_SECONDARY` import from bindcar
- `src/bind9/zone_ops_tests.rs`:
  - Updated test documentation to reflect new function names
  - Updated `test_add_zone_duplicate()` to use `add_zones()` instead of `add_zone()`
- `docs/src/concepts/architecture-http-api.md`:
  - Updated Bind9Manager method list to show `add_zones()`, `add_primary_zone()`, `add_secondary_zone()`
  - Updated data flow diagram to use `add_zones()` with correct parameters
  - Updated code examples to show centralized dispatcher pattern
- `docs/src/operations/error-handling.md`:
  - Updated idempotent operations section to document all three zone addition functions
  - Clarified that `add_zones()` is the centralized dispatcher
- `docs/src/reference/api.md`:
  - Regenerated API documentation to reflect latest CRD schemas
- `CLAUDE.md`:
  - Added "Building Documentation" section with `make docs` instructions
  - Documented the complete documentation build workflow
  - Added requirement to always use `make docs` instead of building components separately
  - Updated validation checklist to include documentation build verification

### Why
The zone addition logic had two separate methods (`add_zone()` for primary and `add_secondary_zone()` for secondary), which led to:
- Code duplication in reconcilers (separate code paths for primary vs secondary)
- Lack of a single entry point for zone creation
- Zone type parameter on `add_zone()` that was unused (always passed `ZONE_TYPE_PRIMARY`)

This refactoring creates a cleaner architecture:
- **`add_zones()`**: Single entry point that handles both zone types by dispatching to the appropriate function
- **`add_primary_zone()`**: Specialized function for primary zones (no `zone_type` parameter needed)
- **`add_secondary_zone()`**: Specialized function for secondary zones (already existed)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Refactoring only (internal API changes, no behavior changes)

**Code Quality Improvements:**
- Single entry point for all zone additions via `add_zones()`
- Clearer separation of concerns (primary vs secondary logic)
- Reduced parameter count on `add_primary_zone()` (removed unused `zone_type`)
- Better type safety through centralized dispatch logic

## [2025-12-10 00:00] - Implement Secondary Zone Synchronization

**Author:** Erick Bourgeois

### Added
- `src/reconcilers/dnszone.rs`: Added secondary zone synchronization logic
  - `find_all_primary_pod_ips()`: Find all primary server IPs for configuring secondaries
  - `find_all_secondary_pods()`: Find all secondary pods in a cluster
  - `add_dnszone_to_secondaries()`: Create secondary zones with primaries configured
  - Updated `reconcile_dnszone()` to create zones on both primary AND secondary instances
  - Updated `delete_dnszone()` to delete zones from both primary AND secondary instances
- `src/bind9/mod.rs`: Added `add_secondary_zone()` method to Bind9Manager
- `src/bind9/zone_ops.rs`: Added `add_secondary_zone()` function to create secondary zones with primaries

### Changed
- `Cargo.toml`: Updated bindcar dependency from `0.2.7` to `0.2.8`
- `src/bind9/zone_ops.rs`: Updated `add_zone()` to include `primaries: None` for primary zones
- `src/bind9/zone_ops_tests.rs`: Updated all test ZoneConfig initializations to include `primaries` field

### Why
The DNSZone reconciler was only creating zones on PRIMARY instances, never on SECONDARY instances. This meant secondary servers had no knowledge of zones and couldn't perform zone transfers from primaries.

For BIND9 zone transfers to work properly:
- **Primary zones** need: `also-notify` and `allow-transfer` configured with secondary IPs ✅ (already implemented)
- **Secondary zones** need: Zone definition with `primaries` field listing primary server IPs ❌ (was missing)

Bindcar 0.2.8 added support for the `primaries` field in `ZoneConfig`, enabling the controller to create properly configured secondary zones that will automatically transfer from their configured primary servers.

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (to get new secondary zone configurations)
- [ ] Config change only
- [ ] Documentation only

**Behavior Changes:**
- DNSZone reconciliation now creates zones on BOTH primary and secondary instances
- Secondary zones are created with `primaries` field pointing to all running primary pod IPs
- Zone deletion now removes zones from BOTH primary and secondary instances
- Status messages now show counts for both primary and secondary servers

**Zone Transfer Flow:**
1. Controller finds all primary pod IPs in the cluster
2. Controller creates primary zones with `also-notify` and `allow-transfer` for secondary IPs
3. Controller creates secondary zones with `primaries` configured to primary IPs
4. BIND9 secondary servers automatically initiate zone transfers (AXFR) from configured primaries
5. Primaries send NOTIFY messages to secondaries when zones change
6. Secondaries respond by requesting fresh zone data via IXFR/AXFR

**Example Status Message:**
```
Zone example.com configured on 2 primary and 1 secondary server(s) for cluster production-dns
```

**Testing:**
All 277 tests pass (270 library + 7 main tests).

**Automatic Primaries Updates:**
The DNSZone reconciler is fully idempotent - every reconciliation fetches current primary IPs and creates/updates secondary zones. Combined with periodic reconciliation (default: every 5 minutes), secondary zones will automatically sync with current primary IPs when:
- Primary pods restart (new IPs assigned)
- Primary instances scale up/down
- New primary instances are added to the cluster

**Current Limitations:**
- Secondary zone updates wait for periodic reconciliation instead of immediate watch-based updates
- Bindcar 0.2.8 doesn't support updating existing zones, so primaries are only set at creation
- To update primaries on existing secondary zones, manually delete and let controller recreate them

**Future Enhancements:**
- Add explicit Kubernetes watch on Bind9Instance resources to trigger immediate DNSZone reconciliation when primary instances change
- Add zone update API to bindcar to support in-place primary IP updates without zone deletion
- Add unit tests for secondary zone synchronization logic

## [2025-12-09 20:35] - Fix Confusing Record Reconciliation Log Messages

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/records.rs`: Changed log messages from "Successfully added" to "Successfully reconciled"
  - Line 182-184: Changed "Adding" to "Reconciling" in INFO log
  - Line 195: Changed error context from "Failed to add" to "Failed to reconcile"
  - Line 198-200: Changed DEBUG log from "Successfully added" to "Successfully reconciled"
  - Line 209-212: Changed INFO log from "Successfully added" to "Successfully reconciled"

### Why
The previous log messages were confusing because they stated "Successfully added X record" even when the record already existed with the correct value and no changes were made. The controller's declarative reconciliation pattern (implemented in `should_update_record`) correctly skips updates when records match the desired state, logging "already exists with correct value - no changes needed". However, the reconciler's log messages incorrectly implied that an add operation always occurred.

The new "reconciled" terminology accurately reflects that reconciliation can result in:
- Creating a new record (if it doesn't exist)
- Updating an existing record (if it has different values)
- Skipping changes (if the record already matches desired state)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Logging/observability improvement only

Example log output after fix:
```
INFO  A record api already exists with correct value - no changes needed
DEBUG Successfully reconciled A record in zone example.com at endpoint 10.244.1.132:53: api.example.com -> 192.0.2.2
```

## [2025-12-09 19:30] - Upgrade bindcar to 0.2.7

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Updated bindcar dependency from `0.2.5` to `0.2.7`

### Why
Keep dependencies up to date with the latest bugfixes and improvements from the bindcar HTTP REST API library for managing BIND9 zones.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Dependency update only

All tests pass successfully (277 total: 270 library tests + 7 main tests).

## [2025-12-09 17:45] - Split bind9 Tests into Per-Module Files

**Author:** Erick Bourgeois

### Changed
- Split monolithic `src/bind9/mod_tests.rs` (1,454 lines, 80 tests) into 11 separate test files
- Created dedicated test files matching the module structure:
  - `mod_tests.rs` (28 lines, 2 tests) - `Bind9Manager` tests
  - `types_tests.rs` (14 tests) - `RndcKeyData`, `RndcError`, `SRVRecordData` tests
  - `rndc_tests.rs` (28 tests) - RNDC key generation and parsing tests
  - `zone_ops_tests.rs` (28 tests) - Zone operation tests
  - `records/a_tests.rs` (2 tests) - A and AAAA record tests
  - `records/cname_tests.rs` (1 test) - CNAME record tests
  - `records/txt_tests.rs` (1 test) - TXT record tests
  - `records/mx_tests.rs` (1 test) - MX record tests
  - `records/ns_tests.rs` (1 test) - NS record tests
  - `records/srv_tests.rs` (1 test) - SRV record tests
  - `records/caa_tests.rs` (1 test) - CAA record tests
- Added test module declarations to each source file (e.g., `#[cfg(test)] mod a_tests;`)

### Why
The monolithic test file didn't match the modular code structure, making it:
- Hard to find tests for specific functionality
- Difficult to understand which tests belong to which module
- Unclear where to add new tests
- Not following the project's pattern of separate `*_tests.rs` files

Co-locating tests with their corresponding modules provides:
- **Better discoverability**: Tests are next to the code they test
- **Clearer organization**: Each test file has a focused purpose
- **Easier maintenance**: Changes to a module and its tests happen together
- **Consistency**: Matches existing patterns (e.g., `reconcilers/*_tests.rs`)

### Impact
- **All 80 tests preserved**: No tests lost in reorganization
- **All tests pass**: 270 library tests passed, 0 failed (16 ignored as expected)
- **98% reduction** in `mod_tests.rs`: From 1,454 lines to 28 lines
- **Better test organization**: Each module's tests are in a dedicated file
- **Improved maintainability**: Tests co-located with code they verify

### Technical Details

**Test File Distribution:**
```
src/bind9/
├── mod_tests.rs               (2 tests)   - Bind9Manager
├── rndc_tests.rs              (28 tests)  - RNDC operations
├── types_tests.rs             (14 tests)  - Type definitions
├── zone_ops_tests.rs          (28 tests)  - Zone management
└── records/
    ├── a_tests.rs             (2 tests)   - A/AAAA records
    ├── cname_tests.rs         (1 test)    - CNAME records
    ├── txt_tests.rs           (1 test)    - TXT records
    ├── mx_tests.rs            (1 test)    - MX records
    ├── ns_tests.rs            (1 test)    - NS records
    ├── srv_tests.rs           (1 test)    - SRV records
    └── caa_tests.rs           (1 test)    - CAA records
```

**Module Declaration Pattern:**
Each source file now declares its test module at the end:
```rust
// src/bind9/rndc.rs
#[cfg(test)]
#[path = "rndc_tests.rs"]
mod rndc_tests;
```

This keeps tests separate but discoverable, following Rust best practices.

---

## [2025-12-09 17:30] - Refactor bind9.rs into Modular Structure

**Author:** Erick Bourgeois

### Changed
- Refactored monolithic `src/bind9.rs` (1,942 lines) into modular structure across 13 files
- Created `src/bind9/` directory with organized submodules:
  - `mod.rs` (511 lines) - Main exports and `Bind9Manager` struct
  - `types.rs` (95 lines) - Shared types: `RndcKeyData`, `RndcError`, `SRVRecordData`
  - `rndc.rs` (207 lines) - RNDC key generation and management functions
  - `zone_ops.rs` (565 lines) - Zone HTTP API operations
  - `records/mod.rs` (161 lines) - Generic `query_dns_record()` and `should_update_record()`
  - `records/a.rs` (244 lines) - A and AAAA record operations
  - `records/cname.rs` (108 lines) - CNAME record operations
  - `records/txt.rs` (110 lines) - TXT record operations
  - `records/mx.rs` (110 lines) - MX record operations
  - `records/ns.rs` (106 lines) - NS record operations
  - `records/srv.rs` (146 lines) - SRV record operations
  - `records/caa.rs` (186 lines) - CAA record operations
- Split tests into per-module files (see separate changelog entry above)

### Why
The `bind9.rs` file had grown to nearly 2,000 lines, making it difficult to:
- Navigate and find specific functionality
- Make changes without merge conflicts in team environments
- Understand the separation of concerns between zone operations and record operations
- Add new record types without editing a massive file

A modular structure provides:
- **Better organization**: Each file has a single, clear responsibility
- **Easier maintenance**: Smaller files are easier to understand and modify
- **Improved collaboration**: Multiple developers can work on different modules simultaneously
- **Clear boundaries**: Separation between types, RNDC operations, zone operations, and record types

### Impact
- **Zero breaking changes**: All public types and functions re-exported through `mod.rs`
- **100% backward compatibility**: Existing code using `use bindy::bind9::*` continues to work
- **All tests pass**: 291 tests passed, 0 failed (20 ignored as expected)
- **Better code organization**: Each module is <600 lines (most are <200 lines)
- **Improved maintainability**: Clear separation of concerns makes future changes easier

### Technical Details

**New Directory Structure:**
```
src/bind9/
├── mod.rs              # Main exports, Bind9Manager, re-exports all public API
├── types.rs            # Core data structures and constants
├── rndc.rs             # RNDC key operations
├── zone_ops.rs         # Zone management via HTTP API
├── mod_tests.rs        # Unit tests (moved from bind9_tests.rs)
└── records/
    ├── mod.rs          # Generic helpers for all record types
    ├── a.rs            # A/AAAA records
    ├── cname.rs        # CNAME records
    ├── txt.rs          # TXT records
    ├── mx.rs           # MX records
    ├── ns.rs           # NS records
    ├── srv.rs          # SRV records
    └── caa.rs          # CAA records
```

**Re-export Pattern in `mod.rs`:**
```rust
// Module declarations
pub mod types;
pub mod rndc;
pub mod zone_ops;
pub mod records;

// Re-export all public types and functions
pub use types::{RndcKeyData, RndcError, SRVRecordData, SERVICE_ACCOUNT_TOKEN_PATH};
pub use rndc::{generate_rndc_key, create_rndc_secret_data, parse_rndc_secret_data};
pub use zone_ops::{add_dnszone, delete_dnszone, zone_exists, reload_zone};
pub use records::*;  // All record operation functions
```

This ensures that existing code like:
```rust
use bindy::bind9::{Bind9Manager, RndcKeyData, add_a_record};
```
continues to work without any changes.

---

## [2025-12-09 17:00] - Implement Generic Declarative Reconciliation for All DNS Record Types

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Added generic `query_dns_record()` function (lines 910-957)
  - Queries DNS server for any record type using hickory-client
  - Returns matching Record objects from DNS responses
  - Shared by all record types to eliminate code duplication
  - Replaces type-specific query functions like `query_a_record()`

- `src/bind9.rs`: Added generic `should_update_record()` helper (lines 982-1028)
  - Implements observe→diff→act pattern for all record types
  - Takes callback function for type-specific value comparison
  - Logs clear messages: "already exists", "creating", or "updating"
  - Returns `true` if update needed, `false` if record already correct

- `src/bind9.rs`: Refactored all record functions to use declarative reconciliation:
  - `add_a_record()` - A records (IPv4 addresses)
  - `add_aaaa_record()` - AAAA records (IPv6 addresses)
  - `add_cname_record()` - CNAME records (aliases)
  - `add_txt_record()` - TXT records (text data)
  - `add_mx_record()` - MX records (mail exchangers with priority)
  - `add_ns_record()` - NS records (nameserver delegation)
  - `add_srv_record()` - SRV records (service location with port/weight/priority)
  - `add_caa_record()` - CAA records (certificate authority authorization)

### Why
The previous implementation only had declarative reconciliation for A records. All other record types were still sending DNS UPDATEs blindly on every reconciliation, even when records already matched desired state. This violated Kubernetes controller best practices.

**Problems with imperative approach:**
- Every reconciliation sent a DNS UPDATE, even when record was already correct
- Unnecessary network traffic and server load
- Error logs for operations that should be no-ops
- Code duplication across all record types
- Violated observe→diff→act pattern

**Declarative reconciliation benefits:**
- **Idempotent**: Multiple reconciliations with same spec = no-op after first
- **Efficient**: Only sends DNS UPDATE when value differs or record missing
- **Clear**: Logs explain exactly what action is being taken and why
- **Correct**: Follows standard Kubernetes controller pattern
- **Maintainable**: Generic abstraction eliminates code duplication

### Impact
- **All record types** now follow declarative reconciliation pattern
- **Zero** unnecessary DNS UPDATEs for records that already match
- **Reduced** DNS server load and network traffic
- **Clearer** logs showing actual vs. desired state differences
- **Type-safe** comparison logic using callbacks for each record type
- **Maintainable** abstraction makes future record types easy to add

### Technical Details

**Generic Abstraction:**
```rust
// Generic query function - works for ANY record type
async fn query_dns_record(
    zone_name: &str,
    name: &str,
    record_type: RecordType,
    server: &str,
) -> Result<Vec<Record>> {
    // Query DNS and return matching records
}

// Generic reconciliation helper with callback for comparison
async fn should_update_record<F>(
    &self,
    zone_name: &str,
    name: &str,
    record_type: RecordType,
    record_type_name: &str,
    server: &str,
    compare_fn: F,  // Type-specific comparison
) -> Result<bool>
where
    F: FnOnce(&[Record]) -> bool,
{
    // 1. Query existing records
    match query_dns_record(...).await {
        Ok(records) if !records.is_empty() => {
            // 2. Compare using callback
            if compare_fn(&records) {
                info!("Record already correct");
                Ok(false)  // ✅ Skip update
            } else {
                info!("Record differs, updating");
                Ok(true)   // 🔄 Update needed
            }
        }
        Ok(_) => {
            info!("Record missing, creating");
            Ok(true)  // ➕ Create needed
        }
        Err(e) => {
            warn!("Query failed: {}", e);
            Ok(true)  // 🤷 Try update anyway
        }
    }
}
```

**Example: A Record with Callback**
```rust
pub async fn add_a_record(...) -> Result<()> {
    let should_update = self.should_update_record(
        zone_name,
        name,
        RecordType::A,
        "A",
        server,
        |existing_records| {
            // Type-specific comparison logic
            if existing_records.len() == 1 {
                if let Some(RData::A(existing_ip)) = existing_records[0].data() {
                    return existing_ip.to_string() == ipv4;
                }
            }
            false
        },
    ).await?;

    if !should_update {
        return Ok(());  // Already correct!
    }

    // Only send DNS UPDATE if needed
    ...
}
```

**Example: MX Record Comparison (Multi-Field)**
```rust
|existing_records| {
    if existing_records.len() == 1 {
        if let Some(RData::MX(existing_mx)) = existing_records[0].data() {
            return existing_mx.preference() == priority
                && existing_mx.exchange().to_string() == mail_server;
        }
    }
    false
}
```

This implements the **reconciliation loop pattern** for all record types:
1. **Observe**: Query current DNS state
2. **Diff**: Compare with desired state (via callback)
3. **Act**: Only send UPDATE if they differ

---

## [2025-12-09 16:30] - Add Declarative Reconciliation for A Records

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Added `query_a_record()` function (replaced by generic implementation)
- `src/bind9.rs`: Updated `add_a_record()` with declarative reconciliation

### Why
Initial implementation of declarative reconciliation for A records only. This was the prototype that was later generalized to all record types in the next change.

---

## [2025-12-09 16:00] - Improve DNS UPDATE Error Logging

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Enhanced error logging for DNS UPDATE operations (lines 966-992)
  - Added detailed error logging when DNS UPDATE is rejected by server
  - Improved context messages to distinguish between send failures and server rejections
  - Better error messages for debugging DNS UPDATE issues

### Why
DNS UPDATE operations were reporting generic errors without enough context to debug issues. When a DNS UPDATE failed, the logs didn't clearly indicate:
- Whether the UPDATE message was sent successfully
- Whether the server rejected the UPDATE with a specific response code
- Whether there was a message ID mismatch or TSIG validation failure

The improved logging helps diagnose issues like:
- Message ID mismatches (receiving response with wrong ID)
- TSIG signature validation failures
- Server-side policy rejections (e.g., record already exists)

### Impact
- **Debugging**: Easier to diagnose DNS UPDATE failures
- **Observability**: Clear distinction between client-side and server-side errors
- **Error Messages**: More actionable error messages for operators

---

## [2025-12-09 15:30] - Fix DNS Record Updates Using Wrong Port

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Added `port_name` parameter to `for_each_primary_endpoint()` (line 850)
  - Allows callers to specify which port to use ("http" or "dns-tcp")
  - HTTP API operations use "http" port (8080)
  - DNS UPDATE operations use "dns-tcp" port (53)
  - Updated add_dnszone to use "http" port (line 203)
  - Updated delete_dnszone to use "http" port (line 324)
- `src/reconcilers/records.rs`: Updated to use "dns-tcp" port for DNS updates (line 170)
  - DNS UPDATE messages (RFC 2136) now go to port 53 instead of port 8080
  - Fixes timeout errors when adding DNS records

### Why
The record reconcilers were sending DNS UPDATE messages to the wrong port. The `for_each_primary_endpoint` function was always querying for the "http" port (8080), which is the bindcar HTTP API port.

This caused DNS UPDATE operations (which use the DNS protocol, not HTTP) to fail with timeouts:
```
DEBUG signing message: Message { ... }
DEBUG created socket successfully
ERROR Failed to reconcile ARecord: Failed to add A record (5 second timeout)
```

The DNS UPDATE messages were being sent to `10.244.2.100:8080`, but BIND9 listens for DNS protocol messages on port 53, not 8080!

### Impact
- **Correctness**: DNS record operations now use the correct protocol and port
- **Functionality**: DNS record creation (A, AAAA, TXT, MX, etc.) now works
- **Separation of Concerns**: HTTP API operations use HTTP port, DNS protocol operations use DNS port
- **Flexibility**: `for_each_primary_endpoint` can now be used for different operation types

### Technical Details

**Before:**
```rust
for_each_primary_endpoint(...) {
    // Always used "http" port → 10.244.2.100:8080
    zone_manager.add_a_record(..., &pod_endpoint, ...).await?;
    // ❌ Sends DNS UPDATE to HTTP port - timeout!
}
```

**After:**
```rust
// Zone operations (HTTP API)
for_each_primary_endpoint(..., "http", ...) {
    zone_manager.add_zone(..., &pod_endpoint, ...).await?;
    // ✅ HTTP API to 10.244.2.100:8080
}

// Record operations (DNS UPDATE protocol)
for_each_primary_endpoint(..., "dns-tcp", ...) {
    zone_manager.add_a_record(..., &pod_endpoint, ...).await?;
    // ✅ DNS UPDATE to 10.244.2.100:53
}
```

The function now queries Kubernetes Endpoints API for the specified port name:
- `"http"` → queries for the bindcar sidecar API port (8080)
- `"dns-tcp"` → queries for the BIND9 DNS TCP port (53)

---

## [2025-12-09 15:00] - Simplify DNSZone Reconciliation Logic

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Complete rewrite of reconciliation logic (lines 88-140)
  - Simplified from complex multi-branch logic to simple two-state reconciliation
  - Removed complex secondary IP change detection logic
  - Reconciliation now only checks: first time or spec changed?
  - Relies on `add_zone()` idempotency instead of manual duplicate checking
  - Eliminated ~50 lines of complex conditional logic

### Why
The previous reconciliation logic was overly complex with multiple branches:
- Check if first reconciliation
- Check if spec changed
- Check if secondary IPs changed
- Multiple nested conditionals deciding when to update status
- Trying to manually detect when zone already exists

This complexity led to:
- Reconciliation loops from missed edge cases
- Hard-to-debug behavior
- Unnecessary secondary IP queries even when no work needed
- Risk of status updates triggering more reconciliations

The new logic follows standard Kubernetes controller patterns:
1. **First reconciliation** (`observed_generation == None`)? → Do work, update status
2. **Spec changed** (`generation != observed_generation`)? → Do work, update status
3. **Otherwise** → Early return, do nothing

### Impact
- **Simplicity**: Reduced from ~100 lines to ~50 lines
- **Correctness**: Simpler logic = fewer bugs
- **Performance**: No unnecessary secondary IP queries when skipping reconciliation
- **Maintainability**: Easy to understand and modify
- **Idempotency**: Relies on `add_zone()` being idempotent (which it already is)

### Technical Details

**Before (Complex):**
```rust
// Check spec changed
let spec_changed = should_reconcile(...);
// Check if needs reconciliation based on secondary_ips status
let needs_reconciliation = spec_changed || secondary_ips.is_none();
if !needs_reconciliation { return Ok(()); }

// Query secondary IPs
let current_secondary_ips = find_all_secondary_pod_ips(...).await?;

// Check if secondaries changed
let secondaries_changed = /* complex comparison logic */;

if secondaries_changed {
    delete_dnszone(...).await?;
    add_dnszone(...).await?;
    update_status(...).await?;
} else if spec_changed || first_reconciliation {
    add_dnszone(...).await?;
    update_status(...).await?;
} else {
    debug!("No work needed");
}
```

**After (Simple):**
```rust
let first_reconciliation = observed_generation.is_none();
let spec_changed = should_reconcile(current_generation, observed_generation);

// Early return if nothing to do
if !first_reconciliation && !spec_changed {
    debug!("Spec unchanged, skipping");
    return Ok(());
}

// Do the work
let secondary_ips = find_all_secondary_pod_ips(...).await?;
add_dnszone(...).await?;  // Idempotent - handles existing zones
update_status(...).await?;
```

The new logic is clearer, shorter, and follows the standard Kubernetes reconciliation pattern.

---

## [2025-12-09 14:30] - Fix DNSZone Reconciliation Loop from Unconditional Status Updates

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Fixed reconciliation loop caused by unconditional status updates (lines 137-188)
  - Moved `update_status_with_secondaries()` call inside the conditional blocks
  - Status now only updates when actual work is performed (zone added or secondaries changed)
  - Added else branch with debug log when no reconciliation is needed
  - Prevents status-only updates from triggering unnecessary reconciliations

### Why
The DNSZone reconciler had a critical bug: it was **always** calling `update_status_with_secondaries()` at the end of reconciliation, even when:
- Spec hadn't changed (`spec_changed == false`)
- Secondaries hadn't changed (`secondaries_changed == false`)
- It was not the first reconciliation (`first_reconciliation == false`)

This meant the reconciler would:
1. Check generation → spec unchanged, skip early return (because secondary_ips might be None)
2. Query secondary IPs from Kubernetes
3. Determine no work needed (spec and secondaries unchanged)
4. **Still update status** with `observed_generation` ← THIS TRIGGERED ANOTHER RECONCILIATION
5. Loop back to step 1

The status update itself was triggering the reconciliation loop, even though no actual work was being done.

### Impact
- **Performance**: Eliminates continuous reconciliation loop for DNSZone resources
- **API Load**: Drastically reduces unnecessary Kubernetes API status updates
- **Correctness**: Status updates now accurately reflect when work was performed
- **Stability**: Prevents operator from constantly reconciling unchanged resources

### Technical Details
**Before:**
```rust
if secondaries_changed { /* update zone */ }
else if spec_changed || first_reconciliation { /* add zone */ }
// ALWAYS update status here ← BUG!
update_status_with_secondaries(...).await?;
```

**After:**
```rust
if secondaries_changed {
    /* update zone */
    update_status_with_secondaries(...).await?;  // ✅ Only when work done
} else if spec_changed || first_reconciliation {
    /* add zone */
    update_status_with_secondaries(...).await?;  // ✅ Only when work done
} else {
    debug!("No reconciliation needed");  // ✅ No status update
}
```

Now status updates only happen when we actually perform work, preventing spurious reconciliation triggers.

---

## [2025-12-09 14:00] - Fix Misleading "Successfully Added Zone" Log Messages

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Modified `add_zone()` function to return `bool` instead of `()` (lines 598-712)
  - Returns `Ok(true)` when zone was actually added
  - Returns `Ok(false)` when zone already exists (idempotent case)
  - Updated documentation to reflect new return type
- `src/reconcilers/dnszone.rs`: Updated zone addition logic (lines 249-269)
  - Captures return value from `add_zone()` in `was_added` variable
  - Only logs "Successfully added zone" at INFO level when `was_added == true`
  - Removed misleading DEBUG log that appeared even when zone already existed
- `src/bind9_tests.rs`: Enhanced idempotency test (lines 1060-1088)
  - First add now verifies return value is `true` (zone was added)
  - Second add now verifies return value is `false` (zone already exists)
  - Better test coverage for the new boolean return type

### Why
The reconciler was logging "Successfully added zone" (DEBUG level) even when the zone already existed and wasn't actually added. This created misleading log output:

```
INFO  Zone internal.local already exists on 10.244.2.100:8080, skipping add
DEBUG Successfully added zone internal.local to endpoint 10.244.2.100:8080
```

The DEBUG message was technically a lie - the zone wasn't "added", it was skipped because it already existed.

### Impact
- **Log Accuracy**: Log messages now accurately reflect what actually happened
- **Debugging**: Easier to understand what the operator is doing from logs
- **Observability**: INFO-level logs only appear when zones are actually created
- **API Clarity**: `add_zone()` callers can now determine if a zone was created vs. already existed
- **Testing**: Better test coverage for idempotency behavior

### Technical Details
The `add_zone()` function now returns `Result<bool>`:
- `Ok(true)` - Zone was created (new zone added to BIND9)
- `Ok(false)` - Zone already existed (idempotent success, no changes made)
- `Err(...)` - Failed to add zone (non-idempotent error)

This follows the standard Rust pattern for idempotent operations that need to report whether work was performed.

---

## [2025-12-09 13:30] - Consolidate Generation Check Logic Across All Reconcilers

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/mod.rs`: Created `should_reconcile()` helper function (lines 83-130)
  - Centralizes the generation check logic used across all reconcilers
  - Takes `current_generation` and `observed_generation` as parameters
  - Returns `true` if reconciliation needed, `false` if spec unchanged
  - Comprehensive documentation with Kubernetes generation semantics
  - Marked as `#[must_use]` to ensure return value is checked
- `src/reconcilers/dnszone.rs`: Updated to use `should_reconcile()` helper (lines 88-117)
  - Replaced inline match expression with function call
  - Simplified reconciliation logic
- `src/reconcilers/bind9instance.rs`: Updated to use `should_reconcile()` helper (lines 226-242)
  - Replaced inline match expression with function call
  - Consistent with other reconcilers
- `src/reconcilers/bind9cluster.rs`: Updated to use `should_reconcile()` helper (lines 78-94)
  - Replaced inline match expression with function call
  - Consistent with other reconcilers

### Why
Previously, the generation check logic was duplicated across three reconcilers (DNSZone, Bind9Instance, Bind9Cluster). Each reconciler had identical match expressions:

```rust
match (current_generation, observed_generation) {
    (Some(current), Some(observed)) => current != observed,
    (Some(_), None) => true,
    _ => false,
}
```

This duplication:
- Made the code harder to maintain (changes needed in 3 places)
- Increased risk of inconsistencies between reconcilers
- Lacked centralized documentation of the pattern
- Violated DRY (Don't Repeat Yourself) principle

### Impact
- **Maintainability**: Single function to update if logic needs to change
- **Consistency**: All reconcilers use the exact same generation check logic
- **Documentation**: Comprehensive explanation of Kubernetes generation semantics in one place
- **Code Quality**: Reduces duplication from ~12 lines to 3 per reconciler
- **Type Safety**: `#[must_use]` attribute ensures return value is always checked

### Technical Details
The `should_reconcile()` function implements the standard Kubernetes controller pattern:
- `metadata.generation`: Incremented by K8s API server when spec changes
- `status.observed_generation`: Set by controller after processing spec
- When they match: spec unchanged → skip reconciliation
- When they differ: spec changed → reconcile
- When `observed_generation` is None: first reconciliation → reconcile

---

## [2025-12-09 12:00] - Fix Tight Reconciliation Loops in Bind9Instance and Bind9Cluster

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/bind9instance.rs`: Added generation-based change detection (lines 226-251)
  - Checks `metadata.generation` vs `status.observed_generation` before reconciling
  - Skips resource updates (ConfigMap, Deployment, Service, Secret) when spec unchanged
  - Only performs reconciliation on first run or when spec changes
  - Early returns to prevent cascading reconciliations
- `src/reconcilers/bind9cluster.rs`: Added generation-based change detection (lines 78-103)
  - Checks `metadata.generation` vs `status.observed_generation` before reconciling
  - Skips cluster ConfigMap updates and instance reconciliation when spec unchanged
  - Only performs reconciliation on first run or when spec changes
  - Early returns to prevent status-only updates from triggering full reconciliation

### Why
The tight reconciliation loop was not just in DNSZone - it was happening across **all three main reconcilers**:

1. **Bind9Instance**: Every status update triggered full reconciliation:
   - Recreate/update ConfigMap
   - Recreate/update Deployment (causing pod restarts!)
   - Recreate/update Service
   - Recreate/update RNDC Secret
   - Update status → trigger another reconciliation

2. **Bind9Cluster**: Every status update triggered full reconciliation:
   - Recreate cluster ConfigMap
   - Reconcile all managed instances (which triggers Bind9Instance reconciliations!)
   - Update status → trigger another reconciliation

This created a **cascading loop**:
- DNSZone status update → triggers DNSZone reconciliation
- Bind9Cluster status update → triggers Bind9Cluster reconciliation → triggers Bind9Instance reconciliations
- Bind9Instance status update → triggers Bind9Instance reconciliation → updates Deployment → Kubernetes updates pod status → triggers more reconciliations

**Root Cause**: None of the reconcilers distinguished between spec changes (user edits) and status changes (operator updates). They all performed full reconciliation on every trigger.

### Impact
- **Performance**: Eliminates three separate reconciliation loops
- **Stability**: Prevents unnecessary pod restarts from Deployment updates
- **API Load**: Drastically reduces Kubernetes API calls across all reconcilers
- **Resource Usage**: Reduces CPU and memory usage from constant reconciliation
- **Cascading Prevention**: Breaks the chain reaction of reconciliations across resources

### Technical Details

All three reconcilers now use the same pattern:
```rust
let current_generation = resource.metadata.generation;
let observed_generation = resource.status.as_ref()
    .and_then(|s| s.observed_generation);

let spec_changed = match (current_generation, observed_generation) {
    (Some(current), Some(observed)) => current != observed,
    (Some(_), None) => true, // First reconciliation
    _ => false,
};

if !spec_changed {
    debug!("Spec unchanged, skipping reconciliation");
    return Ok(());
}
```

This ensures reconciliation **only happens when the user makes changes**, not when the operator updates status.

## [2025-12-09 11:45] - Add Early Return/Guard Clause Coding Style Guidelines

**Author:** Erick Bourgeois

### Added
- `CLAUDE.md`: Added comprehensive "Early Return / Guard Clause Pattern" section (lines 370-495)
  - Explains the early return coding style and its benefits
  - Provides Rust-specific examples with ✅ GOOD and ❌ BAD patterns
  - Demonstrates how to minimize nested if-else statements
  - Shows proper use of `?` operator for error propagation
  - Includes real-world examples from the codebase (reconciliation patterns)
  - Documents when and why to use early returns

### Why
The codebase has been using early return patterns effectively (as seen in the DNSZone reconciliation loop fix), but this pattern wasn't formally documented in the coding standards. Adding it to CLAUDE.md ensures:
- Consistent application of the pattern across all code
- New contributors understand the preferred coding style
- Code reviews can reference the documented standard
- The pattern used to fix the reconciliation loop is now the documented best practice

This pattern is particularly important for:
- Kubernetes reconciliation loops (checking if work is needed before doing it)
- Input validation (failing fast on invalid inputs)
- Error handling (keeping the happy path clean and readable)

### Impact
- **Code Quality**: Establishes a clear, documented standard for control flow
- **Readability**: Reduces cognitive load by minimizing nesting
- **Maintainability**: Makes it easier to understand and modify control flow logic
- **Consistency**: All future code will follow the same pattern
- **Education**: Provides clear examples for contributors to follow

## [2025-12-09 11:30] - Fix Tight Reconciliation Loop for DNSZone with Secondaries

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Added generation-based change detection to prevent unnecessary reconciliations (lines 88-162)
  - Now checks if `metadata.generation` matches `status.observed_generation`
  - Skips expensive secondary IP discovery when spec hasn't changed
  - Skips primary instance queries and zone addition when nothing has changed
  - Only performs full reconciliation when spec changes or on first reconciliation
  - Early returns when no work is needed, preventing tight reconciliation loops
  - Conditional zone addition: only calls `add_dnszone()` when secondaries change, spec changes, or first reconciliation

### Why
The DNSZone reconciler was running in a **tight loop** every time the status was updated. Every reconciliation would:
1. Query Kubernetes API for all primary `Bind9Instance` resources
2. Query Kubernetes API for all secondary `Bind9Instance` resources
3. Query pod endpoints for each primary instance
4. Query pod endpoints for each secondary instance
5. Call `add_dnszone()` which queries BIND9 pods again
6. Update the status with secondary IPs
7. **Trigger another reconciliation due to status change**
8. Repeat indefinitely

This happened because the reconciler didn't distinguish between **spec changes** (which require work) and **status changes** (which don't). In Kubernetes, `metadata.generation` only increments when the spec changes, while status updates don't change it.

The reconciler was calling `add_dnszone()` unconditionally, even when nothing had changed, causing repeated queries to the Kubernetes API for primary instances and their pods.

**Before**: Every status update → full reconciliation → query primaries + secondaries → add zone → update status → infinite loop
**After**: Status update → check generation → skip if unchanged → no loop

### Impact
- **Performance**: Eliminates unnecessary reconciliation loops (reduces from continuous to only when needed)
- **API Load**: Drastically reduces Kubernetes API calls:
  - No repeated queries for primary `Bind9Instance` resources
  - No repeated queries for secondary `Bind9Instance` resources
  - No repeated queries for primary pod endpoints
  - No repeated queries for secondary pod endpoints
  - No repeated calls to `add_dnszone()` when nothing changed
- **Efficiency**: Full reconciliation (including zone addition) only happens when:
  - The DNSZone spec changes (zone name, cluster ref, etc.)
  - Secondary IPs change (new secondaries added/removed)
  - First reconciliation (zone not yet configured)
- **Stability**: Prevents resource exhaustion from tight loops
- **Cost**: Reduces cloud provider API costs in managed Kubernetes environments

### Technical Details

**Generation-based Change Detection**:
```rust
let current_generation = dnszone.metadata.generation;
let observed_generation = dnszone.status.as_ref()
    .and_then(|s| s.observed_generation);

let spec_changed = match (current_generation, observed_generation) {
    (Some(current), Some(observed)) => current != observed,
    (Some(_), None) => true, // First reconciliation
    _ => false,
};
```

**Early Return Pattern**:
```rust
if !needs_secondary_check {
    debug!("Spec unchanged, skipping expensive operations");
    return Ok(());
}
```

This follows the standard Kubernetes operator pattern of comparing generation values to determine if reconciliation work is actually needed.

## [2025-12-08 15:50] - Remove Unused get_service_port Functions

**Author:** Erick Bourgeois

### Removed
- `src/reconcilers/dnszone.rs`: Removed unused `get_service_port()` function (previously lines 1003-1041)
- `src/reconcilers/dnszone.rs`: Removed unused import `Service` from `k8s_openapi::api::core::v1` (line 15)
- `src/reconcilers/records.rs`: Removed duplicate unused `get_service_port()` function (previously lines 1106-1144)

### Changed
- `src/reconcilers/dnszone.rs`: Updated documentation for `get_pod_service_port()` to remove reference to the deleted function (line 936)

### Why
The `get_service_port()` function appeared in both `dnszone.rs` and `records.rs` but was never called anywhere in the codebase, causing dead code warnings during compilation. These functions were originally meant to look up a service's external port by port name, but the codebase uses `get_pod_service_port()` instead, which returns container ports from the Endpoints object (the actual ports needed for direct pod-to-pod communication).

Having duplicate implementations of the same unused function in two different files indicated incomplete refactoring.

### Impact
- **Code Quality**: Eliminates dead code warnings from Rust compiler (`-D dead-code`)
- **Maintainability**: Removes 78 lines of duplicate unused code (39 lines × 2 files)
- **Clarity**: Simplifies the codebase by removing unused functionality and eliminating duplication
- **Build**: Clean compilation with no warnings (`cargo clippy -- -D warnings` passes)

## [2025-12-08 19:30] - Automatic Detection and Update of Secondary Server IP Changes

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Added `secondary_ips` field to `DNSZoneStatus` (line 242)
  - Stores current secondary server IP addresses for change detection
  - Enables comparison of previous vs current secondary configurations
  - Serialized as `Option<Vec<String>>` (skipped if None)
- `src/reconcilers/dnszone.rs`: Enhanced `reconcile_dnszone()` with secondary IP change detection (lines 90-130)
  - Discovers current secondary IPs on every reconciliation loop
  - Compares discovered IPs with stored IPs in `DNSZoneStatus.secondary_ips`
  - Triggers zone recreation when secondary IPs change
  - Updates status with current secondary IPs after successful reconciliation
- `src/reconcilers/dnszone.rs`: Added `update_status_with_secondaries()` function (lines 667-715)
  - Updates `DNSZone` status including secondary IP tracking
  - Stores secondary IPs in status for future comparison
- `src/reconcilers/dnszone.rs`: Updated `update_status()` to preserve secondary IPs (line 778)
  - Ensures secondary IP list isn't lost during status updates
- `src/crd_tests.rs`: Updated test to initialize `secondary_ips` field (line 523)
- `deploy/crds/dnszones.crd.yaml`: Regenerated with new `secondaryIps` status field

### Added
- **Automatic secondary IP change detection**: Reconciler detects when secondary pod IPs change
- **Automatic zone reconfiguration**: Zones automatically updated with new secondary IPs
- **Status tracking**: Current secondary IPs stored in `DNSZone` status for comparison

### Why
When secondary BIND9 pods are rescheduled, restarted, or scaled, their IP addresses change. Without this change:
- Primary zones continued using old (stale) secondary IPs
- Zone transfers failed because primaries sent NOTIFY to dead IPs
- Manual intervention required to update zone transfer configurations

**Before**: Pod restart → Zone transfers stop working → Manual fix required
**After**: Pod restart → Automatic detection → Automatic zone update → Zone transfers resume

### Technical Details

**Change Detection Algorithm**:
```rust
// On each reconciliation:
1. Discover current secondary IPs via Kubernetes API
2. Retrieve stored secondary IPs from DNSZone.status.secondary_ips
3. Sort both lists for accurate comparison
4. If lists differ:
   - Delete existing zones from all primaries
   - Recreate zones with new secondary IPs
   - Update status with new secondary IPs
```

**IP Comparison Logic**:
```rust
let secondaries_changed = match status_secondary_ips {
    Some(stored_ips) => {
        let mut stored = stored_ips.clone();
        let mut current = current_secondary_ips.clone();
        stored.sort();
        current.sort();
        stored != current
    }
    None => !current_secondary_ips.is_empty(),
};
```

**Reconciliation Frequency**: Standard Kubernetes reconciliation loop (typically every 5-10 minutes)
- Changes detected automatically within one reconciliation period
- No additional watchers or resources required
- Works seamlessly with existing reconciliation infrastructure

### Impact
- ✅ **No manual intervention required** for pod IP changes
- ✅ **Self-healing**: System automatically recovers from pod restarts/rescheduling
- ✅ **Zero downtime**: Zone transfers resume automatically after IP changes
- ⚠️ **Transient downtime**: Brief period (~5-10 min) between IP change and detection
- ⚠️ **Zone recreation overhead**: Deletes and recreates zones (not updates in place)

### Future Enhancements
- Implement `update_zone()` API in bindcar for in-place zone updates (avoid delete/recreate)
- Add faster change detection via pod watcher (reduce detection latency)
- Add metrics to track secondary IP change frequency

## [2025-12-08 15:45] - Optimize GitHub Actions Release Workflow

**Author:** Erick Bourgeois

### Changed
- `.github/workflows/release.yaml`: Optimized CI/CD performance and resource usage
  - Replaced three separate cargo cache steps with `Swatinem/rust-cache@v2` (lines 59-62)
  - Added caching for `cargo-cyclonedx` binary installation (lines 71-80)
  - Added 1-day artifact retention policy to binary and SBOM uploads (lines 84, 91)
  - Removed duplicate Docker SBOM generation via `anchore/sbom-action` (previously lines 169-180)
  - Updated `softprops/action-gh-release` from v1 to v2 (line 212)
  - Fixed shellcheck warnings by quoting `$GITHUB_OUTPUT` variable (lines 129-130)
  - Fixed shellcheck warnings in checksum generation with proper globbing (line 209)

### Removed
- Duplicate SBOM generation: Docker Buildx already generates SBOMs with `sbom: true` flag
- Docker SBOM artifact upload and download steps (no longer needed)
- Redundant cargo cache configurations (consolidated into single rust-cache action)

### Why
The release workflow had multiple redundancies that increased execution time and GitHub Actions storage costs:
- **Duplicate SBOM generation**: Docker SBOM was generated twice (once by buildx, once by anchore)
- **Inefficient caching**: Three separate cache actions instead of one optimized action
- **No tool caching**: `cargo-cyclonedx` was re-installed on every platform build (5 times per release)
- **Indefinite artifact retention**: Artifacts stored forever even though only needed until release upload

### Impact
- **Performance**: Estimated 30-60 seconds faster workflow execution per release
- **Cost**: Reduced GitHub Actions storage usage (artifacts deleted after 1 day instead of indefinitely)
- **Reliability**: Fewer moving parts, less chance of cache conflicts
- **Maintenance**: Simpler workflow with fewer steps to maintain

### Technical Details

**Before - Cargo Caching (3 separate steps)**:
```yaml
- uses: actions/cache@v4  # ~/.cargo/registry
- uses: actions/cache@v4  # ~/.cargo/git
- uses: actions/cache@v4  # target/
```

**After - Unified Rust Caching (1 optimized step)**:
```yaml
- uses: Swatinem/rust-cache@v2
  with:
    key: ${{ matrix.platform.target }}
```

**cargo-cyclonedx Caching**:
- Previously: Installed 5 times per release (once per platform)
- Now: Installed once, cached across all platforms
- Cache key: Based on OS and Cargo.lock for stability

**SBOM Consolidation**:
- Docker SBOM is now only generated once via `docker/build-push-action` with `sbom: true`
- SBOM is attached to the container image as an attestation (standard practice)
- No need for separate artifact upload/download

**Artifact Retention**:
- Binary and SBOM artifacts now deleted after 1 day (only needed until `upload-release-assets` job completes)
- Reduces storage costs while maintaining functionality

## [2025-12-08] - Implement Automatic Zone Transfer Configuration for Secondary Servers

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Upgraded `bindcar` dependency from `0.2.4` to `0.2.5` for zone transfer support
- `src/bind9.rs`: Enhanced `add_zone()` to accept secondary server IPs (lines 621-630)
  - Added parameter `secondary_ips: Option<&[String]>` for secondary server configuration
  - Zone configuration now includes `also_notify` and `allow_transfer` fields
  - Added detailed logging showing which secondary servers are configured
- `src/reconcilers/dnszone.rs`: Added `find_all_secondary_pod_ips()` function (lines 471-566)
  - Discovers all running secondary pods in the cluster using label selectors
  - Queries Kubernetes API for `Bind9Instance` resources with `role=secondary`
  - Collects IP addresses from running secondary pods
- `src/reconcilers/dnszone.rs`: Updated `add_dnszone()` to configure zone transfers (lines 135-191)
  - Calls `find_all_secondary_pod_ips()` before adding zones to primaries
  - Passes secondary IPs to `add_zone()` for each primary pod
- `src/bind9_tests.rs`: Updated all tests for new `add_zone()` signature

### Added
- **Automatic secondary discovery**: Operator now automatically finds all secondary servers
- **Zone transfer configuration**: Primary zones include `also-notify` and `allow-transfer` directives
- **Per-cluster configuration**: Each cluster's zones are configured with that cluster's secondaries

### Why
Secondary BIND9 servers were not receiving zone transfers because primary servers didn't know where the secondaries were located. Primary zones were created without `also-notify` and `allow-transfer` directives.

**Before**: Zones created without transfer configuration → secondaries never received zones
**After**: Operator discovers secondaries and configures zone transfers automatically

### Technical Details

**Zone Configuration Generated**:
```bind
zone "example.com" {
    type primary;
    file "/var/cache/bind/example.com.zone";
    allow-update { key "bindy-operator"; };
    also-notify { 10.244.2.101; 10.244.2.102; };      // ← Automatically configured
    allow-transfer { 10.244.2.101; 10.244.2.102; };   // ← Automatically configured
};
```

**Discovery Process**:
1. Query Kubernetes for `Bind9Instance` resources with `role=secondary` label
2. Find all pods for those instances
3. Collect IP addresses from running pods
4. Pass IPs to bindcar when creating zones

**Graceful Degradation**: If no secondaries exist, zones are created without transfer configuration (primary-only deployments continue to work)

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ `cargo fmt` - Code formatted
- ✅ Upgraded bindcar to v0.2.5
- ✅ Updated all test ZoneConfig initializers

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (critical feature - enables zone transfers)
- [ ] Config change only
- [ ] Documentation only

**Notes:**
- **Critical feature** - Enables DNS high availability with secondary servers
- Zone transfers now work automatically without manual configuration
- Requires bindcar v0.2.5+ (includes `also_notify` and `allow_transfer` fields)
- Operator automatically discovers secondary servers using Kubernetes labels
- Works with any number of secondary servers (0 to N)

**Verification**:
```bash
# Check zone configuration on primary
kubectl exec -it bind9-primary-0 -n dns-system -- \
  rndc showzone example.com | grep -E "also-notify|allow-transfer"

# Check zone exists on secondary
kubectl exec -it bind9-secondary-0 -n dns-system -- \
  rndc zonestatus example.com
```

---

## [2025-12-08] - Fix DNS Record Updates to Target All Primary Pods

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs` (line 627): Made `for_each_primary_endpoint()` public for reuse in records.rs
- `src/reconcilers/records.rs` (lines 111-233): Created generic helper function `add_record_to_all_endpoints()` to eliminate code duplication
- `src/reconcilers/records.rs`: Refactored all 8 record reconcilers to use the generic helper:
  - **A records** (lines 235-332): Now updates all primary pods
  - **TXT records** (lines 334-393): Now updates all primary pods
  - **AAAA records** (lines 395-452): Now updates all primary pods
  - **CNAME records** (lines 454-511): Now updates all primary pods
  - **MX records** (lines 513-572): Now updates all primary pods
  - **NS records** (lines 574-633): Now updates all primary pods
  - **SRV records** (lines 635-701): Now updates all primary pods
  - **CAA records** (lines 703-767): Now updates all primary pods
- `src/bind9.rs` (line 136): Added `#[derive(Clone)]` to `SRVRecordData` to support closure cloning
- Removed unused helper macros and functions: `get_instance_and_key!`, `handle_record_operation!`

### Why
**Root Cause:** Previous implementation only updated DNS records on the FIRST primary pod because `get_instance_and_key!` macro only returned the first endpoint. This caused records to be created on only one pod instead of all primary pods.

**Why This Matters:**
- BIND9 primary pods use **emptyDir storage** (non-shared, per-pod storage)
- Each primary pod maintains its own independent zone files
- Updates to one pod don't automatically sync to other primary pods
- **CRITICAL:** For zone transfers to work, ALL primary pods must have the same records

**User Feedback:** "ok, yes, this worked, but ONLY on the second primary bind instance. it seems to be skipping the other primary bind instance. this call needs to be against ALL primaries, sequentially"

**Solution Design:**
1. Reuse existing `for_each_primary_endpoint()` from dnszone.rs - already handles:
   - Finding all PRIMARY instances in the cluster
   - Loading RNDC keys for each instance
   - Getting endpoints (pod IP + port) for each instance
   - Executing operations sequentially across all endpoints
2. Create generic helper `add_record_to_all_endpoints()` that:
   - Accepts a closure for the DNS record addition operation
   - Calls `for_each_primary_endpoint()` to iterate through all pods
   - Updates record status and sends NOTIFY to secondaries
   - Reduces code duplication from ~90 lines per record type to ~30 lines
3. Refactor all 8 record reconcilers to use the helper - eliminates 500+ lines of duplicated code

**Benefits:**
- **Correctness:** Records are now created on ALL primary pods, not just one
- **Consistency:** All primary pods have identical zone data
- **Code Quality:** ~65% reduction in code (from ~720 lines to ~260 lines for record operations)
- **Maintainability:** Single implementation of multi-endpoint logic
- **High Availability:** Zone transfers work correctly because all primaries have complete data

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout - **DNS records will now be created on all primary pods**
- [ ] Config change only
- [ ] Documentation only

**CRITICAL:** This fixes a critical bug where DNS records were only created on one primary pod. After deploying this fix:
- All primary pods will receive record updates
- Zone transfers from any primary to secondaries will work correctly
- High availability is properly supported

**Testing:**
- Verify records exist on all primary pods: `for pod in $(kubectl get pods -l role=primary -o name); do kubectl exec $pod -- dig @localhost <record-name>; done`
- All primary pods should return the same DNS records

---

## [2025-12-09] - Fix DNS Record Creation - Use append() with must_exist=false

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Fixed `client.append()` calls to use `must_exist=false` for truly idempotent operations
  - **A records** (line 949): `append(record, zone, false)` instead of `true`
  - **AAAA records** (line 1150): `append(record, zone, false)` instead of `true`
  - **CNAME records** (line 1016): `append(record, zone, false)` instead of `true`
  - **TXT records** (line 1082): `append(record, zone, false)` instead of `true`
  - **MX records** (line 1219): `append(record, zone, false)` instead of `true`
  - **NS records** (line 1283): `append(record, zone, false)` instead of `true`
  - **SRV records** (line 1377): `append(record, zone, false)` instead of `true`
  - **CAA records** (line 1505): `append(record, zone, false)` instead of `true`
- All record operations now only succeed on `NoError` response code (no YXRRSet handling)

### Why
**Root Cause Analysis - Deep Dive into YXRRSet Errors:**

The error `DNS update failed with response code: YXRRSet` was caused by **DNS UPDATE prerequisite checks**.

**Discovery Process:**
1. **First attempt (WRONG):** Assumed `YXRRSet` meant record exists, treated as success
2. **User testing revealed:** Records NOT in BIND9 despite success logs (`dig` showed `NXDOMAIN`)
3. **Second attempt (STILL WRONG):** Changed from `create()` to `append()` but used `append(record, zone, true)`
4. **Log analysis showed:** DNS UPDATE message had prerequisite: `api.example.com. 0 NONE A`
5. **Root cause found:** The third parameter `must_exist: bool` controls prerequisite behavior

**hickory-client append() Method Signature:**
```rust
fn append(&self, record: Record, zone_origin: Name, must_exist: bool)
```

**The `must_exist` Parameter (RFC 2136 Section 2.4.1):**
- `must_exist=true`: Adds prerequisite "RRset Exists (Value Independent)"
  - Requires at least one RR with specified NAME and TYPE to already exist
  - Used when you want atomic append-only operations
  - Fails with `YXRRSet` if prerequisites not met
- `must_exist=false`: **No prerequisite checks**
  - Truly idempotent - creates OR appends
  - Perfect for Kubernetes operators that reconcile state
  - DNS server will create new RRset or add to existing one

**Why `must_exist=false` is Correct:**
- Kubernetes operators must be idempotent - applying the same resource multiple times should succeed
- We don't care if the record exists or not - we want it to exist with the correct value
- No prerequisite checks means no `YXRRSet` errors
- BIND9 handles duplicates gracefully - adding the same record twice has no effect

**Comparison of DNS UPDATE Methods:**
- `create(record, zone)`: Prerequisite = "RRset must NOT exist" → Fails if any record of that type exists
- `append(record, zone, true)`: Prerequisite = "RRset MUST exist" → Fails if no records of that type exist
- `append(record, zone, false)`: **No prerequisite** → Always succeeds (idempotent)

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout - **DNS records will now be properly created**
- [ ] Config change only
- [ ] Documentation only

**CRITICAL:** This fixes a major bug where DNS records were NOT being created despite success logs. After deploying this fix, all DNS records will be properly written to BIND9.

**Testing:** Verify with `dig @<pod-ip> <record-type> <record-name>` to confirm records exist in BIND9.

**Evidence:** User's debug log showed DNS UPDATE message with prerequisite check:
```
; query
;; example.com. IN SOA          ← Prerequisite check
; answers 1
api.example.com. 0 NONE A       ← "api.example.com must NOT have an A record"
; nameservers 1
api.example.com. 300 IN A 192.0.2.2  ← Trying to ADD the A record
```

This prerequisite was added by `must_exist=true`. Now using `must_exist=false` to remove prerequisites.

---

## [2025-12-09] - SUPERSEDED - Make DNS Record Additions Idempotent (INCORRECT FIX)

**Author:** Erick Bourgeois

**STATUS:** This change was INCORRECT and has been superseded by the fix above.

### What Was Wrong
- Treated `YXRRSet` errors as success to make operations appear idempotent
- Records were NOT actually being created in BIND9
- Error masking prevented detection of the real problem
- User testing showed records didn't exist despite success logs

### The Real Issue
- `client.create()` has a prerequisite that no RRset exists
- Should have used `client.append()` for idempotent operations
- See the fix above for the correct solution

---

## [2025-12-09] - Enable DNSSEC Cryptographic Features for TSIG Authentication

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Added `dnssec-ring` feature to hickory dependencies (lines 38-39)
  - **Before:** `hickory-client = { version = "0.24", features = ["dnssec"] }`
  - **After:** `hickory-client = { version = "0.24", features = ["dnssec", "dnssec-ring"] }`
  - **Also updated:** `hickory-proto` with the same feature addition

### Why
**Fix Runtime Panic in TSIG Authentication:**
- Error: `panic at .../hickory-proto-0.24.4/.../tsig.rs:530:9: not implemented: one of dnssec-ring or dnssec-openssl features must be enabled`
- The `dnssec` feature alone is not sufficient - it requires a cryptographic backend
- TSIG (Transaction Signature) is used to authenticate DNS updates with RNDC keys
- Without a crypto backend, TSIG signature generation/verification panics at runtime

**Why `dnssec-ring` Instead of `dnssec-openssl`:**
- **ring** is a pure Rust cryptography library (safer, more maintainable)
- **OpenSSL** would add a C library dependency and platform-specific build requirements
- ring is the recommended choice for Rust applications
- Consistent with existing use of rustls-tls (also uses ring)

**Technical Details:**
- hickory-proto uses the `ring` crate for HMAC-SHA256/SHA512 operations
- These are required for TSIG signature computation
- The `dnssec` feature enables TSIG support, but the crypto backend must be chosen separately

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout - **Controller image must be rebuilt with new dependencies**
- [ ] Config change only
- [ ] Documentation only / Refactoring

**Note:** This fixes a critical runtime panic that prevents DNS record updates from working.

---

## [2025-12-09] - Fix DNS Records Reconciler to Use Endpoints API

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/records.rs`: Refactored `get_instance_and_key!` macro to use Endpoints API (lines 57-133)
  - **Before:** Constructed service addresses like `{instance}.{namespace}.svc.cluster.local:{port}`
  - **After:** Uses `get_endpoint()` to query Kubernetes Endpoints API for pod IPs and container ports
  - **Server format:** Changed from service DNS to pod endpoint: `{pod_ip}:{container_port}`
  - **Port lookup:** Queries "dns-tcp" port from Endpoints instead of Services
  - **Error handling:** Changed from "ServicePortLookupFailed" to "EndpointLookupFailed" status reason
- `src/reconcilers/records.rs`: Removed `get_service_port()` function (was at lines 1207-1244)
  - **Why removed:** No longer needed since we're using Endpoints API instead of Services
  - **Removed imports:** Removed `Service` from `k8s_openapi::api::core::v1` imports (line 16)
- `src/reconcilers/dnszone.rs`: Made `get_endpoint()` function public (line 739)
  - **Why:** Now reused by records.rs for endpoint lookups
  - **Usage:** Both dnszone and records reconcilers share the same endpoint discovery logic

### Why
**Fix Service Address Error:**
- Error log showed: `Invalid server address: production-dns-primary-1.dns-system.svc.cluster.local:53`
- The records reconciler was still using service addresses instead of pod endpoints
- Service addresses don't work with per-pod EmptyDir storage (bindcar API needs pod-specific access)

**Consistency with DNSZone Reconciler:**
- The dnszone reconciler was already using Endpoints API correctly
- Records reconciler needed the same pattern for consistency
- Both reconcilers now share the same `get_endpoint()` helper function

**Pod-Specific Communication:**
- bindcar HTTP API runs per-pod, not load-balanced
- DNS record updates must target specific pod IP addresses
- Kubernetes Endpoints API provides pod IPs and container ports

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only / Refactoring - **Fixes runtime error without requiring config changes**

---

## [2025-12-08] - Add Endpoints Permissions to RBAC

**Author:** Erick Bourgeois

### Changed
- `deploy/rbac/role.yaml`: Added permissions for `endpoints` resource (lines 62-64)
  - **New rule:** `get`, `list`, `watch` verbs for `endpoints` in core API group
  - **Why needed:** The `for_each_primary_endpoint()` function queries Kubernetes Endpoints API
  - **Usage:** Discovers pod IPs and container ports for zone operations

### Why
**Required for Endpoints API Access:**
- The refactored code now uses the Kubernetes Endpoints API to discover pod IPs and ports
- Without this permission, the operator would fail with RBAC errors when trying to access endpoints
- This is a critical permission for the per-instance iteration pattern

**Security:**
- Read-only access (`get`, `list`, `watch`) - no modification permissions needed
- Scoped to the core API group (`apiGroups: [""]`)
- Follows principle of least privilege

### Impact
- [x] Breaking change - **Clusters must update RBAC before deploying this version**
- [x] Requires cluster rollout - **Apply updated RBAC first: `kubectl apply -f deploy/rbac/`**
- [ ] Config change only
- [ ] Documentation only / Refactoring

---

## [2025-12-08] - Refactor: Move RNDC Key Loading into Helper Function

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Enhanced `for_each_primary_endpoint()` to optionally load RNDC key (lines 604-720)
  - **New parameter:** `with_rndc_key: bool` - controls whether to load RNDC key from first instance
  - **Closure signature changed:** Now passes `Option<RndcKeyData>` to the closure
  - **RNDC key loading:** Moved inside the helper function, loaded once if requested
  - **Benefits:** Eliminates the need for callers to manually load RNDC key before calling the helper
- `src/reconcilers/dnszone.rs`: Simplified `add_dnszone()` function (lines 124-196)
  - **Before:** Had to manually call `find_all_primary_pods()` and `load_rndc_key()`
  - **After:** Just calls `for_each_primary_endpoint(..., true, ...)` to load key automatically
  - **Removed duplication:** Eliminates RNDC key loading boilerplate from caller
- `src/reconcilers/dnszone.rs`: Updated `delete_dnszone()` function (lines 236-285)
  - **Changed:** Now passes `false` for `with_rndc_key` parameter since deletion doesn't need RNDC key
  - **Unchanged:** Function size and logic remain the same, just updated closure signature

### Why
**Further Elimination of Duplication:**
- The pattern of "find primary pods → load RNDC key from first pod" was still duplicated in `add_dnszone()`
- This refactoring moves that logic into the helper function where it belongs
- `add_dnszone()` no longer needs to know HOW to get the RNDC key, just that it needs one

**Benefits:**
- **Even Simpler Callers:** `add_dnszone()` is now even shorter and clearer
- **Conditional Loading:** RNDC key is only loaded when needed (true for add, false for delete)
- **Single Responsibility:** The helper function handles ALL aspects of endpoint iteration including setup
- **Reduced Coupling:** Callers don't need to know about `find_all_primary_pods()` or `load_rndc_key()`
- **Better Encapsulation:** RNDC key loading logic is hidden inside the helper

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only / Refactoring

---

## [2025-12-08] - Refactor: Extract Common Endpoint Iteration Logic

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Created new `for_each_primary_endpoint()` helper function (lines 680-779)
  - **Purpose:** Extract common pattern of iterating through all primary instances and their endpoints
  - **Signature:** `async fn for_each_primary_endpoint<F, Fut>(client: &Client, namespace: &str, cluster_ref: &str, operation: F) -> Result<(Option<String>, usize)>`
  - **How it works:**
    1. Finds all primary pods for the cluster
    2. Collects unique instance names
    3. Gets endpoints for each instance
    4. Executes provided closure on each endpoint
    5. Returns first endpoint (for NOTIFY) and total count
  - **Generic closure:** Accepts any async operation that takes `(pod_endpoint: String, instance_name: String) -> Result<()>`
- `src/reconcilers/dnszone.rs`: Refactored `add_dnszone()` to use `for_each_primary_endpoint()` (lines 120-227)
  - **Before:** 105 lines with manual endpoint iteration logic
  - **After:** 68 lines using shared helper function
  - **Removed duplication:** No longer manually iterates through instances and endpoints
  - **Closure captures:** zone_name, key_data, soa_record, name_server_ips, zone_manager
- `src/reconcilers/dnszone.rs`: Refactored `delete_dnszone()` to use `for_each_primary_endpoint()` (lines 246-303)
  - **Before:** 75 lines with manual endpoint iteration logic
  - **After:** 48 lines using shared helper function
  - **Removed duplication:** No longer manually iterates through instances and endpoints
  - **Closure captures:** zone_name, zone_manager
  - **Simplified:** No need to track first_endpoint since it's not used for deletion

### Why
**DRY Principle (Don't Repeat Yourself):**
- Both `add_dnszone()` and `delete_dnszone()` had identical logic for:
  - Finding primary pods
  - Collecting unique instance names
  - Getting endpoints for each instance
  - Iterating through all endpoints
- This duplication violated DRY and made maintenance harder

**Benefits:**
- **Single Source of Truth:** Instance/endpoint iteration logic exists in one place
- **Easier Maintenance:** Changes to iteration logic only need to be made once
- **Reduced Code:** Eliminated ~80 lines of duplicated code
- **Better Testability:** Can test endpoint iteration logic independently
- **Flexibility:** The generic closure allows any operation to be performed on endpoints
- **Consistency:** Both add and delete operations use identical iteration patterns

**Pattern:**
- Higher-order function accepting async closures
- Closures capture necessary data from outer scope
- Returns both first endpoint (for NOTIFY) and total count (for logging)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only / Refactoring

---

## [2025-12-08] - Refactor: Extract Zone Addition Logic to Separate Function

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Created new `add_dnszone()` function (lines 120-269)
  - **Extracted from:** Zone addition logic previously embedded in `reconcile_dnszone()`
  - **Purpose:** Separate zone addition logic into its own function for better code organization
  - **Signature:** `pub async fn add_dnszone(client: Client, dnszone: DNSZone, zone_manager: &Bind9Manager) -> Result<()>`
  - **Functionality:** Handles all zone addition logic including finding primary pods, loading RNDC key, iterating through instances, and notifying secondaries
  - Added `#[allow(clippy::too_many_lines)]` attribute
- `src/reconcilers/dnszone.rs`: Simplified `reconcile_dnszone()` to orchestration function (lines 64-102)
  - **Before:** Combined orchestration and zone addition logic (125+ lines)
  - **After:** Simplified to just orchestrate by calling `add_dnszone()` (38 lines)
  - **Pattern:** Now mirrors the structure with `delete_dnszone()` - one orchestrator, two specialized functions
  - Removed `#[allow(clippy::too_many_lines)]` from `reconcile_dnszone()` as it's no longer needed

### Why
**Code Organization and Maintainability:**
- Creates symmetry between zone addition and deletion (both have dedicated functions)
- Separates concerns: `reconcile_dnszone()` orchestrates, `add_dnszone()` implements
- Makes the code easier to understand and maintain
- Follows the single responsibility principle

**Benefits:**
- **Clearer Intent:** Function names clearly indicate what each does
- **Easier Testing:** Can test zone addition logic independently
- **Better Readability:** Shorter functions are easier to understand
- **Consistency:** Matches the pattern already established with `delete_dnszone()`

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only / Refactoring

---

## [2025-12-08] - Use Kubernetes Endpoints API with Per-Instance Iteration

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Implemented `get_endpoint()` function to use Kubernetes Endpoints API (lines 620-669)
  - **New approach:** Query Endpoints API to get pod IPs with their container ports
  - **Why Endpoints API:** Provides the actual container ports, not service external ports
  - Returns `Vec<EndpointAddress>` with pod IP and container port pairs
  - Handles multiple endpoint subsets and ready addresses
  - Comprehensive error handling for missing endpoints or ports
- `src/reconcilers/dnszone.rs`: Updated `reconcile_dnszone()` to loop through instances (lines 64-235)
  - **Before:** Manually iterated over pods using hardcoded `BINDCAR_API_PORT` constant
  - **After:** Loops through each primary instance, gets endpoints for each, processes all endpoints
  - **Pattern:** Outer loop over instances → inner loop over endpoints per instance
  - Ensures ALL pods across ALL primary instances receive zone updates
  - Added `#[allow(clippy::too_many_lines)]` attribute
- `src/reconcilers/dnszone.rs`: Updated `delete_dnszone()` to loop through instances (lines 254-340)
  - **Before:** Manually iterated over pods using hardcoded `BINDCAR_API_PORT` constant
  - **After:** Loops through each primary instance, gets endpoints for each, deletes from all
  - **Pattern:** Outer loop over instances → inner loop over endpoints per instance
  - Ensures complete cleanup across all instances and all pods
- `src/reconcilers/dnszone.rs`: Added `EndpointAddress` struct (lines 620-627)
  - Simple data structure holding pod IP and container port
  - Used as return type from `get_endpoint()`
  - Derives `Debug` and `Clone` for debugging and flexibility
- `src/reconcilers/dnszone.rs`: Added import for `Endpoints` from `k8s_openapi::api::core::v1` (line 15)
  - Required to query Kubernetes Endpoints API

### Why
The Kubernetes **Endpoints API** is the correct way to discover pod IPs and their container ports. Additionally, looping through **each instance separately** ensures that zones are added to all pods across all instances, which is critical when:
1. A cluster has multiple primary instances
2. Each instance has multiple replica pods
3. Storage is per-pod (EmptyDir)

**The Correct Architecture:**
- **Service** → Defines the external port (e.g., 80) and routes traffic
- **Endpoints** → Automatically maintained by Kubernetes, contains pod IPs and container ports
- **Instance-level iteration** → Ensures all instances and all their replicas get updated

**Why This Matters:**
- **Dynamic Port Discovery:** Container ports are discovered at runtime, not hardcoded
- **Kubernetes Native:** Leverages Kubernetes' built-in service discovery
- **Multi-Instance Support:** Correctly handles clusters with multiple primary instances
- **Complete Coverage:** Every pod of every instance receives zone updates
- **Resilience:** Only returns ready endpoints (pods that have passed health checks)

**What Changed:**
```rust
// ❌ BEFORE: Manual pod lookup + hardcoded port
let pods = find_all_primary_pods(&client, &namespace, &cluster_ref).await?;
let bindcar_port = crate::constants::BINDCAR_API_PORT;  // Hardcoded 8080
for pod in &pods {
    let endpoint = format!("{}:{}", pod.ip, bindcar_port);
    zone_manager.add_zone(..., &endpoint, ...).await?;
}

// ✅ AFTER: Loop through instances, get endpoints for each
// Extract unique instance names
let mut instance_names: Vec<String> = primary_pods
    .iter()
    .map(|pod| pod.instance_name.clone())
    .collect();
instance_names.dedup();

// Loop through each instance
for instance_name in &instance_names {
    // Get endpoints for this specific instance
    let endpoints = get_endpoint(&client, &namespace, instance_name, "http").await?;

    // Process all endpoints (pods) for this instance
    for endpoint in &endpoints {
        let pod_endpoint = format!("{}:{}", endpoint.ip, endpoint.port);
        zone_manager.add_zone(..., &pod_endpoint, ...).await?;
    }
}
```

### Technical Details

**Endpoints API Structure:**
Kubernetes maintains an Endpoints object for each Service, organized into subsets:
```yaml
apiVersion: v1
kind: Endpoints
metadata:
  name: bind9-primary-instance
subsets:
- addresses:           # List of ready pod IPs
  - ip: 10.244.1.5
  - ip: 10.244.2.10
  ports:               # Container ports (NOT service ports)
  - name: http
    port: 8080         # Actual container port
    protocol: TCP
```

**How `get_endpoint()` Works:**
1. Queries Endpoints API: `GET /api/v1/namespaces/{ns}/endpoints/{service_name}`
2. Iterates through all subsets (usually one, but can be multiple)
3. Finds the port with matching `port_name` (e.g., "http")
4. Extracts all ready pod IPs from `addresses` field
5. Returns `Vec<EndpointAddress>` with IP:port pairs

**Why Not Use Services Directly:**
- Services define **external ports** for routing (e.g., 80)
- Endpoints define **container ports** where apps listen (e.g., 8080)
- When connecting directly to pod IPs, we bypass the service routing
- Therefore, we must use the container port from Endpoints, not the service port

**Benefits:**
- **No hardcoded ports:** Reads from Kubernetes metadata
- **Automatic discovery:** Kubernetes updates Endpoints when pods change
- **Health awareness:** Only returns ready endpoints
- **Standard practice:** This is how Kubernetes-native applications discover backends

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ `cargo fmt` - Code formatted
- ✅ Comprehensive rustdoc comments on `get_endpoint()` function
- ✅ Error handling for missing endpoints or ports
- ✅ Cleaner, more maintainable code

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Code quality improvement - Uses Kubernetes API correctly
- [ ] Config change only
- [ ] Documentation only

**Notes:**
- This is a **refactoring** that improves code quality without changing behavior
- The Endpoints API approach is the "Kubernetes way" of discovering pod backends
- Eliminates dependency on hardcoded `BINDCAR_API_PORT` constant for zone operations
- Makes the code more flexible if container ports change in the future
- Aligns with Kubernetes best practices for service discovery

---

## [2025-12-08] - Improve Zone Creation Idempotency with Better Error Handling

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Enhanced `zone_exists()` with debug logging
  - Added debug log when zone exists: `"Zone {zone_name} exists on {server}"`
  - Added debug log when zone doesn't exist: `"Zone {zone_name} does not exist on {server}: {error}"`
  - Helps diagnose zone existence check failures
- `src/bind9.rs`: Improved `add_zone()` error handling for duplicate zones (lines 671-690)
  - **Before:** Failed hard on any API error when zone creation failed
  - **After:** Treats "already exists" errors from BIND9 as success (idempotent)
  - Detects multiple BIND9 error patterns:
    - "already exists"
    - "already serves the given zone"
    - "duplicate zone"
    - HTTP 409 Conflict status code
  - Logs: `"Zone {zone_name} already exists on {server} (detected via API error), treating as success"`

### Why
The DNSZone reconciler was experiencing repeated reconciliation errors when zones already existed. The problem occurred when:
1. `zone_exists()` check returns `false` (could be transient network issue, API unavailability, etc.)
2. `add_zone()` attempts to create the zone
3. BIND9's `rndc addzone` command fails because zone already exists
4. Bindcar returns the RNDC error to bindy
5. Controller treats this as a failure and retries indefinitely

This change makes zone creation fully idempotent by:
- Catching BIND9's "zone already exists" error messages
- Treating them as success rather than failure
- Preventing unnecessary reconciliation retry loops

### Technical Details

**Two-Layer Idempotency:**
1. **Primary check (line 628):** `zone_exists()` queries `/api/v1/zones/{name}/status`
   - If bindcar returns 200 OK → zone exists, skip creation
   - If bindcar returns 404 Not Found → zone doesn't exist, proceed with creation
   - If bindcar returns other error → status check failed, proceed with creation (fallback will handle duplicates)
2. **Fallback check (lines 671-690):** Handle RNDC errors from zone creation attempt
   - If RNDC error contains duplicate zone messages → treat as success
   - Otherwise → return error for real failures (permissions, invalid config, etc.)

**BIND9 Error Messages:**
When `rndc addzone` is called on an existing zone, BIND9 can return various error messages:
- "already exists" - Generic duplicate zone error
- "already serves the given zone" - Zone is already configured
- "duplicate zone" - Zone name conflicts with existing zone

**Why Two Layers?**
- The primary check (`zone_exists()`) can fail due to transient network issues or API unavailability
- The fallback ensures we don't fail reconciliation if the zone actually exists but status check failed
- This makes the operator resilient to temporary API issues and eventually consistent

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ `cargo fmt` - Code formatted
- ✅ Debug logging improves troubleshooting
- ✅ No functional changes to successful case (zone creation still works)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Bug fix - eliminates infinite reconciliation loops
- [ ] Config change only
- [ ] Documentation only

**Notes:**
- This fixes the issue where zones were being repeatedly created even though they already existed
- The enhanced logging helps diagnose why `zone_exists()` might return false
- Idempotency is critical for Kubernetes operators to prevent resource churn
- The fallback handles all known BIND9 duplicate zone error messages
- The operator now handles transient API failures gracefully

---

## [2025-12-08] - Fix DNS Record Updates to Use DNS-TCP Port Instead of HTTP API Port

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/records.rs`: Fixed `get_instance_and_key!` macro to use DNS-TCP port instead of HTTP port
  - **Before:** Looked up "http" port and connected to bindcar HTTP API
  - **After:** Looks up "dns-tcp" port and connects to BIND9 DNS service over TCP
  - Updated macro at lines 101-132: Changed from `get_service_port(..., "http")` to `get_service_port(..., "dns-tcp")`
  - Updated variable name from `http_port` to `dns_tcp_port` for clarity
  - Updated error messages to reference "DNS TCP service port"
  - Added RFC 2136 reference explaining TCP requirement for dynamic updates with TSIG

### Why
DNS record updates are performed via `nsupdate`, which uses the DNS protocol (RFC 2136) and must connect to port 53 on the BIND9 server **over TCP**. The code was incorrectly looking up the "http" port and attempting to send DNS updates to the bindcar HTTP API, which cannot process nsupdate requests.

**Why TCP and not UDP?**
RFC 2136 (Dynamic Updates in the Domain Name System) recommends TCP for dynamic updates, especially when using TSIG authentication, because:
- **Reliability**: TCP ensures delivery confirmation
- **TSIG signatures**: Large signature payloads work better over TCP
- **Message size**: Dynamic updates with TSIG can exceed UDP packet limits (512 bytes)
- **Connection-oriented**: Better for authenticated transactions

This caused all DNS record creation (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA records) to fail with connection errors because:
- The HTTP API (bindcar) is for zone management, not record updates
- DNS dynamic updates (nsupdate) require the DNS protocol on port 53 over TCP
- TSIG authentication (RNDC keys) work with DNS protocol, not HTTP
- Kubernetes services expose separate ports for "dns-tcp" and "dns-udp"

### Technical Details

**Before (Incorrect - HTTP Port):**
```rust
let http_port = match get_service_port($client, &instance_name, $namespace, "http").await {
    Ok(port) => port,
    Err(e) => { /* error */ }
};
let server = format!("{}.{}.svc.cluster.local:{}", instance_name, $namespace, http_port);
zone_manager.add_a_record(&zone_name, &name, &ip, ttl, &server, &key_data).await;
```

**After (Correct - DNS-TCP Port):**
```rust
// DNS record updates via nsupdate use TCP for reliability and TSIG authentication
// RFC 2136 recommends TCP for dynamic updates, especially with TSIG signatures
let dns_tcp_port = match get_service_port($client, &instance_name, $namespace, "dns-tcp").await {
    Ok(port) => port,
    Err(e) => { /* error */ }
};
let server = format!("{}.{}.svc.cluster.local:{}", instance_name, $namespace, dns_tcp_port);
zone_manager.add_a_record(&zone_name, &name, &ip, ttl, &server, &key_data).await;
```

**Impact on All Record Types:**
This fix applies to all DNS record reconcilers that use the `get_instance_and_key!` macro:
- `reconcile_a_record()` - A records (IPv4)
- `reconcile_aaaa_record()` - AAAA records (IPv6)
- `reconcile_cname_record()` - CNAME records
- `reconcile_mx_record()` - MX records
- `reconcile_txt_record()` - TXT records
- `reconcile_ns_record()` - NS records
- `reconcile_srv_record()` - SRV records
- `reconcile_caa_record()` - CAA records

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ `cargo fmt` - Code formatted
- ✅ Single macro change fixes all record types
- ✅ RFC 2136 compliance documented in code comments

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (critical bug fix - record creation was broken)
- [ ] Config change only
- [ ] Documentation only

**Notes:**
- **Critical bug fix** - DNS record creation was completely broken before this change
- All record types (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA) are now fixed
- The bindcar HTTP API is still used for zone management (add_zone, delete_zone)
- DNS record updates correctly use the DNS protocol over TCP with TSIG authentication
- Kubernetes Service must expose a port named "dns-tcp" (typically port 53/TCP)
- The service should have separate port definitions for "dns-tcp" and "dns-udp"

## [2025-12-08] - Propagate DNS Zones to All Primary Replicas

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Updated `reconcile_dnszone()` to add zones to ALL primary pods
  - **Before:** Used service endpoint which load-balanced to a single pod
  - **After:** Iterates over all primary pods and adds zone to each pod individually
  - Updated zone creation logic (lines 112-190): Loop through all pods with error context
  - Added detailed logging for each pod operation
  - Better error messages indicating which pod failed
- `src/reconcilers/dnszone.rs`: Updated `delete_dnszone()` to delete from ALL primary pods
  - **Before:** Used service endpoint which load-balanced to a single pod
  - **After:** Iterates over all primary pods and deletes zone from each pod individually
  - Updated zone deletion logic (lines 253-298): Loop through all pods with error context
  - Ensures complete cleanup across all replicas

### Why
With EmptyDir storage (per-pod, non-shared), each primary pod maintains its own zone files. Previously, when creating or deleting a DNS zone, the operator would use the Kubernetes service endpoint, which load-balances requests to one pod. This meant:
- Only one primary pod would receive the zone configuration
- Other primary replicas would be missing the zone
- DNS queries could fail depending on which pod received the request
- Zone inconsistency across replicas

This change ensures all primary pods have identical zone configurations, regardless of storage type (EmptyDir or ReadWriteMany PVC).

### Technical Details

**Before (Service Endpoint Approach):**
```rust
// Service load balancer routes to one pod
let service_endpoint = format!("{instance}.{ns}.svc.cluster.local:{port}");
zone_manager.add_zone(&zone_name, ZONE_TYPE_PRIMARY, &service_endpoint, ...).await?;
```

**After (All Pods Approach):**
```rust
// Add zone to ALL primary pods
for pod in &primary_pods {
    let pod_endpoint = format!("{}:{}", pod.ip, http_port);
    zone_manager.add_zone(&zone_name, ZONE_TYPE_PRIMARY, &pod_endpoint, ...).await
        .context(format!("Failed to add zone {} to pod {}", zone_name, pod.name))?;
}
```

**Benefits:**
- ✅ Zone consistency across all primary replicas
- ✅ Works with both EmptyDir and ReadWriteMany PVC storage
- ✅ Direct pod communication bypasses load balancer
- ✅ Better error reporting (identifies which pod failed)
- ✅ Improved observability with per-pod logging

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ `cargo fmt` - Code formatted
- ✅ Added detailed logging for troubleshooting
- ✅ Error context includes pod name for easier debugging

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (behavior change for multi-replica primaries)
- [ ] Config change only
- [ ] Documentation only

**Notes:**
- This fixes a critical bug where only one primary pod would have the zone
- Operators using multi-replica primary instances should redeploy to get consistent behavior
- Single-replica deployments are unaffected (same behavior as before)
- The `find_all_primary_pods()` function was already collecting all pods, but they weren't being used

## [2025-12-08] - Use bindcar Zone Type Constants

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Updated documentation to reference `ZONE_TYPE_PRIMARY` and `ZONE_TYPE_SECONDARY` constants
  - `add_zone()` docstring: Now documents using constants instead of string literals
  - `create_zone_http()` docstring: Now documents using constants instead of string literals
- `src/reconcilers/dnszone.rs`: Updated to use `ZONE_TYPE_PRIMARY` constant
  - Added import: `use bindcar::ZONE_TYPE_PRIMARY;`
  - Changed `add_zone()` call from string literal `"primary"` to constant `ZONE_TYPE_PRIMARY`
  - Updated comment to reference constant instead of string literal
- `src/bind9_tests.rs`: Updated all tests to use `ZONE_TYPE_PRIMARY` constant
  - Added import: `use bindcar::ZONE_TYPE_PRIMARY;`
  - `test_add_zone_duplicate`: Both `add_zone()` calls use constant
  - `test_create_zone_request_serialization`: CreateZoneRequest and assertion use constant

### Why
Bindcar 0.2.4 introduced `ZONE_TYPE_PRIMARY` and `ZONE_TYPE_SECONDARY` constants to replace hardcoded string literals for zone types. Using these constants provides:
- Type safety and prevents typos
- Single source of truth for zone type values
- Better IDE autocomplete and refactoring support
- Alignment with bindcar library best practices

### Technical Details
**Constants from bindcar 0.2.4:**
```rust
pub const ZONE_TYPE_PRIMARY: &str = "primary";
pub const ZONE_TYPE_SECONDARY: &str = "secondary";
```

**Before:**
```rust
zone_manager.add_zone(&spec.zone_name, "primary", &endpoint, &key, &soa, ips)
```

**After:**
```rust
zone_manager.add_zone(&spec.zone_name, ZONE_TYPE_PRIMARY, &endpoint, &key, &soa, ips)
```

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ No functional changes - constants have same values as previous literals
- ✅ Tests updated to use constants

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Code improvement only
- [ ] Documentation only

**Notes:**
- This is a code quality improvement with no functional impact
- Zone type values remain unchanged ("primary" and "secondary")
- Using constants from the bindcar library ensures compatibility with future versions
- Reduces risk of typos in zone type strings

## [2025-12-08] - Upgrade bindcar to 0.2.4

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Upgraded `bindcar` dependency from `0.2.3` to `0.2.4`

### Why
Keep bindcar library up to date with latest bug fixes and improvements. The bindcar library provides type-safe API communication with BIND9 HTTP API.

### Quality
- ✅ `cargo build` - Successfully compiles with bindcar 0.2.4
- ✅ `cargo fmt` - Code formatted
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (dependency version bump)
- [ ] Documentation only

---

## [2025-12-07 15:00] - Design: RNDC Secret Change Detection and Hot Reload

**Author:** Erick Bourgeois

### Added
- **Architecture Decision Record**: [ADR-0001: RNDC Secret Reload](docs/adr/0001-rndc-secret-reload.md)
  - Comprehensive design for automatic RNDC secret change detection
  - Proposed solution: Track secret `resourceVersion` in status, send SIGHUP to pods on change
  - Evaluated alternatives: rolling restart, sidecar watcher, RNDC reconfig command
  - Implementation plan with 4 phases (MVP, Secret Watch, Observability, Advanced)
- **GitHub Issue Template**: [feature-rndc-secret-reload.md](.github/ISSUE_TEMPLATE/feature-rndc-secret-reload.md)
  - Detailed implementation checklist
  - Testing plan and success criteria
  - Security considerations for `pods/exec` RBAC permission

### Why
**Problem:** When RNDC secrets are updated (manual rotation or external secret manager), BIND9 continues using the old key. This prevents:
- Security best practices (regular key rotation)
- Integration with external secret managers (Vault, sealed-secrets)
- Zero-downtime secret updates

**Solution:** Automatically detect secret changes via `resourceVersion` tracking and send SIGHUP signal to affected pods only, enabling hot reload without pod restart.

### Impact
- [ ] Breaking change: **No** - This is a design document for future implementation
- [x] Documentation: ADR and issue template created
- [x] Future enhancement: Enables secure, automated key rotation

### Next Steps
Implementation tracked in issue (to be created) and ADR-0001. Priority phases:
1. **Phase 1 (MVP)**: Add `rndc_secret_version` to status, implement SIGHUP logic
2. **Phase 2**: Add Secret watcher for automatic reconciliation
3. **Phase 3**: Observability (metrics, events, status conditions)
4. **Phase 4**: Advanced features (validation, rate limiting)

---

## [2025-12-07] - Replace master/slave Terminology with primary/secondary

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Updated documentation to use "primary" and "secondary" instead of "master" and "slave"
  - Updated `add_zone()` docstring: "primary" or "secondary" instead of "master" for primary, "slave" for secondary
  - Updated `create_zone_http()` docstring: "primary" or "secondary" instead of "master" or "slave"
- `src/reconcilers/dnszone.rs`: Updated zone type from "master" to "primary"
  - Changed comment from `The zone type will be "master" (primary)` to `The zone type will be "primary"`
  - Changed `add_zone()` call to pass "primary" instead of "master"
  - Updated module docstring to remove "(master)" and "(slave)" parenthetical references
- `src/bind9_tests.rs`: Updated all test zone types from "master" to "primary"
  - `test_add_zone_duplicate`: Changed both `add_zone()` calls to use "primary"
  - `test_create_zone_request_serialization`: Changed CreateZoneRequest zone_type to "primary" and assertion
- `src/crd.rs`: Updated ServerRole enum documentation
  - Removed "(master)" from Primary variant doc comment
  - Removed "(slave)" from Secondary variant doc comment

### Why
The terms "master" and "slave" are outdated and potentially offensive. The DNS community and BIND9 documentation now use "primary" and "secondary" as the standard terminology. This change aligns the codebase with modern inclusive language standards and current DNS best practices.

### Technical Details
**Zone Type Values:**
- Old: `"master"` and `"slave"`
- New: `"primary"` and `"secondary"`

**Note:** BIND9 and bindcar both support the new terminology. The zone type string is passed directly to bindcar's API, which handles both old and new terminology for backward compatibility.

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ No functional changes - only terminology updates
- ✅ Tests updated to reflect new terminology

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Terminology update only
- [ ] Documentation only

**Notes:**
- This is a terminology-only change with no functional impact
- Bindcar 0.2.3 supports both "master/slave" and "primary/secondary" terminology
- All code, comments, and tests now use inclusive language
- Aligns with IETF draft-knodel-terminology-02 and DNS community standards

## [2025-12-07] - Use DNSZone SOA Record Instead of Hardcoded Values

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Updated `add_zone()` method to accept and use SOA record from DNSZone CRD spec
  - Added `soa_record: &crate::crd::SOARecord` parameter to `add_zone()` signature
  - Changed zone creation to use `soa_record.primary_ns` instead of hardcoded `ns.{zone_name}.`
  - Changed zone creation to use `soa_record.admin_email` instead of hardcoded `admin.{zone_name}.`
  - Use all SOA record fields from spec: `serial`, `refresh`, `retry`, `expire`, `negative_ttl`
  - Updated `name_servers` to use `soa_record.primary_ns` instead of hardcoded value
  - Added clippy allow annotations for safe integer casts (CRD schema validates ranges)
- `src/reconcilers/dnszone.rs`: Updated `reconcile_dnszone()` to pass `spec.soa_record` to `add_zone()`
- `src/bind9_tests.rs`: Updated test to create and pass SOA record to `add_zone()`

### Why
The `add_zone()` method was creating zones with hardcoded SOA record values (`ns.{zone_name}.`, `admin.{zone_name}.`, etc.) instead of using the SOA record specified in the DNSZone CRD. This meant users couldn't control critical DNS zone parameters like the primary nameserver, admin email, serial number, and timing values.

### Technical Details
**Before:**
```rust
soa: SoaRecord {
    primary_ns: format!("ns.{zone_name}."),
    admin_email: format!("admin.{zone_name}."),
    serial: 1,
    refresh: 3600,
    // ... hardcoded values
}
```

**After:**
```rust
soa: SoaRecord {
    primary_ns: soa_record.primary_ns.clone(),
    admin_email: soa_record.admin_email.clone(),
    serial: soa_record.serial as u32,
    refresh: soa_record.refresh as u32,
    // ... values from DNSZone spec
}
```

**Type Conversions:**
- `serial`: `i64` → `u32` (CRD schema validates 0-4294967295 range)
- `refresh`, `retry`, `expire`, `negative_ttl`: `i32` → `u32` (CRD schema validates positive ranges)

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ Safe integer casts with schema validation
- ✅ Test updated to verify SOA record usage

### Impact
- [x] Breaking change - Existing zones may have different SOA records
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

**Notes:**
- Users can now fully control SOA record parameters via DNSZone CRD
- The primary nameserver in the SOA record is also used as the zone's nameserver
- This fixes the issue where bindcar was always using `ns.{zone_name}.` regardless of user configuration
- Integer casts are safe because Kubernetes API server validates field ranges based on CRD schema

## [2025-12-07] - Add Finalizer Support for DNSZone Deletion

**Author:** Erick Bourgeois

### Changed
- `src/main.rs`: Added finalizer support to `reconcile_dnszone_wrapper()` to ensure proper cleanup when DNSZone resources are deleted
  - Added imports for `delete_dnszone` function and `finalizer` from kube-runtime
  - Rewrote wrapper to use `finalizer()` with Apply and Cleanup events
  - On Apply event: calls `reconcile_dnszone()` for create/update operations
  - On Cleanup event: calls `delete_dnszone()` for deletion operations
  - Implements proper error conversion from `finalizer::Error<ReconcileError>` to `ReconcileError`
  - Uses finalizer name: `dns.firestoned.io/dnszone`

### Why
When a DNSZone resource is deleted from Kubernetes, the zone must also be removed from the BIND9 server via bindcar's API. Without a finalizer, the resource would be deleted immediately from Kubernetes, but the zone would remain in BIND9, causing orphaned resources.

### Technical Details
**Deletion Flow:**
1. User deletes DNSZone resource
2. Kubernetes adds `deletionTimestamp` but waits for finalizer to complete
3. Controller receives Cleanup event
4. Calls `delete_dnszone()` which calls `zone_manager.delete_zone()`
5. `Bind9Manager::delete_zone()` sends DELETE request to bindcar API at `/api/v1/zones/{zone_name}`
6. Finalizer is removed, Kubernetes completes resource deletion

**Error Handling:**
- ApplyFailed/CleanupFailed: Returns the original `ReconcileError`
- AddFinalizer/RemoveFinalizer: Wraps Kubernetes API error in `ReconcileError`
- UnnamedObject: Returns error if DNSZone has no name
- InvalidFinalizer: Returns error if finalizer name is invalid

### Quality
- ✅ All tests pass (245 passed, 16 ignored)
- ✅ Clippy passes with strict warnings
- ✅ Proper error handling for all finalizer error cases
- ✅ Requeue intervals based on zone ready status (30s not ready, 5m ready)

### Impact
- [x] Breaking change - DNSZone resources will have finalizer added automatically
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

**Notes:**
- Existing DNSZone resources will have the finalizer added on next reconciliation
- Deletion logic already existed in `delete_dnszone()` and `Bind9Manager::delete_zone()`, this change ensures it's called
- The finalizer prevents accidental deletion of zones from Kubernetes without cleanup
- Users who delete DNSZone resources will now see proper cleanup in bindcar/BIND9

## [2025-12-06 16:00] - Fix Integration Test for New Cluster-Based Architecture

**Author:** Erick Bourgeois

### Changed
- `tests/integration_test.sh`: Updated integration tests to use Bind9Cluster
  - Added Bind9Cluster creation before Bind9Instance
  - Updated Bind9Instance to reference cluster with `clusterRef` and `role: PRIMARY`
  - Updated DNSZone to use `clusterRef` instead of deprecated `type` and `instanceSelector`
  - Fixed SOA record field names: `primaryNS` → `primaryNs`, `negativeTTL` → `negativeTtl`
  - Added Bind9Cluster verification and cleanup steps
  - Updated resource status display to show clusters

### Why
The integration test was using the old standalone Bind9Instance schema, which is no longer valid. Bind9Instances now require `clusterRef` and `role` fields and must be part of a Bind9Cluster. The test needed to be updated to match the current CRD schema.

### Technical Details
- **Previous**: Standalone Bind9Instance with inline config
- **Current**: Bind9Cluster with referenced Bind9Instances
- **Schema Changes**:
  - Bind9Instance now requires: `clusterRef`, `role`
  - DNSZone now uses: `clusterRef` instead of `type` + `instanceSelector`
  - SOA record uses camelCase: `primaryNs`, `negativeTtl`

### Quality
- ✅ Integration test now matches current CRD schema
- ✅ Test creates Bind9Cluster before instances
- ✅ Proper cleanup of all resources including cluster

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Test update only

**Notes:**
- Integration tests now properly test the cluster-based architecture
- Tests create: Bind9Cluster → Bind9Instance → DNSZone → DNS Records
- All resource types (cluster, instance, zone, 8 record types) are verified

## [2025-12-06 15:45] - Regenerate CRDs and API Documentation

**Author:** Erick Bourgeois

### Changed
- `deploy/crds/*.crd.yaml`: Regenerated all CRD YAML files with updated descriptions
  - Updated `logLevel` description in Bind9Instance and Bind9Cluster CRDs
  - Included `nameServerIps` field in DNSZone CRD
- `docs/src/reference/api.md`: Regenerated API documentation
  - All CRD fields now have current descriptions
  - Includes new `nameServerIps` field documentation

### Why
After updating the `log_level` description in the Rust source code, the CRD YAML files and API documentation needed to be regenerated to reflect the updated field descriptions.

### Quality
- ✅ `cargo run --bin crdgen` - CRD YAMLs regenerated successfully
- ✅ `cargo run --bin crddoc` - API documentation regenerated successfully
- ✅ `cargo fmt` - Code formatted
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Documentation update only
- [ ] Config change only

**Notes:**
- CRD YAMLs reflect the latest field descriptions from Rust code
- API documentation is up to date with all CRD changes

## [2025-12-06 15:30] - Add nameServerIps Field to DNSZone CRD for Glue Records

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Added `name_server_ips` field to `DNSZoneSpec`
  - Added `HashMap` import: `use std::collections::{BTreeMap, HashMap}`
  - New optional field: `pub name_server_ips: Option<HashMap<String, String>>`
  - Allows users to specify glue record IP addresses for in-zone nameservers
  - Updated doctests in `lib.rs`, `crd.rs`, and `crd_docs.rs` to include the field
- `src/bind9.rs`: Updated `add_zone()` method to accept `name_server_ips` parameter
  - Added new parameter: `name_server_ips: Option<&HashMap<String, String>>`
  - Passes nameserver IPs to bindcar's `ZoneConfig` struct
  - Updated docstring to document the new parameter
- `src/bind9_tests.rs`: Updated test to pass `None` for `name_server_ips`
- `src/crd_tests.rs`: Updated `DNSZoneSpec` test to include `name_server_ips: None`
- `src/reconcilers/dnszone.rs`: Pass `spec.name_server_ips` to `add_zone()` call
- `deploy/crds/dnszones.crd.yaml`: Regenerated with new `nameServerIps` field

### Why
Users need the ability to configure DNS glue records when delegating subdomains where the nameserver hostname is within the delegated zone itself. For example, when delegating `sub.example.com` with nameserver `ns1.sub.example.com`, the parent zone must include the IP address of `ns1.sub.example.com` as a glue record to break the circular dependency.

### Technical Details
- **CRD Field**: `nameServerIps` (camelCase in YAML)
  - Type: `map[string]string` (HashMap in Rust)
  - Optional field (defaults to none)
  - Maps nameserver FQDNs to IP addresses
  - Example: `{"ns1.example.com.": "192.0.2.1", "ns2.example.com.": "192.0.2.2"}`
- **Implementation Flow**:
  1. User specifies `nameServerIps` in DNSZone CR
  2. DNSZone reconciler passes map to `Bind9Manager::add_zone()`
  3. Bind9Manager includes IPs in bindcar's `ZoneConfig`
  4. bindcar generates glue (A) records in the zone file
- **Usage Example**:
  ```yaml
  apiVersion: dns.firestoned.io/v1alpha1
  kind: DNSZone
  spec:
    zoneName: example.com
    clusterRef: my-cluster
    nameServerIps:
      ns1.sub.example.com.: "192.0.2.10"
      ns2.sub.example.com.: "192.0.2.11"
  ```

### Quality
- ✅ `cargo fmt` - Code formatted
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)
- ✅ `cargo run --bin crdgen` - CRD YAML regenerated successfully

### Impact
- [ ] Breaking change (field is optional, backwards compatible)
- [ ] Requires cluster rollout
- [x] Config change only (new optional CRD field)
- [ ] Documentation only

**Notes:**
- The field is optional and backwards compatible
- Users only need to set this when using in-zone nameservers for delegations
- Most zones will leave this field unset (no glue records needed)

## [2025-12-06 15:00] - Upgrade bindcar to 0.2.3

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Upgraded `bindcar` dependency from `0.2.2` to `0.2.3`
- `src/bind9.rs`: Added `HashMap` import and `name_server_ips` field support
  - Added `use std::collections::HashMap` alongside existing `BTreeMap` import
  - Added `name_server_ips: HashMap::new()` to `ZoneConfig` initialization in `add_zone()`
- `src/bind9_tests.rs`: Updated all test `ZoneConfig` initializations
  - Added `use std::collections::HashMap` to four test functions
  - Added `name_server_ips: HashMap::new()` to all `ZoneConfig` test structs

### Why
bindcar 0.2.3 introduces support for DNS glue records via the new required `name_server_ips` field in `ZoneConfig`. Glue records provide IP addresses for nameservers within the zone's own domain, which is necessary for delegating subdomains.

### Technical Details
- **New Field**: `name_server_ips: HashMap<String, String>` in `bindcar::ZoneConfig`
  - Maps nameserver hostnames to IP addresses
  - Used to generate glue (A) records for in-zone nameservers
  - Empty HashMap means no glue records (sufficient for most zones)
- **Updated Functions**:
  - `Bind9Manager::add_zone()` - Sets `name_server_ips: HashMap::new()`
  - Four test functions in `bind9_tests.rs` - All updated with empty HashMap
- **New Dependencies** (transitive from bindcar 0.2.3):
  - `byteorder v1.5.0`
  - `hmac v0.12.1`
  - `md-5 v0.10.6`
  - `rndc v0.1.3`
  - `sha1 v0.10.6`

### Quality
- ✅ `cargo fmt` - Code formatted
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)
- ✅ `cargo update -p bindcar` - Successfully updated to 0.2.3

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Dependency update only
- [ ] Documentation only

**Notes:**
- The `name_server_ips` field is now exposed via the DNSZone CRD (see next changelog entry)
- Glue records are needed for scenarios like delegating `sub.example.com` with nameserver `ns1.sub.example.com`

## [2025-12-06 14:00] - Revert: Keep logLevel Field in BindcarConfig

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Restored `log_level` field to `BindcarConfig` struct
  - Added back `pub log_level: Option<String>` field at line 1654-1656
  - Field provides easier API for users to set bindcar logging level
- `src/bind9_resources.rs`: Restored RUST_LOG environment variable setting in bindcar sidecar
  - Re-added `log_level` variable extraction from config (lines 990-992)
  - Re-added RUST_LOG environment variable to env_vars list (lines 1008-1012)
  - Default value: "info" if not specified

### Why
The `logLevel` field in the CRD provides a simpler, more user-friendly API than requiring users to set `envVars` manually. While `envVars` provides more flexibility, `logLevel` is the easier approach for the common case of adjusting log verbosity.

### Technical Details
- **Previous State**: Had removed `log_level` field in favor of users setting RUST_LOG via `envVars`
- **Current State**: Restored `log_level` field while keeping `envVars` for advanced use cases
- **Default**: "info" (standard logging level)
- **User Override**: Users can set `logLevel` in `global.bindcarConfig` spec
- **Advanced Override**: Users can still use `envVars` to set RUST_LOG or other environment variables

### Quality
- ✅ `cargo fmt` - Code formatted
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)
- ✅ `cargo run --bin crdgen` - CRDs regenerated successfully

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CRD field restored)
- [ ] Documentation only

**Notes:**
- This reverts the previous attempt to remove `logLevel` in favor of `envVars`
- Both `logLevel` and `envVars` are now available for users
- `logLevel` is the recommended approach for simple log level changes
- `envVars` is available for advanced configuration needs

## [2025-12-06 14:30] - Add RNDC Environment Variables and Volume Mount to bindcar API Sidecar

**Author:** Erick Bourgeois

### Changed
- `src/bind9_resources.rs`: Added RNDC credentials and configuration to bindcar API sidecar container
  - Added `RNDC_SECRET` environment variable sourced from Secret key `secret`
  - Added `RNDC_ALGORITHM` environment variable sourced from Secret key `algorithm`
  - Added rndc.conf volume mount at `/etc/bind/rndc.conf` from `config` ConfigMap
  - Updated `build_api_sidecar_container()` function signature to accept `rndc_secret_name` parameter
  - Added imports: `EnvVarSource`, `SecretKeySelector`

### Why
The bindcar API sidecar requires access to RNDC credentials to authenticate with the BIND9 server for zone management operations. The credentials are stored in a Kubernetes Secret and must be mounted as environment variables. Additionally, the rndc.conf file is needed for RNDC protocol configuration.

### Technical Details
- **Environment Variables**:
  - `RNDC_SECRET`: Sourced from Secret field `secret` (the base64-encoded TSIG key)
  - `RNDC_ALGORITHM`: Sourced from Secret field `algorithm` (e.g., "hmac-sha256")
  - Both use `valueFrom.secretKeyRef` to reference the RNDC Secret
- **Volume Mount**:
  - **Volume Name**: `config` (existing ConfigMap volume)
  - **Mount Path**: `/etc/bind/rndc.conf`
  - **SubPath**: `rndc.conf` (specific file from ConfigMap)
- **Implementation**:
  - Updated `build_api_sidecar_container(bindcar_config, rndc_secret_name)` signature
  - Updated call site in `build_pod_spec()` to pass `rndc_secret_name`
  - Environment variables reference the Secret using Kubernetes `secretKeyRef` mechanism

### Quality
- ✅ `cargo fmt` - Code formatted
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (pods need to be recreated with new environment variables and volume mount)
- [ ] Config change only
- [ ] Documentation only

**Migration Notes:**
- Existing bindcar API sidecars will not have the RNDC credentials or rndc.conf mount until pods are recreated
- No configuration changes required - the environment variables and mount are added automatically
- The Secret and ConfigMap already contain the required data, so this only adds the references

## [2025-12-05 17:30] - Fix bindcar API Port with Dynamic Service Lookup

**Author:** Erick Bourgeois

### Changed
- `src/reconcilers/dnszone.rs`: Implemented dynamic service port lookup for bindcar API endpoint
  - Added `get_service_port()` helper function to query Kubernetes Service for port named "http"
  - Updated zone creation and deletion to use dynamically looked up port instead of hardcoded value
- `src/reconcilers/records.rs`: Implemented dynamic service port lookup for bindcar API endpoint
  - Added `get_service_port()` helper function
  - Updated `get_instance_and_key!` macro to lookup service port dynamically

### Why
The controller was incorrectly using port 953 (RNDC port) to connect to the bindcar HTTP API. The bindcar API uses HTTP protocol and should connect via the Kubernetes Service port named "http". Instead of hardcoding any port number, the controller now queries the Kubernetes Service object to get the actual port number, making it flexible and correct.

### Technical Details
- **Before**: Hardcoded port 953 (RNDC protocol port - WRONG!)
- **After**: Dynamic service lookup for port named "http"
- **Implementation**:
  - New helper function: `get_service_port(client, service_name, namespace, port_name) -> Result<i32>`
  - Queries the Kubernetes Service API to find the service
  - Searches service ports for the port with name matching "http"
  - Returns the port number or error if not found
- **Architecture**:
  - The bindcar API sidecar listens on port 8080 (container port)
  - The Kubernetes Service exposes this as port 80 (service port) with name "http"
  - Controller dynamically discovers the port 80 value at runtime

### Quality
- ✅ `cargo build` - Successfully compiles
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)
- ✅ Fixed clippy warnings: `needless_borrow` and `unnecessary_map_or`

### Impact
- [x] Breaking change (API endpoint changed from port 953 to proper HTTP service port)
- [x] Requires cluster rollout (existing deployments using wrong port)
- [ ] Config change only
- [ ] Documentation only

**Migration Notes:**
- Existing clusters will fail to connect to bindcar API until pods are restarted with the new controller version
- The controller will now correctly connect to the HTTP API port (80) instead of RNDC port (953)
- No configuration changes required - the port is discovered automatically

## [2025-12-05 17:20] - Upgrade bindcar to 0.2.2

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Upgraded `bindcar` dependency from `0.2.1` to `0.2.2`

### Why
Keep bindcar library up to date with latest bug fixes and improvements. The bindcar library provides type-safe API communication with BIND9 HTTP API.

### Quality
- ✅ `cargo build` - Successfully compiles with bindcar 0.2.2
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (dependency version bump)
- [ ] Documentation only

## [2025-12-05 17:15] - Fix Clippy Warning for Rust 1.91

**Author:** Erick Bourgeois

### Changed
- `src/bind9_tests.rs`: Fixed clippy warning `comparison_to_empty` in `test_build_api_url_empty_string`
  - Changed `url == ""` to `url.is_empty()` for clearer, more explicit empty string comparison

### Why
Rust 1.91 introduced stricter clippy lints. The `comparison_to_empty` lint recommends using `.is_empty()` instead of comparing to `""` for better code clarity and explicitness.

### Quality
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All tests passing

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Code quality improvement only

## [2025-12-05 17:10] - Set Build Rust Version to 1.91

**Author:** Erick Bourgeois

### Changed
- `rust-toolchain.toml`: Updated `channel` from `"1.85"` to `"1.91"`
- `Dockerfile`: Updated Rust base image from `rust:1.87.0` to `rust:1.91.0`
- CI/CD workflows: All workflows will now use Rust 1.91 (via `rust-toolchain.toml`)

### Why
Standardize the build Rust version to 1.91 across all environments (local development, Docker builds, and CI/CD pipelines). While the MSRV remains 1.85 (the minimum version required by dependencies), we build and test with Rust 1.91 to ensure compatibility with the latest stable toolchain and benefit from the newest compiler optimizations and features.

### Quality
- ✅ `cargo build` - Successfully compiles with Rust 1.91
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (build toolchain version)
- [ ] Documentation only

**Technical Details:**
- **MSRV (Minimum Supported Rust Version)**: 1.85 (as specified in `Cargo.toml`)
- **Build Version**: 1.91 (as specified in `rust-toolchain.toml` and `Dockerfile`)
- **CI/CD Workflows**: Automatically respect `rust-toolchain.toml` via `dtolnay/rust-toolchain@stable`

**Files Updated:**
- `rust-toolchain.toml`: Toolchain pinning for local development and CI/CD
- `Dockerfile`: Rust base image for Docker builds
- All GitHub Actions workflows inherit the version from `rust-toolchain.toml`

## [2025-12-05 17:05] - Set Minimum Supported Rust Version (MSRV) to 1.85

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Updated `rust-version` from `"1.89"` to `"1.85"`
- `rust-toolchain.toml`: Updated `channel` from `"1.89"` to `"1.85"`

### Why
Set the MSRV to the actual minimum required version based on dependency analysis. The kube ecosystem dependencies (kube, kube-runtime, kube-client, kube-lease-manager) all require Rust 1.85.0 as their MSRV. Using 1.89 was unnecessarily restrictive and prevented compilation on older toolchains that are still supported by all dependencies.

### Quality
- ✅ `cargo build` - Successfully compiles with Rust 1.85
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (MSRV adjustment)
- [ ] Documentation only

**Technical Details:**
Dependency MSRV analysis:
- `kube` 2.0.1 → Rust 1.85.0
- `kube-runtime` 2.0.1 → Rust 1.85.0
- `kube-client` 2.0.1 → Rust 1.85.0
- `kube-lease-manager` 0.10.0 → Rust 1.85.0
- `bindcar` 0.2.1 → Rust 1.75
- `tokio` 1.48.0 → Rust 1.71
- `hickory-client` 0.24.4 → Rust 1.71.1
- `reqwest` 0.12.24 → Rust 1.64.0
- `serde` 1.0.228 → Rust 1.56

**Conclusion:** Rust 1.85 is the minimum version that satisfies all dependencies.

## [2025-12-05 17:00] - Upgrade bindcar to 0.2.1

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Upgraded `bindcar` dependency from `0.2` to `0.2.1`

### Why
Keep bindcar library up to date with latest bug fixes and improvements. The bindcar library provides type-safe API communication with BIND9 HTTP API.

### Quality
- ✅ `cargo build` - Successfully compiles with bindcar 0.2.1
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (dependency version bump)
- [ ] Documentation only

## [2025-12-05 16:45] - Optimize Cargo Dependencies

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Optimized dependency configuration for better build performance and clarity
  - Moved `tempfile` from `[dependencies]` to `[dev-dependencies]` (only used in tests)
  - Removed unused `async-trait` dependency (not referenced anywhere in codebase)
  - Removed unused `tokio-test` from `[dev-dependencies]` (not referenced anywhere)
  - Removed `mdbook-toc` from `[dev-dependencies]` (should be installed separately as a standalone tool)

### Why
Reduce production binary dependencies and compilation overhead. Test-only dependencies should be in `[dev-dependencies]` to avoid including them in release builds. Removing unused dependencies reduces compile time and binary size.

### Quality
- ✅ `cargo build` - Successfully compiles with optimized dependencies
- ✅ `cargo clippy -- -D warnings -W clippy::pedantic` - No warnings
- ✅ `cargo test` - All 252 tests passing (245 unit + 7 integration, 16 ignored)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (dependency cleanup)
- [ ] Documentation only

**Technical Details:**
- **tempfile (v3)**: Only used in `src/bind9_tests.rs` for `TempDir` in tests
- **async-trait (v0.1)**: No usage found in any source file
- **tokio-test (v0.4)**: No usage found in any source file
- **mdbook-toc (v0.14.2)**: Documentation build tool, not a code dependency (install via `cargo install mdbook-toc`)

## [2025-12-05 16:30] - Upgrade Rust Version to 1.89

**Author:** Erick Bourgeois

### Added
- `rust-toolchain.toml`: Pin Rust toolchain to version 1.89 with rustfmt and clippy components

### Changed
- `Cargo.toml`: Set `rust-version = "1.89"` to enforce Minimum Supported Rust Version (MSRV)

### Why
Standardize the Rust version across development environments and CI/CD pipelines to ensure consistent builds and tooling behavior.

### Quality
- ✅ `cargo fmt` - Code properly formatted
- ✅ `cargo clippy` - No warnings (strict pedantic mode)
- ✅ `cargo test` - 266 tests passing (261 total: 245 unit + 7 integration + 13 doc + 1 benchmark, 16 ignored)

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (toolchain pinning)
- [ ] Documentation only

## [2025-12-05] - Comprehensive Test Coverage and Documentation Improvements

**Author:** Erick Bourgeois

### Added
- `src/bind9_tests.rs`: Added 40+ new comprehensive unit tests
  - HTTP API URL building tests (IPv4, IPv6, DNS names, edge cases)
  - Negative test cases (errors, timeouts, connection failures)
  - Edge case tests (empty strings, very long values, special characters)
  - bindcar integration tests (ZoneConfig, CreateZoneRequest, serialization/deserialization)
  - Round-trip tests for all RNDC algorithms
  - Tests for trailing slash handling, unicode support, boundary values
- Made `build_api_url()` function `pub(crate)` for testability

### Changed
- `src/bind9.rs`: Improved `build_api_url()` to handle trailing slashes correctly
  - Now strips trailing slashes from URLs to prevent double slashes in API paths
  - Handles both `http://` and `https://` schemes correctly

### Testing
- **Total Tests**: 266 tests (up from 226)
  - 245 unit tests passing
  - 7 integration tests passing
  - 13 doc tests passing
  - 1 benchmark test passing
  - 16 ignored tests (require real HTTP servers)
- **Test Coverage Areas**:
  - RNDC key generation and parsing (100% coverage)
  - ServiceAccount token handling
  - HTTP API URL construction
  - bindcar type integration
  - Error handling and edge cases
  - Serialization/deserialization
  - Unicode and special character support

### Quality
- ✅ All public functions documented
- ✅ `cargo fmt` - Code properly formatted
- ✅ `cargo clippy` - No warnings (strict pedantic mode)
- ✅ `cargo test` - 266 tests passing

### Impact
- [x] Improved test coverage - comprehensive edge case testing
- [x] Better documentation - all public functions documented
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only

---

## [2025-12-05] - Integrate bindcar Library for Type-Safe API Communication

**Author:** Erick Bourgeois

### Changed
- `Cargo.toml`: Added `bindcar = "0.2"` dependency
- `src/bind9.rs`: Replaced local struct definitions with types from the bindcar library
  - Now uses `bindcar::CreateZoneRequest` with structured `ZoneConfig`
  - Now uses `bindcar::ZoneResponse` for HTTP API responses
  - Now uses `bindcar::SoaRecord` for SOA record configuration
  - Removed local `CreateZoneRequest` and `ZoneResponse` struct definitions
  - Updated `create_zone_http()` to accept `ZoneConfig` instead of raw zone file string
  - Updated `add_zone()` to create structured `ZoneConfig` with minimal SOA/NS records

### Why
- **Type safety**: Share type definitions between bindy and bindcar, preventing drift
- **Single source of truth**: bindcar library maintains canonical API types
- **Better maintainability**: No need to duplicate and sync struct definitions
- **Structured configuration**: Use typed configuration instead of error-prone string manipulation
- **Consistency**: Both server (bindcar) and client (bindy) use the same types

### Technical Details

**Before (local definitions)**:
```rust
struct CreateZoneRequest {
    zone_name: String,
    zone_type: String,
    zone_content: String,  // Raw zone file string
    update_key_name: Option<String>,
}
```

**After (bindcar library)**:
```rust
use bindcar::{CreateZoneRequest, SoaRecord, ZoneConfig, ZoneResponse};

let zone_config = ZoneConfig {
    ttl: 3600,
    soa: SoaRecord { /* structured fields */ },
    name_servers: vec![],
    records: vec![],
};

let request = CreateZoneRequest {
    zone_name: "example.com".into(),
    zone_type: "master".into(),
    zone_config,  // Structured configuration
    update_key_name: Some("bind9-key".into()),
};
```

The bindcar API server will convert the `ZoneConfig` to a zone file using `zone_config.to_zone_file()`.

### Impact
- [x] API change - `create_zone_http()` signature changed to accept `ZoneConfig`
- [ ] Breaking change - internal change only, no user-facing impact
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

---

## [2025-12-05] - Fix Docker Build Version Injection

**Author:** Erick Bourgeois

### Fixed
- `Dockerfile`: Moved version update to occur AFTER copying actual source code
  - **Before**: Version was updated in the cached dependency layer, then overwritten by COPY
  - **After**: Version is updated immediately before building the final binary
  - Ensures `cargo build` uses the correct version from the GitHub release tag
  - Binary and package metadata now correctly reflect the release version

### Why
- **Correct version metadata**: The built binary must report the actual release version, not the dev version
- **Docker layer caching bug**: The previous sed command ran too early and was overwritten
- **Release integrity**: Users can verify the binary version matches the release tag

### Technical Details

**Build Flow**:
1. GitHub release created with tag (e.g., `v1.2.3`)
2. Workflow extracts version: `1.2.3` from `github.event.release.tag_name`
3. Docker build receives: `--build-arg VERSION=1.2.3`
4. Dockerfile updates `Cargo.toml`: `version = "1.2.3"` (line 44)
5. Cargo builds binary with correct version metadata
6. Binary reports: `bindy 1.2.3` (matches release tag)

**Verification**:
```bash
# In the container
/usr/local/bin/bindy --version
# Should output: bindy 1.2.3 (not bindy 0.1.0)
```

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Build fix - ensures version metadata is correct
- [ ] Config change only
- [ ] Documentation only

---

## [2025-12-03 23:15] - Add Automatic NOTIFY to Secondaries for Zone Updates

**Author:** Erick Bourgeois

### Added
- `src/reconcilers/records.rs`: Automatic NOTIFY after every DNS record operation
  - Modified `handle_record_operation!` macro to call `notify_zone()` after successful record additions
  - All record types (A, AAAA, TXT, CNAME, MX, NS, SRV, CAA) now trigger NOTIFY automatically
  - NOTIFY failures are logged as warnings and don't fail the record operation
- `src/reconcilers/dnszone.rs`: Automatic NOTIFY after zone creation
  - Added `notify_zone()` call after `create_zone_http()` completes successfully
  - Ensures secondaries receive immediate AXFR after zone is created on primary
  - Added `warn` to tracing imports for notification failure logging

### Changed
- `src/reconcilers/records.rs`: Updated `handle_record_operation!` macro signature
  - Added parameters: `$zone_name`, `$key_data`, `$zone_manager`
  - All 7 record reconcilers updated to pass new parameters
  - Macro now handles both record status updates AND secondary notifications

### Technical Details

**Why This Was Needed:**
- BIND9's dynamic updates (nsupdate protocol) don't trigger NOTIFY by default
- Without explicit NOTIFY, secondaries only sync via SOA refresh timer (can be hours)
- This caused stale data on secondary servers in multi-primary or primary/secondary setups

**How It Works:**
1. Record is successfully added to primary via nsupdate
2. `notify_zone()` sends RFC 1996 DNS NOTIFY packets to secondaries
3. Secondaries respond by initiating IXFR (incremental zone transfer) from primary
4. Updates propagate to secondaries within seconds instead of hours

**NOTIFY Behavior:**
- NOTIFY is sent via HTTP API: `POST /api/v1/zones/{name}/notify`
- Bindcar API sidecar executes `rndc notify {zone}` locally on primary
- BIND9 reads zone configuration for `also-notify` and `allow-transfer` ACLs
- BIND9 sends NOTIFY packets to all configured secondaries
- If NOTIFY fails (network issue, API timeout), operation still succeeds
  - Warning logged: "Failed to notify secondaries for zone X. Secondaries will sync via SOA refresh timer."
  - Ensures record operations are atomic and don't fail due to transient notification issues

**Affected Operations:**
- `reconcile_a_record()` - A record additions
- `reconcile_aaaa_record()` - AAAA record additions
- `reconcile_txt_record()` - TXT record additions
- `reconcile_cname_record()` - CNAME record additions
- `reconcile_mx_record()` - MX record additions
- `reconcile_ns_record()` - NS record additions
- `reconcile_srv_record()` - SRV record additions
- `reconcile_caa_record()` - CAA record additions
- `create_zone()` in DNSZone reconciler - New zone creation

### Why
- **Real-time replication**: Secondaries receive updates immediately instead of waiting for SOA refresh
- **Consistency**: Eliminates stale data windows between primary and secondary servers
- **RFC compliance**: Proper implementation of DNS NOTIFY (RFC 1996) for zone change notifications
- **Production readiness**: Essential for any primary/secondary DNS architecture

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Behavioral change - secondaries now notified automatically
- [ ] Config change only
- [ ] Documentation only

---

## [2025-12-03 22:40] - Standardize on Linkerd Service Mesh References

**Author:** Erick Bourgeois

### Changed
- `CLAUDE.md`: Added service mesh standard - always use Linkerd as the example
- `docs/src/operations/faq.md`: Updated "service meshes" question to specifically reference Linkerd
  - Added details about Linkerd injection being disabled for DNS services
- `docs/src/advanced/integration.md`: Changed "Service Mesh" section to "Linkerd Service Mesh"
  - Removed generic Istio reference, kept Linkerd as the standard
  - Added Linkerd-specific integration details (mTLS, service discovery)
- `core-bind9/service-dns.yaml`: Updated comment from "service mesh sidecar" to "Linkerd sidecar"

### Why
- Consistency: All documentation and examples now use Linkerd as the service mesh standard
- Clarity: Specific examples are more helpful than generic "service mesh" references
- Project standard: Linkerd is the service mesh used in the k0rdent environment

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2025-12-03 21:40] - Rename apiConfig to bindcarConfig and Add Volume Support

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Renamed `ApiContainerConfig` to `BindcarConfig`
  - Renamed struct to better reflect its purpose as the Bindcar sidecar configuration
- `src/crd.rs`: Renamed field `api_config` to `bindcar_config`
  - In `Bind9InstanceSpec` - instance-level Bindcar configuration
  - In `Bind9Config` - cluster-level Bindcar configuration (inherited by all instances)
- All code and test references updated to use `bindcar_config` consistently

### Added
- `src/crd.rs`: Added volume and environment support to `BindcarConfig`
  - `env_vars: Option<Vec<EnvVar>>` - Environment variables for the Bindcar container
  - `volumes: Option<Vec<Volume>>` - Volumes that can be mounted by the Bindcar container
  - `volume_mounts: Option<Vec<VolumeMount>>` - Volume mounts for the Bindcar container

### Fixed
- `src/bind9_resources.rs`: Fixed `bindcarConfig` inheritance from cluster to instances
  - Added `bindcar_config` field to `DeploymentConfig` struct
  - Updated `resolve_deployment_config()` to resolve `bindcar_config` from cluster global config
  - Instance-level `bindcarConfig` now correctly overrides cluster-level configuration
  - `build_pod_spec()` now receives resolved `bindcar_config` instead of only instance-level config
  - Fixes issue where `bind9cluster.spec.global.bindcarConfig.image` was not being honored
- `Cargo.toml`: Switched from native TLS (OpenSSL) to rustls for HTTP client
  - Changed `reqwest` to use `rustls-tls` feature instead of default native-tls
  - Eliminates OpenSSL dependency, enabling clean musl static builds
  - Docker builds now succeed without OpenSSL build dependencies
- `Dockerfile`: Simplified build process by removing OpenSSL dependencies
  - Removed unnecessary packages: `pkg-config`, `libssl-dev`, `musl-dev`, `make`, `perl`
  - Pure Rust TLS stack (rustls) works perfectly with musl static linking

### Impact
- [x] Breaking change - Field name changed from `apiConfig` to `bindcarConfig` in CRDs
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only

### Why
- Improved naming consistency: "bindcar" better represents the sidecar's purpose
- Added flexibility: Users can now customize environment variables and mount volumes in the Bindcar sidecar
- Docker builds: rustls (pure Rust TLS) ensures reliable static builds across all platforms without C dependencies

---

## [2025-12-03 11:45] - Integrate HTTP API Sidecar (bindcar) for BIND9 Management

**Author:** Erick Bourgeois

### Added
- `src/bind9.rs`: New HTTP API integration for all RNDC operations
  - Added `create_zone_http()` method for zone creation via API
  - Converted `exec_rndc_command()` to use HTTP endpoints instead of RNDC protocol
  - Added `HttpClient` and ServiceAccount token authentication
  - Added request/response types: `CreateZoneRequest`, `ZoneResponse`, `ServerStatusResponse`
- `src/bind9_resources.rs`: API sidecar container deployment
  - Added `build_api_sidecar_container()` function to create API sidecar
  - Modified `build_pod_spec()` to include API sidecar alongside BIND9 container
  - Updated `build_service()` to expose API on port 80 (maps to container port 8080)
- `src/crd.rs`: New `BindcarConfig` struct for API sidecar configuration
  - Added `bindcar_config` field to `Bind9InstanceSpec` and `Bind9Config`
  - Configurable: image, imagePullPolicy, resources, port, logLevel
- `Cargo.toml`: Added `reqwest` dependency for HTTP client

### Changed
- `templates/named.conf.tmpl`: RNDC now listens only on localhost (127.0.0.1)
  - Changed from `inet * port 953 allow { any; }` to `inet 127.0.0.1 port 953 allow { localhost; }`
  - API sidecar now handles all external RNDC access via HTTP
- `src/bind9_resources.rs`: Service port configuration
  - **Removed:** RNDC port 953 from Service (no longer exposed externally)
  - **Added:** HTTP port 80 → API sidecar port (default 8080, configurable)
  - Service now exposes: DNS (53 TCP/UDP) and API (80 HTTP)

### Why
**Architecture Migration:** Moved from direct RNDC protocol access to HTTP API sidecar pattern for better:
- **Security**: RNDC no longer exposed to network, only accessible via localhost
- **Flexibility**: RESTful API is easier to integrate with modern tooling
- **Standardization**: HTTP on port 80 follows standard conventions
- **Scalability**: API sidecar can handle authentication, rate limiting, etc.

The `bindcar` sidecar runs alongside BIND9 in the same pod, sharing volumes for zone files and RNDC keys.

### Impact
- [x] Breaking change (RNDC port no longer exposed, all management via HTTP API)
- [x] Requires cluster rollout (new pod template with sidecar container)
- [x] Config change (new `bindcar_config` CRD field)
- [ ] Documentation only

### Technical Details

**HTTP API Endpoints** (in `bindcar` sidecar):
- `POST /api/v1/zones` - Create zone
- `POST /api/v1/zones/:name/reload` - Reload zone
- `DELETE /api/v1/zones/:name` - Delete zone
- `POST /api/v1/zones/:name/freeze` - Freeze zone
- `POST /api/v1/zones/:name/thaw` - Thaw zone
- `POST /api/v1/zones/:name/notify` - Notify secondaries
- `GET /api/v1/zones/:name/status` - Zone status
- `GET /api/v1/server/status` - Server status

**Default Sidecar Configuration:**
```yaml
apiConfig:
  image: ghcr.io/firestoned/bindcar:latest
  imagePullPolicy: IfNotPresent
  port: 8080
  logLevel: info
```

**Authentication:** Uses Kubernetes ServiceAccount tokens mounted at `/var/run/secrets/kubernetes.io/serviceaccount/token`

**Shared Volumes:**
- `/var/cache/bind` - Zone files (shared between BIND9 and API)
- `/etc/bind/keys` - RNDC keys (shared, read-only for API)

## [2025-12-02 14:30] - Fix RNDC addzone Command Quoting

**Author:** Erick Bourgeois

### Fixed
- `src/bind9.rs`: Removed extra single quotes from `addzone` command formatting that caused "unknown option" errors in BIND9
- `src/bind9_tests.rs`: Removed unused `RndcError` import

### Why
The `addzone` RNDC command was wrapping the zone configuration in single quotes, which caused BIND9 to fail with:
```
addzone: unknown option '''
```

The rndc library already handles proper quoting, so the extra quotes around the zone configuration were being interpreted as part of the command itself rather than string delimiters.

### Impact
- [x] Breaking change (fixes broken zone creation)
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

### Details
Changed from:
```rust
addzone {zone_name} '{{ type {zone_type}; file "{zone_file}"; allow-update {{ key "{update_key_name}"; }}; }};'
```

To:
```rust
addzone {zone_name} {{ type {zone_type}; file "{zone_file}"; allow-update {{ key "{update_key_name}"; }}; }};
```

## [2025-12-02 14:27] - Increase Page TOC Font Size

**Author:** Erick Bourgeois

### Changed
- `docs/theme/custom.css`: Increased font sizes for page-toc navigation elements

### Why
The font sizes for the in-page table of contents (page-toc) on the right side were too small, making navigation difficult to read.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

### Details
Increased font sizes:
- `.page-toc-title`: 0.875rem → 1rem
- `.page-toc nav`: 0.875rem → 1rem
- `.page-toc nav .toc-h3`: 0.8125rem → 0.9375rem
- `.page-toc nav .toc-h4`: 0.8125rem → 0.9375rem

## [2025-12-02 10:15] - Fix ASCII Diagram Alignment in Documentation

**Author:** Erick Bourgeois

### Fixed
- `docs/src/guide/multi-region.md`: Fixed alignment of region boxes in Primary-Secondary deployment pattern diagram
- `docs/src/advanced/ha.md`: Fixed vertical line alignment in Active-Passive HA pattern diagram
- `docs/src/advanced/ha.md`: Fixed vertical line alignment in Anycast pattern diagram
- `docs/src/advanced/zone-transfers.md`: Fixed line spacing in NOTIFY message flow diagram
- `docs/src/development/architecture.md`: Fixed vertical line alignment in Data Flow diagram showing bindy-operator and BIND9 pod structure
- `docs/src/development/cluster-architecture.md`: Reorganized and aligned Bind9Cluster architecture diagram for better readability
- `docs/src/concepts/architecture.md`: Fixed vertical line alignment in High-Level Architecture diagram

### Why
ASCII diagrams had misaligned vertical lines, shifted boxes, and inconsistent spacing that made them difficult to read in monospace environments. This affected the visual clarity of architecture documentation.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

## [2025-12-02 00:30] - Add Structured RNDC Error Parsing with RndcError Type

**Author:** Erick Bourgeois

### Added
- [src/bind9.rs:71-128](src/bind9.rs#L71-L128): New `RndcError` type with structured fields (command, error, details)
- [src/bind9_tests.rs:1866-1945](src/bind9_tests.rs#L1866-L1945): Comprehensive unit tests for RNDC error parsing (8 test cases)

### Fixed
- [src/bind9.rs:415-444](src/bind9.rs#L415-L444): Enhanced `exec_rndc_command` to parse structured RNDC errors
- [src/bind9.rs:520-522](src/bind9.rs#L520-L522): Simplified `zone_exists` to rely on improved error handling

### Why
**Root Cause:** The `exec_rndc_command` method was returning `Ok(response_text)` even when BIND9 included error messages in the response (like "not found", "does not exist", "failed", or "error"). This caused ALL RNDC command methods to incorrectly treat failures as successes.

**Impact on All RNDC Methods:**
- `zone_exists()` - Returned `true` for non-existent zones → zones not created
- `add_zone()` - Skipped zone creation thinking zones already existed
- `reload_zone()` - Silent failures if zone didn't exist
- `delete_zone()` - No error if zone already deleted
- `freeze_zone()`, `thaw_zone()` - Silent failures
- `zone_status()` - Returned "success" with error text
- `retransfer()`, `notify()` - Could fail silently

**Bug Symptoms:**
- Zones not being provisioned despite CRD creation
- Silent failures during reconciliation
- Inconsistent state between Kubernetes resources and BIND9 configuration
- No error logs despite actual BIND9 failures

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Critical bug fix - affects all RNDC operations
- [ ] Config change only

### Technical Details

**Root Fix in `exec_rndc_command`:**

**Before:**
```rust
async fn exec_rndc_command(...) -> Result<String> {
    // ... execute command ...
    Ok(result.text.unwrap_or_default())  // ❌ Always returns Ok, even with error text
}
```

**After (with structured error parsing):**
```rust
// New RndcError type for structured error handling
#[derive(Debug, Clone, thiserror::Error)]
#[error("RNDC command '{command}' failed: {error}")]
pub struct RndcError {
    pub command: String,    // e.g., "zonestatus"
    pub error: String,      // e.g., "not found"
    pub details: Option<String>, // e.g., "no matching zone 'example.com' in any view"
}

async fn exec_rndc_command(...) -> Result<String> {
    // ... execute command ...
    let response_text = result.text.unwrap_or_default();

    // Parse structured RNDC errors (format: "rndc: 'command' failed: error\ndetails")
    if let Some(rndc_error) = RndcError::parse(&response_text) {
        error!(
            server = %server_name,
            command = %rndc_error.command,
            error = %rndc_error.error,
            details = ?rndc_error.details,
            "RNDC command failed with structured error"
        );
        return Err(rndc_error.into());
    }

    // Fallback for unstructured errors
    if response_text.to_lowercase().contains("failed") {
        return Err(anyhow!("RNDC command returned error: {response_text}"));
    }

    Ok(response_text)
}
```

**Simplified `zone_exists` (now that errors are properly detected):**
```rust
pub async fn zone_exists(...) -> bool {
    self.zone_status(zone_name, server, key_data).await.is_ok()
}
```

**Benefits:**
1. ✅ **Structured Error Information** - Errors now include command name, error type, and details
2. ✅ **Better Debugging** - Logs show structured fields (command, error, details) for easier troubleshooting
3. ✅ **Type-Safe Error Handling** - Callers can match on specific error types (e.g., "not found" vs "already exists")
4. ✅ **All RNDC Commands Fixed** - Zone operations, reloads, transfers all properly detect failures
5. ✅ **Zone Provisioning Works** - Zones are created when they should be (no more silent skipping)
6. ✅ **Comprehensive Tests** - 8 unit tests cover various error formats and edge cases

**Example Error Output:**
```
rndc: 'zonestatus' failed: not found
no matching zone 'example.com' in any view
```
Parsed into:
```rust
RndcError {
    command: "zonestatus",
    error: "not found",
    details: Some("no matching zone 'example.com' in any view")
}
```

## [2025-12-02 00:16] - Add Interactive Zoom and Pan for Mermaid Diagrams

**Author:** Erick Bourgeois

### Added
- [docs/mermaid-init.js:20-120](docs/mermaid-init.js#L20-L120): Integrated zoom and pan functionality directly into Mermaid initialization to prevent re-rendering loops
- [docs/theme/custom.css:120-129](docs/theme/custom.css#L120-L129): Minimal CSS to enable overflow for Mermaid SVG diagrams

### Why
Complex architecture diagrams and flowcharts in the documentation (like the ones in [architecture.md](docs/src/concepts/architecture.md)) can be difficult to read due to their size and detail. Interactive zoom and pan functionality significantly improves user experience by:
- Allowing readers to zoom in on specific parts of large diagrams
- Enabling panning to explore different sections of complex flowcharts
- Providing easy reset functionality via double-click
- Making complex architecture more accessible

This enhancement makes technical documentation more accessible and easier to navigate, especially for new users learning about the system architecture.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Documentation enhancement only
- [ ] Config change only

### Features
**User Interactions:**
- **Scroll to Zoom**: Use mouse wheel to zoom in/out (0.5x to 5x scale range)
- **Click and Drag to Pan**: Move around large diagrams
- **Double-Click to Reset**: Return to original view
- **Visual Feedback**: Cursor changes to "grab" hand when hovering over diagrams

**Technical Details:**
- Zoom/pan integrated directly into `mermaid-init.js` to prevent infinite rendering loops
- Uses `svg.dataset.zoomEnabled` flag to prevent re-initialization
- Wraps SVG content in `<g>` element for transform operations
- Multiple initialization strategies (Mermaid callback, window load, MutationObserver)
- Console logging for troubleshooting
- Minimal CSS footprint - only sets overflow properties

**Implementation Notes:**
Based on the proven approach from virtrigaud project. Key difference from initial implementation:
- Zoom/pan code integrated into existing mermaid-init.js instead of separate file
- Prevents infinite loop by checking `svg.dataset.zoomEnabled` before initialization
- Simpler CSS that only handles overflow, not styling

## [2025-12-02 00:50] - Make Author Attribution Mandatory in Changelog

**Author:** Erick Bourgeois

### Changed
- [CLAUDE.md:219-224](CLAUDE.md#L219-L224): Made author attribution a **CRITICAL REQUIREMENT** for all changelog entries
- [CLAUDE.md:866-867](CLAUDE.md#L866-L867): Added author verification to PR/Commit checklist
- [CHANGELOG.md](CHANGELOG.md): Added `**Author:** Erick Bourgeois` to all existing changelog entries (6 entries total)

### Why
In a regulated banking environment, all code changes must be auditable and traceable to a specific person for accountability and compliance purposes. Author attribution in the changelog:
- Provides clear accountability for all changes
- Enables audit trails for regulatory compliance
- Helps track who requested or approved changes
- Supports incident investigation and root cause analysis
- Ensures proper attribution for code contributions

Without mandatory author attribution, it's impossible to determine who was responsible for specific changes, which violates compliance requirements.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Documentation policy change
- [x] All existing entries updated

### Details
**New Requirements**:
- Every changelog entry MUST include `**Author:** [Name]` line immediately after the title
- NO exceptions - this is a critical compliance requirement
- If author is unknown, use "Unknown" but investigate to identify proper author
- Added to PR/Commit checklist as mandatory verification step

**Format**:
```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

**Author:** [Author Name]

### Changed
...
```

**Retroactive Updates**:
All 6 existing changelog entries have been updated with author attribution:
- ✅ [2025-12-02 00:45] - Consolidate All Constants into Single Module
- ✅ [2025-12-02 00:30] - Complete All DNS Record Types Implementation
- ✅ [2025-12-02 00:15] - Eliminate Magic Numbers from Codebase
- ✅ [2025-12-01 23:50] - Add Magic Numbers Policy to Code Quality Standards
- ✅ [2025-12-01 23:19] - Implement Dynamic DNS Record Updates (RFC 2136)
- ✅ [2025-12-01 22:29] - Fix DNSZone Creation with allow-new-zones and Correct Paths

## [2025-12-02 00:45] - Consolidate All Constants into Single Module

**Author:** Erick Bourgeois

### Changed
- [src/constants.rs:9-53](src/constants.rs#L9-L53): Merged all API constants from `api_constants.rs` into `constants.rs` under new "API Constants" section
- [src/bind9_resources.rs:9-14](src/bind9_resources.rs#L9-L14): Updated imports to use `constants` instead of `api_constants`
- [src/bind9_resources_tests.rs:8](src/bind9_resources_tests.rs#L8): Updated imports to use `constants` instead of `api_constants`
- [src/lib.rs:60-66](src/lib.rs#L60-L66): Removed `pub mod api_constants;` module declaration

### Removed
- [src/api_constants.rs](src/api_constants.rs): Deleted file - all constants moved to `constants.rs`

### Why
Having constants split across multiple files (`api_constants.rs` and `constants.rs`) violated the single source of truth principle and made it harder to find constants. This change:
- Consolidates ALL constants (API, DNS, Kubernetes, etc.) into a single `constants.rs` file
- Improves discoverability - developers only need to check one file for constants
- Follows the CLAUDE.md policy: "Use Global Constants for Repeated Strings"
- Eliminates confusion about where to add new constants

### Impact
- [ ] Breaking change (internal refactor only)
- [ ] Requires cluster rollout
- [x] Code organization improvement
- [x] All tests passing

### Details
**Organization**:
All constants are now grouped by category in `src/constants.rs`:
1. API Constants (CRD kinds, API group/version)
2. DNS Protocol Constants (ports, TTLs, timeouts)
3. Kubernetes Health Check Constants
4. Controller Error Handling Constants
5. Leader Election Constants
6. BIND9 Version Constants
7. Runtime Constants
8. Replica Count Constants

**Migration**:
- All imports of `crate::api_constants::*` changed to `crate::constants::*`
- No functional changes - purely organizational

**Code Quality**:
- ✅ `cargo fmt` passed
- ✅ `cargo clippy -- -D warnings` passed
- ✅ `cargo test` passed (217 tests, 8 ignored)

## [2025-12-02 00:30] - Complete All DNS Record Types Implementation

**Author:** Erick Bourgeois

### Added
- [src/bind9.rs:869-940](src/bind9.rs#L869-L940): Implemented `add_aaaa_record()` with RFC 2136 dynamic DNS update for IPv6 addresses
- [src/bind9.rs:942-1005](src/bind9.rs#L942-L1005): Implemented `add_mx_record()` with RFC 2136 dynamic DNS update for mail exchange records
- [src/bind9.rs:1007-1069](src/bind9.rs#L1007-L1069): Implemented `add_ns_record()` with RFC 2136 dynamic DNS update for nameserver delegation
- [src/bind9.rs:1071-1165](src/bind9.rs#L1071-L1165): Implemented `add_srv_record()` with RFC 2136 dynamic DNS update for service location records
- [src/bind9.rs:1167-1302](src/bind9.rs#L1167-L1302): Implemented `add_caa_record()` with RFC 2136 dynamic DNS update for certificate authority authorization
- [Cargo.toml:41](Cargo.toml#L41): Added `url` crate dependency for CAA record iodef URL parsing
- [src/bind9_tests.rs:753](src/bind9_tests.rs#L753): Added `#[ignore]` attribute to AAAA record test
- [src/bind9_tests.rs:826](src/bind9_tests.rs#L826): Added `#[ignore]` attribute to MX record test
- [src/bind9_tests.rs:851](src/bind9_tests.rs#L851): Added `#[ignore]` attribute to NS record test
- [src/bind9_tests.rs:875](src/bind9_tests.rs#L875): Added `#[ignore]` attribute to SRV record test
- [src/bind9_tests.rs:905](src/bind9_tests.rs#L905): Added `#[ignore]` attribute to CAA record test

### Changed
- [src/bind9.rs:646](src/bind9.rs#L646): Fixed TSIG signer creation to convert `TSIG_FUDGE_TIME_SECS` from `u64` to `u16`
- [src/constants.rs:32](src/constants.rs#L32): Fixed clippy warning by adding separator to `DEFAULT_SOA_EXPIRE_SECS` constant (604_800)

### Why
The user requested implementation of ALL DNS record types with actual dynamic DNS updates to BIND9 using RFC 2136 protocol. Previously, only A, CNAME, and TXT records were implemented. This change completes the implementation by adding:
- **AAAA**: IPv6 address records for dual-stack support
- **MX**: Mail exchange records with priority for email routing
- **NS**: Nameserver records for DNS delegation
- **SRV**: Service location records with priority, weight, and port
- **CAA**: Certificate authority authorization with support for issue, issuewild, and iodef tags

All record implementations use TSIG authentication for security and execute in `spawn_blocking` to handle synchronous hickory-client API.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Feature addition
- [x] All tests passing

### Details
**Technical Implementation**:
- All record types use hickory-client for DNS updates over UDP port 53
- TSIG authentication using bindy-operator key for all updates
- Proper type conversions: i32→u16 for SRV priority/weight/port, i32→u8 for CAA flags
- CAA record supports three tags: issue, issuewild, iodef
- All methods execute in `tokio::task::spawn_blocking` since hickory-client is synchronous
- Default TTL of 300 seconds from `DEFAULT_DNS_RECORD_TTL_SECS` constant

**Testing**:
- All placeholder tests updated to `#[ignore]` since they require real BIND9 server
- Tests can be run with `cargo test -- --ignored` when BIND9 server is available
- All non-ignored tests passing (217 tests)

**Code Quality**:
- ✅ `cargo fmt` passed
- ✅ `cargo clippy -- -D warnings` passed
- ✅ `cargo test` passed (217 tests, 8 ignored)
- All rustdoc comments updated with accurate error descriptions
- Proper error handling with context messages for all failure scenarios

## [2025-12-02 00:15] - Eliminate Magic Numbers from Codebase

**Author:** Erick Bourgeois

### Added
- [src/constants.rs](src/constants.rs): Created new global constants module with all numeric constants
  - DNS protocol constants (ports, TTLs, timeouts)
  - Kubernetes health check constants (probe delays, periods, thresholds)
  - Controller error handling constants
  - Leader election constants
  - BIND9 version constants
  - Runtime constants
  - Replica count constants

### Changed
- [src/bind9.rs](src/bind9.rs): Replaced all magic numbers with named constants from `constants` module
  - TTL values now use `DEFAULT_DNS_RECORD_TTL_SECS` and `DEFAULT_ZONE_TTL_SECS`
  - SOA record values use `DEFAULT_SOA_REFRESH_SECS`, `DEFAULT_SOA_RETRY_SECS`, etc.
  - Port numbers use `DNS_PORT` and `RNDC_PORT` constants
- [src/bind9_resources.rs](src/bind9_resources.rs): Updated all numeric literals to use named constants
  - Health check probes use `LIVENESS_*` and `READINESS_*` constants
  - Container ports use `DNS_PORT` and `RNDC_PORT`
- [src/main.rs](src/main.rs): Replaced runtime worker thread count with `TOKIO_WORKER_THREADS`
- [src/reconcilers/bind9cluster.rs](src/reconcilers/bind9cluster.rs): Updated error requeue duration to use `ERROR_REQUEUE_DURATION_SECS`
- [src/lib.rs](src/lib.rs): Added `pub mod constants;` export

### Why
Magic numbers (numeric literals other than 0 or 1) scattered throughout code reduce readability and maintainability. This change:
- Makes all numeric values self-documenting through descriptive constant names
- Allows values to be changed in a single location (`src/constants.rs`)
- Improves code readability by explaining the purpose of each number
- Enforces the "Use Global Constants for Repeated Strings" policy from CLAUDE.md
- Eliminates the need to search the codebase to understand what specific numbers mean

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Code quality improvement
- [x] All tests passing

### Details
**Constants Organization**:
- Grouped by category (DNS, Kubernetes, Controller, etc.)
- Each constant has a descriptive name explaining its purpose
- Rustdoc comments explain what each value represents
- Constants use proper numeric separators for readability (e.g., `604_800` instead of `604800`)

**Verification**:
```bash
# Before: Many magic numbers throughout codebase
# After: All numeric literals (except 0 and 1) are named constants
```

**Examples**:
- ✅ Before: `ttl.unwrap_or(300)`
- ✅ After: `ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS)`

- ✅ Before: `initial_delay_seconds: Some(30)`
- ✅ After: `initial_delay_seconds: Some(LIVENESS_INITIAL_DELAY_SECS)`

## [2025-12-01 23:50] - Add Magic Numbers Policy to Code Quality Standards

**Author:** Erick Bourgeois

### Changed
- [CLAUDE.md:349-454](CLAUDE.md#L349-L454): Added "Magic Numbers Rule" to Rust Style Guidelines section

### Why
Magic numbers (numeric literals other than 0 or 1) scattered throughout code reduce readability and maintainability. Named constants make code self-documenting and allow values to be changed in a single location.

This policy enforces that:
- All numeric literals except `0` and `1` must be declared as named constants
- Constant names must explain the *purpose* of the value, not just restate it
- Constants should be grouped logically at module or crate level

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

### Details
**New Requirements**:
- No numeric literals other than `0` or `1` are allowed in code
- All numbers must be declared as named constants with descriptive names
- Special cases covered: unit conversions, array indexing, buffer sizes
- Verification command provided to find magic numbers in codebase

**Examples Added**:
- ✅ GOOD: `const DEFAULT_ZONE_TTL: u32 = 3600;`
- ❌ BAD: `ttl.unwrap_or(3600)` (no explanation of what 3600 means)

This aligns with existing code quality requirements for using global constants for repeated strings.

## [2025-12-01 23:19] - Implement Dynamic DNS Record Updates (RFC 2136)

**Author:** Erick Bourgeois

### Added
- [Cargo.toml:39-40](Cargo.toml#L39-L40): Added `hickory-client` and `hickory-proto` dependencies with `dnssec` feature for dynamic DNS updates
- [src/bind9.rs:619-650](src/bind9.rs#L619-L650): Implemented `create_tsig_signer()` helper method to convert RNDC key data to hickory TSIG signer
- [src/bind9.rs:652-741](src/bind9.rs#L652-L741): Implemented `add_a_record()` with actual RFC 2136 dynamic DNS update using hickory-client
- [src/bind9.rs:743-806](src/bind9.rs#L743-L806): Implemented `add_cname_record()` with RFC 2136 dynamic DNS update
- [src/bind9.rs:808-867](src/bind9.rs#L808-L867): Implemented `add_txt_record()` with RFC 2136 dynamic DNS update

###Changed
- [src/bind9.rs:45-55](src/bind9.rs#L45-L55): Added hickory-client imports for DNS client, TSIG authentication, and record types
- [src/bind9_tests.rs:727-750](src/bind9_tests.rs#L727-L750): Updated record tests to mark them as `#[ignore]` since they now require a real BIND9 server with TSIG authentication

### Why
The operator needs to dynamically update DNS records in BIND9 zones without reloading the entire zone file. The previous implementation only logged what would be done. This change implements actual RFC 2136 dynamic DNS updates using TSIG authentication for security.

**Use case**: When a `DNSRecord` custom resource is created/updated in Kubernetes, the operator should immediately update the DNS record in the running BIND9 server without disrupting other records or requiring a zone reload.

### Impact
- [x] Breaking change (placeholder methods now make actual DNS updates)
- [ ] Requires cluster rollout
- [x] Requires BIND9 configuration with `allow-update { key "bindy-operator"; };`
- [x] Feature enhancement

### Details
**Technical Implementation**:
- Uses hickory-client library for DNS protocol implementation
- TSIG (Transaction Signature) authentication using HMAC algorithms (MD5, SHA1, SHA224, SHA256, SHA384, SHA512)
- Updates sent over UDP to BIND9 server on port 53
- All methods execute in `tokio::task::spawn_blocking` since hickory-client is synchronous
- Response codes validated (NoError expected, errors returned with context)

**Security**:
- TSIG key authentication prevents unauthorized DNS updates
- TODO: Create separate key for zone updates (currently reuses bindy-operator RNDC key)

**Error Handling**:
- Connection failures: Returns error with server address context
- Invalid parameters: Returns error with parameter value context
- DNS update rejection: Returns error with response code
- Task panic: Returns error with context wrapper

**Testing**:
- Tests marked with `#[ignore]` attribute
- Tests require:
  - Running BIND9 server
  - TSIG key configured
  - Zone with `allow-update` directive
- Can be run with: `cargo test -- --ignored`

## [2025-12-01 22:29] - Fix DNSZone Creation with allow-new-zones and Correct Paths

**Author:** Erick Bourgeois

### Changed
- [templates/named.conf.options.tmpl:10](templates/named.conf.options.tmpl#L10): Added `allow-new-zones yes;` directive to BIND9 configuration
- [src/reconcilers/dnszone.rs:115](src/reconcilers/dnszone.rs#L115): Changed zone file path from `/var/lib/bind/` to `/var/cache/bind/`
- [src/reconcilers/dnszone.rs:153-156](src/reconcilers/dnszone.rs#L153-L156): Removed unnecessary `rndc reload` loop after `rndc addzone`
- [src/bind9.rs:524-543](src/bind9.rs#L524-L543): Added `allow-update { key "<update_key_name>"; }` to zone configuration in `add_zone()` method

### Why
BIND9 was refusing to create zones dynamically via `rndc addzone` because the `allow-new-zones yes;` directive was missing from named.conf. Without this directive, BIND9 rejects all `addzone` commands with "permission denied" errors.

Additionally:
- Zone files must be in `/var/cache/bind/` (writable directory) not `/var/lib/bind/` (read-only in container)
- The `rndc reload` after `addzone` is unnecessary and wrong - `addzone` automatically loads the zone
- Dynamic DNS updates require `allow-update` directive in zone configuration

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (ConfigMap must be updated)
- [x] Bug fix
- [x] Enables dynamic zone creation

### Details
**Root Cause**:
User identified: "the real fix is to add 'allow-new-zones yes;' to named.conf"

**BIND9 Behavior**:
- Without `allow-new-zones yes;`: `rndc addzone` fails with "permission denied"
- With `allow-new-zones yes;`: `rndc addzone` creates zone and loads it automatically
- Zone file path must be writable by named process

**Zone Configuration**:
```
addzone example.com '{ type primary; file "/var/cache/bind/example.com.zone"; allow-update { key "bindy-operator"; }; };'
```

**TODO**: Create separate TSIG key for zone updates (currently reuses bindy-operator RNDC key)

**Verification**:
```bash
# In BIND9 pod:
rndc zonestatus example.com  # Should show zone details
rndc showzone example.com    # Should show zone configuration
ls -la /var/cache/bind/      # Should show zone files
```
