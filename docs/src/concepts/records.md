# DNS Records

Bindy supports all common DNS record types as Custom Resources.

## Supported Record Types

- **ARecord** - IPv4 address mapping
- **AAAARecord** - IPv6 address mapping
- **CNAMERecord** - Canonical name (alias)
- **MXRecord** - Mail exchange
- **TXTRecord** - Text data
- **NSRecord** - Nameserver delegation
- **SRVRecord** - Service location
- **CAARecord** - Certificate authority authorization

## Common Fields

All DNS record types share these fields:

```yaml
metadata:
  name: record-name
  namespace: dns-system
  labels:
    zone: <zone-name>  # Used by DNSZone selector
spec:
  name: record-name    # DNS name (@ for zone apex)
  ttl: 300             # Time to live (optional)
```

### Zone Association via Label Selectors

DNS records are associated with zones using **label selectors**, similar to how Kubernetes Services select Pods.

The DNSZone resource defines selectors in the `recordsFrom` field:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: primary-dns
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com  # Selects all records with this label
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
```

Records with matching labels are automatically included in the zone:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    zone: example.com  # ✅ Matches the DNSZone selector
spec:
  name: www
  ipv4Address: "192.0.2.1"
```

**Benefits of Label Selectors:**
- **Flexible**: Use any label structure
- **Dynamic**: Adding/removing labels updates zones automatically
- **Multi-zone**: Records can belong to multiple zones
- **Kubernetes-native**: Familiar pattern for Kubernetes users

See [Label Selector Guide](../guide/label-selectors.md) for advanced patterns.

## Event-Driven Record Reconciliation

All 8 record types use an **event-driven architecture** for immediate reconciliation:

### How It Works

1. **DNSZone watches all record types**
   - When you create a record, the DNSZone controller receives a watch event immediately (⚡ sub-second)
   - DNSZone evaluates if the record's labels match any `recordsFrom` selectors

2. **DNSZone sets `status.zoneRef`**
   - If labels match, DNSZone sets the record's `status.zoneRef` with full zone metadata:
     ```yaml
     status:
       zoneRef:
         apiVersion: bindy.firestoned.io/v1beta1
         kind: DNSZone
         name: example-com
         namespace: dns-system
         zoneName: example.com
     ```

3. **Record controllers watch status changes**
   - Record controllers watch for **status changes** (not just spec changes)
   - When `status.zoneRef` is set, the record controller receives a watch event immediately (⚡ sub-second)

4. **Record reconciles to BIND9**
   - Record controller reads `status.zoneRef` to find its parent zone
   - Adds the record to BIND9 primaries via dynamic DNS (nsupdate)
   - Updates `status.conditions` to RecordAvailable

### Performance

**Event-driven (current):**
```
Create record (10:00:00.000)
  → DNSZone watch triggered (10:00:00.050) ⚡
  → status.zoneRef set (10:00:00.100)
  → Record watch triggered (10:00:00.150) ⚡
  → BIND9 updated (10:00:00.500)
Total: 500ms ✅
```

**Old polling approach:**
```
Create record (10:00:00.000)
  → Wait for DNSZone reconcile (10:05:00.000) ⏳
  → Wait for record reconcile (10:05:30.000) ⏳
Total: 5 minutes 30 seconds ❌
```

### Record Status Fields

All records have a `status.zoneRef` field showing which zone selected them:

```yaml
status:
  # NEW: Structured zone reference (set by DNSZone controller)
  zoneRef:
    apiVersion: bindy.firestoned.io/v1beta1
    kind: DNSZone
    name: example-com
    namespace: dns-system
    zoneName: example.com

  # DEPRECATED: String-based zone (kept for backward compatibility)
  zone: example.com

  conditions:
    - type: Ready
      status: "True"
      reason: RecordAvailable
      message: "A record www successfully added to zone example.com"
  observedGeneration: 1
```

**Checking if a record is selected:**
```bash
# Check if record has been selected by a zone
kubectl get arecord www-example -o jsonpath='{.status.zoneRef}'

# Check which zone selected this record
kubectl get arecord www-example -o jsonpath='{.status.zoneRef.name}'
```

## ARecord (IPv4)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

[Learn more about A Records](../guide/a-records.md)

## AAAARecord (IPv6)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-example-ipv6
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv6Address: "2001:db8::1"
  ttl: 300
```

[Learn more about AAAA Records](../guide/aaaa-records.md)

## CNAMERecord (Alias)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: blog-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: blog
  target: www.example.com.
  ttl: 300
```

[Learn more about CNAME Records](../guide/cname-records.md)

## MXRecord (Mail Exchange)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mail-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  priority: 10
  mailServer: mail.example.com.
  ttl: 3600
```

[Learn more about MX Records](../guide/mx-records.md)

## TXTRecord (Text)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: spf-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  text:
    - "v=spf1 include:_spf.example.com ~all"
  ttl: 3600
```

[Learn more about TXT Records](../guide/txt-records.md)

## NSRecord (Nameserver)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: delegate-subdomain
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: subdomain
  nameserver: ns1.subdomain.example.com.
  ttl: 3600
```

[Learn more about NS Records](../guide/ns-records.md)

## SRVRecord (Service)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: SRVRecord
metadata:
  name: sip-service
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: _sip._tcp
  priority: 10
  weight: 60
  port: 5060
  target: sipserver.example.com.
  ttl: 3600
```

[Learn more about SRV Records](../guide/srv-records.md)

## CAARecord (Certificate Authority)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: letsencrypt-caa
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org
  ttl: 3600
```

[Learn more about CAA Records](../guide/caa-records.md)

## Record Status

All DNS record types use granular status conditions to provide real-time visibility into the record configuration process.

### Status During Configuration

```yaml
status:
  conditions:
    - type: Progressing
      status: "True"
      reason: RecordReconciling
      message: "Configuring A record on zone endpoints"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 1
```

### Status After Successful Configuration

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSucceeded
      message: "Record configured on 3 endpoint(s)"
      lastTransitionTime: "2024-11-26T10:00:01Z"
  observedGeneration: 1
```

### Status After Failure

```yaml
status:
  conditions:
    - type: Degraded
      status: "True"
      reason: RecordFailed
      message: "Failed to configure record: Zone not found on primary servers"
      lastTransitionTime: "2024-11-26T10:00:01Z"
  observedGeneration: 1
```

### Condition Types

All DNS record types use the following condition types:

- **Progressing** - Record is being configured
  - `RecordReconciling`: Before adding record to zone endpoints

- **Ready** - Record successfully configured
  - `ReconcileSucceeded`: Record configured on all endpoints (message includes endpoint count)

- **Degraded** - Configuration failure
  - `RecordFailed`: Failed to configure record (includes error details)

### Benefits

1. **Real-time progress** - See when records are being configured
2. **Better debugging** - Know immediately if/why a record failed
3. **Accurate reporting** - Status shows exact number of endpoints configured
4. **Consistent across types** - All 8 record types use the same status pattern

## Record Management

### Zone Apex Records

Use `@` for zone apex records:

```yaml
spec:
  name: "@"  # Represents the zone itself
```

### Subdomain Records

Use the subdomain name:

```yaml
spec:
  name: www        # www.example.com
  name: api.v2     # api.v2.example.com
```

## Next Steps

- [Managing DNS Records](../guide/records-guide.md) - Complete record management guide
- [A Records](../guide/a-records.md) - IPv4 address records
- [CNAME Records](../guide/cname-records.md) - Alias records
- [MX Records](../guide/mx-records.md) - Mail server records
- [Label Selector Guide](../guide/label-selectors.md) - Advanced selector patterns
