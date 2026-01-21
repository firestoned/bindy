# Debugging

Step-by-step guide to debugging Bindy DNS operator issues.

## Debug Workflow

### 1. Identify the Problem

Determine what's not working:
- Bind9Instance not creating pods?
- DNSZone not loading?
- DNS records not resolving?
- Zone transfers failing?

### 2. Check Resource Status

```bash
# Get high-level status
kubectl get bind9instances,dnszones,arecords -A

# Check specific resource
kubectl describe bind9instance primary-dns -n dns-system
kubectl describe dnszone example-com -n dns-system
```

### 3. Review Events

```bash
# Recent events
kubectl get events -n dns-system --sort-by='.lastTimestamp'

# Events for specific resource
kubectl describe dnszone example-com -n dns-system | grep -A10 Events
```

### 4. Examine Logs

```bash
# Operator logs
kubectl logs -n dns-system deployment/bindy --tail=100

# BIND9 instance logs
kubectl logs -n dns-system -l instance=primary-dns --tail=50

# Follow logs in real-time
kubectl logs -n dns-system deployment/bindy -f
```

## Debugging Bind9Instance

### Issue: Pods Not Starting

```bash
# 1. Check pod status
kubectl get pods -n dns-system -l instance=primary-dns

# 2. Describe pod
kubectl describe pod -n dns-system <pod-name>

# 3. Check events
kubectl get events -n dns-system --field-selector involvedObject.name=<pod-name>

# 4. Check logs if pod is running
kubectl logs -n dns-system <pod-name>

# 5. Check deployment
kubectl describe deployment primary-dns -n dns-system
```

### Issue: ConfigMap Not Created

```bash
# 1. List ConfigMaps
kubectl get configmaps -n dns-system

# 2. Check operator logs
kubectl logs -n dns-system deployment/bindy | grep -i configmap

# 3. Check RBAC permissions
kubectl auth can-i create configmaps \
  --as=system:serviceaccount:dns-system:bindy \
  -n dns-system

# 4. Manually trigger reconciliation
kubectl annotate bind9instance primary-dns reconcile=true -n dns-system --overwrite
```

## Debugging DNSZone

### Issue: No Instances Match Selector

```bash
# 1. Check zone selector
kubectl get dnszone example-com -n dns-system -o yaml | grep -A5 instanceSelector

# 2. List instances with labels
kubectl get bind9instances -n dns-system --show-labels

# 3. Test selector match
kubectl get bind9instances -n dns-system \
  -l dns-role=primary,environment=production

# 4. Fix labels or selector
kubectl label bind9instance primary-dns dns-role=primary -n dns-system
# Or edit zone selector
kubectl edit dnszone example-com -n dns-system
```

### Issue: Zone File Missing

```bash
# 1. Check if zone reconciliation succeeded
kubectl get dnszone example-com -n dns-system -o jsonpath='{.status.conditions}'

# 2. Exec into pod and check zones directory
kubectl exec -n dns-system deployment/primary-dns -- ls -la /var/lib/bind/zones/

# 3. Check BIND9 configuration
kubectl exec -n dns-system deployment/primary-dns -- cat /etc/bind/named.conf

# 4. Check BIND9 logs
kubectl logs -n dns-system -l instance=primary-dns | grep "example.com"

# 5. Reload BIND9 configuration
kubectl exec -n dns-system deployment/primary-dns -- rndc reload
```

## Debugging DNS Records

### Issue: Record Not in Zone File

```bash
# 1. Verify record exists
kubectl get arecord www-example -n dns-system

# 2. Check record status
kubectl get arecord www-example -n dns-system -o jsonpath='{.status}'

# 3. Verify zone reference
kubectl get arecord www-example -n dns-system -o jsonpath='{.spec.zone}'
# Should match a DNSZone resource name

# 4. Check zone file contents
kubectl exec -n dns-system deployment/primary-dns -- \
  cat /var/lib/bind/zones/example.com.zone

# 5. Trigger record reconciliation
kubectl annotate arecord www-example reconcile=true -n dns-system --overwrite
```

### Issue: DNS Query Not Resolving

```bash
# 1. Get DNS service IP
SERVICE_IP=$(kubectl get svc primary-dns -n dns-system -o jsonpath='{.spec.clusterIP}')

# 2. Test query from within cluster
kubectl run -it --rm debug --image=nicolaka/netshoot --restart=Never -- \
  dig @$SERVICE_IP www.example.com

# 3. Test query from BIND9 pod directly
kubectl exec -n dns-system deployment/primary-dns -- \
  dig @localhost www.example.com

# 4. Check if zone is loaded
kubectl exec -n dns-system deployment/primary-dns -- \
  rndc status | grep "zones loaded"

# 5. Query zone status
kubectl exec -n dns-system deployment/primary-dns -- \
  rndc zonestatus example.com
```

## Debugging Zone Transfers

### Issue: Secondary Not Receiving Transfers

