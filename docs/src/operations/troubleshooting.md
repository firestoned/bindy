# Troubleshooting

Diagnose and resolve common issues with Bindy DNS operator.

## Quick Diagnosis

### Check Overall Health

```bash
# Check all resources
kubectl get all -n bindy-system

# Check CRDs
kubectl get bind9instances,dnszones,arecords -A

# Check events
kubectl get events -n bindy-system --sort-by='.lastTimestamp' | tail -20
```

### View Status Conditions

```bash
# Bind9Instance status
kubectl get bind9instance primary-dns -n bindy-system -o yaml | yq '.status'

# DNSZone status
kubectl get dnszone example-com -n bindy-system -o yaml | yq '.status'
```

## Common Issues

See [Common Issues](./common-issues.md) for frequently encountered problems and solutions.

### DNS Record Label Matching Issues

If you're seeing "No matching DNSZone found" errors:
- Records use labels to match DNSZones via label selectors
- Common mistake: Record missing required labels or labels not matching DNSZone selector
- See [DNS Record Issues - Record Not Matching DNSZone](./common-issues.md#record-not-matching-dnszone-event-driven-architecture) for detailed troubleshooting

### Zone Requeues Forever as "unreconciled" With an Empty INSTANCES Column

If the operator logs `Found N unreconciled instance(s) for zone ...` on every
cycle while the zone serves queries correctly, and `kubectl get dnszones`
shows an empty `INSTANCES` column (with `Bind9Instance` resources showing
`ZONES 0`), the zone's data predates its CR: every endpoint answers "zone
already exists" and versions before v0.8.0-rc.3 never recorded the instance
in `status.bind9Instances`. Fixed in v0.8.0-rc.3; on earlier versions the
loop is harmless to DNS service but re-pushes the zone configuration about
once a minute. Recreating the `DNSZone` CR does not help; upgrading does.

## Debugging Steps

See [Debugging Guide](./debugging.md) for detailed debugging procedures.

## FAQ

See [FAQ](./faq.md) for answers to frequently asked questions.

## Getting Help

### Check Logs

```bash
# Operator logs
kubectl logs -n bindy-system deployment/bindy --tail=100

# BIND9 instance logs
kubectl logs -n bindy-system -l instance=primary-dns
```

### Describe Resources

```bash
# Describe Bind9Instance
kubectl describe bind9instance primary-dns -n bindy-system

# Describe pods
kubectl describe pod -n bindy-system <pod-name>
```

### Check Resource Status

```bash
# Get detailed status
kubectl get bind9instance primary-dns -n bindy-system -o jsonpath='{.status}' | jq
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
   - Operator logs
   - Error messages

## Next Steps

- [Common Issues](./common-issues.md) - Frequently encountered problems
- [Debugging](./debugging.md) - Step-by-step debugging
- [FAQ](./faq.md) - Frequently asked questions
