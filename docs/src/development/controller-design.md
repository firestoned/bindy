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
            // Success - requeue with normal delay
            queue.requeue(resource, Duration::from_secs(300));
        }
        Err(e) => {
            // Error - retry with backoff
            queue.requeue_with_backoff(resource, e);
        }
    }
}
```

## State Management

Operator maintains no local state - all state in Kubernetes:
- CRD resources (desired state)
- Deployments, Services, ConfigMaps (actual state)
- Status fields (observed state)

## Error Handling

- Transient errors: Retry with exponential backoff
- Permanent errors: Update status, log, requeue
- Resource conflicts: Retry with latest version
