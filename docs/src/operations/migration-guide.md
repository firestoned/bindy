# Migration Guide: v0.2.x → v0.3.x

This document explains how to migrate from Bindy v0.2.x to v0.3.x, which introduces breaking changes to how DNS records are associated with zones.

## Breaking Changes in v0.3.0

### 1. Records Now Use Label Selectors (Breaking Change)

**What Changed:** The mechanism for associating DNS records with zones has changed from explicit references to label-based selection.

#### Before (v0.2.x): Explicit `zoneRef` References

```yaml
# DNSZone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: my-dns

---
# Record with explicit zone reference
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
  namespace: dns-system
spec:
  zoneRef: example-com  # ❌ This field no longer exists!
  name: www
  ipv4Address: "192.0.2.1"
```

#### After (v0.3.0+): Label-Based Selection

```yaml
# DNSZone selects records via labels
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: my-dns
  recordsFrom:  # ✅ New: Label selectors
    - selector:
        matchLabels:
          zone: example.com

---
# Record with matching labels
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
  namespace: dns-system
  labels:  # ✅ New: Labels for selection
    zone: example.com
spec:
  # NO zoneRef field - selection is via labels
  name: www
  ipv4Address: "192.0.2.1"
```

### Why This Change?

**Problems with v0.2.x explicit references:**
1. **Tight coupling:** Records hardcoded the zone name
2. **Limited flexibility:** Couldn't dynamically group records
3. **No multi-environment support:** Records couldn't belong to multiple zones
4. **Manual management:** Had to update every record when changing zones

**Benefits of v0.3.0 label selectors:**
1. **Decoupled:** Zones select records, not vice versa
2. **Flexible:** Use any label combination for selection
3. **Dynamic:** New records automatically picked up by matching zones
4. **Multi-tenant:** Isolate records by team, environment, application
5. **Kubernetes-native:** Uses standard label selector pattern

## Migration Steps

### Step 1: Backup Your Configuration

```bash
# Backup all DNS resources
kubectl get bind9clusters,dnszones -A -o yaml > clusters-zones-backup.yaml
kubectl get arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords -A -o yaml > records-backup.yaml
```

### Step 2: Update CRDs

```bash
# Update to v0.3.0 CRDs
kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/crds.yaml
```

Or if installing from source:

```bash
kubectl replace --force -f deploy/crds/
```

**IMPORTANT:** Use `kubectl replace --force` instead of `kubectl apply` to avoid the 256KB annotation size limit.

### Step 3: Update DNSZone Resources

Add `recordsFrom` selectors to all DNSZone resources:

```bash
# For each DNSZone, add recordsFrom selector
kubectl edit dnszone example-com -n dns-system
```

Add this to the spec:

```yaml
spec:
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com  # Use the zone name as the label
```

**Automation Script:**

```bash
#!/bin/bash
# auto-migrate-zones.sh

# Get all DNSZones
kubectl get dnszones -A -o json | jq -r '.items[] | "\(.metadata.namespace) \(.metadata.name) \(.spec.zoneName)"' | while read ns name zonename; do
  echo "Migrating DNSZone $ns/$name (zone: $zonename)"

  # Patch the DNSZone with recordsFrom selector
  kubectl patch dnszone "$name" -n "$ns" --type=merge -p "{
    \"spec\": {
      \"recordsFrom\": [
        {
          \"selector\": {
            \"matchLabels\": {
              \"zone\": \"$zonename\"
            }
          }
        }
      ]
    }
  }"
done
```

### Step 4: Update DNS Record Resources

Add labels to all DNS records matching the zone they belong to:

```bash
# For each record, add the zone label
kubectl label arecord www -n dns-system zone=example.com
```

**Automation Script:**

```bash
#!/bin/bash
# auto-migrate-records.sh

RECORD_TYPES="arecords aaaarecords cnamerecords mxrecords txtrecords nsrecords srvrecords caarecords"

for record_type in $RECORD_TYPES; do
  echo "Migrating $record_type..."

  # Get all records and their old zoneRef
  kubectl get $record_type -A -o json | jq -r '.items[] | select(.spec.zoneRef != null) | "\(.metadata.namespace) \(.metadata.name) \(.spec.zoneRef)"' | while read ns name zoneref; do
    # Get the zone's zoneName
    zonename=$(kubectl get dnszone "$zoneref" -n "$ns" -o jsonpath='{.spec.zoneName}' 2>/dev/null)

    if [ -n "$zonename" ]; then
      echo "  Labeling $ns/$name with zone=$zonename"
      kubectl label $record_type "$name" -n "$ns" "zone=$zonename" --overwrite
    else
      echo "  WARNING: Could not find DNSZone $zoneref in namespace $ns"
    fi
  done
done
```

