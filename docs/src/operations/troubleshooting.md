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

### Records Publish Slowly in Bulk, or Stay Served After Deletion

Before v0.8.0-rc.3, creating hundreds of records at once could keep many of
them `NotSelected` or unpublished for minutes, with the operator pinned at
its client-side API limit (`bindy_firestoned_io_kube_api_requests_total`
climbing at close to `BINDY_KUBE_QPS`). Every record write re-read each
instance's RNDC key Secret and Endpoints, and record reconciles woke each
other through the zone's `status.records`. A deleted record could also stay
in DNS when its finalizer's cleanup failed under that load, because the
zone's backup cleanup gave up after one attempt.

From v0.8.0-rc.3 ([ADR-0015](https://github.com/firestoned/bindy/blob/main/docs/adr/0015-bounded-api-cost-of-dns-writes.md))
a write reads each instance's key and endpoints once per reconcile, a zone
reconcile no longer touches every record, and a deleted record stays listed
in its zone's `status.records` until its data is confirmed gone from every
primary. If a deleted record is still answered, check the zone's events and
the operator log for:

```
N deleted record(s) of zone <namespace>/<zone> may still be served; their DNS cleanup is retried
```

That warning repeats on every zone retry (the zone schedules the retry
itself with capped backoff, ADR-0016) until every primary endpoint is
reachable again; the record is then removed from DNS and from the status.

### A Record or Zone Changed Inside BIND9 Is Not Put Back

From the release that implements
[ADR-0016](https://github.com/firestoned/bindy/blob/main/docs/adr/0016-event-driven-reconciliation.md)
the operator has no periodic resync. It used to re-push every record every
5 minutes, which also reverted changes made directly in BIND9. Now a change
made inside a running BIND9 pod (`nsupdate`, `rndc delzone`, a direct
bindcar call) raises no Kubernetes event, so it stands until the next event
for the resource that owns the data:

- a change to the resource's spec, labels, annotations or finalizers;
- the BIND9 pod being restarted or replaced (the zone's `Endpoints` change,
  the zone is re-created and its records replayed);
- the operator restarting (every object is reconciled from the initial list).

To force the repair now, change any annotation on the resource. The
documented annotation is `bindy.firestoned.io/reconcile-trigger`; its value
only has to differ from the previous one:

```bash
# One record: re-push it to every primary
kubectl annotate arecord www -n dns \
  bindy.firestoned.io/reconcile-trigger="$(date +%s)" --overwrite

# A zone: re-check it on every instance; a missing zone is re-created and all
# of its records are replayed
kubectl annotate dnszone example-com -n dns \
  bindy.firestoned.io/reconcile-trigger="$(date +%s)" --overwrite

# Every record of one kind in a namespace
kubectl annotate arecords --all -n dns \
  bindy.firestoned.io/reconcile-trigger="$(date +%s)" --overwrite
```

A zone that still exists inside BIND9 is not replayed by the zone annotation
alone (its records are only replayed when the zone had to be re-created), so
annotate the records themselves to re-push records deleted from a live zone.

Anything else that waits is woken by the object it waits on, not by a
timer: a `NotSelected` record by a zone tagging it, a `ZoneNotFound` or
`NoPrimaryInstances` record by its zone's next status change, a zone with no
instances by a matching `Bind9Instance`, a `DuplicateZone` zone by the other
claimant's change or deletion. A record whose write BIND9 rejected is retried
with backoff, never sooner than 30 s.

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
