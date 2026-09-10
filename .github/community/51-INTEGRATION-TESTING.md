# DNSZone Operator Consolidation - Integration Test Plan

> **Status:** ✅ Done — superseded by the shipped harness: `tests/integration_test.sh`, `tests/multi_tenancy_integration.rs`, and the `kind-integration-test` / `kind-integration-test-ci` targets in the `Makefile`. Kept for the record of what phase 7 required.
>
> *Migrated 2026-09-10 from the external roadmap set. Status verified against `fix-idempotency` @ `648ff7a`.*

---

**Date:** 2026-01-06
**Author:** Erick Bourgeois
**Original status (as written):** Ready for Execution
**Phase:** Phase 7 - Integration Testing

---

## Overview

This document outlines the integration testing strategy for validating the DNSZone operator consolidation in a live Kubernetes cluster.

## Prerequisites

### Cluster Requirements
- Kubernetes cluster (v1.24+)
- kubectl configured with cluster access  
- Namespace: `bindy-system`

### Component Requirements
- Bindy operator image built
- BIND9 instances with bindcar API
- CRDs deployed

## Test Suite

### Test 1: DNSZone with `clusterRef`
Verify DNSZone selects instances via `spec.clusterRef`

### Test 2: DNSZone with `bind9InstancesFrom`
Verify DNSZone selects instances via label selectors

### Test 3: Union Selection (Both Methods)
Verify instances from BOTH clusterRef AND bind9InstancesFrom

### Test 4: Zone Synchronization
Verify zones created in BIND9 via bindcar API

### Test 5: Status Transitions
Verify status: Claimed → Configured → Unclaimed

### Test 6: Error Handling
Verify Failed status with error messages

### Test 7: Dynamic Instance Changes
Verify instance addition/removal detected

## Success Criteria

- ✅ All test scenarios pass
- ✅ Zones queryable via DNS
- ✅ Status transitions correctly  
- ✅ No legacy status fields

---

See full test details in this document.
