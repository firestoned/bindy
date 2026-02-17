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
  ipv4Addresses:
    - "192.0.2.1"
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
  ipv4Addresses:
    - "192.0.2.1"
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
  ipv4Addresses:
    - "192.0.2.10"
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

## Migrating from nameServerIps to nameServers (v0.4.0)

**What Changed:** The `nameServerIps` field is deprecated in favor of the new `nameServers` field, which provides better structure and clarity.

### Why This Change?

**Problems with `nameServerIps`:**
1. **Misleading name:** Suggests only glue records, but actually defines authoritative nameservers
2. **Limited structure:** HashMap format doesn't support IPv6 addresses
3. **No IPv6 glue records:** Can't specify AAAA records for nameservers
4. **Unclear purpose:** Name doesn't convey that NS records are auto-generated

**Benefits of `nameServers`:**
1. **Clear intent:** Explicitly represents authoritative nameservers
2. **Structured data:** Separate fields for hostname, IPv4, and IPv6
3. **Dual-stack support:** Both IPv4 and IPv6 glue records
4. **Better documentation:** Field name matches DNS terminology

### Old Format (Deprecated)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-dns
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2025012101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  # OLD: nameServerIps (deprecated in v0.4.0)
  nameServerIps:
    ns2.example.com.: "192.0.2.2"
    ns3.example.com.: "192.0.2.3"
  ttl: 3600
```

### New Format (v0.4.0+)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-dns
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2025012101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  # NEW: nameServers (v0.4.0+)
  nameServers:
    - hostname: ns2.example.com.
      ipv4Address: "192.0.2.2"
    - hostname: ns3.example.com.
      ipv4Address: "192.0.2.3"
      ipv6Address: "2001:db8::3"  # Now supports IPv6!
  ttl: 3600
```

### Migration Steps

#### 1. Update CRDs to v0.4.0+

```bash
# Update CRDs from latest release
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml

# Or from source
make crds-combined
kubectl replace --force -f deploy/crds.yaml
```

#### 2. Identify Zones Using nameServerIps

```bash
# Find all DNSZones with nameServerIps field
kubectl get dnszones -A -o json | jq -r '.items[] | select(.spec.nameServerIps != null) | "\(.metadata.namespace)/\(.metadata.name)"'
```

#### 3. Convert Each Zone

For each zone found, update the spec:

**Manual Conversion:**

```bash
# Edit the zone
kubectl edit dnszone example-com -n dns-system
```

Then change the format from HashMap to array:

```yaml
# Remove old field
nameServerIps:
  ns2.example.com.: "192.0.2.2"
  ns3.example.com.: "192.0.2.3"

# Add new field
nameServers:
  - hostname: ns2.example.com.
    ipv4Address: "192.0.2.2"
  - hostname: ns3.example.com.
    ipv4Address: "192.0.2.3"
```

**Automated Conversion Script:**

```bash
#!/bin/bash
# migrate-nameserverips.sh

kubectl get dnszones -A -o json | jq -r '.items[] | select(.spec.nameServerIps != null) | "\(.metadata.namespace) \(.metadata.name)"' | while read ns name; do
  echo "Migrating DNSZone $ns/$name"

  # Get current nameServerIps as JSON
  old_ips=$(kubectl get dnszone "$name" -n "$ns" -o json | jq -c '.spec.nameServerIps // {}')

  # Convert to new nameServers format
  new_servers=$(echo "$old_ips" | jq -c 'to_entries | map({hostname: .key, ipv4Address: .value})')

  # Patch the DNSZone
  kubectl patch dnszone "$name" -n "$ns" --type=json -p "[
    {\"op\": \"add\", \"path\": \"/spec/nameServers\", \"value\": $new_servers},
    {\"op\": \"remove\", \"path\": \"/spec/nameServerIps\"}
  ]"

  echo "  ✓ Migrated $ns/$name"
done
```

#### 4. Add IPv6 Glue Records (Optional)

