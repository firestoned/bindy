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
- the BIND9 pod being replaced (the zones-loaded gate loads every live zone
  and its records onto the new pod before it is Ready, ADR-0017);
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

### A BIND9 Pod Stays Not Ready (Zones-Loaded Readiness Gate)

Every BIND9 pod carries the readiness gate `bindy.firestoned.io/zones-loaded`
([ADR-0017](https://github.com/firestoned/bindy/blob/main/docs/adr/0017-zones-loaded-readiness-gate.md)).
Kubernetes reports the pod `Ready`, and routes its Service to it, only once
the operator has loaded every live zone of the pod's instance onto it. A pod
whose containers are all running but which shows `READY 2/2` with the pod
still out of its Service, or `kubectl rollout status` waiting, is held by the
gate. Read the condition:

```bash
kubectl get pod <pod> -n <namespace> -o jsonpath='{range .status.conditions[*]}{.type}={.status} {.reason}: {.message}{"\n"}{end}'
# or
kubectl get pod <pod> -n <namespace> -o wide   # READINESS GATES column: 0/1 or 1/1
```

| `reason` | Meaning | What to do |
|---|---|---|
| *(no condition)* | The operator has not evaluated the pod yet. Normal for the seconds before the containers are ready. | If it lasts: is the operator running and leader? Can it patch `pods/status` (`kubectl auth can-i patch pods/status -n <namespace> --as=system:serviceaccount:bindy-system:bindy`)? An operator older than the pod template does not know the gate. |
| `ZonesLoading` | The operator is writing the zones and records to the pod. | Wait; large zones take as long as their record replay. |
| `ZonesLoadFailed` | A zone could not be loaded on this pod while another pod of the instance still serves it. The old pod keeps serving; the rollout waits. Retried with backoff (2 s to 60 s). | The message names each zone and the error. Check the zone's `DNSZone` status and the operator log (`Zones-loaded gate:`). |
| `InstanceUnknown` | The pod's `Bind9Instance` is not in the operator's cache. | Check that the instance exists; retried with backoff. |
| `ZonesLoaded` / `NoZones` | Gate open: every live zone loaded, or no live zone selects the instance. | Nothing. |
| `PodTerminating` | The pod is being deleted. The operator closed its gate at the start of termination so traffic moves to the remaining Ready pods while `named` drains (ADR-0017 decision 6). `kubectl get pods` shows it `0/2` (or `1/2`) until it is gone. | Nothing: expected for every terminating BIND9 pod. |
| `ZonesPartiallyLoaded` | Gate open, but the zones in the message could not be loaded and no other pod of the instance served them either. | Fix those zones; the `DNSZone` controller keeps retrying them. |

The gate is evaluated once per pod: once `True` it stays `True` for the pod's
life, including across container restarts, until the pod is deleted (then
`PodTerminating`). A zone created later is configured on the running pod by
the `DNSZone` controller as usual.

If the operator is down, new BIND9 pods stay not Ready on purpose: with the
default rolling update (`maxUnavailable` 0) the old pod keeps serving until
the operator is back.

### A Bind9Instance Does Not Roll Out (`RolloutQueued`)

A pod-template change (a cluster configuration change, an image or bindcar
change, a bindy upgrade that renders differently) is applied to one instance
at a time among instances that serve a zone in common or belong to the same
cluster
([ADR-0018](https://github.com/firestoned/bindy/blob/main/docs/adr/0018-staggered-bind9-rollouts.md)).
An instance waiting its turn shows:

```bash
kubectl get bind9instance <name> -n <namespace> \
  -o jsonpath='{range .status.conditions[?(@.type=="Rollout")]}{.status} {.reason}: {.message}{"\n"}{end}'
# False RolloutQueued: Pod template change waits for Bind9Instance dns/primary-0, which is rolling out (...)
```

Its pods keep serving the previous configuration and its `Ready` condition is
unaffected. It rolls as soon as the named instance finishes:

| The message says the blocker... | What to check |
|---|---|
| is rolling out | Normal: `kubectl rollout status deployment/<blocker> -n <namespace>`. A blocker held by its zones-loaded gate shows why in its new pod's `bindy.firestoned.io/zones-loaded` condition (section above). |
| is starting its rollout | Normal, lasts until the blocker's patch reaches the operator's cache (well under a second). |
| was queued earlier | Instances roll first come, first served; the blocker itself waits for another one. Follow the chain through each `Rollout` condition. |

A blocker whose rollout never completes stops blocking once its Deployment
reports `ProgressDeadlineExceeded` (after `progressDeadlineSeconds`, 600 s by
default); the waiting instance then rolls and reports
`Rollout=True, reason: RolloutPeerStalled`. Replica changes and new
instances are never queued.

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
