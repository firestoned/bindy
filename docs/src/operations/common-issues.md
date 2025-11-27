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
# Verify zone reference is correct
kubectl get arecord www-example -n dns-system -o jsonpath='{.spec.zone}'

# Should match DNSZone resource name
kubectl get dnszones -n dns-system

# Update if incorrect
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
