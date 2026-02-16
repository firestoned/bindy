# Common Issues

Solutions to frequently encountered problems.

## Bind9Instance Issues

### Pods Not Starting

**Symptom:** Bind9Instance created but pods not running

**Diagnosis:**
```bash
kubectl get pods -n dns-system -l instance=primary-dns
kubectl describe pod -n dns-system <pod-name>
```

**Common Causes:**
1. **Image pull errors** - Check image name and registry access
2. **Resource limits** - Insufficient CPU/memory on nodes
3. **RBAC issues** - ServiceAccount lacks permissions

**Solution:**
```bash
# Check events
kubectl get events -n dns-system

# Fix resource limits
kubectl edit bind9instance primary-dns -n dns-system
# Increase resources.requests and resources.limits

# Verify RBAC
kubectl auth can-i create deployments \
  --as=system:serviceaccount:dns-system:bindy
```

### ConfigMap Not Created

**Symptom:** ConfigMap missing for Bind9Instance

**Diagnosis:**
```bash
kubectl get configmap -n dns-system
kubectl logs -n dns-system deployment/bindy | grep ConfigMap
```

**Solution:**
```bash
# Check operator logs for errors
kubectl logs -n dns-system deployment/bindy --tail=50

# Delete and recreate instance
kubectl delete bind9instance primary-dns -n dns-system
kubectl apply -f instance.yaml
```

## DNSZone Issues

### Duplicate Zone Name Error

**Symptom:** DNSZone status shows `Ready=False` with reason `DuplicateZone`

**What This Means:**
Multiple DNSZone custom resources are trying to manage the same DNS zone name (e.g., `example.com`). Bindy prevents this to avoid conflicting DNS configurations. Only one DNSZone can claim a given zone name at a time.

**Diagnosis:**
```bash
# Check the DNSZone status
kubectl get dnszone <zone-name> -n dns-system -o yaml

# Look for the condition
kubectl get dnszone <zone-name> -n dns-system -o jsonpath='{.status.conditions[?(@.type=="Ready")]}'

# Expected output showing the conflict:
# {
#   "type": "Ready",
#   "status": "False",
#   "reason": "DuplicateZone",
#   "message": "Zone 'example.com' is already claimed by other DNSZone(s): production/example-com, staging/example-com-test"
# }
```

**Common Scenarios:**

1. **Accidental Duplication**: Same zone created in multiple namespaces
   ```bash
   # Find all DNSZones claiming the same zone name
   kubectl get dnszones --all-namespaces -o json | \
     jq -r '.items[] | select(.spec.zoneName=="example.com") | "\(.metadata.namespace)/\(.metadata.name)"'
   ```

2. **Migration Leftovers**: Old zone not deleted after migration
   ```bash
   # Check for zones in all namespaces
   kubectl get dnszones --all-namespaces | grep example
   ```

3. **Multi-Tenant Conflicts**: Different teams trying to use the same domain
   ```bash
   # Review zone ownership
   kubectl get dnszones --all-namespaces -o custom-columns=\
     NAMESPACE:.metadata.namespace,\
     NAME:.metadata.name,\
     ZONE:.spec.zoneName,\
     OWNER:.metadata.labels.team
   ```

**Resolution:**

1. **Identify the intended owner**: Determine which DNSZone should own the zone name
   ```bash
   # List all conflicting zones with details
   kubectl get dnszones --all-namespaces -o json | \
     jq -r '.items[] | select(.spec.zoneName=="example.com") |
       "Namespace: \(.metadata.namespace), Name: \(.metadata.name), Created: \(.metadata.creationTimestamp)"'
   ```

2. **Delete duplicate zones**: Remove the DNSZone resources that should NOT manage this zone
   ```bash
   # Delete the unwanted zone
   kubectl delete dnszone <duplicate-zone-name> -n <namespace>
   ```

