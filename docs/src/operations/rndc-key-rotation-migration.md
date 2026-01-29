# RNDC Key Rotation Migration Guide

This guide walks you through migrating existing Bindy deployments to use automatic RNDC key rotation.

---

## Overview

**What's Changing:**

- **Old**: `rndcSecretRef` field (deprecated but still supported)
- **New**: `rndcKey` field with auto-rotation support

**Backward Compatibility:**

- ✅ Existing deployments using `rndcSecretRef` continue to work
- ✅ No breaking changes - migration is optional but recommended
- ✅ Gradual migration supported (migrate instances one at a time)

---

## Prerequisites

Before migrating, ensure:

1. **Bindy operator updated** to version supporting RNDC rotation (v0.4.0+)
2. **CRDs regenerated** with latest schema:
   ```bash
   kubectl replace --force -f deploy/crds/bind9instances.crd.yaml
   kubectl replace --force -f deploy/crds/bind9clusters.crd.yaml
   ```
3. **Backup existing Secrets**:
   ```bash
   kubectl get secrets -n dns-system -l app.kubernetes.io/component=rndc -o yaml > rndc-secrets-backup.yaml
   ```

---

## Migration Scenarios

### Scenario 1: Migrate from Deprecated `rndcSecretRef` (Instance-Level)

**Before** (deprecated field):

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-primary
  namespace: dns-system
spec:
  clusterRef: my-cluster
  role: Primary
  rndcSecretRef:  # DEPRECATED
    name: my-rndc-secret
    keyNameKey: key-name
    algorithmKey: algorithm
    secretKey: secret
```

**After** (new field with rotation):

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-primary
  namespace: dns-system
spec:
  clusterRef: my-cluster
  role: Primary
  rndcKey:  # NEW
    autoRotate: true
    rotateAfter: "2160h"  # 90 days
    algorithm: hmac-sha256
    # No secretRef - operator auto-generates and rotates
```

**Migration Steps:**

1. **Remove `rndcSecretRef` field**:
   ```bash
   kubectl edit bind9instance dns-primary -n dns-system
   # Delete the rndcSecretRef section
   ```

2. **Add `rndcKey` field**:
   ```yaml
   spec:
     rndcKey:
       autoRotate: true
       rotateAfter: "2160h"
       algorithm: hmac-sha256
   ```

3. **Apply changes**:
   ```bash
   kubectl apply -f bind9instance.yaml
   ```

4. **Verify new Secret created**:
   ```bash
   kubectl get secret dns-primary-rndc -n dns-system -o yaml
   ```

5. **Check rotation annotations**:
   ```bash
   kubectl get secret dns-primary-rndc -n dns-system -o jsonpath='{.metadata.annotations}'
   ```

   Expected:
   ```json
   {
     "bindy.firestoned.io/rndc-created-at": "2025-01-27T...",
     "bindy.firestoned.io/rndc-rotate-at": "2025-04-27T...",
     "bindy.firestoned.io/rndc-rotation-count": "0"
   }
   ```

6. **Delete old Secret** (after verifying new one works):
   ```bash
   kubectl delete secret my-rndc-secret -n dns-system
   ```

---

### Scenario 2: Keep Existing Secret (No Rotation)

If you want to keep using your existing Secret **without rotation**:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-primary
  namespace: dns-system
spec:
  clusterRef: my-cluster
  role: Primary
  rndcKey:
    secretRef:  # Reference existing Secret (no rotation)
      name: my-rndc-secret
      keyNameKey: key-name
      algorithmKey: algorithm
      secretKey: secret
    # autoRotate is IGNORED for secretRef
```

**Use Case**: Externally managed secrets (Vault, AWS Secrets Manager, etc.)

---

### Scenario 3: Migrate Cluster-Level Configuration

**Before** (deprecated cluster config):

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: my-cluster
  namespace: dns-system
spec:
  primary:
    rndcSecretRef:  # DEPRECATED
      name: primary-rndc
      keyNameKey: key-name
      algorithmKey: algorithm
      secretKey: secret
  secondary:
    rndcSecretRef:  # DEPRECATED
      name: secondary-rndc
      keyNameKey: key-name
      algorithmKey: algorithm
      secretKey: secret
```

