# Zone Selection and Assignment

Bindy supports two methods for assigning DNS zones to Bind9 instances:

1. **Explicit Cluster References** - Zones explicitly specify which cluster's instances to use
2. **Label Selectors** - Zones select instances using `bind9InstancesFrom` label selectors

This flexibility allows both manual zone assignment and declarative, self-healing zone discovery.

> **Architecture Note**: In Bindy, **zones select instances** (not the other way around). A `DNSZone` resource declares which `Bind9Instance` resources should serve it via label selectors or cluster references.

---

## Quick Start

### Method 1: Cluster Reference (Simplest)

Target all instances in a cluster:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-zone
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-cluster  # All instances in this cluster serve the zone
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com
```

**Result**: All `Bind9Instance` resources with `spec.clusterRef: production-cluster` will serve this zone.

### Method 2: Label Selectors (Most Flexible)

Select instances by labels:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: us-west-zone
  namespace: dns-system
spec:
  zoneName: us-west.example.com
  bind9InstancesFrom:
    - selector:
        matchLabels:
          environment: production
          region: us-west-2
  soaRecord:
    primaryNs: ns1.us-west.example.com.
    adminEmail: admin.example.com
```

**Result**: Only instances with both labels `environment: production` AND `region: us-west-2` will serve this zone.

### Method 3: Combined (Most Control)

Use both cluster reference AND label selectors:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: hybrid-zone
  namespace: dns-system
spec:
  zoneName: hybrid.example.com
  clusterRef: production-cluster
  bind9InstancesFrom:
    - selector:
        matchLabels:
          feature: edge-caching
  soaRecord:
    primaryNs: ns1.hybrid.example.com.
    adminEmail: admin.example.com
```

**Result**: UNION of instances matching `clusterRef` OR `bind9InstancesFrom` will serve the zone.

---

## Explicit Cluster References

### Using clusterRef

Target all instances in a namespace-scoped `Bind9Cluster`:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: corporate-zone
  namespace: dns-system
spec:
  zoneName: corporate.example.com
  clusterRef: production-cluster  # Namespace-scoped cluster
  soaRecord:
    primaryNs: ns1.corporate.example.com.
    adminEmail: admin.example.com
```

**How it works:**
1. Controller finds all `Bind9Instance` resources with `spec.clusterRef: production-cluster` in the same namespace
2. Zone is synchronized to all matching instances
3. Status field `bind9InstancesCount` shows how many instances serve the zone

**Check instance count:**
```bash
kubectl get dnszone corporate-zone -n dns-system
```

Output:
```
NAME             ZONE                   PROVIDER   RECORDS   INSTANCES   TTL    READY
corporate-zone   corporate.example.com              12        3           3600   True
```

The `INSTANCES` column shows 3 instances are serving this zone.

### Using clusterProviderRef

Target instances via a cluster-scoped `ClusterBind9Provider`:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: global-zone
  namespace: dns-system
spec:
  zoneName: global.example.com
  clusterProviderRef: global-dns-provider  # Cluster-scoped provider
  soaRecord:
    primaryNs: ns1.global.example.com.
    adminEmail: admin.example.com
```

**When to use:**
- Centralized DNS management across multiple namespaces
- Global infrastructure zones
- Shared DNS services

---

## Label Selector Instance Selection

### How It Works

Instead of manually assigning zones to clusters, define label selectors that automatically discover and select instances:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: api-zone
  namespace: dns-system
spec:
  zoneName: api.example.com
  bind9InstancesFrom:
    - selector:
        matchLabels:
          environment: production
          team: platform
  soaRecord:
    primaryNs: ns1.api.example.com.
    adminEmail: admin.example.com
```

Any `Bind9Instance` resources with matching labels will be automatically selected:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: platform-dns-west
  namespace: dns-system
  labels:
    environment: production  # ✓ Matches zone selector
    team: platform           # ✓ Matches zone selector
    region: us-west-2
spec:
  clusterRef: production-cluster
  # ... instance config
