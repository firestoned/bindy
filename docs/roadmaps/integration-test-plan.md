# Integration Test Plan - Phase 4 & 5

**Date:** 2025-12-27
**Author:** Erick Bourgeois
**Phases:** Phase 4 (Hash Detection) + Phase 5 (Record Discovery)

## Overview

This integration test plan validates the complete label selector watch implementation with hash-based change detection on a Kind cluster.

## Test Environment

- **Cluster:** Kind (Kubernetes in Docker)
- **Operator:** bindy (locally built image)
- **BIND9:** Deployed via Bind9Instance CRD
- **DNS Records:** All 8 types (A, AAAA, TXT, CNAME, MX, NS, SRV, CAA)

## Pre-Test Setup

### 1. Build and Load Image

```bash
# Build release binary
cargo build --release

# Build Docker image
docker build -t bindy:test .

# Load into Kind cluster
kind load docker-image bindy:test --name bindy-test
```

### 2. Deploy CRDs and Operator

```bash
# Apply CRDs
kubectl apply -f deploy/crds/

# Deploy operator with test image
kubectl apply -f deploy/operator.yaml
# (Edit to use bindy:test image)
```

### 3. Deploy BIND9 Instance

```bash
kubectl apply -f examples/cluster-bind9-provider.yaml
# Wait for BIND9 pods to be Ready
kubectl wait --for=condition=ready pod -l app=bind9 -n dns-system --timeout=300s
```

## Test Cases

### Test 1: Hash-Based Change Detection (Phase 4)

**Objective:** Verify records only update DNS when data changes, not on metadata-only changes.

**Steps:**

1. Create an A record with specific values:
```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: test-hash-www
  namespace: default
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

2. **Verify initial reconciliation:**
   - Check `status.record_hash` is populated
   - Check `status.last_updated` is populated
   - Check `status.conditions` shows Ready=True

3. **Test metadata-only change (should NOT update DNS):**
   ```bash
   kubectl annotate arecord test-hash-www test-annotation=foo
   ```
   - Verify `status.record_hash` UNCHANGED
   - Verify `status.last_updated` UNCHANGED
   - Check operator logs: Should say "data unchanged (hash match), skipping DNS update"

4. **Test data change (SHOULD update DNS):**
   ```bash
   kubectl patch arecord test-hash-www --type=merge -p '{"spec":{"ipv4Address":"192.0.2.2"}}'
   ```
   - Verify `status.record_hash` CHANGED
   - Verify `status.last_updated` CHANGED to new timestamp
   - Check operator logs: Should say "data changed (hash mismatch), updating DNS"

5. **Test TTL change (SHOULD update DNS):**
   ```bash
   kubectl patch arecord test-hash-www --type=merge -p '{"spec":{"ttl":600}}'
   ```
   - Verify `status.record_hash` CHANGED
   - Verify `status.last_updated` CHANGED

**Expected Results:**
- ✅ Metadata changes do NOT trigger DNS updates
- ✅ Spec changes (IP, TTL, name) DO trigger DNS updates
- ✅ Hash and timestamp track actual data changes
- ✅ Operator logs clearly indicate when updates are skipped vs performed

### Test 2: Label Selector Watching (Phase 5)

**Objective:** Verify DNSZone watches for records matching its label selectors and triggers reconciliation.

**Steps:**

1. Create DNSZone with label selector:
```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: default
spec:
  zoneName: example.com
  clusterProviderRef: bind9-cluster
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com
  soa:
    primaryNameserver: ns1.example.com
    adminEmail: admin@example.com
```

2. **Verify DNSZone is Ready:**
   ```bash
   kubectl wait --for=condition=Ready dnszone/example-com --timeout=60s
   ```

3. **Create matching A record:**
   ```yaml
   apiVersion: bindy.firestoned.io/v1beta1
   kind: ARecord
   metadata:
     name: watch-test-www
     namespace: default
     labels:
       zone: example.com  # Matches zone selector
   spec:
     name: www
     ipv4Address: "192.0.2.10"
     ttl: 300
   ```

4. **Verify watch triggered DNSZone reconciliation:**
   - Check operator logs for: "Reconciling DNSZone: default/example-com"
   - Verify ARecord got `bindy.firestoned.io/zone` annotation set to "example.com"
   - Verify DNSZone `status.records` includes the new record

5. **Update record labels to NOT match:**
   ```bash
   kubectl label arecord watch-test-www zone=other.com --overwrite
   ```
   - Verify DNSZone reconciliation triggered
   - Verify `bindy.firestoned.io/zone` annotation removed from record
   - Verify DNSZone `status.records` no longer includes the record

6. **Delete matching record:**
   ```bash
   kubectl delete arecord watch-test-www
   ```
   - Verify DNSZone reconciliation triggered
   - Verify DNSZone `status.records` updated (record removed)

**Expected Results:**
- ✅ Creating matching record triggers DNSZone reconciliation
- ✅ Updating record labels triggers DNSZone reconciliation
- ✅ Deleting matching record triggers DNSZone reconciliation
- ✅ DNSZone controller watches all 8 record types

### Test 3: DNSZone status.records Population (Phase 5)

**Objective:** Verify DNSZone accurately discovers and tracks all matching records.

**Steps:**

1. Create multiple records with matching labels:
```bash
# Create 3 different record types, all matching zone selector
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: status-test-www
  namespace: default
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.20"
---
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: status-test-www-ipv6
  namespace: default
  labels:
    zone: example.com
