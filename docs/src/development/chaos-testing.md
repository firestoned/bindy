# Chaos Testing

Unit tests and the steady-state e2e suites prove that bindy reaches a correct
state once. They do not prove it gets back there after pods, containers and
operator replicas die in the middle of its work, which is where release
candidates kept breaking on real clusters. The chaos suite
(`tests/e2e/chaos_test.sh`, roadmap 18) is that proof: it builds a realistic
topology on kind, breaks it fifteen ways, and after every break checks that
DNS is served correctly from every BIND9 pod, that every status tells the
truth, and that the operator then goes quiet.

## Running it

```bash
# Build the operator image, create the kind cluster (deploy/kind-config-chaos.yaml:
# one control plane, two workers, no host ports) and run both passes
make e2e-chaos

# Against a prebuilt image
make e2e-chaos E2E_IMAGE=ghcr.io/firestoned/bindy:<tag>

# Keep the cluster afterwards, then re-run a subset against it
KEEP_CLUSTER=1 make e2e-chaos
CHAOS_STEPS=4,5,7 CHAOS_PASSES=1 CLUSTER_NAME=bindy-e2e-chaos \
  tests/e2e/chaos_test.sh --skip-deploy
```

The kind credentials go to the dedicated kind kubeconfig, never to the one
your shell uses (see the [Testing Guide](testing-guide.md)). On a podman host
set `KIND_EXPERIMENTAL_PROVIDER=podman`. A full run (baseline, fifteen steps
in a fixed order, the same fifteen in a shuffled order) takes about two and
a half hours.

| Knob | Default | Meaning |
|---|---|---|
| `CHAOS_STEPS` | all | Comma list of step numbers to run |
| `CHAOS_PASSES` | `2` | `1` runs only the fixed order |
| `CHAOS_SEED` | `20261007` | Seed of the shuffled order (printed with the order) |
| `CONVERGE_TIMEOUT` | `120` | Seconds every invariant has to hold after a step |
| `QUIET_WINDOW` | `60` | Seconds the operator must stay quiet after convergence |
| `ALSO_NOTIFY_MODE` | `clusterip` | Expected `also-notify` form (ADR-0019) |

## The fixture

- A `Bind9Cluster` with two primaries and one secondary (one replica each),
  plus a standalone primary in the same cluster. The secondary really
  transfers its zones: nothing is written to it directly.
- A forward zone with A, AAAA, CNAME, MX, TXT, SRV and CAA records, a reverse
  zone with a PTR record, and an extra zone that step 12 replaces.
- The operator at two replicas with leader election.
- A tools pod (dig, used to query each BIND9 pod by pod IP on 5353) and a
  prober pod that queries every BIND9 Service once a second for the whole run.

## Invariants (after every step)

Polled until they all hold at once, for at most `CONVERGE_TIMEOUT`; the time
it took is reported per step.

- Every instance has exactly one live, Ready pod; the operator has all its
  replicas Ready, the lease holder is a live pod, and no operator container
  restarted except the ones the step killed.
- Every BIND9 pod, primaries and secondary, serves the exact expected answer
  for every record (deleted churn records must not resolve) and a SOA for
  every zone. The secondary's serial equals the serial of at least one
  primary (independent primaries can differ in serial; the data cannot).
- `rndc zonestatus` reports every zone loaded on every pod, and deleted zones
  gone.
- `rndc showzone`: every primary's `allow-transfer` is exactly the live
  secondary pod IPs and its `also-notify` exactly the secondary Services'
  ClusterIPs; every secondary's `primaries` are exactly the live primary pod
  IPs on 5353. Nothing stale (ADR-0019).
- Every `DNSZone`, `Bind9Instance`, `Bind9Cluster` and record is `Ready=True`
  with no `Degraded=True` and no `RolloutQueued` left. A step that times out
  with every status claiming Ready while a DNS invariant fails is reported as
  **UNTRUTHFUL STATUS**.
- Quiet: over `QUIET_WINDOW` after convergence the reconcile counters on the
  metrics endpoint grow by at most 3, no Deployment is PATCHed, no `ERROR`
  line is logged and no `WARN` line repeats three times.

## The steps

| # | Chaos |
|---|---|
| 1 | Delete the leader operator pod |
| 2 | Delete both operator pods at once |
| 3 | Scale the operator to 0 for 60 s, delete the secondary's Service and the cluster ConfigMap, scale back (drift repair on start) |
| 4 | Delete one primary pod |
| 5 | Delete the secondary pod |
| 6 | Delete every BIND9 pod at once |
| 7 | Delete a primary and the secondary at the same time |
| 8 | Kill `named` in a primary and in the secondary (container restart, same pod IP) |
| 9 | Kill bindcar in a primary and in the secondary |
| 10 | A cluster configuration change that rolls every instance (a bindcar env var added, then removed); one instance at a time (ADR-0018) |
| 11 | Create, change and delete records while steps 4 to 9 happen |
| 12 | Create a new zone and delete one while a pod is down |
| 13 | Kill the operator in the middle of the step 10 rollout |
| 14 | Delete a pod from a cordoned node so it reschedules to the other worker |
| 15 | Delete one record while a primary's bindcar container is down, and another while its `named` is down, each after the pod reports `ContainersReady=False`; both must disappear from every pod (the regression for an orphaned record, ADR-0015 decision 7) |

## DNS availability bounds

The prober reports, per step, the longest run of consecutive failed seconds
per Service and the longest run of seconds in which a zone had no answering
Service at all. Some gaps are the documented behaviour of a one-replica
instance, so they are bounded rather than forbidden:

- The Service of an instance whose only pod is deleted, whose `named` or
  bindcar container restarts, or whose Service is deleted (step 3) may fail
  for up to 120 s. A restarted `named` keeps the pod's IP but answers only
  once its readiness probe passes; a deleted Service comes back with a new
  ClusterIP, which resolvers learn through the cluster DNS cache.
- Every Service the step did not touch may fail for at most 2 consecutive
  seconds, and a zone must never be left with no answering Service, except in
  step 6, where every nameserver is deleted on purpose and the zone must
  recover within 120 s.
- A staggered rollout (steps 10 and 13) may cost a rolling Service at most
  3 consecutive seconds, and at most one instance of the cluster may be
  mid-rollout at any sampled second.

## Reading a failure

The run keeps going after a failed step and prints a results table at the
end (step, convergence seconds, quiet reconciles, per-Service gaps, zone
outage, verdict) followed by every violation. For a step that did not
converge, the operator, `named` and bindcar logs, `rndc` reports, pod list and
CR dumps are saved under the run's state directory (`diag-<pass>-<step>`).