```

### Label Selector Operators

Label selectors support Kubernetes standard operators:

#### matchLabels (AND Logic)

All labels must match:

```yaml
bind9InstancesFrom:
  - selector:
      matchLabels:
        environment: production  # Instance MUST have this label
        region: us-west-2        # AND this label
```

#### matchExpressions (Advanced Logic)

Use operators for complex selection:

```yaml
bind9InstancesFrom:
  - selector:
      matchLabels:
        environment: production
      matchExpressions:
        # Select instances in us-west-2 OR us-east-1
        - key: region
          operator: In
          values: [us-west-2, us-east-1]
        # Exclude instances marked for maintenance
        - key: maintenance
          operator: DoesNotExist
        # Only instances marked as critical
        - key: critical
          operator: Exists
```

**Supported operators:**
- `In` - Label value must be in the values list
- `NotIn` - Label value must NOT be in the values list
- `Exists` - Label key must exist (any value)
- `DoesNotExist` - Label key must NOT exist

### Multiple Instance Selectors

Define multiple `bind9InstancesFrom` selectors to discover different sets of instances:

```yaml
spec:
  zoneName: multi-region.example.com
  bind9InstancesFrom:
    # US West instances
    - selector:
        matchLabels:
          region: us-west-2
          role: primary

    # US East instances
    - selector:
        matchLabels:
          region: us-east-1
          role: secondary

    # Critical infrastructure instances (any region)
    - selector:
        matchLabels:
          critical: "true"
```

**Result**: Zone is served by instances matching ANY of the selectors (OR logic between selectors, AND logic within each selector).

---

## Checking Zone-Instance Assignments

### View Instances Serving a Zone

```bash
kubectl get dnszone example-zone -n dns-system -o jsonpath='{.status.bind9Instances}' | jq
```

Output:
```json
[
  {
    "apiVersion": "bindy.firestoned.io/v1beta1",
    "kind": "Bind9Instance",
    "name": "primary-dns-west",
    "namespace": "dns-system",
    "status": "Configured",
    "lastReconciledAt": "2026-01-08T17:00:00Z"
  },
  {
    "apiVersion": "bindy.firestoned.io/v1beta1",
    "kind": "Bind9Instance",
    "name": "secondary-dns-east",
    "namespace": "dns-system",
    "status": "Configured",
    "lastReconciledAt": "2026-01-08T17:01:30Z"
  }
]
```

### Quick Instance Count

```bash
kubectl get dnszone example-zone -n dns-system -o jsonpath='{.status.bind9InstancesCount}'
```

Output: `2`

### View Zones on an Instance

```bash
kubectl get bind9instance primary-dns-west -n dns-system -o jsonpath='{.status.zones}' | jq
```

### Quick Zone Count

```bash
kubectl get bind9instance primary-dns-west -n dns-system -o jsonpath='{.status.zonesCount}'
```

---

## Selection Priority and Behavior

### Combined clusterRef and bind9InstancesFrom

When BOTH are specified, instances are selected using **UNION** logic:

```yaml
spec:
  zoneName: hybrid.example.com
  clusterRef: production-cluster
  bind9InstancesFrom:
    - selector:
        matchLabels:
          feature: geo-routing
```

**Selected instances:**
1. All instances with `spec.clusterRef: production-cluster` (**OR**)
2. All instances with label `feature: geo-routing`

**Duplicates are removed** - an instance matching both conditions appears once.

### Dynamic Instance Discovery

Zone-instance assignments are **self-healing** and update automatically:

**When instances are added:**
1. New instance labeled with matching labels is created
2. Zone controller discovers the instance on next reconciliation (~60s)
3. Zone is automatically synchronized to the new instance
4. `bind9InstancesCount` increments

**When instances are deleted:**
1. Instance is removed from the cluster
2. Zone controller detects the deletion
3. Instance is removed from `status.bind9Instances`
4. `bind9InstancesCount` decrements

**When labels change:**
1. Instance labels are modified
2. If labels no longer match: zone is removed from that instance
3. If labels now match: zone is added to that instance
4. Changes reflected in zone status within ~60s

---

## Status Fields

### bind9InstancesCount

Shows how many instances are serving the zone:

```yaml
status:
  bind9InstancesCount: 3  # Zone is on 3 instances
  bind9Instances:
    - name: primary-west
      namespace: dns-system
      status: Configured
    - name: secondary-east
      namespace: dns-system
      status: Configured
    - name: edge-caching
      namespace: dns-system
      status: Configured
