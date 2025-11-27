# Resource Limits

Configure CPU and memory limits for BIND9 pods.

## Setting Resource Limits

Configure resources in the Bind9Instance spec:

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
spec:
  replicas: 2
  resources:
    requests:
      cpu: "100m"
      memory: "128Mi"
    limits:
      cpu: "500m"
      memory: "512Mi"
```

## Recommended Values

### Small Deployment (Few zones)

```yaml
resources:
  requests:
    cpu: "100m"
    memory: "128Mi"
  limits:
    cpu: "500m"
    memory: "512Mi"
```

### Medium Deployment (Multiple zones)

```yaml
resources:
  requests:
    cpu: "200m"
    memory: "256Mi"
  limits:
    cpu: "1000m"
    memory: "1Gi"
```

### Large Deployment (Many zones, high traffic)

```yaml
resources:
  requests:
    cpu: "500m"
    memory: "512Mi"
  limits:
    cpu: "2000m"
    memory: "2Gi"
```

## Best Practices

1. **Set both requests and limits** - Ensures predictable performance
2. **Start conservative** - Begin with lower values and adjust based on monitoring
3. **Monitor usage** - Use metrics to right-size resources
4. **Leave headroom** - Don't max out limits
5. **Consider query volume** - High-traffic DNS needs more resources

## Monitoring Resource Usage

```bash
# View pod resource usage
kubectl top pods -n dns-system -l app=bind9

# Describe pod to see limits
kubectl describe pod -n dns-system <pod-name>
```