If you have dual-stack nameservers, add IPv6 addresses:

```yaml
nameServers:
  - hostname: ns2.example.com.
    ipv4Address: "192.0.2.2"
    ipv6Address: "2001:db8::2"  # Add IPv6 glue record
```

#### 5. Verify Migration

```bash
# Check that zones no longer use nameServerIps
kubectl get dnszones -A -o json | jq '.items[] | select(.spec.nameServerIps != null) | "\(.metadata.namespace)/\(.metadata.name)"'

# Should return nothing if migration is complete

# Verify nameServers field is present
kubectl get dnszone example-com -n dns-system -o jsonpath='{.spec.nameServers}'
```

#### 6. Check Operator Logs

After migration, ensure no deprecation warnings:

```bash
kubectl logs -n dns-system -l app=bindy-operator -f | grep nameServerIps
```

You should NOT see any deprecation warnings after migration.

### Backward Compatibility

**Good News:** Existing zones using `nameServerIps` will continue to work in v0.4.0+. The operator automatically converts the old format internally.

```rust
// In src/reconcilers/dnszone.rs
fn get_effective_name_servers(spec: &DNSZoneSpec) -> Option<Vec<NameServer>> {
    if let Some(ref new_servers) = spec.name_servers {
        // New field takes precedence
        return Some(new_servers.clone());
    }

    // Fallback to deprecated field with automatic conversion
    if let Some(ref old_ips) = spec.name_server_ips {
        warn!("DNSZone uses deprecated `nameServerIps` field. Migrate to `nameServers`.");
        let servers: Vec<NameServer> = old_ips
            .iter()
            .map(|(hostname, ip)| NameServer {
                hostname: hostname.clone(),
                ipv4_address: Some(ip.clone()),
                ipv6_address: None,
            })
            .collect();
        return Some(servers);
    }

    None
}
```

**Deprecation Timeline:**
- **v0.4.0**: `nameServerIps` deprecated, still functional with warnings
- **v1.0.0**: `nameServerIps` will be removed entirely

### Troubleshooting

#### Deprecation Warnings in Logs

**Symptom:** Operator logs show warnings about `nameServerIps`:

```
WARN DNSZone uses deprecated `nameServerIps` field. Migrate to `nameServers`.
```

**Solution:** Follow migration steps above to convert to `nameServers` format.

#### Zone Still Has Both Fields

**Symptom:** Zone has both `nameServers` and `nameServerIps` defined.

**Diagnosis:**

```bash
kubectl get dnszone example-com -n dns-system -o yaml | grep -A 10 "nameServer"
```

**Solution:** Remove `nameServerIps` field - the new `nameServers` takes precedence:

```bash
kubectl patch dnszone example-com -n dns-system --type=json -p '[{"op": "remove", "path": "/spec/nameServerIps"}]'
```

#### Missing IPv6 Glue Records

**Symptom:** Dual-stack nameservers only have A records, no AAAA records.

**Solution:** Add `ipv6Address` to nameServers entries:

```yaml
nameServers:
  - hostname: ns2.example.com.
    ipv4Address: "192.0.2.2"
    ipv6Address: "2001:db8::2"  # Add this
```

## See Also

- [DNSZone Concepts](../concepts/dnszone.md) - Detailed explanation of label selectors
- [DNSZone Specification](../reference/dnszone-spec.md) - Complete field reference
- [Multi-Nameserver Example](../../../examples/dns-zone-multi-nameserver.yaml) - Working example
- [Architecture Overview](../guide/architecture.md) - Event-driven reconciliation
- [API Reference](../reference/api.md) - Complete CRD schemas
- [GitHub Release Notes](https://github.com/firestoned/bindy/releases/tag/v0.3.0) - Full changelog

## Support

For issues or questions:
- [GitHub Issues](https://github.com/firestoned/bindy/issues)
- [Documentation](https://firestoned.github.io/bindy/)
