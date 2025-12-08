---
name: RNDC Secret Reload Feature
about: Implement automatic RNDC secret change detection and reload
title: 'Implement RNDC Secret Change Detection and Hot Reload'
labels: enhancement, security, rndc
assignees: ''
---

## Summary

Implement automatic detection of RNDC secret changes and hot reload BIND9 configuration via SIGHUP signal, enabling secure key rotation without downtime.

## Problem

Currently, when an RNDC secret is updated (manually or via external secret manager):
- BIND9 continues using the old key loaded at startup
- The new key is mounted but BIND9 doesn't reload it
- Management operations fail (controller has new key, BIND9 expects old key)
- **Manual intervention required**: Restart pods or manually exec SIGHUP

This prevents:
- Security best practices (regular key rotation)
- Integration with external secret managers (Vault, sealed-secrets)
- Automated secret lifecycle management

## Proposed Solution

See [ADR-0001: RNDC Secret Reload](../../docs/adr/0001-rndc-secret-reload.md) for full design.

**High-level approach:**
1. Track RNDC secret `resourceVersion` in `Bind9InstanceStatus`
2. Detect version changes in reconciler
3. Send SIGHUP to affected pods only (selective reload)
4. Update status to reflect new version

**Key benefits:**
- ✅ Zero downtime (hot reload vs pod restart)
- ✅ Selective (only affected instances reload)
- ✅ Secure (enables key rotation best practices)
- ✅ Observable (status tracks loaded version)

## Implementation Phases

### Phase 1: MVP - Basic Secret Tracking
- [ ] Add `rndc_secret_version: Option<String>` to `Bind9InstanceStatus` CRD
- [ ] Regenerate CRDs: `cargo run --bin crdgen`
- [ ] Implement `send_sighup_to_bind9_pods()` helper function
- [ ] Add secret version tracking in `reconcile_bind9_instance()`
- [ ] Update RBAC to include `pods/exec` permission
- [ ] Add unit tests for SIGHUP logic
- [ ] Update documentation

### Phase 2: Secret Watch
- [ ] Implement Secret controller/watcher
- [ ] Link Secret changes to Bind9Instance reconciliation
- [ ] Add filtering for RNDC-specific secrets
- [ ] Add metrics: `bind9_rndc_secret_reloads_total{status="success|failed"}`

### Phase 3: Observability
- [ ] Add status condition: `RndcSecretReloaded`
- [ ] Add Kubernetes events: `RndcSecretReloaded`, `RndcSecretReloadFailed`
- [ ] Add troubleshooting guide for reload failures

### Phase 4: Advanced (Future)
- [ ] Support user-provided secrets (not just auto-generated)
- [ ] Validate secret contents before reload
- [ ] Rate limiting for rapid secret changes
- [ ] Graceful handling of secret deletion

## Technical Details

### CRD Changes

```rust
// src/crd.rs
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bind9InstanceStatus {
    pub conditions: Option<Vec<Condition>>,
    pub observed_generation: Option<i64>,
    pub replicas: Option<i32>,
    pub ready_replicas: Option<i32>,

    // NEW: Track RNDC secret version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rndc_secret_version: Option<String>,
}
```

### SIGHUP Implementation

```rust
// src/reconcilers/bind9instance.rs
async fn send_sighup_to_bind9_pods(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<()> {
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);

    // Find pods for this instance
    let selector = format!("app=bind9,instance={}", instance_name);
    let pods = pod_api.list(&ListParams::default().labels(&selector)).await?;

    for pod in pods.items {
        let pod_name = pod.metadata.name.unwrap();

        // Send SIGHUP to BIND9 process (PID 1)
        let attach_params = AttachParams {
            container: Some("bind9".to_string()),
            ..Default::default()
        };

        match pod_api.exec(&pod_name, vec!["kill", "-HUP", "1"], &attach_params).await {
            Ok(_) => info!("Sent SIGHUP to pod {}/{}", namespace, pod_name),
            Err(e) => warn!("Failed to send SIGHUP to {}: {}", pod_name, e),
        }
    }

    Ok(())
}
```

### RBAC Changes

```yaml
# deploy/rbac.yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: bindy-operator
rules:
  # ... existing rules ...

  # NEW: Permission to exec into pods for SIGHUP
  - apiGroups: [""]
    resources: ["pods/exec"]
    verbs: ["create"]
```

## Testing Plan

### Unit Tests
- [ ] `test_secret_version_change_triggers_reload()`
- [ ] `test_sighup_sent_to_all_pod_replicas()`
- [ ] `test_sighup_failure_handling()`
- [ ] `test_no_reload_when_version_unchanged()`

### Integration Tests
```bash
# Test secret rotation
kubectl apply -f examples/bind9instance.yaml
kubectl wait --for=condition=Ready bind9instance/test-instance

# Rotate secret
kubectl delete secret test-instance-rndc-key
sleep 10

# Verify BIND9 reloaded
kubectl logs -l app=bind9,instance=test-instance | grep "reloading configuration"
rndc -s test-instance status  # Should succeed with new key

# Verify status updated
kubectl get bind9instance test-instance -o jsonpath='{.status.rndcSecretVersion}'
```

## Security Considerations

⚠️ **RBAC Permission**: This feature requires `pods/exec` permission, which allows executing commands in pods.

**Mitigations:**
- Use label selectors to limit scope to BIND9 pods only
- Audit log all exec operations
- Document security implications clearly
- Consider making this feature opt-in via configuration flag

## Documentation Updates

Required documentation:
1. **User Guide**: "RNDC Key Rotation" - How to rotate secrets manually and integrate with external secret managers
2. **Operations Guide**: "Monitoring Secret Reloads" - Metrics, verification, troubleshooting
3. **Development Guide**: "Secret Watch Implementation" - Architecture and code walkthrough
4. **Security Guide**: "RBAC and Pod Exec" - Security implications and audit process

## Success Criteria

- [ ] RNDC secret can be updated without pod restart
- [ ] Only instances using the changed secret reload
- [ ] Status accurately reflects loaded secret version
- [ ] SIGHUP failures are logged and visible in status
- [ ] All tests pass
- [ ] Documentation complete
- [ ] RBAC properly configured

## Related

- ADR: [docs/adr/0001-rndc-secret-reload.md](../../docs/adr/0001-rndc-secret-reload.md)
- Related to: External secret integration (#TBD)
- Blocks: Automated key rotation (#TBD)

## References

- BIND9 Signal Handling: https://bind9.readthedocs.io/en/latest/reference.html#namedconf-statement-controls
- Kubernetes Pod Exec: https://kubernetes.io/docs/reference/generated/kubernetes-api/v1.28/#podexecoptions-v1-core
- External Secrets Operator: https://external-secrets.io/
