# Secondary DNS Instances

Secondary DNS instances receive zone data from primary servers via zone transfers (AXFR/IXFR). They provide redundancy and load distribution for DNS queries.

## Creating a Secondary Instance

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-dns
  namespace: dns-system
  labels:
    dns-role: secondary
    environment: production
spec:
  replicas: 1
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
```

Apply with:

```bash
kubectl apply -f secondary-instance.yaml
```

## Key Differences from Primary

### No Zone Transfers Allowed

Secondary servers typically don't allow zone transfers:

```yaml
spec:
  config:
    allowTransfer: []  # Empty or omitted - no transfers from secondary
```

### Read-Only Zones

Secondaries receive zone data from primaries and cannot be updated directly. All zone modifications must be made on the primary server.

### Label for Selection

Use the `role: secondary` label to enable automatic zone transfer configuration:

```yaml
metadata:
  labels:
    role: secondary      # Required for automatic discovery
    cluster: production  # Required - must match cluster name
```

> **Important:** The `role: secondary` label is **required** for Bindy to automatically discover secondary instances and configure zone transfers on primary zones.

## Configuring Secondary Zones

When creating a DNSZone resource for secondary zones, use the `secondary` type and specify primary servers:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com-secondary
  namespace: dns-system
spec:
  zoneName: example.com
  type: secondary
  instanceSelector:
    matchLabels:
      dns-role: secondary
  secondaryConfig:
    primaryServers:
      - "10.0.1.10"  # IP of primary DNS server
      - "10.0.1.11"  # Additional primary for redundancy
```

## Automatic Zone Transfer Configuration

> **New in v0.1.0:** Bindy automatically configures zone transfers from primaries to secondaries!

When you create primary `DNSZone` resources, Bindy automatically:
1. **Discovers secondary instances** using the `role=secondary` label
2. **Configures zone transfers** on primary zones with `also-notify` and `allow-transfer`
3. **Tracks secondary IPs** in `DNSZone.status.secondaryIps`
4. **Detects IP changes** when secondary pods restart or are rescheduled
5. **Auto-updates zones** when secondary IPs change (within 5-10 minutes)

**Example:**
```bash
# Create secondary instance with proper labels
cat <<EOF | kubectl apply -f -
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: secondary-dns
  namespace: dns-system
  labels:
    role: secondary          # Required for discovery
    cluster: production      # Must match cluster name
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
EOF

# Create primary zone - zone transfers auto-configured!
cat <<EOF | kubectl apply -f -
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production  # Matches cluster label
  # ... other config ...
EOF

# Verify automatic configuration
kubectl get dnszone example-com -n dns-system -o jsonpath='{.status.secondaryIps}'
# Output: ["10.244.1.5","10.244.2.8"]
```

**Self-Healing:**
When secondary pods restart and get new IPs:
- Bindy detects the change within one reconciliation cycle (~5-10 minutes)
- Primary zones are automatically updated with new secondary IPs
- Zone transfers resume automatically with no manual intervention

## Verifying Zone Transfers

Check that zones are being transferred:

```bash
# Check zone files on secondary
kubectl exec -n dns-system deployment/secondary-dns -- ls -la /var/lib/bind/zones/

# Check BIND9 logs for transfer messages
kubectl logs -n dns-system -l instance=secondary-dns | grep "transfer of"

# Verify secondary IPs are configured on primary zones
kubectl get dnszone -n dns-system -o yaml | yq '.items[].status.secondaryIps'
```

## Best Practices

### Use Multiple Secondaries

Deploy secondary instances in different locations:

```yaml
# Secondary in different AZ/region
metadata:
  labels:
    dns-role: secondary
    region: us-west-1
```

### Configure NOTIFY

Primary servers send NOTIFY messages to secondaries when zones change. Ensure network connectivity allows these notifications.

### Monitor Transfer Status

Watch for failed transfers in logs:

```bash
kubectl logs -n dns-system -l instance=secondary-dns --tail=100 | grep -i transfer
```

## Network Requirements

Secondaries must be able to:

1. Receive zone transfers from primaries (TCP port 53)
2. Receive NOTIFY messages from primaries (UDP port 53)
3. Respond to DNS queries from clients (UDP/TCP port 53)

Ensure Kubernetes network policies and firewall rules allow this traffic.

## Next Steps

- [Configure Multi-Region Setup](./multi-region.md) with geographically distributed secondaries
- [Create Secondary Zones](./creating-zones.md) that transfer from primaries
- [Monitor DNS Infrastructure](../operations/monitoring.md)