```

### Instance Status Values

Each instance in `bind9Instances` has a status:

- **`Claimed`**: Zone selected this instance (waiting for configuration)
- **`Configured`**: Zone successfully configured on instance
- **`Failed`**: Zone configuration failed on instance
- **`Unclaimed`**: Instance no longer selected by zone (cleanup pending)

### Checking Status

```bash
# View all instances and their status
kubectl get dnszone example-zone -n dns-system -o yaml | yq '.status.bind9Instances'

# Check for failed instances
kubectl get dnszone example-zone -n dns-system -o yaml | yq '.status.bind9Instances[] | select(.status == "Failed")'

# Count configured instances
kubectl get dnszone example-zone -n dns-system -o jsonpath='{.status.bind9Instances[?(@.status=="Configured")]}' | jq 'length'
```

---

## Examples

### Example 1: Regional Instance Selection

Deploy zones to instances in specific regions:

```yaml
# US West zone - only west instances
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: us-west-api
  namespace: dns-system
spec:
  zoneName: api.us-west.example.com
  bind9InstancesFrom:
    - selector:
        matchLabels:
          region: us-west-2
          environment: production
  soaRecord:
    primaryNs: ns1.us-west.example.com.
    adminEmail: admin.example.com
---
# US East zone - only east instances
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: us-east-api
  namespace: dns-system
spec:
  zoneName: api.us-east.example.com
  bind9InstancesFrom:
    - selector:
        matchLabels:
          region: us-east-1
          environment: production
  soaRecord:
    primaryNs: ns1.us-east.example.com.
    adminEmail: admin.example.com
```

**Bind9Instance with labels:**
```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-west-primary
  namespace: dns-system
  labels:
    region: us-west-2        # Will serve us-west-api zone
    environment: production
spec:
  clusterRef: production-cluster
  # ... config
---
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: dns-east-primary
  namespace: dns-system
  labels:
    region: us-east-1        # Will serve us-east-api zone
    environment: production
spec:
  clusterRef: production-cluster
  # ... config
```

### Example 2: Team-Based Zone Separation

Different teams manage their own zones on dedicated instances:

```yaml
# Platform team zone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: platform-services
  namespace: dns-system
spec:
  zoneName: platform.example.com
  bind9InstancesFrom:
    - selector:
        matchLabels:
          owner: platform-team
  soaRecord:
    primaryNs: ns1.platform.example.com.
    adminEmail: platform@example.com
---
# Application team zone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: app-services
  namespace: dns-system
spec:
  zoneName: app.example.com
  bind9InstancesFrom:
    - selector:
        matchLabels:
          owner: app-team
  soaRecord:
    primaryNs: ns1.app.example.com.
    adminEmail: appteam@example.com
```

### Example 3: Critical Infrastructure with Fallback

Critical zones with automatic fallback to backup instances:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: critical-api
  namespace: dns-system
spec:
  zoneName: api.example.com
  bind9InstancesFrom:
    # Primary: High-performance instances
    - selector:
        matchLabels:
          tier: critical
          performance: high

    # Fallback: Standard instances
    - selector:
        matchLabels:
          tier: critical
          performance: standard
  soaRecord:
    primaryNs: ns1.api.example.com.
    adminEmail: sre@example.com
```

**Result**: Zone is served by all instances labeled `tier: critical` regardless of performance tier.

### Example 4: Environment Isolation with Cluster Reference

