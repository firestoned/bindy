# Integration

Integrate Bindy with other Kubernetes and DNS systems.

## Integration Patterns

### 1. Internal Service Discovery

Use Bindy for internal service DNS.

### 2. Hybrid DNS

Combine Bindy with external DNS providers.

### 3. GitOps

Manage DNS configuration through Git.

## Kubernetes Integration

### CoreDNS Integration

Use Bindy alongside CoreDNS:

```yaml
# CoreDNS for cluster.local
# Bindy for custom domains
```

### Service Mesh

Integrate with Istio, Linkerd:
- Custom DNS resolution
- Service discovery
- Traffic routing

## Next Steps

- [External DNS](./external-dns.md) - External provider integration
- [Service Discovery](./service-discovery.md) - Kubernetes service discovery
