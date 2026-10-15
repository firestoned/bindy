# Secret Audit Log

Every Kubernetes Secret operation bindy performs emits exactly one structured
audit event ([ADR-0024](https://github.com/firestoned/bindy/blob/main/docs/adr/0024-secret-operations-audit-log.md)).
The events answer, for each operation: what was done, to which Secret, for
which object, why, and whether it worked. They never contain Secret data,
keys, tokens, or API error messages.

The Kubernetes API server audit log stays the authoritative record that a
request was made. bindy's events add the *why* (which instance a key serves,
whether a write was a rotation, a mirror following its source, or a recreate)
and can be joined to the API server log by namespace, name and time.

## What is recorded

| Field | Values |
|---|---|
| `kind` | always `audit` |
| `resource` | always `Secret` |
| `operation` | `create`, `replace`, `patch`, `delete`, `read` |
| `outcome` | `success`, `failure` |
| `error` | on failure only: the API status code and reason (`409 AlreadyExists`), or a category (`transport error`) |
| `namespace`, `name` | the Secret |
| `purpose` | `rndc-key`, `rndc-key-mirror`, `rndc-key-source`, `bindcar-ca`, `scout-kubeconfig`, `serviceaccount-token` |
| `reason` | `initial`, `rotation`, `mirror-sync`, `verify`, `recreate-algorithm`, `recreate-malformed`, `mode-switch`, `instance-deleted`, `key-load`, `client-build`, `revoke` |
| `actor` | `bind9instance`, `bind9cluster`, `scout`, `bootstrap`, `bindy-bind9` |
| `owner` | the object the operation serves, e.g. `Bind9Instance dns/primary-0` |

A `read` event means key material was actually read: loading a key to sign a
DNS UPDATE (on a key-cache miss), reading a `secretRef` source to mirror it,
or reading a CA bundle or kubeconfig to build a client. Checks that only need
to know whether a Secret exists, its rotation annotations, or whether a
mirror is current read metadata only and emit nothing. The operator's Secret
watch is a metadata watch, so it never receives Secret data at all.

A read that finds no Secret (404) gave up no key and is not recorded.

## Output and routing

Events go to stdout with the other logs, on the tracing target
`bindy::audit`. Use JSON output for a SIEM:

```yaml
env:
  - name: RUST_LOG_FORMAT
    value: json
```

A JSON event looks like:

```json
{
  "timestamp": "2026-10-14T12:49:39.299728Z",
  "level": "INFO",
  "fields": {
    "message": "Secret operation",
    "kind": "audit",
    "resource": "Secret",
    "operation": "replace",
    "outcome": "success",
    "namespace": "bindy-system",
    "name": "primary-0-rndc-key",
    "purpose": "rndc-key-mirror",
    "reason": "mirror-sync",
    "actor": "bind9instance",
    "owner": "Bind9Instance bindy-system/primary-0"
  }
}
```

Route on `fields.kind == "audit"` to send the events to a separate,
retention-controlled sink (see [Audit Log Retention](../security/audit-log-retention.md)).
A failure is logged at `WARN`, a success at `INFO`.

## The audit trail is always on

The operator, `bindy scout` and `bindy bootstrap` add `bindy::audit=info` to
whatever `RUST_LOG` says, unless `RUST_LOG` names `bindy::audit` itself. So
quieting ordinary logs (`RUST_LOG=warn`) keeps the audit trail. To turn it
off, say so explicitly, which is visible in the Deployment spec:

```yaml
env:
  - name: RUST_LOG
    value: "warn,bindy::audit=off"
```

## Alerting

Each event also increments
`bindy_firestoned_io_secret_operations_total{operation, purpose, outcome}`.
For example, alert when RNDC key loads start failing:

```promql
sum(rate(bindy_firestoned_io_secret_operations_total{purpose="rndc-key", outcome="failure"}[5m])) > 0
```

## Not covered

Secrets that only Kubernetes reads on bindy's behalf, such as the kubelet
mounting `{instance}-rndc-key` or a DNSSEC key Secret into a pod. The API
server audit log records those.

## Adding a Secret use (developers)

All Secret access goes through `bindy_controller_sdk::audit::SecretApi`. A
test fails the build when other non-test code builds an `Api<Secret>`. A new
use needs a `SecretPurpose` (and, if none fits, an `AuditReason`) variant;
the closed sets are enums, so an unlisted use does not compile.