Combine explicit cluster reference with label selectors:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: hybrid-deployment
  namespace: dns-system
spec:
  zoneName: hybrid.example.com
  clusterRef: production-cluster  # All production cluster instances
  bind9InstancesFrom:
    # PLUS any instance with edge-caching feature
    - selector:
        matchLabels:
          feature: edge-caching
  soaRecord:
    primaryNs: ns1.hybrid.example.com.
    adminEmail: ops@example.com
```

**Result**: Zone is served by:
1. All instances in `production-cluster`
2. UNION with any instance labeled `feature: edge-caching` (even if not in production-cluster)

---

## Troubleshooting

### Zone Not Selecting Any Instances

**Symptom:**
```bash
$ kubectl get dnszone my-zone -n dns-system
NAME      ZONE            RECORDS   INSTANCES   TTL    READY
my-zone   example.com     0         0           3600   False
```

**Diagnosis:**

1. **Check zone selectors:**
   ```bash
   kubectl get dnszone my-zone -n dns-system -o yaml | yq '.spec.bind9InstancesFrom'
   ```

2. **Check available instances:**
   ```bash
   kubectl get bind9instance -n dns-system --show-labels
   ```

3. **Test selector manually:**
   ```bash
   # If selector is matchLabels: {environment: production}
   kubectl get bind9instance -n dns-system -l environment=production
   ```

**Solutions:**
- Verify instance labels match zone selectors exactly (case-sensitive)
- Ensure instances are in the same namespace as the zone
- Check if `clusterRef` is specified and instances have matching `spec.clusterRef`
- Review controller logs for selection errors

### Instance Count Mismatch

**Symptom:**
```bash
$ kubectl get dnszone my-zone -o jsonpath='{.status.bind9InstancesCount}'
2
$ kubectl get dnszone my-zone -o jsonpath='{.status.bind9Instances}' | jq 'length'
3
```

**Diagnosis:**
This should never happen - indicates a controller bug. The `bind9InstancesCount` is automatically computed from `bind9Instances.len()`.

**Solution:**
- Trigger reconciliation by adding an annotation to the zone
- Check controller logs for errors
- File a bug report if the issue persists

### Instances Not Updating After Label Changes

**Symptom:**
Instance labels changed but zone not selecting/deselecting instances.

**Diagnosis:**
```bash
# Check when zone was last reconciled
kubectl get dnszone my-zone -n dns-system -o jsonpath='{.status.observedGeneration}'

# Trigger manual reconciliation
kubectl annotate dnszone my-zone -n dns-system reconcile-trigger="$(date +%s)" --overwrite
```

**Solution:**
- Wait up to 60 seconds for automatic reconciliation
- Trigger manual reconciliation with annotation
- Check controller logs for watch errors

### Zone Shows "Failed" Status on Some Instances

**Symptom:**
```yaml
status:
  bind9Instances:
    - name: primary-west
      status: Configured  # ✓ Good
    - name: secondary-east
      status: Failed      # ✗ Problem
      message: "Connection to bindcar API failed: connection refused"
```

**Diagnosis:**
Check instance and bindcar sidecar status:
```bash
# Check instance pods
kubectl get pods -n dns-system -l app.kubernetes.io/name=bind9-instance,app.kubernetes.io/instance=secondary-east

# Check bindcar sidecar logs
kubectl logs -n dns-system <instance-pod> -c bindcar
```

**Solutions:**
- Ensure bindcar sidecar is running
- Check network policies allow zone controller → bindcar communication
- Verify bindcar API port is correct (default: 8080)
- Review bindcar logs for configuration errors

---

## Best Practices

### 1. Use Consistent Labeling Schemes

Establish label conventions across your organization:

```yaml
# Standard label schema
metadata:
  labels:
    environment: production|staging|development
    region: us-west-2|us-east-1|eu-west-1
    tier: critical|standard|development
    owner: team-name
    cost-center: department-code
