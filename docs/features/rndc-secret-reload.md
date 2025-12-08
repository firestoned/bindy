# RNDC Secret Hot Reload

> **Status:** 📋 Planned - See [ADR-0001](../adr/0001-rndc-secret-reload.md) for full design

## Overview

Automatically detect RNDC secret changes and reload BIND9 configuration without pod restarts, enabling secure key rotation and integration with external secret managers.

## Problem

Currently, updating an RNDC secret requires manual pod restart:

```bash
# Update secret
kubectl delete secret my-instance-rndc-key

# Manual intervention required
kubectl rollout restart deployment my-instance
# OR
kubectl exec my-instance-0 -- kill -HUP 1
```

This prevents:
- 🔒 Regular security key rotation
- 🔑 External secret manager integration (Vault, sealed-secrets)
- ⚡ Zero-downtime secret updates

## Proposed Solution

**Automatic detection and selective reload:**

1. **Track secret version** in `Bind9InstanceStatus.rndcSecretVersion`
2. **Detect changes** by comparing `resourceVersion` during reconciliation
3. **Send SIGHUP** to affected pods only (not all instances)
4. **Update status** to reflect new version

```yaml
# Status tracks loaded secret version
status:
  rndcSecretVersion: "12345"  # Updated when secret changes
  conditions:
    - type: RndcSecretReloaded
      status: "True"
      lastTransitionTime: "2025-12-07T15:00:00Z"
```

## Benefits

- ✅ **Zero Downtime**: SIGHUP reloads config without pod restart
- ✅ **Selective**: Only instances using changed secret reload
- ✅ **Secure**: Enables automated key rotation
- ✅ **Observable**: Status shows loaded version
- ✅ **External Secrets**: Works with Vault, sealed-secrets, etc.

## Implementation Phases

### Phase 1: MVP ✨
- Add `rndcSecretVersion` to status CRD
- Implement SIGHUP signaling to pods
- Track secret version in reconciler
- Add RBAC: `pods/exec` permission

### Phase 2: Secret Watch
- Watch Secret resources
- Auto-trigger reconciliation on secret change
- Add metrics: `bind9_rndc_secret_reloads_total`

### Phase 3: Observability
- Status conditions: `RndcSecretReloaded`
- Kubernetes events
- Troubleshooting guide

### Phase 4: Advanced
- User-provided secret support
- Secret validation before reload
- Rate limiting

## Example Usage (Future)

```bash
# Create instance with auto-generated secret
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: my-dns
spec:
  role: primary
  replicas: 3
EOF

# Secret auto-generated: my-dns-rndc-key
# Status tracks version: status.rndcSecretVersion = "v1"

# External secret manager updates the secret
# (e.g., Vault rotates key automatically)

# Operator detects change automatically:
# 1. Sees resourceVersion changed from "v1" to "v2"
# 2. Sends SIGHUP to all 3 pods
# 3. Updates status.rndcSecretVersion = "v2"
# 4. Emits event: RndcSecretReloaded

# No manual intervention required! ✨
```

## Security Considerations

⚠️ **RBAC Permission Required**: `pods/exec` allows executing commands in pods.

**Mitigations:**
- Label selectors limit scope to BIND9 pods only
- Audit logs track all exec operations
- Optional: Make feature opt-in via config flag

## Technical Details

See [ADR-0001: RNDC Secret Reload](../adr/0001-rndc-secret-reload.md) for:
- Full architecture design
- Alternative approaches evaluated
- Implementation plan
- Testing strategy
- Code examples

## Related

- **ADR**: [ADR-0001](../adr/0001-rndc-secret-reload.md)
- **Issue**: TBD (to be created from [template](../../.github/ISSUE_TEMPLATE/feature-rndc-secret-reload.md))
- **External Secrets Integration**: TBD
- **Automated Key Rotation**: TBD

## Timeline

- **Design**: ✅ Complete (2025-12-07)
- **Phase 1 (MVP)**: 📅 TBD
- **Phase 2 (Watch)**: 📅 TBD
- **Phase 3 (Observability)**: 📅 TBD
- **Phase 4 (Advanced)**: 📅 TBD

---

**Status:** 📋 Awaiting prioritization and implementation