**After** (new cluster config with rotation):

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: my-cluster
  namespace: dns-system
spec:
  primary:
    rndcKey:  # NEW
      autoRotate: true
      rotateAfter: "720h"   # Primary: 30 days (more frequent)
      algorithm: hmac-sha512
  secondary:
    rndcKey:  # NEW
      autoRotate: true
      rotateAfter: "1440h"  # Secondary: 60 days
      algorithm: hmac-sha256
```

**Migration Steps:**

1. **Update cluster manifest**:
   ```bash
   kubectl edit bind9cluster my-cluster -n dns-system
   ```

2. **Replace `rndcSecretRef` with `rndcKey`** for each role

3. **Apply changes**:
   ```bash
   kubectl apply -f bind9cluster.yaml
   ```

4. **Restart instances** to pick up new configuration:
   ```bash
   kubectl rollout restart deployment/dns-primary -n dns-system
   kubectl rollout restart deployment/dns-secondary-1 -n dns-system
   ```

5. **Verify rotation status**:
   ```bash
   kubectl get bind9instance -n dns-system -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{.status.rndcKeyRotationStatus}{"\n"}{end}'
   ```

---

### Scenario 4: Gradual Migration (Test First)

For production environments, migrate one instance at a time:

**Step 1: Migrate one secondary instance**

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-secondary-1  # Start with secondary
  namespace: dns-system
spec:
  clusterRef: my-cluster
  role: Secondary
  rndcKey:
    autoRotate: true
    rotateAfter: "2160h"
    algorithm: hmac-sha256
```

**Step 2: Verify rotation works**

Wait for first rotation or trigger manual rotation:

```bash
# Trigger manual rotation
kubectl annotate secret dns-secondary-1-rndc \
  bindy.firestoned.io/rndc-created-at="2020-01-01T00:00:00Z" \
  --overwrite \
  -n dns-system

# Wait 1 minute for reconciliation
sleep 60

# Verify rotation occurred
kubectl get secret dns-secondary-1-rndc -n dns-system -o jsonpath='{.metadata.annotations.bindy\.firestoned\.io/rndc-rotation-count}'
# Output should be "1"
```

**Step 3: Migrate remaining secondaries**

Once verified, migrate other secondary instances.

**Step 4: Migrate primary (last)**

After all secondaries are migrated, migrate the primary instance.

---

## Rollback Procedure

If you encounter issues after migration, rollback by reverting to `rndcSecretRef`:

**Step 1: Restore old configuration**

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-primary
  namespace: dns-system
spec:
  clusterRef: my-cluster
  role: Primary
  rndcSecretRef:  # Restore deprecated field
    name: my-rndc-secret
    keyNameKey: key-name
    algorithmKey: algorithm
    secretKey: secret
  # Remove rndcKey section
```

**Step 2: Restore Secret from backup**

```bash
kubectl apply -f rndc-secrets-backup.yaml
```

**Step 3: Restart pods**

```bash
kubectl rollout restart deployment/dns-primary -n dns-system
```

**Step 4: Delete new Secret** (if created)

```bash
kubectl delete secret dns-primary-rndc -n dns-system
```

---

## Validation Checklist

After migration, verify:

- [ ] **CRDs updated** with new schema:
  ```bash
  kubectl get crd bind9instances.bindy.firestoned.io -o yaml | grep -A 10 "rndcKey"
  ```

- [ ] **Instances updated** with `rndcKey` field:
  ```bash
  kubectl get bind9instance -n dns-system -o yaml | grep -A 5 "rndcKey"
  ```

- [ ] **Secrets have rotation annotations**:
  ```bash
  kubectl get secrets -n dns-system -l app.kubernetes.io/component=rndc -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{.metadata.annotations.bindy\.firestoned\.io/rndc-rotation-count}{"\n"}{end}'
  ```

- [ ] **Rotation status populated**:
  ```bash
  kubectl get bind9instance dns-primary -n dns-system -o jsonpath='{.status.rndcKeyRotationStatus}'
  ```

- [ ] **Pods restarted successfully**:
  ```bash
  kubectl get pods -n dns-system -l app.kubernetes.io/name=dns-primary
  ```

- [ ] **RNDC communication working**:
  ```bash
  kubectl exec -n dns-system deployment/dns-primary -- rndc status
  ```

---

## Common Migration Issues

### Issue 1: Pods Not Restarting After Migration

**Symptom**: Pods still use old RNDC key after migration.

**Solution**:

```bash
# Manually trigger pod restart
kubectl rollout restart deployment/dns-primary -n dns-system

