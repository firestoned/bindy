# Managing DNS Records

DNS records are the actual data in your zones - IP addresses, mail servers, text data, etc.

## Record Types

Bindy supports all common DNS record types:

- **A Records** - IPv4 addresses
- **AAAA Records** - IPv6 addresses
- **CNAME Records** - Canonical name (alias)
- **MX Records** - Mail exchange servers
- **TXT Records** - Text data (SPF, DKIM, DMARC, verification)
- **NS Records** - Nameserver delegation
- **SRV Records** - Service location
- **CAA Records** - Certificate authority authorization

## Record Structure

All records share common fields:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: <RecordType>
metadata:
  name: <unique-name>
  namespace: dns-system
spec:
  # Zone reference - use ONE of these:
  zone: <zone-name>            # Match against DNSZone spec.zoneName
  # OR
  zoneRef: <zone-resource-name> # Direct reference to DNSZone metadata.name

  name: <record-name>          # Name within the zone
  ttl: <optional-ttl>          # Override zone default TTL
  # ... record-specific fields
```

## Referencing DNS Zones

DNS records must reference an existing DNSZone. There are **two ways** to reference a zone:

### Method 1: Using `zone` Field (Zone Name Lookup)

The `zone` field searches for a DNSZone by matching its `spec.zoneName`:

```yaml
spec:
  zone: example.com  # Matches DNSZone with spec.zoneName: example.com
  name: www
```

**How it works:**
- The controller lists all DNSZones in the namespace
- Searches for one with `spec.zoneName` matching the provided value
- More intuitive - you specify the actual DNS zone name

**When to use:**
- Quick testing and development
- When you're not sure of the resource name
- When readability is more important than performance

### Method 2: Using `zoneRef` Field (Direct Reference)

The `zoneRef` field directly references a DNSZone by its Kubernetes resource name:

```yaml
spec:
  zoneRef: example-com  # Matches DNSZone with metadata.name: example-com
  name: www
```

**How it works:**
- The controller directly retrieves the DNSZone by `metadata.name`
- No search required - single API call
- More efficient

**When to use:**
- **Production environments** (recommended)
- Large namespaces with many zones
- When performance matters
- Infrastructure-as-code with known resource names

### Choosing Between `zone` and `zoneRef`

| Criteria | `zone` | `zoneRef` |
|----------|--------|-----------|
| Performance | Slower (list + search) | Faster (direct get) |
| Readability | More intuitive | Less obvious |
| Use Case | Development/testing | Production |
| API Calls | Multiple | Single |
| Best For | Humans writing YAML | Automation/templates |

**Important:** You must specify **exactly one** of `zone` or `zoneRef` - not both, not neither.

### Example: Same Record, Two Methods

Given this DNSZone:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com        # Kubernetes resource name
  namespace: dns-system
spec:
  zoneName: example.com    # Actual DNS zone name
  clusterRef: primary-dns
  # ...
```

Create an A record using either method:

**Using `zone` (matches spec.zoneName):**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zone: example.com     # ← Actual zone name
  name: www
  ipv4Address: "192.0.2.1"
```

**Using `zoneRef` (matches metadata.name):**
```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zoneRef: example-com  # ← Resource name
  name: www
  ipv4Address: "192.0.2.1"
```

Both create the same DNS record: `www.example.com → 192.0.2.1`

## Creating Records

After choosing your zone reference method, specify the record details:

```yaml
spec:
  zoneRef: example-com  # Recommended for production
  name: www             # Creates www.example.com
  ipv4Address: "192.0.2.1"
  ttl: 300             # Optional - overrides zone default
```

## Next Steps

- [A Records](./a-records.md) - IPv4 addresses
- [AAAA Records](./aaaa-records.md) - IPv6 addresses  
- [CNAME Records](./cname-records.md) - Aliases
- [MX Records](./mx-records.md) - Mail servers
- [TXT Records](./txt-records.md) - Text data
- [NS Records](./ns-records.md) - Delegation
- [SRV Records](./srv-records.md) - Services
- [CAA Records](./caa-records.md) - Certificate authority
