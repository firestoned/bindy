# Operator Design

Design and implementation of the Bindy operator.

## Operator Pattern

Bindy implements the Kubernetes operator pattern:

1. **Watch** - Monitor CRD resources
2. **Reconcile** - Ensure actual state matches desired
3. **Update** - Apply changes to Kubernetes resources

## Reconciliation Loop

```rust
loop {
    // Get resource from work queue
    let resource = queue.pop();
    
    // Reconcile
    match reconcile(resource).await {
        Ok(_) => {
            // Success - wait for the next watch event, no periodic resync
            // (ADR-0016). A reconcile that knows of a future instant (an RNDC
            // key falling due) schedules one wake for it instead.
        }
        Err(e) => {
            // Error - retry with per-object capped backoff
            queue.requeue_with_backoff(resource, e);
        }
    }
}
```

The real controllers express this through `kube::runtime::Controller`: a
reconcile returns `Action::await_change()` on success and on a wait for
another object, `sdk::error::retry_action` (per-object backoff) when it
finished but failed against BIND9, and `sdk::reconcile::scheduled_action`
for a known future instant. `REQUEUE_WHEN_READY_SECS` and
`REQUEUE_WHEN_NOT_READY_SECS` no longer exist. Every wait must name the watch
that ends it; ADR-0016 has the table.

## State Management

Operator maintains no local state - all state in Kubernetes:
- CRD resources (desired state)
- Deployments, Services, ConfigMaps (actual state)
- Status fields (observed state)

## Error Handling

- Transient errors: Retry with exponential backoff
- Permanent errors: Update status, log, requeue
- Resource conflicts: Retry with latest version