# Verify new Secret mounted
kubectl exec -n dns-system deployment/dns-primary -- cat /etc/bind/rndc.key
```

### Issue 2: Rotation Status Not Showing

**Symptom**: `status.rndcKeyRotationStatus` is empty.

**Possible Causes**:

1. **CRD schema not updated**:
   ```bash
   kubectl replace --force -f deploy/crds/bind9instances.crd.yaml
   ```

2. **Auto-rotation disabled**:
   ```bash
   kubectl get bind9instance dns-primary -n dns-system -o jsonpath='{.spec.rndcKey.autoRotate}'
   # Output should be "true"
   ```

3. **Secret missing annotations**:
   ```bash
   kubectl get secret dns-primary-rndc -n dns-system -o jsonpath='{.metadata.annotations}'
   ```

   If empty, manually add annotations (operator will update on next reconciliation):
   ```bash
   kubectl annotate secret dns-primary-rndc \
     bindy.firestoned.io/rndc-created-at="$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
     bindy.firestoned.io/rndc-rotate-at="$(date -u -v+90d +%Y-%m-%dT%H:%M:%SZ)" \
     bindy.firestoned.io/rndc-rotation-count="0" \
     -n dns-system
   ```

### Issue 3: Instance Using Wrong Configuration Level

**Symptom**: Instance not picking up cluster-level `rndcKey` configuration.

**Debug**:

```bash
# Check instance-level config (highest precedence)
kubectl get bind9instance dns-primary -n dns-system -o jsonpath='{.spec.rndcKey}'

# Check cluster role-level config
kubectl get bind9cluster my-cluster -n dns-system -o jsonpath='{.spec.primary.rndcKey}'

# Check operator logs for precedence resolution
kubectl logs -n dns-system -l app.kubernetes.io/name=bindy-operator | grep "Resolved RNDC config"
```

**Solution**: Ensure instance-level config doesn't override cluster config unintentionally.

---

## Migration Timeline

### Recommended Timeline for Production

| Phase | Duration | Activities |
|-------|----------|----------|
| **Week 1: Planning** | 5 days | - Review current RNDC setup<br>- Test migration in dev/staging<br>- Document rotation policy |
| **Week 2: Testing** | 5 days | - Migrate dev environment<br>- Trigger manual rotation test<br>- Validate pod restarts |
| **Week 3: Staging** | 5 days | - Migrate staging environment<br>- Monitor for 1 week<br>- Verify rotation schedule |
| **Week 4: Production** | 5 days | - Migrate secondary instances<br>- Wait 2-3 days<br>- Migrate primary instances |

**Total**: 4 weeks (can be accelerated for simpler environments)

---

## Post-Migration Best Practices

1. **Set up monitoring** for rotation events:
   ```bash
   # Example: Alert if rotation count hasn't increased in 100 days
   kubectl get bind9instance -A -o json | jq '.items[] | select(.status.rndcKeyRotationStatus.rotationCount == 0)'
   ```

2. **Document rotation policy** for compliance audits

3. **Test manual rotation triggers** quarterly

4. **Review rotation logs** monthly:
   ```bash
   kubectl logs -n dns-system -l app.kubernetes.io/name=bindy-operator --since=720h | grep "Rotating RNDC Secret"
   ```

5. **Update runbooks** with new troubleshooting steps

---

## Next Steps

- [RNDC Key Rotation User Guide](../guide/rndc-key-rotation.md)
- [Compliance Overview](../compliance/overview.md)
- [Troubleshooting Guide](troubleshooting.md)
- [Security Architecture](../security/architecture.md)
