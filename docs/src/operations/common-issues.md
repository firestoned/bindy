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
# Check controller logs for errors
kubectl logs -n dns-system deployment/bindy --tail=50

# Delete and recreate instance
kubectl delete bind9instance primary-dns -n dns-system
kubectl apply -f instance.yaml
```

## DNSZone Issues

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

### DNSZone Not Found

**Symptom:** Controller logs show "DNSZone not found" errors for a zone that exists

**Example Error:**
```
ERROR Failed to find DNSZone for zone 'internal-local' in namespace 'dns-system'
```

**Root Cause:** Mismatch between how the record references the zone and the actual DNSZone fields.

**Diagnosis:**
```bash
# Check what the record is trying to reference
kubectl get arecord www-example -n dns-system -o yaml | grep -A2 spec:

# Check available DNSZones
kubectl get dnszones -n dns-system

# Check the DNSZone details
kubectl get dnszone example-com -n dns-system -o yaml
```

**Understanding the Problem:**

DNS records can reference zones using **two different fields**:

1. **`zone` field** - Matches against `DNSZone.spec.zoneName` (the actual DNS zone name like `example.com`)
2. **`zoneRef` field** - Matches against `DNSZone.metadata.name` (the Kubernetes resource name like `example-com`)

Common mistakes:
- Using `zone: internal-local` when `spec.zoneName: internal.local` (dots vs dashes)
- Using `zone: example-com` when it should be `zone: example.com`
- Using `zoneRef: example.com` when it should be `zoneRef: example-com`

**Solution:**

**Option 1: Use `zone` field with the actual DNS zone name**
```yaml
spec:
  zone: example.com  # Must match DNSZone spec.zoneName
  name: www
```

**Option 2: Use `zoneRef` field with the resource name (recommended)**
```yaml
spec:
  zoneRef: example-com  # Must match DNSZone metadata.name
  name: www
```

**Example Fix:**

Given this DNSZone:
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: internal-local      # ← Resource name
  namespace: dns-system
spec:
  zoneName: internal.local  # ← Actual zone name
```

**Wrong:**
```yaml
spec:
  zone: internal-local  # ✗ This looks for spec.zoneName = "internal-local"
```

**Correct:**
```yaml
# Method 1: Use actual zone name
spec:
  zone: internal.local  # ✓ Matches spec.zoneName

# Method 2: Use resource name (more efficient)
spec:
  zoneRef: internal-local  # ✓ Matches metadata.name
```

**Verification:**
```bash
# After fixing, check the record reconciles
kubectl describe arecord www-example -n dns-system

# Should see no errors in events
kubectl get events -n dns-system --sort-by='.lastTimestamp' | tail -10
```

See [Records Guide - Referencing DNS Zones](../guide/records-guide.md#referencing-dns-zones) for more details.

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
# Verify zone reference is correct (use zone or zoneRef)
kubectl get arecord www-example -n dns-system -o yaml | grep -E 'zone:|zoneRef:'

# Check available DNSZones
kubectl get dnszones -n dns-system

# Update if incorrect - use zone (matches spec.zoneName) or zoneRef (matches metadata.name)
kubectl edit arecord www-example -n dns-system
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

# Check primary allows transfers
kubectl get bind9instance primary-dns -n dns-system -o jsonpath='{.spec.config.allowTransfer}'
```

**Solution:**
```bash
# Update primary to allow transfers
kubectl edit bind9instance primary-dns -n dns-system

# Add secondary network to allowTransfer:
spec:
  config:
    allowTransfer:
      - "10.0.0.0/8"

# Verify network connectivity
kubectl exec -n dns-system deployment/secondary-dns -- dig @primary-dns-service AXFR example.com
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

**Symptom:** Controller logs show "Forbidden" errors

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

# Restart controller
kubectl rollout restart deployment/bindy -n dns-system
```

## Next Steps

- [Debugging Guide](./debugging.md) - Detailed debugging procedures
- [FAQ](./faq.md) - Frequently asked questions
- [Logging](./logging.md) - Log analysis
