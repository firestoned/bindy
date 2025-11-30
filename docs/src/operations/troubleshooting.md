# Troubleshooting

Diagnose and resolve common issues with Bindy DNS operator.

## Quick Diagnosis

### Check Overall Health

```bash
# Check all resources
kubectl get all -n dns-system

# Check CRDs
kubectl get bind9instances,dnszones,arecords -A

# Check events
kubectl get events -n dns-system --sort-by='.lastTimestamp' | tail -20
```

### View Status Conditions

```bash
# Bind9Instance status
kubectl get bind9instance primary-dns -n dns-system -o yaml | yq '.status'

# DNSZone status
kubectl get dnszone example-com -n dns-system -o yaml | yq '.status'
```

## Common Issues

See [Common Issues](./common-issues.md) for frequently encountered problems and solutions.

### DNS Record Zone Reference Issues

If you're seeing "DNSZone not found" errors:
- Records can use `zone` (matches `DNSZone.spec.zoneName`) or `zoneRef` (matches `DNSZone.metadata.name`)
- Common mistake: Using `zone: internal-local` when the zone name is `internal.local`
- See [DNS Record Issues - DNSZone Not Found](./common-issues.md#dnszone-not-found) for detailed troubleshooting

## Debugging Steps

See [Debugging Guide](./debugging.md) for detailed debugging procedures.

## FAQ

See [FAQ](./faq.md) for answers to frequently asked questions.

## Getting Help

### Check Logs

```bash
# Controller logs
kubectl logs -n dns-system deployment/bindy --tail=100

# BIND9 instance logs
kubectl logs -n dns-system -l instance=primary-dns
```

### Describe Resources

```bash
# Describe Bind9Instance
kubectl describe bind9instance primary-dns -n dns-system

# Describe pods
kubectl describe pod -n dns-system <pod-name>
```

### Check Resource Status

```bash
# Get detailed status
kubectl get bind9instance primary-dns -n dns-system -o jsonpath='{.status}' | jq
```

## Escalation

If issues persist:

1. Check [Common Issues](./common-issues.md)
2. Review [Debugging Guide](./debugging.md)
3. Check [FAQ](./faq.md)
4. Search GitHub issues: https://github.com/firestoned/bindy/issues
5. Create a new issue with:
   - Kubernetes version
   - Bindy version
   - Resource YAMLs
   - Controller logs
   - Error messages

## Next Steps

- [Common Issues](./common-issues.md) - Frequently encountered problems
- [Debugging](./debugging.md) - Step-by-step debugging
- [FAQ](./faq.md) - Frequently asked questions
