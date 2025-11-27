# Security

Secure your Bindy DNS infrastructure against threats and unauthorized access.

## Security Layers

### 1. Network Security
- Firewall rules limiting DNS access
- Network policies in Kubernetes
- Private networks for zone transfers

### 2. Access Control
- Query restrictions (allowQuery)
- Transfer restrictions (allowTransfer)  
- RBAC for Kubernetes resources

### 3. DNSSEC
- Cryptographic validation
- Zone signing
- Trust chain verification

### 4. Pod Security
- Pod Security Standards
- SecurityContext settings
- Read-only filesystems

## Best Practices

1. **Principle of Least Privilege** - Minimal permissions
2. **Defense in Depth** - Multiple security layers
3. **Regular Updates** - Keep BIND9 and controller updated
4. **Audit Logging** - Track all changes
5. **Encryption** - TLS for management, DNSSEC for queries

## Quick Security Checklist

- [ ] Enable DNSSEC for public zones
- [ ] Restrict allowQuery to expected networks
- [ ] Limit allowTransfer to secondary servers only
- [ ] Use RBAC for Kubernetes access
- [ ] Enable Pod Security Standards
- [ ] Regular security audits
- [ ] Monitor for suspicious queries
- [ ] Keep software updated

## Next Steps

- [DNSSEC](./dnssec.md) - Enable cryptographic validation
- [Access Control](./access-control.md) - Configure query and transfer restrictions