```bash
# 1. Check primary allows transfers
kubectl get bind9instance primary-dns -n dns-system \
  -o jsonpath='{.spec.config.allowTransfer}'

# 2. Check secondary configuration
kubectl get dnszone example-com-secondary -n dns-system \
  -o jsonpath='{.spec.secondaryConfig}'

# 3. Test network connectivity
kubectl exec -n dns-system deployment/secondary-dns -- \
  nc -zv primary-dns-service 53

# 4. Attempt manual transfer
kubectl exec -n dns-system deployment/secondary-dns -- \
  dig @primary-dns-service example.com AXFR

# 5. Check transfer logs
kubectl logs -n dns-system -l dns-role=secondary | grep -i transfer

# 6. Check NOTIFY messages
kubectl logs -n dns-system -l dns-role=primary | grep -i notify
```

## Enable Debug Logging

### Operator Debug Logging

```bash
# Edit operator deployment
kubectl set env deployment/bindy RUST_LOG=debug -n dns-system

# Or patch deployment
kubectl patch deployment bindy -n dns-system \
  -p '{"spec":{"template":{"spec":{"containers":[{"name":"operator","env":[{"name":"RUST_LOG","value":"debug"}]}]}}}}'

# Restart operator
kubectl rollout restart deployment/bindy -n dns-system

# View debug logs
kubectl logs -n dns-system deployment/bindy -f
```

### Enable JSON Logging

For easier parsing and integration with log aggregation tools:

```bash
# Set JSON format
kubectl set env deployment/bindy RUST_LOG_FORMAT=json -n dns-system

# Or patch deployment for both debug level and JSON format
kubectl patch deployment bindy -n dns-system \
  -p '{"spec":{"template":{"spec":{"containers":[{"name":"operator","env":[{"name":"RUST_LOG","value":"debug"},{"name":"RUST_LOG_FORMAT","value":"json"}]}]}}}}'

# Restart operator
kubectl rollout restart deployment/bindy -n dns-system

# View JSON logs (can be piped to jq for parsing)
kubectl logs -n dns-system deployment/bindy -f | jq .
```

### BIND9 Debug Logging

```bash
# Enable query logging
kubectl exec -n dns-system deployment/primary-dns -- \
  rndc querylog on

# View queries
kubectl logs -n dns-system -l instance=primary-dns -f | grep "query:"

# Disable query logging
kubectl exec -n dns-system deployment/primary-dns -- \
  rndc querylog off
```

## Network Debugging

### Test DNS Resolution

```bash
# From debug pod
kubectl run -it --rm debug --image=nicolaka/netshoot --restart=Never -- /bin/bash

# Inside pod:
dig @primary-dns-service.dns-system.svc.cluster.local www.example.com
nslookup www.example.com primary-dns-service.dns-system.svc.cluster.local
host www.example.com primary-dns-service.dns-system.svc.cluster.local
```

### Check Network Policies

```bash
# List network policies
kubectl get networkpolicies -n dns-system

# Describe policy
kubectl describe networkpolicy <policy-name> -n dns-system

# Temporarily remove policy for testing
kubectl delete networkpolicy <policy-name> -n dns-system
```

## Performance Debugging

### Check Resource Usage

```bash
# Pod resource usage
kubectl top pods -n dns-system

# Node pressure
kubectl describe nodes | grep -A5 "Conditions:\|Allocated resources:"

# Detailed pod metrics
kubectl describe pod <pod-name> -n dns-system | grep -A10 "Limits:\|Requests:"
```

### Profile DNS Queries

```bash
# Measure query latency
for i in {1..100}; do
  dig @$SERVICE_IP www.example.com +stats | grep "Query time:"
done | awk '{sum+=$4; count++} END {print "Average:", sum/count, "ms"}'

# Test concurrent queries
seq 1 100 | xargs -I{} -P10 dig @$SERVICE_IP www.example.com +short
```

## Collect Diagnostic Information

### Create Support Bundle

```bash
#!/bin/bash
# collect-diagnostics.sh

NAMESPACE="dns-system"
OUTPUT_DIR="bindy-diagnostics-$(date +%Y%m%d-%H%M%S)"

mkdir -p $OUTPUT_DIR

# Collect resources
kubectl get all -n $NAMESPACE -o yaml > $OUTPUT_DIR/resources.yaml
kubectl get bind9instances,dnszones,arecords,aaaarecords,cnamerecords -A -o yaml > $OUTPUT_DIR/crds.yaml

# Collect logs
kubectl logs -n $NAMESPACE deployment/bindy --tail=1000 > $OUTPUT_DIR/operator.log
kubectl logs -n $NAMESPACE -l app=bind9 --tail=1000 > $OUTPUT_DIR/bind9.log

# Collect events
kubectl get events -n $NAMESPACE --sort-by='.lastTimestamp' > $OUTPUT_DIR/events.txt

# Collect status
kubectl describe bind9instances -A > $OUTPUT_DIR/bind9instances-describe.txt
kubectl describe dnszones -A > $OUTPUT_DIR/dnszones-describe.txt

# Create archive
tar -czf $OUTPUT_DIR.tar.gz $OUTPUT_DIR/

echo "Diagnostics collected in $OUTPUT_DIR.tar.gz"
```

## Next Steps

- [Common Issues](./common-issues.md) - Known problems and solutions
- [FAQ](./faq.md) - Frequently asked questions
- [Logging](./logging.md) - Log configuration and analysis
