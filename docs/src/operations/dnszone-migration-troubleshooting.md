# DNSZone Operator Consolidation Migration Troubleshooting

This guide helps troubleshoot issues during and after the DNSZone operator consolidation migration (Phases 1-8, January 2026).

## Pre-Migration Checklist

Before upgrading, verify:

1. **Export existing resources**:
   ```bash
   kubectl get dnszones -A -o yaml > dnszones-backup-$(date +%Y%m%d-%H%M%S).yaml
   kubectl get bind9instances -A -o yaml > bind9instances-backup-$(date +%Y%m%d-%H%M%S).yaml
   ```

2. **Check Bindy operator version**:
   ```bash
   kubectl get deployment -n dns-system bindy-operator -o jsonpath='{.spec.template.spec.containers[0].image}'
   ```

3. **Verify all zones are healthy before upgrade**:
   ```bash
   kubectl get dnszones -A -o custom-columns=NAME:.metadata.name,NAMESPACE:.metadata.namespace,READY:.status.conditions[?(@.type=='Ready')].status
   ```

## Common Migration Issues

### Issue 1: DNSZone Status Fields Missing After Upgrade

**Symptoms:**
- `status.syncStatus[]` is empty or missing
- `status.syncedInstancesCount` is null
- `status.totalInstancesCount` is null

**Root Cause:**
These fields were removed as part of the consolidation. The new architecture uses only `status.instances[]`.

**Resolution:**
1. Verify the new operator is running:
   ```bash
   kubectl get pods -n dns-system -l app=bindy-operator
   kubectl logs -n dns-system -l app=bindy-operator --tail=100
   ```

2. Check `status.instances[]` field instead:
   ```bash
   kubectl get dnszone <zone-name> -n <namespace> -o jsonpath='{.status.instances}' | jq .
   ```

3. If `status.instances[]` is empty, trigger a reconciliation:
   ```bash
   kubectl annotate dnszone <zone-name> -n <namespace> bindy.firestoned.io/force-reconcile="$(date +%s)"
   ```

**Expected Status:**
```yaml
status:
  instances:
    - apiVersion: bindy.firestoned.io/v1beta1
      kind: Bind9Instance
      name: primary-dns-0
      namespace: dns-system
      status: Configured
      lastReconciledAt: "2026-01-06T10:00:00Z"
```

### Issue 2: Bind9Instance Missing `selectedZones` Field

**Symptoms:**
- `Bind9Instance.status.selectedZones[]` is empty or missing
- Monitoring dashboards showing zero zones per instance

**Root Cause:**
The `selectedZones` reverse reference was removed. This field created circular dependencies and is no longer maintained.

**Resolution:**
1. Update monitoring queries to use DNSZone status instead:
   ```bash
   # OLD (broken):
   kubectl get bind9instance primary-dns-0 -o jsonpath='{.status.selectedZones}'
   
   # NEW (correct):
   kubectl get dnszones -A -o json | jq -r '.items[] | select(.status.instances[]?.name == "primary-dns-0") | .metadata.name'
   ```

2. Update dashboards to query DNSZone resources for instance relationships

**Migration Note:**
This is an intentional breaking change. The DNSZone operator now owns the instance-zone relationship.

### Issue 3: Zones Not Synchronizing to Instances

**Symptoms:**
- `status.instances[].status` is `Claimed` or `Failed` instead of `Configured`
- Zones missing from BIND9 instance configuration
- Ready condition is `False`

**Diagnosis:**

1. **Check DNSZone status**:
   ```bash
   kubectl get dnszone <zone-name> -n <namespace> -o yaml | grep -A20 "^status:"
   ```

2. **Look for Failed instances**:
   ```bash
   kubectl get dnszone <zone-name> -n <namespace> -o jsonpath='{.status.instances[?(@.status=="Failed")]}' | jq .
   ```

3. **Check error messages**:
   ```bash
   kubectl get dnszone <zone-name> -n <namespace> -o jsonpath='{.status.instances[*].message}' | jq -r .
   ```

4. **Check operator logs**:
   ```bash
   kubectl logs -n dns-system -l app=bindy-operator --tail=100 | grep -E "(ERROR|WARN|add_zones)"
   ```