3. **Verify resolution**: Check that the intended zone is now reconciling
   ```bash
   # Watch the zone status update
   kubectl get dnszone <intended-zone-name> -n dns-system -w

   # Expected: Status should transition from DuplicateZone to Ready
   # Ready     False   DuplicateZone   ...  (before)
   # Ready     True    ReconcileSucceeded  ...  (after)
   ```

**Prevention:**

1. **Use unique names**: Name your DNSZone resources uniquely, even if managing the same domain
   ```yaml
   # Good: Unique names even for same zone
   apiVersion: bindy.firestoned.io/v1beta1
   kind: DNSZone
   metadata:
     name: example-com-production  # ← Unique name
     namespace: dns-system
   spec:
     zoneName: example.com  # ← Shared zone name (but only one should exist)
   ```

2. **Document zone ownership**: Use labels to track which team/environment owns a zone
   ```yaml
   metadata:
     name: example-com
     namespace: production
     labels:
       team: platform-engineering
       environment: production
   ```

3. **Implement admission control**: Use a ValidatingWebhook to prevent duplicate zone creation (advanced)

**Technical Details:**

The duplicate zone check:
- Runs at the start of every reconciliation (before any configuration changes)
- Searches across **all namespaces** for other DNSZones with the same `spec.zoneName`
- Excludes the current zone itself (compares by namespace/name)
- Sets `Ready=False` with reason `DuplicateZone` to prevent configuration
- Lists all conflicting zones in the status message for troubleshooting

This validation ensures DNS consistency and prevents:
- Split-brain scenarios with different zone configurations
- Conflicting DNS records from multiple sources
- Accidental zone overwrites during reconciliation

### No Instances Match Selector

**Symptom:** DNSZone status shows "No Bind9Instances matched selector"

**Diagnosis:**
```bash
kubectl get bind9instances -n dns-system --show-labels
kubectl get dnszone example-com -n dns-system -o yaml | yq '.spec.instanceSelector'
```

**Solution:**
```bash
# Verify labels on instances
kubectl label bind9instance primary-dns dns-role=primary -n dns-system

# Or update zone selector
kubectl edit dnszone example-com -n dns-system
```

### Zone File Not Created

**Symptom:** Zone exists but no zone file in BIND9

**Diagnosis:**
```bash
kubectl exec -n dns-system deployment/primary-dns -- ls -la /var/lib/bind/zones/
kubectl logs -n dns-system deployment/bindy | grep "example-com"
```

**Solution:**
```bash
# Check if zone reconciliation succeeded
kubectl describe dnszone example-com -n dns-system

# Trigger reconciliation by updating zone
kubectl annotate dnszone example-com reconcile=true -n dns-system
```

## DNS Record Issues

### Record Not Matching DNSZone (Event-Driven Architecture)

**Symptom:** Record created but `status.zoneRef` is not set, or record status shows "NotSelected"

**Diagnosis:**
```bash
# Check if record has been selected by a zone
kubectl get arecord www-example -n dns-system -o jsonpath='{.status.zoneRef}'

# Check record status conditions
kubectl get arecord www-example -n dns-system -o jsonpath='{.status.conditions[?(@.type=="Ready")]}'

# Check the record's labels
kubectl get arecord www-example -n dns-system -o jsonpath='{.metadata.labels}'

# Check available DNSZones and their selectors
kubectl get dnszones -n dns-system

# Check the DNSZone's label selector
kubectl get dnszone example-com -n dns-system -o jsonpath='{.spec.recordsFrom[*].selector}'
```

**Understanding the Problem:**

With the **event-driven architecture**, DNS records are matched to DNSZones via watch events:

1. **DNSZone watches all 8 record types** (ARecord, AAAARecord, TXTRecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, CAARecord)
2. When a record is created/updated, **DNSZone receives a watch event immediately** (⚡ sub-second)
3. DNSZone evaluates if record labels match `spec.recordsFrom` selectors
4. If matched, DNSZone **sets `record.status.zoneRef`** with full zone metadata
5. Record operator **watches for status changes** and reconciles when `status.zoneRef` is set

