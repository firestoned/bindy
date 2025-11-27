# External DNS Integration

Integrate Bindy with external DNS management systems.

## Use Cases

1. **Hybrid Cloud** - Internal DNS in Bindy, external in cloud provider
2. **Public/Private Split** - Public zones external, private in Bindy
3. **Migration** - Gradual migration from external to Bindy

## Integration with external-dns

External-dns manages external providers (Route53, CloudDNS), Bindy manages internal BIND9.

### Separate Domains

```yaml
# external-dns manages example.com (public)
# Bindy manages internal.example.com (private)
```

### Forwarding

Configure external DNS to forward to Bindy for internal zones.

## Best Practices

1. **Clear boundaries** - Document which system owns which zones
2. **Consistent records** - Synchronize where needed
3. **Separate responsibilities** - External for public, Bindy for internal

## Next Steps

- [Integration](./integration.md) - Integration overview
- [Service Discovery](./service-discovery.md) - Kubernetes service discovery
