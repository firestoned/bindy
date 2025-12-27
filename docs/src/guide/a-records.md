# A Records (IPv4)

A records map domain names to IPv4 addresses.

## Creating an A Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
  labels:
    zone: example.com  # Used by DNSZone selector
spec:
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

This creates `www.example.com -> 192.0.2.1`.

## How Records Are Associated with Zones

Records are discovered by DNSZones using label selectors. The DNSZone must have a `recordsFrom` selector that matches the record's labels:

```yaml
# DNSZone with selector
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com
  clusterRef: production-dns
  recordsFrom:
    - selector:
        matchLabels:
          zone: example.com  # Selects all records with this label
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2024010101
---
# Record that will be selected
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
  labels:
    zone: example.com  # ✅ Matches selector above
spec:
  name: www
  ipv4Address: "192.0.2.1"
```

See [Label Selector Guide](./label-selectors.md) for advanced patterns.

## Root Record

For the zone apex (example.com):

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: root-example
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: "@"
  ipv4Address: "192.0.2.1"
```

## Multiple A Records

Create multiple records for the same name for load balancing:

```bash
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-1
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.1"
---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-2
  namespace: dns-system
  labels:
    zone: example.com
spec:
  name: www
  ipv4Address: "192.0.2.2"