**Common Mistakes:**
- Record has label `zone: internal-local` but DNSZone expects `zone: internal.local`
- Record missing the required label entirely
- DNSZone `spec.recordsFrom` selector doesn't match any records
- Typo in label key or value
- Record and DNSZone in different namespaces (watches are namespace-scoped)

**Expected Behavior (Event-Driven):**
```
Record created at 10:00:00.000
  → DNSZone watch triggered at 10:00:00.050 ⚡ (immediate)
  → Label selectors evaluated
  → status.zoneRef set at 10:00:00.100 (if matched)
  → Record watch triggered at 10:00:00.150 ⚡ (immediate)
  → Record reconciles to BIND9 at 10:00:00.500
Total time: ~500ms ✅
```

**Troubleshooting:**

1. **Verify record labels match zone selector:**
   ```bash
   # Get record labels
   kubectl get arecord www-example -o jsonpath='{.metadata.labels}'

   # Get zone selector
   kubectl get dnszone example-com -o jsonpath='{.spec.recordsFrom[0].selector}'
   ```

2. **Check if record is in the same namespace as the zone:**
   ```bash
   kubectl get arecord www-example -n dns-system
   kubectl get dnszone example-com -n dns-system
   ```

3. **Verify DNSZone operator is running:**
   ```bash
   kubectl logs -n dns-system deployment/bindy | grep "DNSZone watch"
   ```

4. **Check record status.zoneRef field:**
   ```bash
   # If empty/null, record hasn't been selected
   kubectl get arecord www-example -o yaml | grep -A5 "zoneRef:"
   ```

**Solution:**

Ensure record labels match the DNSZone's selector

**Example:**

Given this DNSZone:
```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  recordSelector:
    matchLabels:
      zone: example.com  # ← Selector expects this label
```

**Wrong:**
```yaml
# Record without matching label
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  # ✗ Missing labels!
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.1"
```

**Correct:**
```yaml
# Record with matching label
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    zone: example.com  # ✓ Matches DNSZone selector
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.1"
```

**Verification:**
```bash
# After fixing, check the record reconciles
kubectl describe arecord www-example -n dns-system

# Check which DNSZone the record matched
kubectl get arecord www-example -n dns-system -o yaml | yq '.status.zone'

# Should see no errors in events
kubectl get events -n dns-system --sort-by='.lastTimestamp' | tail -10
```

See the [Label Selectors Guide](../guide/label-selectors.md) for more details.

### Record Not Appearing in Zone

**Symptom:** ARecord created but not in zone file

**Diagnosis:**
```bash
# Check record status
kubectl get arecord www-example -n dns-system -o yaml

# Check zone file
kubectl exec -n dns-system deployment/primary-dns -- cat /var/lib/bind/zones/example.com.zone
```

**Solution:**
```bash
# Verify record has the correct labels
kubectl get arecord www-example -n dns-system -o yaml | yq '.metadata.labels'

# Check DNSZone selector
kubectl get dnszone example-com -n dns-system -o yaml | yq '.spec.recordSelector'

# Update labels to match selector
kubectl label arecord www-example zone=example.com -n dns-system --overwrite
```

### DNS Query Not Resolving

**Symptom:** dig/nslookup fails to resolve

**Diagnosis:**
```bash
# Get DNS service IP
SERVICE_IP=$(kubectl get svc primary-dns -n dns-system -o jsonpath='{.spec.clusterIP}')

# Test query
dig @$SERVICE_IP www.example.com

# Check BIND9 logs
kubectl logs -n dns-system -l instance=primary-dns | tail -20
```

**Solutions:**

1. **Record doesn't exist:**
```bash
kubectl get arecords -n dns-system
kubectl apply -f record.yaml
```

2. **Zone not loaded:**
```bash
kubectl logs -n dns-system -l instance=primary-dns | grep "loaded serial"
```

3. **Network policy blocking:**
```bash
kubectl get networkpolicies -n dns-system
```

## Zone Transfer Issues

### Secondary Not Receiving Transfers

**Symptom:** Secondary instance not getting zone updates

