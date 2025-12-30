# Zone Selection and Assignment

Bindy supports two methods for assigning DNS zones to Bind9 instances:

1. **Explicit References** - Zones explicitly specify which cluster to use
2. **Label Selectors** - Clusters/instances automatically discover zones based on labels

This flexibility allows both manual zone assignment and declarative, self-healing zone discovery.

---

## Explicit Zone Assignment

### Using clusterRef

Zones can explicitly reference a namespace-scoped `Bind9Cluster`:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-zone
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-cluster  # Explicit reference
  soaRecord:
    primaryServer: ns1.example.com
    adminEmail: admin.example.com
```

**When to use:**
- You want manual control over which cluster serves a zone
- You have specific zones that must run on dedicated infrastructure
- You need to override automatic zone discovery

### Using clusterProviderRef

Zones can also reference a cluster-scoped `ClusterBind9Provider`:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: global-zone
  namespace: dns-system
spec:
  zoneName: global.example.com
  clusterProviderRef: global-dns-provider  # Cluster-scoped reference
  soaRecord:
    primaryServer: ns1.global.example.com
    adminEmail: admin.example.com
```

**When to use:**
- You have a cluster-scoped DNS provider serving multiple namespaces
- You need centralized DNS management across namespaces

---

## Label Selector Zone Discovery

### How It Works

Instead of explicitly assigning zones to clusters, you can define label selectors at the cluster/instance level that automatically discover and select zones:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: production-cluster
  namespace: dns-system
spec:
  common:
    # Automatically discover zones matching these labels
    zonesFrom:
      - selector:
          matchLabels:
            environment: production
            team: platform
```

Any `DNSZone` resources with matching labels will be automatically selected:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: api-zone
  namespace: dns-system
  labels:
    environment: production
    team: platform
spec:
  zoneName: api.example.com
  # No clusterRef needed - automatically discovered!
  soaRecord:
    primaryServer: ns1.api.example.com
    adminEmail: admin.example.com
```

### Label Selector Operators

Label selectors support Kubernetes standard operators:

```yaml
zonesFrom:
  - selector:
      # AND logic - all labels must match
      matchLabels:
        environment: production

      # OR logic with operators
      matchExpressions:
        - key: team
          operator: In
          values: [platform, infrastructure]
        - key: critical
          operator: Exists
```

**Supported operators:**
- `In` - Label value must be in the list
- `NotIn` - Label value must NOT be in the list
- `Exists` - Label key must exist (any value)
- `DoesNotExist` - Label key must NOT exist

### Multiple Zone Selectors

You can define multiple `zonesFrom` selectors to discover different sets of zones:

```yaml
spec:
  common:
    zonesFrom:
      # Production zones
      - selector:
          matchLabels:
            environment: production

      # Critical infrastructure zones
      - selector:
          matchLabels:
            critical: "true"

      # Zones managed by specific teams
      - selector:
          matchExpressions:
            - key: team
              operator: In
              values: [platform, sre, devops]
```

---

## Selection Priority and Conflicts

### Explicit References Take Precedence

If a zone has both labels matching a selector AND an explicit `clusterRef`/`clusterProviderRef`, the explicit reference wins:

```yaml
# This zone will use my-specific-cluster, NOT the label selector
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: special-zone
  labels:
    environment: production  # Matches selector
spec:
  zoneName: special.example.com
  clusterRef: my-specific-cluster  # Takes precedence
```

### Multi-Instance Conflict Prevention

A zone can only be selected by ONE instance at a time. If multiple instances have label selectors that match the same zone, only the first one to reconcile will select it.

This prevents:
- Duplicate zone configuration
- Split-brain scenarios
- Conflicting zone data

**Best practices:**
- Use distinct label selectors for different instances
- Use `matchExpressions` with `NotIn` to exclude zones
- Monitor zone status to ensure proper assignment

---

## Propagation Hierarchy

Label selectors defined at higher levels automatically propagate down:

```
ClusterBind9Provider.spec.common.zonesFrom
    ↓ (propagates to all child clusters)
Bind9Cluster.spec.common.zonesFrom
    ↓ (propagates to all child instances)
Bind9Instance.spec.zonesFrom
    ↓ (performs actual zone discovery)
```

**Example:**

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: global-provider
spec:
  common:
    zonesFrom:
      - selector:
          matchLabels:
            scope: global
    # This selector will propagate to ALL clusters and instances
---
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: regional-cluster
  namespace: us-east-1
spec:
  clusterProviderRef: global-provider
  common:
    # Inherits global selector + adds regional selector
    zonesFrom:
      - selector:
          matchLabels:
            region: us-east-1