```

**Benefits:**
- Predictable zone assignment
- Easy troubleshooting
- Consistent operational patterns

### 2. Avoid Overlapping Selectors

Design selectors to be mutually exclusive when zones should not overlap:

```yaml
# ✓ GOOD - Non-overlapping selectors
# Production zone
bind9InstancesFrom:
  - selector:
      matchLabels:
        environment: production

# Development zone
bind9InstancesFrom:
  - selector:
      matchLabels:
        environment: development
```

```yaml
# ✗ BAD - Overlapping selectors
# Zone 1
bind9InstancesFrom:
  - selector:
      matchLabels:
        tier: critical

# Zone 2
bind9InstancesFrom:
  - selector:
      matchLabels:
        environment: production  # May overlap with tier: critical
```

**Solution**: Use `matchExpressions` with `NotIn` to exclude instances:

```yaml
bind9InstancesFrom:
  - selector:
      matchLabels:
        environment: production
      matchExpressions:
        - key: tier
          operator: NotIn
          values: [critical]  # Exclude critical tier instances
```

### 3. Use Explicit clusterRef for Critical Zones

Pin critical infrastructure zones to specific clusters:

```yaml
# Critical zone - explicit assignment
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: api-production
spec:
  zoneName: api.example.com
  clusterRef: critical-infrastructure  # Explicit - won't move
  soaRecord:
    primaryNs: ns1.api.example.com.
    adminEmail: sre@example.com
```

**When to use explicit refs:**
- Critical production zones
- Zones with specific performance/latency requirements
- Zones that must not auto-migrate
- Compliance/regulatory zones with fixed infrastructure

### 4. Monitor Zone Assignment Metrics

Set up monitoring for zone-instance assignments:

```promql
# Alert when zone has no instances
bindy_dnszone_instance_count{namespace="dns-system"} == 0

# Alert when instance count drops below expected
bindy_dnszone_instance_count{namespace="dns-system",zone_name="api-production"} < 3

# Alert when instance shows Failed status
bindy_dnszone_instance_status{status="Failed"} > 0
```

### 5. Test Label Changes in Non-Production First

Label changes affect zone assignment - test carefully:

```bash
# 1. Test in dev environment first
kubectl label bind9instance dev-instance -n dev-dns new-label=value

# 2. Verify zone assignment changed as expected
kubectl get dnszone -n dev-dns -o custom-columns=NAME:.metadata.name,INSTANCES:.status.bind9InstancesCount

# 3. If successful, apply to production
kubectl label bind9instance prod-instance -n dns-system new-label=value
```

### 6. Document Your Labeling Strategy

Maintain runbooks documenting:
- Required labels for zone selection
- Label value meanings and conventions
- Examples of common selector patterns
- Troubleshooting procedures for label-related issues

---

## Migration from Legacy Architecture

> **Note**: If you're migrating from Bindy versions prior to Phase 5-6 (pre-v0.2.0), the zone selection architecture was reversed.

### OLD Architecture (Deprecated)

Instances selected zones via `Bind9Instance.spec.zonesFrom`:

```yaml
# OLD - Instances selected zones (DEPRECATED)
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: my-instance
spec:
  zonesFrom:  # ← DEPRECATED FIELD
    - selector:
        matchLabels:
          environment: production
```

### NEW Architecture (Current)

Zones select instances via `DNSZone.spec.bind9InstancesFrom`:

```yaml
# NEW - Zones select instances (CURRENT)
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: my-zone
spec:
  bind9InstancesFrom:  # ← NEW FIELD
    - selector:
        matchLabels:
          environment: production
```

### Migration Steps

1. **Update CRDs** to latest version (v0.2.0+)
2. **Label your Bind9Instances** with selection criteria
3. **Add `bind9InstancesFrom`** to DNSZone resources
4. **Remove deprecated `zonesFrom`** from Bind9Instance resources (field is ignored)
5. **Verify selection**: Check `bind9InstancesCount` in zone status
6. **Monitor logs** for any selection errors

See [Migration Guide](../operations/migration-guide-phase5-6.md) for detailed instructions.