**Diagnosis:**
```bash
# Check secondary logs
kubectl logs -n dns-system -l dns-role=secondary | grep transfer

# Check if zone has secondary IPs configured
kubectl get dnszone example-com -n dns-system -o jsonpath='{.status.secondaryIps}'

# Check if secondaries are discovered
kubectl get bind9instance -n dns-system -l role=secondary -o jsonpath='{.items[*].status.podIP}'
```

**Automatic Configuration:**

As of v0.1.0, Bindy **automatically discovers secondary IPs** and configures zone transfers:
- Secondary pods are discovered via Kubernetes API using label selectors (`role=secondary`)
- Primary zones are configured with `also-notify` and `allow-transfer` directives
- Secondary IPs are stored in `DNSZone.status.secondaryIps` for tracking
- When secondary pods restart/reschedule and get new IPs, zones are automatically updated

**Manual Verification:**
```bash
# Check if zone has secondary IPs in status
kubectl get dnszone example-com -n dns-system -o yaml | yq '.status.secondaryIps'

# Expected output: List of secondary pod IPs
# - 10.244.1.5
# - 10.244.2.8

# Verify zone configuration on primary
kubectl exec -n dns-system deployment/primary-dns -- \
  curl -s localhost:8080/api/zones/example.com | jq '.alsoNotify, .allowTransfer'
```

**If Automatic Configuration Fails:**

1. **Verify secondary instances are labeled correctly:**
   ```bash
   kubectl get bind9instance -n dns-system -o yaml | yq '.items[].metadata.labels'

   # Expected labels for secondaries:
   # role: secondary
   # cluster: <cluster-name>
   ```

2. **Check DNSZone reconciler logs:**
   ```bash
   kubectl logs -n dns-system deployment/bindy | grep "secondary"
   ```

3. **Verify network connectivity:**
   ```bash
   # Test AXFR from secondary to primary
   kubectl exec -n dns-system deployment/secondary-dns -- \
     dig @primary-dns-service AXFR example.com
   ```

**Recovery After Secondary Pod Restart:**

When secondary pods are rescheduled and get new IPs:
1. **Detection:** Reconciler automatically detects IP change within 5-10 minutes (next reconciliation)
2. **Update:** Zones are deleted and recreated with new secondary IPs
3. **Transfer:** Zone transfers resume automatically with new IPs

**Manual Trigger (if needed):**
```bash
# Force reconciliation by updating zone annotation
kubectl annotate dnszone example-com -n dns-system \
  reconcile.bindy.firestoned.io/trigger="$(date +%s)" --overwrite
```

## Performance Issues

### High Query Latency

**Symptom:** DNS queries taking too long

**Diagnosis:**
```bash
# Test query time
time dig @$SERVICE_IP example.com

# Check resource usage
kubectl top pods -n dns-system -l instance=primary-dns
```

**Solutions:**

1. **Increase resources:**
```yaml
spec:
  resources:
    limits:
      cpu: "1000m"
      memory: "1Gi"
```

2. **Add more replicas:**
```yaml
spec:
  replicas: 3
```

3. **Enable caching** (if appropriate for your use case)

## RBAC Issues

### Forbidden Errors in Logs

**Symptom:** Operator logs show "Forbidden" errors

**Diagnosis:**
```bash
kubectl logs -n dns-system deployment/bindy | grep Forbidden

# Check permissions
kubectl auth can-i create deployments \
  --as=system:serviceaccount:dns-system:bindy \
  -n dns-system
```

**Solution:**
```bash
# Reapply RBAC
kubectl apply -f deploy/rbac/

# Verify ClusterRoleBinding
kubectl get clusterrolebinding bindy-rolebinding -o yaml

# Restart operator
kubectl rollout restart deployment/bindy -n dns-system
```

## Next Steps

- [Debugging Guide](./debugging.md) - Detailed debugging procedures
- [FAQ](./faq.md) - Frequently asked questions
- [Logging](./logging.md) - Log analysis