**Common Root Causes:**

#### A. Bindcar API Unavailable

**Error Message**: `"HTTP 500: bindcar API unavailable"`

**Resolution**:
1. Verify Bind9Instance pod is running:
   ```bash
   kubectl get pods -n <namespace> -l bindy.firestoned.io/instance=<instance-name>
   ```

2. Check bindcar container logs:
   ```bash
   kubectl logs -n <namespace> <instance-pod> -c bindcar
   ```

3. Verify bindcar service is accessible:
   ```bash
   kubectl get svc -n <namespace> | grep bindcar
   kubectl port-forward -n <namespace> <instance-pod> 8080:8080 &
   curl http://localhost:8080/health
   ```

#### B. Instance Not Selected (Wrong Labels/ClusterRef)

**Error**: Instance remains in `Claimed` state indefinitely

**Resolution**:
1. Check if instance matches the DNSZone selector:
   ```bash
   # Check DNSZone selectors
   kubectl get dnszone <zone-name> -n <namespace> -o yaml | grep -A10 "bind9InstancesFrom:"
   
   # Check instance labels
   kubectl get bind9instance <instance-name> -n <namespace> -o jsonpath='{.metadata.labels}' | jq .
   ```

2. Verify clusterRef if used:
   ```bash
   # DNSZone clusterRef
   kubectl get dnszone <zone-name> -n <namespace> -o jsonpath='{.spec.clusterRef}'
   
   # Instance clusterRef
   kubectl get bind9instance <instance-name> -n <namespace> -o jsonpath='{.spec.clusterRef}'
   ```

3. If labels/clusterRef don't match, instance won't be selected - this is correct behavior

#### C. Operator RBAC Permissions Missing

**Error**: `"Forbidden: User system:serviceaccount:dns-system:bindy-operator cannot update resource..."`

**Resolution**:
1. Verify RBAC is deployed:
   ```bash
   kubectl get clusterrole bindy-operator
   kubectl get clusterrolebinding bindy-operator
   kubectl get serviceaccount -n dns-system bindy-operator
   ```

2. Redeploy RBAC if needed:
   ```bash
   kubectl apply -f deploy/rbac/
   ```

### Issue 4: DNSZone Ready Condition is False

**Symptoms:**
- `status.conditions[?(@.type=='Ready')].status` is `"False"`
- Some instances show `Configured`, others show `Failed`

**Diagnosis:**

1. **Check Ready condition details**:
   ```bash
   kubectl get dnszone <zone-name> -n <namespace> -o jsonpath='{.status.conditions[?(@.type=="Ready")]}' | jq .
   ```

2. **Count configured vs total instances**:
   ```bash
   kubectl get dnszone <zone-name> -n <namespace> -o json | jq '.status.instances | map(select(.status == "Configured")) | length'
   kubectl get dnszone <zone-name> -n <namespace> -o json | jq '.status.instances | length'
   ```

**Resolution:**

The Ready condition is `True` ONLY when **ALL** instances are in `Configured` status. If any instance is `Failed` or `Claimed`, Ready will be `False`.

1. Identify failed instances and check their error messages (see Issue 3)
2. Fix the root cause for each failed instance
3. Operator will automatically retry and update status

### Issue 5: Duplicate Instances in Status

**Symptoms:**
- Same instance appears multiple times in `status.instances[]`
- Instance count is higher than expected

**Root Cause:**
Instance matches BOTH `clusterRef` AND `bind9InstancesFrom` selectors, but deduplication failed.

**Diagnosis:**
```bash
kubectl get dnszone <zone-name> -n <namespace> -o json | jq '.status.instances | group_by(.name) | map(select(length > 1))'
```

**Resolution:**
This should not happen (deduplication is automatic), but if it does:

1. Check operator version (bug may be fixed in newer version)
2. Force reconciliation:
   ```bash
   kubectl annotate dnszone <zone-name> -n <namespace> bindy.firestoned.io/force-reconcile="$(date +%s)" --overwrite
   ```

3. If issue persists, file a bug report with operator logs

### Issue 6: Old ZoneSync Operator Still Running