spec:
  name: www
  ipv6Address: "2001:db8::1"
---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: status-test-mail
  namespace: default
  labels:
    zone: example.com
spec:
  name: "@"
  priority: 10
  mailServer: mail.example.com.
EOF
```

2. **Verify DNSZone status.records populated:**
   ```bash
   kubectl get dnszone example-com -o jsonpath='{.status.records}' | jq
   ```

   Expected output:
   ```json
   [
     {
       "apiVersion": "bindy.firestoned.io/v1beta1",
       "kind": "ARecord",
       "name": "status-test-www",
       "namespace": "default"
     },
     {
       "apiVersion": "bindy.firestoned.io/v1beta1",
       "kind": "AAAARecord",
       "name": "status-test-www-ipv6",
       "namespace": "default"
     },
     {
       "apiVersion": "bindy.firestoned.io/v1beta1",
       "kind": "MXRecord",
       "name": "status-test-mail",
       "namespace": "default"
     }
   ]
   ```

3. **Verify record_count field:**
   ```bash
   kubectl get dnszone example-com -o jsonpath='{.status.recordCount}'
   # Should output: 3
   ```

4. **Create non-matching record:**
   ```yaml
   apiVersion: bindy.firestoned.io/v1beta1
   kind: ARecord
   metadata:
     name: status-test-other
     namespace: default
     labels:
       zone: other.com  # Does NOT match
   spec:
     name: other
     ipv4Address: "192.0.2.99"
   ```

5. **Verify non-matching record NOT in status:**
   ```bash
   kubectl get dnszone example-com -o jsonpath='{.status.records}' | jq '.[] | select(.name=="status-test-other")'
   # Should output nothing (empty)
   ```

**Expected Results:**
- ✅ All matching records appear in `status.records`
- ✅ Non-matching records do NOT appear in `status.records`
- ✅ `status.recordCount` reflects accurate count
- ✅ RecordReference has correct apiVersion, kind, name, namespace

### Test 4: Record Readiness and Zone Transfers (Phase 5)

**Objective:** Verify zone transfers only occur when all records are Ready.

**Steps:**

1. Create DNSZone with secondary configuration
2. Create multiple records (some Ready, some not)
3. Verify zone transfer does NOT happen until all Ready
4. Fix failing records, verify zone transfer triggers

**Expected Results:**
- ✅ Zone transfer waits for all records Ready
- ✅ `check_all_records_ready()` correctly detects readiness
- ✅ Operator logs show "Triggering zone transfers" only when ready

### Test 5: All 8 Record Types (Phase 4)

**Objective:** Verify hash detection works for all record types.

**Test each record type:**
1. ARecord - ✅
2. AAAARecord - ✅
3. TXTRecord - ✅
4. CNAMERecord - ✅
5. MXRecord - ✅
6. NSRecord - ✅
7. SRVRecord - ✅
8. CAARecord - ✅

**For each:**
- Create record
- Verify hash populated
- Change metadata only (no DNS update)
- Change spec (DNS update happens)

## Success Criteria

### Phase 4: Hash Detection
- [ ] All 8 record types calculate and store hash
- [ ] Metadata-only changes skip DNS updates
- [ ] Spec changes trigger DNS updates
- [ ] Logs clearly indicate skip vs update
- [ ] Hash and timestamp updated on data changes

### Phase 5: Record Discovery
- [ ] DNSZone watches all 8 record types
- [ ] Creating matching record triggers zone reconciliation
- [ ] Updating record labels triggers zone reconciliation
- [ ] Deleting matching record triggers zone reconciliation
- [ ] status.records accurately reflects matching records
- [ ] status.recordCount is accurate
- [ ] Records tagged with zone annotation
- [ ] Zone transfers wait for all records Ready

## Cleanup

```bash
# Delete all test resources
kubectl delete arecord,aaaarecord,txtrecord,cnamerecord,mxrecord,nsrecord,srvrecord,caarecord,dnszone --all -n default

# Delete BIND9 instance
kubectl delete -f examples/cluster-bind9-provider.yaml

# Delete operator
kubectl delete -f deploy/operator.yaml

# Delete CRDs
kubectl delete -f deploy/crds/

# Delete Kind cluster
kind delete cluster --name bindy-test
```

## Notes

- All tests should be run on a fresh Kind cluster
- Operator logs should be monitored during tests: `kubectl logs -f -n dns-system deploy/bindy-operator`
- BIND9 logs can be checked: `kubectl logs -f -n dns-system -l app=bind9`
- Use `kubectl describe` to check resource status and events
