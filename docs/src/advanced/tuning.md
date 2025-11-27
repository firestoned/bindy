# Tuning

Fine-tune BIND9 and Kubernetes parameters for optimal performance.

## BIND9 Tuning

### Query Performance

```yaml
# Future enhancement - BIND9 tuning via Bind9Instance spec
spec:
  config:
    tuning:
      maxCacheSize: "512M"
      maxCacheTTL: 86400
      recursiveClients: 1000
```

### Zone Transfer Tuning

- Concurrent transfers: `transfers-in`, `transfers-out`
- Transfer timeout: Adjust for large zones
- Compression: Enable for faster transfers

## Kubernetes Tuning

### Pod Resources

Right-size based on load:

```yaml
# Light load
resources:
  requests: {cpu: "100m", memory: "128Mi"}
  limits: {cpu: "500m", memory: "512Mi"}

# Medium load
resources:
  requests: {cpu: "500m", memory: "512Mi"}
  limits: {cpu: "2000m", memory: "2Gi"}

# Heavy load
resources:
  requests: {cpu: "2000m", memory: "2Gi"}
  limits: {cpu: "4000m", memory: "4Gi"}
```

### HPA (Horizontal Pod Autoscaling)

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: bind9-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: primary-dns
  minReplicas: 2
  maxReplicas: 10
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
```

### Node Affinity

Place DNS pods on optimized nodes:

```yaml
affinity:
  nodeAffinity:
    requiredDuringSchedulingIgnoredDuringExecution:
      nodeSelectorTerms:
      - matchExpressions:
        - key: workload-type
          operator: In
          values:
          - dns
```

## Network Tuning

### Service Type

Consider NodePort or LoadBalancer for external access:

```yaml
apiVersion: v1
kind: Service
spec:
  type: LoadBalancer  # Or NodePort
  externalTrafficPolicy: Local  # Preserve source IP
```

### DNS Caching

Adjust TTL values:

```yaml
# Short TTL for dynamic records
spec:
  ttl: 60  # 1 minute

# Long TTL for static records
spec:
  ttl: 86400  # 24 hours
```

## OS-Level Tuning

### File Descriptors

Increase limits for high query volume:

```yaml
# In pod security context (future enhancement)
securityContext:
  limits:
    nofile: 65536
```

### Network Buffers

Optimize for DNS traffic (node-level):

```bash
# Increase UDP buffer sizes
sysctl -w net.core.rmem_max=8388608
sysctl -w net.core.wmem_max=8388608
```

## Monitoring Tuning Impact

```bash
# Before tuning - baseline
kubectl top pods -n dns-system
time dig @$SERVICE_IP example.com

# Apply tuning
kubectl apply -f tuned-config.yaml

# After tuning - compare
kubectl top pods -n dns-system
time dig @$SERVICE_IP example.com
```

## Tuning Checklist

- [ ] Right-sized pod resources
- [ ] Optimal replica count
- [ ] HPA configured
- [ ] Appropriate TTL values
- [ ] Network policies optimized
- [ ] Node placement configured
- [ ] Monitoring enabled
- [ ] Performance tested

## Next Steps

- [Performance](./performance.md) - Performance overview
- [Benchmarking](./benchmarking.md) - Testing methodology
