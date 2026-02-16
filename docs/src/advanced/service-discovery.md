# Service Discovery

Use Bindy for Kubernetes service discovery and internal DNS.

## Kubernetes Service DNS

### Automatic Service Records

Create DNS records for Kubernetes services:

```yaml
apiVersion: v1
kind: Service
metadata:
  name: myapp
  namespace: production
spec:
  selector:
    app: myapp
  ports:
  - port: 80
---
# Create corresponding DNS record
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: myapp
spec:
  zone: internal-local
  name: myapp.production
  ipv4Addresses:
    - "10.100.5.10"  # Service ClusterIP
```

### Service Discovery Pattern

```mermaid
graph TB
    app["Application Query:<br/>myapp.production.internal.local"]
    dns["Bindy DNS Server"]
    result["Returns: 10.100.5.10"]
    svc["Kubernetes Service"]

    app --> dns
    dns --> result
    result --> svc

    style app fill:#fff9c4,stroke:#f57f17,stroke-width:2px
    style dns fill:#e8f5e9,stroke:#1b5e20,stroke-width:2px
    style result fill:#e1f5ff,stroke:#01579b,stroke-width:2px
    style svc fill:#f3e5f5,stroke:#4a148c,stroke-width:2px
```

## Dynamic Updates

Automatically update DNS when services change (future enhancement):

```yaml
# Operator watches Services and creates DNS records
```

## Best Practices

1. **Consistent naming** - Match service names to DNS names
2. **Namespace separation** - Use subdomains per namespace
3. **TTL management** - Short TTLs for dynamic services
4. **Health checks** - Only advertise healthy services

## Next Steps

- [Integration](./integration.md) - Integration patterns
- [External DNS](./external-dns.md) - External DNS integration