### Step 5: Remove Old `zoneRef` Fields

The `spec.zoneRef` field no longer exists in v0.3.0 CRDs. After migration, you can optionally clean up your YAML files by removing these fields (they're already ignored by the new CRD).

### Step 6: Upgrade the Operator

```bash
# Update the Bindy operator to v0.3.0
kubectl set image deployment/bindy bindy=ghcr.io/firestoned/bindy:v0.3.0 -n dns-system

# Or apply the new deployment
kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.3.0/bindy.yaml
```

### Step 7: Verify Migration

```bash
# Check DNSZone status - should show selected records
kubectl get dnszones -A -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{.status.recordsCount}{"\n"}{end}'

# Check that records have zone labels
kubectl get arecords,cnamerecords,mxrecords -A --show-labels

# Verify DNS resolution still works
kubectl port-forward -n dns-system svc/<bind9-service> 5353:53
dig @localhost -p 5353 www.example.com
```

## Advanced Migration Patterns

### Multi-Environment Records

Use labels to support records in multiple environments:

```yaml
# DNSZone for dev
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-dev
spec:
  zoneName: dev.example.com
  recordsFrom:
    - selector:
        matchLabels:
          app: myapp
          environment: dev

---
# DNSZone for prod
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-prod
spec:
  zoneName: prod.example.com
  recordsFrom:
    - selector:
        matchLabels:
          app: myapp
          environment: prod

---
# Record selected by dev zone
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: myapp-dev
  labels:
    app: myapp
    environment: dev
spec:
  name: api
  ipv4Address: "192.0.2.10"
```

### Team-Based Isolation

Use team labels for multi-tenancy:

```yaml
# Team A's zone
spec:
  recordsFrom:
    - selector:
        matchLabels:
          team: team-a

# Team A's record
metadata:
  labels:
    team: team-a
```

## Troubleshooting

### Records Not Appearing in Zone

**Symptom:** Records exist but don't show up in DNS queries.

**Diagnosis:**

```bash
# Check if zone selected the records
kubectl get dnszone example-com -n dns-system -o jsonpath='{.status.recordsCount}'

# Check if records have the right labels
kubectl get arecords -n dns-system --show-labels
```

**Solution:**
- Ensure record labels match the zone's `recordsFrom` selector
- Check that records are in the same namespace as the zone

### Zone Shows Zero Records

**Symptom:** `status.recordsCount` is 0 or missing.

**Diagnosis:**

```bash
# Check the zone's selector
kubectl get dnszone example-com -n dns-system -o yaml | grep -A 5 recordsFrom
```

**Solution:**
- Add `recordsFrom` selector to the DNSZone spec
- Ensure the selector matches at least one record's labels

### Old `zoneRef` Field Error

**Symptom:** `kubectl apply` fails with "unknown field spec.zoneRef"

**Solution:**
- Update the CRDs to v0.3.0: `kubectl replace --force -f deploy/crds/`
- Remove `spec.zoneRef` from your YAML files (field no longer exists)

## Rollback Procedure

If you need to rollback to v0.2.x:

1. **Restore old CRDs:**
   ```bash
   kubectl apply -f https://github.com/firestoned/bindy/releases/download/v0.2.x/crds.yaml
   ```

2. **Restore backups:**
   ```bash
   kubectl apply -f clusters-zones-backup.yaml
   kubectl apply -f records-backup.yaml
   ```

3. **Downgrade operator:**
   ```bash
   kubectl set image deployment/bindy bindy=ghcr.io/firestoned/bindy:v0.2.x -n dns-system
   ```

## See Also

- [DNSZone Concepts](../concepts/dnszone.md) - Detailed explanation of label selectors
- [Architecture Overview](../guide/architecture.md) - Event-driven reconciliation
- [API Reference](../reference/api.md) - Complete CRD schemas
- [GitHub Release Notes](https://github.com/firestoned/bindy/releases/tag/v0.3.0) - Full changelog

## Support

For issues or questions:
- [GitHub Issues](https://github.com/firestoned/bindy/issues)
- [Documentation](https://firestoned.github.io/bindy/)