**Symptoms:**
- Two operators reconciling the same DNSZone
- Conflicting status updates
- `status.syncStatus[]` is being updated (should not exist)

**Diagnosis:**
```bash
# Check for multiple bindy operator pods
kubectl get pods -n dns-system -l app=bindy-operator

# Check operator version
kubectl get deployment -n dns-system bindy-operator -o jsonpath='{.spec.template.spec.containers[0].image}'
```

**Resolution:**

1. **Verify correct image version**:
   ```bash
   kubectl set image deployment/bindy-operator -n dns-system bindy-operator=ghcr.io/firestoned/bindy:v0.X.Y
   ```

2. **Force rollout**:
   ```bash
   kubectl rollout restart deployment/bindy-operator -n dns-system
   kubectl rollout status deployment/bindy-operator -n dns-system
   ```

3. **Verify old pods are terminated**:
   ```bash
   kubectl get pods -n dns-system -l app=bindy-operator --show-labels
   ```

## Rollback Procedure

If migration fails and you need to rollback:

1. **Restore previous operator version**:
   ```bash
   kubectl set image deployment/bindy-operator -n dns-system bindy-operator=ghcr.io/firestoned/bindy:v0.PREVIOUS.VERSION
   kubectl rollout status deployment/bindy-operator -n dns-system
   ```

2. **Restore old CRDs** (if CRD update was applied):
   ```bash
   kubectl replace --force -f deploy/crds-old/  # backup of old CRDs
   ```

3. **Restore DNSZone resources from backup**:
   ```bash
   kubectl apply -f dnszones-backup-TIMESTAMP.yaml
   ```

4. **Verify zones are working**:
   ```bash
   kubectl get dnszones -A -o custom-columns=NAME:.metadata.name,READY:.status.conditions[?(@.type=='Ready')].status
   ```

## Post-Migration Validation

After successful migration, verify:

1. **All zones show Ready=True**:
   ```bash
   kubectl get dnszones -A -o custom-columns=NAME:.metadata.name,NAMESPACE:.metadata.namespace,READY:.status.conditions[?(@.type=='Ready')].status | grep -v True
   ```
   (Output should be empty)

2. **All instances are Configured**:
   ```bash
   kubectl get dnszones -A -o json | jq -r '.items[] | "\(.metadata.name): " + (.status.instances | map(select(.status != "Configured")) | length | tostring)'
   ```
   (All counts should be 0)

3. **No legacy status fields exist**:
   ```bash
   kubectl get dnszones -A -o json | jq '.items[] | select(.status.syncStatus != null or .status.syncedInstancesCount != null)'
   ```
   (Output should be empty)

4. **Verify zone queries work**:
   ```bash
   # Get instance pod IP
   INSTANCE_IP=$(kubectl get pod -n <namespace> <instance-pod> -o jsonpath='{.status.podIP}')
   
   # Query zone
   dig @${INSTANCE_IP} example.com SOA
   ```

## Getting Help

If issues persist:

1. **Collect diagnostic information**:
   ```bash
   # Save all DNSZone resources
   kubectl get dnszones -A -o yaml > dnszones-debug.yaml
   
   # Save operator logs
   kubectl logs -n dns-system -l app=bindy-operator --tail=500 > operator-logs.txt
   
   # Save events
   kubectl get events -A --sort-by='.lastTimestamp' | grep -i dnszone > dnszone-events.txt
   ```

2. **File a GitHub issue** with:
   - Migration step where failure occurred
   - Error messages from operator logs
   - DNSZone YAML showing problematic status
   - Bind9Instance YAML for affected instances

3. **Check known issues**:
   - [GitHub Issues](https://github.com/firestoned/bindy/issues)
   - [CHANGELOG.md](../../CHANGELOG.md)
   - [DNSZone Consolidation Roadmap](../../roadmaps/dnszone-consolidation-roadmap.md)

## See Also

- [DNSZone Operator Architecture](../concepts/dnszone-operator-architecture.md)
- [Integration Test Plan](../../roadmaps/integration-test-plan.md)
- [API Reference](../reference/api.md)
- [Common Issues](./common-issues.md)
