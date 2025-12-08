# ADR-0001: RNDC Secret Change Detection and Reload

## Status

Proposed

## Context

BIND9 instances use RNDC (Remote Name Daemon Control) for management operations. The RNDC authentication key is stored in a Kubernetes Secret and mounted into the BIND9 pod at `/etc/bind/keys/rndc.key`.

### Current Behavior

When an RNDC secret changes (manually updated or rotated):
- BIND9 continues using the **old key** loaded at startup
- The new key is mounted but BIND9 doesn't reload it
- Management operations fail (controller has new key, BIND9 expects old)
- **Manual intervention required**: Restart pods or exec SIGHUP

### Problem

1. **Secret Rotation**: Security requires periodic key rotation
2. **External Secrets**: Users may manage secrets externally (Vault, sealed-secrets)
3. **No Automatic Reload**: BIND9 doesn't detect file changes
4. **Selective Reload**: Only instances using the changed secret should reload

## Decision

Implement automatic RNDC secret change detection with selective pod SIGHUP signaling.

### Solution Architecture

#### 1. Track Secret Version in Status

```rust
pub struct Bind9InstanceStatus {
    // ... existing fields ...

    /// ResourceVersion of the RNDC secret currently loaded by BIND9
    pub rndc_secret_version: Option<String>,
}
```

#### 2. Detect Changes in Reconciler

```rust
// In reconcile_bind9_instance()
let current_version = secret.metadata.resource_version.clone();
let last_version = instance.status.rndc_secret_version.as_ref();

if last_version != current_version.as_ref() {
    send_sighup_to_pods(client, namespace, name).await?;
    status.rndc_secret_version = current_version;
}
```

#### 3. Send SIGHUP to Pods

```rust
async fn send_sighup_to_bind9_pods(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<()> {
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let selector = format!("app=bind9,instance={}", instance_name);
    let pods = pod_api.list(&ListParams::default().labels(&selector)).await?;

    for pod in pods.items {
        let pod_name = pod.metadata.name.unwrap();

        // Execute: kill -HUP 1 (BIND9 is PID 1)
        let attach_params = AttachParams {
            container: Some("bind9".to_string()),
            ..Default::default()
        };

        pod_api.exec(&pod_name, vec!["kill", "-HUP", "1"], &attach_params).await?;
        info!("Sent SIGHUP to pod {}/{}", namespace, pod_name);
    }
    Ok(())
}
```

#### 4. Watch Secrets (Optional Enhancement)

Add Secret watch to trigger reconciliation:

```rust
let secret_watcher = Controller::new(
    Api::<Secret>::all(client.clone()),
    WatcherConfig::default()
        .labels("app.kubernetes.io/component=rndc-key")
)
.run(reconcile_secret, error_policy, ctx);
```

#### 5. RBAC Updates

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-operator
rules:
  # ... existing rules ...

  # NEW: Permission to exec into pods
  - apiGroups: [""]
    resources: ["pods/exec"]
    verbs: ["create"]
```

## Alternatives Considered

### Option A: Rolling Restart (Rejected)

Update Deployment annotation → trigger rolling restart.

**Cons:**
- ❌ Downtime during pod restart
- ❌ Kills active connections
- ❌ Slower than config reload
- ❌ Unnecessary resource churn

### Option B: Sidecar Watcher (Rejected)

Sidecar container watches secret file and sends SIGHUP.

**Cons:**
- ❌ Resource overhead (extra container per pod)
- ❌ Deployment changes required
- ❌ Over-engineered

### Option C: RNDC Reconfig Command (Rejected)

Use `rndc reconfig` instead of SIGHUP.

**Cons:**
- ❌ Circular dependency: need working RNDC to reload RNDC key
- ❌ Fails if key already changed

### Selected: SIGHUP with Status Tracking ✅

**Pros:**
- ✅ Zero downtime (hot reload)
- ✅ Selective (only affected instances)
- ✅ Efficient (no pod restarts)
- ✅ Observable (status tracks version)
- ✅ Works when RNDC auth broken

## Consequences

### Positive

1. **Automatic Key Rotation**: No manual intervention
2. **Security**: Enables regular rotation
3. **External Secrets**: Works with Vault, etc.
4. **Zero Downtime**: SIGHUP reloads without restart
5. **Observability**: Status shows loaded version

### Negative

1. **RBAC**: Requires `pods/exec` permission (security risk)
2. **Complexity**: Additional reconciliation logic
3. **Timing Window**: Brief gap between controller and BIND9 having different keys
4. **Error Handling**: SIGHUP could fail

### Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| SIGHUP fails silently | Log errors, status condition, retry |
| Rapid secret changes | Rate limiting, debounce |
| Secret deleted | Graceful handling, don't crash |
| Exec permission abuse | Audit logs, minimal scope |

## Implementation Plan

### Phase 1: Foundation (MVP)
- [ ] Add `rndc_secret_version` to `Bind9InstanceStatus` CRD
- [ ] Regenerate CRDs: `cargo run --bin crdgen`
- [ ] Implement `send_sighup_to_bind9_pods()` function
- [ ] Add secret version tracking in reconciler
- [ ] Update RBAC with `pods/exec`
- [ ] Add unit tests

### Phase 2: Secret Watch
- [ ] Implement Secret controller
- [ ] Link Secret changes to reconciliation
- [ ] Add metrics: `bind9_rndc_secret_reloads_total`

### Phase 3: Observability
- [ ] Add status condition: `RndcSecretReloaded`
- [ ] Add events: `RndcSecretReloaded`, `RndcSecretReloadFailed`
- [ ] Documentation: secret rotation guide

### Phase 4: Advanced
- [ ] Support user-provided secrets
- [ ] Validate secret before reload
- [ ] Rate limiting for rapid changes

## Testing

### Unit Tests
```rust
#[tokio::test]
async fn test_secret_version_change_triggers_reload() {
    // Given: instance with rndc_secret_version = "v1"
    // When: secret changes to "v2"
    // Then: SIGHUP sent to pods
}

#[tokio::test]
async fn test_sighup_sent_to_all_replicas() {
    // Given: instance with 3 pod replicas
    // When: secret changes
    // Then: all 3 pods receive SIGHUP
}
```

### Integration Tests
```bash
# Create instance
kubectl apply -f examples/bind9instance.yaml
kubectl wait --for=condition=Ready bind9instance/test

# Rotate secret
kubectl delete secret test-rndc-key
sleep 5

# Verify reload
kubectl logs -l app=bind9,instance=test | grep "reloading"
rndc -s test status  # Should succeed
```

## Documentation

Required updates:
1. **User Guide**: RNDC Key Rotation
2. **Operations**: Monitoring Secret Reloads
3. **Development**: Secret Watch Architecture
4. **Security**: RBAC and Pod Exec

## References

- BIND9 Signals: https://bind9.readthedocs.io/en/latest/reference.html
- Kubernetes Secrets: https://kubernetes.io/docs/concepts/configuration/secret/

---

**Author:** Erick Bourgeois
**Date:** 2025-12-01
**Status:** Proposed