```

---

## Zone Status Tracking

### Selection Method

Zones report how they were selected in their status:

```yaml
status:
  selectionMethod: "labelSelector"  # or "explicit"
  selectedByInstance: "production-primary-instance"
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSucceeded
```

**Status fields:**
- `selectionMethod`: How the zone was assigned
  - `"explicit"` - Via `clusterRef` or `clusterProviderRef`
  - `"labelSelector"` - Via `zonesFrom` label matching
- `selectedByInstance`: Name of the instance that selected the zone (label selector only)

### Monitoring Zone Assignment

Check which zones are selected by an instance:

```bash
kubectl get bind9instance production-primary -n dns-system -o jsonpath='{.status.selectedZones}'
```

Check how a zone was selected:

```bash
kubectl get dnszone example-com -n dns-system -o jsonpath='{.status.selectionMethod}'
```

---

## Self-Healing Behavior

Label selector-based zone assignment is **self-healing**:

### Adding Zones

When you create a new zone with matching labels, instances automatically discover and select it on their next reconciliation (typically within 30-60 seconds).

### Updating Labels

If you change a zone's labels:
- Instances that no longer match will **untag** the zone
- Instances that now match will **tag** and select the zone
- Zone automatically moves between instances

### Deleting Instances

If an instance is deleted:
- Its `selected-by-instance` annotation is removed from zones
- Zones become available for other instances to select
- Another instance with matching `zonesFrom` selector will pick them up

---

## Examples

### Example 1: Environment-Based Selection

Create zones and let clusters discover them by environment:

```yaml
# Production cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: prod-cluster
spec:
  common:
    zonesFrom:
      - selector:
          matchLabels:
            environment: production
---
# Development cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: dev-cluster
spec:
  common:
    zonesFrom:
      - selector:
          matchLabels:
            environment: development
---
# Production zone - auto-selected by prod-cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: prod-api
  labels:
    environment: production
    service: api
spec:
  zoneName: api.example.com
---
# Development zone - auto-selected by dev-cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: dev-api
  labels:
    environment: development
    service: api
spec:
  zoneName: api.dev.example.com
```

### Example 2: Team-Based Separation

Different teams manage their own zones:

```yaml
# Platform team cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: platform-dns
spec:
  common:
    zonesFrom:
      - selector:
          matchLabels:
            owner: platform-team
---
# Application team cluster
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: app-dns
spec:
  common:
    zonesFrom:
      - selector:
          matchLabels:
            owner: app-team
---
# Platform infrastructure zone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: infrastructure
  labels:
    owner: platform-team
    category: infrastructure
spec:
  zoneName: infra.example.com
---
# Application service zone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: user-api
  labels:
    owner: app-team
    category: application
spec:
  zoneName: users.example.com
```

### Example 3: Mixed Explicit and Label Selector

Some zones use explicit references, others use label selectors:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: general-cluster
spec:
  common:
    zonesFrom:
      - selector:
          matchLabels:
            auto-assign: "true"
---
# Auto-assigned zone (uses label selector)
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: auto-zone
  labels:
    auto-assign: "true"
spec:
  zoneName: auto.example.com
---
# Explicitly assigned zone (overrides selector)
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: specific-zone
  labels:
    auto-assign: "true"  # Matches selector, but explicit ref wins
spec:
  zoneName: specific.example.com
  clusterRef: dedicated-cluster  # Explicit assignment
```

---

## Troubleshooting

### Zone Not Being Selected

**Check zone labels:**
```bash
kubectl get dnszone my-zone -o yaml | grep -A5 "labels:"
```

**Check instance selectors:**
```bash
kubectl get bind9instance my-instance -o jsonpath='{.spec.zonesFrom}'
```

**Check zone status:**
```bash
kubectl get dnszone my-zone -o jsonpath='{.status.selectionMethod}'
```

### Zone Selected by Wrong Instance

**Check for conflicts:**
- Verify the zone doesn't have an explicit `clusterRef` that takes precedence
- Check if multiple instances have overlapping `zonesFrom` selectors
- Review instance reconciliation logs for conflict messages

### Zone Assignment Changed Unexpectedly

**Check label changes:**
```bash
kubectl get dnszone my-zone -o yaml | grep -A10 "labels:"
```

Zone labels may have been modified, causing different instances to match.

**Check instance changes:**
Instances may have been created/deleted/updated with new `zonesFrom` selectors.

---

## Best Practices

1. **Use consistent labeling schemes**
   - Establish label conventions across your organization
   - Document required labels for zone assignment
   - Use label validation admission webhooks if needed

2. **Avoid overlapping selectors**
   - Design `zonesFrom` selectors to be mutually exclusive
   - Use `matchExpressions` with `NotIn` to exclude zones
   - Monitor instance status to detect conflicts

3. **Use explicit refs for critical zones**
   - Pin critical infrastructure zones to specific clusters
   - Use label selectors for application zones that can move

4. **Monitor zone assignments**
   - Check `status.selectionMethod` and `status.selectedByInstance` regularly
   - Alert on zones with no cluster assignment
   - Track zone movement between instances

5. **Test label changes carefully**
   - Changing labels affects zone assignment
   - Test in non-production first
   - Use canary deployments for label changes

6. **Document your labeling strategy**
   - Maintain documentation of label meanings
   - Include examples in runbooks
   - Train teams on proper label usage
